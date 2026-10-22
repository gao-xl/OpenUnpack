#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args
        .iter()
        .any(|a| a == "--headless" || a == "-d" || a == "--daemon")
    {
        openunpack_desktop_lib::run_headless();
    } else {
        openunpack_desktop_lib::run();
    }
}
