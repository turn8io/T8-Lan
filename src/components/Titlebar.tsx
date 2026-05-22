import { useEffect, useState, type MouseEvent } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import icoUrl from "../../logo/T8_backup.ico?url";
import { ipc } from "../lib/ipc";
import { flash } from "../store";
import { t } from "../lib/i18n";
import Tooltip from "./Tooltip";

export default function Titlebar() {
  const win = getCurrentWindow();
  const [canUndo, setCanUndo] = useState(false);
  const [canRedo, setCanRedo] = useState(false);
  const [busy, setBusy] = useState(false);

  const refreshHistory = () => {
    ipc.hasUndo().then((p) => setCanUndo(p !== null)).catch(() => setCanUndo(false));
    ipc.hasRedo().then((p) => setCanRedo(p !== null)).catch(() => setCanRedo(false));
  };

  useEffect(() => {
    refreshHistory();
    const id = setInterval(refreshHistory, 2000);
    return () => clearInterval(id);
  }, []);

  const onUndo = async () => {
    setBusy(true);
    try {
      const prev = await ipc.undoLastSwitch();
      const what = prev.was_dhcp ? "DHCP" : prev.ip ?? "vorige config";
      flash(`Hersteld naar ${what} op ${prev.adapter_name}`, "ok");
      refreshHistory();
    } catch (e) {
      flash(String(e), "err");
    } finally {
      setBusy(false);
    }
  };

  const onRedo = async () => {
    setBusy(true);
    try {
      const cfg = await ipc.redoLastSwitch();
      const what = cfg.was_dhcp ? "DHCP" : cfg.ip ?? "config";
      flash(`Opnieuw: ${what} op ${cfg.adapter_name}`, "ok");
      refreshHistory();
    } catch (e) {
      flash(String(e), "err");
    } finally {
      setBusy(false);
    }
  };

  const startDrag = (e: MouseEvent) => {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest(".titlebar__actions")) return;
    win.startDragging().catch(() => {});
  };

  const onClose = async () => {
    await ipc.saveWindowPosition().catch(() => {});
    await win.hide();
  };

  return (
    <header className="titlebar" onMouseDown={startDrag}>
      <div className="titlebar__brand">
        <img src={icoUrl} alt="T8" className="titlebar__logo" />
        <span className="titlebar__title">T8-Lan</span>
      </div>
      <div className="titlebar__actions">
        <Tooltip text={canUndo ? t("title.undo") : t("title.undoNone")} side="left">
          <button
            className="titlebar__btn"
            aria-label={t("title.undo")}
            disabled={!canUndo || busy}
            onClick={onUndo}
          >
            <UndoIcon />
          </button>
        </Tooltip>
        <Tooltip text={canRedo ? t("title.redo") : t("title.redoNone")} side="left">
          <button
            className="titlebar__btn"
            aria-label={t("title.redo")}
            disabled={!canRedo || busy}
            onClick={onRedo}
          >
            <RedoIcon />
          </button>
        </Tooltip>
        <Tooltip text={t("title.close")} side="left">
          <button
            className="titlebar__btn titlebar__btn--close"
            aria-label={t("title.close")}
            onClick={onClose}
          >
            <CloseIcon />
          </button>
        </Tooltip>
      </div>
    </header>
  );
}

const UndoIcon = () => (
  <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
    <path d="M9 14 4 9l5-5" />
    <path d="M4 9h11a5 5 0 0 1 0 10h-3" />
  </svg>
);
const RedoIcon = () => (
  <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
    <path d="M15 14l5-5-5-5" />
    <path d="M20 9H9a5 5 0 0 0 0 10h3" />
  </svg>
);
const CloseIcon = () => (
  <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
    <path d="M5 5 L19 19 M19 5 L5 19" />
  </svg>
);
