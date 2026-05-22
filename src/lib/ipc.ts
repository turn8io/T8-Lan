import { invoke } from "@tauri-apps/api/core";

export type AdapterKind = "ethernet" | "wifi" | "other";
export type StatusState =
  | "idle"
  | "busy"
  | "success"
  | "error"
  | "waitingdhcp";

export type AdapterInfo = {
  friendly_name: string;
  description: string;
  luid: number;
  kind: AdapterKind;
  is_up: boolean;
  is_dhcp: boolean;
  current_ip: string | null;
  current_subnet: string | null;
  current_gateway: string | null;
  dns_servers: string[];
};

export type CurrentStatus = {
  adapter: AdapterInfo | null;
  state: StatusState;
  message: string;
};

export type RecentIp = {
  ip: string;
  label?: string | null;
  last_used?: number | null;
};

export type DnsPreset = "cloudflare" | "google" | "opendns" | "quad9" | "custom";

export const DNS_PRESET_SERVERS: Record<Exclude<DnsPreset, "custom">, [string, string]> = {
  cloudflare: ["1.1.1.1", "1.0.0.1"],
  google: ["8.8.8.8", "8.8.4.4"],
  opendns: ["208.67.222.222", "208.67.220.220"],
  quad9: ["9.9.9.9", "149.112.112.112"],
};

export type Settings = {
  version: number;
  selected_adapter: { friendly_name: string; luid: number | null } | null;
  subnet: string;
  gateway_mode: "auto-first" | "auto-last" | "manual";
  gateway_manual: string | null;
  dns: {
    apply_on_switch: boolean;
    preset: DnsPreset;
    custom_primary: string | null;
    custom_secondary: string | null;
  };
  recent_ips_by_adapter: Record<string, RecentIp[]>;
  ping: { target: string };
  window: { x: number; y: number; width: number; height: number } | null;
  global_hotkey_enabled: boolean;
  hotkey_dhcp: string;
  hotkey_static: string;
  ssid_rules: SsidRule[];
  examples_seeded: boolean;
};

export type SsidAction =
  | { type: "dhcp" }
  | { type: "static-ip"; value: string };

export type SsidRule = {
  ssid: string;
  action: SsidAction;
};

export type PingResult = {
  target: string;
  timestamp_ms: number;
  rtt_ms: number | null;
  error: string | null;
};

/** Eén ontdekt netwerkapparaat uit de netwerkscan. */
export type DeviceInfo = {
  ip: string;
  /** Camera-merk of NVR-product ("Hikvision", "Nx Witness V6.1"), of null. */
  brand: string | null;
  is_camera: boolean;
  /** Optionele derde regel (bv. paneelnaam van een alarmsysteem), of null. */
  detail: string | null;
  /** Klikbare webinterface-URL, of null als er geen webpoort open staat. */
  web_url: string | null;
};

export const ipc = {
  getAdapters: () => invoke<AdapterInfo[]>("get_adapters"),
  getCurrentStatus: (adapter_name?: string) =>
    invoke<CurrentStatus>("get_current_status", { adapterName: adapter_name }),
  switchToDhcp: (adapter_name: string) =>
    invoke<void>("switch_to_dhcp", { adapterName: adapter_name }),
  switchToStatic: (adapter_name: string, ip_addr: string) =>
    invoke<void>("switch_to_static", { adapterName: adapter_name, ipAddr: ip_addr }),
  checkIpConflict: (ip: string) =>
    invoke<string | null>("check_ip_conflict", { ip }),
  scanDevices: (subnet_base: string) =>
    invoke<void>("scan_devices", { subnetBase: subnet_base }),
  listWifiNetworks: () => invoke<string[]>("list_wifi_networks"),
  updateHotkeys: (settings: Settings) =>
    invoke<void>("update_hotkeys", { settings }),
  loadSettings: () => invoke<Settings>("load_settings"),
  saveSettings: (settings: Settings) =>
    invoke<void>("save_settings", { settings }),
  hasUndo: () => invoke<PreviousConfig | null>("has_undo"),
  hasRedo: () => invoke<PreviousConfig | null>("has_redo"),
  undoLastSwitch: () => invoke<PreviousConfig>("undo_last_switch"),
  redoLastSwitch: () => invoke<PreviousConfig>("redo_last_switch"),
  ssidWatcherSetEnabled: (enabled: boolean) =>
    invoke<void>("ssid_watcher_set_enabled", { enabled }),
  pingStart: (target: string) => invoke<void>("ping_start", { target }),
  pingStop: () => invoke<void>("ping_stop"),
  dnsPing: (target: string) => invoke<PingResult>("dns_ping", { target }),
  openExternal: (url: string) => invoke<void>("open_external", { url }),
  saveWindowPosition: () => invoke<void>("save_window_position"),
};

export type PreviousConfig = {
  adapter_name: string;
  was_dhcp: boolean;
  ip: string | null;
  subnet: string | null;
  gateway: string | null;
};

export type SwitchResultEvent = {
  ok: boolean;
  msg: string;
  trigger?: string;
};

/** Effective [primary, secondary] DNS for the configured preset. */
export function dnsServers(settings: Settings): [string, string] {
  const d = settings.dns;
  if (d.preset !== "custom") return DNS_PRESET_SERVERS[d.preset];
  return [d.custom_primary ?? "", d.custom_secondary ?? ""];
}

/** Primary DNS server IP for the configured preset (used for the health ping). */
export function dnsPrimary(settings: Settings): string | null {
  const [primary] = dnsServers(settings);
  return primary && isValidIpv4(primary) ? primary : null;
}

export function isValidIpv4(s: string): boolean {
  const parts = s.split(".");
  if (parts.length !== 4) return false;
  return parts.every((p) => {
    const n = Number(p);
    return Number.isInteger(n) && n >= 0 && n <= 255 && String(n) === p;
  });
}
