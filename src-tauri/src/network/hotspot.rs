//! WiFi-hotspot: een toegangspunt (gewone SSID + WPA2-wachtwoord) op een virtuele adapter
//! die de hotspot zelf van het server-IP (standaard 192.168.8.8) voorziet en bij stoppen
//! weer vrijgeeft. De eigen DHCP-server kan er optioneel op draaien, met dezelfde
//! instellingen als bij een directe kabel.
//!
//! Waarom niet de Windows "Mobiele hotspot": die hangt aan Internet Connection Sharing,
//! met een vast 192.168.137.1 en een eigen DHCP-server, en eist een te delen verbinding.
//!
//! De gebruiker kiest een WiFi-kaart. Windows laat dat bij geen enkele techniek
//! rechtstreeks kiezen, dus deze module kiest de techniek die daar het beste kan landen en
//! controleert het na de start:
//!
//! - Wi-Fi Direct: een "autonome group owner" met legacy-instellingen, zodat ook camera's
//!   en telefoons zonder Wi-Fi Direct kunnen verbinden. Werkt op vrijwel elke ingebouwde
//!   kaart sinds Windows 10; Windows kiest zelf de kaart, in de praktijk de ingebouwde.
//! - Hosted network (`netsh wlan hostednetwork`): door Microsoft afgeschaft en op moderne
//!   ingebouwde chips weg, maar door veel USB-adapters nog ondersteund. Windows pakt de
//!   eerste kaart die het kan; of een kaart het kan, zegt de driver zelf (`wifi.rs`).
//!
//! Na de start wordt aan het MAC-adres (en de omschrijving) gecontroleerd of de virtuele
//! adapter van de gekozen kaart afstamt. Zo niet, dan gaat die techniek weer uit en komt de
//! andere aan de beurt; lukt geen van beide, dan één duidelijke fout.
//!
//! De WinRT-objecten van Wi-Fi Direct leven op een eigen worker-thread (MTA) die zo lang
//! bestaat als het proces; commando's gaan via een channel. Hosted network is puur netsh en
//! draait op de aanroepende thread.

use crate::network::{adapter, ip, nat, netsh, wifi, AdapterInfo, AdapterKind};
use serde::Serialize;
use std::collections::HashSet;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::core::{GUID, HSTRING};
use windows::Devices::WiFiDirect::{
    WiFiDirectAdvertisementPublisher, WiFiDirectAdvertisementPublisherStatus as PubStatus,
    WiFiDirectAdvertisementPublisherStatusChangedEventArgs, WiFiDirectError,
};
use windows::Foundation::TypedEventHandler;
use windows::Security::Credentials::PasswordCredential;
use windows::Win32::Foundation::NO_ERROR;
use windows::Win32::NetworkManagement::IpHelper::ConvertInterfaceLuidToGuid;
use windows::Win32::NetworkManagement::Ndis::NET_LUID_LH;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

/// Hoe lang we wachten tot het toegangspunt "Started" meldt en de virtuele adapter omhoog is.
const START_TIMEOUT: Duration = Duration::from_secs(12);

/// Techniek waarmee het toegangspunt is opgezet; zie de moduledocumentatie.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HotspotMethod {
    WifiDirect,
    HostedNetwork,
}

#[derive(Debug, Clone, Serialize)]
pub struct HotspotInfo {
    pub ssid: String,
    /// Friendly name van de virtuele adapter waarop het toegangspunt draait.
    pub adapter: String,
    pub method: HotspotMethod,
    /// Server-IP en masker op de virtuele adapter; ook de basis voor de NAT-regel.
    pub server_ip: String,
    pub subnet: String,
    /// Internet delen staat aan (WinNAT-regel actief, zie `nat.rs`).
    pub internet: bool,
}

