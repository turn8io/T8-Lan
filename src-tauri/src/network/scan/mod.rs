//! Netwerkscan: ontdek alle bezette IP's in een bereik en verrijk camera's, NVR's,
//! intercoms en alarmpanelen met merk en type.
//!
//! Het bereik is standaard het eigen subnet van de gekozen adapter (uit IP en masker), of
//! een door de monteur opgegeven subnet. Twee routes:
//!
//! - **Eigen subnet (on-link):** ARP-sweep via de gekozen adapter, plus Hikvision SADP en
//!   ONVIF als multicast.
//! - **Ander subnet (gerouteerd):** ARP en multicast komen niet door een router, dus daar
//!   bepalen een ping en enkele TCP-poorten of een adres leeft.
//!
//! In beide gevallen gaat er ook een ONVIF-probe rechtstreeks naar elk adres: die geeft het
//! type terug, ook als multicast niet aankomt en ook door een router heen.
//!
//! Bewust "rustig": korte poortchecks en gerichte fingerprints (NX op 7001, intercoms via
//! hun webpagina, ATS-identify op 5555/32000 alleen voor niet-camera's). Geen
//! poort-bruteforce, geen credential-tests.

pub mod onvif;
pub mod oui;
pub mod probe;
pub mod sadp;

use crate::network::{ip, netsh, ping};
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Korte timeout voor de TCP-poortchecks (klikbaarheid + camerabevestiging).
const PORT_TIMEOUT: Duration = Duration::from_millis(400);
/// Hoe lang we naar SADP-antwoorden (Hikvision) luisteren tijdens de scan.
const SADP_LISTEN: Duration = Duration::from_secs(3);
/// Hoe lang we naar ONVIF-antwoorden op de multicast-probe luisteren.
const ONVIF_LISTEN: Duration = Duration::from_millis(2500);
/// Luistertijd voor de ONVIF-probes per adres: drie rondes met een seconde ertussen, want
/// de eerste ronde valt deels weg zolang Windows de buren nog niet via ARP kent.
const ONVIF_UNICAST_LISTEN: Duration = Duration::from_millis(3500);
/// Veiligheidsplafond voor de ARP-fase. Alle probes vuren tegelijk: levende apparaten
/// antwoorden binnen milliseconden, lege adressen melden zich pas na de systeem-ARP-timeout
/// (~1–2 s). We wachten tot álle probes gemeld hebben (of dit plafond).
const ARP_MAX_WAIT: Duration = Duration::from_secs(3);
/// Tweede ARP-ronde voor adressen die in ronde één niet antwoordden. Bij honderden
/// gelijktijdige broadcasts (zeker via WiFi of een drukke switch) raakt wel eens een
/// request of reply kwijt; ronde twee vindt die alsnog.
const ARP_RETRY_WAIT: Duration = Duration::from_secs(2);
/// Plafond voor de leeft-het-check in een gerouteerd subnet: ping plus twee poortchecks.
const ROUTED_MAX_WAIT: Duration = Duration::from_secs(4);
const ROUTED_PING_MS: u32 = 800;
/// Poorten die een apparaat verraden dat geen ping beantwoordt (web en RTSP).
const ROUTED_PORTS: [u16; 2] = [80, 554];
/// Kleine stack voor de korte probe-threads (we vuren er honderden tegelijk af).
const PROBE_STACK: usize = 64 * 1024;
/// Grootste bereik dat in één scan past: /22 = 1022 adressen.
const MIN_PREFIX: u32 = 22;

/// Herkomst van merk en type, oplopend in betrouwbaarheid. De UI voegt meldingen voor
/// hetzelfde IP samen en laat per veld de hoogste rang winnen.
pub mod rank {
    /// Alleen een fabrikant uit het MAC-adres.
    pub const OUI: u8 = 1;
    /// ONVIF `name`-scope: op het apparaat aanpasbaar, dus minder hard dan `hardware`.
    pub const ONVIF_NAME: u8 = 2;
    pub const ONVIF_HARDWARE: u8 = 3;
    pub const SADP: u8 = 4;
    /// Het apparaat heeft zichzelf geïdentificeerd (NX, intercom, ATS-paneel).
    pub const FINGERPRINT: u8 = 5;
}

