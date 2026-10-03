//! Kleine, modulebrede helpers zonder eigen domein.

use tauri::async_runtime::JoinHandle;
use tokio::sync::Mutex as TokioMutex;

/// Huidige tijd in milliseconden sinds de UNIX-epoch (0 als de klok vóór 1970 staat).
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Een geldig gevormde (8-4-4-4-12, hoofdletters) pseudo-UUID voor discovery-protocollen
/// (SADP, WS-Discovery). Geen crypto-eisen: uniek per aanroep is genoeg.
pub fn pseudo_uuid() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    // SplitMix64-stap over tijd, teller en pid: goed verspreide bits.
    let mut z = nanos ^ n.rotate_left(32) ^ (std::process::id() as u64).rotate_left(17);
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    let a = z ^ (z >> 31);
    let b = a.wrapping_mul(0x2545_F491_4F6C_DD1D).rotate_left(23) ^ nanos;
    // Versie 4 / variant 10xx-bits zetten zodat het als UUIDv4 oogt.
    let hi = (a & 0xFFFF_FFFF_FFFF_0FFF) | 0x0000_0000_0000_4000;
    let lo = (b & 0x3FFF_FFFF_FFFF_FFFF) | 0x8000_0000_0000_0000;
    format!(
        "{:08X}-{:04X}-{:04X}-{:04X}-{:012X}",
        hi >> 32,
        (hi >> 16) & 0xFFFF,
        hi & 0xFFFF,
        lo >> 48,
        lo & 0xFFFF_FFFF_FFFF
    )
}

/// Houdt precies één lopende achtergrondtaak vast en breekt een eventuele vorige
/// taak af bij vervanging. Gedeeld door de ping-loop ([`crate::commands::PingController`])
/// en de SSID-watcher ([`crate::ssid_watcher::SsidWatcher`]).
pub struct AbortableTask {
    handle: TokioMutex<Option<JoinHandle<()>>>,
}

impl AbortableTask {
    pub fn new() -> Self {
        Self {
            handle: TokioMutex::new(None),
        }
    }

    /// Vervang de lopende taak; een eventuele vorige taak wordt afgebroken.
    pub async fn replace(&self, task: JoinHandle<()>) {
        let mut guard = self.handle.lock().await;
        if let Some(old) = guard.take() {
            old.abort();
        }
        *guard = Some(task);
    }

    /// Breek de lopende taak af, indien aanwezig.
    pub async fn abort(&self) {
        let mut guard = self.handle.lock().await;
        if let Some(h) = guard.take() {
            h.abort();
        }
    }
}

impl Default for AbortableTask {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pseudo_uuid_has_uuid_shape_and_is_unique() {
        let a = pseudo_uuid();
        let b = pseudo_uuid();
        assert_ne!(a, b);
        let parts: Vec<&str> = a.split('-').collect();
        assert_eq!(parts.iter().map(|p| p.len()).collect::<Vec<_>>(), [8, 4, 4, 4, 12]);
        assert!(a.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
        assert!(parts[2].starts_with('4'));
    }
}
