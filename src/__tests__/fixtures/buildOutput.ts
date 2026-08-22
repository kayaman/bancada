// Verbatim `arduino-cli compile` output, captured on 2026-08-22 on Linux with
// arduino-cli 1.5.0 (commit dd407d42d) against `arduino:avr` 1.8.8 (avr-gcc
// 7.3.0) and `esp32:esp32` 3.3.11 (xtensa-esp-elf gcc 14.2.0). Every export
// below except GIT_SYNC_NOISE is real captured output — nothing is retyped:
// the arrays were generated from the raw stdout/stderr files, so the spacing
// inside every gutter and caret line is byte-exact.
//
// Why this fixture exists: the build console has to read compiler output it
// did not produce, and the *shape* is what the parser stands or falls on.
// Three shape facts here would not have survived guessing:
//
//  1. Streams. gcc diagnostics and the final trailer arrive on STDERR; the
//     blank line and the ANSI-coloured "Used platform" table arrive on
//     STDOUT, and in the merged `build://line` stream the table lands between
//     the diagnostics and the trailer. Memory lines are STDOUT.
//  2. The trailer. arduino-cli 1.5.0 prints ONE line, `Error during build:
//     exit status 1` — not a bare `exit status 1` line, and not
//     `Compilation error: …`. The parser still recognises both of the other
//     forms (older/newer CLIs print them) but no fixture here produces them.
//  3. The gutter is per-toolchain, not per-CLI. avr-gcc 7.3.0 echoes the
//     source line with a BARE caret underneath (` #include <Nope.h>` /
//     `          ^~~~~~~~`); esp32's gcc 14.2.0 uses the gcc>=9 numbered
//     gutter (`    1 | #include <Nope.h>` / `      |          ^~~~~~~~`).
//     A parser that only knows one of them drops half the detail lines.
//
// A fourth fact cost an extra sketch: a `#include` that cannot be resolved is
// a FATAL error, so gcc prints `compilation terminated.` and never reaches the
// undeclared identifier or the -Wsign-compare warning in the same file. One
// "broken" sketch therefore cannot yield both shapes — AVR_FATAL_INCLUDE is
// `Broken/` (bad include), AVR_ERRORS is the same sketch with the include
// removed (`BrokenNoInc/`), which is what actually produces error + warning.
//
// Sketch sources live in the capture scratch dir (not checked in); the
// absolute paths below are the ones gcc reported, because arduino-cli injects
// `#line N "<abs>/Sketch.ino"` into the merged .ino.cpp. That
// `/tmp/claude-…/<uuid>/scratchpad` prefix is a dead scratch directory — it is
// kept verbatim precisely because gcc printed it; do not "fix" it to a real
// path or the capture stops being a capture.

import type { OutputLine } from "../../api";

export const AVR_ERRORS: readonly OutputLine[] = [
  { stream: "stderr", line: "/tmp/claude-1000/-home-kayaman-Projects-bancada/a16ff65a-b777-4db8-91b0-95302faffca8/scratchpad/BrokenNoInc/BrokenNoInc.ino: In function 'void loop()':" },
  { stream: "stderr", line: "/tmp/claude-1000/-home-kayaman-Projects-bancada/a16ff65a-b777-4db8-91b0-95302faffca8/scratchpad/BrokenNoInc/BrokenNoInc.ino:10:13: warning: comparison between signed and unsigned integer expressions [-Wsign-compare]" },
  { stream: "stderr", line: "   if (count < limit) {" },
  { stream: "stderr", line: "       ~~~~~~^~~~~~~" },
  { stream: "stderr", line: "/tmp/claude-1000/-home-kayaman-Projects-bancada/a16ff65a-b777-4db8-91b0-95302faffca8/scratchpad/BrokenNoInc/BrokenNoInc.ino:13:3: error: 'undeclaredHelper' was not declared in this scope" },
  { stream: "stderr", line: "   undeclaredHelper();" },
  { stream: "stderr", line: "   ^~~~~~~~~~~~~~~~" },
  { stream: "stdout", line: "" },
  { stream: "stdout", line: "\u001b[92mUsed platform\u001b[0m \u001b[92mVersion\u001b[0m \u001b[90mPath\u001b[0m" },
  { stream: "stdout", line: "\u001b[93marduino:avr\u001b[0m   1.8.8   \u001b[90m/home/kayaman/.arduino15/packages/arduino/hardware/avr/1.8.8\u001b[0m" },
  { stream: "stderr", line: "Error during build: exit status 1" },
];

