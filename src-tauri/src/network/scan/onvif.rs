//! ONVIF WS-Discovery: als multicast (`239.255.255.250:3702`) en rechtstreeks per adres.
//!
//! Vrijwel elke IP-camera en NVR van de laatste tien jaar spreekt ONVIF, ongeacht merk
//! (Hikvision, Dahua, Axis, Uniview, Hanwha, Reolink, ...). Eén `Probe` levert per
//! apparaat een `ProbeMatch` met het adres (`XAddrs`) en scopes waarin fabrikant, naam en
//! hardwaretype staan. Daarmee herkennen we camera's die niet in de OUI-tabel staan en geen
//! SADP (Hikvision-specifiek) spreken.
//!
//! De probe rechtstreeks naar `ip:3702` is het vangnet: multicast-antwoorden komen niet
//! altijd aan (firewall, switch, ander VLAN), terwijl dezelfde probe als unicast wel
//! antwoord geeft, ook door een router heen (gemeten op 24 Hikvision-apparaten).
//!
//! Bewust "rustig": enkele probes, luisteren, klaar. Geen authenticatie, geen SOAP-calls
//! naar het apparaat.

use super::{rank, DeviceInfo};
use socket2::{Domain, Protocol, Socket, Type};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

const WSD_GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const WSD_PORT: u16 = 3702;
const PROBE_INTERVAL: Duration = Duration::from_millis(900);
/// Bij een probe per adres antwoorden honderden apparaten vrijwel tegelijk met 2-3 kB elk;
/// de standaardbuffer van 64 kB loopt dan over.
const RECV_BUFFER: usize = 2 * 1024 * 1024;

/// WS-Discovery Probe. `types` is leeg (alles) of `dn:NetworkVideoTransmitter` (camera's).
fn probe_xml(types: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<e:Envelope xmlns:e=\"http://www.w3.org/2003/05/soap-envelope\" \
xmlns:w=\"http://schemas.xmlsoap.org/ws/2004/08/addressing\" \
xmlns:d=\"http://schemas.xmlsoap.org/ws/2005/04/discovery\" \
xmlns:dn=\"http://www.onvif.org/ver10/network/wsdl\">\
<e:Header><w:MessageID>uuid:{}</w:MessageID>\
<w:To e:mustUnderstand=\"true\">urn:schemas-xmlsoap-org:ws:2005:04:discovery</w:To>\
<w:Action e:mustUnderstand=\"true\">http://schemas.xmlsoap.org/ws/2004/08/discovery/Probe</w:Action>\
</e:Header><e:Body><d:Probe><d:Types>{}</d:Types></d:Probe></e:Body></e:Envelope>",
        crate::util::pseudo_uuid().to_lowercase(),
        types
    )
}

/// WS-Discovery als multicast via de adapter met IP `iface`; roept `on_device` aan per
/// gevonden ONVIF-apparaat (ontdubbeld op IP). Faalt stil als multicast niet kan.
pub fn discover<F>(iface: Ipv4Addr, listen: Duration, on_device: F)
where
    F: Fn(DeviceInfo),
{
    let group: SocketAddr = SocketAddrV4::new(WSD_GROUP, WSD_PORT).into();
    probe_loop(iface, listen, on_device, |socket, probe| {
        let _ = socket.send_to(probe, group);
    });
}

/// Dezelfde probe rechtstreeks naar elk adres in `hosts`, via de adapter met IP `iface`.
pub fn discover_unicast<F>(iface: Ipv4Addr, hosts: &[Ipv4Addr], listen: Duration, on_device: F)
where
    F: Fn(DeviceInfo),
{
    probe_loop(iface, listen, on_device, |socket, probe| {
        for host in hosts {
            let _ = socket.send_to(probe, SocketAddrV4::new(*host, WSD_PORT));
        }
    });
}

