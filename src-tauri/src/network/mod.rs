pub mod adapter;
pub mod dns;
pub mod ip;
mod netsh;
pub mod ping;
pub mod scan;
pub mod wifi;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterInfo {
    pub friendly_name: String,
    pub description: String,
    pub luid: u64,
    pub kind: AdapterKind,
    pub is_up: bool,
    pub is_dhcp: bool,
    pub current_ip: Option<String>,
    pub current_subnet: Option<String>,
    pub current_gateway: Option<String>,
    pub dns_servers: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AdapterKind {
    Ethernet,
    WiFi,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentStatus {
    pub adapter: Option<AdapterInfo>,
    pub state: StatusState,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StatusState {
    Idle,
    Busy,
    Success,
    Error,
    WaitingDhcp,
}
