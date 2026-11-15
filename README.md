# OPON

OPON is an offline-first workspace built with Rust, Slint, and SQLite. Pages save locally as you type. Windows and Linux have a workspace sidebar. Android uses a page list and a full-screen editor. Choose **Settings → Appearance → Light** or **Dark**. OPON remembers the choice on this device.

## Editing pages

- Create a page, give it a title, and press **Enter** to start writing.
- Type **/** in an empty block and search for text, headings, bulleted or numbered lists, to-dos, quotes, code, or a divider. Press **Enter** to choose a result.
- Press **Enter** in text to split the block, **Backspace** at the start to join it with the previous block, or **Shift+Enter** for a line break. In code blocks, use **Ctrl+Enter** to start a new block.
- Use **+** to add a block below and the block menu to change its type, move it, duplicate it, or delete it. Tap a to-do box to mark it complete.
- Use **Ctrl+K** to search page titles and text and **Ctrl+S** to save immediately.

OPON opens existing plain-text notes as text blocks. Update all syncing devices before editing block pages so older clients do not strip block formatting.

## Windows

```powershell
cargo run --locked
```

Use **Ctrl+K** to focus search and **Ctrl+S** to save immediately. Edits also save automatically. If you run OPON from the original checkout, it opens and migrates an existing `data/opon.db`. New Windows workspaces default to `%LOCALAPPDATA%\OPON`. Set `OPON_DATA_DIR` to point to a specific data directory.

The shareable Windows build is a single x64 executable. Build it from PowerShell with the Windows C runtime linked statically.

```powershell
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = '-C target-feature=+crt-static'
cargo build --locked --release --target x86_64-pc-windows-msvc --bin opon
```

## Linux

Extract the Linux archive and run `./opon`, or run `./install.sh` to add an OPON launcher to your application menu. The x64 build requires a glibc-based Linux desktop with X11 or Wayland. Notes default to `${XDG_DATA_HOME:-$HOME/.local/share}/opon`.

Build the portable x64 executable with [Zig and cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild):

```sh
rustup target add x86_64-unknown-linux-gnu
cargo zigbuild --locked --release --target x86_64-unknown-linux-gnu.2.28 --bin opon
```

To recreate the archive, package the executable together with `assets/opon.png` and `packaging/linux/install.sh`. Keep the executable bit on both `opon` and `install.sh`.

## Android

Install Android SDK Platform 36, Android Build Tools, the NDK, and a JDK, then set `ANDROID_HOME`, `ANDROID_NDK_HOME`, and `JAVA_HOME`. Build a debug APK for a 64-bit ARM phone.

```powershell
rustup target add aarch64-linux-android
cargo install cargo-apk --locked
cargo apk build --target aarch64-linux-android --lib
```

Open `target/debug/apk/opon_app.apk` on the phone, or run `adb install -r target/debug/apk/opon_app.apk` with a connected device. OPON stores the phone database in the app's private data directory.

For a shareable optimized APK, set the `CARGO_APK_RELEASE_KEYSTORE` and `CARGO_APK_RELEASE_KEYSTORE_PASSWORD` environment variables locally, then add `--release` to the build command. Use the same signing key for future updates.

The Android release with direct Google Drive uses Gradle 8.13 and Google's native authorization library. Build the Rust release library with the release command above, then run `gradle -p android assembleRelease`. Sign the resulting APK in `android/build/outputs/apk/release` with the same key you use for OPON updates. The basic cargo-apk package supports Supabase.

## Optional sync

Open **Settings** and choose a provider. OPON syncs pages and folders while the app is open. **Sync now** starts a sync immediately. A local workspace stays linked to its first sync account and provider. Use the same account on each device. Sign in again after fully restarting OPON.

### Supabase on Windows, Linux, and Android

1. Create a project and run [the sync schema](supabase/sync.sql) once in the SQL editor.
2. Choose **Settings → Supabase**, enter the project URL and **publishable** key, and save.
3. With the Supabase Email provider enabled, create an account or sign in with email and password. If the project requires email confirmation, confirm the account before signing in. [Supabase email setup](https://supabase.com/docs/guides/auth/passwords)
4. For **Google via Supabase**, enable Google under **Authentication → Sign In / Providers** and enter your Google Cloud **Web application** OAuth client ID and secret there. Add the callback URI Supabase shows to the Google client's authorized redirect URIs. If Google OAuth is in testing, add your account as a test user. [Supabase Google provider setup](https://supabase.com/docs/guides/auth/social-login/auth-google)
5. In Supabase **Authentication → URL Configuration**, add `http://127.0.0.1:17824/opon/**` to Redirect URLs for the browser callback. [Supabase redirect URLs](https://supabase.com/docs/guides/auth/redirect-urls)

If OPON reports that Google is disabled, check the provider in the Supabase project that matches the saved URL, or sign in with email.

To check sync, create a note on Windows and find it on the phone. Edit it offline on the phone, reconnect, and check the desktop. Edit a different note on both devices while offline and reconnect both. OPON keeps a conflict copy when both versions contain edits. Delete a synced note on one device and check that it disappears from the other.

The Supabase URL and publishable key can also be set with `OPON_SUPABASE_URL` and `OPON_SUPABASE_KEY`. Settings saved inside OPON take precedence.

### Direct Google Drive

1. In a Google Cloud project, enable the Drive API, configure the OAuth consent screen, and create a **Desktop app** OAuth client. Add your account as a test user if the OAuth app is in testing.
2. On Windows or Linux, choose **Settings → Google Drive**, enter the Desktop client ID and client secret, and connect. Desktop builds can also take those values from `OPON_GOOGLE_CLIENT_ID` and `OPON_GOOGLE_CLIENT_SECRET`.
3. For the Android release, create an **Android** OAuth client in the same Google Cloud project. Use package name `io.github.yashu7.opon` and the signing certificate SHA-1 shown by `apksigner verify --print-certs OPON-Android.apk`. On the phone, choose **Settings → Google Drive → Connect Google Drive** and continue through Google Play Services.
4. Use the same Google account and Google Cloud project across devices. OPON stores versioned changes in the private Drive application folder and keeps the working copy in SQLite.

[Drive app data](https://developers.google.com/workspace/drive/api/guides/appdata) · [Desktop OAuth](https://developers.google.com/identity/protocols/oauth2/native-app) · [Android authorization](https://developer.android.com/identity/authorization)

## Roadmap

| Stage | Scope |
| --- | --- |
| Available | Light and dark pages, folders, search, local autosave, block types, slash commands, Supabase sync, and direct Google Drive sync. |
| Editor next | Inline formatting and links, drag-to-reorder, multi-block selection and undo, nested lists, and Markdown import/export. |
| Workspace | Page hierarchy, backlinks, tags and favorites, templates, and task views. |
| Sync options | Test recovery on two devices and support moving a workspace between providers. |
| Later | Attachments, reminders, Android widgets, and collaboration. |

## License

OPON is licensed under the [MIT License](LICENSE). Dependencies retain their own licenses. OPON includes Slint attribution under **About OPON**. [Slint licensing](https://github.com/slint-ui/slint/blob/master/LICENSE.md)