/// Fysieke WiFi-kaart waar de hotspot op kan, voor de keuzelijst in de DHCP-tab.
#[derive(Debug, Clone, Serialize)]
pub struct HotspotCard {
    pub friendly_name: String,
    pub description: String,
    pub hosted_capable: bool,
}

pub fn cards() -> Vec<HotspotCard> {
    let hosted = wifi::hosted_network_capable();
    adapter::list_adapters()
        .unwrap_or_default()
        .into_iter()
        .filter(is_physical_wifi)
        .map(|a| {
            let hosted_capable = hosted_capable(&a, &hosted);
            HotspotCard {
                friendly_name: a.friendly_name,
                description: a.description,
                hosted_capable,
            }
        })
        .collect()
}

enum Cmd {
    Start {
        ssid: String,
        passphrase: String,
        reply: Sender<Result<String, String>>,
    },
    Stop {
        reply: Sender<()>,
    },
}

pub struct Hotspot {
    tx: Mutex<Option<Sender<Cmd>>>,
    info: Mutex<Option<HotspotInfo>>,
}

impl Hotspot {
    pub fn new() -> Self {
        Self {
            tx: Mutex::new(None),
            info: Mutex::new(None),
        }
    }

    pub fn info(&self) -> Option<HotspotInfo> {
        self.info.lock().ok().and_then(|g| g.clone())
    }

    pub fn is_running(&self) -> bool {
        self.info().is_some()
    }

    /// Start het toegangspunt op `card` (friendly name; `None` laat Windows kiezen), zet
    /// `server_ip`/`subnet` op de virtuele adapter, met `internet` ook de NAT-regel, en
    /// geef terug waar het draait.
    pub fn start(
        &self,
        ssid: &str,
        passphrase: &str,
        card: Option<&str>,
        server_ip: &str,
        subnet: &str,
        internet: bool,
    ) -> Result<HotspotInfo, String> {
        let ssid = ssid.trim();
        if ssid.is_empty() || ssid.len() > 32 {
            return Err("netwerknaam moet 1 t/m 32 tekens zijn".into());
        }
        if !(8..=63).contains(&passphrase.len()) || !passphrase.is_ascii() {
            return Err("wachtwoord moet 8 t/m 63 tekens zijn (WPA2)".into());
        }
        if self.is_running() {
            return Err("hotspot draait al".into());
        }
        let target = match card {
            None => None,
            Some(name) => Some(
                physical_cards()
                    .into_iter()
                    .find(|a| a.friendly_name == name)
                    .ok_or_else(|| format!("WiFi-kaart '{name}' niet gevonden"))?,
            ),
        };

        let mut last_err = String::from("geen hotspot-techniek beschikbaar");
        for method in plan_for(target.as_ref()) {
            let adapter_name = match self.start_method(ssid, passphrase, method) {
                Ok(name) => name,
                Err(e) => {
                    last_err = e;
                    continue;
                }
            };
            if let Some(t) = &target {
                let landed = adapter::list_adapters()
                    .unwrap_or_default()
                    .into_iter()
                    .find(|a| a.friendly_name == adapter_name);
                if !landed.as_ref().is_some_and(|v| same_card(v, t)) {
                    self.stop_method(method);
                    last_err = format!(
                        "Windows zette de hotspot op '{adapter_name}' in plaats van op '{}'",
                        t.friendly_name
                    );
                    continue;
                }
            }
            // Windows zet de virtuele adapter eerst op APIPA; even wachten voordat netsh erop
            // schrijft. De hotspot is eigenaar van dit adres en geeft het bij `stop` weer vrij.
            std::thread::sleep(Duration::from_millis(500));
            if let Err(e) = ip::claim_static_no_gateway(&adapter_name, server_ip, subnet) {
                self.stop_method(method);
                return Err(e);
            }
            if internet {
                if let Err(e) = enable_nat(server_ip, subnet) {
                    let _ = ip::set_dhcp(&adapter_name);
                    self.stop_method(method);
                    return Err(e);
                }
            } else {
                // Een oude NAT-regel (na een crash of hard afsluiten) zou anders stilletjes
                // toch internet delen op een hotspot die gesloten hoort te zijn.
                nat::disable();
            }
            let info = HotspotInfo {
                ssid: ssid.to_string(),
                adapter: adapter_name,
                method,
                server_ip: server_ip.to_string(),
                subnet: subnet.to_string(),
                internet,
            };
            if let Ok(mut g) = self.info.lock() {
                *g = Some(info.clone());
            }
            return Ok(info);
        }
        Err(last_err)
    }

