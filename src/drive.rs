use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

#[cfg(not(target_os = "android"))]
use std::env;

use chrono::DateTime;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;
use uuid::Uuid;

use crate::{
    cloud::CloudResult,
    oauth::AuthResult,
    replica::{Change, Download, Record, Remote},
};

const FILES: &str = "https://www.googleapis.com/drive/v3/files";
#[cfg(not(target_os = "android"))]
const TOKEN: &str = "https://oauth2.googleapis.com/token";
const PREFIX: &str = "opon-sync-v1-";

pub struct Drive {
    #[cfg(not(target_os = "android"))]
    client_id: String,
    #[cfg(not(target_os = "android"))]
    client_secret: String,
    account: String,
    token: Token,
    expires: Instant,
    http: ureq::Agent,
    batches: BTreeMap<String, Vec<Operation>>,
}

#[derive(Deserialize)]
struct Token {
    access_token: String,
    #[cfg(not(target_os = "android"))]
    refresh_token: Option<String>,
    expires_in: u64,
}

#[derive(Deserialize)]
struct About {
    user: Account,
}

#[derive(Deserialize)]
struct Account {
    #[serde(rename = "permissionId")]
    permission_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FilePage {
    next_page_token: Option<String>,
    files: Vec<FileId>,
}

#[derive(Deserialize)]
struct FileId {
    id: String,
}

#[derive(Serialize, Deserialize)]
struct Operation {
    base: i64,
    revision: i64,
    record: Record,
}

#[cfg(not(target_os = "android"))]
pub fn defaults() -> (String, String) {
    let id = env::var("OPON_GOOGLE_CLIENT_ID")
        .ok()
        .or_else(|| option_env!("OPON_GOOGLE_CLIENT_ID").map(str::to_owned))
        .unwrap_or_default();
    let secret = env::var("OPON_GOOGLE_CLIENT_SECRET")
        .ok()
        .or_else(|| option_env!("OPON_GOOGLE_CLIENT_SECRET").map(str::to_owned))
        .unwrap_or_default();
    (id, secret)
}

impl Drive {
    #[cfg(not(target_os = "android"))]
    pub fn sign_in(client_id: String, client_secret: String) -> AuthResult<Self> {
        if !client_id.ends_with(".apps.googleusercontent.com") || client_secret.is_empty() {
            return Err("Enter a Google Desktop OAuth client ID and client secret.");
        }
        let http = agent();
        let login = crate::oauth::BrowserLogin::bind(0, "/".into())?;
        let state = Uuid::new_v4().to_string();
        let mut authorize = Url::parse("https://accounts.google.com/o/oauth2/v2/auth").unwrap();
        authorize.query_pairs_mut().extend_pairs([
            ("client_id", client_id.as_str()),
            ("redirect_uri", login.redirect.as_str()),
            ("response_type", "code"),
            ("scope", "https://www.googleapis.com/auth/drive.appdata"),
            ("code_challenge", login.challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("state", state.as_str()),
            ("access_type", "offline"),
            ("prompt", "consent select_account"),
        ]);
        let code = login.receive(&authorize, Some(&state))?;
        let mut response = http
            .post(TOKEN)
            .send_form([
                ("client_id", client_id.as_str()),
                ("client_secret", client_secret.as_str()),
                ("code", code.as_str()),
                ("code_verifier", login.verifier.as_str()),
                ("redirect_uri", login.redirect.as_str()),
                ("grant_type", "authorization_code"),
            ])
            .map_err(|_| {
                "Google rejected the OAuth exchange. Check the Desktop OAuth client setup."
            })?;
        let token: Token = response
            .body_mut()
            .read_json()
            .map_err(|_| "Google did not return a session.")?;
        Self::open(http, token, (client_id, client_secret))
    }

    #[cfg(target_os = "android")]
    pub fn sign_in_mobile() -> AuthResult<Self> {
        let token = Token {
            access_token: crate::android_drive::authorize(true)?,
            expires_in: 3600,
        };
        Self::open(agent(), token)
    }

    fn open(
        http: ureq::Agent,
        token: Token,
        #[cfg(not(target_os = "android"))] (client_id, client_secret): (String, String),
    ) -> AuthResult<Self> {
        let account = account_id(&http, &token);
        let expires = refresh_at(&token);
        let drive = Self {
            #[cfg(not(target_os = "android"))]
            client_id,
            #[cfg(not(target_os = "android"))]
            client_secret,
            account: account.map_err(|_| {
                "Could not access OPON’s Drive folder. Enable the Drive API and grant access."
            })?,
            token,
            expires,
            http,
            batches: BTreeMap::new(),
        };
        drive.list_page(None).map_err(|_| {
            "Could not access OPON’s Drive folder. Enable the Drive API and grant access."
        })?;
        Ok(drive)
    }

    pub fn owner(&self) -> String {
        format!("drive:{}", self.account)
    }

    pub fn sync(&mut self, sent: Vec<Change>) -> CloudResult<Download> {
        self.refresh()?;
        self.pull()?;
        let current = heads(&self.batches);
        let mut uploads = Vec::new();
        let mut accepted = Vec::new();
        for change in &sent {
            if let Some(head) = current.get(&change.record.key) {
                if head.revision != change.revision {
                    accepted.push(head.clone());
                    continue;
                }
            } else if change.revision != 0 {
                return Err("Drive is missing a previously synced version".into());
            }
            let op = operation(change)?;
            accepted.push(Remote {
                record: op.record.clone(),
                revision: op.revision,
            });
            uploads.push(op);
        }
        if !uploads.is_empty() {
            let id = self.upload(&uploads)?;
            self.batches.insert(id, uploads);
        }
        Ok(Download {
            sent,
            accepted,
            current: heads(&self.batches).into_values().collect(),
        })
    }

    fn refresh(&mut self) -> CloudResult<()> {
        if Instant::now() < self.expires {
            return Ok(());
        }
        let token = self.fresh_token()?;
        if account_id(&self.http, &token)? != self.account {
            return Err("Google Drive switched accounts".into());
        }
        self.expires = refresh_at(&token);
        self.token = token;
        Ok(())
    }

    #[cfg(target_os = "android")]
    fn fresh_token(&self) -> CloudResult<Token> {
        Ok(Token {
            access_token: crate::android_drive::authorize(false)?,
            expires_in: 3600,
        })
    }

    #[cfg(not(target_os = "android"))]
    fn fresh_token(&self) -> CloudResult<Token> {
        let refresh = self
            .token
            .refresh_token
            .as_deref()
            .ok_or("Sign in to Google Drive again")?;
        let mut response = self.http.post(TOKEN).send_form([
            ("client_id", self.client_id.as_str()),
            ("client_secret", self.client_secret.as_str()),
            ("refresh_token", refresh),
            ("grant_type", "refresh_token"),
        ])?;
        let mut token: Token = response.body_mut().read_json()?;
        if token.refresh_token.is_none() {
            token.refresh_token = self.token.refresh_token.clone();
        }
        Ok(token)
    }

    fn pull(&mut self) -> CloudResult<()> {
        let bearer = format!("Bearer {}", self.token.access_token);
        let mut page_token: Option<String> = None;
        loop {
            let files = self.list_page(page_token.as_deref())?;
            for file in files.files {
                if !self.batches.contains_key(&file.id) {
                    let mut url = Url::parse(FILES)?;
                    url.path_segments_mut()
                        .expect("files API URL")
                        .push(&file.id);
                    url.query_pairs_mut().append_pair("alt", "media");
                    let operations = self
                        .http
                        .get(url.as_str())
                        .header("Authorization", &bearer)
                        .call()?
                        .body_mut()
                        .read_json()?;
                    self.batches.insert(file.id.clone(), operations);
                }
            }
            page_token = files.next_page_token;
            if page_token.is_none() {
                break;
            }
        }
        // Successful uploaded files stay in memory until this session ends, even if a list is briefly stale.
        Ok(())
    }

    fn list_page(&self, page_token: Option<&str>) -> CloudResult<FilePage> {
        let mut url = Url::parse(FILES)?;
        url.query_pairs_mut().extend_pairs([
            ("spaces", "appDataFolder"),
            ("q", "name contains 'opon-sync-v1-'"),
            ("fields", "nextPageToken,files(id)"),
            ("pageSize", "1000"),
        ]);
        if let Some(token) = page_token {
            url.query_pairs_mut().append_pair("pageToken", token);
        }
        Ok(self
            .http
            .get(url.as_str())
            .header(
                "Authorization",
                format!("Bearer {}", self.token.access_token),
            )
            .call()?
            .body_mut()
            .read_json()?)
    }

    fn upload(&self, operations: &[Operation]) -> CloudResult<String> {
        let boundary = Uuid::new_v4().simple().to_string();
        let meta = serde_json::json!({
            "name": format!("{PREFIX}{boundary}.json"),
            "parents": ["appDataFolder"],
            "mimeType": "application/json",
        });
        let body = format!(
            "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{meta}\r\n--{boundary}\r\nContent-Type: application/json\r\n\r\n{}\r\n--{boundary}--\r\n",
            serde_json::to_string(operations)?,
        );
        let uploaded: FileId = self
            .http
            .post("https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart&fields=id")
            .header(
                "Authorization",
                format!("Bearer {}", self.token.access_token),
            )
            .header(
                "Content-Type",
                format!("multipart/related; boundary={boundary}"),
            )
            .send(body.as_bytes())?
            .body_mut()
            .read_json()?;
        Ok(uploaded.id)
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(25)))
        .max_redirects(0)
        .build()
        .into()
}

fn account_id(http: &ureq::Agent, token: &Token) -> CloudResult<String> {
    let about: About = http
        .get("https://www.googleapis.com/drive/v3/about?fields=user(permissionId)")
        .header("Authorization", format!("Bearer {}", token.access_token))
        .call()?
        .body_mut()
        .read_json()?;
    Ok(about.user.permission_id)
}

fn refresh_at(token: &Token) -> Instant {
    Instant::now() + Duration::from_secs(token.expires_in.clamp(90, 86_400) - 60)
}

fn operation(change: &Change) -> CloudResult<Operation> {
    let stamp = DateTime::parse_from_rfc3339(&change.record.updated_at)
        .map_or(1, |time| time.timestamp())
        .max(1);
    let logical = stamp.max((change.revision >> 31) + 1).min(i64::MAX >> 31);
    let digest = Sha256::digest(serde_json::to_vec(&(change.revision, &change.record))?);
    let tag = u32::from_be_bytes(digest[..4].try_into()?) as i64 & 0x7fff_ffff;
    let revision = (logical << 31) | tag;
    if revision <= change.revision {
        return Err("Drive version overflow".into());
    }
    Ok(Operation {
        base: change.revision,
        revision,
        record: change.record.clone(),
    })
}

fn heads(batches: &BTreeMap<String, Vec<Operation>>) -> BTreeMap<String, Remote> {
    let mut by_note = BTreeMap::<&str, BTreeMap<i64, &Operation>>::new();
    for op in batches.values().flatten() {
        by_note
            .entry(&op.record.key)
            .or_default()
            .insert(op.revision, op);
    }
    let mut current = BTreeMap::new();
    let mut copies = Vec::new();
    for (key, versions) in by_note {
        let Some((_, head)) = versions.last_key_value() else {
            continue;
        };
        current.insert(
            key.to_owned(),
            Remote {
                record: head.record.clone(),
                revision: head.revision,
            },
        );
        if !key.starts_with("note:") {
            continue;
        }
        let parents: BTreeSet<_> = versions.values().map(|op| op.base).collect();
        for losing in versions
            .values()
            .filter(|op| !parents.contains(&op.revision))
        {
            if losing.revision == head.revision
                || losing.record.deleted
                || losing.record == head.record
            {
                continue;
            }
            let digest = Sha256::digest(format!("{key}:{}", losing.revision));
            let copy_key = format!(
                "note:{}",
                Uuid::from_bytes(digest[..16].try_into().unwrap()).simple()
            );
            let mut record = losing.record.clone();
            record.key = copy_key;
            record.title = format!(
                "{} (conflict copy)",
                if record.title.trim().is_empty() {
                    "Untitled"
                } else {
                    &record.title
                }
            );
            copies.push(Remote {
                record,
                revision: losing.revision,
            });
        }
    }
    for copy in copies {
        current.entry(copy.record.key.clone()).or_insert(copy);
    }
    current
}