/// Eén ontdekt apparaat. Het MAC-adres blijft bewust intern (alleen voor de OUI-lookup).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// IPv4-adres, bv. "192.168.1.64".
    pub ip: String,
    /// Merk ("Hikvision") of volledig productlabel ("Nx Witness V6.1"); `None` indien onbekend.
    pub brand: Option<String>,
    /// Typenummer ("DS-2CD2387G3-LIS2UY/SL"), los van het merk; `None` indien onbekend.
    pub model: Option<String>,
    /// Herkomst van merk en type, zie [`rank`]; 0 als geen van beide bekend is.
    pub rank: u8,
    /// `true` als het apparaat (vrijwel zeker) een camera of NVR is.
    pub is_camera: bool,
    /// Optionele derde regel (bv. firmware of de paneelnaam van een alarmsysteem).
    pub detail: Option<String>,
    /// Klikbare webinterface-URL, of `None` als er geen webpoort open staat.
    pub web_url: Option<String>,
}

/// Gebeurtenis tijdens een scan.
pub enum ScanEvent {
    Device(DeviceInfo),
    Progress { done: usize, total: usize },
    /// Iets waardoor de scan minder vindt dan mogelijk; de code wordt in de UI vertaald:
    /// `"sadp"` (multicast-socket mislukt), `"firewall"` (geen inkomende regel voor deze
    /// exe) of `"routed"` (ander subnet: geen ARP en SADP).
    Warning(&'static str),
}

/// Wat een scan gaat doen: via welke adapter, welke adressen, en of die adressen direct
/// bereikbaar zijn (ARP en multicast) of achter een router liggen.
#[derive(Debug, PartialEq)]
pub struct ScanPlan {
    pub iface: Ipv4Addr,
    pub hosts: Vec<Ipv4Addr>,
    pub on_link: bool,
}

/// Bepaal het scanbereik. Zonder `target` het eigen subnet van de adapter (uit IP en
/// masker; bij een masker ruimer dan /22 het /24 rond het adapter-IP). Met `target` een
/// bereik ("192.168.101.1-254") of een subnet ("192.168.101.0/24", "192.168.101.0" of
/// "192.168.101", alle drie /24).
pub fn plan(iface_ip: &str, mask: Option<&str>, target: Option<&str>) -> Result<ScanPlan, String> {
    let iface: Ipv4Addr = iface_ip
        .parse()
        .map_err(|e| format!("ongeldig adapter-IP '{iface_ip}': {e}"))?;
    let own_prefix = mask
        .and_then(|m| m.parse::<Ipv4Addr>().ok())
        .map(|m| u32::from(m).leading_ones())
        .unwrap_or(24);

    let hosts = match target.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) if t.contains('-') => parse_range(t)?,
        Some(t) => {
            let (net, prefix) = parse_target(t)?;
            if prefix < MIN_PREFIX {
                return Err(format!("bereik te groot: maximaal /{MIN_PREFIX} (1022 adressen)"));
            }
            hosts_of(net, prefix)
        }
        None => {
            let prefix = if (MIN_PREFIX..=30).contains(&own_prefix) { own_prefix } else { 24 };
            hosts_of(u32::from(iface) & mask_of(prefix), prefix)
        }
    };
    // Direct bereikbaar als het hele bereik binnen het eigen subnet van de adapter valt.
    let own_net = |ip: &Ipv4Addr| u32::from(*ip) & mask_of(own_prefix);
    let on_link = hosts.iter().all(|h| own_net(h) == own_net(&iface));

    Ok(ScanPlan { iface, hosts, on_link })
}

/// "192.168.101.1-254" → de adressen .1 t/m .254 (grenzen inbegrepen).
fn parse_range(target: &str) -> Result<Vec<Ipv4Addr>, String> {
    let invalid = || format!("ongeldig bereik '{target}', bv. 192.168.101.1-254");
    let (first, last) = target.split_once('-').ok_or_else(invalid)?;
    let first: Ipv4Addr = first.trim().parse().map_err(|_| invalid())?;
    let last: u8 = last.trim().parse().map_err(|_| invalid())?;
    let [a, b, c, start] = first.octets();
    if last < start {
        return Err(invalid());
    }
    Ok((start..=last).map(|h| Ipv4Addr::new(a, b, c, h)).collect())
}