    /// Internet delen live aan- of uitzetten op een draaiende hotspot: alleen de NAT-regel
    /// wisselt, de DNS-servers deelt de DHCP-server toch al uit. Zonder hotspot is het
    /// een no-op; de instelling geldt dan bij de volgende start.
    pub fn set_internet(&self, enabled: bool) -> Result<(), String> {
        let mut guard = self.info.lock().map_err(|e| format!("lock: {e}"))?;
        let Some(info) = guard.as_mut() else {
            return Ok(());
        };
        if enabled && !info.internet {
            enable_nat(&info.server_ip, &info.subnet)?;
        } else if !enabled && info.internet {
            nat::disable();
        }
        info.internet = enabled;
        Ok(())
    }

    pub fn stop(&self) {
        let Some(info) = self.info.lock().ok().and_then(|mut g| g.take()) else {
            return;
        };
        // NAT en het server-IP weer vrijgeven vóór de adapter verdwijnt: Windows bewaart de
        // configuratie van een verdwenen virtuele adapter, en een volgende start op een
        // andere kaart zou dan op "het object bestaat al" stuklopen.
        if info.internet {
            nat::disable();
        }
        let _ = ip::set_dhcp(&info.adapter);
        self.stop_method(info.method);
    }

    fn start_method(
        &self,
        ssid: &str,
        passphrase: &str,
        method: HotspotMethod,
    ) -> Result<String, String> {
        match method {
            HotspotMethod::WifiDirect => self.start_wifi_direct(ssid, passphrase),
            HotspotMethod::HostedNetwork => start_hosted(ssid, passphrase),
        }
    }

    fn stop_method(&self, method: HotspotMethod) {
        match method {
            HotspotMethod::WifiDirect => {
                let (reply, rx) = mpsc::channel();
                if self.sender().send(Cmd::Stop { reply }).is_ok() {
                    let _ = rx.recv_timeout(Duration::from_secs(5));
                }
            }
            HotspotMethod::HostedNetwork => stop_hosted(),
        }
    }

    /// Wi-Fi Direct draait op de worker-thread: stuur het commando en wacht op de adapter.
    fn start_wifi_direct(&self, ssid: &str, passphrase: &str) -> Result<String, String> {
        let (reply, rx) = mpsc::channel();
        self.sender()
            .send(Cmd::Start {
                ssid: ssid.to_string(),
                passphrase: passphrase.to_string(),
                reply,
            })
            .map_err(|_| "hotspot-worker niet beschikbaar".to_string())?;
        rx.recv()
            .map_err(|_| "hotspot-worker gaf geen antwoord".to_string())?
    }

    fn sender(&self) -> Sender<Cmd> {
        let mut guard = self.tx.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(tx) = guard.as_ref() {
            return tx.clone();
        }
        let (tx, rx) = mpsc::channel::<Cmd>();
        let _ = std::thread::Builder::new()
            .name("t8-hotspot".into())
            .spawn(move || worker(rx));
        *guard = Some(tx.clone());
        tx
    }
}

impl Default for Hotspot {
    fn default() -> Self {
        Self::new()
    }
}

