use super::netsh;

pub fn set_static(adapter: &str, ip: &str, subnet: &str, gateway: &str) -> Result<(), String> {
    let name = format!("name={adapter}");
    let addr = format!("addr={ip}");
    let mask = format!("mask={subnet}");
    let gw = format!("gateway={gateway}");
    netsh::run(&[
        "interface", "ipv4", "set", "address",
        &name, "source=static", &addr, &mask, &gw,
    ])
}

pub fn set_dhcp(adapter: &str) -> Result<(), String> {
    let name = format!("name={adapter}");
    netsh::run(&["interface", "ipv4", "set", "address", &name, "source=dhcp"])
}

pub fn check_ip_conflict(ip: &str, _timeout_ms: u32) -> Result<Option<String>, String> {
    use std::net::Ipv4Addr;
    use windows::Win32::NetworkManagement::IpHelper::SendARP;

    let parsed: Ipv4Addr = ip
        .parse()
        .map_err(|e| format!("ongeldig IPv4 adres '{ip}': {e}"))?;
    let dest_ip: u32 = u32::from_le_bytes(parsed.octets());

    let mut mac_buf = [0u8; 8];
    let mut mac_len: u32 = 6;

    let result = unsafe {
        SendARP(
            dest_ip,
            0,
            mac_buf.as_mut_ptr() as *mut _,
            &mut mac_len,
        )
    };

    if result == 0 && mac_len >= 6 {
        let mac = format!(
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            mac_buf[0], mac_buf[1], mac_buf[2], mac_buf[3], mac_buf[4], mac_buf[5]
        );
        Ok(Some(mac))
    } else {
        Ok(None)
    }
}
