#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Installer bootstrap (spec E-06): register Explorer verbs and exit
    // without initializing the window or the single-instance plugin.
    if std::env::args().any(|argument| argument == "--register-shell") {
        anole_desktop_lib::register_shell_and_exit();
    }
    anole_desktop_lib::run();
}
