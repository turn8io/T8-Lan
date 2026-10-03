//! Ingebouwde DHCPv4-server (RFC 2131) voor veldgebruik.
//!
//! Scenario: een netwerk zonder DHCP-server, of een apparaat op DHCP dat met een directe
//! (cross)kabel aan de laptop hangt. T8-Lan zet de gekozen adapter op een statisch IP
//! (standaard 192.168.8.8/24, zonder gateway) en deelt adressen uit een kleine pool
//! (standaard 192.168.8.100 t/m .149).
//!
//! Bewust beperkt tot één adapter: de socket bindt op het IP van die adapter, dus
//! DISCOVER-broadcasts via WiFi of een andere kaart worden nooit beantwoord. Zolang de
//! kabel niet is aangesloten heeft Windows dat IP nog niet actief en faalt de bind; de
//! serverthread probeert het dan elke seconde opnieuw ("wacht op link").
//!
//! Bij stoppen (of afsluiten van de app) krijgt de adapter zijn vorige configuratie terug.

use crate::switching::{self, PreviousConfig};
use crate::util::now_ms;
use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// Event naar de UI met de volledige [`DhcpStatus`] bij elke wijziging.
pub const STATUS_EVENT: &str = "dhcp-status";

const SERVER_PORT: u16 = 67;
const CLIENT_PORT: u16 = 68;
const MAGIC_COOKIE: [u8; 4] = [99, 130, 83, 99];
/// Minimale BOOTP-pakketlengte; oudere clients negeren kortere antwoorden.
const MIN_PACKET: usize = 300;
/// Hoe lang een OFFER gereserveerd blijft als de client nooit een REQUEST stuurt.
const OFFER_HOLD_MS: i64 = 60_000;
/// Hoe lang een door een client geweigerd (DECLINE) adres buiten gebruik blijft.
const DECLINE_HOLD_MS: i64 = 10 * 60_000;
/// Read-timeout van de socket: bepaalt hoe snel een stop-verzoek doorkomt.
const RECV_TIMEOUT: Duration = Duration::from_millis(300);
/// Wachttijd tussen bind-pogingen zolang de adapter nog geen link/IP heeft.
const REBIND_WAIT: Duration = Duration::from_millis(1000);

const DHCPDISCOVER: u8 = 1;
const DHCPOFFER: u8 = 2;
const DHCPREQUEST: u8 = 3;
const DHCPDECLINE: u8 = 4;
const DHCPACK: u8 = 5;
const DHCPNAK: u8 = 6;
const DHCPRELEASE: u8 = 7;
const DHCPINFORM: u8 = 8;

const OPT_SUBNET_MASK: u8 = 1;
const OPT_ROUTER: u8 = 3;
const OPT_DNS: u8 = 6;
const OPT_HOSTNAME: u8 = 12;
const OPT_BROADCAST: u8 = 28;
const OPT_REQUESTED_IP: u8 = 50;
const OPT_LEASE_TIME: u8 = 51;
const OPT_MSG_TYPE: u8 = 53;
const OPT_SERVER_ID: u8 = 54;
const OPT_RENEWAL_T1: u8 = 58;
const OPT_REBINDING_T2: u8 = 59;
const OPT_VENDOR_CLASS: u8 = 60;
const OPT_END: u8 = 255;
const OPT_PAD: u8 = 0;

/// Door de UI meegegeven configuratie (zelfde velden als `settings.dhcp`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DhcpConfig {
    pub server_ip: String,
    pub subnet: String,
    pub pool_start: String,
    pub pool_size: u32,
    pub lease_secs: u32,
}

/// Gevalideerde pool.
#[derive(Debug, Clone, Copy)]
struct Pool {
    server: Ipv4Addr,
    mask: Ipv4Addr,
    first: u32,
    last: u32,
    lease_secs: u32,
}

impl Pool {
    fn parse(cfg: &DhcpConfig) -> Result<Self, String> {
        let server: Ipv4Addr = cfg
            .server_ip
            .trim()
            .parse()
            .map_err(|_| format!("ongeldig server-IP '{}'", cfg.server_ip))?;
        let mask: Ipv4Addr = cfg
            .subnet
            .trim()
            .parse()
            .map_err(|_| format!("ongeldig subnetmasker '{}'", cfg.subnet))?;
        let start: Ipv4Addr = cfg
            .pool_start
            .trim()
            .parse()
            .map_err(|_| format!("ongeldig pool-startadres '{}'", cfg.pool_start))?;
        if !(1..=1000).contains(&cfg.pool_size) {
            return Err("scope moet tussen 1 en 1000 adressen liggen".into());
        }
        if !(60..=7 * 24 * 3600).contains(&cfg.lease_secs) {
            return Err("lease-tijd moet tussen 60 s en 7 dagen liggen".into());
        }
        let mask_u = u32::from(mask);
        if mask_u == 0 || (!mask_u).wrapping_add(1) & !mask_u != 0 {
            return Err("subnetmasker is geen aaneengesloten masker".into());
        }
        let network = u32::from(server) & mask_u;
        let broadcast = network | !mask_u;
        let first = u32::from(start);
        let last = first
            .checked_add(cfg.pool_size - 1)
            .ok_or("pool loopt buiten het adresbereik")?;
        if first & mask_u != network || last & mask_u != network {
            return Err("pool valt buiten het subnet van het server-IP".into());
        }
        if first <= network || last >= broadcast {
            return Err("pool mag het netwerk- en broadcastadres niet bevatten".into());
        }
        let server_u = u32::from(server);
        if (first..=last).contains(&server_u) {
            return Err("server-IP mag niet in de pool liggen".into());
        }
        if server_u == network || server_u == broadcast {
            return Err("server-IP mag geen netwerk- of broadcastadres zijn".into());
        }
        Ok(Self {
            server,
            mask,
            first,
            last,
            lease_secs: cfg.lease_secs,
        })
    }

