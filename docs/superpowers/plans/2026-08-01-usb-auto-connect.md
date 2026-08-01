# USB Auto-Connect Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Plugging a board into USB auto-refreshes the port list, fills a free selection slot (never steals), starts serial capture via the existing effect, and greets known fleet boards by nickname — then ship it as release 0.5.0.

**Architecture:** A Rust background thread polls `serialport::available_ports()` every 2s and emits a payload-free `ports://changed` event only when the port set changes. The frontend listener reuses the existing `refreshPorts()` behind a busy guard (deferred during flash, then delayed 1500ms for USB re-enumeration to settle) and a 500ms debounce. A new pure module diffs fleet `online` ids to announce arrivals.

**Tech Stack:** Tauri v2 (Rust: `serialport` crate re-exported via `bancada_core::scope`, `tauri::Emitter`), React + TypeScript frontend, vitest for TS unit tests.

**Spec:** `docs/superpowers/specs/2026-07-31-usb-auto-connect-design.md`

## Global Constraints

- Event name is exactly `ports://changed`, no payload (matches `build://line` / `serial://line` naming style).
- Watcher tick: 2 seconds. Frontend debounce: 500 ms. Post-busy settle delay: 1500 ms.
- Selection changes only via the existing `nextSelectedPort` — no new selection logic anywhere.
- First fleet sync after launch announces nothing.
- Release is metadata-only (Cargo.toml, Cargo.lock, package.json, tauri.conf.json), version 0.5.0, GPG-signed annotated tag `v0.5.0` titled `Bancada 0.5.0 — USB auto-connect`.
- Comment style: comments state constraints the code can't show (see existing `ports.ts`); match surrounding density.

---

### Task 1: Arrival diff module (`portWatch.ts`)

**Files:**
- Create: `src/portWatch.ts`
- Test: `src/__tests__/portWatch.test.ts`

**Interfaces:**
- Consumes: `FleetSnapshot`, `FleetEntry` types from `src/api.ts` (already exist).
- Produces: `arrivals(prevOnline: string[] | null, snap: FleetSnapshot): Arrival[]` where `Arrival = { id: string; name: string; port: string | null }`. Task 3 calls this from `refreshPorts()`.

- [ ] **Step 1: Write the failing test**

Create `src/__tests__/portWatch.test.ts` (helper style mirrors `ports.test.ts`):

```ts
import { describe, expect, it } from "vitest";
import { arrivals } from "../portWatch";
import type { FleetEntry, FleetSnapshot } from "../api";

const entry = (id: string, over: Partial<FleetEntry> = {}): FleetEntry => ({
  id,
  id_kind: "mac",
  nickname: null,
  chip_type: null,
  board_name: null,
  fqbns: [],
  last_port: null,
  vid: null,
  pid: null,
  first_seen: 0,
  last_seen: 0,
  ...over,
});

const snap = (boards: FleetEntry[], online: string[]): FleetSnapshot => ({
  boards,
  online,
  unidentified: [],
});

describe("arrivals", () => {
  it("announces nothing on the first sync after launch", () => {
    const s = snap([entry("aa:bb")], ["aa:bb"]);
    expect(arrivals(null, s)).toEqual([]);
  });

  it("reports a board that came online since the previous sync", () => {
    const s = snap(
      [entry("aa:bb", { nickname: "sonar-node", last_port: "/dev/ttyACM0" })],
      ["aa:bb"],
    );
    expect(arrivals([], s)).toEqual([
      { id: "aa:bb", name: "sonar-node", port: "/dev/ttyACM0" },
    ]);
  });

  it("ignores boards that were already online", () => {
    const s = snap([entry("aa:bb"), entry("cc:dd")], ["aa:bb", "cc:dd"]);
    expect(arrivals(["aa:bb", "cc:dd"], s)).toEqual([]);
  });

  it("falls back nickname -> board_name -> id for the display name", () => {
    const s = snap(
      [
        entry("aa:bb", { board_name: "ESP32S3 Dev Module" }),
        entry("cc:dd"),
      ],
      ["aa:bb", "cc:dd"],
    );
    expect(arrivals([], s).map((a) => a.name)).toEqual([
      "ESP32S3 Dev Module",
      "cc:dd",
    ]);
  });

  it("handles simultaneous arrivals", () => {
    const s = snap([entry("aa:bb"), entry("cc:dd")], ["aa:bb", "cc:dd"]);
    expect(arrivals([], s)).toHaveLength(2);
  });

  it("does not treat departures as arrivals", () => {
    const s = snap([entry("aa:bb")], []);
    expect(arrivals(["aa:bb"], s)).toEqual([]);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/__tests__/portWatch.test.ts`
