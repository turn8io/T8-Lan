import { useState, type KeyboardEvent } from "react";
import { t } from "../lib/i18n";

/** Pretty-print a global-hotkey string ("Control+Alt+KeyD") to "Ctrl + Alt + D". */
export function prettyHotkey(combo: string): string {
  if (!combo) return "—";
  return combo
    .split("+")
    .map((part) => {
      const p = part.toLowerCase();
      if (p === "control" || p === "ctrl") return "Ctrl";
      if (p === "alt" || p === "option") return "Alt";
      if (p === "shift") return "Shift";
      if (p === "super" || p === "meta" || p === "cmd" || p === "win") return "Win";
      if (part.startsWith("Key")) return part.slice(3);
      if (part.startsWith("Digit")) return part.slice(5);
      return part;
    })
    .join(" + ");
}

type Props = {
  value: string;
  onChange: (combo: string) => void;
};

export default function HotkeyInput({ value, onChange }: Props) {
  const [capturing, setCapturing] = useState(false);

  const onKeyDown = (e: KeyboardEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const key = e.key;
    if (key === "Escape") {
      setCapturing(false);
      return;
    }
    // Wait for a non-modifier key.
    if (["Control", "Alt", "Shift", "Meta"].includes(key)) return;

    const mods: string[] = [];
    if (e.ctrlKey) mods.push("Control");
    if (e.altKey) mods.push("Alt");
    if (e.shiftKey) mods.push("Shift");
    if (e.metaKey) mods.push("Super");
    if (mods.length === 0) return; // require at least one modifier

    const combo = [...mods, e.code].join("+");
    onChange(combo);
    setCapturing(false);
  };

  return (
    <button
      type="button"
      className="hotkey-input"
      data-capturing={capturing}
      onClick={() => setCapturing(true)}
      onBlur={() => setCapturing(false)}
      onKeyDown={capturing ? onKeyDown : undefined}
    >
      {capturing ? t("auto.pressKeys") : prettyHotkey(value)}
    </button>
  );
}
