use std::{
    env,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

use serde::Deserialize;
use url::Url;
use uuid::Uuid;

use crate::{
    oauth::{AuthResult, BrowserLogin},
    replica::{Change, Download},
};

pub type CloudResult<T> = Result<T, Box<dyn std::error::Error>>;

pub enum Request {
    Configure(Cloud),
    SupabaseGoogle,
    SupabaseEmail {
        email: String,
        password: String,
        signup: bool,
    },
    #[cfg(not(target_os = "android"))]
    Drive {
        client_id: String,
        client_secret: String,
    },
    #[cfg(target_os = "android")]
    DriveMobile,
    SignOut,
    Sync(Vec<Change>),
}

pub enum Reply {
    SignedIn(String, &'static str),
    AuthMessage(&'static str),
    Synced(Download),
    SyncFailed,
}

pub struct Worker {
    pub send: Sender<Request>,
    pub receive: Receiver<Reply>,
}

pub struct Cloud {
    project: Url,
    key: String,
    http: ureq::Agent,
}

#[derive(Deserialize)]
struct Session {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
    user: User,
}

#[derive(Deserialize)]
struct User {
    id: String,
}

#[derive(Deserialize)]
struct Settings {
    external: Providers,
}

#[derive(Deserialize)]
struct Providers {
    #[serde(default)]
    google: bool,
}

enum Active {
    Supabase(Session, Instant),
    Drive(crate::drive::Drive),
}

impl Worker {
    pub fn start(mut config: Option<Cloud>) -> Self {
        let (send, requests) = mpsc::channel();
        let (replies, receive) = mpsc::channel();
        thread::spawn(move || {
            let missing = "Save your Supabase project URL and publishable key first.";
            let mut session = None;
            while let Ok(request) = requests.recv() {
                let reply = match request {
                    Request::Configure(cloud) => {
                        config = Some(cloud);
                        session = None;
                        continue;
                    }
                    Request::SupabaseGoogle => supabase_session(
                        config.as_ref().ok_or(missing).and_then(Cloud::sign_in),
                        &mut session,
                    ),
                    Request::SupabaseEmail {
                        email,
                        password,
                        signup,
                    } => supabase_session(
                        config
                            .as_ref()
                            .ok_or(missing)
                            .and_then(|cloud| cloud.email(&email, &password, signup)),
                        &mut session,
                    ),
                    #[cfg(not(target_os = "android"))]
                    Request::Drive {
                        client_id,
                        client_secret,
                    } => drive_session(
                        crate::drive::Drive::sign_in(client_id, client_secret),
                        &mut session,
                    ),
                    #[cfg(target_os = "android")]
                    Request::DriveMobile => {
                        drive_session(crate::drive::Drive::sign_in_mobile(), &mut session)
                    }
                    Request::SignOut => {
                        session = None;
                        continue;
                    }
                    Request::Sync(changes) => {
                        let result = (|| -> CloudResult<Download> {
                            match session.as_mut().ok_or("Sign in to sync")? {
                                Active::Supabase(active, expires) => {
                                    let cloud = config.as_ref().ok_or(missing)?;
                                    if Instant::now() >= *expires {
                                        *active = cloud.refresh(active)?;
                                        *expires = refresh_at(active);
                                    }
                                    cloud.sync(active, changes)
                                }
                                Active::Drive(drive) => drive.sync(changes),
                            }
                        })();
                        match result {
                            Ok(download) => Reply::Synced(download),
                            Err(_) => Reply::SyncFailed,
                        }
                    }
                };
                if replies.send(reply).is_err() {
                    break;
                }
            }
        });
        Self { send, receive }
    }
}

fn supabase_session(result: AuthResult<Session>, active: &mut Option<Active>) -> Reply {
    match result {
        Ok(session) => {
            let owner = session.user.id.clone();
            let expires = refresh_at(&session);
            *active = Some(Active::Supabase(session, expires));
            Reply::SignedIn(owner, "Supabase")
        }
        Err(message) => Reply::AuthMessage(message),
    }
}

fn drive_session(result: AuthResult<crate::drive::Drive>, active: &mut Option<Active>) -> Reply {
    match result {
        Ok(drive) => {
            let owner = drive.owner();
            *active = Some(Active::Drive(drive));
            Reply::SignedIn(owner, "Google Drive")
        }
        Err(message) => Reply::AuthMessage(message),
    }
}

fn refresh_at(session: &Session) -> Instant {
    Instant::now() + Duration::from_secs(session.expires_in.clamp(90, 86_400) - 60)
}

impl Cloud {
    pub fn environment() -> Option<(String, String)> {
        let project = env::var("OPON_SUPABASE_URL")
            .ok()
            .or_else(|| option_env!("OPON_SUPABASE_URL").map(str::to_owned))?;
        let key = env::var("OPON_SUPABASE_KEY")
            .ok()
            .or_else(|| option_env!("OPON_SUPABASE_KEY").map(str::to_owned))?;
        Some((project, key))
    }

    pub fn new(project: &str, key: &str) -> Option<Self> {
        let project = Url::parse(project).ok()?;
        if project.scheme() != "https"
            || project.host_str().is_none()
            || !project.username().is_empty()
            || project.password().is_some()
            || project.path() != "/"
            || project.query().is_some()
            || project.fragment().is_some()
            || !key.starts_with("sb_publishable_")
        {
            return None;
        }
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(20)))
            .max_redirects(0)
            .build();
        Some(Self {
            project,
            key: key.into(),
            http: config.into(),
        })
    }

    fn check_google_provider(&self) -> AuthResult<()> {
        let settings = self
            .project
            .join("/auth/v1/settings")
            .expect("valid settings path");
        let mut response = self.http.get(settings.as_str()).header("apikey", &self.key).call()
            .map_err(|error| match error {
                ureq::Error::StatusCode(401 | 403) => "Supabase rejected the key. Check your project URL and publishable key.",
                _ => "Could not check Supabase sign-in settings. Check your project URL and connection.",
            })?;
        let settings: Settings = response
            .body_mut()
            .read_json()
            .map_err(|_| "Could not read Supabase sign-in settings.")?;
        if !settings.external.google {
            return Err("Google sign-in is disabled in this Supabase project. Enable Google under Authentication → Sign In / Providers, or sign in with email.");
        }
        Ok(())
    }

    fn sign_in(&self) -> AuthResult<Session> {
        self.check_google_provider()?;
        let login = BrowserLogin::bind(17824, format!("/opon/{}", Uuid::new_v4().simple()))?;
        let mut authorize = self
            .project
            .join("/auth/v1/authorize")
            .expect("valid authorize path");
        authorize.query_pairs_mut().extend_pairs([
            ("provider", "google"),
            ("redirect_to", login.redirect.as_str()),
            ("code_challenge", login.challenge.as_str()),
            ("code_challenge_method", "s256"),
            ("prompt", "select_account"),
        ]);
        let code = login.receive(&authorize, None)?;
        self.authenticate(
            "/auth/v1/token?grant_type=pkce",
            serde_json::json!({
                "auth_code": code, "code_verifier": login.verifier,
            }),
            false,
        )
    }

    fn email(&self, email: &str, password: &str, signup: bool) -> AuthResult<Session> {
        let path = if signup {
            "/auth/v1/signup"
        } else {
            "/auth/v1/token?grant_type=password"
        };
        self.authenticate(
            path,
            serde_json::json!({ "email": email, "password": password }),
            signup,
        )
    }

    fn authenticate(
        &self,
        path: &str,
        body: serde_json::Value,
        signup: bool,
    ) -> AuthResult<Session> {
        let url = self.project.join(path).expect("valid authentication path");
        let mut response = self
            .http
            .post(url.as_str())
            .header("apikey", &self.key)
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(body)
            .map_err(|_| "Could not reach Supabase. Check your connection.")?;
        if !response.status().is_success() {
            let error: serde_json::Value = response.body_mut().read_json().unwrap_or_default();
            return Err(match error["error_code"].as_str() {
                Some("invalid_credentials") => "Email or password is incorrect.",
                Some("email_not_confirmed") => {
                    "Confirm your email, then return to OPON and sign in."
                }
                Some("weak_password") => "Choose a stronger password for this project.",
                Some("signup_disabled") => "Sign-up is disabled in this Supabase project.",
                Some("email_provider_disabled") => {
                    "Email sign-in is disabled in this Supabase project."
                }
                Some("over_email_send_rate_limit" | "over_request_rate_limit") => {
                    "Too many requests. Wait a moment and retry."
                }
                _ => "Supabase rejected sign-in. Check your project and authentication settings.",
            });
        }
        response.body_mut().read_json().map_err(|_| {
            if signup {
                "Check your email to confirm your account, then return to OPON and sign in."
            } else {
                "Supabase did not return a sign-in session."
            }
        })
    }

    fn refresh(&self, session: &Session) -> AuthResult<Session> {
        self.authenticate(
            "/auth/v1/token?grant_type=refresh_token",
            serde_json::json!({ "refresh_token": session.refresh_token }),
            false,
        )
    }

    fn sync(&self, session: &Session, sent: Vec<Change>) -> CloudResult<Download> {
        let accepted = if sent.is_empty() {
            Vec::new()
        } else {
            self.rpc("opon_push", session)
                .send_json(Push { changes: &sent })?
                .body_mut()
                .read_json()?
        };
        let current = self
            .rpc("opon_pull", session)
            .send_json(std::collections::BTreeMap::<String, String>::new())?
            .body_mut()
            .read_json()?;
        Ok(Download {
            sent,
            accepted,
            current,
        })
    }

    fn rpc(
        &self,
        function: &str,
        session: &Session,
    ) -> ureq::RequestBuilder<ureq::typestate::WithBody> {
        self.http
            .post(format!(
                "{}/rest/v1/rpc/{function}",
                self.project.origin().ascii_serialization()
            ))
            .header("apikey", &self.key)
            .header("Authorization", format!("Bearer {}", session.access_token))
    }
}

#[derive(serde::Serialize)]
struct Push<'a> {
    changes: &'a [Change],
}
