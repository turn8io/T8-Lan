import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { createPortal } from "react-dom";

export type SelectOption = { value: string; label: string };

type Props = {
  value: string;
  options: SelectOption[];
  onChange: (value: string) => void;
  placeholder?: string;
  disabled?: boolean;
  className?: string;
};

/**
 * Custom dark dropdown. Native <select> option popups render with the OS
 * light theme in WebView2 regardless of color-scheme, so we render our own.
 * The menu is position:fixed (computed from the trigger rect) so it escapes
 * the scrollable tab panel's overflow clipping.
 */
export default function Select({
  value,
  options,
  onChange,
  placeholder,
  disabled,
  className,
}: Props) {
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLUListElement>(null);
  const [menuStyle, setMenuStyle] = useState<CSSProperties>({});

  const current = options.find((o) => o.value === value);

  useLayoutEffect(() => {
    if (!open || !wrapRef.current) return;
    const rect = wrapRef.current.getBoundingClientRect();
    const menuH = Math.min(options.length * 28 + 8, 200);
    const below = window.innerHeight - rect.bottom;
    const openUp = below < menuH + 8 && rect.top > below;
    setMenuStyle({
      position: "fixed",
      left: rect.left,
      width: rect.width,
      ...(openUp
        ? { bottom: window.innerHeight - rect.top + 4 }
        : { top: rect.bottom + 4 }),
      maxHeight: menuH,
    });
  }, [open, options.length]);

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
    const onScroll = () => setOpen(false);
    document.addEventListener("mousedown", onDoc);
    window.addEventListener("scroll", onScroll, true);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      window.removeEventListener("scroll", onScroll, true);
    };
  }, [open]);

  return (
    <div className={`cselect${className ? ` ${className}` : ""}`} ref={wrapRef}>
      <button
        type="button"
        className="cselect__btn"
        disabled={disabled}
        onClick={() => setOpen((o) => !o)}
        data-open={open}
      >
        <span className="cselect__label">{current?.label ?? placeholder ?? "—"}</span>
        <ChevronIcon />
      </button>
      {open &&
        createPortal(
          <ul className="cselect__menu" style={menuStyle} ref={menuRef}>
            {options.map((o) => (
              <li key={o.value}>
                <button
                  type="button"
                  className="cselect__opt"
                  data-active={o.value === value}
                  onClick={() => {
                    onChange(o.value);
                    setOpen(false);
                  }}
                >
                  {o.label}
                </button>
              </li>
            ))}
          </ul>,
          document.body,
        )}
    </div>
  );
}

const ChevronIcon = () => (
  <svg viewBox="0 0 12 12" width="10" height="10" fill="none" stroke="currentColor" strokeWidth="1.5">
    <path d="M3 4.5 L6 7.5 L9 4.5" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
);
