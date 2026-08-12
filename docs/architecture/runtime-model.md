# Runtime model

State, locks, threads and shutdown. This is the page to read before touching
anything in `src-tauri/src/lib.rs` that owns a resource — the invariants here
are load-bearing and several were arrived at by hitting the deadlock first.

---

## 1. `AppState`

Five **mutually independent session slots** plus one cross-cutting gate.

```
AppState
├── cli              ArduinoCli            cloned per command, cheap
├── serial           Mutex<Option<SerialOwner>>   ← single-owner, evicting
├── serial_ring      Mutex<SerialRing>            ← scrollback for the agent
├── selected_target  Mutex<Option<SelectedTarget>>
├── mqtt             Mutex<Option<MqttSession>>   ┐
├── device_browse    Mutex<Option<DeviceBrowse>>  ├ siblings — never coupled
├── agent            Mutex<Option<AgentSession>>  ┘
└── build_gate       Mutex<()>                    ← try-lock only
```

"Sibling" is exact: acquiring the serial port never disturbs MQTT, the device
proxy or the agent, and vice versa. Only `serial` has an eviction protocol,
because only it guards genuinely exclusive hardware.

---

## 2. The serial single-owner rule

```rust
enum SerialOwner {
    Monitor(Child),        // an arduino-cli monitor subprocess
    Scope(ScopeSession),   // a raw serialport handle + reader thread
}
```

> Whoever currently holds the serial port. **Exactly one owner at a time.**

Acquiring the port for either owner **evicts** the other (`evict_owner`), which
kills and reaps a monitor child or politely stops a scope session — writing
`cmd_stop()` to the firmware, flagging the reader thread down, dropping the
writer, and joining.

`identify_board` evicts too, because esptool needs the port to itself.

### The lock contract

`serial` is a **leaf lock**:

- Held only across bounded-short operations — child spawn, port open, pipe
  write, evict. **Never** across a compile, an upload, or a wait loop.
- May be taken by: Tauri commands, `RunEvent::Exit`, and the agent's MCP
  listener thread.
- **Never** taken by the monitor, scope or MQTT reader threads — those are
  killed or joined *under* it, so taking it would deadlock the join.
- Lock order: `build_gate` (try-only) → `serial` → nothing.

`serial_ring` is separate and is **never taken together with `serial`**. Reader
threads may lock the ring, because nothing joins a thread while holding it.

---

## 3. The build gate

Four entry points drive the same arduino-cli build cache for one sketch:

| Entry point | Triggered by |
|---|---|
| `compile_sketch` | the Verify button |
| `upload_sketch` | the Flash button |
| MCP `verify` | the agent |
| MCP `upload` | the agent |

They share `build_gate`, taken with **`try_build_gate` — non-blocking**.
Contention returns `"build already in progress"` rather than queueing, so a
second Verify click, or an agent `verify` during a user build, fails fast
instead of stacking behind a multi-minute platform build.

Before the gate existed, the only mutual exclusion was the frontend's `busy`
flag — which agent-initiated builds bypass entirely.

**The gate is deliberately not every arduino-cli invocation.** `install_core`,
`uninstall_core`, `update_core_index` and `install_library` run outside it: they
touch the platform and library trees, not a sketch's build cache, and
serialising them behind a long compile would make the Boards and Libraries
panels fail for no benefit.

There are **five** `try_build_gate` call sites. Four are builds — user Verify,
user Flash, and the agent's MCP `verify` and `upload`. The fifth is
`rename_project`, which holds the gate for a different reason: it moves the
very tree the other four compile from, so it must not run beside them.

---

## 4. Threads

Tokio is present only as `tauri::async_runtime` (and transitively via rumqttc's
sync client). **Every `async` command has the same shape**: clone what is needed
out of `State`, then `spawn_blocking`. There are no hand-rolled `tokio::spawn`
tasks — everything long-lived is a `std::thread`.

