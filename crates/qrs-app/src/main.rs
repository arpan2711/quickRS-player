//! quickRS desktop app.
//!
//!   quickrs                      the app
//!   quickrs --silent             the app on no sound devices (nothing is heard; a simulated MG-300)
//!   quickrs --gallery            the component gallery (a debug screen)
//!   quickrs --screenshots DIR    render the screens in every theme to PNGs in DIR, then quit
//!           [--size 1440x900]    (physical pixels)
//!   quickrs --after-update PID   (the updater's restart: waits for the old app to close, P20.1)

// release builds are a windowed app (no console window); messages for the command line reach the terminal it
// was started from (see `attach_console`)
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> anyhow::Result<()> {
    qrs_app::desktop_main()
}
