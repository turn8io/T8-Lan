import { useEffect, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useStore, flash, confirmDialog } from "../store";
import {
  ipc,
  isValidIpv4,
  DHCP_DEFAULTS,
  type DhcpConfig,
  type DhcpStatus,
  type DhcpLease,
  type HotspotCard,
} from "../lib/ipc";
import { copyIp } from "../lib/toast";
import { t } from "../lib/i18n";
import Select from "../components/Select";
import Toggle from "../components/Toggle";
import Tooltip from "../components/Tooltip";
import { IconCopy, IconPing } from "../components/icons";

/**
 * DHCP-tab: een kleine DHCP-server op de gekozen adapter, voor netwerken zonder DHCP of
 * een apparaat aan een directe kabel. Inschakelen gaat altijd via een waarschuwing.
 * Daaronder de WiFi-hotspot, waarop dezelfde DHCP-server standaard meedraait maar ook
 * uit kan (apparaten met een vast IP); de hotspot houdt dan zelf het server-IP.
 */
export default function DhcpTab() {
  const settings = useStore((s) => s.settings);
  const setSettings = useStore((s) => s.setSettings);
  const dhcp = useStore((s) => s.dhcpStatus);
  const setDhcp = useStore((s) => s.setDhcpStatus);
  const requestPing = useStore((s) => s.requestPing);
  const [busy, setBusy] = useState(false);
  const [editing, setEditing] = useState<keyof DhcpConfig | null>(null);
  const [draft, setDraft] = useState("");
  const [cards, setCards] = useState<HotspotCard[]>([]);

  const running = dhcp?.running ?? false;
  const hotspotOn = dhcp?.hotspot != null;
  const viaHotspot = running && hotspotOn;
  // Zolang er iets draait zijn de velden in gebruik en niet te bewerken.
  const locked = running || hotspotOn;
  const adapterName = settings?.selected_adapter?.friendly_name ?? null;
  const cfg: DhcpConfig = { ...DHCP_DEFAULTS, ...(settings?.dhcp ?? {}) };

  useEffect(() => {
    let cancelled = false;
    let unlisten: UnlistenFn | undefined;
    ipc.dhcpStatus().then((s) => !cancelled && setDhcp(s)).catch(() => {});
    ipc.hotspotCards().then((c) => !cancelled && setCards(c)).catch(() => {});
    listen<DhcpStatus>("dhcp-status", (e) => {
      if (!cancelled) setDhcp(e.payload);
    }).then((u) => {
      unlisten = u;
    });
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, [setDhcp]);

  const persist = async (next: DhcpConfig) => {
    if (!settings) return;
    const s = { ...settings, dhcp: next };
    setSettings(s);
    await ipc.saveSettings(s).catch(console.error);
  };

  const start = async () => {
    if (!adapterName) {
      flash(t("dhcp.noAdapter"), "warn");
      return;
    }
    const ok = await confirmDialog({
      title: t("dhcp.warnTitle"),
      body: t("dhcp.warnBody").replace("{adapter}", adapterName),
      confirmLabel: t("dhcp.warnConfirm"),
      cancelLabel: t("common.cancel"),
      danger: true,
    });
    if (!ok) return;
    setBusy(true);
    try {
      const s = await ipc.dhcpStart(adapterName, cfg);
      setDhcp(s);
      flash(t("dhcp.started"), "ok");
    } catch (e) {
      flash(String(e), "err");
    } finally {
      setBusy(false);
    }
  };

  const stop = async () => {
    setBusy(true);
    try {
      const s = await ipc.dhcpStop();
      setDhcp(s);
      flash(t("dhcp.stopped"), "ok");
    } catch (e) {
      flash(String(e), "err");
    } finally {
      setBusy(false);
    }
  };

  // Op de hotspot is de DHCP-server een keuze: standaard mee, maar uit te zetten voor
  // apparaten met een vast IP. Geïsoleerd netwerk, dus geen waarschuwing; de keuze wordt
  // onthouden voor de volgende start.
  const toggleDhcpOnHotspot = async () => {
    const hotspotAdapter = dhcp?.hotspot?.adapter;
    if (!hotspotAdapter) return;
    setBusy(true);
    try {
      const s = running ? await ipc.dhcpStop() : await ipc.dhcpStart(hotspotAdapter, cfg);
      setDhcp(s);
      await persist({ ...cfg, hotspot_dhcp: !running });
      flash(t(running ? "dhcp.stopped" : "dhcp.started"), "ok");
    } catch (e) {
      flash(String(e), "err");
    } finally {
      setBusy(false);
    }
  };

  // Hotspot: eigen WiFi-netwerk met (standaard) de DHCP-server op de virtuele adapter.
  // Geïsoleerd netwerk, dus geen "enge" waarschuwing nodig.
  const startHotspot = async () => {
    if (cfg.hotspot_password.length < 8 || cfg.hotspot_password.length > 63) {
      flash(t("dhcp.passwordShort"), "warn");
      return;
    }
    setBusy(true);
    try {
      const s = await ipc.hotspotStart(
        cfg.hotspot_ssid,
        cfg.hotspot_password,
        cfg.hotspot_adapter,
        cfg.hotspot_dhcp,
        cfg.hotspot_internet,
        cfg,
      );
      setDhcp(s);
      flash(t("dhcp.hotspotStarted").replace("{ssid}", cfg.hotspot_ssid), "ok");
    } catch (e) {
      flash(String(e), "err");
    } finally {
      setBusy(false);
    }
  };

  // Internet delen mag ook terwijl de hotspot draait: NAT gaat direct aan of uit, en de
  // DNS-servers deelt de DHCP-server altijd al uit. De keuze wordt onthouden.
  const toggleInternet = async () => {
    const next = !cfg.hotspot_internet;
    setBusy(true);
    try {
      if (hotspotOn) setDhcp(await ipc.hotspotSetInternet(next));
      await persist({ ...cfg, hotspot_internet: next });
    } catch (e) {
      flash(String(e), "err");
    } finally {
      setBusy(false);
    }
  };

  const stopHotspot = async () => {
    setBusy(true);
    try {
      const s = await ipc.hotspotStop();
      setDhcp(s);
      flash(t("dhcp.hotspotStopped"), "ok");
    } catch (e) {
      flash(String(e), "err");
    } finally {
      setBusy(false);
    }
  };

  const beginEdit = (key: keyof DhcpConfig) => {
    if (locked) return;
    setDraft(String(cfg[key]));
    setEditing(key);
  };

  const commitEdit = async () => {
    const key = editing;
    setEditing(null);
    if (!key) return;
    const v = draft.trim();
    if (key === "pool_size") {
      const n = Number(v);
      if (!Number.isInteger(n) || n < 1 || n > 1000) return;
      await persist({ ...cfg, pool_size: n });
      return;
    }
    if (key === "hotspot_ssid") {
      if (v.length < 1 || v.length > 32) return;
      await persist({ ...cfg, hotspot_ssid: v });
      return;
    }
    if (key === "hotspot_password") {
      if (v.length < 8 || v.length > 63) {
        flash(t("dhcp.passwordShort"), "warn");
        return;
      }
      await persist({ ...cfg, hotspot_password: v });
      return;
    }
    if (!isValidIpv4(v)) return;
    await persist({ ...cfg, [key]: v });
  };

  const poolEnd = poolEndOf(cfg.pool_start, cfg.pool_size);
  const leases = dhcp?.leases ?? [];

  // Kaartkeuze: "" is Automatisch (Windows kiest). Een opgeslagen kaart die nu ontbreekt
  // (USB-stick eruit) blijft zichtbaar, gemarkeerd als down, zodat de keuze niet stil verdwijnt.
  const chosenCard = cfg.hotspot_adapter;
  const cardOptions = [
    { value: "", label: t("dhcp.cardAuto") },
    ...cards.map((c) => ({ value: c.friendly_name, label: c.friendly_name })),
  ];
  if (chosenCard && !cards.some((c) => c.friendly_name === chosenCard)) {
    cardOptions.push({ value: chosenCard, label: `${chosenCard} ${t("adapter.down")}` });
  }

  const row = (key: keyof DhcpConfig, label: string, shown: string) => (
    <div className="status-card__row">
      <span className="status-line__key">{label}</span>
      {editing === key ? (
        <input
          autoFocus
          className="status-edit mono"
          maxLength={key === "hotspot_password" ? 63 : key === "hotspot_ssid" ? 32 : 15}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commitEdit}
          onKeyDown={(e) => {
            if (e.key === "Enter") (e.target as HTMLInputElement).blur();
            if (e.key === "Escape") setEditing(null);
          }}
        />
      ) : (
        <button
          type="button"
          className={`status-edit-val mono ${locked ? "" : "status-edit-val--edit"}`}
          disabled={locked}
          onClick={() => beginEdit(key)}
        >
          {shown}
        </button>
      )}
    </div>
  );

  return (
    <section className="tab-content">
      <div className="tile">
        <span className="tile__label">
          <span
            className={`tab__dot dns-health__dot ${
              !running ? "" : dhcp?.listening ? "tab__dot--ok" : "tab__dot--err"
            }`}
          />
          {t("dhcp.server")}
        </span>
        <Toggle
          on={running}
          disabled={busy || !settings}
          onChange={hotspotOn ? toggleDhcpOnHotspot : running ? stop : start}
          aria-label={t("dhcp.server")}
        />
      </div>

      <div className={`status-card${running ? " dhcp-card--running" : ""}`}>
        {row("server_ip", t("dhcp.serverIp"), cfg.server_ip)}
        {row("pool_start", t("dhcp.poolStart"), cfg.pool_start)}
        {row("pool_size", t("dhcp.poolSize"), `${cfg.pool_size}${poolEnd ? ` (.${poolEnd})` : ""}`)}
        {row("subnet", t("dhcp.subnet"), cfg.subnet)}
      </div>

      {running && dhcp?.error && <p className="hint hint--err">{dhcp.error}</p>}
      {running && !dhcp?.error && leases.length === 0 && (
        <p className="hint">
          {t("dhcp.waiting").replace(
            "{adapter}",
            viaHotspot ? `${dhcp?.hotspot?.ssid} (${t("dhcp.viaHotspot")})` : dhcp?.adapter ?? "",
          )}
        </p>
      )}
      {hotspotOn && !running && (
        <p className="hint">{t("dhcp.hotspotNoDhcp").replace("{ip}", cfg.server_ip)}</p>
      )}

      {/* WiFi-hotspot: eigen netwerk met dezelfde DHCP-instellingen erachter. */}
      <div className="tile">
        <span className="tile__label">
          <span className={`tab__dot dns-health__dot ${hotspotOn ? "tab__dot--ok" : ""}`} />
          {t("dhcp.hotspot")}
        </span>
        <Toggle
          on={hotspotOn}
          disabled={busy || !settings || (running && !hotspotOn)}
          onChange={hotspotOn ? stopHotspot : startHotspot}
          aria-label={t("dhcp.hotspot")}
        />
      </div>
      <div className={`status-card${hotspotOn ? " dhcp-card--running" : ""}`}>
        {row("hotspot_ssid", t("dhcp.ssid"), cfg.hotspot_ssid)}
        {row("hotspot_password", t("dhcp.password"), cfg.hotspot_password)}
        <div className="status-card__row">
          <span className="status-line__key">{t("dhcp.method")}</span>
          <Select
            className="dhcp-method"
            value={chosenCard ?? ""}
            options={cardOptions}
            onChange={(v) => persist({ ...cfg, hotspot_adapter: v || null })}
            disabled={locked}
          />
        </div>
        <div className="status-card__row">
          <span className="status-line__key">{t("dhcp.internet")}</span>
          <Toggle
            on={cfg.hotspot_internet}
            disabled={busy || !settings}
            onChange={toggleInternet}
            aria-label={t("dhcp.internet")}
          />
        </div>
      </div>
      <p className="hint">{t("dhcp.methodHint")}</p>

      {leases.length > 0 && (
        <>
          <span className="field__label">
            {t("dhcp.leases")} ({leases.length})
          </span>
          <ul className="device-list">
            {leases.map((l) => (
              <LeaseRow key={l.mac} lease={l} onPing={() => requestPing(l.ip)} />
            ))}
          </ul>
        </>
      )}
    </section>
  );
}

