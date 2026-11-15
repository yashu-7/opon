#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    if let Err(error) = opon_app::run_desktop() {
        eprintln!("OPON could not start: {error}");
        std::process::exit(1);
    }
}
