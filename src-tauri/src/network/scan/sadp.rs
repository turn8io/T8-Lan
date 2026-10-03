//! Hikvision SADP (Search Active Devices Protocol) discovery over UDP-multicast.
//!
//! Eén lichte multicast-inquiry naar `239.255.255.250:37020`; Hikvision-apparaten (en
//! OEM-rebrands die SADP spreken) antwoorden met een XML-blob met o.a. model, IPv4 en
//! HTTP-poort. Zo herkennen we het *exacte type* (bv. "Hikvision DS-2CD2143G0-I"), ook
//! als de OUI onbekend is — veel waardevoller dan enkel "Hikvision".
//!
//! Bewust "rustig": geen sweep, geen poort-bruteforce — één broadcast + luisteren.

use super::DeviceInfo;
use socket2::{Domain, Protocol, Socket, Type};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

const SADP_GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const SADP_PORT: u16 = 37020;
/// UDP is onbetrouwbaar: de inquiry gaat als burst van 3 met 200 ms ertussen (bewezen
/// tegen v3.0- én v3.1-firmware, zie Hik-SADP-discovery.md), daarna nog elke seconde.
const BURST_COUNT: usize = 3;
const BURST_GAP: Duration = Duration::from_millis(200);
const PROBE_INTERVAL: Duration = Duration::from_secs(1);

/// De inquiry waar Hikvision-apparaten op reageren. Nieuwere firmware controleert het
/// `Uuid`-veld strenger: het moet een echte (hex) UUID zijn, anders wordt de inquiry
/// genegeerd. Daarom per scan een verse, geldig gevormde UUID.
fn inquiry() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
<Probe><Uuid>{}</Uuid><Types>inquiry</Types></Probe>",
        crate::util::pseudo_uuid()
    )
}

/// Voer een SADP-discovery uit op de adapter met IP `iface` en roep `on_device` aan voor
/// elk gevonden Hikvision-apparaat. Luistert `listen` lang naar antwoorden.
///
/// Faalt stil (geen panic, geen fout) als poort 37020 bezet is of multicast geblokkeerd —
/// de rest van de scan loopt dan gewoon door.
pub fn discover<F>(iface: Ipv4Addr, listen: Duration, on_device: F)
where
    F: Fn(DeviceInfo),
{
    let socket = match bind_socket(iface) {
        Some(s) => s,
        None => return,
    };

    let dest: SocketAddr = SocketAddrV4::new(SADP_GROUP, SADP_PORT).into();
    let _ = socket.set_read_timeout(Some(Duration::from_millis(400)));

    let start = Instant::now();
    let deadline = start + listen;
    let mut next_probe = start; // eerste inquiry direct bij t=0
    let mut sent = 0usize;
    // v3.1-antwoorden (met Salt e.d.) zijn fors groter dan de oude 4 KiB.
    let mut buf = [0u8; 8192];
    // Ontdubbelen op MAC (elk apparaat antwoordt op elke probe, soms per interface);
    // terugval op IP als een firmware geen MAC meestuurt.
    let mut seen: Vec<String> = Vec::new();
    let inquiry = inquiry();

    while Instant::now() < deadline {
        let now = Instant::now();
        if now >= next_probe {
            let _ = socket.send_to(inquiry.as_bytes(), dest);
            sent += 1;
            next_probe = now + if sent < BURST_COUNT { BURST_GAP } else { PROBE_INTERVAL };
        }

        // Read-timeout (Err) → opnieuw proberen tot de deadline.
        if let Ok((n, _)) = socket.recv_from(&mut buf) {
            let xml = String::from_utf8_lossy(&buf[..n]);
            if let Some((key, dev)) = parse_probe_match(&xml) {
                if !seen.contains(&key) {
                    seen.push(key);
                    on_device(dev);
                }
            }
        }
    }
}

/// Bind op 0.0.0.0:37020 (vereist om multicast te ontvangen), join de SADP-groep en kies
/// expliciet de scan-adapter als multicast-interface.
fn bind_socket(iface: Ipv4Addr) -> Option<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).ok()?;
    socket.set_reuse_address(true).ok()?;
    let bind_addr: SocketAddr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, SADP_PORT).into();
    socket.bind(&bind_addr.into()).ok()?;
    socket.join_multicast_v4(&SADP_GROUP, &iface).ok()?;
    let _ = socket.set_multicast_if_v4(&iface);
    let _ = socket.set_multicast_loop_v4(false);
    Some(socket.into())
}

