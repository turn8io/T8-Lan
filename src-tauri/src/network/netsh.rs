//! Gedeelde wrapper rond het `netsh`-commando voor IPv4-configuratie (adres + DNS), en de
//! generieke runner eronder voor andere systeemtools (`nat.rs` gebruikt PowerShell).
//! Draait zonder consolevenster en normaliseert de foutmelding.

use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Voer `netsh` uit met de gegeven argumenten. `Ok(())` bij exitcode 0, anders een
/// foutmelding met de eerste niet-lege regel uit stderr/stdout.
pub fn run(args: &[&str]) -> Result<(), String> {
    run_program("netsh", args)
}

/// Zelfde contract als `run`, voor een willekeurig programma.
pub fn run_program(program: &str, args: &[&str]) -> Result<(), String> {
    output(program, args).map(|_| ())
}

/// Voer een programma uit en geef bij exitcode 0 zijn stdout terug.
pub fn output(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .creation_flags(CREATE_NO_WINDOW)
        .args(args)
        .output()
        .map_err(|e| format!("{program} starten mislukt: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() {
        return Ok(stdout);
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let msg = if !stderr.is_empty() { stderr } else { stdout };
    Err(format!(
        "{program} exit {:?}: {}",
        output.status.code(),
        if msg.is_empty() { "geen output".into() } else { msg }
    ))
}

/// Start een programma en wacht hooguit `timeout`. Loopt het langer, dan wordt het
/// afgebroken en is het resultaat `Ok(false)`; `Ok(true)` bij exitcode 0.
pub fn run_with_timeout(program: &str, args: &[&str], timeout: Duration) -> Result<bool, String> {
    let mut child = Command::new(program)
        .creation_flags(CREATE_NO_WINDOW)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{program} starten mislukt: {e}"))?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.success()),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(200)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(false);
            }
        }
    }
}
