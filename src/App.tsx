import { useEffect, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import Titlebar from "./components/Titlebar";
import Tabs, { type TabKey } from "./components/Tabs";
import {
  IconAdapter,
  IconSubnet,
  IconWifi,
  IconDns,
  IconPing,
  IconAuto,
  IconAbout,
} from "./components/icons";
import AdapterTab from "./tabs/AdapterTab";
import NetworkTab from "./tabs/NetworkTab";
import WifiTab from "./tabs/WifiTab";
import DnsTab from "./tabs/DnsTab";
import PingTab from "./tabs/PingTab";
import AutomationTab from "./tabs/AutomationTab";
import AboutTab from "./tabs/AboutTab";
import { useStore, flash } from "./store";
import { ipc, dnsPrimary, type SwitchResultEvent } from "./lib/ipc";
import { t } from "./lib/i18n";

export default function App() {
  const [active, setActive] = useState<TabKey>("adapter");
  const [prev, setPrev] = useState<TabKey>("adapter");
  const status = useStore((s) => s.status);
  const setStatus = useStore((s) => s.setStatus);
  const setSettings = useStore((s) => s.setSettings);
  const dnsAlive = useStore((s) => s.dnsAlive);
  const pingRequest = useStore((s) => s.pingRequest);

  const TABS = [
    { key: "adapter" as TabKey, label: t("tab.adapter"), icon: <IconAdapter /> },
    { key: "subnet" as TabKey, label: t("tab.network"), icon: <IconSubnet /> },
    { key: "wifi" as TabKey, label: t("tab.wifi"), icon: <IconWifi /> },
    {
      key: "dns" as TabKey,
      label: t("tab.dns"),
      icon: <IconDns />,
      badge:
        dnsAlive === null ? null : (
          <span className={`tab__bar ${dnsAlive ? "tab__bar--ok" : "tab__bar--err"}`} />
        ),
    },
    { key: "ping" as TabKey, label: t("tab.ping"), icon: <IconPing /> },
    { key: "automation" as TabKey, label: t("tab.auto"), icon: <IconAuto /> },
    { key: "about" as TabKey, label: t("tab.about"), icon: <IconAbout /> },
  ];

  useEffect(() => {
    ipc.loadSettings().then(setSettings).catch(console.error);
    ipc.getCurrentStatus().then(setStatus).catch(console.error);

    let unlisten: UnlistenFn | undefined;
    listen<SwitchResultEvent>("switch-result", (event) => {
      const p = event.payload;
      useStore.getState().setSwitchPulse({ kind: p.ok ? "ok" : "err", at: Date.now() });
      if (!p.ok) flash(p.msg, "err");
      ipc.getCurrentStatus().then(setStatus).catch(() => {});
    }).then((u) => {
      unlisten = u;
    });

    return () => {
      if (unlisten) unlisten();
    };
  }, [setSettings, setStatus]);

  // Pause all CSS animations when the window loses focus (mostly: hidden tray
  // app). The software compositor then does no work for animations nobody sees.
  useEffect(() => {
    const root = document.documentElement;
    const onBlur = () => root.classList.add("is-idle");
    const onFocus = () => root.classList.remove("is-idle");
    if (!document.hasFocus()) root.classList.add("is-idle");
    window.addEventListener("blur", onBlur);
    window.addEventListener("focus", onFocus);
    return () => {
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("focus", onFocus);
    };
  }, []);

  // A "ping this device" click from the network scan jumps to the Ping tab.
  useEffect(() => {
    if (!pingRequest) return;
    setPrev(active);
    setActive("ping");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pingRequest?.at]);

  // Continuous DNS health ping (2x/sec) → green/red dot on the DNS tab.
  useEffect(() => {
    let cancelled = false;
    const tick = async () => {
      const s = useStore.getState().settings;
      const target = s ? dnsPrimary(s) : null;
      if (!target) {
        useStore.getState().setDnsHealth(null, null);
        return;
      }
      try {
        const r = await ipc.dnsPing(target);
        if (!cancelled) useStore.getState().setDnsHealth(r.rtt_ms !== null, r.rtt_ms);
      } catch {
        if (!cancelled) useStore.getState().setDnsHealth(false, null);
      }
    };
    const id = window.setInterval(tick, 500);
    tick();
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, []);

  const handleTabChange = (next: TabKey) => {
    setPrev(active);
    setActive(next);
  };

  const direction =
    TABS.findIndex((t) => t.key === active) >
    TABS.findIndex((t) => t.key === prev)
      ? "forward"
      : "back";

  const flashMsg = useStore((s) => s.flash);

  return (
    <div className="app" data-state={status?.state ?? "idle"}>
      <div className="shell">
        <Titlebar />
        <Tabs tabs={TABS} active={active} onChange={handleTabChange} />
        <main className="tab-stage" data-direction={direction}>
          <div key={active} className="tab-panel">
            {active === "adapter" && <AdapterTab />}
            {active === "subnet" && <NetworkTab />}
            {active === "wifi" && <WifiTab />}
            {active === "dns" && <DnsTab />}
            {active === "ping" && <PingTab />}
            {active === "automation" && <AutomationTab />}
            {active === "about" && <AboutTab />}
          </div>
        </main>
        {flashMsg && (
          <div className={`flashbar flashbar--${flashMsg.kind}`}>{flashMsg.msg}</div>
        )}
      </div>
    </div>
  );
}
