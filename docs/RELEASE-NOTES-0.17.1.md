# bancada 0.17.1

**0.17.0 could not flash. Upgrade.**

The serial-monitor recovery added in 0.17.0 took the port back off `esptool`
mid-flash. Uploads failed with messages that named the monitor nowhere:

```
A fatal error occurred: Unable to verify flash chip connection
  (No more data to read from the serial port…)
A fatal error occurred: Serial data stream stopped:
  Possible serial noise or corruption.
```

## What went wrong

Two halves, both mine.

**The ladder raced the flash.** The upload path frees the port with
`toggleMonitor()`, but only `stopMonitorIfOn()` cleared the standing capture
request — so the stop read as an *unexpected* close. The monitor is also
stopped before the busy flag is set, so the guard saw a quiet system and
scheduled a retry. A second later that retry reopened the port, with esptool
still on it.

**Then capture never came back.** With the first half fixed, flashing worked
and the monitor stayed dead. The flash clears the capture request to free the
port; the post-flash restart fired once at 1200 ms; and a native-USB board
takes about two seconds to re-enumerate, so the attempt threw into an empty
catch. A *failed start* emits no `serial://closed`, and recovery hung entirely
off that event — so the ladder never engaged, and with the port returning at
the same address nothing else re-fired either.

## The fixes

- An explicit stop clears the capture request wherever it happens, not just
  in one of the two functions that do it.
- Automatic capture is refused while a flash owns the port — checked where the
  port is actually opened, not only where a retry is scheduled. The ladder
  decides a second or more before it acts, and the world changes in between.
  The manual Start button remains the deliberate override.
- Recovery is driven by **both** failures that can lose the port: a monitor
  that closed, and a start that would not open. One scheduler, called from
  both, and it also keeps asking when there is no port yet.
- The post-flash restart re-arms the request the flash cleared.

Five source-level invariants now pin exactly these mistakes, using the
`App.tsx?raw` pattern the conflict guard already established — the busy guard
precedes opening the port, the intent is cleared before stopping, a failed
start schedules a retry, the post-flash path re-arms capture, and upload frees
the port before flashing. None of this is reachable by any other kind of test
in this repository, which is why it shipped broken.

## Also fixed

A flash that resolved no FQBN set no board-offer cooldown, so it could offer
back the project it had just been flashed from.

## Notes

`cargo test --workspace` 650 passed, `vitest` 651 passed, `tsc --noEmit`
clean, and every opt-in suite green against a real `arduino-cli` and an
attached ESP32-S3.

Flashing and the post-flash monitor reconnect were both confirmed on hardware
for this cut — which 0.17.0's were not, and that is the lesson worth keeping.

Still open, and the next thing to do: `serial://closed` carries no session
identity, so a dying reader thread from a previous monitor can report a live
one as closed. The agent's events solved the same problem by stamping a pid;
the serial path has no equivalent yet.