/// De strings zijn al door `ip::claim_static_no_gateway` (netsh) geaccepteerd; hier alleen
/// nog omzetten voor de subnetberekening van de NAT-regel.
fn enable_nat(server_ip: &str, subnet: &str) -> Result<(), String> {
    let server: std::net::Ipv4Addr = server_ip
        .parse()
        .map_err(|_| format!("ongeldig server-IP '{server_ip}'"))?;
    let mask: std::net::Ipv4Addr = subnet
        .parse()
        .map_err(|_| format!("ongeldig subnetmasker '{subnet}'"))?;
    nat::enable(server, mask)
}

/// Volgorde van technieken. Hosted network landt gegarandeerd op een kaart die het kan, dus
/// die gaat voor zodra er echt iets te kiezen valt. Met één kaart, of zonder keuze, is
/// Wi-Fi Direct de moderne eerste keus en hosted network de uitwijk.
fn plan_for(target: Option<&AdapterInfo>) -> Vec<HotspotMethod> {
    use HotspotMethod::*;
    let Some(t) = target else {
        return vec![WifiDirect, HostedNetwork];
    };
    let hosted = hosted_capable(t, &wifi::hosted_network_capable());
    if hosted && physical_cards().len() > 1 {
        vec![HostedNetwork, WifiDirect]
    } else if hosted {
        vec![WifiDirect, HostedNetwork]
    } else {
        vec![WifiDirect]
    }
}

fn physical_cards() -> Vec<AdapterInfo> {
    adapter::list_adapters()
        .unwrap_or_default()
        .into_iter()
        .filter(is_physical_wifi)
        .collect()
}

/// Alleen echte kaarten. Virtuele adapters (Wi-Fi Direct, hosted network, de verborgen
/// HBS-tweelingen van een Qualcomm) hebben een lokaal toegekend MAC-adres (bit 0x02 van het
/// eerste byte) of "Virtual" in de omschrijving.
fn is_physical_wifi(a: &AdapterInfo) -> bool {
    a.kind == AdapterKind::WiFi
        && !a.description.to_ascii_lowercase().contains("virtual")
        && mac_bytes(&a.mac).first().is_some_and(|b| b & 0x02 == 0)
}

/// De virtuele adapter van een toegangspunt erft bijna het hele MAC-adres van de fysieke
/// kaart: Wi-Fi Direct wisselt het eerste byte (dc:56:7b:d9:fc:1b wordt 1e:56:7b:d9:fc:1b),
/// hosted network telt bij het laatste byte op. Vijf van de zes bytes gelijk is dus
/// "dezelfde kaart". Tweede kenmerk: de omschrijving zonder " #n", want een Qualcomm noemt
/// zijn virtuele adapter precies zoals de fysieke.
fn same_card(virtual_adapter: &AdapterInfo, card: &AdapterInfo) -> bool {
    let (a, b) = (mac_bytes(&virtual_adapter.mac), mac_bytes(&card.mac));
    let mac_match =
        a.len() == 6 && b.len() == 6 && a.iter().zip(&b).filter(|(x, y)| x == y).count() >= 5;
    mac_match || base_description(&virtual_adapter.description) == base_description(&card.description)
}

fn mac_bytes(mac: &str) -> Vec<u8> {
    mac.split(':')
        .filter_map(|h| u8::from_str_radix(h, 16).ok())
        .collect()
}

fn base_description(d: &str) -> &str {
    match d.rsplit_once(" #") {
        Some((base, n)) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => base,
        _ => d,
    }
}

fn hosted_capable(a: &AdapterInfo, capable: &[GUID]) -> bool {
    luid_to_guid(a.luid).is_some_and(|g| capable.contains(&g))
}

/// De WLAN-API spreekt in interface-GUID's, `GetAdaptersAddresses` in LUID's.
fn luid_to_guid(luid: u64) -> Option<GUID> {
    let net_luid = NET_LUID_LH { Value: luid };
    let mut guid = GUID::zeroed();
    (unsafe { ConvertInterfaceLuidToGuid(&net_luid, &mut guid) } == NO_ERROR).then_some(guid)
}

