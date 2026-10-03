//! ONVIF WS-Discovery over UDP-multicast (`239.255.255.250:3702`).
//!
//! Vrijwel elke IP-camera en NVR van de laatste tien jaar spreekt ONVIF, ongeacht merk
//! (Hikvision, Dahua, Axis, Uniview, Hanwha, Reolink, ...). Eén `Probe` levert per
//! apparaat een `ProbeMatch` met het adres (`XAddrs`) en scopes waarin fabrikant, naam en
//! hardwaretype staan. Daarmee herkennen we camera's die niet in de OUI-tabel staan en geen
//! SADP (Hikvision-specifiek) spreken.
//!
//! Bewust "rustig": twee probes, luisteren, klaar. Geen authenticatie, geen SOAP-calls naar
//! het apparaat.

use super::DeviceInfo;
use socket2::{Domain, Protocol, Socket, Type};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

const WSD_GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const WSD_PORT: u16 = 3702;
const PROBE_INTERVAL: Duration = Duration::from_millis(900);

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

/// Voer een WS-Discovery uit via de adapter met IP `iface`; roept `on_device` aan per
/// gevonden ONVIF-apparaat (gededupliceerd op IP). Faalt stil als multicast niet kan.
pub fn discover<F>(iface: Ipv4Addr, listen: Duration, on_device: F)
where
    F: Fn(DeviceInfo),
{
    let Some(socket) = bind_socket(iface) else {
        return;
    };
    let dest: SocketAddr = SocketAddrV4::new(WSD_GROUP, WSD_PORT).into();
    let _ = socket.set_read_timeout(Some(Duration::from_millis(300)));

    // Afwisselend een camera-specifieke en een generieke probe: sommige apparaten
    // reageren alleen op de één, andere alleen op de ander.
    let probes = [probe_xml("dn:NetworkVideoTransmitter"), probe_xml("")];
    let start = Instant::now();
    let deadline = start + listen;
    let mut next_probe = start;
    let mut probe_idx = 0usize;
    let mut buf = [0u8; 8192];
    let mut seen: Vec<String> = Vec::new();

    while Instant::now() < deadline {
        let now = Instant::now();
        if now >= next_probe {
            let _ = socket.send_to(probes[probe_idx % probes.len()].as_bytes(), dest);
            probe_idx += 1;
            next_probe = now + PROBE_INTERVAL;
        }
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

    let scopes = extract_tag(xml, "Scopes").unwrap_or_default();
    let brand = brand_from_scopes(&scopes);

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
        is_camera: true,
        detail: None,
        web_url,
    })
}

/// Merk + model uit ONVIF-scopes, bv.
/// `onvif://www.onvif.org/name/HIKVISION%20DS-2CD2143G0-I onvif://www.onvif.org/hardware/DS-2CD2143G0-I`
/// → `"HIKVISION DS-2CD2143G0-I"`. Voorkeur: `name` (bevat meestal merk+model), anders
/// `hardware`, anders `manufacturer`/`mfr`.
fn brand_from_scopes(scopes: &str) -> Option<String> {
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
    match (name, hardware, manufacturer) {
        // Naam bevat vaak al het merk; anders merk + hardware combineren.
        (Some(n), _, _) => Some(n),
        (None, Some(h), Some(m)) if !h.to_ascii_lowercase().contains(&m.to_ascii_lowercase()) => {
            Some(format!("{m} {h}"))
        }
        (None, Some(h), _) => Some(h),
        (None, None, Some(m)) => Some(m),
        _ => None,
    }
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
        assert_eq!(dev.brand.as_deref(), Some("HIKVISION DS-2CD2143G0-I"));
        assert!(dev.is_camera);
        assert_eq!(dev.web_url.as_deref(), Some("http://192.168.1.64"));
    }

    #[test]
    fn falls_back_to_hardware_and_manufacturer() {
        let scopes = "onvif://www.onvif.org/hardware/IPC-HDW2431T onvif://www.onvif.org/manufacturer/Dahua";
        assert_eq!(brand_from_scopes(scopes).as_deref(), Some("Dahua IPC-HDW2431T"));
        assert_eq!(brand_from_scopes("onvif://www.onvif.org/Profile/S"), None);
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
        assert_eq!(dev.brand.as_deref(), Some("Cam"));
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
