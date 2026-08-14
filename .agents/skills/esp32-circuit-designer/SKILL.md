---
name: esp32-circuit-designer
description: Design, review, troubleshoot, and maintain ESP32-family circuits whose hardware/circuit.yaml, generated wiring artifacts, Arduino C++ pin constants, selected board profile, and firmware usage must remain synchronized. Use for GPIO assignments, sensors, LEDs, buses, power rails, motors, relays, breadboards, schematics, wiring changes, circuit safety reviews, or any code change that adds or changes a physical pin.
---

# ESP32 Circuit Designer

Treat `hardware/circuit.yaml` as the project's only editable hardware source of truth. Bancada deterministically generates the pin header, SVG, wiring table, BOM, and validation report from it and blocks Verify and Upload when hardware is unsafe, incompatible, stale, or out of sync with Arduino C++.

## Required workflow

1. Inspect the active `sketch.yaml` profile/FQBN, `hardware/circuit.yaml` when present, Arduino `.ino`/`.cpp`/`.h` sources, and the generated validation report.
2. Identify the exact board catalog entry before assigning GPIOs. Do not substitute a related ESP32 family or guess a pin capability. If the exact board is not supported, stop definitive pin assignment and state what must be verified from the official board schematic.
3. Model every part and every electrical connection in `hardware/circuit.yaml`. Include power, ground, signals, required resistors, pull-ups, drivers, flyback protection, current draw, operating voltage, and part verification.
4. Change firmware to consume every declared symbol from `src/bancada_circuit_pins.h`. Do not duplicate its pin numbers in handwritten constants or call Arduino GPIO functions with raw circuit pin numbers.
5. Run circuit synchronization after each manifest edit:

   ```sh
   bash .agents/skills/esp32-circuit-designer/scripts/circuit.sh sync --project <project-dir> --fqbn <active-fqbn>
   ```

6. Run the check after hardware-related code edits and before reporting completion:

   ```sh
   bash .agents/skills/esp32-circuit-designer/scripts/circuit.sh check --project <project-dir> --fqbn <active-fqbn> --json
   ```

7. Resolve every `error` diagnostic. Explain remaining warnings and why they are acceptable. Never bypass, delete, or hand-edit a generated artifact to make a check pass.

For a new circuit, initialize once, then edit the manifest and synchronize:

```sh
bash .agents/skills/esp32-circuit-designer/scripts/circuit.sh init \
  --project <project-dir> \
  --board <catalog-board-id> \
  --name "<circuit-name>" \
  --fqbn <active-fqbn>
```

Supported board ids are `esp32-devkitc-v4`, `esp32-s3-devkitc-1`, `esp32-c3-devkitc-02`, and `esp32-c6-devkitc-1`. Use Bancada's Hardware → Circuit workspace when a guided editor and diagram preview are more useful than YAML editing.

## Artifact contract

Only edit:

- `hardware/circuit.yaml`
- Arduino C++ source files that include the generated header

The synchronization command owns:

- `src/bancada_circuit_pins.h`
- `hardware/circuit.svg`
- `hardware/wiring.md`
- `hardware/bom.csv`
- `hardware/validation.json`

Commit the manifest and generated artifacts together. A digest and byte-for-byte check make manual edits or missed regeneration build-blocking.

## Electrical design rules

Read [references/safety-and-review.md](references/safety-and-review.md) when designing or reviewing a circuit, selecting power/driver topology, or interpreting a warning. The non-negotiable minimum is:

- ESP32 GPIO logic is 3.3 V; never assume a GPIO is 5 V tolerant.
- Never power substantial or inductive loads directly from a GPIO.
- Show common ground wherever non-isolated supplies exchange signals.
- Verify the development-board pinout and the exact external part datasheet.
- Account for every relevant pin and name intentionally unconnected pins in notes.
- Treat boot/strapping, USB, flash, PSRAM, and reserved pins deliberately.
- Verify I²C pull-ups, LED current limiting, load drivers, flyback protection, rail voltage, and total current.
- Power off before rewiring. Separate low-voltage design from mains or high-energy battery work.

## Firmware handoff

Arduino C++ is the only supported firmware binding. Use generated constants directly:

```cpp
#include "src/bancada_circuit_pins.h"

void setup() {
  pinMode(bancada_circuit::PIN_STATUS_LED, OUTPUT);
}
```

When a GPIO changes, update the manifest first, synchronize, then adapt code only if the semantic symbol changed. Keep names semantic (`PIN_SENSOR_SDA`), uppercase, and independent of the numeric GPIO.

## Completion report

State the exact board and active FQBN, components and power assumptions, verified datasheets, changed connections and generated artifacts, electrical warnings or bring-up precautions, and the final check result. Do not call the hardware complete unless synchronization and validation pass.
