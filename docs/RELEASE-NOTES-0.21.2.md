# bancada 0.21.2

**Two papercuts in the platform choice that shipped in 0.21.0, and a warning
that could never have been right.** The first two break nothing; both are the
kind of thing that makes a new feature feel unfinished the third time you use
it. The third fired on every upload to a native-USB Espressif board.

## Chips had tool spellings in a human-facing picker

The target select listed `esp32s3` and `esp32c6` — the identifiers the
toolchain wants, shown to the person choosing. Nobody calls the chip that.
"ESP32-S3" is how the silkscreen, the datasheet and every forum post write it.

`targets::Target` has carried Espressif's own spelling all along;
`known_idf_targets` was throwing it away and returning bare ids. It now returns
`{ id, name, native_usb }`, and the select shows the name while sending the id.

That separation is the whole point, so a test pins it: picking "ESP32-C6" must
still send `esp32c6`. What lands in `sdkconfig.defaults` is unchanged — this is
a display fix, and a display fix that quietly altered the build configuration
would be a much worse bug than the one it replaced.

`native_usb` rides along unused, for a later "this board needs no bridge chip
to flash" note. It is already in the table; carrying it now costs nothing and
saves widening the shape twice.

## The platform reset to Arduino on every open

Someone working through a run of ESP-IDF projects re-picked ESP-IDF every
single time. `last_new_project_platform` now follows `last_new_project_parent`
exactly — written after a successful creation, restored when the form mounts,
and never allowed to fail a creation. A preference that can break the thing it
is a preference *for* is not worth having.

Two deliberate asymmetries in how it is stored:

`set_last_project_platform` **ignores** anything that is not one of the two
platforms rather than storing it. Storing a value the next launch would have to
guess at is worse than storing nothing — the wizard would fall back to Arduino
while the settings file claimed something else, and the disagreement would be
invisible.

The field is a `String` rather than an enum for the mirror-image reason. A
value written by a newer build must degrade to the default on an older one, not
fail the entire settings load. Strictness at the write boundary, tolerance at
the read boundary. A test covers a settings file written before the field
existed.

Also corrects the invoke count in the README and the IPC contract, 108 → 109.

## Every upload to an ESP32 accused the profile of targeting the wrong board

Flashing a C3 SuperMini raised an error toast that stuck for the whole build:
the port "reports `esp32:esp32:ozobot_drvkit`", while the profile builds for
`makergo_c3_supermini`. Nothing was wrong. No Ozobot has ever been on this
bench.

Native-USB Espressif parts all enumerate on the same `303a:1001` descriptor, so
`arduino-cli board list` can only answer at family granularity: the hidden
`esp32_family` umbrella plus one sibling. In `boards.txt` that sibling is not
even a near miss — `ozobot_drvkit` declares `2d81:1901`, and the umbrella is
the only entry claiming `1001` at all.

`ports.ts` already knew better. `confidentBoardName` has refused family matches
for as long as the fleet has existed, which is why the same toast called the
port a "USB serial bridge" and then named a board in the next clause. The flash
warning was the last caller reading `visibleBoard` as a fact rather than as the
best guess to compile with.

Going silent on ambiguity would have cost the warning that matters — an esp8266
profile on an ESP32 port. So the detected side now carries its own precision:
`comparableIdentity` answers `vendor:arch:board` when arduino-cli is sure and
`vendor:arch` when it is guessing, and `flashTargetMismatch` truncates the
profile to match. All four existing mismatch tests pass untouched.

`detectedFqbn` is deliberately unchanged: choosing something to compile with
still wants the best guess available.

## Tests

```
cargo test --workspace   742 core · 84 src-tauri, 0 failed
npm test                 1276 passed, 78 files
tsc --noEmit             clean
vite build               clean
```

Run the Rust suite with `--test-threads=1`: `cli::tests` has a known parallel
flake — `ExecutableFileBusy`, one test writing a fake tool binary while another
thread execs it — that is not a real failure.

## Unchanged from 0.21.1

The bench passes owed by 0.19.0, 0.20.0, 0.21.0 and 0.21.1 are all still owed,
and this release adds nothing to that debt's substance: a string shown in a
select, a string written to a settings file, and a toast that now knows when to
keep quiet — none of which a board can disagree with. The flash path itself is
untouched: what reaches the board is what 0.21.1 sent. The four-step pass in `RELEASE-NOTES-0.20.0.md`
remains the thing standing between this stack and a build anyone should trust.
