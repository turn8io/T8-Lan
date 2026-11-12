//! Lichte, "rustige" netwerkprobes: een TCP-poortcheck, een NX-NVR-fingerprint, een
//! ATS-alarmpaneel-fingerprint en intercomherkenning (2N, Zenitel/Stentofon).
//!
//! Geen externe crates: TCP via `std::net`, http(s) via WinHTTP (de HTTP-stack van Windows
//! zelf, dus TLS zonder rustls-boom). De NX-fingerprint gebruikt bewust *plain* HTTP op
//! poort 7001: Nx Witness draait daar met een self-signed certificaat en de plain-HTTP
//! `Server:`-header bevat alles wat we nodig hebben.

use std::ffi::c_void;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption,
    WinHttpSetTimeouts, SECURITY_FLAG_IGNORE_CERT_CN_INVALID,
    SECURITY_FLAG_IGNORE_CERT_DATE_INVALID, SECURITY_FLAG_IGNORE_CERT_WRONG_USAGE,
    SECURITY_FLAG_IGNORE_UNKNOWN_CA, WINHTTP_ACCESS_TYPE_NO_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_OPEN_REQUEST_FLAGS, WINHTTP_OPTION_SECURITY_FLAGS, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE,
};

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
    // Een drukke NX-server (of een server achter een trage switch) haalt 500 ms soms
    // niet, zeker midden in de ARP-storm van de scan. Ruimere timeout + één herkansing.
    nx_fingerprint_once(ip, Duration::from_millis(1000))
        .or_else(|| nx_fingerprint_once(ip, Duration::from_millis(1500)))
}

fn nx_fingerprint_once(ip: &str, connect_timeout: Duration) -> Option<String> {
    let addr: SocketAddr = format!("{ip}:{NX_PORT}").parse().ok()?;
    let mut stream = TcpStream::connect_timeout(&addr, connect_timeout).ok()?;
    stream.set_read_timeout(Some(Duration::from_millis(2000))).ok()?;
    stream.set_write_timeout(Some(Duration::from_millis(800))).ok()?;

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

/// ATS "Panel Identify"-request - exact deze bytes.
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
/// bewust niet - die voegt voor de gebruiker weinig toe).
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

// ---- http(s) via WinHTTP ----

/// Maximale body die we lezen: de 2N-JSON en de loginpagina van een Stentofon passen ruim.
const HTTP_MAX_BODY: usize = 64 * 1024;
/// Per fase (resolve, connect, send, receive) van één http-probe.
const HTTP_TIMEOUT_MS: i32 = 1500;

/// Sluit een WinHTTP-handle bij het verlaten van de scope, ook na een vroege `?`.
struct WinHttpHandle(*mut c_void);

impl Drop for WinHttpHandle {
    fn drop(&mut self) {
        // SAFETY: de handle is door WinHTTP uitgegeven en wordt precies één keer gesloten.
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

/// Eén korte GET. Self-signed certificaten worden geaccepteerd: het gaat om herkenning op
/// het LAN, niet om vertrouwen. Geeft `(status, body)` terug, `None` als er niets te halen
/// viel (dicht, timeout, geen HTTP).
pub fn http_get(ip: &str, port: u16, secure: bool, path: &str) -> Option<(u32, String)> {
    // SAFETY: alle handles sluiten via `WinHttpHandle`; de buffers waarin WinHTTP schrijft
    // leven langer dan de aanroepen.
    unsafe {
        let session = WinHttpOpen(
            w!("T8-LAN"),
            WINHTTP_ACCESS_TYPE_NO_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        );
        if session.is_null() {
            return None;
        }
        let session = WinHttpHandle(session);
        WinHttpSetTimeouts(
            session.0,
            HTTP_TIMEOUT_MS,
            HTTP_TIMEOUT_MS,
            HTTP_TIMEOUT_MS,
            HTTP_TIMEOUT_MS,
        )
        .ok()?;

        let connect = WinHttpConnect(session.0, &HSTRING::from(ip), port, 0);
        if connect.is_null() {
            return None;
        }
        let connect = WinHttpHandle(connect);

        let flags = if secure { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) };
        let request = WinHttpOpenRequest(
            connect.0,
            w!("GET"),
            &HSTRING::from(path),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            flags,
        );
        if request.is_null() {
            return None;
        }
        let request = WinHttpHandle(request);

        if secure {
            let ignore = SECURITY_FLAG_IGNORE_UNKNOWN_CA
                | SECURITY_FLAG_IGNORE_CERT_CN_INVALID
                | SECURITY_FLAG_IGNORE_CERT_DATE_INVALID
                | SECURITY_FLAG_IGNORE_CERT_WRONG_USAGE;
            WinHttpSetOption(
                Some(request.0 as *const c_void),
                WINHTTP_OPTION_SECURITY_FLAGS,
                Some(&ignore.to_ne_bytes()),
            )
            .ok()?;
        }

        WinHttpSendRequest(request.0, None, None, 0, 0, 0).ok()?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).ok()?;

        let mut status = 0u32;
        let mut len = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut c_void),
            &mut len,
            std::ptr::null_mut(),
        )
        .ok()?;

        let mut body = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let mut read = 0u32;
            let ok = WinHttpReadData(
                request.0,
                chunk.as_mut_ptr() as *mut c_void,
                chunk.len() as u32,
                &mut read,
            )
            .is_ok();
            if !ok || read == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..read as usize]);
            if body.len() >= HTTP_MAX_BODY {
                break;
            }
        }
        Some((status, String::from_utf8_lossy(&body).into_owned()))
    }
}