export const AVR_FATAL_INCLUDE: readonly OutputLine[] = [
  { stream: "stderr", line: "/tmp/claude-1000/-home-kayaman-Projects-bancada/a16ff65a-b777-4db8-91b0-95302faffca8/scratchpad/Broken/Broken.ino:1:10: fatal error: Nope.h: No such file or directory" },
  { stream: "stderr", line: " #include <Nope.h>" },
  { stream: "stderr", line: "          ^~~~~~~~" },
  { stream: "stderr", line: "compilation terminated." },
  { stream: "stdout", line: "" },
  { stream: "stdout", line: "\u001b[92mUsed platform\u001b[0m \u001b[92mVersion\u001b[0m \u001b[90mPath\u001b[0m" },
  { stream: "stdout", line: "\u001b[93marduino:avr\u001b[0m   1.8.8   \u001b[90m/home/kayaman/.arduino15/packages/arduino/hardware/avr/1.8.8\u001b[0m" },
  { stream: "stderr", line: "Error during build: exit status 1" },
];

export const ESP32_ERRORS_WITH_INCLUDE_CHAIN: readonly OutputLine[] = [
  { stream: "stderr", line: "/tmp/claude-1000/-home-kayaman-Projects-bancada/a16ff65a-b777-4db8-91b0-95302faffca8/scratchpad/Chain/Chain.ino: In function 'void setup()':" },
  { stream: "stderr", line: "/tmp/claude-1000/-home-kayaman-Projects-bancada/a16ff65a-b777-4db8-91b0-95302faffca8/scratchpad/Chain/Chain.ino:7:21: error: no matching function for call to 'push_back(const char [11])'" },
  { stream: "stderr", line: "    7 |   readings.push_back(\"not an int\");" },
  { stream: "stderr", line: "      |   ~~~~~~~~~~~~~~~~~~^~~~~~~~~~~~~~" },
  { stream: "stderr", line: "In file included from /home/kayaman/.arduino15/packages/esp32/tools/esp-x32/2601/xtensa-esp-elf/include/c++/14.2.0/vector:66," },
  { stream: "stderr", line: "                 from /home/kayaman/.arduino15/packages/esp32/tools/esp-x32/2601/xtensa-esp-elf/include/c++/14.2.0/functional:64," },
  { stream: "stderr", line: "                 from /home/kayaman/.arduino15/packages/esp32/hardware/esp32/3.3.11/cores/esp32/HardwareSerial.h:49," },
  { stream: "stderr", line: "                 from /home/kayaman/.arduino15/packages/esp32/hardware/esp32/3.3.11/cores/esp32/Arduino.h:202," },
  { stream: "stderr", line: "                 from /home/kayaman/.cache/arduino/sketches/DDBBFF4EB9CCA0684E0CE05B9FFE68F6/sketch/Chain.ino.cpp:1:" },
  { stream: "stderr", line: "/home/kayaman/.arduino15/packages/esp32/tools/esp-x32/2601/xtensa-esp-elf/include/c++/14.2.0/bits/stl_vector.h:1283:7: note: candidate: 'constexpr void std::vector<_Tp, _Alloc>::push_back(const value_type&) [with _Tp = int; _Alloc = std::allocator<int>; value_type = int]' (near match)" },
  { stream: "stderr", line: " 1283 |       push_back(const value_type& __x)" },
  { stream: "stderr", line: "      |       ^~~~~~~~~" },
  { stream: "stderr", line: "/home/kayaman/.arduino15/packages/esp32/tools/esp-x32/2601/xtensa-esp-elf/include/c++/14.2.0/bits/stl_vector.h:1283:7: note:   conversion of argument 1 would be ill-formed:" },
  { stream: "stderr", line: "/tmp/claude-1000/-home-kayaman-Projects-bancada/a16ff65a-b777-4db8-91b0-95302faffca8/scratchpad/Chain/Chain.ino:7:22: error: invalid conversion from 'const char*' to 'std::vector<int>::value_type' {aka 'int'} [-fpermissive]" },
  { stream: "stderr", line: "    7 |   readings.push_back(\"not an int\");" },
  { stream: "stderr", line: "      |                      ^~~~~~~~~~~~" },
  { stream: "stderr", line: "      |                      |" },
  { stream: "stderr", line: "      |                      const char*" },
  { stream: "stderr", line: "/home/kayaman/.arduino15/packages/esp32/tools/esp-x32/2601/xtensa-esp-elf/include/c++/14.2.0/bits/stl_vector.h:1300:7: note: candidate: 'constexpr void std::vector<_Tp, _Alloc>::push_back(value_type&&) [with _Tp = int; _Alloc = std::allocator<int>; value_type = int]' (near match)" },
  { stream: "stderr", line: " 1300 |       push_back(value_type&& __x)" },
  { stream: "stderr", line: "      |       ^~~~~~~~~" },
  { stream: "stderr", line: "/home/kayaman/.arduino15/packages/esp32/tools/esp-x32/2601/xtensa-esp-elf/include/c++/14.2.0/bits/stl_vector.h:1300:7: note:   conversion of argument 1 would be ill-formed:" },
  { stream: "stderr", line: "/tmp/claude-1000/-home-kayaman-Projects-bancada/a16ff65a-b777-4db8-91b0-95302faffca8/scratchpad/Chain/Chain.ino:7:22: error: invalid conversion from 'const char*' to 'std::vector<int>::value_type' {aka 'int'} [-fpermissive]" },
  { stream: "stderr", line: "    7 |   readings.push_back(\"not an int\");" },
  { stream: "stderr", line: "      |                      ^~~~~~~~~~~~" },
  { stream: "stderr", line: "      |                      |" },
  { stream: "stderr", line: "      |                      const char*" },
  { stream: "stdout", line: "" },
  { stream: "stdout", line: "\u001b[92mUsed platform\u001b[0m \u001b[92mVersion\u001b[0m \u001b[90mPath\u001b[0m" },
  { stream: "stdout", line: "\u001b[93mesp32:esp32\u001b[0m   3.3.11  \u001b[90m/home/kayaman/.arduino15/packages/esp32/hardware/esp32/3.3.11\u001b[0m" },
  { stream: "stderr", line: "Error during build: exit status 1" },
];

