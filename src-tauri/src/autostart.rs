//! Autostart via de Windows Taakplanner.
//!
//! Waarom geen Run-registersleutel: T8-Lan vereist admin (manifest `requireAdministrator`)
//! en Windows start zulke programma's stilletjes NIET vanuit de Run-sleutel. Een taak met
//! "hoogste rechten" bij aanmelden werkt wel, zonder UAC-prompt.
//!
//! Waarom een XML-definitie i.p.v. `schtasks /Create /SC ONLOGON`: de standaardinstellingen
//! van `schtasks /Create` zijn ongeschikt voor een tray-app die altijd moet draaien:
//! "niet starten op accu", "stoppen bij overgang naar accu" en "taak na 72 uur beëindigen".
//! Dat verklaart waarom T8-Lan op laptops na een herstart (of na drie dagen) weg was.
//! De XML hieronder zet die beperkingen expliciet uit.
//!
//! De app is de enige eigenaar van de taak: bij elke start (release-build) wordt hij
//! opnieuw geregistreerd zodat het pad naar de exe en de instellingen altijd kloppen. De
//! installer roept `T8-Lan.exe --register-autostart` aan zodat de taak ook bestaat als de
//! gebruiker de app na installatie niet direct opent.

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

const TASK_NAME: &str = "T8-Lan";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Zet autostart aan of uit. Idempotent.
pub fn apply(enabled: bool) -> Result<(), String> {
    if enabled {
        register()
    } else {
        unregister()
    }
}

/// `true` als de taak bestaat.
pub fn is_registered() -> bool {
    schtasks(&["/Query", "/TN", TASK_NAME]).is_ok()
}

fn register() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("pad van exe onbekend: {e}"))?;
    let xml = task_xml(&exe, &current_user());
    let path = std::env::temp_dir().join("t8-lan-autostart.xml");
    write_utf16(&path, &xml)?;
    let result = schtasks(&[
        "/Create",
        "/TN",
        TASK_NAME,
        "/XML",
        &path.to_string_lossy(),
        "/F",
    ]);
    let _ = std::fs::remove_file(&path);
    result
}

fn unregister() -> Result<(), String> {
    // Een niet-bestaande taak is geen fout: het doel (geen autostart) is dan al bereikt.
    if !is_registered() {
        return Ok(());
    }
    schtasks(&["/Delete", "/TN", TASK_NAME, "/F"])
}

/// "DOMEIN\gebruiker" van het huidige proces. Dit is ook de gebruiker voor wie de taak
/// bij aanmelden afgaat (zelfde gedrag als `schtasks /Create` zonder `/RU`).
fn current_user() -> String {
    let user = std::env::var("USERNAME").unwrap_or_default();
    match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() => format!("{domain}\\{user}"),
        _ => user,
    }
}

fn task_xml(exe: &PathBuf, user: &str) -> String {
    let exe = xml_escape(&exe.to_string_lossy());
    let user = xml_escape(user);
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Author>Turn8</Author>
    <Description>Start T8-Lan in het systeemvak bij aanmelden.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
      <Delay>PT3S</Delay>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>false</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <DisallowStartOnRemoteAppSession>false</DisallowStartOnRemoteAppSession>
    <UseUnifiedSchedulingEngine>true</UseUnifiedSchedulingEngine>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{exe}</Command>
    </Exec>
  </Actions>
</Task>
"#
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Taakplanner-XML is UTF-16 LE met BOM (het formaat dat `schtasks /Query /XML` zelf
/// uitvoert); zo accepteert `schtasks /Create /XML` het zonder gezeur.
fn write_utf16(path: &std::path::Path, text: &str) -> Result<(), String> {
    let mut bytes = Vec::with_capacity(text.len() * 2 + 2);
    bytes.extend_from_slice(&[0xFF, 0xFE]);
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    std::fs::write(path, bytes).map_err(|e| format!("taak-xml schrijven: {e}"))
}

fn schtasks(args: &[&str]) -> Result<(), String> {
    let output = Command::new("schtasks.exe")
        .creation_flags(CREATE_NO_WINDOW)
        .args(args)
        .output()
        .map_err(|e| format!("schtasks starten mislukt: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let msg = if stderr.is_empty() { stdout } else { stderr };
    Err(format!(
        "schtasks exit {:?}: {}",
        output.status.code(),
        if msg.is_empty() { "geen output".into() } else { msg }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_contains_exe_user_and_no_battery_limits() {
        let xml = task_xml(&PathBuf::from(r"C:\Program Files\T8-Lan\T8-Lan.exe"), r"PC\bert");
        assert!(xml.contains(r"<Command>C:\Program Files\T8-Lan\T8-Lan.exe</Command>"));
        assert!(xml.contains(r"<UserId>PC\bert</UserId>"));
        assert!(xml.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"));
        assert!(xml.contains("<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>"));
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(xml.contains("<RunLevel>HighestAvailable</RunLevel>"));
    }

    #[test]
    fn xml_escapes_special_chars() {
        let xml = task_xml(&PathBuf::from(r"C:\A&B\T8-Lan.exe"), "x<y");
        assert!(xml.contains(r"C:\A&amp;B\T8-Lan.exe"));
        assert!(xml.contains("x&lt;y"));
    }
}
