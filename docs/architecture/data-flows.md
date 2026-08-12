# Data flows

Seven actions traced end to end. If you read one page in this set, read this one —
it is where the layers stop being abstract.

---

## 1. Verify (compile)

The simplest complete round trip, and the template for every long-running
engine call.

```
Toolbar "Verify"
  └─▶ App.verify()
        ├─ blockedByConflict()            src/conflicts.ts  — refuse if the agent
        │                                   edited files the user also has dirty
        └─▶ api.compileSketch(…)          src/api.ts
              └─▶ invoke("compile_sketch", { sketchDir, profile, fqbn })
                    │
                    ▼  src-tauri
              async fn compile_sketch
                └─ spawn_blocking
                     ├─ try_build_gate(&gate)?      ← non-blocking; else BUILD_BUSY
                     └─ cli.compile(…, |line| app.emit("build://line", &line))
                          │
                          ▼  core/src/cli.rs
                     run_streaming
                       ├─ spawn arduino-cli, piped stdio
                       ├─ thread(stdout) ─┐
                       ├─ thread(stderr) ─┼─▶ mpsc ─▶ OutputLine { stream, line }
                       └─ join both       ─┘
                                              │
                    ┌─────────────────────────┘  event: build://line
                    ▼
              App's subscription effect ─▶ setBuildLines ─▶ <Console/>
```

**Why the two reader threads:** stdout and stderr must interleave in real order.
A single-pipe read would reorder compiler warnings against progress lines.