Expected: FAIL — cannot resolve `../portWatch`.

- [ ] **Step 3: Write the implementation**

Create `src/portWatch.ts`:

```ts
import type { FleetSnapshot } from "./api";

/** A board that came online since the previous fleet sync. */
export interface Arrival {
  id: string;
  /** Nickname when set, else the board model, else the raw id. */
  name: string;
  port: string | null;
}

/**
 * Boards newly online in `snap` relative to the previous sync.
 *
 * `prevOnline === null` marks the first sync after launch: boards already
 * plugged in when the app started are not "arrivals", so none are announced.
 */
export function arrivals(
  prevOnline: string[] | null,
  snap: FleetSnapshot,
): Arrival[] {
  if (prevOnline === null) return [];
  const seen = new Set(prevOnline);
  return snap.online
    .filter((id) => !seen.has(id))
    .map((id) => {
      const b = snap.boards.find((e) => e.id === id);
      return {
        id,
        name: b?.nickname ?? b?.board_name ?? id,
        port: b?.last_port ?? null,
      };
    });
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run src/__tests__/portWatch.test.ts`
Expected: PASS (6 tests).

- [ ] **Step 5: Run the whole TS suite to check nothing broke**

Run: `npm test`
Expected: all suites pass.

- [ ] **Step 6: Commit**

```bash
git add src/portWatch.ts src/__tests__/portWatch.test.ts
git commit -m "Add the fleet arrival diff for USB auto-connect"
```

---

### Task 2: Rust port watcher + event wrapper

**Files:**
- Modify: `src-tauri/src/lib.rs` (setup closure, ~line 1572; module doc comment listing events, ~line 4)
- Modify: `src/api.ts` (events section at the bottom, after `onSerialClosed`)

**Interfaces:**
- Consumes: `serialport::available_ports()` (already imported at `lib.rs:38` via `use bancada_core::scope::{self, serialport, ...}`), `tauri::Emitter` (already imported at `lib.rs:44`).
- Produces: Tauri event `ports://changed` (no payload); `onPortsChanged(cb: () => void): Promise<UnlistenFn>` in `api.ts`. Task 3 subscribes via `onPortsChanged`.

There is no unit test for this task: the only logic is set inequality, which the spec explicitly exempts. Verification is compile + the manual bring-up in Task 4.

- [ ] **Step 1: Spawn the watcher in the Tauri setup**

In `src-tauri/src/lib.rs`, inside `.setup(|app| { ... })`, after `app.manage(AppState { ... });` and before `Ok(())`:

```rust
// Hotplug watcher: enumeration (does the port exist?) is orders of
// magnitude cheaper than identification (arduino-cli), so poll the
// former and let the frontend run the latter only on a change. The
// first tick only seeds `prev` — the frontend does its own initial
// scan, and boards present at launch are not arrivals.
let watcher = app.handle().clone();
std::thread::spawn(move || {
    let mut prev: Option<std::collections::BTreeSet<String>> = None;
    loop {
        std::thread::sleep(std::time::Duration::from_secs(2));
        // A failed enumeration keeps the previous set: never emit on
        // error, never die.
        let Ok(ports) = serialport::available_ports() else {
            continue;
        };
        let names: std::collections::BTreeSet<String> =
            ports.into_iter().map(|p| p.port_name).collect();
        if prev.as_ref().is_some_and(|p| *p != names) {
            let _ = watcher.emit("ports://changed", ());
        }
        prev = Some(names);
    }
});
```

