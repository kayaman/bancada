# bancada 0.23.0

**The serial monitor is reliable again, and the assistant's flash-then-read
loop is visible end to end.**

## Serial monitor

- The monitor no longer starts and immediately closes in a loop when a port is
  selected. Two React effects both fired `startMonitor` on the same render;
  the second call evicted the first session (a deliberate stop, so no error
  message appeared), and the resulting close triggered a recapture, repeating
  indefinitely. A `monitorStartingRef` latch — set before the `await` and
  cleared in `finally` — ensures only one `startMonitor` call is ever in
  flight at a time.

## Assistant

- After the assistant flashes a board, Bancada now returns focus to the
  Assistant tab rather than the Serial tab. The assistant continues working
  after a flash — reading serial output and judging it against the request —
  so the Serial tab's raw scroll is the wrong place to be watching. The
  assistant's next turn, which contains what it read and its verdict, is what
  matters.

## Documentation

- The "Starting the serial monitor" data-flow diagram in
  `docs/architecture/data-flows.md` now reflects the native-serialport
  architecture introduced in 0.22.0. The old diagram described the
  `arduino-cli monitor` child-process approach (two reader threads, piped
  stdio); the new one correctly shows the single reader thread, the
  `try_clone()` port descriptor, and the `MonitorSession` ownership model.

## Tests

```
cargo test -p bancada-core --lib   767 passed
cargo check -p bancada             clean
npx tsc --noEmit && npx vitest run 1313 passed, 82 files
npm run build                      clean
```