    fn contains(&self, ip: Ipv4Addr) -> bool {
        (self.first..=self.last).contains(&u32::from(ip))
    }

    fn broadcast(&self) -> Ipv4Addr {
        Ipv4Addr::from(u32::from(self.server) | !u32::from(self.mask))
    }

    fn label(&self) -> String {
        format!("{} - {}", Ipv4Addr::from(self.first), Ipv4Addr::from(self.last))
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LeaseState {
    Offered,
    Bound,
}

#[derive(Debug, Clone, Serialize)]
pub struct Lease {
    pub ip: String,
    pub mac: String,
    pub hostname: Option<String>,
    pub vendor: Option<String>,
    pub state: LeaseState,
    pub since_ms: i64,
    pub expires_ms: i64,
}

/// Toestand zoals de UI die toont.
#[derive(Debug, Clone, Serialize, Default)]
pub struct DhcpStatus {
    pub running: bool,
    pub adapter: Option<String>,
    pub server_ip: Option<String>,
    pub pool: Option<String>,
    /// `true` zodra de socket op het server-IP gebonden is (adapter heeft link + IP).
    pub listening: bool,
    pub leases: Vec<Lease>,
    pub error: Option<String>,
    /// Gevuld als de server op een eigen WiFi-hotspot draait.
    pub hotspot: Option<crate::network::hotspot::HotspotInfo>,
}

/// Door de serverthread en de IPC-laag gedeelde toestand.
struct Shared {
    leases: Vec<Lease>,
    /// Door clients geweigerde adressen (DECLINE) → tot wanneer ze buiten gebruik blijven.
    declined: HashMap<u32, i64>,
    listening: bool,
    error: Option<String>,
}

struct Running {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    shared: Arc<Mutex<Shared>>,
    adapter: String,
    restore: PreviousConfig,
    pool: Pool,
    hotspot: Option<crate::network::hotspot::HotspotInfo>,
}

/// Door Tauri beheerde servertoestand (één server tegelijk).
pub struct DhcpServer {
    inner: Mutex<Option<Running>>,
}

impl DhcpServer {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    pub fn is_running(&self) -> bool {
        self.inner.lock().map(|g| g.is_some()).unwrap_or(false)
    }

    pub fn status(&self) -> DhcpStatus {
        let guard = match self.inner.lock() {
            Ok(g) => g,
            Err(_) => return DhcpStatus::default(),
        };
        let Some(run) = guard.as_ref() else {
            return DhcpStatus::default();
        };
        let shared = run.shared.lock();
        let (leases, listening, error) = match shared {
            Ok(s) => (s.leases.clone(), s.listening, s.error.clone()),
            Err(_) => (Vec::new(), false, None),
        };
        DhcpStatus {
            running: true,
            adapter: Some(run.adapter.clone()),
            server_ip: Some(run.pool.server.to_string()),
            pool: Some(run.pool.label()),
            listening,
            leases,
            error,
            hotspot: run.hotspot.clone(),
        }
    }

    /// Start de server op `adapter_name`: zet de adapter op het server-IP (zonder
    /// gateway), onthoudt de vorige configuratie en start de luisterthread. `hotspot`
    /// is informatief (zichtbaar in de status) als de adapter een eigen hotspot is.
    pub fn start(
        &self,
        app: &AppHandle,
        adapter_name: &str,
        cfg: &DhcpConfig,
        hotspot: Option<crate::network::hotspot::HotspotInfo>,
    ) -> Result<(), String> {
        let pool = Pool::parse(cfg)?;
        let mut guard = self.inner.lock().map_err(|e| format!("lock: {e}"))?;
        if guard.is_some() {
            return Err("DHCP-server draait al".into());
        }
        let restore = switching::capture_current(adapter_name)
            .ok_or_else(|| format!("adapter '{adapter_name}' niet gevonden"))?;

        let already_set = !restore.was_dhcp
            && restore.ip.as_deref() == Some(&pool.server.to_string())
            && restore.subnet.as_deref() == Some(&pool.mask.to_string())
            && restore.gateway.is_none();
        if !already_set {
            crate::network::ip::set_static_no_gateway(
                adapter_name,
                &pool.server.to_string(),
                &pool.mask.to_string(),
            )?;
        }

        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(Shared {
            leases: Vec::new(),
            declined: HashMap::new(),
            listening: false,
            error: None,
        }));
        let thread = {
            let app = app.clone();
            let stop = Arc::clone(&stop);
            let shared = Arc::clone(&shared);
            let hotspot = hotspot.clone();
            std::thread::Builder::new()
                .name("t8-dhcp".into())
                .spawn(move || serve(app, pool, shared, stop, hotspot))
                .map_err(|e| format!("serverthread starten: {e}"))?
        };
        *guard = Some(Running {
            stop,
            thread: Some(thread),
            shared,
            adapter: adapter_name.to_string(),
            restore,
            pool,
            hotspot,
        });
        drop(guard);
        emit_status(app, self.status());
        Ok(())
    }

    /// Stop de server en zet de adapter terug op de vorige configuratie.
    pub fn stop(&self, app: &AppHandle) -> Result<(), String> {
        let run = {
            let mut guard = self.inner.lock().map_err(|e| format!("lock: {e}"))?;
            guard.take()
        };
        let Some(mut run) = run else {
            return Ok(());
        };
        run.stop.store(true, Ordering::SeqCst);
        if let Some(t) = run.thread.take() {
            let _ = t.join();
        }
        let result = switching::apply_config(app, &run.restore);
        emit_status(app, DhcpStatus::default());
        result
    }
}

impl Default for DhcpServer {
    fn default() -> Self {
        Self::new()
    }
}

fn emit_status(app: &AppHandle, status: DhcpStatus) {
    let _ = app.emit(STATUS_EVENT, status);
}

fn status_snapshot(
    pool: &Pool,
    shared: &Arc<Mutex<Shared>>,
    adapter: Option<String>,
    hotspot: &Option<crate::network::hotspot::HotspotInfo>,
) -> DhcpStatus {
    let (leases, listening, error) = match shared.lock() {
        Ok(s) => (s.leases.clone(), s.listening, s.error.clone()),
        Err(_) => (Vec::new(), false, None),
    };
    DhcpStatus {
        running: true,
        adapter,
        server_ip: Some(pool.server.to_string()),
        pool: Some(pool.label()),
        listening,
        leases,
        error,
        hotspot: hotspot.clone(),
    }
}

// ---- Serverthread ----

fn serve(
    app: AppHandle,
    pool: Pool,
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    hotspot: Option<crate::network::hotspot::HotspotInfo>,
) {
    let mut buf = [0u8; 1500];
    let mut last_error: Option<String> = None;

    'outer: while !stop.load(Ordering::SeqCst) {
        let socket = match bind(pool.server) {
            Ok(s) => s,
            Err(e) => {
                if last_error.as_deref() != Some(&e) {
                    last_error = Some(e.clone());
                    if let Ok(mut s) = shared.lock() {
                        s.listening = false;
                        s.error = Some(e);
                    }
                    emit_status(&app, status_snapshot(&pool, &shared, None, &hotspot));
                }
                // Wachten in kleine stapjes zodat een stop snel doorkomt.
                let waited = std::time::Instant::now();
                while waited.elapsed() < REBIND_WAIT {
                    if stop.load(Ordering::SeqCst) {
                        break 'outer;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                continue;
            }
        };
        last_error = None;
        if let Ok(mut s) = shared.lock() {
            s.listening = true;
            s.error = None;
        }
        emit_status(&app, status_snapshot(&pool, &shared, None, &hotspot));

        loop {
            if stop.load(Ordering::SeqCst) {
                break 'outer;
            }
            match socket.recv_from(&mut buf) {
                Ok((n, from)) => {
                    let Some(req) = Packet::parse(&buf[..n]) else {
                        continue;
                    };
                    let reply = {
                        let Ok(mut s) = shared.lock() else { continue };
                        handle(&req, &pool, &mut s, now_ms())
                    };
                    if let Some((msg_type, bytes)) = reply {
                        let dest = reply_dest(&req, msg_type, from);
                        let _ = socket.send_to(&bytes, dest);
                    }
                    emit_status(&app, status_snapshot(&pool, &shared, None, &hotspot));
                }
                Err(e) => match e.kind() {
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {}
                    // Windows meldt een eerder ICMP "port unreachable" als ConnectionReset
                    // op een UDP-socket; onschuldig, gewoon doorgaan.
                    std::io::ErrorKind::ConnectionReset => {}
                    // Andere fouten (bv. adapter weg/IP veranderd): opnieuw binden.
                    _ => break,
                },
            }
        }
    }

    if let Ok(mut s) = shared.lock() {
        s.listening = false;
    }
}

fn bind(server: Ipv4Addr) -> Result<UdpSocket, String> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
        .map_err(|e| format!("socket: {e}"))?;
    socket
        .set_broadcast(true)
        .map_err(|e| format!("broadcast: {e}"))?;
    let addr: SocketAddr = SocketAddrV4::new(server, SERVER_PORT).into();
    // Bewust géén SO_REUSEADDR: als poort 67 bezet is (Hyper-V, ICS, andere DHCP-server)
    // willen we dat weten, niet er stil naast gaan zitten.
    socket.bind(&addr.into()).map_err(|e| match e.raw_os_error() {
        Some(10048) => "poort 67 is al in gebruik (andere DHCP-server, ICS of Hyper-V?)".to_string(),
        Some(10049) => format!("wacht op link: {server} is nog niet actief op de adapter"),
        _ => format!("bind {server}:67: {e}"),
    })?;
    socket
        .set_read_timeout(Some(RECV_TIMEOUT))
        .map_err(|e| format!("read timeout: {e}"))?;
    Ok(socket.into())
}

/// Bestemming van een antwoord (RFC 2131 §4.1): via relay → giaddr:67; NAK altijd
/// broadcast; client met bekend IP (renew) → unicast; anders broadcast op :68.
fn reply_dest(req: &Packet, msg_type: u8, from: SocketAddr) -> SocketAddr {
    if !req.giaddr.is_unspecified() {
        return SocketAddrV4::new(req.giaddr, SERVER_PORT).into();
    }
    if msg_type == DHCPNAK {
        return SocketAddrV4::new(Ipv4Addr::BROADCAST, CLIENT_PORT).into();
    }
    if !req.ciaddr.is_unspecified() {
        return SocketAddrV4::new(req.ciaddr, CLIENT_PORT).into();
    }
    // Een INFORM komt unicast van een client die al een IP heeft: antwoord daarheen.
    if req.msg_type == Some(DHCPINFORM) {
        if let SocketAddr::V4(v4) = from {
            if !v4.ip().is_unspecified() {
                return SocketAddrV4::new(*v4.ip(), CLIENT_PORT).into();
            }
        }
    }
    SocketAddrV4::new(Ipv4Addr::BROADCAST, CLIENT_PORT).into()
}

// ---- Protocol ----

struct Packet<'a> {
    htype: u8,
    hlen: u8,
    xid: [u8; 4],
    flags: [u8; 2],
    ciaddr: Ipv4Addr,
    giaddr: Ipv4Addr,
    chaddr: [u8; 16],
    msg_type: Option<u8>,
    options: HashMap<u8, &'a [u8]>,
}

