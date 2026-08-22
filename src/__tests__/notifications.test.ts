import { describe, expect, it } from "vitest";
import {
  MAX_VISIBLE,
  TOAST_TTL_MS,
  dismissToast,
  emptyToasts,
  expireToasts,
  kindOfNotify,
  nextExpiry,
  pushToast,
} from "../notifications";

const T0 = 1_700_000_000_000;

describe("pushToast", () => {
  it("stamps the per-kind TTL, and never one for an error", () => {
    let s = emptyToasts();
    s = pushToast(s, "info", "scanning ports", T0);
    s = pushToast(s, "success", "✓ Compiled", T0);
    s = pushToast(s, "error", "upload failed", T0);

    expect(s.toasts.map((t) => t.expiresAt)).toEqual([
      T0 + TOAST_TTL_MS.info!,
      T0 + TOAST_TTL_MS.success!,
      null,
    ]);
    expect(TOAST_TTL_MS.info).toBe(3000);
    expect(TOAST_TTL_MS.success).toBe(4000);
    expect(TOAST_TTL_MS.error).toBe(null);
  });

  it("hands out increasing ids and keeps the newest last", () => {
    let s = emptyToasts();
    s = pushToast(s, "info", "one", T0);
    s = pushToast(s, "info", "two", T0);
    expect(s.toasts.map((t) => t.id)).toEqual([1, 2]);
    expect(s.toasts.map((t) => t.message)).toEqual(["one", "two"]);
    expect(s.nextId).toBe(3);
  });

  it("collapses a repeat of the newest toast into a count", () => {
    let s = emptyToasts();
    s = pushToast(s, "info", "board detached", T0);
    s = pushToast(s, "info", "board detached", T0 + 500);

    expect(s.toasts.length).toBe(1);
    expect(s.toasts[0].count).toBe(2);
    expect(s.toasts[0].id).toBe(1);
    expect(s.nextId).toBe(2);
    // the repeat refreshes the deadline, so a burst stays visible
    expect(s.toasts[0].expiresAt).toBe(T0 + 500 + 3000);
    expect(s.toasts[0].createdAt).toBe(T0);
  });

  it("does not collapse across kinds, or across a different message", () => {
    let s = emptyToasts();
    s = pushToast(s, "info", "same text", T0);
    s = pushToast(s, "success", "same text", T0);
    s = pushToast(s, "success", "other text", T0);
    expect(s.toasts.map((t) => t.count)).toEqual([1, 1, 1]);
    expect(s.toasts.length).toBe(3);
  });

  it("only dedupes against the newest — an older twin is left alone", () => {
    let s = emptyToasts();
    s = pushToast(s, "info", "a", T0);
    s = pushToast(s, "info", "b", T0);
    s = pushToast(s, "info", "a", T0);
    expect(s.toasts.map((t) => t.message)).toEqual(["a", "b", "a"]);
  });

  it("caps at MAX_VISIBLE, dropping the oldest non-error before any error", () => {
    let s = emptyToasts();
    s = pushToast(s, "error", "boom", T0);
    s = pushToast(s, "info", "i2", T0);
    s = pushToast(s, "info", "i3", T0);
    s = pushToast(s, "info", "i4", T0);
    expect(s.toasts.length).toBe(MAX_VISIBLE);

    s = pushToast(s, "info", "i5", T0);
    expect(s.toasts.length).toBe(MAX_VISIBLE);
    expect(s.toasts.map((t) => t.message)).toEqual(["boom", "i3", "i4", "i5"]);
  });

  it("falls back to dropping the oldest error when every toast is one", () => {
    let s = emptyToasts();
    for (const m of ["e1", "e2", "e3", "e4", "e5"])
      s = pushToast(s, "error", m, T0);
    expect(s.toasts.map((t) => t.message)).toEqual(["e2", "e3", "e4", "e5"]);
  });
});

describe("expireToasts", () => {
  it("drops a toast the moment its deadline is reached", () => {
    let s = emptyToasts();
    s = pushToast(s, "info", "gone soon", T0);
    expect(expireToasts(s, T0 + 2999).toasts.length).toBe(1);
    expect(expireToasts(s, T0 + 3000).toasts.length).toBe(0);
  });

  it("keeps an error forever", () => {
    let s = emptyToasts();
    s = pushToast(s, "error", "upload failed", T0);
    s = expireToasts(s, T0 + 3_600_000);
    expect(s.toasts.map((t) => t.message)).toEqual(["upload failed"]);
  });

  it("returns the same reference when nothing expired", () => {
    let s = emptyToasts();
    s = pushToast(s, "info", "still here", T0);
    expect(expireToasts(s, T0 + 100)).toBe(s);
  });
});

describe("nextExpiry", () => {
  it("is the earliest deadline on the stack", () => {
    let s = emptyToasts();
    s = pushToast(s, "success", "✓ Compiled", T0);
    s = pushToast(s, "info", "scanning", T0 + 100);
    expect(nextExpiry(s)).toBe(T0 + 100 + 3000);
  });

  it("is null when nothing can expire", () => {
    let s = emptyToasts();
    expect(nextExpiry(s)).toBe(null);
    s = pushToast(s, "error", "boom", T0);
    s = pushToast(s, "error", "bang", T0);
    expect(nextExpiry(s)).toBe(null);
  });
});

describe("dismissToast", () => {
  it("removes exactly the given id and leaves the rest", () => {
    let s = emptyToasts();
    s = pushToast(s, "info", "a", T0);
    s = pushToast(s, "info", "b", T0);
    s = dismissToast(s, 1);
    expect(s.toasts.map((t) => t.message)).toEqual(["b"]);
    expect(s.nextId).toBe(3);
  });

  it("is a no-op for an unknown id", () => {
    let s = emptyToasts();
    s = pushToast(s, "info", "a", T0);
    expect(dismissToast(s, 99).toasts.length).toBe(1);
  });
});

describe("kindOfNotify", () => {
  it("maps the App.tsx notify() call shape onto a kind", () => {
    expect(kindOfNotify("upload failed: no port", true)).toBe("error");
    expect(kindOfNotify("✓ Compiled in 4.1s")).toBe("success");
    expect(kindOfNotify("Compiling…")).toBe("info");
    expect(kindOfNotify("Compiling…", false)).toBe("info");
    // isError wins over the checkmark
    expect(kindOfNotify("✓ but actually not", true)).toBe("error");
  });
});
