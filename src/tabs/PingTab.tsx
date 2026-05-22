import { useEffect, useMemo, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { ipc, type PingResult } from "../lib/ipc";
import { useStore } from "../store";
import { t } from "../lib/i18n";

const HISTORY = 60;

export default function PingTab() {
  const settings = useStore((s) => s.settings);
  const setSettings = useStore((s) => s.setSettings);
  const pingRequest = useStore((s) => s.pingRequest);
  const clearPingRequest = useStore((s) => s.clearPingRequest);
  const [target, setTarget] = useState(settings?.ping.target ?? "1.1.1.1");
  const [running, setRunning] = useState(false);
  const [results, setResults] = useState<PingResult[]>([]);
  const unlistenRef = useRef<UnlistenFn | null>(null);

  useEffect(() => {
    let cancelled = false;
    listen<PingResult>("ping-result", (event) => {
      if (cancelled) return;
      setResults((prev) => [...prev.slice(-(HISTORY - 1)), event.payload]);
    }).then((un) => {
      unlistenRef.current = un;
    });
    return () => {
      cancelled = true;
      if (unlistenRef.current) unlistenRef.current();
      ipc.pingStop().catch(() => {});
    };
  }, []);

  const startTarget = async (ip: string) => {
    const tgt = ip.trim();
    if (!tgt) return;
    if (settings) {
      const next = { ...settings, ping: { target: tgt } };
      setSettings(next);
      await ipc.saveSettings(next).catch(console.error);
    }
    await ipc.pingStop().catch(() => {});
    await ipc.pingStart(tgt).catch(() => {});
    setRunning(true);
    setResults([]);
  };

  const start = () => startTarget(target);

  // "Ping this device" from the network scan: adopt the IP, start immediately.
  useEffect(() => {
    if (!pingRequest) return;
    setTarget(pingRequest.ip);
    void startTarget(pingRequest.ip);
    clearPingRequest();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pingRequest?.at]);

  const stop = async () => {
    await ipc.pingStop();
    setRunning(false);
  };

  const stats = useMemo(() => computeStats(results), [results]);

  return (
    <section className="tab-content">
      <label className="field">
        <span className="field__label">{t("ping.target")}</span>
        <div className="row">
          <input
            type="text"
            className="input mono"
            value={target}
            placeholder="1.1.1.1"
            onChange={(e) => setTarget(e.target.value)}
            disabled={running}
          />
          <button
            className={running ? "btn btn--ghost" : "btn btn--primary"}
            onClick={running ? stop : start}
          >
            {running ? t("ping.stop") : t("ping.start")}
          </button>
        </div>
      </label>

      <div className="status-card">
        <Stat label={t("ping.last")} val={fmtMs(stats.last)} />
        <Stat label={t("ping.avg")} val={fmtMs(stats.avg)} />
        <Stat label={t("ping.minmax")} val={`${fmtMs(stats.min)} / ${fmtMs(stats.max)}`} />
        <Stat label={t("ping.loss")} val={`${stats.loss.toFixed(0)}%`} />
      </div>

      <Sparkline results={results} />
    </section>
  );
}

function Stat({ label, val }: { label: string; val: string }) {
  return (
    <div className="status-card__row">
      <span className="status-line__key">{label}</span>
      <span className="mono ping-stat-val">{val}</span>
    </div>
  );
}

function fmtMs(v: number | null): string {
  if (v === null || Number.isNaN(v)) return "—";
  return `${Math.round(v)} ms`;
}

function computeStats(results: PingResult[]) {
  if (results.length === 0) {
    return { last: null, avg: null, min: null, max: null, loss: 0 };
  }
  const ok = results.filter((r) => r.rtt_ms !== null).map((r) => r.rtt_ms!);
  const last = results[results.length - 1].rtt_ms;
  if (ok.length === 0) return { last: null, avg: null, min: null, max: null, loss: 100 };
  const sum = ok.reduce((a, b) => a + b, 0);
  return {
    last,
    avg: sum / ok.length,
    min: Math.min(...ok),
    max: Math.max(...ok),
    loss: (1 - ok.length / results.length) * 100,
  };
}

function Sparkline({ results }: { results: PingResult[] }) {
  if (results.length < 2) {
    return (
      <div className="sparkline">
        <span className="hint">{t("ping.waiting")}</span>
      </div>
    );
  }
  const W = 380;
  const H = 60;
  const PAD = 16; // vertical inset so peaks (and the glowing dot) stay clear of the edges
  const MX = 12; // horizontal inset so the glowing newest dot stays fully visible
  const valid = results.filter((r) => r.rtt_ms !== null).map((r) => r.rtt_ms!);
  const aMax = Math.max(...valid);
  const aMin = Math.min(...valid);
  const flat = aMax - aMin < 0.5; // (near-)constant rtt → draw it centered
  const yFor = (v: number) =>
    flat ? H / 2 : PAD + (1 - (v - aMin) / (aMax - aMin)) * (H - PAD * 2);

  const n = results.length;
  const pts = results.map((r, i) => {
    const x = MX + (i / (n - 1)) * (W - 2 * MX);
    if (r.rtt_ms === null) return { x, y: H - 2, miss: true };
    return { x, y: yFor(r.rtt_ms), miss: false };
  });
  const line = pts
    .filter((p) => !p.miss)
    .map((p, i) => `${i === 0 ? "M" : "L"}${p.x.toFixed(1)} ${p.y.toFixed(1)}`)
    .join(" ");
  const lastIdx = n - 1;

  return (
    <div className="sparkline">
      <svg viewBox={`0 0 ${W} ${H}`} width="100%" height="100%" preserveAspectRatio="none">
        <path d={line} fill="none" stroke="var(--t8-yellow)" strokeWidth="1.2" opacity="0.55" />
        {pts.map((p, i) => {
          // Older points are fainter, newer ones brighter — a moving "trail".
          const trail = 0.2 + 0.8 * (i / Math.max(lastIdx, 1));
          if (p.miss) {
            return <circle key={i} cx={p.x} cy={H - 2} r="2" fill="var(--err)" opacity={trail} />;
          }
          if (i === lastIdx) {
            // newest measurement: a glowing live dot
            return (
              <circle
                key={i}
                className="spark-dot--live"
                cx={p.x}
                cy={p.y}
                r="2.6"
                fill="var(--t8-yellow)"
              />
            );
          }
          return <circle key={i} cx={p.x} cy={p.y} r="1.5" fill="var(--t8-yellow)" opacity={trail} />;
        })}
      </svg>
    </div>
  );
}
