import { useStore } from "../store";
import { ipc, dnsPrimary, dnsServers, type DnsPreset } from "../lib/ipc";
import { t } from "../lib/i18n";
import Toggle from "../components/Toggle";
import Select from "../components/Select";

const PRESET_OPTIONS = [
  { value: "cloudflare", label: "Cloudflare" },
  { value: "google", label: "Google" },
  { value: "opendns", label: "OpenDNS" },
  { value: "quad9", label: "Quad9" },
  { value: "custom", label: "Custom" },
];

export default function DnsTab() {
  const settings = useStore((s) => s.settings);
  const setSettings = useStore((s) => s.setSettings);
  const dnsAlive = useStore((s) => s.dnsAlive);
  const dnsRtt = useStore((s) => s.dnsRtt);

  if (!settings) return <p className="hint">{t("common.loading")}</p>;

  const dns = settings.dns;
  const target = dnsPrimary(settings);
  const [primary, secondary] = dnsServers(settings);

  const update = async (patch: Partial<typeof settings.dns>) => {
    const next = { ...settings, dns: { ...settings.dns, ...patch } };
    setSettings(next);
    await ipc.saveSettings(next).catch(console.error);
  };

  // Editing a server forces the custom preset (seeding both fields from the
  // currently effective values).
  const setServer = (which: "primary" | "secondary", value: string) =>
    update({
      preset: "custom",
      custom_primary: which === "primary" ? value : primary,
      custom_secondary: which === "secondary" ? value : secondary,
    });

  return (
    <section className="tab-content">
      {/* Health */}
      <div className="tile">
        <span className="tile__label">
          <span
            className={`tab__dot ${
              dnsAlive === null ? "" : dnsAlive ? "tab__dot--ok" : "tab__dot--err"
            } dns-health__dot`}
          />
          {target ?? "—"}
        </span>
        <span className="mono dns-health__rtt">
          {target == null
            ? "—"
            : dnsAlive === false
              ? t("dns.noReply")
              : dnsRtt != null
                ? `${dnsRtt} ms`
                : "…"}
        </span>
      </div>

      {/* One tile: toggle, preset, and the two editable DNS servers.
          A 2-column grid keeps all controls the exact same width. */}
      <div className="dns-grid">
        <span className="status-line__key">{t("dns.toggle")}</span>
        <span className="dns-toggle-cell">
          <Toggle
            on={dns.apply_on_switch}
            onChange={() => update({ apply_on_switch: !dns.apply_on_switch })}
            aria-label={t("dns.toggle")}
          />
        </span>

        <span className="status-line__key">{t("dns.preset")}</span>
        <Select
          value={dns.preset}
          options={PRESET_OPTIONS}
          onChange={(v) => update({ preset: v as DnsPreset })}
        />

        <span className="status-line__key">{t("dns.primary")}</span>
        <input
          className="input mono dns-server-input"
          placeholder="1.1.1.1"
          value={primary}
          onChange={(e) => setServer("primary", e.target.value)}
        />

        <span className="status-line__key">{t("dns.fallback")}</span>
        <input
          className="input mono dns-server-input"
          placeholder="1.0.0.1"
          value={secondary}
          onChange={(e) => setServer("secondary", e.target.value)}
        />
      </div>
    </section>
  );
}