impl<'a> Packet<'a> {
    fn parse(buf: &'a [u8]) -> Option<Self> {
        if buf.len() < 240 || buf[0] != 1 || buf[236..240] != MAGIC_COOKIE {
            return None;
        }
        let mut options = HashMap::new();
        let mut i = 240;
        while i < buf.len() {
            let code = buf[i];
            if code == OPT_END {
                break;
            }
            if code == OPT_PAD {
                i += 1;
                continue;
            }
            let len = *buf.get(i + 1)? as usize;
            let data = buf.get(i + 2..i + 2 + len)?;
            // Eerste voorkomen wint (RFC 3396-concatenatie is hier niet nodig).
            options.entry(code).or_insert(data);
            i += 2 + len;
        }
        let msg_type = options.get(&OPT_MSG_TYPE).and_then(|v| v.first().copied());
        let mut chaddr = [0u8; 16];
        chaddr.copy_from_slice(&buf[28..44]);
        Some(Self {
            htype: buf[1],
            hlen: buf[2],
            xid: [buf[4], buf[5], buf[6], buf[7]],
            flags: [buf[10], buf[11]],
            ciaddr: Ipv4Addr::new(buf[12], buf[13], buf[14], buf[15]),
            giaddr: Ipv4Addr::new(buf[24], buf[25], buf[26], buf[27]),
            chaddr,
            msg_type,
            options,
        })
    }