function LeaseRow({ lease, onPing }: { lease: DhcpLease; onPing: () => void }) {
  const onCopy = async () => {
    try {
      await copyIp(lease.ip);
      flash(t("network.copied"), "ok");
    } catch {
      /* klembord niet beschikbaar */
    }
  };
  const sub = [lease.mac, lease.hostname].filter(Boolean).join(" · ");
  return (
    <li className={`device-row${lease.state === "offered" ? " device-row--offered" : ""}`}>
      <div className="device-row__main">
        <span className="device-ip">{lease.ip}</span>
        <span className="device-actions">
          <Tooltip text={t("network.pingDevice")} side="top" delay={300}>
            <button className="device-copy device-ping" aria-label={t("network.pingDevice")} onClick={onPing}>
              <IconPing size={14} />
            </button>
          </Tooltip>
          <Tooltip text={t("network.copyIp")} side="top" delay={300}>
            <button className="device-copy" aria-label={t("network.copyIp")} onClick={onCopy}>
              <IconCopy size={14} />
            </button>
          </Tooltip>
        </span>
      </div>
      <span className="device-brand">{sub}</span>
      {lease.vendor && <span className="device-detail">{lease.vendor}</span>}
    </li>
  );
}

/** Laatste octet van het laatste pool-adres, of null als de pool het /24 overschrijdt. */
function poolEndOf(start: string, size: number): number | null {
  if (!isValidIpv4(start)) return null;
  const last = Number(start.split(".")[3]) + size - 1;
  return last <= 254 ? last : null;
}
