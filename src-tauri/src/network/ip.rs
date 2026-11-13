use super::netsh;

pub fn set_static(adapter: &str, ip: &str, subnet: &str, gateway: &str) -> Result<(), String> {
    let name = format!("name={adapter}");
    let addr = format!("addr={ip}");
    let mask = format!("mask={subnet}");
    let gw = format!("gateway={gateway}");
    netsh::run(&[
        "interface", "ipv4", "set", "address",
        &name, "source=static", &addr, &mask, &gw,
    ])
}

/// Statisch IP zónder default gateway. Gebruikt door de DHCP-server: de adapter hangt
/// dan aan een los netwerk of een directe kabel, en mag de internetroute van de pc (via
/// WiFi of een tweede kaart) niet kapen.
pub fn set_static_no_gateway(adapter: &str, ip: &str, subnet: &str) -> Result<(), String> {
    let name = format!("name={adapter}");
    let addr = format!("addr={ip}");
    let mask = format!("mask={subnet}");
    netsh::run(&[
        "interface", "ipv4", "set", "address",
        &name, "source=static", &addr, &mask, "gateway=none",
    ])
}

pub fn set_dhcp(adapter: &str) -> Result<(), String> {
    let name = format!("name={adapter}");
    netsh::run(&["interface", "ipv4", "set", "address", &name, "source=dhcp"])
}

/// Zonder DHCP-server blijft `ipconfig /renew` ruim drie minuten hangen (gemeten: 219 s).
/// Een server die er is antwoordt binnen enkele seconden.
const RENEW_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(12);

/// Vraag opnieuw een DHCP-lease aan (`ipconfig /renew`) voor een kaart die op een
/// 169.254-noodadres staat. Windows probeert zelf pas na minuten opnieuw; de monteur die
/// net een kabel heeft ingestoken wil dat nu.
pub fn renew_dhcp(adapter: &str) -> Result<(), String> {
    match netsh::run_with_timeout("ipconfig", &["/renew", adapter], RENEW_TIMEOUT)? {
        true => Ok(()),
        false => Err("Geen antwoord van een DHCP-server".into()),
    }
}

/// Zet `ip` statisch op `adapter`, maar maak het adres eerst vrij als een slapende adapter
/// het nog vasthoudt. Windows bewaart de IP-configuratie van een virtuele hotspot-adapter
/// ook nadat die verdwenen is; na een crash of hard afsluiten staat het server-IP dan nog
/// op de oude adapter en weigert `set address` met "het object bestaat al". Een adapter
/// die UP is en het adres heeft is wel een echte botsing: dan een duidelijke fout.
pub fn claim_static_no_gateway(adapter: &str, ip: &str, subnet: &str) -> Result<(), String> {
    let holders = super::adapter::list_adapters()?
        .into_iter()
        .filter(|a| a.friendly_name != adapter && a.current_ip.as_deref() == Some(ip));
    for other in holders {
        if other.is_up {
            return Err(format!("{ip} is al in gebruik op '{}'", other.friendly_name));
        }
        set_dhcp(&other.friendly_name)?;
    }
    set_static_no_gateway(adapter, ip, subnet)
}

/// Vraag via ARP het MAC-adres bij `ip` op; `None` als niemand antwoordt. Met `src` (het
/// IP van een adapter) gaat de ARP via die adapter; zonder `src` kiest Windows er zelf een,
/// en dat is op een pc met meerdere kaarten niet altijd de gekozen kaart.
pub fn check_ip_conflict(ip: &str, src: Option<std::net::Ipv4Addr>) -> Result<Option<String>, String> {
    use std::net::Ipv4Addr;
    use windows::Win32::NetworkManagement::IpHelper::SendARP;

    let parsed: Ipv4Addr = ip
        .parse()
        .map_err(|e| format!("ongeldig IPv4 adres '{ip}': {e}"))?;
    let dest_ip: u32 = u32::from_le_bytes(parsed.octets());

    let mut mac_buf = [0u8; 8];
    let mut mac_len: u32 = 6;

    let result = unsafe {
        SendARP(
            dest_ip,
            src.map_or(0, |a| u32::from_le_bytes(a.octets())),
            mac_buf.as_mut_ptr() as *mut _,
            &mut mac_len,
        )
    };

    if result == 0 && mac_len >= 6 {
        let mac = format!(
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            mac_buf[0], mac_buf[1], mac_buf[2], mac_buf[3], mac_buf[4], mac_buf[5]
        );
        Ok(Some(mac))
    } else {
        Ok(None)
    }
}
