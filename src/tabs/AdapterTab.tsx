import { useEffect, useMemo, useState } from "react";
import { useStore, flash } from "../store";
import { ipc, isValidIpv4, type AdapterInfo, type RecentIp } from "../lib/ipc";
import { applyStaticIp, applyDhcp } from "../lib/actions";
import { copyIp } from "../lib/toast";
import { t } from "../lib/i18n";
import Select from "../components/Select";
import IpCombobox from "../components/IpCombobox";

// Example entries seeded once into a fresh adapter's recents. They behave like
// any other recent (the user can rename or delete them permanently).
const EXAMPLE_IPS: RecentIp[] = [
  { ip: "192.168.254.88", label: "Hik NVR" },
  { ip: "192.168.1.88", label: "Hik Cam" },
];

export default function AdapterTab() {
  const status = useStore((s) => s.status);
  const setStatus = useStore((s) => s.setStatus);
  const settings = useStore((s) => s.settings);
  const setSettings = useStore((s) => s.setSettings);
  const switchPulse = useStore((s) => s.switchPulse);

  const [adapters, setAdapters] = useState<AdapterInfo[]>([]);
  const [ipInput, setIpInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [pulseClass, setPulseClass] = useState("");
  const [copiedKey, setCopiedKey] = useState<"ip" | "subnet" | "gw" | null>(null);
  const [editingIp, setEditingIp] = useState(false);
  const [ipDraft, setIpDraft] = useState("");
  const [editingSubnet, setEditingSubnet] = useState(false);
  const [subnetDraft, setSubnetDraft] = useState("");
  const [editingGw, setEditingGw] = useState(false);
  const [gwDraft, setGwDraft] = useState("");

  const selectedName = settings?.selected_adapter?.friendly_name ?? null;
  const isDhcp = status?.adapter?.is_dhcp ?? false;
  const currentIp = status?.adapter?.current_ip ?? null;
  const gwMode = settings?.gateway_mode ?? "auto-first";

  useEffect(() => {
    if (!switchPulse) return;
    // Only pulse for a fresh switch result — not when this tab is re-mounted
    // (e.g. switching back from another tab) with a stale pulse still in store.
    if (Date.now() - switchPulse.at > 1500) return;
    setPulseClass(switchPulse.kind === "ok" ? "status-card--ok" : "status-card--err");
    const timer = setTimeout(() => setPulseClass(""), 1300);
    return () => clearTimeout(timer);
  }, [switchPulse?.at, switchPulse?.kind]);

  const recents: RecentIp[] = useMemo(() => {
    if (!settings || !selectedName) return [];
    return settings.recent_ips_by_adapter[selectedName] ?? [];
  }, [settings, selectedName]);

  // First run: auto-select a default adapter (active Ethernet, else first active,
  // else first) so the user lands on a working adapter — and so the example IPs
  // below get seeded into its recents.
  useEffect(() => {
    if (!settings || selectedName || adapters.length === 0) return;
    const pick =
      adapters.find((a) => a.kind === "ethernet" && a.is_up) ||
      adapters.find((a) => a.is_up) ||
      adapters[0];
    if (pick) {
      persistSettings({
        ...settings,
        selected_adapter: { friendly_name: pick.friendly_name, luid: null },
      });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settings, selectedName, adapters]);

  // Seed the two example IPs once (so they appear initially but can be deleted
  // permanently afterwards), and backfill their labels if a still-present
  // example lost its name in an earlier version. Both steps are idempotent.
  useEffect(() => {
    if (!settings || !selectedName) return;
    const list = settings.recent_ips_by_adapter[selectedName] ?? [];
    const next = [...list];
    let changed = false;

    // Seed missing examples (only the very first time for this install).
    if (!settings.examples_seeded) {
      for (const ex of EXAMPLE_IPS) {
        if (!next.some((r) => r.ip === ex.ip)) {
          next.push(ex);
          changed = true;
        }
      }
    }

    // Restore the label on an example that is present but unnamed (don't touch
    // a label the user set themselves).
    for (let i = 0; i < next.length; i++) {
      const ex = EXAMPLE_IPS.find((e) => e.ip === next[i].ip);
      if (ex && !next[i].label) {
        next[i] = { ...next[i], label: ex.label };
        changed = true;
      }
    }

    if (changed || !settings.examples_seeded) {
      persistSettings({
        ...settings,
        examples_seeded: true,
        recent_ips_by_adapter: { ...settings.recent_ips_by_adapter, [selectedName]: next },
      });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settings, selectedName]);

  const gateways = useMemo(
    () => computeGateways(currentIp, settings?.subnet ?? "255.255.255.0"),
    [currentIp, settings?.subnet],
  );

  const refresh = () => {
    ipc.getAdapters().then(setAdapters).catch(console.error);
    ipc.getCurrentStatus(selectedName ?? undefined).then(setStatus).catch(console.error);
    ipc.loadSettings().then(setSettings).catch(console.error);
  };

  useEffect(() => {
    refresh();
    const id = setInterval(() => {
      ipc.getCurrentStatus(selectedName ?? undefined).then(setStatus).catch(() => {});
    }, 3000);
    return () => clearInterval(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedName]);

  const persistSettings = async (next: NonNullable<typeof settings>) => {
    setSettings(next);
    await ipc.saveSettings(next).catch(console.error);
  };

  const adapterOptions = adapters.map((a) => ({
    value: a.friendly_name,
    label: `${a.kind === "wifi" ? "📶" : "🔌"} ${a.friendly_name}${a.is_up ? "" : ` ${t("adapter.down")}`}`,
  }));

  const onSelectAdapter = async (friendlyName: string) => {
    if (!settings) return;
    await persistSettings({
      ...settings,
      selected_adapter: { friendly_name: friendlyName, luid: null },
    });
  };

  const ipValid = ipInput === "" || isValidIpv4(ipInput);

  // Give the adapter a moment to reflect the change, then pull fresh status so
  // the tile fades back in showing the NEW values (not the old ones).
  const settleAndRefresh = async () => {
    await new Promise((r) => setTimeout(r, 350));
    await ipc.getCurrentStatus(selectedName ?? undefined).then(setStatus).catch(() => {});
    await ipc.loadSettings().then(setSettings).catch(() => {});
  };

  const onApplyStatic = async (ip: string) => {
    if (!selectedName) return;
    setBusy(true);
    try {
      await applyStaticIp(selectedName, ip);
      setIpInput("");
      await settleAndRefresh();
    } catch (e) {
      if (String(e) !== "Error: cancelled") flash(String(e), "err");
    } finally {
      setBusy(false);
    }
  };

  const onApplyDhcp = async () => {
    if (!selectedName) return;
    setBusy(true);
    try {
      await applyDhcp(selectedName);
      await settleAndRefresh();
    } catch (e) {
      flash(String(e), "err");
    } finally {
      setBusy(false);
    }
  };

  const copyVal = async (key: "ip" | "subnet" | "gw", val: string | null) => {
    if (!val || val === "—") return;
    try {
      await copyIp(val);
    } catch {
      /* ignore */
    }
    setCopiedKey(key);
    window.setTimeout(() => setCopiedKey((k) => (k === key ? null : k)), 1000);
  };

  // Re-apply the current static IP so changed subnet/gateway/DNS take effect on
  // the live adapter. do_static reads the freshly-saved settings.
  const reapplyStatic = async () => {
    if (!selectedName || !currentIp) return;
    setBusy(true);
    try {
      await ipc.switchToStatic(selectedName, currentIp);
      await settleAndRefresh();
    } catch (e) {
      flash(String(e), "err");
    } finally {
      setBusy(false);
    }
  };

  const isLast = gwMode === "auto-last";
  const gwValue =
    gwMode === "manual"
      ? settings?.gateway_manual ?? "—"
      : gateways
        ? isLast
          ? gateways.last
          : gateways.first
        : "—";

  const toggleGateway = async () => {
    if (!settings) return;
    await persistSettings({ ...settings, gateway_mode: isLast ? "auto-first" : "auto-last" });
    if (!isDhcp) await reapplyStatic();
  };

  const subnet = settings?.subnet ?? "255.255.255.0";

  // What to show per row depends on mode: DHCP shows the live adapter values
  // (read-only, click-to-copy); static shows the configured values (editable).
  const subnetShown = isDhcp ? status?.adapter?.current_subnet ?? "—" : subnet;
  const gwShown = isDhcp ? status?.adapter?.current_gateway ?? "—" : gwValue;

  const saveSubnet = async () => {
    setEditingSubnet(false);
    const v = subnetDraft.trim();
    if (!settings || !isValidIpv4(v)) return;
    if (v !== settings.subnet) await persistSettings({ ...settings, subnet: v });
    if (!isDhcp) await reapplyStatic();
  };

  const saveGateway = async () => {
    setEditingGw(false);
    const v = gwDraft.trim();
    if (!settings || !isValidIpv4(v)) return;
    await persistSettings({ ...settings, gateway_mode: "manual", gateway_manual: v });
    if (!isDhcp) await reapplyStatic();
  };

  const onApplyIpEdit = async () => {
    setEditingIp(false);
    const v = ipDraft.trim();
    if (!isValidIpv4(v) || v === currentIp) return;
    await onApplyStatic(v);
  };

  const editLabel = async (ip: string, label: string) => {
    if (!settings || !selectedName) return;
    const next = (settings.recent_ips_by_adapter[selectedName] ?? []).map((r) =>
      r.ip === ip ? { ...r, label: label || null } : r,
    );
    await persistSettings({
      ...settings,
      recent_ips_by_adapter: { ...settings.recent_ips_by_adapter, [selectedName]: next },
    });
  };

  const removeRecent = async (ip: string) => {
    if (!settings || !selectedName) return;
    const next = (settings.recent_ips_by_adapter[selectedName] ?? []).filter((r) => r.ip !== ip);
    await persistSettings({
      ...settings,
      recent_ips_by_adapter: { ...settings.recent_ips_by_adapter, [selectedName]: next },
    });
  };

  return (
    <section className="tab-content">
      {/* Network adapter tile */}
      <div className="tile">
        <span className="tile__label">{t("adapter.card")}</span>
        <div className="tile__control">
          <Select
            value={selectedName ?? ""}
            options={adapterOptions}
            onChange={onSelectAdapter}
            placeholder={t("adapter.choose")}
          />
        </div>
      </div>

      {/* Status tile */}
      <div className={`status-card ${pulseClass}`}>
        {busy && <span className="card-spinner" aria-label="bezig…" />}
        <div className={`status-card__body ${busy ? "is-fading" : ""}`}>
        {/* IP — DHCP: read-only + copy, Static: editable */}
        <div className="status-card__row">
          <span className="status-line__key">{isDhcp ? "DHCP IP" : "Static IP"}</span>
          {!isDhcp && editingIp ? (
            <input
              autoFocus
              className="status-edit mono"
              value={ipDraft}
              placeholder="192.168.1.100"
              onChange={(e) => setIpDraft(e.target.value)}
              onBlur={onApplyIpEdit}
              onKeyDown={(e) => {
                if (e.key === "Enter") (e.target as HTMLInputElement).blur();
                if (e.key === "Escape") setEditingIp(false);
              }}
            />
          ) : (
            <button
              type="button"
              className={`status-edit-val mono ${isDhcp ? "status-edit-val--copy" : "status-edit-val--edit"}`}
              disabled={isDhcp ? !currentIp : !selectedName}
              onMouseDown={isDhcp ? () => copyVal("ip", currentIp) : undefined}
              onClick={
                isDhcp
                  ? undefined
                  : () => {
                      setIpDraft(currentIp ?? "");
                      setEditingIp(true);
                    }
              }
            >
              {currentIp ?? "—"}
              {copiedKey === "ip" && <span className="tooltip tooltip--bottom">Copied</span>}
            </button>
          )}
        </div>

        {/* Subnet */}
        <div className="status-card__row">
          <span className="status-line__key">{t("adapter.subnet")}</span>
          {!isDhcp && editingSubnet ? (
            <input
              autoFocus
              className="status-edit mono"
              value={subnetDraft}
              onChange={(e) => setSubnetDraft(e.target.value)}
              onBlur={saveSubnet}
              onKeyDown={(e) => {
                if (e.key === "Enter") (e.target as HTMLInputElement).blur();
                if (e.key === "Escape") setEditingSubnet(false);
              }}
            />
          ) : (
            <button
              type="button"
              className={`status-edit-val mono ${isDhcp ? "status-edit-val--copy" : "status-edit-val--edit"}`}
              onMouseDown={isDhcp ? () => copyVal("subnet", subnetShown) : undefined}
              onClick={
                isDhcp
                  ? undefined
                  : () => {
                      setSubnetDraft(subnet);
                      setEditingSubnet(true);
                    }
              }
            >
              {subnetShown}
              {copiedKey === "subnet" && <span className="tooltip tooltip--bottom">Copied</span>}
            </button>
          )}
        </div>

        {/* Gateway */}
        <div className="status-card__row">
          <span className="gw-keyline">
            <span className="status-line__key">{t("adapter.gateway")}</span>
            {!isDhcp && (
              <button
                type="button"
                className="gw-arrow"
                aria-label="Gateway .1 / .254"
                disabled={!gateways}
                onClick={toggleGateway}
              >
                {isLast ? <ArrowDown /> : <ArrowUp />}
              </button>
            )}
          </span>
          {!isDhcp && editingGw ? (
            <input
              autoFocus
              className="status-edit mono"
              value={gwDraft}
              onChange={(e) => setGwDraft(e.target.value)}
              onBlur={saveGateway}
              onKeyDown={(e) => {
                if (e.key === "Enter") (e.target as HTMLInputElement).blur();
                if (e.key === "Escape") setEditingGw(false);
              }}
            />
          ) : (
            <button
              type="button"
              className={`status-edit-val mono ${isDhcp ? "status-edit-val--copy" : "status-edit-val--edit"}`}
              onMouseDown={isDhcp ? () => copyVal("gw", gwShown) : undefined}
              onClick={
                isDhcp
                  ? undefined
                  : () => {
                      setGwDraft(gwShown === "—" ? "" : gwShown);
                      setEditingGw(true);
                    }
              }
            >
              {gwShown}
              {copiedKey === "gw" && <span className="tooltip tooltip--bottom">Copied</span>}
            </button>
          )}
        </div>
        </div>
      </div>

      {/* Static IP */}
      <div className="tile">
        <span className="tile__label">{t("adapter.setStatic")}</span>
        <div className="tile__control">
        <IpCombobox
          value={ipInput}
          onChange={setIpInput}
          recents={recents}
          onApply={onApplyStatic}
          onEditLabel={editLabel}
          onRemove={removeRecent}
          valid={ipValid}
          disabled={busy || !selectedName}
        />
        </div>
      </div>

      {!isDhcp && (
        <button
          className="btn btn--ghost btn--full"
          disabled={busy || !selectedName}
          onClick={onApplyDhcp}
        >
          {t("adapter.toDhcp")}
        </button>
      )}
    </section>
  );
}

function computeGateways(ip: string | null, subnet: string): { first: string; last: string } | null {
  if (!ip) return null;
  const ipP = ip.split(".").map(Number);
  const mP = subnet.split(".").map(Number);
  if (ipP.length !== 4 || mP.length !== 4 || [...ipP, ...mP].some(isNaN)) return null;
  const ipN = ((ipP[0] << 24) | (ipP[1] << 16) | (ipP[2] << 8) | ipP[3]) >>> 0;
  const mN = ((mP[0] << 24) | (mP[1] << 16) | (mP[2] << 8) | mP[3]) >>> 0;
  const net = (ipN & mN) >>> 0;
  const hostBits = (~mN) >>> 0;
  const firstN = (net | 1) >>> 0;
  const lastN = (net | (hostBits - 1)) >>> 0;
  const fmt = (n: number) => [(n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255].join(".");
  return { first: fmt(firstN), last: fmt(lastN) };
}

const ArrowUp = () => (
  <svg viewBox="0 0 12 12" width="10" height="10" fill="none" stroke="currentColor" strokeWidth="1.6">
    <path d="M6 9.5 V2.5 M3 5.5 L6 2.5 L9 5.5" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
);
const ArrowDown = () => (
  <svg viewBox="0 0 12 12" width="10" height="10" fill="none" stroke="currentColor" strokeWidth="1.6">
    <path d="M6 2.5 V9.5 M3 6.5 L6 9.5 L9 6.5" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
);
