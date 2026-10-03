//! Eén instantie van T8-Lan per sessie, zonder extra crate: een named mutex bepaalt wie
//! de eerste is, een named event laat een tweede start het bestaande venster tonen.
//!
//! Waarom niet de Tauri single-instance plugin: die werkt prima, maar T8-Lan moet klein
//! blijven en de `windows`-crate zit er al in. Dit zijn vier Win32-aanroepen.
//!
//! Gedrag: de tweede instantie (Startmenu-klik terwijl de tray-app al draait, of de
//! Taakplanner-taak naast een handmatige start) zet het event en stopt direct, zónder
//! een tweede tray-icoon. De eerste instantie wacht op dat event en toont zijn venster.

use crate::tray;
use tauri::AppHandle;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE,
    INFINITE,
};

/// `Local\`: per aanmeldsessie, zodat een geëleveerde en een gewone start van dezelfde
/// gebruiker elkaar wél zien.
const MUTEX_NAME: &str = "Local\\T8-Lan-SingleInstance";
const SHOW_EVENT_NAME: &str = "Local\\T8-Lan-ShowWindow";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `true` als dit de eerste (en dus de echte) instantie is. Bij `false` is het bestaande
/// venster al gevraagd zichzelf te tonen en hoort de aanroeper meteen te stoppen.
///
/// De mutex-handle wordt bewust nooit gesloten: hij moet precies zo lang leven als het
/// proces.
pub fn claim_primary() -> bool {
    let name = wide(MUTEX_NAME);
    // SAFETY: geldige, nul-getermineerde UTF-16-naam; geen security-attributen nodig.
    let created = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) };
    let already = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    match created {
        Ok(_handle) if !already => true,
        // Mutex bestaat al (of kon niet gemaakt worden terwijl hij bestaat): tweede start.
        _ => {
            signal_show();
            false
        }
    }
}

/// Vraag de draaiende instantie om zijn venster te tonen. Faalt stil als er (net) geen
/// draaiende instantie is.
fn signal_show() {
    let name = wide(SHOW_EVENT_NAME);
    // SAFETY: zie `claim_primary`.
    if let Ok(ev) = unsafe { OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(name.as_ptr())) } {
        let _ = unsafe { SetEvent(ev) };
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(ev) };
    }
}

/// Start de wachtthread die bij elk show-signaal het hoofdvenster naar voren haalt.
pub fn watch_show_requests(app: AppHandle) {
    let name = wide(SHOW_EVENT_NAME);
    // Auto-reset event: na elke WaitForSingleObject weer "uit", dus één show per signaal.
    // SAFETY: zie `claim_primary`.
    let Ok(ev) = (unsafe { CreateEventW(None, false, false, PCWSTR(name.as_ptr())) }) else {
        return;
    };
    let raw = ev.0 as usize;
    std::thread::Builder::new()
        .name("t8-single-instance".into())
        .spawn(move || {
            let handle = HANDLE(raw as *mut _);
            loop {
                // SAFETY: de handle blijft geldig zolang dit proces leeft (nooit gesloten).
                let _ = unsafe { WaitForSingleObject(handle, INFINITE) };
                tray::show_main(&app, None);
            }
        })
        .ok();
}
