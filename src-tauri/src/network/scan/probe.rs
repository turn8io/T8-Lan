//! Lichte, "rustige" netwerkprobes: een TCP-poortcheck, een NX-NVR-fingerprint en een
//! ATS-alarmpaneel-fingerprint.
//!
//! Geen externe tools of crates — alles via `std::net::TcpStream`. De NX-fingerprint
//! gebruikt bewust *plain* HTTP op poort 7001: Nx Witness draait daar standaard met een
//! self-signed certificaat, waardoor een TLS-handshake onbetrouwbaar is. De plain-HTTP
//! `Server:`-header bevat alle informatie die we nodig hebben.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

/// Webpoort van een Network Optix / Nx Witness server.
pub const NX_PORT: u16 = 7001;

/// `true` als er een TCP-verbinding naar `ip:port` tot stand komt binnen `timeout`.
pub fn port_open(ip: &str, port: u16, timeout: Duration) -> bool {
    match format!("{ip}:{port}").parse::<SocketAddr>() {
        Ok(addr) => TcpStream::connect_timeout(&addr, timeout).is_ok(),
        Err(_) => false,
    }
}

/// Probeer een NX-NVR (Network Optix / Nx Witness) te herkennen op poort 7001.
///
/// Doet één plain-HTTP `GET /api/moduleInformation` en leest de `Server:`-header.
/// Geeft bv. `Some("Nx Witness V6.1")` terug, of `None` als het geen NX is.
pub fn nx_fingerprint(ip: &str) -> Option<String> {
    let addr: SocketAddr = format!("{ip}:{NX_PORT}").parse().ok()?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(500)).ok()?;
    stream.set_read_timeout(Some(Duration::from_millis(1200))).ok()?;
    stream.set_write_timeout(Some(Duration::from_millis(500))).ok()?;

    let request = format!(
        "GET /api/moduleInformation HTTP/1.0\r\nHost: {ip}\r\nUser-Agent: T8-LAN\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).ok()?;

    // We hebben enkel de responseheaders nodig; lees een beperkt venster.
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0u8; 1024];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() >= 4096 || buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => break,
        }
    }

    let text = String::from_utf8_lossy(&buf);
    let server = text
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("server:"))?;
    parse_nx_server(server)
}

/// Parse een `Server:`-header van een Network Optix server naar "Naam Vmajor.minor".
///
/// `Server: Nx Witness/6.1.0.42176 (Network Optix) Apache/2.4.16 (Unix)`
///   → `Some("Nx Witness V6.1")`
fn parse_nx_server(server_header: &str) -> Option<String> {
    if !server_header.contains("Network Optix") {
        return None;
    }
    // Waarde na "Server:" pakken en tot de eerste "/" = productnaam.
    let value = server_header.split_once(':')?.1.trim();
    let (product, rest) = value.split_once('/')?;
    let product = product.trim();
    // Versietoken loopt tot de eerste spatie, bv. "6.1.0.42176".
    let version = rest.split_whitespace().next().unwrap_or("");
    let mut parts = version.split('.');
    match (parts.next(), parts.next()) {
        (Some(major), Some(minor)) if !major.is_empty() => {
            Some(format!("{product} V{major}.{minor}"))
        }
        _ => Some(product.to_string()),
    }
}

/// Poorten van een Aritech/Carrier ATS-alarmpaneel: primair 5555, fallback 32000.
const ATS_PORTS: [u16; 2] = [5555, 32000];

/// ATS "Panel Identify"-request — exact deze bytes.
const ATS_IDENTIFY: [u8; 10] = [
    0xc0, 0xdb, 0xdc, 0x03, 0x50, 0x00, 0x00, 0x44, 0x24, 0xc0,
];

/// Probeer een ATS-alarmpaneel te herkennen. Eerst poort 5555; lukt dat niet (dicht,
/// timeout, of geen geldig ATS-antwoord) dan fallback naar 32000.
///
/// Geeft `(label, paneelnaam)` terug, bv. `("ATS1500AIP ATS3 MR_4.4.43343", "Bert en Manuela")`.
pub fn ats_fingerprint(ip: &str) -> Option<(String, String)> {
    ATS_PORTS.iter().find_map(|&port| ats_probe(ip, port))
}

/// Eén ATS-identify tegen `ip:port`: connect, stuur het identify-packet, lees het
/// antwoord en parse de ASCII-velden.
fn ats_probe(ip: &str, port: u16) -> Option<(String, String)> {
    let addr: SocketAddr = format!("{ip}:{port}").parse().ok()?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(600)).ok()?;
    stream.set_read_timeout(Some(Duration::from_millis(1500))).ok()?;
    stream.set_write_timeout(Some(Duration::from_millis(500))).ok()?;

    stream.write_all(&ATS_IDENTIFY).ok()?;

    let mut buf = [0u8; 1024];
    let n = stream.read(&mut buf).ok()?;
    if n == 0 {
        return None;
    }
    parse_ats(&buf[..n])
}

/// Parse een ATS-identify-antwoord: pak de printbare ASCII-strings (>3 tekens) en
/// interpreteer de eerste vier als paneelnaam, type, ATS-generatie en firmwareversie.
/// Geeft `(label, paneelnaam)` met label = "{type} {versie}" (de ATS-generatie tonen we
/// bewust niet — die voegt voor de gebruiker weinig toe).
fn parse_ats(data: &[u8]) -> Option<(String, String)> {
    let s = printable_strings(data, 4);
    match s.as_slice() {
        [name, ptype, _family, version, ..] => {
            Some((format!("{ptype} {version}"), name.clone()))
        }
        _ => None,
    }
}

/// Verzamel aaneengesloten reeksen van printbare ASCII-tekens (0x20–0x7e) van minimaal
/// `min_len` tekens lang.
fn printable_strings(data: &[u8], min_len: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for &b in data {
        if (0x20..=0x7e).contains(&b) {
            cur.push(b as char);
        } else {
            if cur.len() >= min_len {
                out.push(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
        }
    }
    if cur.len() >= min_len {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nx_witness_server_header() {
        let h = "Server: Nx Witness/6.1.0.42176 (Network Optix) Apache/2.4.16 (Unix)";
        assert_eq!(parse_nx_server(h).as_deref(), Some("Nx Witness V6.1"));
    }

    #[test]
    fn ignores_non_nx_server_header() {
        assert_eq!(parse_nx_server("Server: Apache/2.4.16 (Unix)"), None);
    }

    #[test]
    fn parses_ats_identify_response() {
        // Binaire framing met de vier ASCII-velden ertussen.
        let data = b"\x00\x01Bert en Manuela\x00\x10ATS1500AIP\x00ATS3\x00\x05MR_4.4.43343\x00";
        assert_eq!(
            parse_ats(data),
            Some(("ATS1500AIP MR_4.4.43343".to_string(), "Bert en Manuela".to_string()))
        );
    }

    #[test]
    fn ats_needs_at_least_four_fields() {
        let data = b"\x00ATS1500AIP\x00ATS3\x00";
        assert_eq!(parse_ats(data), None);
    }

    #[test]
    fn printable_strings_filters_short_runs() {
        let data = b"ab\x00Hello\x00x\x00World";
        // "ab" en "x" zijn te kort (<4); "Hello"/"World" blijven.
        assert_eq!(printable_strings(data, 4), vec!["Hello", "World"]);
    }
}
