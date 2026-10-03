import { useEffect } from "react";
import { useStore } from "../store";

/**
 * Huisstijl-bevestigingsdialoog. Wordt gevoed door `confirmDialog()` uit de store en één
 * keer gerenderd in App. Vervangt window.confirm (dat een kale OS-popup geeft).
 *
 * `danger` geeft de rode variant: een duidelijke, korte waarschuwing voor acties die een
 * netwerk kunnen verstoren (DHCP-server, IP-conflict negeren).
 */
export default function ConfirmDialog() {
  const req = useStore((s) => s.confirmReq);
  const setReq = useStore((s) => s.setConfirmReq);

  const answer = (ok: boolean) => {
    if (!req) return;
    setReq(null);
    req.resolve(ok);
  };

  useEffect(() => {
    if (!req) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") answer(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [req]);

  if (!req) return null;

  return (
    <div className="modal-backdrop" onMouseDown={() => answer(false)}>
      <div
        className={`modal${req.danger ? " modal--danger" : ""}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby="modal-title"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="modal__head">
          {req.danger && <WarnIcon />}
          <h2 id="modal-title" className="modal__title">
            {req.title}
          </h2>
        </div>
        <p className="modal__body">{req.body}</p>
        <div className="modal__actions">
          <button type="button" className="btn btn--ghost" onClick={() => answer(false)} autoFocus>
            {req.cancelLabel}
          </button>
          <button
            type="button"
            className={`btn ${req.danger ? "btn--danger" : "btn--primary"}`}
            onClick={() => answer(true)}
          >
            {req.confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}

const WarnIcon = () => (
  <svg
    className="modal__icon"
    viewBox="0 0 24 24"
    width="18"
    height="18"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.9"
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <path d="M12 3 2.5 19.5h19L12 3z" />
    <path d="M12 9v5M12 17.2h.01" />
  </svg>
);
