use crate::util::now_ms;
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;
use std::sync::OnceLock;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::NetworkManagement::IpHelper::{IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY};

const REQUEST_DATA: [u8; 32] = [b'T'; 32];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingResult {
    pub target: String,
    pub timestamp_ms: i64,
    pub rtt_ms: Option<u32>,
    pub error: Option<String>,
}

impl PingResult {
    /// Een mislukte ping (geen RTT) met de gegeven foutmelding.
    fn err(target: &str, timestamp_ms: i64, msg: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            timestamp_ms,
            rtt_ms: None,
            error: Some(msg.into()),
        }
    }
}

static ICMP_HANDLE: OnceLock<usize> = OnceLock::new();

fn get_or_init_handle() -> Result<HANDLE, String> {
    if let Some(&h) = ICMP_HANDLE.get() {
        return Ok(HANDLE(h as *mut _));
    }
    let new_handle = unsafe { IcmpCreateFile() }
        .map_err(|e| format!("IcmpCreateFile mislukt: {e}"))?;
    let _ = ICMP_HANDLE.set(new_handle.0 as usize);
    Ok(new_handle)
}

pub fn ping_once(target: &str, timeout_ms: u32) -> PingResult {
    let timestamp_ms = now_ms();

    let ip: Ipv4Addr = match target.parse() {
        Ok(ip) => ip,
        Err(e) => return PingResult::err(target, timestamp_ms, format!("ongeldig IP '{target}': {e}")),
    };

    let handle = match get_or_init_handle() {
        Ok(h) => h,
        Err(e) => return PingResult::err(target, timestamp_ms, e),
    };

    let dest: u32 = u32::from_le_bytes(ip.octets());
    let mut reply_buf =
        vec![0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + REQUEST_DATA.len() + 8];

    let replies = unsafe {
        IcmpSendEcho(
            handle,
            dest,
            REQUEST_DATA.as_ptr() as *const _,
            REQUEST_DATA.len() as u16,
            None,
            reply_buf.as_mut_ptr() as *mut _,
            reply_buf.len() as u32,
            timeout_ms,
        )
    };

    if replies == 0 {
        return PingResult::err(target, timestamp_ms, "timeout");
    }

    let reply: &ICMP_ECHO_REPLY = unsafe { &*(reply_buf.as_ptr() as *const ICMP_ECHO_REPLY) };

    if reply.Status != 0 {
        return PingResult::err(target, timestamp_ms, format!("ICMP status {}", reply.Status));
    }

    PingResult {
        target: target.into(),
        timestamp_ms,
        rtt_ms: Some(reply.RoundTripTime),
        error: None,
    }
}