export const AVR_LINKER_UNDEFINED_REFERENCE: readonly OutputLine[] = [
  { stream: "stderr", line: "/tmp/ccHuwhAA.ltrans0.ltrans.o: In function `setup':" },
  { stream: "stderr", line: "/tmp/claude-1000/-home-kayaman-Projects-bancada/a16ff65a-b777-4db8-91b0-95302faffca8/scratchpad/Linker/Linker.ino:4: undefined reference to `helper()'" },
  { stream: "stderr", line: "collect2: error: ld returned 1 exit status" },
  { stream: "stdout", line: "" },
  { stream: "stdout", line: "\u001b[92mUsed platform\u001b[0m \u001b[92mVersion\u001b[0m \u001b[90mPath\u001b[0m" },
  { stream: "stdout", line: "\u001b[93marduino:avr\u001b[0m   1.8.8   \u001b[90m/home/kayaman/.arduino15/packages/arduino/hardware/avr/1.8.8\u001b[0m" },
  { stream: "stderr", line: "Error during build: exit status 1" },
];

export const AVR_OK_MEMORY: readonly OutputLine[] = [
  { stream: "stdout", line: "Sketch uses 924 bytes (2%) of program storage space. Maximum is 32256 bytes." },
  { stream: "stdout", line: "Global variables use 9 bytes (0%) of dynamic memory, leaving 2039 bytes for local variables. Maximum is 2048 bytes." },
];

// HANDWRITTEN — not compiler output. `build://line` is a shared stream: core
// installs, sketch sync and the agent's MCP verify all push through it, so the
// parser has to stay silent (no summary strip, no diagnostics) on traffic that
// merely looks like a build. Modelled on the repo's sync output.
export const GIT_SYNC_NOISE: readonly OutputLine[] = [
  { stream: "stdout", line: "Syncing sketch to origin…" },
  { stream: "stderr", line: "From github.com:kayaman/embedded" },
  { stream: "stderr", line: " * branch            main       -> FETCH_HEAD" },
  { stream: "stdout", line: "Updating 751237e..b0507eb" },
  { stream: "stdout", line: "Fast-forward" },
  { stream: "stdout", line: " src/main.cpp | 12 ++++++------" },
  {
    stream: "stdout",
    line: " 1 file changed, 6 insertions(+), 6 deletions(-)",
  },
];

/** Sketch directories the captures above were compiled from — the parser's
 *  `relativeToSketch` needs the same absolute prefix gcc reported. */
const SCRATCH =
  "/tmp/claude-1000/-home-kayaman-Projects-bancada/a16ff65a-b777-4db8-91b0-95302faffca8/scratchpad";
export const AVR_ERRORS_SKETCH_DIR = `${SCRATCH}/BrokenNoInc`;
export const AVR_FATAL_SKETCH_DIR = `${SCRATCH}/Broken`;
export const ESP32_SKETCH_DIR = `${SCRATCH}/Chain`;