fn mask_of(prefix: u32) -> u32 {
    if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) }
}

/// "a.b.c.d/nn", "a.b.c.d" of "a.b.c" → (netwerkadres, prefix). Zonder prefix /24.
fn parse_target(target: &str) -> Result<(u32, u32), String> {
    let invalid = || format!("ongeldig subnet '{target}', bv. 192.168.101.0/24");
    let (addr, prefix) = match target.split_once('/') {
        Some((a, p)) => (a.trim(), p.trim().parse::<u32>().map_err(|_| invalid())?),
        None => (target, 24),
    };
    if prefix > 32 {
        return Err(invalid());
    }
    // Drie octetten ("192.168.101") zijn een /24 in monteurstaal.
    let full = if addr.matches('.').count() == 2 { format!("{addr}.0") } else { addr.to_string() };
    let ip: Ipv4Addr = full.parse().map_err(|_| invalid())?;
    Ok((u32::from(ip) & mask_of(prefix), prefix))
}

/// Alle hostadressen van een netwerk, zonder netwerk- en broadcastadres (behalve bij /31
/// en /32, waar die niet bestaan).
fn hosts_of(net: u32, prefix: u32) -> Vec<Ipv4Addr> {
    let size = 1u32 << (32 - prefix);
    let range = if size <= 2 { 0..size } else { 1..size - 1 };
    range.map(|h| Ipv4Addr::from(net + h)).collect()
}

/// Voer de scan uit. Roept `on_event` aan zodra een apparaat is ontdekt en verrijkt, zodat
/// de UI resultaten live toont. De callback komt vanuit meerdere worker-threads.
pub fn scan_devices<F>(plan: ScanPlan, on_event: F) -> Result<(), String>
where
    F: Fn(ScanEvent) + Send + Sync + 'static,
{
    let ScanPlan { iface, hosts, on_link } = plan;
    let total = hosts.len();
    let on_event = Arc::new(on_event);
    let mut listeners = Vec::new();

    // ONVIF rechtstreeks per adres: werkt ook als multicast niet aankomt of het bereik
    // achter een router ligt, en de modelinfo stroomt vaak al binnen vóór de sweep klaar is.
    {
        let on_event = Arc::clone(&on_event);
        let hosts = hosts.clone();
        listeners.push(thread::spawn(move || {
            onvif::discover_unicast(iface, &hosts, ONVIF_UNICAST_LISTEN, move |dev| {
                on_event(ScanEvent::Device(dev))
            });
        }));
    }

    if on_link {
        let ev = Arc::clone(&on_event);
        listeners.push(thread::spawn(move || {
            let found = Arc::clone(&ev);
            if sadp::discover(iface, SADP_LISTEN, move |dev| found(ScanEvent::Device(dev))).is_err() {
                ev(ScanEvent::Warning("sadp"));
            }
        }));
        let ev = Arc::clone(&on_event);
        listeners.push(thread::spawn(move || {
            onvif::discover(iface, ONVIF_LISTEN, move |dev| ev(ScanEvent::Device(dev)));
        }));
        // SADP-antwoorden komen als multicast binnen en hebben de inkomende firewallregel
        // van de installer nodig; een exe op een andere plek heeft die niet.
        let ev = Arc::clone(&on_event);
        listeners.push(thread::spawn(move || {
            if !firewall_rule_present() {
                ev(ScanEvent::Warning("firewall"));
            }
        }));
    } else {
        on_event(ScanEvent::Warning("routed"));
    }

    let targets: Vec<String> = hosts.iter().map(Ipv4Addr::to_string).collect();
    let mut enrich_handles = Vec::new();
    let mut done = 0usize;
    let mut report = |n: usize| {
        done = n;
        Some((n, total))
    };
    if on_link {
        // Ronde 1: álle ARP-probes tegelijk, via de gekozen adapter. Ronde 2: alleen de
        // adressen die niet antwoordden, als vangnet tegen verloren broadcasts.
        let arp = move |ip_addr: &str| ip::check_ip_conflict(ip_addr, Some(iface)).ok().flatten().map(Some);
        let silent = sweep(targets, ARP_MAX_WAIT, iface, arp, &on_event, &mut enrich_handles, &mut report);
        if !silent.is_empty() {
            // Voortgang blijft op het niveau van ronde 1 staan; ronde 2 is kort.
            sweep(silent, ARP_RETRY_WAIT, iface, arp, &on_event, &mut enrich_handles, &mut |_| None);
        }
    } else {
        let alive = move |ip_addr: &str| routed_alive(iface, ip_addr).then_some(None);
        sweep(targets, ROUTED_MAX_WAIT, iface, alive, &on_event, &mut enrich_handles, &mut report);
    }
    // Niet-geantwoorde adressen (leeg) tellen als afgerond voor de voortgangsbalk.
    if done < total {
        on_event(ScanEvent::Progress { done: total, total });
    }

    for h in enrich_handles.into_iter().chain(listeners) {
        let _ = h.join();
    }
    Ok(())
}