Also add `ports://changed` to the module doc comment at the top of `lib.rs` that lists emitted events (near line 4).

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p bancada`
Expected: clean (warnings only if pre-existing).

- [ ] **Step 3: Add the frontend event wrapper**

In `src/api.ts`, after `onSerialClosed`:

```ts
/** Fires when the set of serial ports on the machine changes (hotplug). */
export const onPortsChanged = (cb: () => void): Promise<UnlistenFn> =>
  listen("ports://changed", () => cb());
```

- [ ] **Step 4: Verify the frontend builds**

Run: `npm run build`
Expected: `tsc --noEmit` and vite both succeed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs src/api.ts
git commit -m "Emit ports://changed when the serial port set changes"
```

---

### Task 3: Wire auto-refresh and arrival notices into App.tsx

**Files:**
- Modify: `src/App.tsx` — refs near `const [busy, setBusy] = useState(false);` (~line 136), the mount-effect subscription list (~line 211), and `refreshPorts` (~line 256)

**Interfaces:**
- Consumes: `api.onPortsChanged` (Task 2), `arrivals` from `./portWatch` (Task 1), existing `refreshPorts`, `notify`, `busy`.
- Produces: nothing consumed by later tasks; user-visible behavior.

App.tsx logic is not covered by the vitest suite (it never has been — pure logic lives in extracted modules, which is why Task 1 exists). Verification is typecheck + manual bring-up in Task 4.

- [ ] **Step 1: Add the refs and the import**

Import at the top of `App.tsx` alongside the other local imports:

```ts
import { arrivals } from "./portWatch";
```

Below `const [busy, setBusy] = useState(false);` add:

```ts
// Hotplug plumbing. busyRef mirrors `busy` for the event listener;
// pendingScanRef queues a rescan that arrived mid-flash; prevOnlineRef
// is null until the first fleet sync so launch-time boards aren't
// announced as arrivals.
const busyRef = useRef(false);
const pendingScanRef = useRef(false);
const scanTimerRef = useRef<number | undefined>(undefined);
const prevOnlineRef = useRef<string[] | null>(null);
```

- [ ] **Step 2: Sync busyRef and flush the queued scan when busy clears**

Add an effect (near the other small effects, after the `notify` callback):

```ts
useEffect(() => {
  busyRef.current = busy;
  if (!busy && pendingScanRef.current) {
    pendingScanRef.current = false;
    // `busy` clears before a just-flashed native-USB board finishes
    // re-enumerating; scanning immediately would read the port's brief
    // absence as a detach and drop the selection. Wait out the settle
    // window (same reasoning as the post-flash monitor restart).
    window.setTimeout(refreshPorts, 1500);
  }
  // eslint-disable-next-line react-hooks/exhaustive-deps
}, [busy]);
```

- [ ] **Step 3: Subscribe to ports://changed in the mount effect**

In the `subs` array of the mount effect (alongside `api.onSerialClosed(...)`):

```ts
api.onPortsChanged(() => {
  if (busyRef.current) {
    // arduino-cli probing ports mid-flash can disrupt esptool — defer.
    pendingScanRef.current = true;
    return;
  }
  // USB enumeration surfaces sibling ports a beat apart; coalesce.
  window.clearTimeout(scanTimerRef.current);
  scanTimerRef.current = window.setTimeout(refreshPorts, 500);
}),
```

- [ ] **Step 4: Announce arrivals from the fleet sync**

In `refreshPorts`, replace the fire-and-forget

```ts
api.fleetSync(ps).catch(() => {});
```

with

