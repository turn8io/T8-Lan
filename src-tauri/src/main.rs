// Hide console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Admin elevation is handled by the embedded Windows manifest
    // (requireAdministrator in build.rs), so by the time we get here the
    // process is always elevated.
    t8_lan_lib::run();
}