/// Leeft dit adres in een gerouteerd subnet? Ping eerst, twee keer: bij honderden pings
/// tegelijk raakt er wel eens één kwijt (gemeten op een alarmpaneel). Apparaten die ICMP
/// negeren verraden zich via een open web- of RTSP-poort.
fn routed_alive(iface: Ipv4Addr, ip_addr: &str) -> bool {
    let ping = || ping::ping_once(ip_addr, ROUTED_PING_MS).rtt_ms.is_some();
    ping() || ping() || ROUTED_PORTS.iter().any(|&p| probe::port_open(iface, ip_addr, p, PORT_TIMEOUT))
}

/// Bestaat de inkomende firewallregel "T8-Lan" voor de exe die nu draait?
fn firewall_rule_present() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return true;
    };
    let exe = exe.to_string_lossy().to_lowercase();
    netsh::output("netsh", &["advfirewall", "firewall", "show", "rule", "name=T8-Lan", "verbose"])
        .is_ok_and(|out| out.to_lowercase().contains(&exe))
}

/// Eén ronde over `targets`: alle probes tegelijk, antwoorden via een channel, tot alle
/// probes gemeld hebben of `max_wait` verstrijkt. `probe` geeft `Some(mac)` voor een bezet
/// adres (`mac` is `None` als dat onbekend is, zoals achter een router) en `None` voor een
/// leeg adres. Bezette adressen worden direct (parallel) verrijkt; `progress(n)` mag een
/// voortgangsmelding teruggeven. Geeft de adressen terug die (nog) niet antwoordden.
fn sweep<F, C>(
    targets: Vec<String>,
    max_wait: Duration,
    iface: Ipv4Addr,
    probe: C,
    on_event: &Arc<F>,
    enrich_handles: &mut Vec<thread::JoinHandle<()>>,
    progress: &mut dyn FnMut(usize) -> Option<(usize, usize)>,
) -> Vec<String>
where
    F: Fn(ScanEvent) + Send + Sync + 'static,
    C: Fn(&str) -> Option<Option<String>> + Send + Copy + 'static,
{
    let total = targets.len();
    let mut pending: Vec<String> = targets.clone();
    let (tx, rx) = mpsc::channel::<(String, Option<Option<String>>)>();
    for ip_addr in targets {
        let tx = tx.clone();
        let _ = thread::Builder::new().stack_size(PROBE_STACK).spawn(move || {
            let found = probe(&ip_addr);
            let _ = tx.send((ip_addr, found));
        });
    }
    drop(tx);

    let deadline = Instant::now() + max_wait;
    let mut done = 0usize;
    while done < total {
        let remaining = match deadline.checked_duration_since(Instant::now()) {
            Some(r) if !r.is_zero() => r,
            _ => break,
        };
        match rx.recv_timeout(remaining) {
            Ok((ip_addr, found)) => {
                done += 1;
                if let Some((d, t)) = progress(done) {
                    on_event(ScanEvent::Progress { done: d, total: t });
                }
                if let Some(mac) = found {
                    pending.retain(|p| p != &ip_addr);
                    let on_event = Arc::clone(on_event);
                    enrich_handles.push(thread::spawn(move || {
                        on_event(ScanEvent::Device(enrich(iface, &ip_addr, mac.as_deref())));
                    }));
                }
            }
            Err(_) => break,
        }
    }
    pending
}

