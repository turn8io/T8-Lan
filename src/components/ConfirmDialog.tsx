import { useEffect } from "react";
import { useStore, type ConfirmChoice } from "../store";

/**
 * Huisstijl-bevestigingsdialoog. Wordt gevoed door `confirmDialog()` / `askDialog()` uit
 * de store en één keer gerenderd in App. Vervangt window.confirm (kale OS-popup).
 *
 * `danger` geeft de rode variant voor acties die een netwerk kunnen verstoren.
 * `altLabel` voegt een derde keuze toe; de knoppen staan dan onder elkaar.
 */
export default function ConfirmDialog() {
  const req = useStore((s) => s.confirmReq);
  const setReq = useStore((s) => s.setConfirmReq);

  const answer = (choice: ConfirmChoice) => {
    if (!req) return;
    setReq(null);
    req.resolve(choice);
  };

  useEffect(() => {
    if (!req) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") answer("cancel");
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [req]);

  if (!req) return null;
  const stacked = Boolean(req.altLabel);

  return (
    <div className="modal-backdrop" onMouseDown={() => answer("cancel")}>
      <div
        className={`modal${req.danger ? " modal--danger" : ""}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby="modal-title"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="modal__head">
          {req.danger ? <WarnIcon /> : <InfoIcon />}
          <h2 id="modal-title" className="modal__title">
            {req.title}
          </h2>
        </div>
        <p className="modal__body">{req.body}</p>
        <div className={`modal__actions${stacked ? " modal__actions--stack" : ""}`}>
          {stacked ? (
            <>
              <button
                type="button"
                className={`btn btn--full ${req.danger ? "btn--danger" : "btn--primary"}`}
                onClick={() => answer("confirm")}
                autoFocus
              >
                {req.confirmLabel}
              </button>
              <div className="modal__actions-row">
                <button type="button" className="btn btn--ghost" onClick={() => answer("cancel")}>
                  {req.cancelLabel}
                </button>
                <button type="button" className="btn btn--ghost" onClick={() => answer("alt")}>
                  {req.altLabel}
                </button>
              </div>
            </>
          ) : (
            <>
              <button type="button" className="btn btn--ghost" onClick={() => answer("cancel")} autoFocus>
                {req.cancelLabel}
              </button>
              <button
                type="button"
                className={`btn ${req.danger ? "btn--danger" : "btn--primary"}`}
                onClick={() => answer("confirm")}
              >
                {req.confirmLabel}
              </button>
            </>
          )}
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

const InfoIcon = () => (
  <svg
    className="modal__icon modal__icon--info"
    viewBox="0 0 24 24"
    width="18"
    height="18"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.9"
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <circle cx="12" cy="12" r="9" />
    <path d="M12 11v5M12 8h.01" />
  </svg>
);