// ---- Intercoms ----

/// Herkende intercom: merk en type voor regel 2, detail voor regel 3, en de webinterface.
pub struct Intercom {
    pub brand: String,
    pub detail: String,
    pub web_url: String,
}

/// Intercomherkenning zonder inloggen, alleen op poorten die al open bleken.
///
/// 2N: het open endpoint `/api/system/info` geeft type, firmware en serienummer als JSON;
/// https eerst, want 2N kan zijn API op "alleen TLS" hebben staan. Zenitel/Stentofon: de
/// StationWeb-loginpagina noemt het merk en de firmware (`?version=9.2.3.3(vsft)`); het
/// exacte type staat pas achter de login. Maximaal drie GET's per apparaat.
pub fn intercom_fingerprint(ip: &str, http_open: bool, https_open: bool) -> Option<Intercom> {
    let open: Vec<(&str, u16, bool)> = [("https", 443u16, true), ("http", 80u16, false)]
        .into_iter()
        .filter(|(_, port, _)| if *port == 443 { https_open } else { http_open })
        .collect();

    for (scheme, port, secure) in &open {
        if let Some((200, body)) = http_get(ip, *port, *secure, "/api/system/info") {
            if let Some((brand, detail)) = parse_2n_info(&body) {
                return Some(Intercom { brand, detail, web_url: format!("{scheme}://{ip}") });
            }
        }
    }
    // Zenitel luistert gewoonlijk op http; één poging, op de laagste open poort.
    if let Some((scheme, port, secure)) = open.last() {
        if let Some((_, body)) = http_get(ip, *port, *secure, "/") {
            if let Some((brand, detail)) = parse_zenitel_page(&body) {
                return Some(Intercom { brand, detail, web_url: format!("{scheme}://{ip}") });
            }
        }
    }
    None
}

/// 2N `/api/system/info`:
/// `{"success":true,"result":{"variant":"2N IP Verso 2.0","swVersion":"3.2.0.79.2",
/// "serialNumber":"52-6350-3806",...}}` → ("2N IP Verso 2.0", "Intercom, firmware ..., s/n ...").
fn parse_2n_info(body: &str) -> Option<(String, String)> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let r = v.get("result")?;
    let variant = r.get("variant")?.as_str()?.trim();
    let sw = r.get("swVersion")?.as_str()?.trim();
    if variant.is_empty() || sw.is_empty() {
        return None;
    }
    let brand = if variant.starts_with("2N") { variant.to_string() } else { format!("2N {variant}") };
    let mut detail = format!("Intercom, firmware {sw}");
    if let Some(sn) = r.get("serialNumber").and_then(|s| s.as_str()).filter(|s| !s.is_empty()) {
        detail.push_str(&format!(", s/n {sn}"));
    }
    Some((brand, detail))
}

