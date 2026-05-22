//! Netwerkscan: ontdek alle bezette IP's op het lokale /24 en verrijk camera's, NVR's
//! en alarmpanelen met hun merk/product.
//!
//! Bewust "rustig": alle ARP-probes ([`crate::network::ip::check_ip_conflict`]) vuren
//! tegelijk af en de antwoorden worden binnen een kort venster verzameld. Verrijking per
//! bezet IP gebeurt met enkele korte TCP-poortchecks en gerichte fingerprints: NX-NVR
//! (plain HTTP op 7001) en — alleen voor niet-camera's — ATS-alarmpanelen (identify op
//! 5555/32000). Daarnaast loopt één Hikvision SADP-multicast voor het hele subnet. Geen
//! poort-bruteforce, geen credential-tests.

pub mod oui;
pub mod probe;
pub mod sadp;

use crate::network::ip;
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Korte timeout voor de TCP-poortchecks (klikbaarheid + camerabevestiging).
const PORT_TIMEOUT: Duration = Duration::from_millis(400);
/// Hoe lang we naar SADP-antwoorden (Hikvision) luisteren tijdens de scan.
const SADP_LISTEN: Duration = Duration::from_secs(2);
/// Veiligheidsplafond voor de ARP-fase. Alle probes vuren tegelijk: levende apparaten
/// antwoorden binnen milliseconden, lege adressen melden zich pas na de systeem-ARP-timeout
/// (~1–2 s). We wachten tot álle probes gemeld hebben (of dit plafond), zodat we niets
/// missen — door de volledige parallelliteit blijft dat ~2 s i.p.v. de oude ~20 s.
const ARP_MAX_WAIT: Duration = Duration::from_secs(3);
/// Kleine stack voor de korte ARP-probe-threads (we vuren er ~254 tegelijk af).
const PROBE_STACK: usize = 64 * 1024;

/// Eén ontdekt apparaat. Het MAC-adres blijft bewust intern (alleen voor de OUI-lookup).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// IPv4-adres, bv. "192.168.1.64".
    pub ip: String,
    /// Camera-merk ("Hikvision") of NVR-product ("Nx Witness V6.1"); `None` indien onbekend.
    pub brand: Option<String>,
    /// `true` als het apparaat (vrijwel zeker) een camera of NVR is.
    pub is_camera: bool,
    /// Optionele derde regel (bv. de paneelnaam van een alarmsysteem); `None` indien geen.
    pub detail: Option<String>,
    /// Klikbare webinterface-URL, of `None` als er geen webpoort open staat.
    pub web_url: Option<String>,
}

/// Gebeurtenis tijdens een scan: een gevonden apparaat, of voortgang van de ARP-sweep.
pub enum ScanEvent {
    Device(DeviceInfo),
    Progress { done: usize, total: usize },
}

/// Scan het /24-subnet van `subnet_base`. Roept `on_device` aan zodra een apparaat is
/// ontdekt en verrijkt — zo kan de UI resultaten live tonen i.p.v. te wachten tot het
/// einde. De callback wordt vanuit meerdere worker-threads aangeroepen.
pub fn scan_devices<F>(subnet_base: &str, on_event: F) -> Result<(), String>
where
    F: Fn(ScanEvent) + Send + Sync + 'static,
{
    let base: Ipv4Addr = subnet_base
        .parse()
        .map_err(|e| format!("ongeldig subnet base '{subnet_base}': {e}"))?;
    let o = base.octets();

    let candidates: Vec<String> = (1u8..=254u8)
        .map(|h| format!("{}.{}.{}.{}", o[0], o[1], o[2], h))
        .collect();
    let total = candidates.len();

    let on_event = Arc::new(on_event);

    // Hikvision SADP-discovery draait parallel: de modelinfo (bv. "Hikvision
    // DS-2CD2143G0-I") stroomt zo vaak al binnen vóór de ARP-fase klaar is.
    let sadp_handle = {
        let on_event = Arc::clone(&on_event);
        thread::spawn(move || {
            sadp::discover(base, SADP_LISTEN, move |dev| on_event(ScanEvent::Device(dev)));
        })
    };

    // Vuur álle ARP-probes tegelijk af; antwoorden komen via een channel binnen.
    let (tx, rx) = mpsc::channel::<(String, Option<String>)>();
    for ip_addr in candidates {
        let tx = tx.clone();
        let _ = thread::Builder::new()
            .stack_size(PROBE_STACK)
            .spawn(move || {
                let mac = ip::check_ip_conflict(&ip_addr, 0).ok().flatten();
                let _ = tx.send((ip_addr, mac));
            });
    }
    drop(tx);

    // Verzamel antwoorden tot álle probes gemeld hebben (of het plafond); verrijk
    // gevonden apparaten parallel zodra hun MAC binnen is.
    let deadline = Instant::now() + ARP_MAX_WAIT;
    let mut done = 0usize;
    let mut enrich_handles = Vec::new();
    while done < total {
        let remaining = match deadline.checked_duration_since(Instant::now()) {
            Some(r) if !r.is_zero() => r,
            _ => break,
        };
        match rx.recv_timeout(remaining) {
            Ok((ip_addr, mac)) => {
                done += 1;
                on_event(ScanEvent::Progress { done, total });
                if let Some(mac) = mac {
                    let on_event = Arc::clone(&on_event);
                    enrich_handles.push(thread::spawn(move || {
                        on_event(ScanEvent::Device(enrich(&ip_addr, &mac)));
                    }));
                }
            }
            Err(_) => break,
        }
    }
    // Niet-geantwoorde adressen (leeg) tellen als afgerond voor de voortgangsbalk.
    if done < total {
        on_event(ScanEvent::Progress { done: total, total });
    }

    for h in enrich_handles {
        let _ = h.join();
    }
    let _ = sadp_handle.join();

    Ok(())
}

