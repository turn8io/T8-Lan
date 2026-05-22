import { useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useStore, flash } from "../store";
import { ipc, type DeviceInfo } from "../lib/ipc";
import { t } from "../lib/i18n";
import DeviceList from "../components/DeviceList";

/**
 * Netwerkscan: één knop bovenaan die tegelijk de live status toont (voortgang +
 * aantal gevonden apparaten/camera's). Resultaten stromen live binnen via events.
 */
export default function NetworkTab() {
  const status = useStore((s) => s.status);
  // Scan results live in the store so they survive navigating to another tab and back.
  const devices = useStore((s) => s.scanDevices);
  const setDevices = useStore((s) => s.setScanDevices);
  const scanned = useStore((s) => s.scanDone);
  const setScanned = useStore((s) => s.setScanDone);
  const pct = useStore((s) => s.scanPct);
  const setPct = useStore((s) => s.setScanPct);
  const resetScan = useStore((s) => s.resetScan);
  const [scanning, setScanning] = useState(false);
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
    const baseIp = status?.adapter?.current_ip;
    if (!baseIp) {
      flash(t("network.noCurrentIp"), "warn");
      return;
    }
    resetScan();
    setScanned(true);
    setScanning(true);
    try {
      await ipc.scanDevices(baseIp);
    } catch (e) {
      flash(String(e), "err");
      setScanning(false);
    }
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
        <button
          className="btn btn--primary btn--full btn--compact network-scan-btn"
          disabled={scanning}
          onClick={onScan}
        >
          {label}
        </button>
      </div>

      {scanned && !scanning && devices.length === 0 && (
        <p className="hint">{t("network.noDevices")}</p>
      )}

      <DeviceList devices={devices} />
    </section>
  );
}

/** Kies het meest specifieke merk-label: een met model (bevat een spatie) wint van een
 *  kaal merk; bij gelijke specificiteit de langste. */
function pickBrand(a: string | null, b: string | null): string | null {
  if (!a) return b;
  if (!b) return a;
  const aHasModel = a.includes(" ");
  const bHasModel = b.includes(" ");
  if (aHasModel !== bHasModel) return aHasModel ? a : b;
  return b.length > a.length ? b : a;
}

/** Voeg twee meldingen voor hetzelfde IP samen tot de rijkste variant. */
function mergeDevice(a: DeviceInfo, b: DeviceInfo): DeviceInfo {
  return {
    ip: a.ip,
    brand: pickBrand(a.brand, b.brand),
    is_camera: a.is_camera || b.is_camera,
    detail: a.detail ?? b.detail,
    web_url: a.web_url ?? b.web_url,
  };
}
