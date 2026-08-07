# Assistant Debug Detail + Turn-Aware Footer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Assistant footer stops saying `thinking…` between turns (shows `Ready`), and the panel gains three debug affordances: a raw event log view, expandable generic tool cards, and a footer activity strip with full paths + elapsed time.

**Architecture:** All state lives in the plain-JS `AgentStore` (`src/agent/agentStore.ts`) which the panel polls via `version`; new state follows the same discipline. `activityLabel` (`src/agent/activity.ts`) stays a pure tested function; JSX/CSS changes in `AgentPanel.tsx`/`styles.css` are verified visually per repo convention (no component tests exist).

**Tech Stack:** TypeScript, React 18, vitest (pure modules only), Tauri 2 shell (untouched).

**Spec:** `docs/superpowers/specs/2026-08-07-assistant-debug-detail-design.md`

## Global Constraints

- `push()` must never throw on an unrecognised event shape (task-brief invariant, `agentStore.ts` module doc).
- Unknown event types still produce **no transcript message** — but now DO bump `version` and land in the raw log (spec: "Raw event log"). Update the module doc comment accordingly.
- Raw log ring buffer cap: **500** entries; tool result text in cards capped at **4000** chars.
- Between-turns footer copy is exactly `Ready`.
- Tests: `npm test` (vitest run). Type gate: `npm run build` (tsc --noEmit + vite).

---

### Task 1: Store — turn-in-flight flag + activity timestamps

**Files:**
- Modify: `src/agent/agentStore.ts`
- Test: `src/agent/__tests__/agentStore.test.ts` (append a new describe block)

**Interfaces:**
- Produces: `snapshot()` gains `turnActive: boolean` and `turnStartedAt?: number`; the `tool` variant of `AgentMessage` gains `startedAt?: number`. Task 3's `activityLabel` and Task 4's panel consume all three.

- [ ] **Step 1: Write the failing tests**

Append to `src/agent/__tests__/agentStore.test.ts`:

```ts
describe("turn-in-flight tracking", () => {
  it("is inactive until the user sends, active until result", () => {
    const s = new AgentStore();
    expect(s.snapshot().turnActive).toBe(false);
    s.userSent("hi");
    expect(s.snapshot().turnActive).toBe(true);
    s.push({ type: "result", result: "done" });
    expect(s.snapshot().turnActive).toBe(false);
  });

  it("clears on closed(), security_alarm and clear()", () => {
    const a = new AgentStore();
    a.userSent("hi");
    a.closed("child exited");
    expect(a.snapshot().turnActive).toBe(false);

    const b = new AgentStore();
    b.userSent("hi");
    b.push({ type: "security_alarm", kind: "path_escape", detail: "x" });
    expect(b.snapshot().turnActive).toBe(false);

    const c = new AgentStore();
    c.userSent("hi");
    c.clear();
    expect(c.snapshot().turnActive).toBe(false);
  });

  it("stamps turnStartedAt on userSent and startedAt on tool messages", () => {
    const now = vi.spyOn(Date, "now").mockReturnValue(1000);
    const s = new AgentStore();
    s.userSent("hi");
    expect(s.snapshot().turnStartedAt).toBe(1000);
    now.mockReturnValue(2500);
    s.push({
      type: "assistant",
      message: {
        content: [{ type: "tool_use", id: "t1", name: "Read", input: {} }],
      },
    });
    const tool = s.snapshot().messages.find((m) => m.kind === "tool");
    expect(tool && tool.kind === "tool" && tool.startedAt).toBe(2500);
    now.mockRestore();
  });
});
```