```ts
api
  .fleetSync(ps)
  .then((snap) => {
    const fresh = arrivals(prevOnlineRef.current, snap);
    prevOnlineRef.current = snap.online;
    if (fresh.length > 0)
      notify(
        `⚡ ${fresh
          .map((a) => (a.port ? `${a.name} (${a.port})` : a.name))
          .join(", ")} attached`,
      );
  })
  .catch(() => {});
```

(The existing comment above the call — enrollment on plug-in, must never
break port detection — still holds and stays.)

- [ ] **Step 5: Typecheck, test, build**

Run: `npm test && npm run build`
Expected: all tests pass; build clean.

- [ ] **Step 6: Commit**

```bash
git add src/App.tsx
git commit -m "Auto-connect boards when they appear on USB"
```

---

### Task 4: End-to-end verification

**Files:** none (verification only)

- [ ] **Step 1: Full automated check**

Run: `npm test && npm run build && cargo check -p bancada`
Expected: everything green.

- [ ] **Step 2: Manual bring-up (needs the physical bench)**

Run: `npm run tauri dev`, then:
1. Launch with a board attached → no arrival notification (first-sync rule), board selected, monitor capturing.
2. Unplug it → within ~2.5s the port list empties and the monitor stops.
3. Replug it → within ~2.5s it is selected again, monitor capturing, status bar shows `⚡ <name> attached (<port>)`.
4. With board A selected, plug in board B → list shows both, selection stays on A, B's arrival is announced.
5. Flash a native-USB board → no selection churn during or just after the flash; monitor resumes on the same port.

If any step fails, STOP — do not proceed to the release task; report what happened.

Note: a loose bench contact can fake a dropout (it has before) — if step 2/3 misbehaves, reseat before debugging software.

- [ ] **Step 3: Commit anything the bring-up shook out** (only if fixes were needed, each with its own test where logic changed)

---

### Task 5: Release 0.5.0

**Files:**
- Modify: `Cargo.toml` (workspace version, line 6)
- Modify: `package.json` (version, line 4)
- Modify: `src-tauri/tauri.conf.json` (version, line 4)
- Modify: `Cargo.lock` (via cargo, not by hand)

Convention (from releases 0.2.0–0.4.0): the release commit changes version metadata only and must build on its own; the tag is GPG-signed and annotated.

- [ ] **Step 1: Bump the three version fields to 0.5.0**

`Cargo.toml` line 6: `version = "0.5.0"` — `package.json` line 4: `"version": "0.5.0"` — `src-tauri/tauri.conf.json` line 4: `"version": "0.5.0"`.

- [ ] **Step 2: Refresh the lockfile**

Run: `cargo update --workspace`
Expected: only `bancada` and `bancada-core` entries change in `Cargo.lock`.

- [ ] **Step 3: Verify the release commit builds on its own**

Run: `npm test && npm run build && cargo check -p bancada`
Expected: green.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock package.json src-tauri/tauri.conf.json
git commit -m "Release 0.5.0

Bumps the workspace, package and Tauri versions to 0.5.0 for USB
auto-connect: a Rust-side watcher notices serial ports appearing or
vanishing and the app rescans by itself — a plugged-in board takes a
free selection slot (never stealing one), serial capture starts, and
known fleet boards are greeted by nickname in the status bar. Rescans
are deferred while a flash is running so re-enumeration is never
mistaken for a detach.

No bundle identifier change, so saved state stays where it is.

Deliberately limited to the version metadata, same as previous
releases: a release commit should be something that builds on its own."
```

- [ ] **Step 5: Signed tag**

```bash
git tag -s v0.5.0 -m "Bancada 0.5.0 — USB auto-connect"
git tag -v v0.5.0
```

Expected: `Good signature` from the user's key. If signing fails (agent not caching the passphrase), STOP and ask the user to run `! git tag -s v0.5.0 -m "Bancada 0.5.0 — USB auto-connect"` themselves.

- [ ] **Step 6: Report** — do NOT push; previous releases were pushed by the user.