/// Verrijk een bezet IP met merk/product en een eventuele klikbare webinterface. `mac` is
/// `None` in een gerouteerd subnet; dan valt de OUI-herkenning weg.
///
/// Slimme volgorde: NX op 7001, dan intercoms (vóór de cameraclassificatie, want een 2N
/// heeft ook RTSP open en zou anders "camera, merk onbekend" worden), dan camera/NVR
/// (cameramerk via OUI of RTSP-poort 554). De duurdere ATS-alarmprobe (5555/32000) draaien
/// we **alleen** als het apparaat géén camera/NVR is - zo sturen we nooit een ATS-packet
/// naar camera's.
fn enrich(iface: Ipv4Addr, ip_addr: &str, mac: Option<&str>) -> DeviceInfo {
    let device = |brand: Option<String>, rank: u8, is_camera: bool, detail, web_url| DeviceInfo {
        ip: ip_addr.to_string(),
        rank: if brand.is_some() { rank } else { 0 },
        brand,
        model: None,
        is_camera,
        detail,
        web_url,
    };

    // NX-NVR (Network Optix) op 7001 wordt áltijd gecheckt: ook als er op poort 80 een
    // andere app draait, willen we weten dat 7001 een NX-server is.
    if let Some(product) = probe::nx_fingerprint(iface, ip_addr) {
        let url = format!("https://{ip_addr}:{}/static/index.html#/", probe::NX_PORT);
        return device(Some(product), rank::FINGERPRINT, true, None, Some(url));
    }

    // Webpoorten één keer checken: bepalen de klikbare URL en of een http-probe zin heeft.
    let http_open = probe::port_open(iface, ip_addr, 80, PORT_TIMEOUT);
    let https_open = probe::port_open(iface, ip_addr, 443, PORT_TIMEOUT);
    let url = web_url(ip_addr, http_open, https_open);

    if let Some(ic) = probe::intercom_fingerprint(ip_addr, http_open, https_open) {
        return device(Some(ic.brand), rank::FINGERPRINT, false, Some(ic.detail), Some(ic.web_url));
    }

    // Cameraherkenning via MAC-OUI (gratis) + RTSP-poort 554. Is het een camera, dan
    // tonen we het merk en slaan we de ATS-probe over.
    let camera_brand = mac.and_then(oui::lookup_brand).map(str::to_string);
    if camera_brand.is_some() || probe::port_open(iface, ip_addr, 554, PORT_TIMEOUT) {
        return device(camera_brand, rank::OUI, true, None, url);
    }

    // Geen camera → kandidaat-alarmpaneel. ATS-identify (Aritech/Carrier) op poort 5555,
    // fallback 32000: type/generatie/firmware op regel 2, paneelnaam op regel 3 (detail).
    if let Some((label, panel_name)) = probe::ats_fingerprint(iface, ip_addr) {
        return device(Some(label), rank::FINGERPRINT, false, Some(panel_name), url);
    }

    // Geen ATS-antwoord → val terug op een eventueel alarm-/securitymerk via de OUI.
    let alarm_brand = mac.and_then(oui::lookup_alarm_brand).map(str::to_string);
    device(alarm_brand, rank::OUI, false, None, url)
}