/// Stuur elke [`PROBE_INTERVAL`] een probe via `send` en verwerk de antwoorden, `listen`
/// lang. Afwisselend een camera-specifieke en een generieke probe: sommige apparaten
/// reageren alleen op de één, andere alleen op de ander.
fn probe_loop<F, S>(iface: Ipv4Addr, listen: Duration, on_device: F, send: S)
where
    F: Fn(DeviceInfo),
    S: Fn(&UdpSocket, &[u8]),
{
    let Some(socket) = bind_socket(iface) else {
        return;
    };
    let _ = socket.set_read_timeout(Some(Duration::from_millis(300)));

    let probes = [probe_xml("dn:NetworkVideoTransmitter"), probe_xml("")];
    let start = Instant::now();
    let deadline = start + listen;
    let mut next_probe = start;
    let mut probe_idx = 0usize;
    // Antwoorden zijn 1,7 tot 3,4 kB (gemeten) en komen gefragmenteerd binnen.
    let mut buf = [0u8; 8192];
    let mut seen: Vec<String> = Vec::new();

    while Instant::now() < deadline {
        let now = Instant::now();
        if now >= next_probe {
            send(&socket, probes[probe_idx % probes.len()].as_bytes());
            probe_idx += 1;
            next_probe = now + PROBE_INTERVAL;
        }
        // Een fout is een read-timeout of de ICMP-melding van een adres zonder ONVIF.
        if let Ok((n, from)) = socket.recv_from(&mut buf) {
            let xml = String::from_utf8_lossy(&buf[..n]);
            let from_ip = match from {
                SocketAddr::V4(v4) => Some(*v4.ip()),
                _ => None,
            };
            if let Some(dev) = parse_probe_match(&xml, from_ip) {
                if !seen.contains(&dev.ip) {
                    seen.push(dev.ip.clone());
                    on_device(dev);
                }
            }
        }
    }
}

/// Bind op een vrije poort van de scan-adapter; antwoorden komen unicast terug.
fn bind_socket(iface: Ipv4Addr) -> Option<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).ok()?;
    let bind_addr: SocketAddr = SocketAddrV4::new(iface, 0).into();
    socket.bind(&bind_addr.into()).ok()?;
    let _ = socket.set_recv_buffer_size(RECV_BUFFER);
    let _ = socket.set_multicast_if_v4(&iface);
    let _ = socket.set_multicast_ttl_v4(1);
    Some(socket.into())
}

/// Parse een `ProbeMatch`. Het IP komt uit `XAddrs` (eerste http-URL); valt terug op het
/// afzenderadres als XAddrs ontbreekt of onbruikbaar is. Merk/model uit de scopes.
fn parse_probe_match(xml: &str, from: Option<Ipv4Addr>) -> Option<DeviceInfo> {
    if !xml.contains("ProbeMatch") {
        return None;
    }
    let xaddrs = extract_tag(xml, "XAddrs").unwrap_or_default();
    let ip = xaddrs
        .split_whitespace()
        .find_map(ip_from_url)
        .or_else(|| from.map(|a| a.to_string()))?;

    // WS-Discovery is breder dan ONVIF: ook printers en pc's antwoorden op de generieke
    // probe. Alleen een antwoord met ONVIF-scopes is een camera of recorder.
    let scopes = extract_tag(xml, "Scopes").unwrap_or_default();
    if !scopes.contains("onvif://") {
        return None;
    }
    let (brand, model, rank) = label_from_scopes(&scopes, xml);

    // Webinterface: de XAddrs-poort is de ONVIF-poort (vaak 80 of 8080), niet per se de
    // webpoort. Alleen bij poort 80 weten we zeker dat http://ip werkt.
    let web_url = xaddrs
        .split_whitespace()
        .find_map(|u| {
            let (host, port) = host_port_from_url(u)?;
            (host == ip && port == 80).then(|| format!("http://{ip}"))
        });

    Some(DeviceInfo {
        ip,
        brand,
        model,
        rank,
        is_camera: true,
        detail: None,
        web_url,
    })
}

/// Merk, type en herkomst ([`rank`]) uit de ONVIF-scopes.
///
/// Het type komt uit `hardware`: die scope is bij alle gemeten apparaten gevuld, terwijl
/// `name` op de camera aanpasbaar is en soms ontbreekt. `name` is alleen het type als er
/// geen `hardware` is. Het merk komt uit `manufacturer`, anders uit wat het antwoord
/// verraadt (Hikvision), anders uit het deel van `name` vóór het type
/// (`name/AXIS%20P3245` + `hardware/P3245` → "AXIS").
fn label_from_scopes(scopes: &str, xml: &str) -> (Option<String>, Option<String>, u8) {
    let mut name = None;
    let mut hardware = None;
    let mut manufacturer = None;
    for scope in scopes.split_whitespace() {
        let Some(rest) = scope.strip_prefix("onvif://www.onvif.org/") else {
            continue;
        };
        let Some((key, value)) = rest.split_once('/') else {
            continue;
        };
        let value = url_decode(value);
        if value.is_empty() {
            continue;
        }
        match key {
            "name" => name = Some(value),
            "hardware" => hardware = Some(value),
            "manufacturer" | "mfr" | "Manufacturer" => manufacturer = Some(value),
            _ => {}
        }
    }

    let brand = manufacturer
        .or_else(|| {
            let model = hardware.as_deref().or(name.as_deref()).unwrap_or("");
            is_hikvision(model, xml).then(|| "Hikvision".to_string())
        })
        .or_else(|| brand_before_model(name.as_deref()?, hardware.as_deref()?));
    let (model, rank) = match (hardware, name) {
        (Some(h), _) => (Some(h), rank::ONVIF_HARDWARE),
        (None, Some(n)) => (Some(n), rank::ONVIF_NAME),
        (None, None) if brand.is_some() => (None, rank::ONVIF_NAME),
        (None, None) => (None, 0),
    };
    (brand, model, rank)
}

