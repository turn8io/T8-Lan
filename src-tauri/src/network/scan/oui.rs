//! Camera-/CCTV-merkherkenning op basis van de MAC-OUI (eerste 3 bytes).
//!
//! Bewust beperkt tot toegewijde camera-/surveillance-fabrikanten, zodat we geen
//! algemene netwerkapparatuur (switches, access points, pc's) ten onrechte als camera
//! bestempelen. De tabel is pure data en kan eenvoudig worden uitgebreid; nieuwe
//! prefixes horen hoofdletters + dubbele punt te gebruiken ("XX:XX:XX").

/// (OUI-prefix "XX:XX:XX", merknaam). Prefixes in hoofdletters.
pub const CAMERA_OUIS: &[(&str, &str)] = &[
    // Hikvision (Hangzhou Hikvision Digital Technology)
    ("44:19:B6", "Hikvision"),
    ("4C:BD:8F", "Hikvision"),
    ("28:57:BE", "Hikvision"),
    ("BC:AD:28", "Hikvision"),
    ("C0:56:E3", "Hikvision"),
    ("C4:2F:90", "Hikvision"),
    ("E0:CA:3C", "Hikvision"),
    ("A4:14:37", "Hikvision"),
    ("54:C4:15", "Hikvision"),
    ("58:03:FB", "Hikvision"),
    ("EC:C8:9C", "Hikvision"),
    ("18:80:90", "Hikvision"),
    ("24:18:1D", "Hikvision"),
    ("3C:1B:F8", "Hikvision"),
    ("44:47:CC", "Hikvision"),
    ("84:9A:40", "Hikvision"),
    ("B4:A3:82", "Hikvision"),
    ("BC:BA:C2", "Hikvision"),
    ("F8:4D:FC", "Hikvision"),
    // Dahua Technology
    ("3C:EF:8C", "Dahua"),
    ("90:02:A9", "Dahua"),
    ("14:A7:8B", "Dahua"),
    ("E0:50:8B", "Dahua"),
    ("4C:11:BF", "Dahua"),
    ("08:ED:ED", "Dahua"),
    ("24:52:6A", "Dahua"),
    ("38:AF:29", "Dahua"),
    ("6C:1C:71", "Dahua"),
    ("9C:14:63", "Dahua"),
    ("A0:BD:1D", "Dahua"),
    ("B4:4C:3B", "Dahua"),
    ("BC:32:5F", "Dahua"),
    ("D4:43:0E", "Dahua"),
    ("FC:5F:49", "Dahua"),
    ("FC:B6:9D", "Dahua"),
    // Axis Communications
    ("00:40:8C", "Axis"),
    ("AC:CC:8E", "Axis"),
    ("B8:A4:4F", "Axis"),
    ("E8:27:25", "Axis"),
    // Bosch Security Systems
    ("00:07:5F", "Bosch"),
    ("00:1B:86", "Bosch"),
    // Hanwha Vision / Samsung Techwin (Wisenet)
    ("00:09:18", "Hanwha (Wisenet)"),
    ("E4:30:22", "Hanwha (Wisenet)"),
    // Zhejiang Uniview (UNV)
    ("48:EA:63", "Uniview"),
    // Vivotek
    ("00:02:D1", "Vivotek"),
    // Mobotix
    ("00:03:C5", "Mobotix"),
    // Reolink (Shenzhen Reolink)
    ("EC:71:DB", "Reolink"),
    // Amcrest (Dahua OEM)
    ("9C:8E:CD", "Amcrest"),
    // Foscam
    ("00:62:6E", "Foscam"),
    // Avigilon
    ("00:18:85", "Avigilon"),
];

/// Niet-camera security-/alarmfabrikanten (GE Security → Interlogix/Aritech, nu
/// UTC/Carrier). Apart van [`CAMERA_OUIS`] zodat deze apparaten wél een merk-label
/// krijgen maar niet als camera worden gemarkeerd. Bron: IEEE OUI-registry.
pub const ALARM_OUIS: &[(&str, &str)] = &[
    ("00:17:55", "GE/Interlogix alarm"),
    ("00:08:F8", "UTC/Interlogix alarm"),
    ("00:B0:19", "UTC/Interlogix alarm"),
    ("9C:F6:1A", "Carrier Fire & Security"),
];

fn lookup(table: &[(&str, &'static str)], mac: &str) -> Option<&'static str> {
    if mac.len() < 8 {
        return None;
    }
    let prefix = mac[..8].to_ascii_uppercase();
    table
        .iter()
        .find(|(oui, _)| *oui == prefix)
        .map(|(_, brand)| *brand)
}

/// Zoek het camera-merk bij een MAC-adres ("XX:XX:XX:XX:XX:XX"). `None` als de
/// fabrikant niet in de camera-tabel staat.
pub fn lookup_brand(mac: &str) -> Option<&'static str> {
    lookup(CAMERA_OUIS, mac)
}

/// Zoek een security-/alarmmerk (niet-camera) bij een MAC-adres. `None` indien
/// niet in [`ALARM_OUIS`].
pub fn lookup_alarm_brand(mac: &str) -> Option<&'static str> {
    lookup(ALARM_OUIS, mac)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_oui_matches() {
        assert_eq!(lookup_brand("44:19:B6:11:22:33"), Some("Hikvision"));
        assert_eq!(lookup_brand("3C:EF:8C:00:00:00"), Some("Dahua"));
    }

    #[test]
    fn lookup_is_case_insensitive() {
        assert_eq!(lookup_brand("44:19:b6:aa:bb:cc"), Some("Hikvision"));
    }

    #[test]
    fn alarm_oui_matches() {
        assert_eq!(lookup_alarm_brand("00:17:55:78:43:BF"), Some("GE/Interlogix alarm"));
        assert_eq!(lookup_alarm_brand("9C:F6:1A:00:00:00"), Some("Carrier Fire & Security"));
    }

    #[test]
    fn camera_lookup_ignores_alarm_ouis() {
        // Een alarm-OUI is géén camera.
        assert_eq!(lookup_brand("00:17:55:78:43:BF"), None);
    }

    #[test]
    fn unknown_or_too_short_is_none() {
        assert_eq!(lookup_brand("AB:CD:EF:00:00:00"), None);
        assert_eq!(lookup_brand("44:19"), None);
        assert_eq!(lookup_alarm_brand(""), None);
    }
}
