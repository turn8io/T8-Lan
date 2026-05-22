import { useEffect, useRef, useState, type ReactNode } from "react";

export type TabKey =
  | "adapter"
  | "subnet"
  | "wifi"
  | "dns"
  | "ping"
  | "automation"
  | "about";

type TabDef = { key: TabKey; label: string; icon: ReactNode; badge?: ReactNode };

type Props = {
  tabs: TabDef[];
  active: TabKey;
  onChange: (key: TabKey) => void;
};

const TOOLTIP_DELAY = 500;

export default function Tabs({ tabs, active, onChange }: Props) {
  const [hovered, setHovered] = useState<TabKey | null>(null);
  const instantRef = useRef(false);
  const timerRef = useRef<number | undefined>(undefined);

  useEffect(() => {
    const reset = () => {
      instantRef.current = false;
    };
    window.addEventListener("click", reset);
    return () => window.removeEventListener("click", reset);
  }, []);

  const onEnter = (key: TabKey) => {
    if (timerRef.current) window.clearTimeout(timerRef.current);
    if (instantRef.current) {
      setHovered(key);
    } else {
      timerRef.current = window.setTimeout(() => {
        setHovered(key);
        instantRef.current = true;
      }, TOOLTIP_DELAY);
    }
  };

  const onLeave = () => {
    if (timerRef.current) window.clearTimeout(timerRef.current);
    setHovered(null);
  };

  return (
    <nav className="tabs" role="tablist">
      {tabs.map((t) => (
        <button
          key={t.key}
          role="tab"
          aria-selected={active === t.key}
          aria-label={t.label}
          className="tab"
          data-active={active === t.key}
          onMouseEnter={() => onEnter(t.key)}
          onMouseLeave={onLeave}
          onClick={() => {
            onChange(t.key);
            onLeave();
          }}
        >
          <span className="tab__icon">{t.icon}</span>
          {t.badge}
          {hovered === t.key && <span className="tooltip tooltip--bottom">{t.label}</span>}
        </button>
      ))}
    </nav>
  );
}
