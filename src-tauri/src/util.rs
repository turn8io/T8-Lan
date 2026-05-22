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