/// Parse een SADP-`ProbeMatch` naar `(ontdubbelsleutel, DeviceInfo)`. `None` voor alles
/// wat geen ProbeMatch met IPv4 is (onze eigen `Probe`-echo, een ander SADP-type, junk).
/// De sleutel is de MAC (kleine letters, dubbele punten), of het IP als die ontbreekt.
fn parse_probe_match(xml: &str) -> Option<(String, DeviceInfo)> {
    if !xml.contains("<ProbeMatch") {
        return None;
    }
    let ip = extract(xml, "IPv4Address").filter(|s| !s.is_empty())?;
    let mac = extract(xml, "MAC")
        .map(|m| m.to_lowercase().replace('-', ":"))
        .filter(|m| !m.is_empty());
    let key = mac.clone().unwrap_or_else(|| ip.clone());

    let brand = match model_from(xml) {
        Some(model) => format!("Hikvision {model}"),
        None => "Hikvision".to_string(),
    };

    // Derde regel: firmware, generatie (v3.1 herken je aan Encrypt=true of SADPVersion
    // die met 3 begint) en of het apparaat nog geactiveerd moet worden. Precies wat een
    // monteur wil weten voordat hij de webinterface opent.
    let is_v31 = extract(xml, "Encrypt").is_some_and(|e| e.eq_ignore_ascii_case("true"))
        || extract(xml, "SADPVersion").is_some_and(|v| v.trim_start().starts_with('3'));
    let mut parts: Vec<String> = Vec::new();
    if let Some(fw) = extract(xml, "SoftwareVersion").filter(|s| !s.is_empty()) {
        // "V5.9.23build 240401" → "V5.9.23"
        parts.push(fw.split("build").next().unwrap_or(&fw).trim().to_string());
    }
    parts.push(if is_v31 { "SADP v3.1".into() } else { "SADP v3.0".into() });
    if extract(xml, "Activated").is_some_and(|a| a.eq_ignore_ascii_case("false")) {
        parts.push("not activated".into());
    }
    let detail = Some(parts.join(" · "));

    // SADP meldt de HTTP-poort zelf — daarmee bouwen we direct een klikbare link.
    let web_url = extract(xml, "HttpPort")
        .and_then(|p| p.parse::<u16>().ok())
        .map(|port| {
            if port == 80 {
                format!("http://{ip}")
            } else {
                format!("http://{ip}:{port}")
            }
        });

    Some((
        key,
        DeviceInfo {
            ip,
            brand: Some(brand),
            is_camera: true,
            detail,
            web_url,
        },
    ))
}

/// Haal het modeltype uit de SADP-XML. `DeviceDescription` bevat doorgaans het type;
/// anders leiden we het af uit het serienummer (model staat vooraan), met `DeviceType` als
/// laatste redmiddel.
fn model_from(xml: &str) -> Option<String> {
    if let Some(d) = extract(xml, "DeviceDescription").filter(|s| !s.is_empty()) {
        return Some(d);
    }
    if let Some(sn) = extract(xml, "DeviceSN").filter(|s| !s.is_empty()) {
        return Some(model_from_sn(&sn));
    }
    extract(xml, "DeviceType").filter(|s| !s.is_empty())
}

/// Hikvision-serienummers zijn "<MODEL><lange cijferreeks>". Knip bij de eerste reeks van
/// 8+ cijfers (datum/serienr), zodat het modeltype overblijft.
fn model_from_sn(sn: &str) -> String {
    let mut run = 0usize;
    for (i, b) in sn.bytes().enumerate() {
        if b.is_ascii_digit() {
            run += 1;
            if run >= 8 {
                let cut = i + 1 - run;
                return sn[..cut].trim_end_matches(['-', ' ']).to_string();
            }
        } else {
            run = 0;
        }
    }
    sn.to_string()
}

/// Pak de tekst tussen `<tag>` en `</tag>`.
fn extract(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(xml[start..end].trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?><ProbeMatch>\
<Types>inquiry</Types><DeviceType>1</DeviceType>\
<DeviceDescription>DS-2CD2143G0-I</DeviceDescription>\
<DeviceSN>DS-2CD2143G0-I20190101AAWR123456789</DeviceSN><MAC>50-E5-38-D9-1E-CB</MAC>\
<IPv4Address>192.168.1.64</IPv4Address><HttpPort>80</HttpPort>\
<Activated>false</Activated><SoftwareVersion>V5.5.160build 230101</SoftwareVersion></ProbeMatch>";

    const V31: &str = "<ProbeMatch><Types>inquiry</Types>\
<DeviceSN>iDS-2CD7547G2/P-XZHSY20240101AAWR123456789</DeviceSN>\
<IPv4Address>192.168.62.227</IPv4Address><HttpPort>8080</HttpPort>\
<Encrypt>true</Encrypt><SADPVersion>3.0,3.1.1</SADPVersion></ProbeMatch>";

    #[test]
    fn parses_model_ip_link_and_detail() {
        let (key, dev) = parse_probe_match(SAMPLE).expect("moet parsen");
        assert_eq!(key, "50:e5:38:d9:1e:cb");
        assert_eq!(dev.ip, "192.168.1.64");
        assert_eq!(dev.brand.as_deref(), Some("Hikvision DS-2CD2143G0-I"));
        assert!(dev.is_camera);
        assert_eq!(dev.web_url.as_deref(), Some("http://192.168.1.64"));
        assert_eq!(dev.detail.as_deref(), Some("V5.5.160 · SADP v3.0 · not activated"));
    }

    #[test]
    fn parses_v31_device_model_from_serial_and_falls_back_to_ip_key() {
        let (key, dev) = parse_probe_match(V31).unwrap();
        assert_eq!(key, "192.168.62.227");
        assert_eq!(dev.brand.as_deref(), Some("Hikvision iDS-2CD7547G2/P-XZHSY"));
        assert_eq!(dev.web_url.as_deref(), Some("http://192.168.62.227:8080"));
        assert_eq!(dev.detail.as_deref(), Some("SADP v3.1"));
    }

    #[test]
    fn derives_model_from_serial_when_no_description() {
        let sn = "DS-2CD2143G0-I20190101AAWR123456789";
        assert_eq!(model_from_sn(sn), "DS-2CD2143G0-I");
    }

    #[test]
    fn ignores_own_probe_and_junk() {
        assert!(parse_probe_match("<Probe><Types>inquiry</Types></Probe>").is_none());
        assert!(parse_probe_match("<ProbeMatch><Types>inquiry</Types></ProbeMatch>").is_none());
    }
}
