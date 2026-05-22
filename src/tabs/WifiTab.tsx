import { useEffect, useState } from "react";
import { useStore } from "../store";
import { ipc, isValidIpv4, type SsidRule } from "../lib/ipc";
import { t } from "../lib/i18n";
import Select from "../components/Select";

export default function WifiTab() {
  const settings = useStore((s) => s.settings);
  const setSettings = useStore((s) => s.setSettings);
  const [networks, setNetworks] = useState<string[]>([]);

  useEffect(() => {
    ipc.listWifiNetworks().then(setNetworks).catch(() => setNetworks([]));
  }, []);

  if (!settings) return <p className="hint">{t("common.loading")}</p>;

  const persist = async (next: typeof settings) => {
    setSettings(next);
    await ipc.saveSettings(next).catch(console.error);
  };

  const addRule = async () => {
    const next = {
      ...settings,
      ssid_rules: [...settings.ssid_rules, { ssid: "", action: { type: "dhcp" } as const }],
    };
    await persist(next);
    if (settings.ssid_rules.length === 0) await ipc.ssidWatcherSetEnabled(true).catch(console.error);
  };

  const updateRule = async (idx: number, patch: Partial<SsidRule>) => {
    await persist({
      ...settings,
      ssid_rules: settings.ssid_rules.map((r, i) => (i === idx ? { ...r, ...patch } : r)),
    });
  };

  const removeRule = async (idx: number) => {
    const remaining = settings.ssid_rules.filter((_, i) => i !== idx);
    await persist({ ...settings, ssid_rules: remaining });
    if (remaining.length === 0) await ipc.ssidWatcherSetEnabled(false).catch(console.error);
  };

  return (
    <section className="tab-content">
      <div className="field">
        <span className="field__label">
          {t("auto.ssidRules")} ({settings.ssid_rules.length})
        </span>
        {settings.ssid_rules.length === 0 && <p className="hint">{t("auto.ssidEmpty")}</p>}
        <datalist id="ssid-networks">
          {networks.map((n) => (
            <option key={n} value={n} />
          ))}
        </datalist>
        <ul className="ssid-list">
          {settings.ssid_rules.map((rule, idx) => (
            <SsidRow
              key={idx}
              rule={rule}
              onChange={(patch) => updateRule(idx, patch)}
              onRemove={() => removeRule(idx)}
            />
          ))}
        </ul>
        <button className="btn btn--ghost btn--full btn--compact" onClick={addRule}>
          {t("auto.addRule")}
        </button>
        {networks.length > 0 && (
          <p className="hint">
            {networks.length} {t("auto.networksInRange")}
          </p>
        )}
      </div>
    </section>
  );
}

function SsidRow({
  rule,
  onChange,
  onRemove,
}: {
  rule: SsidRule;
  onChange: (patch: Partial<SsidRule>) => void;
  onRemove: () => void;
}) {
  const isStatic = rule.action.type === "static-ip";
  const ipVal = isStatic ? (rule.action as { value: string }).value : "";
  const ipValid = ipVal === "" || isValidIpv4(ipVal);

  return (
    <li className="ssid-card">
      <div className="ssid-card__top">
        <input
          className="input ssid-card__ssid"
          list="ssid-networks"
          placeholder="SSID..."
          value={rule.ssid}
          onChange={(e) => onChange({ ssid: e.target.value })}
        />
        <Select
          className="ssid-card__action"
          value={rule.action.type}
          options={[
            { value: "dhcp", label: t("auto.dhcp") },
            { value: "static-ip", label: t("auto.static") },
          ]}
          onChange={(v) =>
            onChange({
              action: v === "dhcp" ? { type: "dhcp" } : { type: "static-ip", value: "" },
            })
          }
        />
        <button
          className="recent-item__close ssid-card__del"
          onClick={onRemove}
          aria-label={t("adapter.remove")}
        >
          ×
        </button>
      </div>
      {isStatic && (
        <input
          className={`input mono ssid-card__ip ${!ipValid ? "input--invalid" : ""}`}
          placeholder="192.168.1.100"
          value={ipVal}
          onChange={(e) => onChange({ action: { type: "static-ip", value: e.target.value } })}
        />
      )}
    </li>
  );
}
