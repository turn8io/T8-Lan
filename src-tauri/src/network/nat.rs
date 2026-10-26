//! Internet delen via WinNAT (`New-NetNat`), het NAT dat Hyper-V en WSL ook gebruiken.
//!
//! Waarom niet ICS (Internet Connection Sharing, de techniek achter de Windows "Mobiele
//! hotspot"): die dwingt 192.168.137.x af en neemt DHCP over, en dan verliest de hotspot
//! ons adresplan en onze DHCP-server. WinNAT vertaalt alleen: alles uit het hotspot-subnet
//! gaat via de standaardroute van de pc naar buiten, de hotspot-adapter blijft het
//! gateway-adres en onze DHCP-server blijft uitdelen (met echte DNS-servers, want de pc
//! draait zelf geen DNS). Windows staat één NAT-netwerk per host toe; bestaat er al een,
//! dan geeft `New-NetNat` een duidelijke fout die we doorgeven.
//!
//! Er is geen netsh-commando voor WinNAT, alleen PowerShell-cmdlets; vandaar PowerShell
//! zonder profiel en zonder venster.

use crate::network::netsh;
use std::net::Ipv4Addr;

const NAT_NAME: &str = "T8-Lan";

/// Zet NAT aan voor het subnet van `server`/`mask`. Een oude regel met onze naam (na een
/// crash of hard afsluiten) gaat eerst weg, anders weigert `New-NetNat`.
pub fn enable(server: Ipv4Addr, mask: Ipv4Addr) -> Result<(), String> {
    let network = Ipv4Addr::from(u32::from(server) & u32::from(mask));
    let prefix = format!("{network}/{}", u32::from(mask).count_ones());
    disable();
    powershell(&format!(
        "New-NetNat -Name '{NAT_NAME}' -InternalIPInterfaceAddressPrefix '{prefix}' | Out-Null"
    ))
    .map_err(|e| format!("internet delen (NAT) aanzetten mislukt: {e}"))
}

/// Haal de NAT-regel weg; een ontbrekende regel is geen fout.
pub fn disable() {
    let _ = powershell(&format!(
        "Remove-NetNat -Name '{NAT_NAME}' -Confirm:$false -ErrorAction SilentlyContinue"
    ));
}

fn powershell(command: &str) -> Result<(), String> {
    netsh::run_program(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", command],
    )
}
