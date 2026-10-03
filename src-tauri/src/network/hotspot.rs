//! WiFi-hotspot via Wi-Fi Direct: een "autonome group owner" met legacy-instellingen
//! (gewone SSID + WPA2-wachtwoord), zodat ook camera's en telefoons zonder Wi-Fi Direct
//! kunnen verbinden.
//!
//! Waarom niet de Windows "Mobiele hotspot": die hangt aan Internet Connection Sharing,
//! met een vast 192.168.137.1 en een eigen DHCP-server, en eist een te delen verbinding.
//! Deze route is een kaal toegangspunt zonder IP-laag: T8-Lan zet daarna zelf het
//! server-IP (standaard 192.168.8.8) op de virtuele adapter en draait er de eigen
//! DHCP-server op, met dezelfde instellingen als bij een directe kabel.
//!
//! Werkt op elke adapter die Wi-Fi Direct ondersteunt (vrijwel alle laptops sinds
//! Windows 10). Het oude `netsh wlan hostednetwork` is bewust niet gebruikt: dat is
//! afgeschaft en werkt op moderne chips niet meer.
//!
//! WinRT-objecten leven op een eigen worker-thread (MTA) die zo lang bestaat als het
//! proces; commando's gaan via een channel.

use crate::network::adapter;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::core::HSTRING;
use windows::Devices::WiFiDirect::{
    WiFiDirectAdvertisementPublisher, WiFiDirectAdvertisementPublisherStatus as PubStatus,
    WiFiDirectAdvertisementPublisherStatusChangedEventArgs, WiFiDirectError,
};
use windows::Foundation::TypedEventHandler;
use windows::Security::Credentials::PasswordCredential;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

/// Hoe lang we wachten tot de hotspot "Started" meldt en de virtuele adapter omhoog is.
const START_TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Debug, Clone, Serialize)]
pub struct HotspotInfo {
    pub ssid: String,
    /// Friendly name van de virtuele Wi-Fi Direct-adapter waarop de hotspot draait.
    pub adapter: String,
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

    /// Start de hotspot en geef de adapter terug waarop hij draait.
    pub fn start(&self, ssid: &str, passphrase: &str) -> Result<HotspotInfo, String> {
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
        let (reply, rx) = mpsc::channel();
        self.sender()
            .send(Cmd::Start {
                ssid: ssid.to_string(),
                passphrase: passphrase.to_string(),
                reply,
            })
            .map_err(|_| "hotspot-worker niet beschikbaar".to_string())?;
        let adapter = rx
            .recv()
            .map_err(|_| "hotspot-worker gaf geen antwoord".to_string())??;
        let info = HotspotInfo {
            ssid: ssid.to_string(),
            adapter,
        };
        if let Ok(mut g) = self.info.lock() {
            *g = Some(info.clone());
        }
        Ok(info)
    }

    pub fn stop(&self) {
        let had = self.info.lock().map(|mut g| g.take().is_some()).unwrap_or(false);
        if !had {
            return;
        }
        let (reply, rx) = mpsc::channel();
        if self.sender().send(Cmd::Stop { reply }).is_ok() {
            let _ = rx.recv_timeout(Duration::from_secs(5));
        }
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
    let up_before: HashSet<u64> = adapter::list_adapters()
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a.is_up)
        .map(|a| a.luid)
        .collect();

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

    // De virtuele Wi-Fi Direct-adapter komt nu omhoog; dat is de adapter die net UP werd
    // en niet in de lijst van vóór de start zat.
    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        if let Ok(list) = adapter::list_adapters() {
            if let Some(a) = list.iter().find(|a| {
                a.is_up
                    && !up_before.contains(&a.luid)
                    && a.description.to_ascii_lowercase().contains("wi-fi direct")
            }) {
                return Ok((publisher, a.friendly_name.clone()));
            }
        }
        if Instant::now() > deadline {
            let _ = publisher.Stop();
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

    #[test]
    fn validates_ssid_and_passphrase() {
        let h = Hotspot::new();
        assert!(h.start("", "turn8-lan").is_err());
        assert!(h.start("T8-Lan", "kort").is_err());
        assert!(h.start(&"x".repeat(33), "turn8-lan").is_err());
        assert!(h.start("T8-Lan", "wachtwoord-é").is_err());
    }

    /// Live-test op een machine met WiFi: `cargo test --lib hotspot_live -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn hotspot_live() {
        let h = Hotspot::new();
        let info = h.start("T8-Lan-test", "turn8-lan").expect("hotspot start");
        println!("hotspot actief: {info:?}");
        std::thread::sleep(Duration::from_secs(8));
        h.stop();
        assert!(!h.is_running());
    }
}