**Why the gate is non-blocking:** a second Verify click during a multi-minute
platform build fails fast with `"build already in progress"` rather than
queueing invisibly. See [runtime-model §3](runtime-model.md#3-the-build-gate).

`upload_sketch` is identical, plus a `port` argument.

---

## 2. Starting the serial monitor

```
port selected ─▶ api.setSelectedTarget({ port, baud })   ← mirrored into Rust so
                                                            the agent's MCP tools
                                                            need no port argument
"Start" ─▶ api.startMonitor({ port, baud })
             │
             ▼  src-tauri: start_monitor
        lock serial                        ← the leaf lock
          ├─ evict_owner(&mut slot)        ← kills a monitor OR stops a scope session
          ├─ cli.monitor(port, baud)       ← Child with piped stdio
          ├─ spawn thread(stdout) ─▶ emit "serial://line"  + serial_ring.push()
          ├─ spawn thread(stderr) ─▶ emit "serial://line"
          └─ slot = Some(SerialOwner::Monitor(child))
        unlock
                    │
                    ▼
         App subscription ─▶ setSerialLines ─▶ <Console mode="serial"/>
                          └▶ ScopeView also subscribes (plotter source)
```

At EOF the stdout thread emits `serial://closed`.

**The two locks never meet.** Reader threads push into `serial_ring` but must
never take `serial` — they are killed or joined *under* it, so taking it would
deadlock the join.

**The ring outlives the monitor.** Its sequence numbers survive restarts, which
is what lets the agent's `serial_read` resume from a cursor rather than replay.

---

## 3. A scope ADC sample becomes a pixel

The highest-rate path in the app — up to tens of thousands of samples per
second — and the reason several unusual choices exist.

```
ESP32 firmware
  └─ binary frame: sync A5 5A · type · flags · seq · len · first_idx · payload · CRC16
       │ USB serial
       ▼  src-tauri: scope reader thread
  FrameScanner::push()                      core/src/scope.rs — sync, CRC, seq
    ├─ bad CRC  ─▶ counted, emitted as {"ev":"crc"} every 50
    ├─ dropped  ─▶ {"ev":"drop","frames":N}
    └─ good     ─▶ envelope_samples(flags, first_sample_index, payload)
                     │  Channel<InvokeResponseBody>::Raw   ← binary, not an event
                     ▼  src/scope/binary.ts
                decodeEnvelope()  kind 0x01 samples / 0x02 JSON
                     │
                     ▼  src/scope/engine.ts
                ScopeEngine.feedBinary()
                  ├─ CalTable: raw counts ─▶ volts (piecewise linear, from META)
                  ├─ RingBuffer.push()      absolute sample index, no allocation
                  └─ TriggerEngine.scan()   incremental from triggerScannedTo
                     │
                     ▼  WaveformCanvas rAF loop (60 fps)
                engine.renderFrame(columns)
                  ├─ minMaxDecimate ─▶ reused scratch arrays
                  └─ returns the SAME RenderFrame object every call
                     │
                     ▼
                canvas 2D draw
```

**Why a `Channel` and not an event.** Tauri events are broadcast and JSON. This
stream is binary and belongs to one panel's session.

**Why `renderFrame` allocates nothing.** At 60 fps, per-frame allocation means
GC pauses that show up as visible jitter. The engine decimates into reused
scratch arrays and returns a single reused `RenderFrame`. `engineRender.test.ts`
pins this — if your change makes it allocate, that test is right.

**Why the trigger scan is incremental.** Rescanning a 2²⁰-sample ring every
frame would be O(n) per frame; `triggerScannedTo` makes it O(new samples).

FFT is computed off-thread in a module worker, with a synchronous fallback for
node and for worker failure.

Full byte-level contract: [`docs/scope-architecture.md`](../scope-architecture.md).

---

## 4. An agent turn (including an MCP `verify`)

The most involved flow in the app. Four threads and a loopback HTTP server.

```
user types ─▶ api.agentSend(text)
                └─▶ mpsc ─▶ [stdin writer thread] ─▶ claude child stdin
                     ▲
                     └── the agent mutex is NEVER held across this pipe write

claude child stdout ─▶ [stdout reader thread]
   ├─ agent::parse_event(line)          validate only
   ├─ SECURITY BACKSTOP                 path_is_confined · EXPECTED_TOOLS
   │    └─ on failure ─▶ {type:"security_alarm"} + stop session
   └─ emit "agent://event"  ── the CLI's object, VERBATIM
        │
        ▼  src/api.ts onAgentEvent ─▶ App
   resumeWatch.offerEvent()   gate for --resume confirm/fail
        ├─ agentStore.push(ev)          ─▶ AgentPanel polls store.version (100 ms)
        └─ chatRecorder.record(push)    ─▶ NDJSON line, fire-and-forget

── meanwhile, if the model calls mcp__bancada__verify ──

claude ─HTTP POST /mcp─▶ [MCP listener thread]   127.0.0.1:<ephemeral>
   ├─ bearer check ─▶ 401 on failure
   ├─ run_verify:
   │    ├─ check verify_cancel BEFORE taking the gate
   │    ├─ try_build_gate  ← the SAME gate as the Verify button
   │    ├─ emit "agent://event" {type:"verify_started"}
   │    ├─ cli.compile(…) ─▶ emit "build://line"   ← shares the build console
   │    └─ emit "agent://event" {type:"verify_done", success}
   └─ reply: "success: …\nexit_code: …\n\n<summary>"
             │
             ▼  src/agent/verifyResult.ts parses it ─▶ the Verify tool card
```

**The listener never locks `state.agent`.** A `verify` arriving while a command
held that mutex would deadlock the compile against the UI.

**The frontend's contract is the wire shape.** Events are forwarded verbatim, so
`src/agent/types.ts` mirrors the CLI's objects and must not throw on an unknown
`type`.

**Persistence is an operation log.** Each mutating store call is one NDJSON
line, so replaying reproduces the rendering exactly — no second schema. See
[persistence §1](persistence.md#chatssketch_keyndjson).

Confinement: [agent-safety](agent-safety.md).

---

## 5. A git commit from the pill

```
GitPill opens ─▶ api.gitState({ sketchDir })
                   └─▶ git_state ─▶ core/src/git.rs
                         ├─ repo_root, is_under_git
                         ├─ parse_status_v2()      git status --porcelain=v2
                         ├─ tracked_secrets()      warn before committing keys
                         └─ suggested_message()
                              │  RepoState (serde-tagged union)
                              ▼
                   src/gitStatus.ts ─▶ pillLabel · popoverMode · syncDisabledReason
                              │           (the pill's ENTIRE vocabulary)
                              ▼
                        the pill and its popover

"Commit" ─▶ api.gitCommit ─▶ git_commit ─▶ CommitOutcome
"Sync"   ─▶ api.gitSync   ─▶ git_sync   ─▶ streams "build://line"
```

**All display vocabulary lives in `src/gitStatus.ts`**, not in the component —
so it is testable in the node environment. The component renders strings it is
handed.

**`git_sync` streams to `build://line`** rather than inventing an event: it is
another long-running engine, and the build console is where engine output
already goes.

---

## 6. A device-browser request

An iframe cannot load a bench device's page under the app's origin, so Bancada
runs a loopback reverse proxy.

```
DeviceBrowserPanel ─▶ api.deviceBrowseStart(target, onEvent)
                        └─▶ device_browse_start
                              ├─ core::devproxy::parse_target()  ← REFUSES https
                              ├─ tiny_http bind 127.0.0.1:0      ← ephemeral port
                              ├─ spawn accept loop thread
                              └─ returns the port
                                   │  Channel: {"type":"stage","stage":"listening",port}
                                   ▼
                        <iframe src="http://127.0.0.1:<port>/">

iframe request ─▶ [accept loop] ─▶ spawn ONE THREAD PER REQUEST
                                     ├─ strip hop-by-hop headers  core::devproxy
                                     ├─ ureq ─▶ the device (plain HTTP)
                                     ├─ body_preview()
                                     └─ Channel: {"type":"exchange", method, path,
                                                  status, duration_ms, …}
                                          │
                                          ▼
                                    ObsStore.push() ─▶ <ObsLog/> (polled ~4 Hz)
```

**One thread per request, not a loop.** Device pages fetch assets in parallel; a
serial loop would serialise page load and make the device look broken.

**`unblock()`, not an `AtomicBool`.** No atomic flag can interrupt a blocking
`accept()`.

**`https` is refused up front.** Bench devices do not serve TLS, and pretending
to support it would be worse than refusing.

---

## 7. A flash becomes a tag

```
Toolbar "Flash"                          agent's MCP `upload` tool
      │                                            │
      └─▶ upload_sketch ────┐          ┌──── run_upload
                            ▼          ▼
                     try_build_gate()  (non-blocking; BUILD_BUSY on contention)
                            │
                     ArduinoCli::upload()     arduino-cli compile -u -p <port>
                            │  every OutputLine ─▶ "build://line"
                            ▼
                     result.success? ── no ─▶ done (a failed flash is data,
                            │                        not an error)
                           yes
                            ▼
                     tag_flash(sketch_dir, profile, fqbn, port, board)
                            │
                            ├─ repo_state()   NoGit / Nested ─▶ note, stop
                            │                 (the scope's /tmp firmware dir
                            │                  lands here — no special case)
                            ├─ commit()       "<suggested>\n\nCheckpointed
                            │                  automatically before flashing…"
                            │      NothingToCommit + HEAD already flash-tagged
                            │              └─▶ note, stop (same code, same tag)
                            ├─ flash_tag_name(now_secs())   flash/2026-08-12T1430
                            ├─ tag_annotated()   -c tag.gpgSign=false
                            │                    body: port · profile · fqbn ·
                            │                          board · bancada version
                            └─ push_tag()        git push origin <tag>
                                                 skipped when there is no remote
                            │
                            ▼  every step reports to "build://line"
                     the gate is released
```

**Nothing in `tag_flash` can fail the flash.** Every step that goes wrong
writes one line and returns — the precedent is `note_board_fqbn`, whose doc
says bookkeeping must not turn a good flash into an error. The push is both the
likeliest step to fail (a bench is often offline) and the least important.

**The tag is written under the build gate**, still held from the upload, so
nothing can compile against the tree between the flash and the record of it.

**One tag per distinct code state, not per flash.** A clean tree whose HEAD
already carries a `flash/*` tag is a re-flash of something already recorded.
A clean tree *without* one — code committed by hand, then flashed — still gets
a tag.

`core` cannot do any of this itself: it has no clock and no app paths. So
`flash_tag_name` takes `u64` unix seconds from the caller, exactly as `fleet`
takes `now` — the layering rule, applied to time.

---

## Patterns to reuse

Every flow above is an instance of one of four shapes. New work should pick one
rather than invent a fifth:

| Shape | Use when | Example |
|---|---|---|
| `invoke` → `spawn_blocking` → result | transactional, fast | `git_state` |
| `invoke` → engine → `build://line` | long-running subprocess with output | compile, upload, sync |
| `invoke` → `Channel` → panel store | per-session or high-rate stream | scope, MQTT, device browse |
| broadcast event → App → props | app-wide state change | `ports://changed` |

---

## See also

- [ipc-contract](ipc-contract.md) — every command, event and channel
- [runtime-model](runtime-model.md) — the locks and threads these flows cross
- [frontend](frontend.md) — where each flow lands in the UI
