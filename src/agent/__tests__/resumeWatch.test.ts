import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createResumeWatch } from "../resumeWatch";

// createResumeWatch is the state machine that decides whether a
// `--resume <session_id>` attempt actually landed. The CLI gives no direct
// "resume failed" signal: an unknown/stale session id (or a CLI too old for
// the flag) just exits fast without ever emitting `system/init`, while a
// successful resume emits `system/init` like any fresh session. So every
// event in between must be held back (buffered) until either the init
// arrives (CONFIRMED) or the child dies first (FAILED) — and a live session
// must never go invisible, hence the timeout backstop.

const initEvent = { type: "system", subtype: "init", session_id: "s1" };

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

describe("createResumeWatch: WATCHING buffers events until init", () => {
  it("buffers non-init events, consuming each (offerEvent returns true)", () => {
    const onDeliver = vi.fn();
    const onFailed = vi.fn();
    const watch = createResumeWatch({ pid: 100, onDeliver, onFailed });

    expect(watch.offerEvent({ type: "stream_event" })).toBe(true);
    expect(watch.offerEvent({ type: "assistant" })).toBe(true);
    expect(onDeliver).not.toHaveBeenCalled();
    expect(onFailed).not.toHaveBeenCalled();
  });

  it("flushes the buffer through onDeliver IN ORDER, then delivers the init event, on init", () => {
    const onDeliver = vi.fn();
    const onFailed = vi.fn();
    const watch = createResumeWatch({ pid: 100, onDeliver, onFailed });

    const ev1 = { type: "stream_event", n: 1 };
    const ev2 = { type: "assistant", n: 2 };
    watch.offerEvent(ev1);
    watch.offerEvent(ev2);
    const consumed = watch.offerEvent(initEvent);

    expect(consumed).toBe(true);
    expect(onDeliver.mock.calls.map((c) => c[0])).toEqual([ev1, ev2, initEvent]);
    expect(onFailed).not.toHaveBeenCalled();
  });
});

describe("createResumeWatch: CONFIRMED transition (init)", () => {
  it("after init, offerEvent returns false forever — pass-through mode", () => {
    const onDeliver = vi.fn();
    const watch = createResumeWatch({ pid: 100, onDeliver, onFailed: vi.fn() });
    watch.offerEvent(initEvent);
    onDeliver.mockClear();

    expect(watch.offerEvent({ type: "assistant", text: "later" })).toBe(false);
    expect(watch.offerEvent({ type: "result" })).toBe(false);
    // pass-through: the watch does not deliver post-CONFIRMED events itself —
    // the caller's normal (non-watch) path handles them.
    expect(onDeliver).not.toHaveBeenCalled();
  });

  it("cancels the timeout timer on init — no late spurious flush", () => {
    const onDeliver = vi.fn();
    const watch = createResumeWatch({ pid: 100, timeoutMs: 20000, onDeliver, onFailed: vi.fn() });
    watch.offerEvent(initEvent);
    onDeliver.mockClear();

    vi.advanceTimersByTime(30000);
    expect(onDeliver).not.toHaveBeenCalled();
  });

  it("calls onConfirmed once on the init transition, alongside the flush", () => {
    const onDeliver = vi.fn();
    const onConfirmed = vi.fn();
    const watch = createResumeWatch({
      pid: 100,
      onDeliver,
      onFailed: vi.fn(),
      onConfirmed,
    });
    watch.offerEvent({ type: "stream_event" });
    watch.offerEvent(initEvent);

    expect(onConfirmed).toHaveBeenCalledTimes(1);
    expect(onDeliver).toHaveBeenCalledTimes(2); // buffered event + init event
  });
});

describe("createResumeWatch: FAILED transition (closed before init)", () => {
  it("a closed with the matching pid drops the buffer and calls onFailed once", () => {
    const onDeliver = vi.fn();
    const onFailed = vi.fn();
    const onConfirmed = vi.fn();
    const watch = createResumeWatch({ pid: 100, onDeliver, onFailed, onConfirmed });

    watch.offerEvent({ type: "stream_event" });
    watch.offerEvent({ type: "assistant" });
    const consumed = watch.offerClosed({ pid: 100 });

    expect(consumed).toBe(true);
    expect(onFailed).toHaveBeenCalledTimes(1);
    expect(onDeliver).not.toHaveBeenCalled(); // buffer dropped, not flushed
    expect(onConfirmed).not.toHaveBeenCalled(); // FAILED, not CONFIRMED
  });

  it("a closed with a DIFFERENT pid is not consumed and does not fail the watch", () => {
    const onDeliver = vi.fn();
    const onFailed = vi.fn();
    const watch = createResumeWatch({ pid: 100, onDeliver, onFailed });

    const consumed = watch.offerClosed({ pid: 999 });

    expect(consumed).toBe(false);
    expect(onFailed).not.toHaveBeenCalled();
    // the watch is still alive afterwards
    expect(watch.offerEvent({ type: "assistant" })).toBe(true);
  });

  it("does not double-call onFailed if offerClosed somehow fires twice", () => {
    const onFailed = vi.fn();
    const watch = createResumeWatch({ pid: 100, onDeliver: vi.fn(), onFailed });
    watch.offerClosed({ pid: 100 });
    const secondConsumed = watch.offerClosed({ pid: 100 });

    expect(secondConsumed).toBe(false); // already dead
    expect(onFailed).toHaveBeenCalledTimes(1);
  });

  it("cancels the timeout timer on FAILED — no late spurious flush", () => {
    const onDeliver = vi.fn();
    const watch = createResumeWatch({ pid: 100, timeoutMs: 20000, onDeliver, onFailed: vi.fn() });
    watch.offerClosed({ pid: 100 });

    vi.advanceTimersByTime(30000);
    expect(onDeliver).not.toHaveBeenCalled();
  });

  it("offerEvent after FAILED returns false (dead, not buffering)", () => {
    const watch = createResumeWatch({ pid: 100, onDeliver: vi.fn(), onFailed: vi.fn() });
    watch.offerClosed({ pid: 100 });
    expect(watch.offerEvent({ type: "assistant" })).toBe(false);
  });
});