/// Verrijk een bezet IP met merk/product en een eventuele klikbare webinterface.
///
/// Slimme volgorde: eerst classificeren als camera/NVR (NX op 7001, cameramerk via OUI,
/// of RTSP-poort 554). De duurdere ATS-alarmprobe (5555/32000) draaien we **alleen** als
/// het apparaat géén camera/NVR is — zo sturen we nooit een ATS-packet naar camera's.
fn enrich(ip_addr: &str, mac: &str) -> DeviceInfo {
    // NX-NVR (Network Optix) op 7001 wordt áltijd gecheckt: ook als er op poort 80 een
    // andere app draait, willen we weten dat 7001 een NX-server is. Heeft voorrang op de
    // OUI voor merk + URL (de NX-webinterface is de relevante interface van een NVR).
    if let Some(product) = probe::nx_fingerprint(ip_addr) {
        return DeviceInfo {
            ip: ip_addr.to_string(),
            brand: Some(product),
            is_camera: true,
            detail: None,
            web_url: Some(format!("https://{ip_addr}:{}/static/index.html#/", probe::NX_PORT)),
        };
    }

    // Cameraherkenning via MAC-OUI (gratis) + RTSP-poort 554. Is het een camera, dan
    // tonen we het merk en slaan we de ATS-probe over.
    let camera_brand = oui::lookup_brand(mac);
    let is_camera = camera_brand.is_some() || probe::port_open(ip_addr, 554, PORT_TIMEOUT);
    if is_camera {
        return DeviceInfo {
            ip: ip_addr.to_string(),
            brand: camera_brand.map(str::to_string),
            is_camera: true,
            detail: None,
            web_url: web_url_for(ip_addr),
        };
    }

    // Geen camera → kandidaat-alarmpaneel. ATS-identify (Aritech/Carrier) op poort 5555,
    // fallback 32000: type/generatie/firmware op regel 2, paneelnaam op regel 3 (detail).
    if let Some((label, panel_name)) = probe::ats_fingerprint(ip_addr) {
        return DeviceInfo {
            ip: ip_addr.to_string(),
            brand: Some(label),
            is_camera: false,
            detail: Some(panel_name),
            web_url: web_url_for(ip_addr),
        };
    }

    // Geen ATS-antwoord → val terug op een eventueel alarm-/securitymerk via de OUI.
    DeviceInfo {
        ip: ip_addr.to_string(),
        brand: oui::lookup_alarm_brand(mac).map(str::to_string),
        is_camera: false,
        detail: None,
        web_url: web_url_for(ip_addr),
    }
}

/// Klikbare webinterface-URL: poort 80 (http) heeft voorkeur, anders 443 (https).
fn web_url_for(ip_addr: &str) -> Option<String> {
    if probe::port_open(ip_addr, 80, PORT_TIMEOUT) {
        Some(format!("http://{ip_addr}"))
    } else if probe::port_open(ip_addr, 443, PORT_TIMEOUT) {
        Some(format!("https://{ip_addr}"))
    } else {
        None
    }
}