fn worker(rx: mpsc::Receiver<Cmd>) {
    // SAFETY: eenmalige WinRT-initialisatie op deze thread; nooit ge-uninitialize-d
    // omdat de thread zo lang leeft als het proces.
    let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
    let mut publisher: Option<WiFiDirectAdvertisementPublisher> = None;
    for cmd in rx {
        match cmd {
            Cmd::Start {
                ssid,
                passphrase,
                reply,
            } => {
                if let Some(p) = publisher.take() {
                    let _ = p.Stop();
                }
                let result = start_publisher(&ssid, &passphrase);
                let _ = match result {
                    Ok((p, adapter_name)) => {
                        publisher = Some(p);
                        reply.send(Ok(adapter_name))
                    }
                    Err(e) => reply.send(Err(e)),
                };
            }
            Cmd::Stop { reply } => {
                if let Some(p) = publisher.take() {
                    let _ = p.Stop();
                }
                let _ = reply.send(());
            }
        }
    }
}

fn start_publisher(
    ssid: &str,
    passphrase: &str,
) -> Result<(WiFiDirectAdvertisementPublisher, String), String> {
    let up_before = up_luids();

    let publisher = WiFiDirectAdvertisementPublisher::new().map_err(winrt_err)?;
    let adv = publisher.Advertisement().map_err(winrt_err)?;
    adv.SetIsAutonomousGroupOwnerEnabled(true).map_err(winrt_err)?;
    let legacy = adv.LegacySettings().map_err(winrt_err)?;
    legacy.SetIsEnabled(true).map_err(winrt_err)?;
    legacy.SetSsid(&HSTRING::from(ssid)).map_err(winrt_err)?;
    let cred = PasswordCredential::new().map_err(winrt_err)?;
    cred.SetPassword(&HSTRING::from(passphrase)).map_err(winrt_err)?;
    legacy.SetPassphrase(&cred).map_err(winrt_err)?;

    // De foutreden (radio uit, adapter bezet) komt alleen via het StatusChanged-event.
    let last_error: Arc<Mutex<Option<WiFiDirectError>>> = Arc::new(Mutex::new(None));
    {
        let last_error = Arc::clone(&last_error);
        let handler = TypedEventHandler::<
            WiFiDirectAdvertisementPublisher,
            WiFiDirectAdvertisementPublisherStatusChangedEventArgs,
        >::new(move |_, args| {
            if let Some(args) = args.as_ref() {
                if let Ok(err) = args.Error() {
                    if let Ok(mut g) = last_error.lock() {
                        *g = Some(err);
                    }
                }
            }
            Ok(())
        });
        let _ = publisher.StatusChanged(&handler);
    }

    publisher.Start().map_err(winrt_err)?;

    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        match publisher.Status().map_err(winrt_err)? {
            PubStatus::Started => break,
            PubStatus::Aborted => {
                let reason = last_error.lock().ok().and_then(|g| *g);
                return Err(match reason {
                    Some(WiFiDirectError::RadioNotAvailable) => {
                        "WiFi-radio staat uit of de adapter ondersteunt geen hotspot".into()
                    }
                    Some(WiFiDirectError::ResourceInUse) => {
                        "WiFi-adapter is bezet (andere hotspot of Wi-Fi Direct-sessie)".into()
                    }
                    _ => "hotspot kon niet starten (Wi-Fi Direct niet ondersteund?)".into(),
                });
            }
            _ => {
                if Instant::now() > deadline {
                    let _ = publisher.Stop();
                    return Err("hotspot start niet (timeout)".into());
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }

    match wait_for_new_wifi_adapter(&up_before, "wi-fi direct") {
        Ok(name) => Ok((publisher, name)),
        Err(e) => {
            let _ = publisher.Stop();
            Err(e)
        }
    }
}

/// Hosted network via netsh. Vereist administrator, wat de app toch al is.
fn start_hosted(ssid: &str, passphrase: &str) -> Result<String, String> {
    // Eerst elk draaiend hosted network weg: na een crash of hard afsluiten loopt het
    // door, en `start` op een lopend netwerk levert geen nieuwe adapter op, waardoor de
    // nulmeting hieronder hem nooit zou zien.
    stop_hosted();
    let up_before = up_luids();
    let ssid_arg = format!("ssid={ssid}");
    let key_arg = format!("key={passphrase}");
    netsh::run(&["wlan", "set", "hostednetwork", "mode=allow", &ssid_arg, &key_arg])
        .map_err(|e| format!("hosted network instellen mislukt: {e}"))?;
    if let Err(e) = netsh::run(&["wlan", "start", "hostednetwork"]) {
        stop_hosted();
        return Err(format!(
            "hosted network start niet: {e}. Geen WiFi-kaart ondersteunt dit, of de kaart staat uit"
        ));
    }
    wait_for_new_wifi_adapter(&up_before, "hosted network").inspect_err(|_| stop_hosted())
}

/// Stoppen en weer op `disallow`, anders blijft de hosted-network-instelling in Windows
/// op "aan" staan nadat T8-Lan klaar is.
fn stop_hosted() {
    let _ = netsh::run(&["wlan", "stop", "hostednetwork"]);
    let _ = netsh::run(&["wlan", "set", "hostednetwork", "mode=disallow"]);
}

/// LUID's van alle adapters die nu UP zijn: de nulmeting voor `wait_for_new_wifi_adapter`.
fn up_luids() -> HashSet<u64> {
    adapter::list_adapters()
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a.is_up)
        .map(|a| a.luid)
        .collect()
}

/// Wacht tot er een WiFi-adapter UP komt die bij de nulmeting nog niet bestond: dat is de
/// virtuele adapter van het toegangspunt.
///
/// Zoek NIET alleen op de omschrijving ("Wi-Fi Direct", "Hosted Network"): lang niet elke
/// chip noemt hem zo. Op een Qualcomm FastConnect 7800 erft de virtuele adapter letterlijk
/// de naam van de fysieke kaart, waardoor de hotspot wel startte maar hier werd afgebroken.
/// Soort (WiFi) plus "was er nog niet" klopt overal; `prefer` geeft alleen de doorslag als
/// er meer dan een kandidaat tegelijk opkomt.
fn wait_for_new_wifi_adapter(up_before: &HashSet<u64>, prefer: &str) -> Result<String, String> {
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        if let Ok(list) = adapter::list_adapters() {
            let fresh: Vec<&AdapterInfo> = list
                .iter()
                .filter(|a| {
                    a.is_up && !up_before.contains(&a.luid) && a.kind == AdapterKind::WiFi
                })
                .collect();
            let pick = fresh
                .iter()
                .copied()
                .find(|a| a.description.to_ascii_lowercase().contains(prefer))
                .or_else(|| fresh.first().copied());
            if let Some(a) = pick {
                return Ok(a.friendly_name.clone());
            }
        }
        if Instant::now() > deadline {
            return Err("virtuele hotspot-adapter niet gevonden".into());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn winrt_err(e: windows::core::Error) -> String {
    format!("Wi-Fi Direct: {}", e.message())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(name: &str, desc: &str, mac: &str) -> AdapterInfo {
        AdapterInfo {
            friendly_name: name.into(),
            description: desc.into(),
            luid: 0,
            mac: mac.into(),
            kind: AdapterKind::WiFi,
            is_up: true,
            is_dhcp: false,
            current_ip: None,
            current_subnet: None,
            current_gateway: None,
            dns_servers: vec![],
        }
    }

    const IP: &str = "192.168.8.8";
    const MASK: &str = "255.255.255.0";

    #[test]
    fn validates_ssid_and_passphrase() {
        let h = Hotspot::new();
        assert!(h.start("", "T8online", None, IP, MASK, false).is_err());
        assert!(h.start("T8-Lan", "kort", None, IP, MASK, false).is_err());
        assert!(h.start(&"x".repeat(33), "T8online", None, IP, MASK, false).is_err());
        assert!(h.start("T8-Lan", "wachtwoord-é", None, IP, MASK, false).is_err());
    }

    /// Gemeten op een pc met een Qualcomm FastConnect 7800 (ingebouwd) en een UGREEN
    /// USB-stick, 26-10-2026.
    #[test]
    fn only_physical_cards_are_listed() {
        assert!(is_physical_wifi(&fake("Wi-Fi", "Qualcomm FastConnect 7800", "dc:56:7b:d9:fc:1b")));
        assert!(is_physical_wifi(&fake("Wi-Fi 6", "UGREEN Wi-Fi 6 USB Adapter", "6c:1f:f7:91:03:46")));
        // Wi-Fi Direct-adapter: lokaal toegekend MAC, zelfde naam als de kaart.
        assert!(!is_physical_wifi(&fake("Wi-Fi 5", "Qualcomm FastConnect 7800 #5", "1e:56:7b:d9:fc:1b")));
        // HBS-tweeling van de Qualcomm: ook een lokaal MAC.
        assert!(!is_physical_wifi(&fake("Wi-Fi 3", "Qualcomm FastConnect 7800 #3", "fe:56:7b:d9:fc:1b")));
        // Hosted network: globaal ogend MAC, maar "Virtual" in de naam.
        assert!(!is_physical_wifi(&fake(
            "LAN-verbinding* 9",
            "Microsoft Hosted Network Virtual Adapter",
            "6c:1f:f7:91:03:47"
        )));
    }

    #[test]
    fn virtual_adapter_is_matched_to_its_card() {
        let qualcomm = fake("Wi-Fi", "Qualcomm FastConnect 7800 Network Adapter", "dc:56:7b:d9:fc:1b");
        let ugreen = fake("Wi-Fi 6", "UGREEN Wi-Fi 6 USB Adapter", "6c:1f:f7:91:03:46");
        let wfd = fake("Wi-Fi 5", "Qualcomm FastConnect 7800 Network Adapter #5", "1e:56:7b:d9:fc:1b");
        let hosted = fake("LAN-verbinding* 9", "Microsoft Hosted Network Virtual Adapter", "6c:1f:f7:91:03:47");
        assert!(same_card(&wfd, &qualcomm));
        assert!(!same_card(&wfd, &ugreen));
        assert!(same_card(&hosted, &ugreen));
        assert!(!same_card(&hosted, &qualcomm));
    }

    /// Toont de kaartenlijst zoals de DHCP-tab die krijgt, met de hosted-network-vlag van de
    /// driver: `cargo test --lib cards_live -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn cards_live() {
        for c in cards() {
            println!("{:<12} hosted={:<5} {}", c.friendly_name, c.hosted_capable, c.description);
        }
    }

    /// Live-test op een machine met WiFi, als administrator (zet een IP via netsh):
    /// `cargo test --lib hotspot_live -- --ignored --nocapture`.
    /// Zet `T8_HOTSPOT_CARD` op een friendly name om een kaart te kiezen, en
    /// `T8_HOTSPOT_INTERNET=1` om ook de NAT-regel te zetten.
    #[test]
    #[ignore]
    fn hotspot_live() {
        let card = std::env::var("T8_HOTSPOT_CARD").ok();
        let internet = std::env::var("T8_HOTSPOT_INTERNET").is_ok_and(|v| v == "1");
        let h = Hotspot::new();
        let info = h
            .start("T8-Lan-test", "T8online", card.as_deref(), IP, MASK, internet)
            .expect("hotspot start");
        println!("hotspot actief: {info:?}");
        std::thread::sleep(Duration::from_secs(8));
        h.stop();
        assert!(!h.is_running());
    }
}
