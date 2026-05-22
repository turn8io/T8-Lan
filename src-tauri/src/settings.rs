use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

const SETTINGS_FILE: &str = "settings.json";
const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub version: u32,
    #[serde(default)]
    pub selected_adapter: Option<AdapterRef>,
    #[serde(default = "default_subnet")]
    pub subnet: String,
    #[serde(default = "default_gateway_mode")]
    pub gateway_mode: GatewayMode,
    #[serde(default)]
    pub gateway_manual: Option<String>,
    #[serde(default)]
    pub dns: DnsSettings,
    #[serde(default)]
    pub recent_ips_by_adapter: HashMap<String, Vec<RecentIp>>,
    #[serde(default)]
    pub ping: PingSettings,
    #[serde(default)]
    pub window: Option<WindowPos>,
    #[serde(default = "default_true")]
    pub global_hotkey_enabled: bool,
    #[serde(default = "default_hotkey_dhcp")]
    pub hotkey_dhcp: String,
    #[serde(default = "default_hotkey_static")]
    pub hotkey_static: String,
    #[serde(default)]
    pub ssid_rules: Vec<SsidRule>,
    #[serde(default)]
    pub examples_seeded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterRef {
    pub friendly_name: String,
    pub luid: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum GatewayMode {
    AutoFirst,
    AutoLast,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DnsSettings {
    pub apply_on_switch: bool,
    pub preset: DnsPreset,
    pub custom_primary: Option<String>,
    pub custom_secondary: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DnsPreset {
    #[default]
    Cloudflare,
    Google,
    Opendns,
    Quad9,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentIp {
    pub ip: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub last_used: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PingSettings {
    #[serde(default = "default_ping_target")]
    pub target: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WindowPos {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsidRule {
    pub ssid: String,
    pub action: SsidAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "type", content = "value")]
pub enum SsidAction {
    Dhcp,
    StaticIp(String),
}

fn default_subnet() -> String {
    "255.255.255.0".to_string()
}
fn default_gateway_mode() -> GatewayMode {
    GatewayMode::AutoFirst
}
fn default_ping_target() -> String {
    "1.1.1.1".to_string()
}
fn default_true() -> bool {
    true
}
fn default_hotkey_dhcp() -> String {
    "Control+Alt+KeyD".to_string()
}
fn default_hotkey_static() -> String {
    "Control+Alt+KeyS".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            selected_adapter: None,
            subnet: default_subnet(),
            gateway_mode: default_gateway_mode(),
            gateway_manual: None,
            dns: DnsSettings::default(),
            recent_ips_by_adapter: HashMap::new(),
            ping: PingSettings {
                target: default_ping_target(),
            },
            window: None,
            global_hotkey_enabled: true,
            hotkey_dhcp: default_hotkey_dhcp(),
            hotkey_static: default_hotkey_static(),
            ssid_rules: Vec::new(),
            examples_seeded: false,
        }
    }
}

pub fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("config dir niet beschikbaar: {e}"))?;
    Ok(dir.join(SETTINGS_FILE))
}

pub fn load(app: &AppHandle) -> Result<Settings, String> {
    let path = settings_path(app)?;
    if !path.exists() {
        return Ok(Settings::default());
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("lezen settings: {e}"))?;
    serde_json::from_str::<Settings>(&raw).or_else(|e| {
        eprintln!("settings.json corrupt, terug naar default: {e}");
        Ok(Settings::default())
    })
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = settings_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("config dir aanmaken: {e}"))?;
    }
    let tmp = path.with_extension("json.tmp");
    let raw = serde_json::to_string_pretty(settings).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&tmp, raw).map_err(|e| format!("schrijven tmp: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("rename tmp: {e}"))?;
    Ok(())
}
