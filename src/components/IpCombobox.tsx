import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { createPortal } from "react-dom";
import type { RecentIp } from "../lib/ipc";
import { t } from "../lib/i18n";

type Props = {
  value: string;
  onChange: (v: string) => void;
  recents: RecentIp[];
  presets?: RecentIp[];
  onApply: (ip: string) => void;
  onEditLabel: (ip: string, label: string) => void;
  onRemove: (ip: string) => void;
  valid: boolean;
  disabled?: boolean;
};

export default function IpCombobox({
  value,
  onChange,
  recents,
  presets = [],
  onApply,
  onEditLabel,
  onRemove,
  valid,
  disabled,
}: Props) {
  const [open, setOpen] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  const [labelDraft, setLabelDraft] = useState("");
  const wrapRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLUListElement>(null);
  const [menuStyle, setMenuStyle] = useState<CSSProperties>({});

  const hasMenu = recents.length + presets.length > 0;

  useLayoutEffect(() => {
    if (!open || !wrapRef.current) return;
    const rect = wrapRef.current.getBoundingClientRect();
    const menuH = Math.min((recents.length + presets.length) * 30 + 16, 220);
    const below = window.innerHeight - rect.bottom;
    const openUp = below < menuH + 8 && rect.top > below;
    // Span nearly the full window, leaving a 4px gutter on both sides.
    const left = 4;
    const width = window.innerWidth - 8;
    setMenuStyle({
      position: "fixed",
      left,
      width,
      ...(openUp ? { bottom: window.innerHeight - rect.top + 4 } : { top: rect.bottom + 4 }),
      maxHeight: menuH,
    });
  }, [open, recents.length, presets.length]);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      const t = e.target as Node;
      if (
        wrapRef.current &&
        !wrapRef.current.contains(t) &&
        menuRef.current &&
        !menuRef.current.contains(t)
      )
        setOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open]);

  const canApply = !disabled && valid && value.length > 0;

  return (
    <div className="ipcombo" ref={wrapRef}>
      <div className="ipcombo__field">
        <input
          type="text"
          className={`input mono ipcombo__input ${!valid ? "input--invalid" : ""}`}
          placeholder="192.168.1.100"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && canApply && onApply(value)}
          disabled={disabled}
        />
        {hasMenu && (
          <button
            type="button"
            className="ipcombo__chevron"
            aria-label="Recent IPs"
            disabled={disabled}
            onClick={() => setOpen((o) => !o)}
          >
            <svg viewBox="0 0 12 12" width="11" height="11" fill="none" stroke="currentColor" strokeWidth="1.5">
              <path d="M3 4.5 L6 7.5 L9 4.5" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </button>
        )}
      </div>

      {open &&
        hasMenu &&
        createPortal(
          <ul className="ipcombo__menu" style={menuStyle} ref={menuRef}>
            {recents.map((r) => (
            <li key={r.ip} className="ipcombo__row">
              {editing === r.ip ? (
                <input
                  autoFocus
                  className="ipcombo__edit"
                  value={labelDraft}
                  placeholder={t("adapter.labelPlaceholder")}
                  onChange={(e) => setLabelDraft(e.target.value)}
                  onBlur={() => {
                    onEditLabel(r.ip, labelDraft.trim());
                    setEditing(null);
                  }}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") (e.target as HTMLInputElement).blur();
                    if (e.key === "Escape") setEditing(null);
                  }}
                />
              ) : (
                <button
                  type="button"
                  className="ipcombo__pick"
                  aria-label={r.ip}
                  onClick={() => {
                    onApply(r.ip);
                    setOpen(false);
                  }}
                >
                  <span className="mono">{r.ip}</span>
                  {r.label && <span className="ipcombo__label">{r.label}</span>}
                </button>
              )}
              <button
                type="button"
                className="ipcombo__icon"
                aria-label="Label"
                onClick={() => {
                  setEditing(r.ip);
                  setLabelDraft(r.label ?? "");
                }}
              >
                <svg viewBox="0 0 16 16" width="10" height="10" fill="none" stroke="currentColor" strokeWidth="1.4">
                  <path d="M2 12.5V14h1.5l8-8L10 4.5l-8 8z" strokeLinejoin="round" />
                  <path d="M9 5.5l1.5 1.5" />
                </svg>
              </button>
              <button
                type="button"
                className="ipcombo__icon ipcombo__icon--del"
                aria-label="x"
                onClick={() => onRemove(r.ip)}
              >
                ×
              </button>
            </li>
            ))}
            {presets.length > 0 && recents.length > 0 && <li className="ipcombo__divider" />}
            {presets.map((p) => (
              <li key={`preset-${p.ip}`} className="ipcombo__row">
                <button
                  type="button"
                  className="ipcombo__pick"
                  aria-label={p.ip}
                  onClick={() => {
                    onApply(p.ip);
                    setOpen(false);
                  }}
                >
                  <span className="mono">{p.ip}</span>
                  {p.label && <span className="ipcombo__label">{p.label}</span>}
                </button>
              </li>
            ))}
          </ul>,
          document.body,
        )}
    </div>
  );
}