/// Hikvision zet geen merk in de scopes van elke camera, maar is te herkennen aan zijn
/// eigen namespace in het antwoord en aan typenummers die met `DS-` of `iDS-` beginnen.
fn is_hikvision(model: &str, xml: &str) -> bool {
    model.starts_with("DS-") || model.starts_with("iDS-") || xml.contains("hikvision.com")
}

/// Het deel van `name` vóór het type, als `name` op het type eindigt.
fn brand_before_model(name: &str, hardware: &str) -> Option<String> {
    let cut = name.len().checked_sub(hardware.len())?;
    let (prefix, tail) = (name.get(..cut)?, name.get(cut..)?);
    let prefix = prefix.trim();
    (tail.eq_ignore_ascii_case(hardware) && !prefix.is_empty()).then(|| prefix.to_string())
}

/// Pak de tekst van het eerste element `<prefix:tag>` of `<tag>`, ongeacht de prefix
/// (`d:`, `wsdd:`, `tns:`, ...), die per fabrikant verschilt.
fn extract_tag(xml: &str, tag: &str) -> Option<String> {
    let mut search_from = 0usize;
    while let Some(rel) = xml[search_from..].find(&format!("{tag}>")) {
        let close_angle = search_from + rel + tag.len() + 1;
        // Het begin van deze opening-tag: terug naar '<'.
        let open_start = xml[..close_angle].rfind('<')?;
        let open_tag = &xml[open_start + 1..close_angle - 1];
        // Moet een opening zijn (geen '/') en `tag` als lokale naam hebben.
        let local = open_tag.rsplit(':').next().unwrap_or(open_tag);
        if !open_tag.starts_with('/') && local == tag {
            let end = xml[close_angle..].find("</")? + close_angle;
            return Some(xml[close_angle..end].trim().to_string());
        }
        search_from = close_angle;
    }
    None
}

fn host_port_from_url(url: &str) -> Option<(String, u16)> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))?;
    let authority = rest.split('/').next()?;
    let default_port = if url.starts_with("https://") { 443 } else { 80 };
    match authority.rsplit_once(':') {
        Some((host, port)) => Some((host.to_string(), port.parse().unwrap_or(default_port))),
        None => Some((authority.to_string(), default_port)),
    }
}

fn ip_from_url(url: &str) -> Option<String> {
    let (host, _) = host_port_from_url(url)?;
    host.parse::<Ipv4Addr>().ok().map(|ip| ip.to_string())
}