    fn mac(&self) -> String {
        let len = (self.hlen as usize).clamp(1, 16);
        self.chaddr[..len]
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(":")
    }

    fn opt_ipv4(&self, code: u8) -> Option<Ipv4Addr> {
        self.options
            .get(&code)
            .filter(|v| v.len() == 4)
            .map(|v| Ipv4Addr::new(v[0], v[1], v[2], v[3]))
    }

    fn opt_string(&self, code: u8) -> Option<String> {
        self.options.get(&code).and_then(|v| {
            let s = String::from_utf8_lossy(v).trim_matches(char::from(0)).trim().to_string();
            (!s.is_empty()).then_some(s)
        })
    }
}

/// Verwerk één verzoek. Geeft `(berichttype, bytes)` van het antwoord, of `None` als we
/// niets sturen. Werkt op de gedeelde leasetabel.
fn handle(req: &Packet, pool: &Pool, s: &mut Shared, now: i64) -> Option<(u8, Vec<u8>)> {
    expire(s, now);
    let mac = req.mac();
    match req.msg_type? {
        DHCPDISCOVER => {
            let ip = choose_ip(s, pool, &mac, req.opt_ipv4(OPT_REQUESTED_IP), now)?;
            upsert_lease(s, &mac, ip, LeaseState::Offered, now + OFFER_HOLD_MS, req, now);
            Some((DHCPOFFER, build_reply(req, pool, DHCPOFFER, Some(ip))))
        }
        DHCPREQUEST => {
            // Client kiest een andere server: onze offer intrekken, niets sturen.
            if let Some(sid) = req.opt_ipv4(OPT_SERVER_ID) {
                if sid != pool.server {
                    s.leases
                        .retain(|l| !(l.mac == mac && l.state == LeaseState::Offered));
                    return None;
                }
            }
            let requested = req
                .opt_ipv4(OPT_REQUESTED_IP)
                .or_else(|| (!req.ciaddr.is_unspecified()).then_some(req.ciaddr))?;
            let free_for_mac = pool.contains(requested)
                && !s.declined.contains_key(&u32::from(requested))
                && !s
                    .leases
                    .iter()
                    .any(|l| l.ip == requested.to_string() && l.mac != mac);
            if !free_for_mac {
                s.leases.retain(|l| l.mac != mac);
                return Some((DHCPNAK, build_reply(req, pool, DHCPNAK, None)));
            }
            let expires = now + i64::from(pool.lease_secs) * 1000;
            upsert_lease(s, &mac, requested, LeaseState::Bound, expires, req, now);
            Some((DHCPACK, build_reply(req, pool, DHCPACK, Some(requested))))
        }
        DHCPRELEASE => {
            s.leases.retain(|l| l.mac != mac);
            None
        }
        DHCPDECLINE => {
            if let Some(ip) = req.opt_ipv4(OPT_REQUESTED_IP) {
                s.declined.insert(u32::from(ip), now + DECLINE_HOLD_MS);
            }
            s.leases.retain(|l| l.mac != mac);
            None
        }
        DHCPINFORM => Some((DHCPACK, build_reply(req, pool, DHCPACK, None))),
        _ => None,
    }
}

