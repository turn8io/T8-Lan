// Hide console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Admin elevation is handled by the embedded Windows manifest
    // (requireAdministrator in build.rs), so by the time we get here the
    // process is always elevated.

    // Installer-/beheerhooks: registreer of verwijder de autostart-taak en stop meteen,
    // zonder GUI. De installer roept `--register-autostart` aan na het kopiëren.
    let mut args = std::env::args().skip(1);
    if let Some(flag) = args.next() {
        let result = match flag.as_str() {
            "--register-autostart" => Some(t8_lan_lib::autostart::apply(true)),
            "--unregister-autostart" => Some(t8_lan_lib::autostart::apply(false)),
            _ => None,
        };
        if let Some(result) = result {
            match result {
                Ok(()) => std::process::exit(0),
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            }
        }
    }

    // Nooit twee instanties: een tweede start laat de eerste zijn venster tonen en stopt.
    if !t8_lan_lib::single_instance::claim_primary() {
        return;
    }

    t8_lan_lib::run();
}
