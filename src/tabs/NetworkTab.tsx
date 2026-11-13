import { useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useStore, flash } from "../store";
import { ipc, type DeviceInfo } from "../lib/ipc";
import { t, type TKey } from "../lib/i18n";
import DeviceList from "../components/DeviceList";

/**
 * Netwerkscan: één knop bovenaan die tegelijk de live status toont (voortgang +
 * aantal gevonden apparaten/camera's). Resultaten stromen live binnen via events.
 */
export default function NetworkTab() {
  const status = useStore((s) => s.status);
  const settings = useStore((s) => s.settings);
  const setSettings = useStore((s) => s.setSettings);
  // Scan results live in the store so they survive navigating to another tab and back.
  const devices = useStore((s) => s.scanDevices);
  const setDevices = useStore((s) => s.setScanDevices);
  const scanned = useStore((s) => s.scanDone);
  const setScanned = useStore((s) => s.setScanDone);
  const pct = useStore((s) => s.scanPct);
  const setPct = useStore((s) => s.setScanPct);
  const resetScan = useStore((s) => s.resetScan);
  const warnings = useStore((s) => s.scanWarnings);
  const addWarning = useStore((s) => s.addScanWarning);
  const target = useStore((s) => s.scanTarget);
  const setTarget = useStore((s) => s.setScanTarget);
  const [scanning, setScanning] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [draft, setDraft] = useState("");
  const ranges = settings?.scan_ranges ?? [];
  const unlistenRef = useRef<UnlistenFn[]>([]);

  useEffect(() => {
    let cancelled = false;
    Promise.all([
      listen<DeviceInfo>("scan-device", (e) => {
        if (cancelled) return;
        setDevices((prev) => {
          const i = prev.findIndex((d) => d.ip === e.payload.ip);
          if (i === -1) return [...prev, e.payload];
          // Zelfde IP kan via ARP én SADP binnenkomen: samenvoegen i.p.v. negeren,
          // zodat de rijkere modelinfo (bv. "Hikvision DS-...") de bare "Hikvision" wint.
          const copy = prev.slice();
          copy[i] = mergeDevice(prev[i], e.payload);
          return copy;
        });
      }),
      listen<[number, number]>("scan-progress", (e) => {
        if (cancelled) return;
        const [done, total] = e.payload;
        if (total > 0) setPct(Math.round((done / total) * 100));
      }),
      listen<string>("scan-warning", (e) => {
        if (!cancelled) addWarning(e.payload);
      }),
      listen("scan-done", () => {
        if (!cancelled) {
          setScanning(false);
          setPct(100);
        }
      }),
    ]).then((uns) => {
      unlistenRef.current = uns;
    });
    return () => {
      cancelled = true;
      unlistenRef.current.forEach((un) => un());
    };
  }, []);

  const onScan = async () => {
    const adapter = status?.adapter;
    const baseIp = adapter?.current_ip;
    if (!adapter || !baseIp) {
      flash(t("network.noCurrentIp"), "warn");
      return;
    }
    resetScan();
    setScanned(true);
    setScanning(true);
    try {
      await ipc.scanDevices(baseIp, adapter.current_subnet, target.trim() || null);
    } catch (e) {
      flash(String(e), "err");
      setScanning(false);
    }
  };

  const saveRanges = async (next: string[]) => {
    if (!settings) return;
    const updated = { ...settings, scan_ranges: next };
    setSettings(updated);
    await ipc.saveSettings(updated).catch((e) => flash(String(e), "err"));
  };

  const choose = (range: string) => {
    setTarget(range);
    setMenuOpen(false);
  };

  // Een nieuw bereik bewaren en meteen kiezen. De backend controleert het bereik bij de
  // scan nog eens; hier vangen we alleen de tikfout af vóór die in de lijst staat.
  const addRange = async () => {
    const range = draft.replace(/\s+/g, "");
    if (!RANGE_PATTERN.test(range)) {
      flash(t("network.rangeInvalid"), "warn");
      return;
    }
    if (!ranges.includes(range)) await saveRanges([...ranges, range]);
    setDraft("");
    choose(range);
  };

  const removeRange = async (range: string) => {
    await saveRanges(ranges.filter((r) => r !== range));
    if (target === range) setTarget("");
  };

  const cameraCount = devices.filter((d) => d.is_camera).length;

  let label: string;
  if (scanning) {
    label = `${t("network.scanning")} ${pct}% · ${devices.length} / ${cameraCount} ${t("network.camsShort")}`;
  } else if (scanned) {
    label = `${devices.length} ${t("network.devices")} · ${cameraCount} ${t("network.camsShort")}`;
  } else {
    label = t("network.scan");
  }

  return (
    <section className="tab-content">
      <div className="network-scan-bar">
        <div className="scan-split">
          <button
            className="btn btn--primary btn--full btn--compact network-scan-btn"
            disabled={scanning}
            onClick={onScan}
          >
            {label}
          </button>
          <button
            className="btn btn--primary btn--compact scan-split__caret"
            aria-label={t("network.chooseRange")}
            aria-expanded={menuOpen}
            disabled={scanning}
            onClick={() => setMenuOpen((open) => !open)}
          >
            <Caret />
          </button>
        </div>
        {target && !menuOpen && <span className="scan-range mono">{target}</span>}

        {menuOpen && (
          <div className="scan-menu">
            <button
              className={`cselect__opt ${target === "" ? "is-selected" : ""}`}
              onClick={() => choose("")}
            >
              {t("network.ownSubnet")}
            </button>
            {ranges.map((range) => (
              <div key={range} className="scan-menu__row">
                <button
                  className={`cselect__opt mono ${target === range ? "is-selected" : ""}`}
                  onClick={() => choose(range)}
                >
                  {range}
                </button>
                <button
                  className="scan-menu__remove"
                  aria-label={t("adapter.remove")}
                  onClick={() => removeRange(range)}
                >
                  ×
                </button>
              </div>
            ))}
            <div className="scan-menu__row">
              <input
                className="input mono"
                value={draft}
                placeholder="192.168.101.1-254"
                spellCheck={false}
                autoFocus
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") addRange();
                }}
              />
              <button className="btn btn--ghost btn--compact" onClick={addRange}>
                {t("network.rangeAdd")}
              </button>
            </div>
          </div>
        )}
      </div>

      {warnings.map((code) => (
        <p key={code} className={code === "routed" ? "hint" : "hint hint--err"}>
          {t(`network.warn.${code}` as TKey)}
        </p>
      ))}

      {scanned && !scanning && devices.length === 0 && (
        <p className="hint">{t("network.noDevices")}</p>
      )}

      <DeviceList devices={devices} />
    </section>
  );
}

/** "192.168.101.1-254", "192.168.101.0/24", "192.168.101.0" of "192.168.101". */
const RANGE_PATTERN = /^\d{1,3}\.\d{1,3}\.\d{1,3}(\.\d{1,3}(-\d{1,3}|\/\d{1,2})?)?$/;

const Caret = () => (
  <svg viewBox="0 0 12 12" width="10" height="10" fill="currentColor" aria-hidden="true">
    <path d="M2 4 L10 4 L6 9 Z" />
  </svg>
);

/** Voeg twee meldingen voor hetzelfde IP samen (ARP, SADP en ONVIF melden elk apart).
 *  Per veld wint de bron met de hoogste rang; de andere vult aan wat ontbreekt. */
function mergeDevice(a: DeviceInfo, b: DeviceInfo): DeviceInfo {
  const [hi, lo] = b.rank > a.rank ? [b, a] : [a, b];
  return {
    ip: a.ip,
    brand: hi.brand ?? lo.brand,
    model: hi.model ?? lo.model,
    rank: hi.rank,
    is_camera: a.is_camera || b.is_camera,
    detail: hi.detail ?? lo.detail,
    web_url: hi.web_url ?? lo.web_url,
  };
}
