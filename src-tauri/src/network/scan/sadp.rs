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
/// Interval waarmee we de inquiry herhalen: houdt de firewall-allowance vers en vangt
/// verloren multicast-pakketten op.
const PROBE_INTERVAL: Duration = Duration::from_secs(1);

/// De inquiry waar Hikvision-apparaten op reageren.
const INQUIRY: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
<Probe><Uuid>T8LAN0000-0000-0000-0000-000000000001</Uuid><Types>inquiry</Types></Probe>";

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
    let mut buf = [0u8; 4096];
    let mut seen: Vec<String> = Vec::new();

    while Instant::now() < deadline {
        // Herhaal de inquiry periodiek (t=0, 1s, 2s, ...).
        let now = Instant::now();
        if now >= next_probe {
            let _ = socket.send_to(INQUIRY.as_bytes(), dest);
            next_probe = now + PROBE_INTERVAL;
        }

        // Read-timeout (Err) → opnieuw proberen tot de deadline.
        if let Ok((n, _)) = socket.recv_from(&mut buf) {
            let xml = String::from_utf8_lossy(&buf[..n]);
            if let Some(dev) = parse_probe_match(&xml) {
                if !seen.contains(&dev.ip) {
                    seen.push(dev.ip.clone());
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

/// Parse een SADP-`ProbeMatch` naar een [`DeviceInfo`]. `None` als er geen IPv4 in staat
/// (bv. onze eigen `Probe`-echo, of een onbekend pakket).
fn parse_probe_match(xml: &str) -> Option<DeviceInfo> {
    let ip = extract(xml, "IPv4Address").filter(|s| !s.is_empty())?;

    let brand = match model_from(xml) {
        Some(model) => format!("Hikvision {model}"),
        None => "Hikvision".to_string(),
    };

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

    Some(DeviceInfo {
        ip,
        brand: Some(brand),
        is_camera: true,
        detail: None,
        web_url,
    })
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
<DeviceSN>DS-2CD2143G0-I20190101AAWR123456789</DeviceSN>\
<IPv4Address>192.168.1.64</IPv4Address><HttpPort>80</HttpPort></ProbeMatch>";

    #[test]
    fn parses_model_ip_and_link() {
        let dev = parse_probe_match(SAMPLE).expect("moet parsen");
        assert_eq!(dev.ip, "192.168.1.64");
        assert_eq!(dev.brand.as_deref(), Some("Hikvision DS-2CD2143G0-I"));
        assert!(dev.is_camera);
        assert_eq!(dev.web_url.as_deref(), Some("http://192.168.1.64"));
    }

    #[test]
    fn derives_model_from_serial_when_no_description() {
        let sn = "DS-2CD2143G0-I20190101AAWR123456789";
        assert_eq!(model_from_sn(sn), "DS-2CD2143G0-I");
    }

    #[test]
    fn ignores_packet_without_ip() {
        assert!(parse_probe_match("<Probe><Types>inquiry</Types></Probe>").is_none());
    }
}