| Thread | Lifetime and stop mechanism |
|---|---|
| **Hotplug watcher** | Never stops. 2 s poll of `available_ports()`, keyed by `port_key`; emits `ports://changed`. The first tick only seeds the previous set. A failed enumeration never emits and never kills the thread. |
| **Monitor stdout reader** | Ends at pipe EOF, emitting `serial://closed`. Killed indirectly by `evict_owner`. Owns only the emitter and ring `Arc`s. |
| **Monitor stderr reader** | Same. **Not optional** — an undrained pipe wedges the child. |
| **Scope reader** | `AtomicBool` stop flag checked each loop; `stop_scope_session` sets it, drops the writer, joins. Also exits early if a channel send fails (the frontend is gone). |
| **MQTT connection** | `AtomicBool` plus `client.disconnect()` — which is what actually unblocks `connection.iter()` — then join. **Never retries:** any error emits `closed` and breaks. |
| **Device-proxy accept loop** | `Server::unblock()`. An `AtomicBool` cannot interrupt a blocking `accept`. |
| **Device-proxy per-request** | One detached thread per request, because device pages fetch assets in parallel and a serial loop would serialise page load. |
| **Agent — MCP listener** | Owns clones taken at spawn. **Never locks `state.agent`.** Stopped by `Server::unblock()`, which is *sticky*: a listener mid-handler finishes it, then its next `recv()` pops the marker. |
| **Agent — stdin writer** | Fed by an `mpsc::Sender<String>`; ends when the sender drops. |
| **Agent — stdout reader** | Line-reads stream-json. At EOF emits `agent://closed` and tears its own session down. Also runs the security backstop. |
| **Agent — stderr drain** | Emits `{type:"stderr"}`. Not optional, same pipe-wedge reason. |
| **Streaming pairs** | `cli::run_streaming`, `git::run_streaming` and the `gh` path each spawn two short-lived readers feeding one `mpsc`, joined after the receiver drains. |

### Two deadlocks the design avoids

**The MCP listener must never lock `agent`.** A `verify` arriving while a
command held that mutex would deadlock the compile against the UI. It may lock
`serial`, because it is never joined under it.

**The agent mutex is never held across a pipe write.** A message larger than the
64 KiB pipe buffer, sent while the child is blocked waiting on an MCP reply,
would deadlock. Hence the `mpsc`-fed writer thread.

---

## 5. Poison recovery

Every `Mutex` in `AppState` is locked with `unwrap_or_else(|e| e.into_inner())`
rather than `.unwrap()`. This is deliberate and the rationale is worth
internalising:

> What the mutex guards has already lost its invariants when the panicking
> command returned. Refusing the lock only wedges the process.

For `build_gate` it is even clearer — it guards `()`, so there is no state that
could have been corrupted, and propagating poison would break every future build
for the process lifetime.

In the exit handler it is a correctness requirement, not a convenience: the
slots are torn down in order, so a panic on an early one would abort shutdown
before the agent teardown ran — orphaning the `claude` child and leaking both
its 0600 temp files.

---

## 6. Shutdown

`RunEvent::Exit` tears down **in this order**, every lock with poison recovery:

1. **`serial`** — `evict_owner`, so the port is immediately free for other tools
2. **`mqtt`** — sends the broker a clean DISCONNECT
3. **`device_browse`** — `unblock()`s the accept loop so its thread exits
4. **`agent`** — kills and reaps the child, removes both temp files

---

## 7. Cancellation

**Agent cancellation is ordered.** `stop_agent_session` sets `verify_cancel`
**first**, because `unblock()` cannot reach a listener that is inside a
multi-minute `run_verify`. `run_verify` checks the flag before taking the gate
and before every emit.

Documented residual: an *already-started* compile runs to completion and holds
the gate until it does. `ArduinoCli::compile` has no abort handle.

**Teardown is pid-scoped.** `should_stop_agent` and `stop_agent_session_by_pid`
prevent session A's late EOF or interrupt from killing session B. The
`agent_interrupt` grace timer (2 s, detached) is pid-scoped for the same reason.

**Nothing auto-restarts.** Not the MQTT thread, not the agent child. The panel
shows "Session ended" and the user decides. Reconnection policy lives in the
frontend where it can be shown.

---

## See also

- [ipc-contract](ipc-contract.md) — the commands that take these locks
- [agent-safety](agent-safety.md) — the agent session's confinement
- [persistence](persistence.md) — the temp files shutdown must remove
