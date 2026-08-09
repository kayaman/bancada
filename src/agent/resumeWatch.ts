// Resume-failure state machine for "Continue this chat"'s native
// `--resume <session_id>` attempt.
//
// The CLI gives no direct "resume failed" signal: an unknown/stale session
// id (or a `claude` build too old for the flag) exits fast without ever
// emitting a `system`/`init` line, while a *successful* resume emits one
// just like a fresh session does. So the caller cannot know, event by
// event, whether what it's looking at belongs to a session that is about to
// prove itself real — every event has to be held back until the verdict is
// in:
//
//   WATCHING  — buffering every offered event, waiting for either the
//               child's own `system/init` (resume worked) or its `closed`
//               (resume failed, exited before saying anything).
//   CONFIRMED — init arrived (or the timeout backstop fired first, so a
//               genuinely live session is never left invisible forever):
//               `onConfirmed` fires exactly once (even when the buffer was
//               empty and no init ever arrived — the only signal for that
//               case, since `onDeliver` is not called at all when there is
//               nothing to flush), then the buffer flushes through
//               `onDeliver` in original order, the init event (if any) is
//               delivered last, and the watch goes inert — `offerEvent`
//               returns false from then on so the caller's ordinary
//               event-handling path takes back over.
//   FAILED    — `closed` with the watched pid arrived before init: the
//               buffer is dropped (nothing in it ever painted, so nothing
//               needs undoing) and `onFailed` fires once, letting the
//               caller fall back to a facts-only fresh session.
//
// Pure state machine, no Tauri/DOM — timers are the only side effect, which
// is exactly what makes it testable with `vi.useFakeTimers()`.

const DEFAULT_TIMEOUT_MS = 20000;

export interface ResumeWatch {
  /** Offer one `agent://event` payload. Returns true if the watch consumed
   *  it (buffered, or handled the init transition) — the caller must not
   *  also route a consumed event through its normal handling. */
  offerEvent(ev: unknown): boolean;
  /** Offer an `agent://closed` payload. Returns true if the watch consumed
   *  it (the pid matched and the watch was still live). */
  offerClosed(payload: { pid?: number }): boolean;
  /** Tear down: clears the timeout, drops any buffered events, goes dead.
   *  Neither `onDeliver` nor `onFailed` fires as a result of cancelling. */
  cancel(): void;
}

function isInitEvent(ev: unknown): boolean {
  return (
    typeof ev === "object" &&
    ev !== null &&
    (ev as Record<string, unknown>).type === "system" &&
    (ev as Record<string, unknown>).subtype === "init"
  );
}

export function createResumeWatch(opts: {
  pid: number;
  timeoutMs?: number;
  onDeliver: (ev: unknown) => void;
  onFailed: () => void;
  /** Fires exactly once, on the WATCHING -> CONFIRMED transition, before any
   *  buffered events are flushed through `onDeliver` — including when the
   *  buffer is empty and the timeout backstop fired with no init ever seen,
   *  the one case `onDeliver` itself can never signal. Never fires on the
   *  FAILED path. Optional: callers that don't need a CONFIRMED signal
   *  independent of `onDeliver` can omit it. */
  onConfirmed?: () => void;
}): ResumeWatch {
  const { pid, onDeliver, onFailed, onConfirmed } = opts;
  const timeoutMs = opts.timeoutMs ?? DEFAULT_TIMEOUT_MS;

  let watching = true;
  let buffer: unknown[] = [];

  let timer: ReturnType<typeof setTimeout> | undefined = setTimeout(() => {
    timer = undefined;
    confirm(undefined); // no init ever arrived — flush what we have anyway
  }, timeoutMs);

  function stopTimer(): void {
    if (timer !== undefined) {
      clearTimeout(timer);
      timer = undefined;
    }
  }

  /** WATCHING -> CONFIRMED: signal `onConfirmed` (fires even on an empty
   *  buffer — the timeout-backstop-with-nothing-buffered case, which
   *  `onDeliver` alone cannot signal), then flush the buffer in order, then
   *  (if given) the init event itself, then go inert. */
  function confirm(initEvent: unknown): void {
    stopTimer();
    watching = false;
    onConfirmed?.();
    const flushed = buffer;
    buffer = [];
    for (const ev of flushed) onDeliver(ev);
    if (initEvent !== undefined) onDeliver(initEvent);
  }

  return {
    offerEvent(ev: unknown): boolean {
      if (!watching) return false;
      if (isInitEvent(ev)) {
        confirm(ev);
        return true;
      }
      buffer.push(ev);
      return true;
    },

    offerClosed(payload: { pid?: number }): boolean {
      if (!watching) return false;
      if (payload.pid !== pid) return false;
      stopTimer();
      watching = false;
      buffer = [];
      onFailed();
      return true;
    },

    cancel(): void {
      stopTimer();
      watching = false;
      buffer = [];
    },
  };
}