/// Zenitel/Stentofon StationWeb-loginpagina: merk uit de tekst, firmware uit de
/// `?version=`-querystring van een script- of stylesheetverwijzing.
fn parse_zenitel_page(body: &str) -> Option<(String, String)> {
    let lower = body.to_ascii_lowercase();
    if !(lower.contains("zenitel") || lower.contains("stationweb")) {
        return None;
    }
    let detail = match zenitel_version(body) {
        Some((ver, Some(family))) => format!("Intercom, firmware {ver} ({family})"),
        Some((ver, None)) => format!("Intercom, firmware {ver}"),
        None => "Intercom".to_string(),
    };
    Some(("Zenitel Stentofon".to_string(), detail))
}

/// `?version=9.2.3.3(vsft)` → ("9.2.3.3", Some("vsft")); zonder haakjes geen familie.
fn zenitel_version(body: &str) -> Option<(String, Option<String>)> {
    let start = body.find("?version=")? + "?version=".len();
    let rest = &body[start..];
    let ver: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    if ver.is_empty() {
        return None;
    }
    let family = rest[ver.len()..]
        .trim_start()
        .strip_prefix('(')
        .and_then(|a| a.split(')').next())
        .filter(|f| !f.is_empty() && f.chars().all(|c| c.is_ascii_alphanumeric()))
        .map(str::to_string);
    Some((ver, family))
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

    /// Antwoord van een 2N IP Verso 2.0, gemeten 12-11-2026.
    #[test]
    fn parses_2n_system_info() {
        let body = r#"{"success":true,"result":{"variant":"2N IP Verso 2.0","serialNumber":"52-6350-3806","hwVersion":"570v1","swVersion":"3.2.0.79.2","buildType":"release","deviceName":"Voordeur","macAddr":"7C1EB3010203"}}"#;
        assert_eq!(
            parse_2n_info(body),
            Some((
                "2N IP Verso 2.0".to_string(),
                "Intercom, firmware 3.2.0.79.2, s/n 52-6350-3806".to_string()
            ))
        );
        assert_eq!(parse_2n_info(r#"{"success":false}"#), None);
        assert_eq!(parse_2n_info("<html>niet json</html>"), None);
    }

    /// Loginpagina van een Stentofon (Zenitel), gemeten 12-11-2026: alleen de firmware,
    /// het type staat achter de login.
    #[test]
    fn parses_zenitel_login_page() {
        let body = r#"<html><head><title>StationWeb</title><link rel="stylesheet" href="/css/main.css?version=9.2.3.3(vsft)"></head><body>Zenitel</body></html>"#;
        assert_eq!(
            parse_zenitel_page(body),
            Some(("Zenitel Stentofon".to_string(), "Intercom, firmware 9.2.3.3 (vsft)".to_string()))
        );
        assert_eq!(zenitel_version("x.js?version=1.2.3\""), Some(("1.2.3".to_string(), None)));
        assert_eq!(parse_zenitel_page("<html><title>Printer</title></html>"), None);
    }

    /// Live: `T8_HTTP_IP=192.168.62.1 cargo test --lib http_live -- --ignored --nocapture`.
    /// Bewijst de WinHTTP-route inclusief https met een self-signed certificaat.
    #[test]
    #[ignore]
    fn http_live() {
        let ip = std::env::var("T8_HTTP_IP").expect("T8_HTTP_IP");
        for (port, secure) in [(443u16, true), (80u16, false)] {
            match http_get(&ip, port, secure, "/") {
                Some((status, body)) => println!("{ip}:{port} secure={secure}: HTTP {status}, {} bytes", body.len()),
                None => println!("{ip}:{port} secure={secure}: geen antwoord"),
            }
        }
    }
}