fn expire(s: &mut Shared, now: i64) {
    s.leases.retain(|l| l.expires_ms > now);
    s.declined.retain(|_, until| *until > now);
}

/// Kies een adres: eerst een bestaande lease van dit MAC, dan het gevraagde adres als
/// dat vrij is, anders het eerste vrije adres in de pool.
fn choose_ip(
    s: &Shared,
    pool: &Pool,
    mac: &str,
    requested: Option<Ipv4Addr>,
    now: i64,
) -> Option<Ipv4Addr> {
    if let Some(l) = s.leases.iter().find(|l| l.mac == mac) {
        if let Ok(ip) = l.ip.parse::<Ipv4Addr>() {
            if pool.contains(ip) {
                return Some(ip);
            }
        }
    }
    let is_free = |ip: Ipv4Addr| {
        pool.contains(ip)
            && !s.declined.get(&u32::from(ip)).is_some_and(|until| *until > now)
            && !s.leases.iter().any(|l| l.ip == ip.to_string())
    };
    if let Some(r) = requested.filter(|r| is_free(*r)) {
        return Some(r);
    }
    (pool.first..=pool.last)
        .map(Ipv4Addr::from)
        .find(|ip| is_free(*ip))
}

fn upsert_lease(
    s: &mut Shared,
    mac: &str,
    ip: Ipv4Addr,
    state: LeaseState,
    expires_ms: i64,
    req: &Packet,
    now: i64,
) {
    let hostname = req.opt_string(OPT_HOSTNAME);
    let vendor = req.opt_string(OPT_VENDOR_CLASS);
    // Eén lease per MAC; en het adres mag niet ook bij een ander MAC hangen.
    s.leases.retain(|l| l.mac != mac && l.ip != ip.to_string());
    s.leases.push(Lease {
        ip: ip.to_string(),
        mac: mac.to_string(),
        hostname,
        vendor,
        state,
        since_ms: now,
        expires_ms,
    });
    s.leases.sort_by_key(|l| l.ip.parse::<Ipv4Addr>().map(u32::from).unwrap_or(0));
}

