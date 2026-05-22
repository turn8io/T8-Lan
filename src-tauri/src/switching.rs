//! Gedeelde switch-logica, gebruikt door tray, hotkeys, SSID-watcher en IPC.
//! Houdt gateway-berekening, DNS, recents-bijwerken, tray-tooltip en events
//! op één plek zodat alle triggers identiek gedrag vertonen.

use crate::network::{adapter, dns, ip};
use crate::settings::{self, DnsPreset, DnsSettings, GatewayMode, RecentIp, Settings};
use serde::Serialize;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

/// Show a Windows toast — only used for hotkey-triggered switches, since the
/// window is closed then and the in-app pulse is invisible.
fn toast_hotkey(app: &AppHandle, body: &str) {
    let _ = app
        .notification()
        .builder()
        .title("T8-Lan")
        .body(body)
        .show();
}

#[derive(Clone, Debug, Serialize)]
pub struct PreviousConfig {
    pub adapter_name: String,
    pub was_dhcp: bool,
    pub ip: Option<String>,
    pub subnet: Option<String>,
    pub gateway: Option<String>,
}

pub struct UndoState {
    pub prev: Mutex<Option<PreviousConfig>>,
    pub next: Mutex<Option<PreviousConfig>>,
}

impl UndoState {
    pub fn new() -> Self {
        Self {
            prev: Mutex::new(None),
            next: Mutex::new(None),
        }
    }
}

pub fn capture_current(adapter_name: &str) -> Option<PreviousConfig> {
    let adapters = adapter::list_adapters().ok()?;
    let a = adapters.into_iter().find(|a| a.friendly_name == adapter_name)?;
    Some(PreviousConfig {
        adapter_name: a.friendly_name,
        was_dhcp: a.is_dhcp,
        ip: a.current_ip,
        subnet: a.current_subnet,
        gateway: a.current_gateway,
    })
}

fn record_undo(app: &AppHandle, adapter_name: &str) {
    if let Some(state) = app.try_state::<Arc<UndoState>>() {
        if let Some(prev) = capture_current(adapter_name) {
            if let Ok(mut guard) = state.prev.lock() {
                *guard = Some(prev);
            }
        }
        // A new switch invalidates any pending redo.
        if let Ok(mut guard) = state.next.lock() {
            *guard = None;
        }
    }
}

pub fn compute_gateway(ip: &str, subnet: &str, s: &Settings) -> Result<String, String> {
    if s.gateway_mode == GatewayMode::Manual {
        return s
            .gateway_manual
            .clone()
            .filter(|g| !g.is_empty())
            .ok_or_else(|| "handmatige gateway ontbreekt".into());
    }
    let ip_addr: Ipv4Addr = ip.parse().map_err(|e| format!("IP parse: {e}"))?;
    let mask_addr: Ipv4Addr = subnet.parse().map_err(|e| format!("subnet parse: {e}"))?;
    let network = u32::from(ip_addr) & u32::from(mask_addr);
    let host_bits = !u32::from(mask_addr);
    let host = match s.gateway_mode {
        GatewayMode::AutoLast => host_bits.saturating_sub(1),
        _ => 1,
    };
    Ok(Ipv4Addr::from(network | host).to_string())
}

fn resolve_dns(dns: &DnsSettings) -> (String, Option<String>) {
    match dns.preset {
        DnsPreset::Cloudflare => ("1.1.1.1".into(), Some("1.0.0.1".into())),
        DnsPreset::Google => ("8.8.8.8".into(), Some("8.8.4.4".into())),
        DnsPreset::Opendns => ("208.67.222.222".into(), Some("208.67.220.220".into())),
        DnsPreset::Quad9 => ("9.9.9.9".into(), Some("149.112.112.112".into())),
        DnsPreset::Custom => (
            dns.custom_primary.clone().unwrap_or_default(),
            dns.custom_secondary.clone(),
        ),
    }
}

fn bump_recent(s: &mut Settings, adapter: &str, ip: &str) {
    let list = s
        .recent_ips_by_adapter
        .entry(adapter.to_string())
        .or_default();
    let label = list.iter().find(|r| r.ip == ip).and_then(|r| r.label.clone());
    list.retain(|r| r.ip != ip);
    list.insert(
        0,
        RecentIp {
            ip: ip.to_string(),
            label,
            last_used: Some(crate::util::now_ms()),
        },
    );
    list.truncate(5);
}

fn emit_result(app: &AppHandle, ok: bool, msg: String, trigger: &str) {
    let _ = app.emit(
        "switch-result",
        serde_json::json!({ "ok": ok, "msg": msg, "trigger": trigger }),
    );
}

/// Markeer een mislukte switch: optionele hotkey-toast en een `switch-result`-event
/// met de foutmelding. Gedeeld door alle faalpaden.
fn fail(app: &AppHandle, trigger: &str, msg: String) {
    if trigger == "hotkey" {
        toast_hotkey(app, &format!("✗ {msg}"));
    }
    emit_result(app, false, msg, trigger);
}

/// Switch the given adapter to DHCP. Blocking — call from a worker thread.
pub fn do_dhcp(app: &AppHandle, adapter_name: &str, trigger: &str) {
    record_undo(app, adapter_name);
    crate::tray::update_tooltip(app, &format!("T8-Lan — switching {adapter_name} to DHCP..."));
    match ip::set_dhcp(adapter_name) {
        Ok(()) => {
            let _ = dns::set_dhcp_dns(adapter_name);
            if trigger == "hotkey" {
                toast_hotkey(app, &format!("✓ DHCP on {adapter_name}"));
            }
            emit_result(app, true, format!("DHCP on {adapter_name}"), trigger);
        }
        Err(e) => fail(app, trigger, e),
    }
}