/// Klikbare webinterface-URL: poort 80 (http) heeft voorkeur, anders 443 (https).
fn web_url(ip_addr: &str, http_open: bool, https_open: bool) -> Option<String> {
    if http_open {
        Some(format!("http://{ip_addr}"))
    } else if https_open {
        Some(format!("https://{ip_addr}"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn ip(s: &str) -> Ipv4Addr {
        s.parse().unwrap()
    }

    #[test]
    fn default_range_follows_the_adapter_mask() {
        let p = plan("192.168.62.225", Some("255.255.255.0"), None).unwrap();
        assert_eq!(p.hosts.len(), 254);
        assert_eq!((p.hosts[0], p.hosts[253]), (ip("192.168.62.1"), ip("192.168.62.254")));
        assert!(p.on_link);

        let p = plan("10.0.5.20", Some("255.255.254.0"), None).unwrap();
        assert_eq!(p.hosts.len(), 510);
        assert_eq!((p.hosts[0], p.hosts[509]), (ip("10.0.4.1"), ip("10.0.5.254")));
    }

    #[test]
    fn wide_adapter_mask_falls_back_to_the_slash_24_around_the_ip() {
        let p = plan("10.20.30.40", Some("255.0.0.0"), None).unwrap();
        assert_eq!(p.hosts.len(), 254);
        assert_eq!(p.hosts[0], ip("10.20.30.1"));
        assert!(p.on_link);
    }

    #[test]
    fn other_subnet_is_routed_and_accepts_short_notations() {
        for target in ["192.168.101.0/24", "192.168.101.0", "192.168.101", " 192.168.101.77 "] {
            let p = plan("192.168.21.5", Some("255.255.255.0"), Some(target)).unwrap();
            assert_eq!(p.hosts.len(), 254, "{target}");
            assert_eq!(p.hosts[0], ip("192.168.101.1"));
            assert!(!p.on_link);
        }
    }

    #[test]
    fn target_inside_a_wide_own_subnet_stays_on_link() {
        let p = plan("10.20.30.40", Some("255.255.0.0"), Some("10.20.99.0/24")).unwrap();
        assert!(p.on_link);
        // Groter dan het eigen subnet: een deel ligt achter de router.
        let p = plan("192.168.4.10", Some("255.255.255.0"), Some("192.168.4.0/22")).unwrap();
        assert_eq!(p.hosts.len(), 1022);
        assert!(!p.on_link);
    }

    #[test]
    fn rejects_too_large_and_malformed_targets() {
        assert!(plan("10.0.0.5", None, Some("10.0.0.0/16")).is_err());
        assert!(plan("10.0.0.5", None, Some("camera's")).is_err());
        assert!(plan("10.0.0.5", None, Some("10.0.0.0/40")).is_err());
        assert_eq!(plan("10.0.0.5", None, Some("10.0.0.9/32")).unwrap().hosts, vec![ip("10.0.0.9")]);
    }

    #[test]
    fn range_notation_gives_exactly_those_hosts() {
        let p = plan("192.168.21.5", Some("255.255.255.0"), Some("192.168.101.1-254")).unwrap();
        assert_eq!(p.hosts.len(), 254);
        assert_eq!((p.hosts[0], p.hosts[253]), (ip("192.168.101.1"), ip("192.168.101.254")));
        assert!(!p.on_link);

        let p = plan("192.168.21.5", Some("255.255.255.0"), Some("192.168.21.100 - 120")).unwrap();
        assert_eq!(p.hosts.len(), 21);
        assert!(p.on_link);

        assert!(plan("10.0.0.5", None, Some("10.0.0.50-20")).is_err());
        assert!(plan("10.0.0.5", None, Some("10.0.0-20")).is_err());
    }

    /// Live: `T8_SCAN_IP=192.168.62.225 cargo test --lib scan_live -- --ignored --nocapture`,
    /// optioneel met `T8_SCAN_MASK` en `T8_SCAN_TARGET` (ander subnet).
    #[test]
    #[ignore]
    fn scan_live() {
        let iface = std::env::var("T8_SCAN_IP").expect("T8_SCAN_IP");
        let mask = std::env::var("T8_SCAN_MASK").ok();
        let target = std::env::var("T8_SCAN_TARGET").ok();
        let plan = plan(&iface, mask.as_deref(), target.as_deref()).expect("plan");
        println!("{} adressen, on_link={}", plan.hosts.len(), plan.on_link);
        let found: Arc<Mutex<Vec<DeviceInfo>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&found);
        let start = Instant::now();
        scan_devices(plan, move |ev| match ev {
            ScanEvent::Device(d) => sink.lock().unwrap().push(d),
            ScanEvent::Warning(w) => println!("waarschuwing: {w}"),
            ScanEvent::Progress { .. } => {}
        })
        .expect("scan");
        let mut list = found.lock().unwrap().clone();
        list.sort_by_key(|d| d.ip.parse::<Ipv4Addr>().map(u32::from).unwrap_or(0));
        println!("scan in {:.1}s, {} meldingen", start.elapsed().as_secs_f32(), list.len());
        for d in &list {
            println!(
                "  {:<15} r{} {:<24} {:<26} {:<34} {}",
                d.ip,
                d.rank,
                d.brand.as_deref().unwrap_or("-"),
                d.model.as_deref().unwrap_or("-"),
                d.detail.as_deref().unwrap_or(""),
                d.web_url.as_deref().unwrap_or("")
            );
        }
    }
}
