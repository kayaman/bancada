// ToastStack — dumb renderer for the toast reducer in `src/notifications.ts`.
// Owns no state and no clock: the stack, the TTLs and the expiry timer all
// live above it. Props in, cards out.
//
// Roles are split on kind for a reason: an error is the one notification worth
// interrupting a screen-reader user for (`alert`, assertive), while chatter
// like "scanning ports" is announced only when the user is idle (`status`,
// polite). Announcing every info toast assertively would make the app
// unusable with a reader attached.

import type { Toast } from "../notifications";

interface Props {
  toasts: Toast[];
  onDismiss: (id: number) => void;
}

export default function ToastStack({ toasts, onDismiss }: Props) {
  // No empty-state markup: an empty fixed-position container still traps
  // pointer events over the corner of the window it covers.
  if (toasts.length === 0) return null;

  return (
    <div className="toast-stack">
      {toasts.map((t) => (
        <div
          key={t.id}
          className={`toast toast-${t.kind}`}
          role={t.kind === "error" ? "alert" : "status"}
        >
          <span className="toast-text">{t.message}</span>
          {t.count > 1 && <span className="toast-count">×{t.count}</span>}
          <button
            className="btn small icon toast-close"
            title="Dismiss"
            aria-label="Dismiss notification"
            onClick={() => onDismiss(t.id)}
          >
            ×
          </button>
        </div>
      ))}
    </div>
  );
}
