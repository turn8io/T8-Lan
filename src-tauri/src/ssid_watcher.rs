use crate::network::wifi;
use crate::settings::{self, SsidAction};
use crate::switching;
use std::sync::Arc;
use std::time::Duration;
use tauri::AppHandle;
use tokio::time::interval;

pub struct SsidWatcher {
    task: crate::util::AbortableTask,
}

impl SsidWatcher {
    pub fn new() -> Self {
        Self {
            task: crate::util::AbortableTask::new(),
        }
    }
}

pub async fn start(app: AppHandle, watcher: Arc<SsidWatcher>) {
    let app_handle = app.clone();
    let handle = tauri::async_runtime::spawn(async move {
        let mut last_ssid: Option<String> = None;
        let mut tick = interval(Duration::from_secs(5));
        tick.tick().await;
        if let Ok(ssids) = wifi::current_ssids() {
            last_ssid = ssids.into_iter().next();
        }

        loop {
            tick.tick().await;
            let current = wifi::current_ssids().ok().and_then(|s| s.into_iter().next());
            if current != last_ssid {
                if let Some(ssid) = current.clone() {
                    apply_ssid_rule(&app_handle, &ssid);
                }
                last_ssid = current;
            }
        }
    });

    watcher.task.replace(handle).await;
}

pub async fn stop(watcher: Arc<SsidWatcher>) {
    watcher.task.abort().await;
}

fn apply_ssid_rule(app: &AppHandle, ssid: &str) {
    let settings = match settings::load(app) {
        Ok(s) => s,
        Err(_) => return,
    };
    let Some(rule) = settings.ssid_rules.iter().find(|r| r.ssid == ssid).cloned() else {
        return;
    };
    let Some(adapter_ref) = settings.selected_adapter.clone() else {
        return;
    };

    let app = app.clone();
    let name = adapter_ref.friendly_name;
    std::thread::spawn(move || match &rule.action {
        SsidAction::Dhcp => switching::do_dhcp(&app, &name, "ssid"),
        SsidAction::StaticIp(ip) => switching::do_static(&app, &name, ip, "ssid"),
    });
}
