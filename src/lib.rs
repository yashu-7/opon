#[cfg(target_os = "android")]
mod android_drive;
mod cloud;
mod database;
mod drive;
mod oauth;
mod page;
mod replica;
mod repository;

use std::{
    cell::Cell,
    env,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

use cloud::{Cloud, Reply, Request, Worker};
use page::Page;
use repository::NoteRepository;
use rusqlite::Result;
use slint::{
    CloseRequestResponse, ComponentHandle, Model, SharedString, Timer, TimerMode, VecModel, Weak,
};

slint::include_modules!();

struct Workspace {
    window: Weak<MainWindow>,
    notes: NoteRepository,
    page: Page,
    dirty: Cell<bool>,
    autosave: Timer,
    cloud: Worker,
    sync_timer: Timer,
    next_sync: Cell<Instant>,
}

impl Workspace {
    fn refresh_notes(&self, window: &MainWindow) -> Result<Option<i64>> {
        let query = window.get_query();
        let folder = query
            .trim()
            .is_empty()
            .then(|| id(&window.get_active_folder()))
            .flatten();
        let notes = self.notes.get_notes(folder, query.as_str())?;
        let first = notes.first().map(|(id, _)| *id);
        let entries = notes.into_iter().map(|(id, title)| Entry {
            id: id.to_string().into(),
            label: if title.trim().is_empty() {
                "Untitled".into()
            } else {
                title.into()
            },
        });
        window.set_notes(Rc::new(VecModel::from_iter(entries)).into());
        Ok(first)
    }

    fn refresh_folders(&self, window: &MainWindow) -> Result<()> {
        let folders = self.notes.get_folders()?;
        let options = std::iter::once(SharedString::from("Unfiled"))
            .chain(folders.iter().map(|folder| folder.name.as_str().into()));
        window.set_folder_options(Rc::new(VecModel::from_iter(options)).into());
        let entries = folders.into_iter().map(|folder| Entry {
            id: folder.id.to_string().into(),
            label: folder.name.into(),
        });
        window.set_folders(Rc::new(VecModel::from_iter(entries)).into());
        Ok(())
    }

    fn open(&self, window: &MainWindow, selected: Option<i64>) -> Result<()> {
        let previous = id(&window.get_selected_id());
        if previous != selected {
            window.set_focus_block("".into());
            window.invoke_reset_page_scroll();
        }
        let note = selected
            .map(|id| self.notes.get_note_by_id(id))
            .transpose()?
            .flatten();
        if let Some(note) = note {
            window.set_selected_id(note.id.to_string().into());
            self.page.open(&note, previous != selected);
            window.set_note_title(note.title.into());
            let folders = window.get_folders();
            let position = folders
                .iter()
                .position(|entry| id(&entry.id) == note.folder_id);
            window.set_folder_index(position.map_or(0, |index| index as i32 + 1));
            window.set_location(
                position
                    .and_then(|index| folders.row_data(index))
                    .map_or_else(|| "All pages".into(), |entry| entry.label),
            );
            window.set_status("Saved on this device".into());
        } else {
            window.set_selected_id("".into());
            window.set_note_title("".into());
            self.page.clear();
            window.set_status("".into());
        }
        if previous != selected {
            window.set_confirming_delete(false);
        }
        Ok(())
    }

    fn save(&self, window: &MainWindow) -> Result<()> {
        self.autosave.stop();
        if !self.dirty.get() {
            return Ok(());
        }
        let body = self.page.encode();
        let saved = id(&window.get_selected_id())
            .ok_or(rusqlite::Error::QueryReturnedNoRows)
            .and_then(|id| body.map(|(plain, blocks)| (id, plain, blocks)))
            .and_then(|(id, plain, blocks)| {
                require_row(self.notes.update_note(
                    id,
                    &window.get_note_title(),
                    &plain,
                    Some(&blocks),
                )?)?;
                self.page.mark_saved(plain, Some(blocks));
                Ok(())
            });
        window.set_save_failed(saved.is_err());
        if let Err(error) = saved {
            window.set_status("Changes unsaved".into());
            return Err(error);
        }
        self.dirty.set(false);
        window.set_status("Saved on this device".into());
        self.refresh_notes(window)?;
        Ok(())
    }

    fn handle(self: &Rc<Self>, window: &MainWindow, action: Action, value: &str) -> Result<()> {
        window.set_error_message("".into());
        if action == Action::Edit {
            self.dirty.set(true);
            if window.get_mobile() {
                return self.save(window);
            }
            window.set_save_failed(false);
            window.set_status("Saving…".into());
            let workspace = Rc::downgrade(self);
            self.autosave.start(
                TimerMode::SingleShot,
                Duration::from_millis(350),
                move || {
                    if let Some(workspace) = workspace.upgrade() {
                        if let Some(window) = workspace.window.upgrade() {
                            report(&window, workspace.save(&window));
                        }
                    }
                },
            );
            return Ok(());
        }
        if action == Action::Search {
            self.refresh_notes(window)?;
            return Ok(());
        }
        if action == Action::Theme {
            self.notes.set_preference("theme", value)?;
            window.set_dark_mode(value != "light");
            return Ok(());
        }
        if matches!(
            action,
            Action::Configure
                | Action::Connect
                | Action::ConnectEmail
                | Action::SignupEmail
                | Action::ConnectDrive
                | Action::Disconnect
                | Action::SyncNow
        ) {
            return self.sync_action(window, action);
        }

        self.save(window)?;
        let selected = id(&window.get_selected_id());
        match action {
            Action::OpenNote => {
                self.open(window, id(value))?;
                window.set_show_editor(true);
            }
            Action::Back => window.set_show_editor(false),
            Action::Filter => {
                window.set_active_folder(value.into());
                window.set_query("".into());
                let first = self.refresh_notes(window)?;
                self.open(window, first)?;
            }
            Action::NewNote => {
                let folder = id(&window.get_active_folder());
                let new = self.notes.add_note("", "", folder)?;
                window.set_query("".into());
                self.refresh_notes(window)?;
                self.open(window, Some(new))?;
                window.set_show_editor(true);
                window.invoke_focus_title();
            }
            Action::DeleteNote => {
                if let Some(id) = selected {
                    require_row(self.notes.delete_note(id)?)?;
                    let first = self.refresh_notes(window)?;
                    self.open(window, first)?;
                    if first.is_none() {
                        window.set_show_editor(false);
                    }
                }
            }
            Action::NewFolder => {
                let name = value.trim();
                if name.is_empty() {
                    window.set_error_message("Enter a folder name.".into());
                    return Ok(());
                }
                if let Err(error) = self.notes.add_folder(name) {
                    if error.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) {
                        window.set_error_message("A folder with that name already exists.".into());
                        return Ok(());
                    }
                    return Err(error);
                }
                self.refresh_folders(window)?;
                window.set_adding_folder(false);
                window.set_folder_name("".into());
                self.open(window, selected)?;
            }
            Action::MoveNote => {
                if let Some(note) = selected {
                    let folder = window
                        .get_folder_index()
                        .checked_sub(1)
                        .and_then(|index| usize::try_from(index).ok())
                        .and_then(|index| window.get_folders().row_data(index))
                        .and_then(|entry| id(&entry.id));
                    require_row(self.notes.move_note(note, folder)?)?;
                    self.open(window, Some(note))?;
                    self.refresh_notes(window)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn sync_action(&self, window: &MainWindow, action: Action) -> Result<()> {
        let cloud = &self.cloud;
        let (request, status) = match action {
            Action::Configure if !window.get_sync_busy() && !window.get_sync_connected() => {
                let project = window.get_project_url();
                let key = window.get_publishable_key();
                let (project, key) = (project.trim(), key.trim());
                if let Some(config) = Cloud::new(project, key) {
                    self.notes.save_sync_setup(project, key)?;
                    if cloud.send.send(Request::Configure(config)).is_ok() {
                        window.set_sync_configured(true);
                        window.set_editing_sync(false);
                        window.set_sync_status("Project saved. Choose a sign-in method.".into());
                    }
                } else {
                    window.set_sync_status(
                        "Enter an HTTPS project URL and a publishable key.".into(),
                    );
                }
                return Ok(());
            }
            Action::Connect if !window.get_sync_busy() && !window.get_sync_connected() => (
                Request::SupabaseGoogle,
                "Checking the Supabase Google provider…",
            ),
            Action::ConnectEmail | Action::SignupEmail
                if !window.get_sync_busy() && !window.get_sync_connected() =>
            {
                let email = window.get_email().trim().to_owned();
                let password = window.get_password().to_string();
                if email.is_empty() || password.is_empty() {
                    window.set_sync_status("Enter your email and password.".into());
                    return Ok(());
                }
                window.set_password("".into());
                (
                    Request::SupabaseEmail {
                        email,
                        password,
                        signup: action == Action::SignupEmail,
                    },
                    "Connecting to Supabase…",
                )
            }
            #[cfg(not(target_os = "android"))]
            Action::ConnectDrive if !window.get_sync_busy() && !window.get_sync_connected() => {
                let client_id = window.get_drive_client().trim().to_owned();
                let client_secret = window.get_drive_secret().trim().to_owned();
                if !client_id.ends_with(".apps.googleusercontent.com") || client_secret.is_empty() {
                    window.set_sync_status(
                        "Enter your Google Desktop OAuth client ID and client secret.".into(),
                    );
                    return Ok(());
                }
                self.notes.set_preference("drive_client_id", &client_id)?;
                (
                    Request::Drive {
                        client_id,
                        client_secret,
                    },
                    "Continue in your browser to authorize Google Drive.",
                )
            }
            #[cfg(target_os = "android")]
            Action::ConnectDrive
                if window.get_drive_available()
                    && !window.get_sync_busy()
                    && !window.get_sync_connected() =>
            {
                (
                    Request::DriveMobile,
                    "Continue with Google Play Services to authorize Drive.",
                )
            }
            Action::Disconnect => {
                window.set_sync_connected(false);
                window.set_sync_provider("".into());
                window.set_sync_busy(false);
                window.set_sync_status("".into());
                let _ = cloud.send.send(Request::SignOut);
                return Ok(());
            }
            Action::SyncNow if window.get_sync_connected() && !window.get_sync_busy() => {
                self.save(window)?;
                (Request::Sync(self.notes.sync_changes()?), "Syncing…")
            }
            _ => return Ok(()),
        };
        if cloud.send.send(request).is_ok() {
            window.set_sync_busy(true);
            window.set_sync_status(status.into());
        } else {
            window.set_sync_status("Restart OPON to reconnect.".into());
        }
        self.next_sync.set(Instant::now() + Duration::from_secs(15));
        Ok(())
    }

    fn poll_sync(&self, window: &MainWindow) -> Result<()> {
        let cloud = &self.cloud;
        if let Ok(reply) = cloud.receive.try_recv() {
            match reply {
                Reply::SignedIn(owner, provider) => {
                    window.set_sync_busy(false);
                    if !self.notes.claim_workspace(&owner)? {
                        let _ = cloud.send.send(Request::SignOut);
                        window.set_sync_status(
                            "This workspace is linked to another account or provider. Sign in with the original account, or use a separate workspace.".into(),
                        );
                        return Ok(());
                    }
                    window.set_sync_provider(provider.into());
                    window.set_sync_connected(true);
                    self.next_sync.set(Instant::now());
                }
                Reply::AuthMessage(message) => {
                    window.set_sync_busy(false);
                    window.set_sync_status(message.into());
                }
                Reply::Synced(download) if window.get_sync_connected() => {
                    window.set_sync_busy(false);
                    self.save(window)?;
                    let selected = id(&window.get_selected_id());
                    let conflicts = self.notes.apply_sync(download)?;
                    self.refresh_folders(window)?;
                    let first = self.refresh_notes(window)?;
                    let next = selected
                        .map(|id| self.notes.get_note_by_id(id))
                        .transpose()?
                        .flatten()
                        .map(|note| note.id)
                        .or(first);
                    self.open(window, next)?;
                    if next.is_none() {
                        window.set_show_editor(false);
                    }
                    window.set_sync_status(if conflicts == 0 {
                        "Synced".into()
                    } else {
                        format!("{conflicts} conflict copies saved").into()
                    });
                }
                Reply::SyncFailed if window.get_sync_connected() => {
                    window.set_sync_busy(false);
                    window.set_sync_status("Cloud unavailable. Local edits are saved.".into());
                }
                _ => {}
            }
        }
        if window.get_sync_connected()
            && !self.dirty.get()
            && Instant::now() >= self.next_sync.get()
        {
            self.sync_action(window, Action::SyncNow)?;
        }
        Ok(())
    }
}

fn id(value: &str) -> Option<i64> {
    value.parse().ok()
}

fn require_row(rows: usize) -> Result<()> {
    if rows == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    Ok(())
}

fn report(window: &MainWindow, result: Result<()>) -> bool {
    if let Err(error) = result {
        eprintln!("OPON: {error}");
        window.set_error_message(if window.get_save_failed() {
            "Your edit is still open. Retry save before switching notes or closing OPON.".into()
        } else {
            "OPON could not finish that action. Please try again.".into()
        });
        return false;
    }
    true
}

fn data_directory() -> PathBuf {
    if let Some(path) = env::var_os("OPON_DATA_DIR") {
        return path.into();
    }
    if Path::new("data/opon.db").is_file() {
        return "data".into();
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(directory) = env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|directory| directory.is_absolute())
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        {
            return directory.join("opon");
        }
    }
    env::var_os("LOCALAPPDATA")
        .map(|base| PathBuf::from(base).join("OPON"))
        .unwrap_or_else(|| PathBuf::from("data"))
}

pub fn run_desktop() -> std::result::Result<(), Box<dyn std::error::Error>> {
    run(data_directory(), false)
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    let directory = app
        .internal_data_path()
        .expect("Android application data directory");
    slint::android::init(app).expect("initialize Android interface");
    run(directory, true).expect("run OPON");
}

fn run(directory: PathBuf, mobile: bool) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let window = MainWindow::new()?;
    #[cfg(target_os = "linux")]
    slint::set_xdg_app_id("io.github.yashu7.opon")?;
    #[cfg(windows)]
    window.set_code_font("Consolas".into());
    window.set_license_text(include_str!("../LICENSE").into());
    window.set_mobile(mobile);
    let notes = NoteRepository::new(database::initialize(&directory)?);
    window.set_dark_mode(notes.preference("theme")?.as_deref() != Some("light"));
    #[cfg(target_os = "android")]
    window.set_drive_available(android_drive::available());
    #[cfg(not(target_os = "android"))]
    {
        let (client_id, client_secret) = drive::defaults();
        window.set_drive_client(
            notes
                .preference("drive_client_id")?
                .unwrap_or(client_id)
                .into(),
        );
        window.set_drive_secret(client_secret.into());
    }
    let settings = notes.sync_setup()?.or_else(Cloud::environment);
    let config = settings
        .as_ref()
        .and_then(|(project, key)| Cloud::new(project, key));
    if let Some((project, key)) = settings {
        window.set_project_url(project.into());
        window.set_publishable_key(key.into());
    }
    window.set_sync_configured(config.is_some());
    let workspace = Rc::new(Workspace {
        window: window.as_weak(),
        notes,
        page: Page::default(),
        dirty: Cell::new(false),
        autosave: Timer::default(),
        cloud: Worker::start(config),
        sync_timer: Timer::default(),
        next_sync: Cell::new(Instant::now()),
    });
    window.set_blocks(workspace.page.rows.clone().into());
    window.on_block_choices(page::choices);
    let editing = Rc::clone(&workspace);
    window.on_block_action(move |action, index, value, anchor, cursor| {
        if let Some(window) = editing.window.upgrade() {
            if editing
                .page
                .apply(&window, action, index, &value, anchor, cursor)
            {
                report(&window, editing.handle(&window, Action::Edit, ""));
            }
        }
    });
    let editing = Rc::clone(&workspace);
    window.on_focus_first_block(move || {
        if let Some(window) = editing.window.upgrade() {
            editing.page.focus_first(&window);
        }
    });
    {
        let syncing = Rc::downgrade(&workspace);
        workspace
            .sync_timer
            .start(TimerMode::Repeated, Duration::from_millis(300), move || {
                if let Some(workspace) = syncing.upgrade() {
                    if let Some(window) = workspace.window.upgrade() {
                        report(&window, workspace.poll_sync(&window));
                    }
                }
            });
    }
    workspace.refresh_folders(&window)?;
    let first = workspace.refresh_notes(&window)?;
    workspace.open(&window, first)?;

    let actions = Rc::clone(&workspace);
    window.on_action(move |action, value| {
        if let Some(window) = actions.window.upgrade() {
            report(&window, actions.handle(&window, action, &value));
        }
    });
    window.window().on_close_requested(move || {
        if let Some(window) = workspace.window.upgrade() {
            if !report(&window, workspace.save(&window)) && workspace.dirty.get() {
                return CloseRequestResponse::KeepWindowShown;
            }
        }
        CloseRequestResponse::HideWindow
    });

    window.run()?;
    Ok(())
}
