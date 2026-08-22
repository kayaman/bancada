// Toast notifications — the reducer behind the app's second notification
// surface. Pure: `now` is injected everywhere, so a TTL can be tested without
// a clock or a timer.
//
// Why this exists: the status bar is single-slot. `notify()` overwrites it, so
// a message that arrived while you were reading the previous one simply never
// existed — "board detached" clobbered by "✓ Compiled in 4.1s" is a real loss.
// A stack keeps the last few, and the rules below encode the three things that
// made the single slot painful:
//
//   - **Errors never expire.** A failure you did not see is the one message
//     that must survive; everything else is chatter with a deadline.
//   - **A repeat collapses.** Hotplug and the fleet sync emit the same line in
//     bursts; four identical cards is noise, `×4` on one card is information.
//   - **The stack is capped.** Past MAX_VISIBLE the oldest chatter goes first
//     and errors are only sacrificed when there is nothing else to drop.

export type ToastKind = "info" | "success" | "error";

export interface Toast {
  id: number;
  kind: ToastKind;
  message: string;
  createdAt: number;
  /** Epoch-ms deadline, or null for a toast that must be dismissed by hand. */
  expiresAt: number | null;
  /** How many times this identical message arrived; 1 for a lone toast. */
  count: number;
}

export interface ToastState {
  toasts: Toast[];
  nextId: number;
}

/** Time on screen, per kind. `null` means "until dismissed" — errors only. */
export const TOAST_TTL_MS: Record<ToastKind, number | null> = {
  info: 3000,
  success: 4000,
  error: null,
};

/** The stack is a corner of the window, not a log; the Build panel is the log. */
export const MAX_VISIBLE = 4;

export const emptyToasts = (): ToastState => ({ toasts: [], nextId: 1 });

/** Trim to MAX_VISIBLE: oldest non-error first, and only then the oldest
 *  error. Mutates the array it is handed — callers pass a fresh copy.
 *
 *  The toast being pushed — the last element — is never a drop candidate.
 *  It looks like a detail and is not: errors never expire, so four of them
 *  fill the stack permanently, and a rule that hunts for "the oldest
 *  non-error" would find only the new arrival and splice it straight back
 *  out. The surface would go silently and permanently deaf after the fourth
 *  failure. The cap exists to protect errors from chatter, never to mute
 *  what just happened. */
function capped(toasts: Toast[]): Toast[] {
  while (toasts.length > MAX_VISIBLE) {
    const i = toasts.slice(0, -1).findIndex((t) => t.kind !== "error");
    toasts.splice(i === -1 ? 0 : i, 1);
  }
  return toasts;
}

/** Append a toast, or bump the newest one when it is the same message again.
 *  Dedupe deliberately looks at the **newest** toast only: an identical
 *  message with something else in between is a second event, not a repeat. */
export function pushToast(
  s: ToastState,
  kind: ToastKind,
  message: string,
  now: number,
): ToastState {
  const ttl = TOAST_TTL_MS[kind];
  const expiresAt = ttl === null ? null : now + ttl;
  const newest = s.toasts[s.toasts.length - 1];

  if (newest && newest.kind === kind && newest.message === message) {
    const bumped: Toast = {
      ...newest,
      count: newest.count + 1,
      // Refreshed, not extended from the original: a burst should stay up for
      // one full TTL after the *last* repeat.
      expiresAt,
    };
    return {
      toasts: [...s.toasts.slice(0, -1), bumped],
      nextId: s.nextId,
    };
  }

  const toast: Toast = {
    id: s.nextId,
    kind,
    message,
    createdAt: now,
    expiresAt,
    count: 1,
  };
  return {
    toasts: capped([...s.toasts, toast]),
    nextId: s.nextId + 1,
  };
}

export function dismissToast(s: ToastState, id: number): ToastState {
  const toasts = s.toasts.filter((t) => t.id !== id);
  return toasts.length === s.toasts.length ? s : { ...s, toasts };
}

/** Drop everything whose deadline has been reached. Returns the **same
 *  reference** when nothing expired, so a caller can bail out of a re-render
 *  on the timer tick that found nothing to do. */
export function expireToasts(s: ToastState, now: number): ToastState {
  const toasts = s.toasts.filter((t) => t.expiresAt === null || t.expiresAt > now);
  return toasts.length === s.toasts.length ? s : { ...s, toasts };
}

/** The earliest deadline on the stack, or null when nothing can expire —
 *  which is the signal to arm no timer at all rather than poll. */
export function nextExpiry(s: ToastState): number | null {
  let earliest: number | null = null;
  for (const t of s.toasts) {
    if (t.expiresAt === null) continue;
    if (earliest === null || t.expiresAt < earliest) earliest = t.expiresAt;
  }
  return earliest;
}

/** Map the existing `notify(msg, isError)` call shape onto a kind, so the
 *  ~90 call sites in `App.tsx` need no argument of their own. The `✓` prefix
 *  is already the app's own convention for "that worked". */
export function kindOfNotify(msg: string, isError = false): ToastKind {
  if (isError) return "error";
  return msg.startsWith("✓") ? "success" : "info";
}