fn build_reply(req: &Packet, pool: &Pool, msg_type: u8, yiaddr: Option<Ipv4Addr>) -> Vec<u8> {
    let mut p = Vec::with_capacity(MIN_PACKET + 64);
    p.push(2); // BOOTREPLY
    p.push(req.htype);
    p.push(req.hlen);
    p.push(0); // hops
    p.extend_from_slice(&req.xid);
    p.extend_from_slice(&[0, 0]); // secs
    p.extend_from_slice(&req.flags);
    // ciaddr: alleen terugzetten bij een INFORM/renew-ACK (client heeft dan al een IP).
    let ciaddr = if msg_type == DHCPACK && yiaddr.is_none() || msg_type == DHCPACK && !req.ciaddr.is_unspecified() {
        req.ciaddr
    } else {
        Ipv4Addr::UNSPECIFIED
    };
    p.extend_from_slice(&ciaddr.octets());
    p.extend_from_slice(&yiaddr.unwrap_or(Ipv4Addr::UNSPECIFIED).octets());
    p.extend_from_slice(&pool.server.octets()); // siaddr
    p.extend_from_slice(&req.giaddr.octets());
    p.extend_from_slice(&req.chaddr);
    p.extend_from_slice(&[0u8; 64]); // sname
    p.extend_from_slice(&[0u8; 128]); // file
    p.extend_from_slice(&MAGIC_COOKIE);

    push_opt(&mut p, OPT_MSG_TYPE, &[msg_type]);
    push_opt(&mut p, OPT_SERVER_ID, &pool.server.octets());
    if msg_type != DHCPNAK {
        if yiaddr.is_some() {
            let lease = pool.lease_secs;
            push_opt(&mut p, OPT_LEASE_TIME, &lease.to_be_bytes());
            push_opt(&mut p, OPT_RENEWAL_T1, &(lease / 2).to_be_bytes());
            push_opt(&mut p, OPT_REBINDING_T2, &(lease / 8 * 7).to_be_bytes());
        }
        push_opt(&mut p, OPT_SUBNET_MASK, &pool.mask.octets());
        push_opt(&mut p, OPT_ROUTER, &pool.server.octets());
        push_opt(&mut p, OPT_DNS, &pool.server.octets());
        push_opt(&mut p, OPT_BROADCAST, &pool.broadcast().octets());
    }
    p.push(OPT_END);
    if p.len() < MIN_PACKET {
        p.resize(MIN_PACKET, 0);
    }
    p
}