describe("createResumeWatch: timeout backstop", () => {
  it("defaults to 20000ms, then CONFIRMED-flushes the buffer (no init ever arrived)", () => {
    const onDeliver = vi.fn();
    const onFailed = vi.fn();
    const watch = createResumeWatch({ pid: 100, onDeliver, onFailed });

    const ev1 = { type: "stream_event", n: 1 };
    watch.offerEvent(ev1);

    vi.advanceTimersByTime(19999);
    expect(onDeliver).not.toHaveBeenCalled();

    vi.advanceTimersByTime(1);
    expect(onDeliver).toHaveBeenCalledTimes(1);
    expect(onDeliver).toHaveBeenCalledWith(ev1);
    expect(onFailed).not.toHaveBeenCalled();

    // and pass-through afterwards
    expect(watch.offerEvent({ type: "assistant" })).toBe(false);
  });

  it("calls onConfirmed on the timeout backstop even with an EMPTY buffer — the case " +
    "onDeliver alone cannot signal, since it is never invoked when there is nothing " +
    "buffered and no init event", () => {
    const onDeliver = vi.fn();
    const onConfirmed = vi.fn();
    createResumeWatch({
      pid: 100,
      onDeliver,
      onFailed: vi.fn(),
      onConfirmed,
    });

    vi.advanceTimersByTime(20000);

    expect(onConfirmed).toHaveBeenCalledTimes(1);
    expect(onDeliver).not.toHaveBeenCalled(); // nothing buffered, no init — nothing to flush
  });

  it("respects a custom timeoutMs", () => {
    const onDeliver = vi.fn();
    const watch = createResumeWatch({ pid: 100, timeoutMs: 5000, onDeliver, onFailed: vi.fn() });
    watch.offerEvent({ type: "x" });

    vi.advanceTimersByTime(4999);
    expect(onDeliver).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(onDeliver).toHaveBeenCalledTimes(1);
  });

  it("a closed that arrives exactly at the failure window still wins over a not-yet-fired timeout", () => {
    const onDeliver = vi.fn();
    const onFailed = vi.fn();
    const watch = createResumeWatch({ pid: 100, timeoutMs: 20000, onDeliver, onFailed });
    watch.offerEvent({ type: "x" });
    vi.advanceTimersByTime(19000);
    watch.offerClosed({ pid: 100 });

    vi.advanceTimersByTime(5000); // past the original timeout
    expect(onFailed).toHaveBeenCalledTimes(1);
    expect(onDeliver).not.toHaveBeenCalled();
  });
});

describe("createResumeWatch: cancel()", () => {
  it("clears the timer, drops the buffer, and goes dead — no callbacks ever fire", () => {
    const onDeliver = vi.fn();
    const onFailed = vi.fn();
    const watch = createResumeWatch({ pid: 100, onDeliver, onFailed });
    watch.offerEvent({ type: "x" });

    watch.cancel();
    vi.advanceTimersByTime(60000);

    expect(onDeliver).not.toHaveBeenCalled();
    expect(onFailed).not.toHaveBeenCalled();
  });

  it("offerEvent/offerClosed after cancel both return false", () => {
    const watch = createResumeWatch({ pid: 100, onDeliver: vi.fn(), onFailed: vi.fn() });
    watch.cancel();
    expect(watch.offerEvent({ type: "x" })).toBe(false);
    expect(watch.offerClosed({ pid: 100 })).toBe(false);
  });

  it("cancel is safe to call twice", () => {
    const watch = createResumeWatch({ pid: 100, onDeliver: vi.fn(), onFailed: vi.fn() });
    watch.cancel();
    expect(() => watch.cancel()).not.toThrow();
  });
});
