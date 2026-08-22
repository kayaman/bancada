// Build progress, read out of the uploader's own stdout. Pure: one line in,
// one immutable state out, so the whole thing is testable against a captured
// transcript with no board attached.
//
// There is no progress API to ask. `arduino-cli` streams whatever esptool or
// avrdude printed, and those two disagree about almost everything:
//
//   - **esptool** writes several independent flash regions per upload —
//     bootloader, partition table, boot_app0, sketch — and restarts its
//     percentage at 0 for each. Trusting that percentage directly makes the
//     bar snap back to nothing three times per flash. It does, however,
//     announce every region up front (`Flash will be erased from … to …`), so
//     the regions can be size-weighted into a single monotonic fraction. That
//     matters because the four regions are wildly uneven: the bootloader is
//     ~3% of a typical flash, not 25%.
//   - **avrdude** prints a `#` progress bar only to a TTY. Through a pipe
//     there is no percentage at all, so the honest answer is a note ("Writing
//     flash") and no number — never a fabricated one.
//
// Every matcher is anchored at the start of the line: `Writing at …` inside a
// wrapped or prefixed log line is somebody quoting the uploader, not the
// uploader talking.

export type BuildPhase = "compiling" | "uploading";

/** One flash region, inclusive of both ends — the form esptool prints. */
export interface Segment {
  from: number;
  to: number;
}

export interface BuildProgress {
  op: "compile" | "upload";
  phase: BuildPhase;
  segments: Segment[];
  /** Index into `segments`, or -1 while no region has been identified. */
  segIndex: number;
  /** Percent within the current region, as the uploader reported it. */
  segPercent: number | null;
  /** 0..1 across the whole upload, or null when nothing can be known. */
  fraction: number | null;
  /** Short human label for what the uploader is doing right now. */
  note: string | null;
}

export function startProgress(op: "compile" | "upload"): BuildProgress {
  return {
    op,
    phase: "compiling",
    segments: [],
    segIndex: -1,
    segPercent: null,
    fraction: null,
    note: null,
  };
}

const SKETCH_USES = /^Sketch uses \d+ bytes/;
const ERASE = /^Flash will be erased from 0x([0-9a-f]+) to 0x([0-9a-f]+)/i;
// esptool 4.x prints `Writing at 0x00010000... (12 %)`; esptool 5 tightened
// the spacing and can print a decimal. Both are accepted; the decimal is
// dropped rather than rounded, matching the "floor, never overstate" rule the
// elapsed clock uses.
const WRITING =
  /^Writing at 0x([0-9a-f]+)\.\.\.\s*\(\s*(\d{1,3})(?:\.\d+)?\s*%\s*\)/i;
const WROTE = /^Wrote \d+ bytes/;
const VERIFIED = /^Hash of data verified/;
const DONE = /^Leaving|^Hard resetting/;
const AVR_WRITING = /^avrdude: writing flash/;
const AVR_VERIFYING = /^avrdude: verifying/;
const AVR_DONE = /^avrdude done/;
const NEW_PORT = /^New upload port:/;

const size = (s: Segment) => s.to - s.from + 1;

/** Size-weighted position across every known region, or the bare per-region
 *  percent when no region was ever announced. The fallback restarts at each
 *  region — visibly wrong, but wrong in a way that is still moving, which
 *  beats a frozen bar. */
function fractionAt(
  segments: Segment[],
  segIndex: number,
  percent: number,
): number {
  if (segIndex < 0 || segIndex >= segments.length) return percent / 100;
  const total = segments.reduce((n, s) => n + size(s), 0);
  if (total <= 0) return percent / 100;
  let done = 0;
  for (let i = 0; i < segIndex; i++) done += size(segments[i]);
  done += (size(segments[segIndex]) * percent) / 100;
  return Math.min(done / total, 1);
}

/** Fold one line of uploader output into the progress state. Returns the
 *  **same reference** for anything it does not understand — which is the vast
 *  majority of a build log, and is what lets a caller skip the re-render. */
export function reduceBuildLine(
  p: BuildProgress,
  line: string,
): BuildProgress {
  if (SKETCH_USES.test(line)) {
    // For a plain compile this is the last thing that happens, not a handover:
    // a compile must never claim to be uploading.
    if (p.op !== "upload") return p;
    return { ...p, phase: "uploading", note: "Connecting…" };
  }

  const erase = ERASE.exec(line);
  if (erase) {
    return {
      ...p,
      segments: [
        ...p.segments,
        { from: parseInt(erase[1], 16), to: parseInt(erase[2], 16) },
      ],
    };
  }

  const writing = WRITING.exec(line);
  if (writing) {
    const addr = parseInt(writing[1], 16);
    const percent = parseInt(writing[2], 10);
    const found = p.segments.findIndex((s) => addr >= s.from && addr <= s.to);
    const segIndex = found === -1 ? p.segIndex : found;
    return {
      ...p,
      segIndex,
      segPercent: percent,
      fraction: fractionAt(p.segments, segIndex, percent),
      note: "Writing",
    };
  }

  if (WROTE.test(line)) {
    return {
      ...p,
      segPercent: 100,
      fraction: fractionAt(p.segments, p.segIndex, 100),
      note: p.note,
    };
  }

  if (VERIFIED.test(line)) return { ...p, note: "Verified" };
  if (DONE.test(line)) return { ...p, fraction: 1, note: "Resetting" };

  // avrdude, through a pipe: notes only, and a number only once it is over.
  if (AVR_WRITING.test(line)) return { ...p, note: "Writing flash" };
  if (AVR_VERIFYING.test(line)) return { ...p, note: "Verifying" };
  if (AVR_DONE.test(line)) return { ...p, fraction: 1, note: "Done" };

  // arduino-cli's own line, printed after the board re-enumerates.
  if (NEW_PORT.test(line)) return { ...p, note: "Resetting" };

  return p;
}
