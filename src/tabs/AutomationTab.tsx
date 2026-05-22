import { useStore, flash } from "../store";
import { ipc } from "../lib/ipc";
import { t } from "../lib/i18n";
import Toggle from "../components/Toggle";
import HotkeyInput from "../components/HotkeyInput";

export default function AutomationTab() {
  const settings = useStore((s) => s.settings);
  const setSettings = useStore((s) => s.setSettings);

  if (!settings) return <p className="hint">{t("common.loading")}</p>;

  // Hotkeys: persist + re-register in backend.
  const applyHotkeys = async (next: typeof settings) => {
    setSettings(next);
    await ipc.updateHotkeys(next).catch((e) => flash(String(e), "err"));
  };

  const toggleHotkeys = async () => {
    await applyHotkeys({ ...settings, global_hotkey_enabled: !settings.global_hotkey_enabled });
  };

  return (
    <section className="tab-content">
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
