import { describe, expect, it } from "vitest";
import {
  type BuildProgress,
  reduceBuildLine,
  startProgress,
} from "../buildProgress";

// A real `arduino-cli upload` transcript for an ESP32-S3, esptool 4.x format:
// bootloader at 0x0, the partition table at 0x8000, boot_app0 at 0xe000 and
// the sketch at 0x10000. The four `Flash will be erased` lines all arrive
// before the first `Writing at`, which is what makes a size-weighted fraction
// possible at all.
//
// TODO: paste a transcript from a real flash on this bench when one is to
// hand — esptool 5 tightened the spacing (`(12%)` with no space) and the
// parser is written to accept both, but only a live line can prove it.
const ESPTOOL: readonly string[] = [
  "Sketch uses 274573 bytes (20%) of program storage space. Maximum is 1310720 bytes.",
  "Global variables use 21140 bytes (6%) of dynamic memory.",
  "esptool.py v4.5.1",
  "Serial port /dev/ttyACM0",
  "Connecting....",
  "Chip is ESP32-S3 (QFN56) (revision v0.2)",
  "Auto-detected Flash size: 8MB",
  "Flash will be erased from 0x00000000 to 0x00004fff...",
  "Flash will be erased from 0x00008000 to 0x00008fff...",
  "Flash will be erased from 0x0000e000 to 0x0000ffff...",
  "Flash will be erased from 0x00010000 to 0x000b2fff...",
  "Compressed 20480 bytes to 13000...",
  "Writing at 0x00000000... (20 %)",
  "Writing at 0x00001000... (40 %)",
  "Writing at 0x00002800... (60 %)",
  "Writing at 0x00003c00... (80 %)",
  "Writing at 0x00004800... (100 %)",
  "Wrote 20480 bytes (13000 compressed) at 0x00000000 in 0.6 seconds...",
  "Hash of data verified.",
  "Compressed 4096 bytes to 23...",
  "Writing at 0x00008000... (100 %)",
  "Wrote 4096 bytes (23 compressed) at 0x00008000 in 0.1 seconds...",
  "Hash of data verified.",
  "Compressed 8192 bytes to 47...",
  "Writing at 0x0000e000... (100 %)",
  "Wrote 8192 bytes (47 compressed) at 0x0000e000 in 0.1 seconds...",
  "Hash of data verified.",
  "Compressed 667648 bytes to 401220...",
  "Writing at 0x00010000... (12 %)",
  "Writing at 0x0001d4a0... (25 %)",
  "Writing at 0x00029b90... (37 %)",
  "Writing at 0x0005a000... (68 %)",
  "Writing at 0x000ac000... (100 %)",
  "Wrote 667648 bytes (401220 compressed) at 0x00010000 in 9.3 seconds...",
  "Hash of data verified.",
  "Leaving...",
  "Hard resetting via RTS pin...",
  "New upload port: /dev/ttyACM0 (serial)",
];

/** The same flash with the erase lines suppressed — esptool's `--no-stub`
 *  and some board recipes never print them. */
const ESPTOOL_NO_ERASE = ESPTOOL.filter(
  (l) => !l.startsWith("Flash will be erased"),
);

const AVRDUDE: readonly string[] = [
  "Sketch uses 924 bytes (2%) of program storage space. Maximum is 32256 bytes.",
  'avrdude: Version 6.3-20190619, compiled on Sep 14 2020',
  "avrdude: AVR device initialized and ready to accept instructions",
  "avrdude: Device signature = 0x1e950f (probably m328p)",
  'avrdude: reading input file "/tmp/blink.ino.hex"',
  "avrdude: writing flash (924 bytes):",
  "avrdude: 924 bytes of flash written",
  "avrdude: verifying flash memory against /tmp/blink.ino.hex:",
  "avrdude: 924 bytes of flash verified",
  "avrdude done.  Thank you.",
];

/** Feed a transcript, collecting the state after each line. */
const run = (
  op: "compile" | "upload",
  lines: readonly string[],
): BuildProgress[] => {
  let p = startProgress(op);
  return lines.map((l) => (p = reduceBuildLine(p, l)));
};

const TOTAL = 0x5000 + 0x1000 + 0x2000 + 0xa3000;

describe("startProgress", () => {
  it("begins compiling, knowing nothing", () => {
    expect(startProgress("upload")).toEqual({
      op: "upload",
      phase: "compiling",
      segments: [],
      segIndex: -1,
      segPercent: null,
      fraction: null,
      note: null,
    });
  });
});

describe("reduceBuildLine — phases", () => {
  it("flips to uploading when the sketch size is reported", () => {
    const p = reduceBuildLine(
      startProgress("upload"),
      "Sketch uses 274573 bytes (20%) of program storage space. Maximum is 1310720 bytes.",
    );
    expect(p.phase).toBe("uploading");
    expect(p.note).toBe("Connecting…");
  });

  it("never leaves compiling when there is nothing to upload", () => {
    const states = run("compile", ESPTOOL);
    expect(states.every((s) => s.phase === "compiling")).toBe(true);
    // the size line is a no-op for a compile, not a state change
    const p0 = startProgress("compile");
    expect(reduceBuildLine(p0, ESPTOOL[0])).toBe(p0);
  });
});

