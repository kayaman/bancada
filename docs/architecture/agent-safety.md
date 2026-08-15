# Agent safety model

What is actually enforced when the AI Assistant panel runs a `claude` session,
and — just as important — what is not.

**This page is the single source.** The same model was previously written in
three places with different emphases (the README, the agent-panel spec, and the
`src-tauri/src/lib.rs` rustdoc), which is exactly how a security statement
drifts. Those now point here.

The canonical implementation is `core/src/agent.rs` — deliberately pure
functions, so every rule below is unit-tested without a live CLI.

---

## 1. The threat

The embedded session runs with the **user's own Claude Code configuration
loaded**. The two flags that would suppress it were both probe-verified and
both rejected:

| Flag | Why it was rejected |
|---|---|
| `--bare` | breaks keychain auth |
| `--safe-mode` | disables `--mcp-config`, so the `verify` tool disappears |

So the user's hooks load, and **hooks are shell commands**. Composed with an
unconfined `Write`, that is a path to arbitrary command execution as the user:
the agent writes a `PreToolUse` hook into a settings file, and the CLI runs it.

Closing that requires closing the **write** leg. The hooks leg cannot be closed
without losing either authentication or the compiler.

---

## 2. Four enforcement layers

In order of strength. The ordering is the design — each layer exists because the
one above it cannot express something.

### Layer 1 — `permissions.deny` rules (the anchor)

`core::agent::deny_rules` protects the project's `.claude/**`, `.git/**` and
`.mcp.json`, the session's own 0600 temp files, and the user's `~/.claude/**`.

Rules are built from the **canonical** project directory, so a symlinked project
still matches.

Two properties make this the anchor rather than the hook:

- Deny rules are evaluated **before** hooks.
- They are **unaffected by `disableAllHooks`** — which is precisely why the hook
  below cannot be what protects `.claude/`. A project settings file setting
  `disableAllHooks` stops the hook firing at all (verified live).

> A deny-rule refusal does **not** appear in the CLI's `permission_denials` —
> only hook refusals do. That field is not an audit signal.

### Layer 2 — the `PreToolUse` hook (subtree containment)

A denylist has no "everything except here" form, so containment needs a hook.

Its command is **this very binary**, re-invoked as:

```
bancada --agent-guard <sketch_dir>
```

`run()` handles that argv before Tauri starts and acts as a plain stdin→stdout
JSON filter. It adjudicates every `Write` / `Edit` / `MultiEdit` /
`NotebookEdit` with `core::agent::guard_decision`.

**This is the key structural decision:** the policy is the same unit-tested Rust
function the test suite exercises. There is no generated shell script, no
dependency on `sh` or `python3`, and no second copy that can drift.

`path_is_confined` resolves relative candidates against the sketch dir,
normalises `..` traversal lexically **before** the prefix test, and refuses any
path whose first component below the sketch dir is in
`REFUSED_DIRS = [".claude", ".git"]`.

Probe-verified end to end, including that a permissive hook alongside it does
not override the deny. Its refusals *do* appear in `permission_denials`.

### Layer 3 — the pre-flight refusal

`check_hooks_are_enabled` scans every settings file from the sketch directory up
to the filesystem root, plus the user's own, for `disableAllHooks`. If any sets
it, **the session refuses to start**, naming the offending path — because layer
2 would silently not exist.

`--managed-settings` was probed as an alternative and rejected: it does not
carry hooks.

### Layer 4 — detect-and-stop (the backstop)

The stdout reader independently re-checks:

- every `Edit`/`Write` `tool_use` against `path_is_confined`
- the `system`/`init` `tools` array against `EXPECTED_TOOLS`

Either failing emits `{ type: "security_alarm", kind, detail, pid }` and stops
the session. `kind` is `"path_escape"` or `"unexpected_tools"`.

This layer is **genuinely weaker** than 1–3 and is documented as such: it runs
*after* the model emitted the `tool_use`, so a write it reports may already have
happened.

It exists because layers 1–3 all depend on the CLI's own policy engine behaving
as probed. Without a backstop, a regression there would fail **open, with no
signal at all**.

---

## 3. The tool surface

```rust
BUILTIN_TOOLS  = "Read,Edit,Write,Glob,Grep,WebFetch,WebSearch,Skill"

EXPECTED_TOOLS = BUILTIN_TOOLS + mcp__bancada__{verify, upload, serial_read, serial_send, circuit_status, circuit_sync}
```

**`--tools` is a boundary. `--disallowedTools` is not.** The latter is a
permission-layer nudge — a session with it set still lists 25 built-in tools.
`--tools` genuinely narrows the built-in set while leaving MCP tools intact.

> `BUILTIN_TOOLS`, `EXPECTED_TOOLS` and the `--allowedTools` literal in
> `agent_args` **must change in the same commit**. Drift either alarms on every
> session at init or silently stops asserting anything.

### The web pair is a deliberate egress trade

`WebFetch` and `WebSearch` were added in 0.12.0 knowingly: reads were never
confined, and web access lets what is read leave the machine. That trade is
recorded, not accidental.

### `Skill` closes a gap between the two injection paths