(If the file doesn't already import `vi`, extend the vitest import: `import { describe, expect, it, vi } from "vitest";`)

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run src/agent/__tests__/agentStore.test.ts`
Expected: FAIL — `turnActive` is `undefined`, not `false`/`true`.

- [ ] **Step 3: Implement**

In `src/agent/agentStore.ts`:

1. In `AgentMessage`, extend the tool variant:

```ts
  | {
      kind: "tool";
      id: string;
      name: string;
      input: unknown;
      status: "running" | "ok" | "error";
      result?: string;
      /** Date.now() when the tool_use block arrived — for the footer's elapsed counter. */
      startedAt?: number;
    }
```

2. New private fields next to `streamingFlag`:

```ts
  /** True between userSent() and the turn's result/close/alarm — the
   *  footer's "is the agent actually working" bit, distinct from
   *  statusFlag ("is the session alive"). */
  private turnActiveFlag = false;
  private turnStartedAtVal?: number;
```

3. `userSent()` — after `this.streamingFlag = false;` add:

```ts
    this.turnActiveFlag = true;
    this.turnStartedAtVal = Date.now();
```

4. `handleResult()` — next to `this.streamingFlag = false;` add `this.turnActiveFlag = false;`
5. `closed()` — same addition next to its `this.streamingFlag = false;`
6. `handleAlarm()` — add `this.turnActiveFlag = false;` next to `this.verifyRunningFlag = false;`
7. `clear()` — add `this.turnActiveFlag = false; this.turnStartedAtVal = undefined;`
8. In `handleAssistant()`, stamp the tool message: add `startedAt: Date.now(),` to the `this.msgs.push({ kind: "tool", ... })` object.
9. `snapshot()` — add `turnActive: boolean; turnStartedAt?: number;` to the return type and `turnActive: this.turnActiveFlag, turnStartedAt: this.turnStartedAtVal,` to the returned object.

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run src/agent/__tests__/agentStore.test.ts`
Expected: PASS (all pre-existing tests too — none read `turnActive`).

- [ ] **Step 5: Commit**

```bash
git add src/agent/agentStore.ts src/agent/__tests__/agentStore.test.ts
git commit -m "feat: agent store tracks turn-in-flight and activity timestamps"
```

---

### Task 2: Store — raw event ring-buffer log

**Files:**
- Modify: `src/agent/agentStore.ts`
- Test: `src/agent/__tests__/agentStore.test.ts` (append)

**Interfaces:**
- Produces: `export interface RawLogEntry { ts: number; type: string; subtype?: string; count: number; json: string }`; `snapshot()` gains `rawLog: RawLogEntry[]`. Task 4's `DebugLogView` consumes both.

- [ ] **Step 1: Write the failing tests**

Append to `src/agent/__tests__/agentStore.test.ts`:

```ts
describe("raw event log", () => {
  it("records every event — including types push ignores — and bumps version", () => {
    const s = new AgentStore();
    const v0 = s.version;
    s.push({ type: "rate_limit_event" } as never);
    expect(s.version).toBeGreaterThan(v0);
    const log = s.snapshot().rawLog;
    expect(log).toHaveLength(1);
    expect(log[0].type).toBe("rate_limit_event");
    expect(log[0].count).toBe(1);
    // still no transcript message
    expect(s.snapshot().messages).toHaveLength(0);
  });

  it("keeps system subtype and pretty JSON", () => {
    const s = new AgentStore();
    s.push({ type: "system", subtype: "status" });
    const e = s.snapshot().rawLog[0];
    expect(e.subtype).toBe("status");
    expect(JSON.parse(e.json)).toEqual({ type: "system", subtype: "status" });
  });

  it("coalesces consecutive stream_event entries, keeping the newest json", () => {
    const s = new AgentStore();
    const delta = (t: string) => ({
      type: "stream_event" as const,
      event: {
        type: "content_block_delta",
        delta: { type: "text_delta", text: t },
      },
    });
    s.push(delta("a"));
    s.push(delta("b"));
    s.push({ type: "system", subtype: "status" });
    s.push(delta("c"));
    const log = s.snapshot().rawLog;
    expect(log.map((e) => [e.type, e.count])).toEqual([
      ["stream_event", 2],
      ["system", 1],
      ["stream_event", 1],
    ]);
    expect(log[0].json).toContain('"b"');
  });

  it("caps the buffer at 500, dropping the oldest", () => {
    const s = new AgentStore();
    for (let i = 0; i < 510; i++) {
      s.push({ type: "system", subtype: `s${i}` });
    }
    const log = s.snapshot().rawLog;
    expect(log).toHaveLength(500);
    expect(log[0].subtype).toBe("s10");
  });

  it("closed() appends a synthetic entry; clear() resets the log", () => {
    const s = new AgentStore();
    s.push({ type: "system", subtype: "init", session_id: "x" });
    s.closed("child exited", undefined);
    let log = s.snapshot().rawLog;
    expect(log[log.length - 1].type).toBe("closed");
    s.clear();
    expect(s.snapshot().rawLog).toHaveLength(0);
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run src/agent/__tests__/agentStore.test.ts`
Expected: FAIL — `rawLog` is `undefined`.

- [ ] **Step 3: Implement**

In `src/agent/agentStore.ts`:

1. Export the entry type and cap near the top (after `AgentAlarm`):

```ts
/** One row of the 🐛 Debug view: a raw `agent://event` payload as received.
 *  Consecutive `stream_event`s coalesce into one row (count > 1, newest
 *  json wins) — text deltas arrive dozens per second. */
export interface RawLogEntry {
  ts: number;
  type: string;
  subtype?: string;
  count: number;
  json: string;
}

const RAW_LOG_CAP = 500;
```

2. New private field: `private rawLog: RawLogEntry[] = [];`
3. New private method:

```ts
  private recordRaw(payload: unknown): void {
    const p = (payload ?? {}) as Record<string, unknown>;
    const type = typeof p.type === "string" ? p.type : "unknown";
    let json: string;
    try {
      json = JSON.stringify(payload, null, 2);
    } catch {
      json = String(payload);
    }
    const last = this.rawLog[this.rawLog.length - 1];
    if (last && type === "stream_event" && last.type === "stream_event") {
      last.count++;
      last.json = json;
      last.ts = Date.now();
      return;
    }
    this.rawLog.push({
      ts: Date.now(),
      type,
      subtype: typeof p.subtype === "string" ? p.subtype : undefined,
      count: 1,
      json,
    });
    if (this.rawLog.length > RAW_LOG_CAP) this.rawLog.shift();
  }
```

4. In `push()`, right after the pid-guard early return (so events from superseded sessions are NOT logged), add:

```ts
    this.recordRaw(ev);
    // Every surviving event repaints: an open 🐛 Debug view must show it
    // even when no handler below changes transcript state.
    this.ver++;
```

   and change the `default:` branch's comment — the event was already recorded and bumped above, it just produces no transcript message. Update the module doc's "no version bump" sentence to match the spec ("unknown types land in the raw log and repaint; they still paint nothing into the transcript").

5. In `closed()`, after the `isOurs` guard: `this.recordRaw({ type: "closed", reason, pid });`
6. In `clear()`: `this.rawLog = [];`
7. In `snapshot()`: add `rawLog: RawLogEntry[];` to the type, `rawLog: this.rawLog.slice(),` to the object.

- [ ] **Step 4: Run the full store suite**

Run: `npx vitest run src/agent/__tests__/agentStore.test.ts src/agent/__tests__/chatLog.test.ts`
Expected: PASS. (chatLog's `replayChat` pushes events through the same store — replays now populate `rawLog`, which nothing asserts against and ReplayView never shows. If a chatLog test asserts exact `version` values, adjust it to compare relative bumps.)

- [ ] **Step 5: Commit**

```bash
git add src/agent/agentStore.ts src/agent/__tests__/agentStore.test.ts
git commit -m "feat: agent store records a capped raw event log"
```

---

### Task 3: activity.ts — turnActive gate, full paths, elapsed time

**Files:**
- Modify: `src/agent/activity.ts`
- Test: `src/agent/__tests__/activity.test.ts`

**Interfaces:**
- Consumes: `turnActive`, `turnStartedAt`, tool `startedAt` from Task 1.
- Produces: `ActivityInput` gains `turnActive: boolean; turnStartedAt?: number; now?: number`. Labels: `⚙ Edit /home/x/soil/soil.ino… 12s`, `✍ writing… 3s`, `thinking… 2s`; `null` when `!turnActive` (verify still outranks). Task 4 passes `{ ...snap, now: Date.now() }`.

- [ ] **Step 1: Update/extend the tests**

In `src/agent/__tests__/activity.test.ts`, add `turnActive: true` to `base`, change the basename expectation to the full path, and add gate/elapsed cases:

```ts
const base = {
  status: "running" as const,
  verifyRunning: false,
  streaming: false,
  turnActive: true,
  messages: [] as AgentMessage[],
};
```

Existing test updates:
- `"names the newest running tool with the file basename"` → rename to `"names the newest running tool with the full file path"`, expect `"⚙ Edit /home/x/soil/soil.ino…"`.

New tests:

```ts
  it("is null between turns even while the session runs", () => {
    expect(activityLabel({ ...base, turnActive: false })).toBeNull();
    expect(
      activityLabel({ ...base, turnActive: false, streaming: true }),
    ).toBeNull();
  });

  it("verify outranks the turnActive gate", () => {
    const a = activityLabel({ ...base, turnActive: false, verifyRunning: true });
    expect(a).toBe("🔨 verify (compiling)…");
  });

  it("appends elapsed seconds when timestamps are known", () => {
    const msgs = [
      { ...tool("Edit", "running", { file_path: "/x/soil.ino" }), startedAt: 1000 },
    ];
    expect(activityLabel({ ...base, messages: msgs, now: 13400 })).toBe(
      "⚙ Edit /x/soil.ino… 12s",
    );
    expect(
      activityLabel({ ...base, streaming: true, turnStartedAt: 1000, now: 4000 }),
    ).toBe("✍ writing… 3s");
    expect(activityLabel({ ...base, turnStartedAt: 1000, now: 3100 })).toBe(
      "thinking… 2s",
    );
  });

  it("omits elapsed under one second or without timestamps", () => {
    expect(activityLabel({ ...base, turnStartedAt: 1000, now: 1500 })).toBe(
      "thinking…",
    );
    expect(activityLabel(base)).toBe("thinking…");
  });
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run src/agent/__tests__/activity.test.ts`
Expected: FAIL — full-path and elapsed/gate expectations.

- [ ] **Step 3: Implement**

Rewrite `src/agent/activity.ts`'s exported pieces:

```ts
export interface ActivityInput {
  status: AgentStatus;
  verifyRunning: boolean;
  streaming: boolean;
  /** A turn is in flight (userSent → result/close/alarm). Off between
   *  turns, so the footer can say "Ready" instead of a phantom
   *  "thinking…" while the CLI merely sits alive awaiting input. */
  turnActive: boolean;
  messages: AgentMessage[];
  turnStartedAt?: number;
  /** Injected clock so this stays a pure, testable function. */
  now?: number;
}

/** " 12s" once an activity is at least a second old; "" otherwise. */
function elapsed(since: number | undefined, now: number | undefined): string {
  if (since === undefined || now === undefined) return "";
  const s = Math.floor((now - since) / 1000);
  return s >= 1 ? ` ${s}s` : "";
}

export function activityLabel(a: ActivityInput): string | null {
  if (a.status !== "running" && a.status !== "starting") return null;
  if (a.verifyRunning) return "🔨 verify (compiling)…";
  if (!a.turnActive) return null;
  for (let i = a.messages.length - 1; i >= 0; i--) {
    const m = a.messages[i];
    if (m.kind === "tool" && m.status === "running") {
      const hint = toolHint(m.name, m.input);
      const base = hint ? `⚙ ${m.name} ${hint}…` : `⚙ ${m.name}…`;
      return base + elapsed(m.startedAt, a.now);
    }
  }
  if (a.streaming) return `✍ writing…${elapsed(a.turnStartedAt, a.now)}`;
  return `thinking…${elapsed(a.turnStartedAt, a.now)}`;
}
```

`toolHint`: change the Edit/Write/Read branch from `i.file_path.split("/").pop() ?? ""` to `i.file_path` (full path; CSS ellipsis in Task 4 handles overflow). Update the function's doc comment.

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run src/agent/__tests__/activity.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/agent/activity.ts src/agent/__tests__/activity.test.ts
git commit -m "feat: activity label gates on turn-in-flight, shows full paths and elapsed time"
```

---

### Task 4: Panel — Ready label, ticking repaint, debug view, expandable tool cards, CSS

**Files:**
- Modify: `src/components/AgentPanel.tsx`
- Modify: `src/styles.css` (agent section, after `.agent-stderr-body` and near `.agent-footer`)

**Interfaces:**
- Consumes: `snapshot().turnActive/turnStartedAt/rawLog`, `RawLogEntry`, tool `startedAt`, `activityLabel({ ...snap, now })`.
- Produces: user-visible behavior only. No unit tests — panel JSX/CSS is verified visually per repo convention.

- [ ] **Step 1: statusLabel + activity call + ticking repaint**

In `AgentPanel.tsx`:

1. `statusLabel`: change `case "running": return "Running";` to `case "running": return "Ready";` (with a comment: only reachable between turns — during a turn `activityLabel` always wins).
2. Footer status: `{activityLabel({ ...snap, now: Date.now() }) ?? statusLabel(snap.status, snap.verifyRunning)}`
3. Poll effect body becomes:

```ts
    const iv = window.setInterval(() => {
      const changed = store.version !== lastVersionRef.current;
      if (changed) lastVersionRef.current = store.version;
      // Repaint on every poll while a turn is live — the footer's elapsed
      // counter ticks with wall time, not with store versions.
      if (changed || store.snapshot().turnActive) setTick((t) => t + 1);
    }, POLL_MS);
```

- [ ] **Step 2: Expandable generic tool card**

Replace the final `return` of `ToolCard` with:

```tsx
  return (
    <details className="agent-tool agent-tool-generic">
      <summary>
        <span
          className={
            msg.status === "error" ? "agent-tool-icon fail" : "agent-tool-icon"
          }
        >
          {msg.status === "running" ? "⟳" : msg.status === "error" ? "✗" : "✓"}
        </span>{" "}
        🔍 {msg.name}({argsSummary(msg.input)})
      </summary>
      <pre className="agent-tool-detail">{prettyJson(msg.input)}</pre>
      {msg.result && (
        <pre className="agent-tool-detail">{truncate(msg.result, 4000)}</pre>
      )}
    </details>
  );
```

Add next to `truncate`:

```ts
/** Pretty JSON for the expanded debug/tool views; never throws on cycles. */
function prettyJson(v: unknown): string {
  try {
    return JSON.stringify(v, null, 2);
  } catch {
    return String(v);
  }
}
```

- [ ] **Step 3: Debug view + footer toggle**

1. Import the type: add `RawLogEntry` to the `import type { ... } from "../agent/agentStore";` list.
2. State next to `viewTurn`: `const [debugOpen, setDebugOpen] = useState(false);`
3. Scroll-area branch order becomes: `viewTurn ? <TurnSummaryView…> : debugOpen ? <DebugLogView entries={snap.rawLog} /> : histStore ? …` (rest unchanged).
4. Footer, before the History button:

```tsx
        <button
          className={debugOpen ? "btn small primary" : "btn small"}
          onClick={() => setDebugOpen((o) => !o)}
          title="Raw agent event log (all stream-json events, including unmodelled ones)"
        >
          🐛
        </button>
```

5. New component after `HistListView`:

```tsx
// ---------- raw event debug log ----------

/** Every agent://event as received, one collapsible row each — including
 *  shapes the store ignores (system:status, rate_limit_event, hooks…).
 *  Coalesced stream_event rows show ×N. */
function DebugLogView({ entries }: { entries: RawLogEntry[] }) {
  if (entries.length === 0) {
    return <div className="agent-empty">No events yet this session.</div>;
  }
  return (
    <div className="agent-debug-log">
      {entries.map((e, i) => (
        <details key={`${e.ts}-${i}`} className="agent-debug-entry">
          <summary>
            <span className="agent-debug-time">
              {new Date(e.ts).toLocaleTimeString()}
            </span>{" "}
            {e.type}
            {e.subtype ? `/${e.subtype}` : ""}
            {e.count > 1 ? ` ×${e.count}` : ""}
          </summary>
          <pre className="agent-debug-json">{e.json}</pre>
        </details>
      ))}
    </div>
  );
}
```

Notes: the autoscroll effect's early-return list (`viewTurn || histList || histStore`) does NOT get `debugOpen` — the debug log should follow the tail like a console. History/turn views take precedence over debug only via the branch order above; opening History with debug on shows History (debug reappears when History closes) — acceptable, no extra state juggling.

- [ ] **Step 4: CSS**

In `src/styles.css`, after `.agent-stderr-body`:

```css
.agent-tool-detail {
  margin: 2px 0 0;
  padding: 4px 6px;
  background: var(--bg);
  border-radius: var(--radius);
  font-family: var(--font-mono);
  font-size: 11px;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.agent-debug-entry {
  margin: 2px 0;
  color: var(--text-dim);
  font-family: var(--font-mono);
  font-size: 11px;
}
.agent-debug-entry summary {
  cursor: pointer;
}
.agent-debug-time {
  opacity: 0.7;
}
.agent-debug-json {
  margin: 2px 0 6px;
  padding: 4px 6px;
  background: var(--bg);
  border-radius: var(--radius);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
```

And next to `.agent-footer`, a full-path-safe status:

```css
.agent-status {
  min-width: 0;
  flex: 0 1 auto;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
```

(Check first whether `.agent-status` already has a rule elsewhere in the file — if so, merge these properties into it instead of adding a duplicate selector.)

- [ ] **Step 5: Full verification**

Run: `npm test`
Expected: all suites PASS.

Run: `npm run build`
Expected: tsc + vite complete with no type errors.

Visual check (if a dev shell is available): `npm run tauri dev` — send a message, watch the footer show `thinking…`/tool + elapsed, then flip to `Ready` when the reply lands; toggle 🐛 and expand a row; expand a generic tool card.

- [ ] **Step 6: Commit**

```bash
git add src/components/AgentPanel.tsx src/styles.css
git commit -m "feat: assistant panel debug view, expandable tool cards, Ready footer"
```
