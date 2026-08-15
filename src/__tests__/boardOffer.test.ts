import { describe, expect, it } from "vitest";
import { boardOffer, driftLabel } from "../boardOffer";
import type { FleetEntry, FleetSnapshot, FlashRecord } from "../api";

const rec = (over: Partial<FlashRecord> = {}): FlashRecord => ({
  project_dir: "/home/u/Projects/porch-light",
  tag: "flash/2026-08-10T0915",
  branch: "main",
  commit: "a".repeat(40),
  at: 1_786_000_000,
  ...over,
});

const board = (over: Partial<FleetEntry> = {}): FleetEntry => ({
  id: "3c:84:27:aa:bb:cc",
  id_kind: "mac",
  nickname: "porch-light",
  chip_type: null,
  board_name: null,
  fqbns: [],
  last_port: "/dev/ttyACM0",
  vid: null,
  pid: null,
  first_seen: 0,
  last_seen: 100,
  last_flash: rec(),
  ...over,
});

const snap = (boards: FleetEntry[], online?: string[]): FleetSnapshot => ({
  boards,
  online: online ?? boards.map((b) => b.id),
  unidentified: [],
});

const none: ReadonlySet<string> = new Set();

describe("boardOffer", () => {
  it("offers the project a plugged-in board was last flashed from", () => {
    const o = boardOffer(snap([board()]), null, none, none);
    expect(o?.boardId).toBe("3c:84:27:aa:bb:cc");
    expect(o?.title).toBe("porch-light");
    expect(o?.rec.project_dir).toBe("/home/u/Projects/porch-light");
  });

  it("names the board by the shared fleet chain, not the nickname alone", () => {
    const o = boardOffer(
      snap([board({ nickname: null, board_name: "Arduino UNO Q" })]),
      null,
      none,
      none,
    );
    expect(o?.title).toBe("Arduino UNO Q");
  });

  it("says nothing without a fleet snapshot", () => {
    expect(boardOffer(null, null, none, none)).toBeNull();
  });

  it("says nothing for a board with no flash record", () => {
    expect(boardOffer(snap([board({ last_flash: null })]), null, none, none)).toBeNull();
  });

  it("ignores a board that is not plugged in", () => {
    // Its record is a memory; the offer is about what is on the bench now.
    expect(boardOffer(snap([board()], []), null, none, none)).toBeNull();
  });

  it("says nothing when that project is already open", () => {
    const o = boardOffer(snap([board()]), "/home/u/Projects/porch-light", none, none);
    expect(o).toBeNull();
  });

  it("still offers when a different project is open", () => {
    const o = boardOffer(snap([board()]), "/home/u/Projects/something-else", none, none);
    expect(o?.rec.project_dir).toBe("/home/u/Projects/porch-light");
  });

  it("stays quiet once dismissed this session", () => {
    const o = boardOffer(snap([board()]), null, new Set(["3c:84:27:aa:bb:cc"]), none);
    expect(o).toBeNull();
  });

  it("stays quiet for a board flashed moments ago", () => {
    // The regression this exists for: flashing resets the board, it
    // re-enumerates, and without the cooldown the act of flashing offers you
    // the project you are already in.
    const o = boardOffer(snap([board()]), null, none, new Set(["3c:84:27:aa:bb:cc"]));
    expect(o).toBeNull();
  });

  it("picks the most recently seen when several boards qualify", () => {
    const older = board({ id: "aa:11", last_seen: 100, nickname: "older" });
    const newer = board({ id: "bb:22", last_seen: 500, nickname: "newer" });
    expect(boardOffer(snap([older, newer]), null, none, none)?.title).toBe("newer");
    expect(boardOffer(snap([newer, older]), null, none, none)?.title).toBe("newer");
  });

  it("falls through to the next candidate when the best one is suppressed", () => {
    const a = board({ id: "aa:11", last_seen: 500, nickname: "flashed-just-now" });
    const b = board({ id: "bb:22", last_seen: 100, nickname: "other" });
    const o = boardOffer(snap([a, b]), null, none, new Set(["aa:11"]));
    expect(o?.title).toBe("other");
  });
});

describe("driftLabel", () => {
  it("distinguishes identical from moved", () => {
    expect(driftLabel(0, "flash/x")).toBe("still at flash/x");
    expect(driftLabel(1, "flash/x")).toBe("1 commit ahead of flash/x");
    expect(driftLabel(4, "flash/x")).toBe("4 commits ahead of flash/x");
  });

  it("distinguishes 'cannot compare' from 'identical'", () => {
    // A deleted tag, a re-cloned repo, a force-push — genuinely not the same
    // answer as "no drift", and worded so it cannot be misread as one.
    expect(driftLabel(null, "flash/x")).toBe("flash/x is no longer in this repository");
  });
});