Claude Code injects into an embedded session along two independent paths:
**capability** (`--tools`) and **instruction** (the user's `CLAUDE.md`, plugins
and skills). Only `--bare` and `--safe-mode` cut both, and neither is usable
here — `--bare` restricts auth to `ANTHROPIC_API_KEY`/`apiKeyHelper` (probe:
`authentication_failed` on turn one), `--safe-mode` drops `--mcp-config`
servers and takes `mcp__bancada__verify` with them. Both findings are recorded
in `agent_args`' doc comment in `core/src/agent.rs`. So the instruction path
stays open no matter what this argv does. Gating `Skill` away therefore never removed the
user's skills from the session; it only produced an agent reading
"you MUST invoke the skill" with no tool to invoke it with, which it then said
out loud instead of working.

`Skill` is in `BUILTIN_TOOLS` because it is the one built-in that **grants no
capability** — it loads text into context, it does not act. What a skill then
reaches for meets the unchanged surface: `Bash`, `Task`, `NotebookEdit` are
absent from `--tools`, and an out-of-project `Write` still meets the layer-2
hook. The genuine cost is **prompt-injection surface**: skill text from
`~/.claude` now enters a session that can write inside the sketch dir, at the
agent's own initiative rather than only at startup. That is the same class of
residue as the pre-existing-hostile-hook case in §5, and is accepted on the
same grounds — Bancada stops the agent from *installing* such content, it
cannot stop content that is already on the machine.

### Hardware is scoped structurally, not by policy

- MCP `upload` takes **no port argument**. It flashes the UI-selected port with
  the **session-frozen** profile and FQBN — the agent must flash what its
  `verify` built, not what the user switched to mid-session.
- It is refused unless the panel's **"Allow uploads"** switch is armed.
- `serial_read` / `serial_send` drive the app's own monitor under the same
  single-owner discipline as the UI.
- `circuit_status` is read-only; `circuit_sync` can only regenerate the fixed
  project-local artifact set from `hardware/circuit.yaml`. Verify and Upload
  independently apply the same circuit guard.
- None of them can touch the scope.

---

## 4. Session hardening

- **Bearer token per session**, from `/dev/urandom`, with **no fallback**. A
  degraded nanos-plus-ASLR mix would be searchable while the caller believed the
  listener was protected — so failure to read real entropy is an error, not a
  downgrade.
- **The token rides a 0600 `O_EXCL` temp file**, not argv, because argv is
  readable via `/proc/<pid>/cmdline`.
- **The settings file lives outside the project tree**, so the thing it
  constrains cannot edit it — and it is covered by the deny rules anyway.
- **`valid_session_id`** rejects anything flag-shaped before `--resume` sees it.
- **`clamp_facts`** truncates the fallback summary on a char boundary.
- **Pid-stamping.** Every synthetic event and several commands are pid-guarded,
  so a stale session cannot render into, or kill, a newer one.
- Both temp files are removed on every normal exit path, including
  `RunEvent::Exit`.

---

## 5. What is *not* enforced

Stated plainly, because a security model that only lists its strengths is
misleading:

- **Reads are not confined at all.** The agent can read any file the user can.
- **A pre-existing hostile hook in the user's own config still runs.** Bancada
  stops the agent from *installing* one; it cannot stop one already there.
- **The user's own skills and plugins load into the session** — unavoidably, as
  §3 explains — and since `Skill` is in the tool set the agent can also pull
  more of that text in mid-turn. Instruction surface, not capability surface:
  whatever a skill asks for still has to exist in `--tools`.
- **None of this is OS-level.** It is in-process policy inside the process the
  model drives. There is no sandbox, no seccomp, no container.
- **An already-started compile cannot be aborted**, so cancelling a session
  leaves it holding the build gate until it finishes.
- **Layer 4 is after-the-fact** by construction (§2).

### Why `rename_project` refuses while a session is live

The confinement anchor is a **path string, fixed at spawn time**. `agent_start`
bakes it into four places, none of which can be rewritten afterwards: the
child's working directory (a kernel-held inode), the system prompt, the
canonical `permissions.deny` rules in a 0600 temp file, and the
`--agent-guard` hook's argv.

Rename the project underneath a live session and containment still *fails
closed* — `path_is_confined` resolves the old root to a directory that no
longer exists, so every write is denied and the session bricks rather than
escapes. But layer 1 is the layer that must hold even when hooks are switched
off, and it would then be anchoring nothing. The guarantee is voided quietly,
which is worse than the session breaking loudly.

So the rename is refused outright rather than patched up, and the message says
to stop the session first — which is what a project switch already does
(`teardownAgentSession("project switched")`).

---

## 6. Changing any of this

1. Policy goes in `core/src/agent.rs` as a **pure function**, with tests. Never
   inline it into `src-tauri`.
2. If you touch the tool lists, change all three sites in one commit (§3).
3. If you touch containment, add a `path_is_confined` case — traversal,
   symlink, and `REFUSED_DIRS` cases already exist to copy.
4. Re-verify against the live CLI. Every claim here was probe-verified against
   a specific version (CLI 2.1.220 for the flag findings), and the CLI's
   permission engine is not a stable contract.
5. Update this page. It is the single source; the README and the spec point
   here.

---

## See also

- [ipc-contract §5](ipc-contract.md#5-the-loopback-mcp-server) — the MCP tool surface
- [runtime-model](runtime-model.md) — the agent's four threads and cancellation
- [persistence](persistence.md) — the 0600 temp files
- `docs/superpowers/specs/2026-08-01-agent-panel-design.md` — the original design and its risk register