fn push_opt(p: &mut Vec<u8>, code: u8, data: &[u8]) {
    p.push(code);
    p.push(data.len() as u8);
    p.extend_from_slice(data);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> DhcpConfig {
        DhcpConfig {
            server_ip: "192.168.8.8".into(),
            subnet: "255.255.255.0".into(),
            pool_start: "192.168.8.100".into(),
            pool_size: 50,
            lease_secs: 3600,
        }
    }

    fn shared() -> Shared {
        Shared {
            leases: Vec::new(),
            declined: HashMap::new(),
            listening: true,
            error: None,
        }
    }

    /// Bouw een client-pakket met gegeven berichttype en opties.
    fn client_packet(msg_type: u8, mac: [u8; 6], ciaddr: Ipv4Addr, opts: &[(u8, &[u8])]) -> Vec<u8> {
        let mut p = vec![0u8; 240];
        p[0] = 1;
        p[1] = 1;
        p[2] = 6;
        p[4..8].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        p[12..16].copy_from_slice(&ciaddr.octets());
        p[28..34].copy_from_slice(&mac);
        p[236..240].copy_from_slice(&MAGIC_COOKIE);
        push_opt(&mut p, OPT_MSG_TYPE, &[msg_type]);
        for (code, data) in opts {
            push_opt(&mut p, *code, data);
        }
        p.push(OPT_END);
        p
    }

    fn opt_of(reply: &[u8], code: u8) -> Option<Vec<u8>> {
        let pk = Packet::parse(reply_as_request(reply))?;
        pk.options.get(&code).map(|v| v.to_vec())
    }

    /// Onze parser eist op=1; voor het inspecteren van een antwoord zetten we dat even om.
    fn reply_as_request(reply: &[u8]) -> &'static [u8] {
        let mut v = reply.to_vec();
        v[0] = 1;
        Box::leak(v.into_boxed_slice())
    }

    #[test]
    fn pool_defaults_validate() {
        let p = Pool::parse(&cfg()).unwrap();
        assert_eq!(p.label(), "192.168.8.100 - 192.168.8.149");
        assert_eq!(p.broadcast(), Ipv4Addr::new(192, 168, 8, 255));
        assert!(p.contains(Ipv4Addr::new(192, 168, 8, 149)));
        assert!(!p.contains(Ipv4Addr::new(192, 168, 8, 150)));
    }

    #[test]
    fn pool_rejects_bad_configs() {
        let mut c = cfg();
        c.pool_start = "192.168.9.100".into();
        assert!(Pool::parse(&c).is_err(), "pool buiten subnet");
        let mut c = cfg();
        c.pool_start = "192.168.8.1".into();
        c.pool_size = 10;
        assert!(Pool::parse(&c).is_err(), "server-IP in pool");
        let mut c = cfg();
        c.pool_start = "192.168.8.250".into();
        c.pool_size = 10;
        assert!(Pool::parse(&c).is_err(), "pool bevat broadcast");
        let mut c = cfg();
        c.subnet = "255.0.255.0".into();
        assert!(Pool::parse(&c).is_err(), "niet-aaneengesloten masker");
        let mut c = cfg();
        c.pool_size = 0;
        assert!(Pool::parse(&c).is_err(), "lege scope");
    }

    #[test]
    fn discover_then_request_binds_first_pool_address() {
        let pool = Pool::parse(&cfg()).unwrap();
        let mut s = shared();
        let mac = [0x44, 0x19, 0xB6, 0x01, 0x02, 0x03];

        let disc = client_packet(DHCPDISCOVER, mac, Ipv4Addr::UNSPECIFIED, &[(OPT_HOSTNAME, b"cam-1")]);
        let req = Packet::parse(&disc).unwrap();
        let (t, offer) = handle(&req, &pool, &mut s, 1_000).unwrap();
        assert_eq!(t, DHCPOFFER);
        assert_eq!(&offer[16..20], &[192, 168, 8, 100], "yiaddr");
        assert_eq!(opt_of(&offer, OPT_SERVER_ID).unwrap(), vec![192, 168, 8, 8]);
        assert_eq!(opt_of(&offer, OPT_SUBNET_MASK).unwrap(), vec![255, 255, 255, 0]);
        assert_eq!(opt_of(&offer, OPT_LEASE_TIME).unwrap(), 3600u32.to_be_bytes().to_vec());
        assert!(offer.len() >= MIN_PACKET);
        assert_eq!(s.leases.len(), 1);
        assert_eq!(s.leases[0].state, LeaseState::Offered);
        assert_eq!(s.leases[0].hostname.as_deref(), Some("cam-1"));

        let rq = client_packet(
            DHCPREQUEST,
            mac,
            Ipv4Addr::UNSPECIFIED,
            &[(OPT_REQUESTED_IP, &[192, 168, 8, 100]), (OPT_SERVER_ID, &[192, 168, 8, 8])],
        );
        let req = Packet::parse(&rq).unwrap();
        let (t, ack) = handle(&req, &pool, &mut s, 2_000).unwrap();
        assert_eq!(t, DHCPACK);
        assert_eq!(&ack[16..20], &[192, 168, 8, 100]);
        assert_eq!(s.leases[0].state, LeaseState::Bound);
        assert_eq!(s.leases[0].expires_ms, 2_000 + 3_600_000);
        assert_eq!(s.leases[0].mac, "44:19:B6:01:02:03");
    }

    #[test]
    fn second_client_gets_next_address_and_same_mac_keeps_its_lease() {
        let pool = Pool::parse(&cfg()).unwrap();
        let mut s = shared();
        let a = [1, 1, 1, 1, 1, 1];
        let b = [2, 2, 2, 2, 2, 2];
        let d = |mac| client_packet(DHCPDISCOVER, mac, Ipv4Addr::UNSPECIFIED, &[]);
        let pa = d(a);
        let pb = d(b);
        let (_, offer_a) = handle(&Packet::parse(&pa).unwrap(), &pool, &mut s, 0).unwrap();
        let (_, offer_b) = handle(&Packet::parse(&pb).unwrap(), &pool, &mut s, 0).unwrap();
        assert_eq!(&offer_a[16..20], &[192, 168, 8, 100]);
        assert_eq!(&offer_b[16..20], &[192, 168, 8, 101]);
        // A vraagt opnieuw: zelfde adres.
        let (_, again) = handle(&Packet::parse(&pa).unwrap(), &pool, &mut s, 10).unwrap();
        assert_eq!(&again[16..20], &[192, 168, 8, 100]);
        assert_eq!(s.leases.len(), 2);
    }

    #[test]
    fn request_for_foreign_or_taken_address_is_nak() {
        let pool = Pool::parse(&cfg()).unwrap();
        let mut s = shared();
        // Apparaat met oude lease uit een ander netwerk (renew via broadcast).
        let rq = client_packet(DHCPREQUEST, [9; 6], Ipv4Addr::new(10, 0, 0, 5), &[]);
        let (t, nak) = handle(&Packet::parse(&rq).unwrap(), &pool, &mut s, 0).unwrap();
        assert_eq!(t, DHCPNAK);
        assert_eq!(opt_of(&nak, OPT_MSG_TYPE).unwrap(), vec![DHCPNAK]);
        assert!(opt_of(&nak, OPT_LEASE_TIME).is_none());

        // Adres bezet door een ander MAC → NAK.
        let d = client_packet(DHCPDISCOVER, [1; 6], Ipv4Addr::UNSPECIFIED, &[]);
        handle(&Packet::parse(&d).unwrap(), &pool, &mut s, 0).unwrap();
        let rq2 = client_packet(
            DHCPREQUEST,
            [2; 6],
            Ipv4Addr::UNSPECIFIED,
            &[(OPT_REQUESTED_IP, &[192, 168, 8, 100])],
        );
        let (t, _) = handle(&Packet::parse(&rq2).unwrap(), &pool, &mut s, 0).unwrap();
        assert_eq!(t, DHCPNAK);
    }

    #[test]
    fn request_to_other_server_withdraws_offer_silently() {
        let pool = Pool::parse(&cfg()).unwrap();
        let mut s = shared();
        let d = client_packet(DHCPDISCOVER, [1; 6], Ipv4Addr::UNSPECIFIED, &[]);
        handle(&Packet::parse(&d).unwrap(), &pool, &mut s, 0).unwrap();
        let rq = client_packet(
            DHCPREQUEST,
            [1; 6],
            Ipv4Addr::UNSPECIFIED,
            &[(OPT_REQUESTED_IP, &[192, 168, 1, 50]), (OPT_SERVER_ID, &[192, 168, 1, 1])],
        );
        assert!(handle(&Packet::parse(&rq).unwrap(), &pool, &mut s, 0).is_none());
        assert!(s.leases.is_empty());
    }

    #[test]
    fn release_decline_and_expiry_free_addresses() {
        let pool = Pool::parse(&cfg()).unwrap();
        let mut s = shared();
        let d = client_packet(DHCPDISCOVER, [1; 6], Ipv4Addr::UNSPECIFIED, &[]);
        handle(&Packet::parse(&d).unwrap(), &pool, &mut s, 0).unwrap();
        let rel = client_packet(DHCPRELEASE, [1; 6], Ipv4Addr::new(192, 168, 8, 100), &[]);
        assert!(handle(&Packet::parse(&rel).unwrap(), &pool, &mut s, 1).is_none());
        assert!(s.leases.is_empty());

        // DECLINE blokkeert het adres: de volgende DISCOVER krijgt .101.
        let dec = client_packet(
            DHCPDECLINE,
            [1; 6],
            Ipv4Addr::UNSPECIFIED,
            &[(OPT_REQUESTED_IP, &[192, 168, 8, 100])],
        );
        handle(&Packet::parse(&dec).unwrap(), &pool, &mut s, 2);
        let d2 = client_packet(DHCPDISCOVER, [3; 6], Ipv4Addr::UNSPECIFIED, &[]);
        let (_, offer) = handle(&Packet::parse(&d2).unwrap(), &pool, &mut s, 3).unwrap();
        assert_eq!(&offer[16..20], &[192, 168, 8, 101]);

        // Een verlopen offer verdwijnt.
        assert_eq!(s.leases.len(), 1);
        expire(&mut s, 3 + OFFER_HOLD_MS + 1);
        assert!(s.leases.is_empty());
    }

    #[test]
    fn inform_gets_ack_without_address() {
        let pool = Pool::parse(&cfg()).unwrap();
        let mut s = shared();
        let inf = client_packet(DHCPINFORM, [5; 6], Ipv4Addr::new(192, 168, 8, 77), &[]);
        let req = Packet::parse(&inf).unwrap();
        let (t, ack) = handle(&req, &pool, &mut s, 0).unwrap();
        assert_eq!(t, DHCPACK);
        assert_eq!(&ack[16..20], &[0, 0, 0, 0], "geen yiaddr");
        assert_eq!(&ack[12..16], &[192, 168, 8, 77], "ciaddr terug");
        assert!(opt_of(&ack, OPT_LEASE_TIME).is_none());
        assert!(opt_of(&ack, OPT_ROUTER).is_some());
        assert!(s.leases.is_empty());
        let from: SocketAddr = "192.168.8.77:68".parse().unwrap();
        assert_eq!(reply_dest(&req, DHCPACK, from), from);
    }

    #[test]
    fn reply_destinations_follow_rfc() {
        let pool = Pool::parse(&cfg()).unwrap();
        let _ = pool;
        let any: SocketAddr = "0.0.0.0:68".parse().unwrap();
        let d = client_packet(DHCPDISCOVER, [1; 6], Ipv4Addr::UNSPECIFIED, &[]);
        let req = Packet::parse(&d).unwrap();
        assert_eq!(reply_dest(&req, DHCPOFFER, any), "255.255.255.255:68".parse().unwrap());
        let r = client_packet(DHCPREQUEST, [1; 6], Ipv4Addr::new(192, 168, 8, 100), &[]);
        let req = Packet::parse(&r).unwrap();
        assert_eq!(reply_dest(&req, DHCPACK, any), "192.168.8.100:68".parse().unwrap());
        assert_eq!(reply_dest(&req, DHCPNAK, any), "255.255.255.255:68".parse().unwrap());
    }

    #[test]
    fn ignores_garbage_and_bootreply() {
        assert!(Packet::parse(&[0u8; 100]).is_none());
        let mut p = client_packet(DHCPDISCOVER, [1; 6], Ipv4Addr::UNSPECIFIED, &[]);
        p[0] = 2;
        assert!(Packet::parse(&p).is_none());
        p[0] = 1;
        p[236] = 0;
        assert!(Packet::parse(&p).is_none());
    }
}
