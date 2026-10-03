import { useState } from "react";
import { useStore, flash } from "../store";
import { ipc } from "../lib/ipc";
import { t, LANGUAGES, type LangSetting } from "../lib/i18n";
import Toggle from "../components/Toggle";
import HotkeyInput from "../components/HotkeyInput";
import Select from "../components/Select";

export default function AutomationTab() {
  const settings = useStore((s) => s.settings);
  const setSettings = useStore((s) => s.setSettings);
  const [autostartBusy, setAutostartBusy] = useState(false);

  if (!settings) return <p className="hint">{t("common.loading")}</p>;

  // Hotkeys: persist + re-register in backend.
  const applyHotkeys = async (next: typeof settings) => {
    setSettings(next);
    await ipc.updateHotkeys(next).catch((e) => flash(String(e), "err"));
  };

  const toggleHotkeys = async () => {
    await applyHotkeys({ ...settings, global_hotkey_enabled: !settings.global_hotkey_enabled });
  };

  // Autostart: de backend bewaart de instelling en beheert de Taakplanner-taak.
  const toggleAutostart = async () => {
    const enabled = !settings.autostart;
    setAutostartBusy(true);
    setSettings({ ...settings, autostart: enabled });
    try {
      await ipc.setAutostart(enabled);
    } catch (e) {
      flash(String(e), "err");
    } finally {
      setAutostartBusy(false);
    }
  };

  // Taal: opslaan; App herleidt de effectieve taal uit settings en remount de shell.
  const setLanguageSetting = async (code: string) => {
    const next = { ...settings, language: code as LangSetting };
    setSettings(next);
    await ipc.saveSettings(next).catch((e) => flash(String(e), "err"));
  };

  return (
    <section className="tab-content">
      <div className="status-card">
        <div className="hk-row">
          <span className="status-line__key">{t("settings.language")}</span>
          <Select
            className="lang-select"
            value={settings.language ?? "auto"}
            options={LANGUAGES.map((l) => ({ value: l.code, label: l.label }))}
            onChange={setLanguageSetting}
          />
        </div>
        <div className="hk-row">
          <span className="status-line__key">{t("auto.autostart")}</span>
          <Toggle
            on={settings.autostart}
            disabled={autostartBusy}
            onChange={toggleAutostart}
            aria-label={t("auto.autostart")}
          />
        </div>
      </div>

      <div className="status-card">
        <div className="hk-row">
          <span className="status-line__key">{t("auto.hotkeysOn")}</span>
          <Toggle
            on={settings.global_hotkey_enabled}
            onChange={toggleHotkeys}
            aria-label={t("auto.hotkeysOn")}
          />
        </div>
        <div className="hk-row" data-disabled={!settings.global_hotkey_enabled}>
          <span className="status-line__key">{t("auto.toDhcp")}</span>
          <HotkeyInput
            value={settings.hotkey_dhcp}
            onChange={(combo) => applyHotkeys({ ...settings, hotkey_dhcp: combo })}
          />
        </div>
        <div className="hk-row" data-disabled={!settings.global_hotkey_enabled}>
          <span className="status-line__key">{t("auto.toLastStatic")}</span>
          <HotkeyInput
            value={settings.hotkey_static}
            onChange={(combo) => applyHotkeys({ ...settings, hotkey_static: combo })}
          />
        </div>
      </div>
    </section>
  );
}
