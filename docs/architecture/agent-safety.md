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
folds `..` traversal, and refuses any path whose first component below the
sketch dir is in `REFUSED_DIRS = [".claude", ".git"]`.

**The order of those two steps is the whole trick, and it was wrong once.**
The fold must happen *after* the longest existing prefix has been
`canonicalize`d, never before. Folding first pops a symlink as though it were
an ordinary directory, so `<link>/../x` — where `<link>` points out of the
project — collapses to `<sketch>/x` and reports as confined, while the kernel
resolves the link and lands `..` in the *target's* parent. Only components
that do not exist are folded lexically, and something that does not exist
cannot be a symlink. `confined_rejects_dot_dot_traversal_through_a_symlink`
pins it.

Both consumers of a tool's path — this hook and the layer-4 backstop — read it
through `core::agent::guarded_tool_path`, because [`GUARDED_TOOLS`] is not
uniform: `NotebookEdit` names its target `notebook_path`, not `file_path`. A
new spelling belongs in that one function.

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
BUILTIN_TOOLS  = "Read,Edit,Write,Glob,Grep,WebFetch,WebSearch"

EXPECTED_TOOLS = BUILTIN_TOOLS + mcp__bancada__{verify, upload, serial_read, serial_send}
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

### Hardware is scoped structurally, not by policy

- MCP `upload` takes **no port argument**. It flashes the UI-selected port with
  the **session-frozen** profile and FQBN — the agent must flash what its
  `verify` built, not what the user switched to mid-session.
- It is refused unless the panel's **"Allow uploads"** switch is armed.
- `serial_read` / `serial_send` drive the app's own monitor under the same
  single-owner discipline as the UI.
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
