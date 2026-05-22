use super::netsh;

pub fn set_static_dns(
    adapter: &str,
    primary: &str,
    secondary: Option<&str>,
) -> Result<(), String> {
    let name = format!("name={adapter}");
    let addr = format!("address={primary}");
    netsh::run(&[
        "interface", "ipv4", "set", "dnsservers",
        &name, "source=static", &addr,
        "register=primary", "validate=no",
    ])?;

    if let Some(sec) = secondary.filter(|s| !s.is_empty()) {
        let addr2 = format!("address={sec}");
        netsh::run(&[
            "interface", "ipv4", "add", "dnsservers",
            &name, &addr2, "index=2", "validate=no",
        ])?;
    }
    Ok(())
}

pub fn set_dhcp_dns(adapter: &str) -> Result<(), String> {
    let name = format!("name={adapter}");
    netsh::run(&[
        "interface", "ipv4", "set", "dnsservers",
        &name, "source=dhcp",
    ])
}