/// Minimale percent-decoding (scopes zijn URL-gecodeerd: `%20` = spatie).
fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<SOAP-ENV:Envelope xmlns:SOAP-ENV=\"http://www.w3.org/2003/05/soap-envelope\" \
xmlns:wsdd=\"http://schemas.xmlsoap.org/ws/2005/04/discovery\">\
<SOAP-ENV:Body><wsdd:ProbeMatches><wsdd:ProbeMatch>\
<wsdd:Types>dn:NetworkVideoTransmitter tds:Device</wsdd:Types>\
<wsdd:Scopes>onvif://www.onvif.org/type/video_encoder \
onvif://www.onvif.org/Profile/Streaming \
onvif://www.onvif.org/hardware/DS-2CD2143G0-I \
onvif://www.onvif.org/name/HIKVISION%20DS-2CD2143G0-I \
onvif://www.onvif.org/location/city/hangzhou</wsdd:Scopes>\
<wsdd:XAddrs>http://192.168.1.64/onvif/device_service</wsdd:XAddrs>\
<wsdd:MetadataVersion>10</wsdd:MetadataVersion>\
</wsdd:ProbeMatch></wsdd:ProbeMatches></SOAP-ENV:Body></SOAP-ENV:Envelope>";

    #[test]
    fn parses_hikvision_probe_match() {
        let dev = parse_probe_match(SAMPLE, None).expect("moet parsen");
        assert_eq!(dev.ip, "192.168.1.64");
        assert_eq!(dev.brand.as_deref(), Some("Hikvision"));
        assert_eq!(dev.model.as_deref(), Some("DS-2CD2143G0-I"));
        assert_eq!(dev.rank, rank::ONVIF_HARDWARE);
        assert!(dev.is_camera);
        assert_eq!(dev.web_url.as_deref(), Some("http://192.168.1.64"));
    }

    fn label(scopes: &str, xml: &str) -> (Option<String>, Option<String>, u8) {
        label_from_scopes(scopes, xml)
    }

    fn some(s: &str) -> Option<String> {
        Some(s.to_string())
    }

    #[test]
    fn manufacturer_scope_is_the_brand() {
        let scopes = "onvif://www.onvif.org/hardware/IPC-HDW2431T onvif://www.onvif.org/manufacturer/Dahua";
        assert_eq!(label(scopes, ""), (some("Dahua"), some("IPC-HDW2431T"), rank::ONVIF_HARDWARE));
        assert_eq!(label("onvif://www.onvif.org/Profile/S", ""), (None, None, 0));
    }

    /// Scopes van 192.168.101.108, gemeten 13-11-2026: geen `name`, geen `manufacturer`.
    #[test]
    fn hikvision_without_name_scope_still_gets_brand_and_type() {
        let scopes = "onvif://www.onvif.org/type/video_encoder onvif://www.onvif.org/Profile/Streaming \
onvif://www.onvif.org/Profile/G onvif://www.onvif.org/Profile/T onvif://www.onvif.org/Profile/M \
onvif://www.onvif.org/type/audio_encoder onvif://www.onvif.org/MAC/08:cc:81:f6:34:54 \
onvif://www.onvif.org/hardware/DS-2CD2387G3-LIS2UY/SL";
        let expected = (some("Hikvision"), some("DS-2CD2387G3-LIS2UY/SL"), rank::ONVIF_HARDWARE);
        assert_eq!(label(scopes, ""), expected);
    }

    #[test]
    fn type_comes_from_hardware_even_when_name_differs() {
        let scopes = "onvif://www.onvif.org/name/Voordeur onvif://www.onvif.org/hardware/DS-K1T673DWX";
        assert_eq!(label(scopes, ""), (some("Hikvision"), some("DS-K1T673DWX"), rank::ONVIF_HARDWARE));
    }

    #[test]
    fn brand_from_namespace_or_from_the_name_prefix() {
        let hik_ns = "xmlns:tnshik=\"http://www.hikvision.com/2011/event/topics\"";
        let scopes = "onvif://www.onvif.org/hardware/NVR-216";
        assert_eq!(label(scopes, hik_ns).0, some("Hikvision"));
        let scopes = "onvif://www.onvif.org/name/AXIS%20P3245-LV onvif://www.onvif.org/hardware/P3245-LV";
        assert_eq!(label(scopes, ""), (some("AXIS"), some("P3245-LV"), rank::ONVIF_HARDWARE));
    }

    #[test]
    fn non_80_onvif_port_gives_no_web_url() {
        let xml = SAMPLE.replace("http://192.168.1.64/onvif", "http://192.168.1.64:8899/onvif");
        let dev = parse_probe_match(&xml, None).unwrap();
        assert_eq!(dev.ip, "192.168.1.64");
        assert_eq!(dev.web_url, None);
    }

    #[test]
    fn uses_sender_when_xaddrs_missing() {
        let xml = "<d:ProbeMatch><d:Scopes>onvif://www.onvif.org/name/Cam</d:Scopes></d:ProbeMatch>";
        let dev = parse_probe_match(xml, Some(Ipv4Addr::new(10, 0, 0, 5))).unwrap();
        assert_eq!(dev.ip, "10.0.0.5");
        assert_eq!((dev.brand, dev.model.as_deref(), dev.rank), (None, Some("Cam"), rank::ONVIF_NAME));
    }

    #[test]
    fn ignores_ws_discovery_devices_without_onvif_scopes() {
        let xml = "<d:ProbeMatch><d:Types>wsdp:Device pub:Computer</d:Types><d:XAddrs>http://10.0.0.9:5357/x</d:XAddrs></d:ProbeMatch>";
        assert!(parse_probe_match(xml, Some(Ipv4Addr::new(10, 0, 0, 9))).is_none());
    }

    #[test]
    fn ignores_non_probe_match() {
        assert!(parse_probe_match("<e:Envelope><e:Body/></e:Envelope>", None).is_none());
    }

    #[test]
    fn extract_tag_ignores_prefix_and_closing_tags() {
        let xml = "<a:Env><b:XAddrs>http://1.2.3.4/x</b:XAddrs></a:Env>";
        assert_eq!(extract_tag(xml, "XAddrs").as_deref(), Some("http://1.2.3.4/x"));
        assert_eq!(extract_tag("<X></X>", "XAddrs"), None);
    }

    #[test]
    fn url_decode_handles_percent_and_plain() {
        assert_eq!(url_decode("A%20B%2DC"), "A B-C");
        assert_eq!(url_decode("plain"), "plain");
        assert_eq!(url_decode("bad%zz"), "bad%zz");
    }
}
