use std::ffi::c_void;
use windows::core::GUID;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::NetworkManagement::WiFi::{
    wlan_intf_opcode_current_connection, wlan_interface_state_connected, WlanCloseHandle,
    WlanEnumInterfaces, WlanFreeMemory, WlanGetAvailableNetworkList, WlanOpenHandle,
    WlanQueryInterface, DOT11_SSID, WLAN_AVAILABLE_NETWORK_LIST, WLAN_CONNECTION_ATTRIBUTES,
    WLAN_INTERFACE_INFO_LIST,
};

/// WLAN client API-versie (v2; vereist op Windows Vista en nieuwer).
const WLAN_CLIENT_VERSION: u32 = 2;

/// Open een WLAN-handle, enumereer de interfaces en roep `f` aan voor elke interface.
/// De handle wordt altijd weer gesloten. Faalt alleen als de handle niet te openen is;
/// als er geen interfaces zijn loopt `f` simpelweg nul keer.
fn for_each_interface<F>(mut f: F) -> Result<(), String>
where
    F: FnMut(HANDLE, &GUID),
{
    let mut client_handle = HANDLE::default();
    let mut negotiated = 0u32;
    let open =
        unsafe { WlanOpenHandle(WLAN_CLIENT_VERSION, None, &mut negotiated, &mut client_handle) };
    if open != 0 {
        return Err(format!("WlanOpenHandle faalt: WIN32 {open}"));
    }

    let mut list_ptr: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
    let enum_r = unsafe { WlanEnumInterfaces(client_handle, None, &mut list_ptr) };
    if enum_r == 0 && !list_ptr.is_null() {
        unsafe {
            let list = &*list_ptr;
            let count = list.dwNumberOfItems as usize;
            let interfaces = std::slice::from_raw_parts(list.InterfaceInfo.as_ptr(), count);
            for iface in interfaces {
                f(client_handle, &iface.InterfaceGuid);
            }
            WlanFreeMemory(list_ptr as *mut c_void);
        }
    }

    unsafe {
        let _ = WlanCloseHandle(client_handle, None);
    }
    Ok(())
}

/// Decodeer een `DOT11_SSID` naar een String (max 32 bytes). `None` bij een lege of
/// niet-UTF-8 SSID.
fn ssid_to_string(dot11: &DOT11_SSID) -> Option<String> {
    let len = (dot11.uSSIDLength as usize).min(32);
    if len == 0 {
        return None;
    }
    std::str::from_utf8(&dot11.ucSSID[..len])
        .ok()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

/// Returns the SSID currently associated on each WiFi interface (typically 0 or 1).
/// Empty vec means "no WiFi connection".
pub fn current_ssids() -> Result<Vec<String>, String> {
    let mut ssids = Vec::new();
    for_each_interface(|handle, guid| {
        if let Some(ssid) = query_ssid(handle, guid) {
            ssids.push(ssid);
        }
    })?;
    Ok(ssids)
}

fn query_ssid(handle: HANDLE, guid: &GUID) -> Option<String> {
    let mut data_size = 0u32;
    let mut data_ptr: *mut c_void = std::ptr::null_mut();

    let r = unsafe {
        WlanQueryInterface(
            handle,
            guid as *const GUID,
            wlan_intf_opcode_current_connection,
            None,
            &mut data_size,
            &mut data_ptr,
            None,
        )
    };

    if r != 0 || data_ptr.is_null() {
        return None;
    }

    unsafe {
        let conn: &WLAN_CONNECTION_ATTRIBUTES = &*(data_ptr as *const WLAN_CONNECTION_ATTRIBUTES);
        let ssid = if conn.isState == wlan_interface_state_connected {
            ssid_to_string(&conn.wlanAssociationAttributes.dot11Ssid)
        } else {
            None
        };
        WlanFreeMemory(data_ptr);
        ssid
    }
}

/// Returns a de-duplicated list of SSIDs that are currently in range
/// (across all WiFi interfaces).
pub fn list_available_networks() -> Result<Vec<String>, String> {
    let mut ssids: Vec<String> = Vec::new();
    for_each_interface(|handle, guid| collect_networks(handle, guid, &mut ssids))?;
    ssids.sort();
    ssids.dedup();
    Ok(ssids)
}

fn collect_networks(handle: HANDLE, guid: &GUID, out: &mut Vec<String>) {
    let mut list_ptr: *mut WLAN_AVAILABLE_NETWORK_LIST = std::ptr::null_mut();
    let r = unsafe {
        WlanGetAvailableNetworkList(handle, guid as *const GUID, 0, None, &mut list_ptr)
    };
    if r != 0 || list_ptr.is_null() {
        return;
    }
    unsafe {
        let list = &*list_ptr;
        let count = list.dwNumberOfItems as usize;
        let networks = std::slice::from_raw_parts(list.Network.as_ptr(), count);
        for net in networks {
            if let Some(ssid) = ssid_to_string(&net.dot11Ssid) {
                out.push(ssid);
            }
        }
        WlanFreeMemory(list_ptr as *mut c_void);
    }
}
