import { create } from "zustand";
import type { Settings, CurrentStatus, DeviceInfo, DhcpStatus } from "./lib/ipc";

export type Flash = { msg: string; kind: "ok" | "err" | "warn" } | null;
export type SwitchPulse = { kind: "ok" | "err"; at: number } | null;

export type PingRequest = { ip: string; at: number } | null;

/** Een openstaande bevestigingsvraag (huisstijl-dialoog i.p.v. window.confirm). */
export type ConfirmRequest = {
  title: string;
  body: string;
  confirmLabel: string;
  cancelLabel: string;
  /** Rode, "enge" variant voor riskante acties. */
  danger?: boolean;
  resolve: (ok: boolean) => void;
} | null;

type State = {
  settings: Settings | null;
  status: CurrentStatus | null;
  flash: Flash;
  switchPulse: SwitchPulse;
  dnsAlive: boolean | null;
  dnsRtt: number | null;
  pingRequest: PingRequest;
  // Network-scan results persist across tab switches so they survive navigating away.
  scanDevices: DeviceInfo[];
  scanDone: boolean;
  scanPct: number;
  dhcpStatus: DhcpStatus | null;
  confirmReq: ConfirmRequest;
  setSettings: (s: Settings) => void;
  setStatus: (s: CurrentStatus) => void;
  setFlash: (f: Flash) => void;
  setSwitchPulse: (p: SwitchPulse) => void;
  setDnsHealth: (alive: boolean | null, rtt: number | null) => void;
  requestPing: (ip: string) => void;
  clearPingRequest: () => void;
  setScanDevices: (updater: DeviceInfo[] | ((prev: DeviceInfo[]) => DeviceInfo[])) => void;
  setScanDone: (v: boolean) => void;
  setScanPct: (v: number) => void;
  resetScan: () => void;
  setDhcpStatus: (s: DhcpStatus | null) => void;
  setConfirmReq: (r: ConfirmRequest) => void;
};

export const useStore = create<State>((set) => ({
  settings: null,
  status: null,
  flash: null,
  switchPulse: null,
  dnsAlive: null,
  dnsRtt: null,
  pingRequest: null,
  scanDevices: [],
  scanDone: false,
  scanPct: 0,
  dhcpStatus: null,
  confirmReq: null,
  setSettings: (settings) => set({ settings }),
  setStatus: (status) => set({ status }),
  setFlash: (flash) => set({ flash }),
  setSwitchPulse: (switchPulse) => set({ switchPulse }),
  setDnsHealth: (dnsAlive, dnsRtt) => set({ dnsAlive, dnsRtt }),
  requestPing: (ip) => set({ pingRequest: { ip, at: Date.now() } }),
  clearPingRequest: () => set({ pingRequest: null }),
  setScanDevices: (updater) =>
    set((s) => ({
      scanDevices: typeof updater === "function" ? updater(s.scanDevices) : updater,
    })),
  setScanDone: (scanDone) => set({ scanDone }),
  setScanPct: (scanPct) => set({ scanPct }),
  resetScan: () => set({ scanDevices: [], scanDone: false, scanPct: 0 }),
  setDhcpStatus: (dhcpStatus) => set({ dhcpStatus }),
  setConfirmReq: (confirmReq) => set({ confirmReq }),
}));

let flashTimer: number | undefined;
/** Show a transient in-app status message (auto-clears). */
export function flash(msg: string, kind: "ok" | "err" | "warn" = "ok") {
  useStore.getState().setFlash({ msg, kind });
  if (flashTimer) window.clearTimeout(flashTimer);
  flashTimer = window.setTimeout(() => useStore.getState().setFlash(null), 3500);
}

/**
 * Vraag de gebruiker om bevestiging via de huisstijl-dialoog (<ConfirmHost/> in App).
 * Resolvet met true (bevestigd) of false (geannuleerd / Escape / klik buiten).
 */
export function confirmDialog(opts: Omit<NonNullable<ConfirmRequest>, "resolve">): Promise<boolean> {
  return new Promise((resolve) => {
    const prev = useStore.getState().confirmReq;
    if (prev) prev.resolve(false);
    useStore.getState().setConfirmReq({ ...opts, resolve });
  });
}
