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
BUILTIN_TOOLS  = "Read,Edit,Write,Glob,Grep,WebFetch,WebSearch,Skill"

expected_tools(false) = BUILTIN_TOOLS + mcp__bancada__{verify, upload, serial_read, serial_send, board_pinout}
expected_tools(true)  = the above + mcp__espressif-docs__search_espressif_sources
```

**`expected_tools` is a function, and that is the point.** The documentation
server below is offered to ESP-IDF sessions only, so a single constant would
have to be a *superset* — and a superset asserts nothing for the Arduino case,
which is most sessions. The argument is `with_docs`, frozen at spawn beside the
backend, so the set A2 checks is exact for the session that is actually
running: an Arduino session reporting the docs tool is still an alarm.

**`--tools` is a boundary. `--disallowedTools` is not.** The latter is a
permission-layer nudge — a session with it set still lists 25 built-in tools.
`--tools` genuinely narrows the built-in set while leaving MCP tools intact.

> `BUILTIN_TOOLS`, `EXPECTED_TOOLS` and the `--allowedTools` literal in
> `agent_args` **must change in the same commit**. Drift either alarms on every
> session at init or silently stops asserting anything.

### `Skill` widens what the session can be *told*, not what it can *do*

Without it the session announces "The Skill tool is disabled here, so I'll
proceed directly" and ignores the user's own workflows — which defeats the
point of loading their Claude Code configuration at all, the same reasoning
that rejected `--bare`.

It grants no capability. Invoking a skill loads instructions into the context,
and anything those instructions then ask for is still gated by this list: a
skill wanting `Bash` or a subagent still cannot have one, since `Task` is
absent from `--tools` *and* named in `--disallowedTools`.

What it does add is an **instruction surface**. Skill bodies come from
`~/.claude/skills`, the user's plugins, and the project's own
`.claude/skills`, and they become instructions the model follows. That is the
same trust level as the user's hooks, which this design already loads and
cannot suppress (§1) — and the agent cannot author one, because `.claude/**`
is covered by the layer-1 deny rules and by the containment hook.

### The one third-party server, and why it is admissible

ESP-IDF sessions are additionally given Espressif's hosted **documentation**
server (`https://mcp.espressif.com/docs`, one tool:
`search_espressif_sources`). This is the only MCP server in the design that
Bancada does not implement itself, so it needs its own justification.

- **It is retrieval only.** Its documentation states it "does not execute code,
  modify files, or perform actions". Every objection that rules out Espressif's
  *Tools* server — `flash_project(port)` taking a raw port, outside the build
  gate, undoing the property stated below — is about capabilities this server
  does not have.
- **It compensates for a limitation this design accepts.** §5 records that an
  ESP-IDF session cannot read ESP-IDF's own headers under `$IDF_PATH`, because
  the containment anchor is the project directory. Without a documentation
  source the model is guessing at APIs it cannot see. The session prompt names
  the tool as the substitute for exactly that.
- **It holds none of our credentials.** The entry in the `--mcp-config` file is
  a bare URL; the CLI owns its OAuth. Bancada could not supply a token if it
  wanted to.
- **It is inert until the user authorises it.** Unauthenticated, the server
  reports `needs-auth` and contributes **no tools**, which is silent here
  because [`unexpected_tools`] reports only *extra* tools, never missing ones
  (probe-verified: a headless session shows
  `mcp_servers: [{"name":"espressif-docs","status":"needs-auth"}]` and no
  `mcp__espressif-docs__*` entry in `tools`).
- **It is an egress**, in the same category as the web pair below: search
  queries leave the machine, tied to an anonymised account id, under a
  published rate limit. Recorded, not accidental.

`--strict-mcp-config` still applies: this server is present because Bancada's
own generated config names it, **not** because the user registered it in their
Claude Code. A server the user adds themselves is still excluded.

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
- `board_pinout` is the one read-only tool, and the only one that takes an
  argument. It touches no hardware, no subprocess and no file — it reads a
  static table — so there is nothing to bind at spawn time and nothing to get
  wrong. The **board is still not a parameter**: it is resolved from the
  session's project exactly as the GUI resolves it, so the assistant cannot
  reason about a board the user is not holding. It answers "no board profile"
  as an explicit sentence rather than an empty success, because an agent that
  reads silence as "no caveats" gives precisely the confidently-wrong pin
  advice the board model exists to prevent.

### ESP-IDF: the same structure, one new refusal

Bancada drives two build backends, and the tool surface does **not** grow to
accommodate the second one. `verify` and `upload` keep their names, their
empty JSON schemas and their guarantees; only the toolchain behind them
changes, chosen from the project directory at spawn and frozen for the session
exactly as the profile and FQBN are. `EXPECTED_TOOLS` is therefore unchanged,
and so is the §2 backstop that asserts it.

Two additions, both refusals:

- **`set-target` is never a tool.** `idf.py set-target` deletes `build/` and
  regenerates `sdkconfig`, discarding everything the user set by hand. It is
  the ESP-IDF analogue of "`upload` takes no port argument": a destructive
  choice that belongs to the person, not the model. It ships as a Tauri
  command behind a confirmation, and the session prompt forbids running it —
  along with `menuconfig` and `fullclean` — directly.
- **Target drift is refused at flash time.** An Arduino session cannot drift:
  its board is on the command line being run. An ESP-IDF target lives in
  `sdkconfig`, so `set-target` in another window would make `upload` flash a
  different chip than `verify` built for. `run_upload` re-reads
  `CONFIG_IDF_TARGET` and refuses on a mismatch, in the same shape as
  `UPLOAD_NOT_ARMED`.

**We deliberately do not use ESP-IDF's own `idf.py mcp-server`.** It exists,
and this machine's install even ships it. Routing the agent at it would hand
the model `flash_project(port)` — a raw port argument, outside the build gate —
dissolving the property the first bullet of this section exists to state. It
would also trip the §2 backstop, since bancada passes `--strict-mcp-config`
and asserts the tool list at init.

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
- **An ESP-IDF session cannot read the headers it is working against.**
  Everything under `$IDF_PATH/components/` is outside the project directory,
  so the containment hook refuses writes there and the agent has no reason to
  expect reads to be useful either. It is told so in its system prompt rather
  than left to discover it by looping. Widening the anchor to a second tree
  would weaken the one thing layer 2 asserts, so the gap is **compensated
  rather than closed** — ESP-IDF sessions get Espressif's documentation server
  (§3) to look up what they cannot read. That is a substitute for the headers,
  not a replacement: it returns prose and examples, not the project's own
  vendored component sources.

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
