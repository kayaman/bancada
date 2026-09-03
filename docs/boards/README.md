# Board profiles

A **board profile** is what `bancada-idf` knows about the devkit a chip sits
on: its headers pin by pin, the silkscreen labels, the onboard LED and BOOT
button, the USB ports, and — per GPIO — a closed set of **caveats** worth
knowing before wiring to it. The chip is the *target* (`bidf targets`); the
board is this.

The profiles live in one table, `core/src/boards.rs`, and every page in this
directory is **generated from it**: `bidf board <id> --markdown` prints the
page, and a unit test (`every_board_page_matches_the_table`) fails when a
checked-in page differs from the table. Edit the table, never the page.

| Board | id | target |
|---|---|---|
| [DOIT ESP32 DEVKIT V1 (30-pin)](esp32-doit-devkit-v1.md) | `esp32-doit-devkit-v1` | `esp32` |
| [ESP32-S3-DevKitC-1 v1.1](esp32-s3-devkitc-1.md) | `esp32-s3-devkitc-1` | `esp32s3` |
| [ESP32-C6-DevKitC-1 v1.2](esp32-c6-devkitc-1.md) | `esp32-c6-devkitc-1` | `esp32c6` |

## What a profile contains

| Field | Meaning |
|---|---|
| `id` | Stable kebab-case id used everywhere: the CLI, the marker line, the page name. |
| `name`, `vendor`, `revision`, `module` | How the vendor names it, which revision the rows describe, and the module soldered on. Revisions matter: the S3-DevKitC-1 moved its LED from GPIO48 (v1.0) to GPIO38 (v1.1). |
| `target` | The chip, as a `bidf targets` id. |
| `usb` | Each USB connector: its silkscreen label, whether it is a bridge chip (and which) or the SoC's native USB-Serial/JTAG, and the GPIOs it occupies. |
| `led`, `boot_button` | The onboard LED (plain, or an addressable WS2812 that a plain toggle will not light) and the GPIO the BOOT button pulls low. |
| `headers` | One entry per header, top to bottom with the USB connector at the bottom. Each row: the silkscreen label, the GPIO (none for power/ground/reset/NC), notable alternate functions, caveats. |
| `sources`, `notes` | Where the rows came from, and the quirks worth a sentence. |

## The caveats

Closed vocabulary, so the CLI can print it, the window can badge it and the
assistant can explain it without parsing prose. Each has a short label and a
sentence of advice; `bidf pin <gpio>` prints both.

| id | label | advice |
|---|---|---|
| `strapping` | strapping pin | The level at reset selects the boot mode. Leave it at its default during reset; a pull-up or pull-down on it changes how the chip boots. |
| `input-only` | input only | Input only. It cannot drive an output and has no internal pull-up or pull-down. |
| `flash-or-psram` | flash/PSRAM | Wired to the module's SPI flash or PSRAM on some module variants. Using it as GPIO corrupts memory access on those modules. |
| `usb-serial-jtag` | native USB | Native USB D-/D+. Reconfiguring it as a GPIO disables USB-Serial/JTAG, and with it the USB port. |
| `uart0-console` | UART0 console | UART0. The USB-UART bridge and the default console log use it; driving it as GPIO garbles the console. |
| `jtag` | JTAG | A JTAG signal by default. Free to use if you do not debug over JTAG. |
| `adc2-wifi-conflict` | ADC2 — not with Wi-Fi | An ADC2 channel. ADC2 cannot be read while Wi-Fi is on; use an ADC1 pin for analog input alongside Wi-Fi. |
| `onboard-led` | onboard LED | Drives the onboard LED. Free to use, but the LED follows it. |
| `boot-button` | BOOT button | The BOOT button pulls it low. Free to use as an input after boot; holding it low through a reset enters the bootloader. |

Chip-level caveats (the first seven) are stated once per SoC in
`SOC_PINS` and repeated on the rows they apply to; a test keeps the two in
agreement. `check_pin(board, gpio)` is the one function that turns a row into
a verdict — *free*, *caution* (with the caveats), or *not broken out* — and
`bidf pin`, the Board tab and the assistant all call it.

## How a project remembers its board

As one comment line in the project's `sdkconfig.defaults`:

```
# bancada.board = esp32-s3-devkitc-1
```

A comment rather than a `CONFIG_` key on purpose: ESP-IDF's `kconfgen` prints
`note: unknown kconfig symbol …` on every reconfigure for a key it does not
know, while a comment is silent. `sdkconfig.defaults` is hand-written and
committed, so the choice travels with the repository and survives
`idf.py fullclean` (which regenerates `sdkconfig`, not the defaults file).
`bidf board <id>` writes it; `bidf new --board <id>` writes it at creation;
the window's Board tab writes it.

A board cannot be detected — the chip id narrows it to a family, no further —
so the pick is always a person's. The window filters its picker by the
configured target.

## Adding a board

1. Add a `Board` entry to `KNOWN_BOARDS` in `core/src/boards.rs`. Transcribe
   the header rows from the vendor's user guide (record the URL in `sources`);
   if there is none, from the silkscreen, checked against the physical board.
   Put the chip-level facts in `SOC_PINS` if the target is new.
2. `cargo test -p bancada-idf-core boards::` — the invariant tests will tell
   you about a GPIO listed twice, an LED off the header, or a row that
   disagrees with its chip.
3. `bidf board <id> --markdown > docs/boards/<id>.md`, and add the row to the
   table at the top of this file.
4. Look at the board and the page side by side once. The test proves the page
   matches the table; only a person proves the table matches the board.

Pages are ASCII and box-drawing only (see `docs/architecture/conventions.md`
§7): the layout block is text, not an image.
