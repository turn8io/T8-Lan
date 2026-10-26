//! Gedeelde wrapper rond het `netsh`-commando voor IPv4-configuratie (adres + DNS), en de
//! generieke runner eronder voor andere systeemtools (`nat.rs` gebruikt PowerShell).
//! Draait zonder consolevenster en normaliseert de foutmelding.

use std::os::windows::process::CommandExt;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Voer `netsh` uit met de gegeven argumenten. `Ok(())` bij exitcode 0, anders een
/// foutmelding met de eerste niet-lege regel uit stderr/stdout.
pub fn run(args: &[&str]) -> Result<(), String> {
    run_program("netsh", args)
}

/// Zelfde contract als `run`, voor een willekeurig programma.
pub fn run_program(program: &str, args: &[&str]) -> Result<(), String> {
    let output = Command::new(program)
        .creation_flags(CREATE_NO_WINDOW)
        .args(args)
        .output()
        .map_err(|e| format!("{program} starten mislukt: {e}"))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let msg = if !stderr.is_empty() { stderr } else { stdout };
    Err(format!(
        "{program} exit {:?}: {}",
        output.status.code(),
        if msg.is_empty() { "geen output".into() } else { msg }
    ))
}