/// Switch the given adapter to a static IP, applying subnet/gateway/DNS from
/// settings and updating the recents list. Blocking — call from a worker thread.
pub fn do_static(app: &AppHandle, adapter_name: &str, ip_addr: &str, trigger: &str) {
    record_undo(app, adapter_name);
    let mut s = settings::load(app).unwrap_or_default();
    let subnet = s.subnet.clone();
    let gateway = match compute_gateway(ip_addr, &subnet, &s) {
        Ok(g) => g,
        Err(e) => {
            fail(app, trigger, format!("gateway error: {e}"));
            return;
        }
    };

    crate::tray::update_tooltip(app, &format!("T8-Lan — switching to {ip_addr}..."));

    match ip::set_static(adapter_name, ip_addr, &subnet, &gateway) {
        Ok(()) => {
            if s.dns.apply_on_switch {
                let (p, sec) = resolve_dns(&s.dns);
                if !p.is_empty() {
                    let _ = dns::set_static_dns(adapter_name, &p, sec.as_deref());
                }
            }
            bump_recent(&mut s, adapter_name, ip_addr);
            let _ = settings::save(app, &s);
            let _ = crate::tray::rebuild(app);
            if trigger == "hotkey" {
                toast_hotkey(app, &format!("✓ Static {ip_addr} on {adapter_name}"));
            }
            emit_result(app, true, format!("Static {ip_addr} on {adapter_name}"), trigger);
        }
        Err(e) => fail(app, trigger, e),
    }
}

/// Build the tray tooltip text from the current adapter status.
pub fn tooltip_for_selected(app: &AppHandle) -> String {
    let s = settings::load(app).unwrap_or_default();
    let adapters = match adapter::list_adapters() {
        Ok(a) => a,
        Err(_) => return "T8-Lan".into(),
    };
    let selected = match &s.selected_adapter {
        Some(r) => adapters.iter().find(|a| a.friendly_name == r.friendly_name),
        None => adapters.iter().find(|a| a.is_up && a.current_ip.is_some()),
    };
    // Quick internet check: ping the configured DNS (fallback 1.1.1.1).
    let dns_target = {
        let (p, _) = resolve_dns(&s.dns);
        if p.is_empty() { "1.1.1.1".to_string() } else { p }
    };
    let online = crate::network::ping::ping_once(&dns_target, 700).rtt_ms.is_some();
    let dot = if online { " 🟢" } else { " 🔴" };

    match selected {
        Some(a) => {
            let mode = if a.is_dhcp { "DHCP" } else { "Static" };
            match &a.current_ip {
                Some(ip) => format!("{mode} {ip}{dot}"),
                None => format!("{mode} — no IP{dot}"),
            }
        }
        None => format!("no adapter{dot}"),
    }
}

/// Resolve the most recent static IP for the selected adapter (for the static hotkey).
pub fn last_static_ip(app: &AppHandle) -> Option<(String, String)> {
    let s = settings::load(app).unwrap_or_default();
    let adapter = s.selected_adapter.as_ref()?.friendly_name.clone();
    let ip = s
        .recent_ips_by_adapter
        .get(&adapter)
        .and_then(|list| list.first())
        .map(|r| r.ip.clone())?;
    Some((adapter, ip))
}

pub fn selected_adapter_name(app: &AppHandle) -> Option<String> {
    settings::load(app)
        .ok()
        .and_then(|s| s.selected_adapter.map(|r| r.friendly_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::GatewayMode;

    fn settings_with(mode: GatewayMode, manual: Option<&str>) -> Settings {
        let mut s = Settings::default();
        s.gateway_mode = mode;
        s.gateway_manual = manual.map(str::to_string);
        s
    }

    #[test]
    fn gateway_auto_first_is_dot_one() {
        let s = settings_with(GatewayMode::AutoFirst, None);
        assert_eq!(
            compute_gateway("192.168.1.50", "255.255.255.0", &s).unwrap(),
            "192.168.1.1"
        );
    }

    #[test]
    fn gateway_auto_last_is_dot_254() {
        let s = settings_with(GatewayMode::AutoLast, None);
        assert_eq!(
            compute_gateway("192.168.1.50", "255.255.255.0", &s).unwrap(),
            "192.168.1.254"
        );
    }

    #[test]
    fn gateway_auto_first_respects_mask() {
        // /16 → eerste host is x.x.0.1
        let s = settings_with(GatewayMode::AutoFirst, None);
        assert_eq!(
            compute_gateway("10.5.20.30", "255.255.0.0", &s).unwrap(),
            "10.5.0.1"
        );
    }

    #[test]
    fn gateway_manual_returns_configured_value() {
        let s = settings_with(GatewayMode::Manual, Some("10.0.0.9"));
        assert_eq!(
            compute_gateway("192.168.1.50", "255.255.255.0", &s).unwrap(),
            "10.0.0.9"
        );
    }

    #[test]
    fn gateway_manual_missing_is_error() {
        let s = settings_with(GatewayMode::Manual, None);
        assert!(compute_gateway("192.168.1.50", "255.255.255.0", &s).is_err());
    }

    #[test]
    fn gateway_invalid_ip_is_error() {
        let s = settings_with(GatewayMode::AutoFirst, None);
        assert!(compute_gateway("geen-ip", "255.255.255.0", &s).is_err());
    }
}
