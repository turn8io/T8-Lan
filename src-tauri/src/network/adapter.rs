use super::{AdapterInfo, AdapterKind};
use std::net::Ipv4Addr;

use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, NO_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_INCLUDE_PREFIX,
    GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
};
use windows::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};

const IF_TYPE_ETHERNET_CSMACD: u32 = 6;
const IF_TYPE_IEEE80211: u32 = 71;
const IF_OPER_STATUS_UP: i32 = 1;

pub fn list_adapters() -> Result<Vec<AdapterInfo>, String> {
    let mut size: u32 = 15_000;
    let mut buf: Vec<u8> = vec![0u8; size as usize];

    let flags = GAA_FLAG_INCLUDE_GATEWAYS | GAA_FLAG_INCLUDE_PREFIX
        | GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST;

    loop {
        let res = unsafe {
            GetAdaptersAddresses(
                AF_INET.0 as u32,
                flags,
                None,
                Some(buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
                &mut size,
            )
        };

        if res == ERROR_BUFFER_OVERFLOW.0 {
            buf = vec![0u8; size as usize];
            continue;
        }
        if res != NO_ERROR.0 {
            return Err(format!("GetAdaptersAddresses faalt: WIN32 {res}"));
        }
        break;
    }

    let mut adapters = Vec::new();
    let mut current = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;

    while !current.is_null() {
        let adapter = unsafe { &*current };

        let kind = match adapter.IfType {
            IF_TYPE_ETHERNET_CSMACD => AdapterKind::Ethernet,
            IF_TYPE_IEEE80211 => AdapterKind::WiFi,
            _ => AdapterKind::Other,
        };

        if matches!(kind, AdapterKind::Other) {
            current = adapter.Next;
            continue;
        }

        let friendly_name = unsafe { adapter.FriendlyName.to_string() }.unwrap_or_default();
        let description = unsafe { adapter.Description.to_string() }.unwrap_or_default();

        if friendly_name.is_empty() {
            current = adapter.Next;
            continue;
        }

        let luid: u64 = unsafe { adapter.Luid.Value };
        let is_up = adapter.OperStatus.0 == IF_OPER_STATUS_UP;

        let is_dhcp = adapter.Dhcpv4Server.iSockaddrLength > 0
            && !adapter.Dhcpv4Server.lpSockaddr.is_null();

        let (current_ip, current_subnet) = read_first_unicast_ipv4(adapter);
        let current_gateway = read_first_gateway_ipv4(adapter);
        let dns_servers = read_dns_servers_ipv4(adapter);

        adapters.push(AdapterInfo {
            friendly_name,
            description,
            luid,
            kind,
            is_up,
            is_dhcp,
            current_ip,
            current_subnet,
            current_gateway,
            dns_servers,
        });

        current = adapter.Next;
    }

    adapters.sort_by(|a, b| {
        match (a.is_up, b.is_up) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.friendly_name.cmp(&b.friendly_name),
        }
    });

    Ok(adapters)
}

fn read_first_unicast_ipv4(
    adapter: &IP_ADAPTER_ADDRESSES_LH,
) -> (Option<String>, Option<String>) {
    let mut node = adapter.FirstUnicastAddress;
    while !node.is_null() {
        let entry = unsafe { &*node };
        if let Some(ipv4) = sockaddr_to_ipv4(entry.Address.lpSockaddr) {
            let prefix = entry.OnLinkPrefixLength;
            return (Some(ipv4.to_string()), Some(prefix_to_mask(prefix).to_string()));
        }
        node = entry.Next;
    }
    (None, None)
}

fn read_first_gateway_ipv4(adapter: &IP_ADAPTER_ADDRESSES_LH) -> Option<String> {
    let mut node = adapter.FirstGatewayAddress;
    while !node.is_null() {
        let entry = unsafe { &*node };
        if let Some(ipv4) = sockaddr_to_ipv4(entry.Address.lpSockaddr) {
            return Some(ipv4.to_string());
        }
        node = entry.Next;
    }
    None
}

fn read_dns_servers_ipv4(adapter: &IP_ADAPTER_ADDRESSES_LH) -> Vec<String> {
    let mut servers = Vec::new();
    let mut node = adapter.FirstDnsServerAddress;
    while !node.is_null() {
        let entry = unsafe { &*node };
        if let Some(ipv4) = sockaddr_to_ipv4(entry.Address.lpSockaddr) {
            servers.push(ipv4.to_string());
        }
        node = entry.Next;
    }
    servers
}

fn sockaddr_to_ipv4(
    raw: *const windows::Win32::Networking::WinSock::SOCKADDR,
) -> Option<Ipv4Addr> {
    if raw.is_null() {
        return None;
    }
    let sa = unsafe { &*raw };
    if sa.sa_family != AF_INET {
        return None;
    }
    let sin: &SOCKADDR_IN = unsafe { &*(raw as *const SOCKADDR_IN) };
    let bytes: [u8; 4] = unsafe {
        std::ptr::read_unaligned(&sin.sin_addr as *const _ as *const [u8; 4])
    };
    Some(Ipv4Addr::from(bytes))
}

fn prefix_to_mask(prefix: u8) -> Ipv4Addr {
    if prefix == 0 {
        return Ipv4Addr::new(0, 0, 0, 0);
    }
    let mask: u32 = (!0u32).checked_shl(32 - prefix as u32).unwrap_or(0);
    Ipv4Addr::from(mask)
}