describe("reduceBuildLine — esptool", () => {
  it("parses every erase range into a segment", () => {
    const states = run("upload", ESPTOOL);
    expect(states[states.length - 1].segments).toEqual([
      { from: 0x0, to: 0x4fff },
      { from: 0x8000, to: 0x8fff },
      { from: 0xe000, to: 0xffff },
      { from: 0x10000, to: 0xb2fff },
    ]);
  });

  it("places a write address in the segment that contains it", () => {
    const states = run("upload", ESPTOOL);
    const at = (line: string) => states[ESPTOOL.indexOf(line)];
    expect(at("Writing at 0x00002800... (60 %)").segIndex).toBe(0);
    expect(at("Writing at 0x00008000... (100 %)").segIndex).toBe(1);
    expect(at("Writing at 0x0000e000... (100 %)").segIndex).toBe(2);
    expect(at("Writing at 0x0005a000... (68 %)").segIndex).toBe(3);
    expect(at("Writing at 0x0005a000... (68 %)").segPercent).toBe(68);
    expect(at("Writing at 0x0005a000... (68 %)").note).toBe("Writing");
  });

  it("weights the fraction by segment size, not by segment count", () => {
    const states = run("upload", ESPTOOL);
    const at = (line: string) => states[ESPTOOL.indexOf(line)].fraction;
    // the bootloader is 20 KiB of a 684 KiB flash: finishing it is ~3%,
    // not the 25% a naive "one of four segments" count would claim
    expect(at("Writing at 0x00004800... (100 %)")).toBeCloseTo(
      0x5000 / TOTAL,
      6,
    );
    expect(at("Writing at 0x00008000... (100 %)")).toBeCloseTo(
      (0x5000 + 0x1000) / TOTAL,
      6,
    );
    expect(at("Writing at 0x00010000... (12 %)")).toBeCloseTo(
      (0x8000 + 0.12 * 0xa3000) / TOTAL,
      6,
    );
  });

  it("never goes backwards across the whole transcript", () => {
    let last = 0;
    for (const p of run("upload", ESPTOOL)) {
      if (p.fraction === null) continue;
      expect(p.fraction).toBeGreaterThanOrEqual(last);
      last = p.fraction;
    }
    expect(last).toBe(1);
  });

  it("finishes at 1 and says so", () => {
    const states = run("upload", ESPTOOL);
    const verified = states[ESPTOOL.indexOf("Hash of data verified.")];
    expect(verified.note).toBe("Verified");
    expect(states[ESPTOOL.indexOf("Leaving...")].note).toBe("Resetting");
    expect(states[ESPTOOL.indexOf("Leaving...")].fraction).toBe(1);
    expect(
      states[ESPTOOL.indexOf("Hard resetting via RTS pin...")].fraction,
    ).toBe(1);
    expect(
      states[ESPTOOL.indexOf("New upload port: /dev/ttyACM0 (serial)")].note,
    ).toBe("Resetting");
  });

  it("falls back to the per-segment percent when no erase lines arrived", () => {
    const states = run("upload", ESPTOOL_NO_ERASE);
    const at = (line: string) => states[ESPTOOL_NO_ERASE.indexOf(line)];
    expect(at("Writing at 0x00000000... (20 %)").fraction).toBeCloseTo(0.2, 6);
    expect(at("Writing at 0x00004800... (100 %)").fraction).toBe(1);
    // documented cost of the fallback: the bar restarts at each segment
    expect(at("Writing at 0x00010000... (12 %)").fraction).toBeCloseTo(
      0.12,
      6,
    );
    expect(at("Writing at 0x00000000... (20 %)").segIndex).toBe(-1);
  });

  it("reads esptool 5's tighter spacing and a fractional percent", () => {
    let p = startProgress("upload");
    p = reduceBuildLine(p, "Writing at 0x00010000... (12%)");
    expect(p.segPercent).toBe(12);
    p = reduceBuildLine(p, "Writing at 0x0001d4a0...  ( 37.5 % )");
    expect(p.segPercent).toBe(37);
  });
});

describe("reduceBuildLine — avrdude", () => {
  it("gives notes but never invents a number until it is done", () => {
    const states = run("upload", AVRDUDE);
    const at = (line: string) => states[AVRDUDE.indexOf(line)];
    expect(at("avrdude: writing flash (924 bytes):").note).toBe("Writing flash");
    expect(at("avrdude: writing flash (924 bytes):").fraction).toBe(null);
    expect(
      at("avrdude: verifying flash memory against /tmp/blink.ino.hex:").note,
    ).toBe("Verifying");
    expect(
      at("avrdude: verifying flash memory against /tmp/blink.ino.hex:")
        .fraction,
    ).toBe(null);
    expect(at("avrdude done.  Thank you.").fraction).toBe(1);

    // nothing before the last line ever claimed a fraction
    const upToDone = states.slice(0, states.length - 1);
    expect(upToDone.every((s) => s.fraction === null)).toBe(true);
  });
});

describe("reduceBuildLine — irrelevant lines", () => {
  it("returns the very same object so a caller can skip the re-render", () => {
    const p = reduceBuildLine(startProgress("upload"), ESPTOOL[0]);
    for (const line of [
      "",
      "Connecting....",
      "Chip is ESP32-S3 (QFN56) (revision v0.2)",
      "Compressed 20480 bytes to 13000...",
      "avrdude: 924 bytes of flash written",
      "  Writing at 0x00010000... (12 %) in a log prefix",
      "Sketch uses lots of bytes",
    ]) {
      expect(reduceBuildLine(p, line)).toBe(p);
    }
  });
});
