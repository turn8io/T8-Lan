//! Gedeelde wrapper rond het `netsh`-commando voor IPv4-configuratie (adres + DNS).
//! Draait zonder consolevenster en normaliseert de foutmelding.

use std::os::windows::process::CommandExt;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Voer `netsh` uit met de gegeven argumenten. `Ok(())` bij exitcode 0, anders een
/// foutmelding met de eerste niet-lege regel uit stderr/stdout.
pub fn run(args: &[&str]) -> Result<(), String> {
    let output = Command::new("netsh")
        .creation_flags(CREATE_NO_WINDOW)
        .args(args)
        .output()
        .map_err(|e| format!("netsh starten mislukt: {e}"))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let msg = if !stderr.is_empty() { stderr } else { stdout };
    Err(format!(
        "netsh exit {:?}: {}",
        output.status.code(),
        if msg.is_empty() { "geen output".into() } else { msg }
    ))
}
