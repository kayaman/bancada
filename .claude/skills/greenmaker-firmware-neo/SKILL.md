---
name: greenmaker-firmware-neo
description: Build and evolve GreenMaker firmware according to ESP RainMaker Neo contract discipline, firmware safety constraints, and cross-stack update rules.
---

# GreenMaker Firmware (RainMaker Neo Standards)

Use this skill when the Bancada assistant is helping with GreenMaker firmware work.

## Quick choose

- Build or flash an existing product: follow **Path A**.
- Modify existing firmware behavior: follow **Path B**.
- Add a new product: follow **Path C**.

## Path A: Build and flash existing product (quick path)

```bash
. ~/esp/esp-idf/export.sh
cd firmware/products/<product>
idf.py set-target <esp32|esp32c6|esp32s3>
idf.py build
```

If hardware is connected:

```bash
idf.py flash monitor
```

Run host tests before handoff:

```bash
cmake -S firmware/host_tests -B firmware/host_tests/build -DCMAKE_BUILD_TYPE=Debug
cmake --build firmware/host_tests/build
firmware/host_tests/build/gm_core_tests
```

## Path B: Modify firmware behavior (quick path)

1. Classify the change:
   - internal refactor
   - firmware-visible behavior change
   - contract/wire behavior change
2. Implement firmware changes.
3. If contract/wire behavior changed, update spec and consumers in the same change.
4. Validate:
   ```bash
   make test
   cmake -S firmware/host_tests -B firmware/host_tests/build -DCMAKE_BUILD_TYPE=Debug
   cmake --build firmware/host_tests/build
   firmware/host_tests/build/gm_core_tests
   . ~/esp/esp-idf/export.sh
   cd firmware/products/<touched-product>
   idf.py set-target <target>
   idf.py build
   ```

## Path C: Add a new firmware product (quick path)

1. Copy `firmware/products/grow-light` to `firmware/products/<product-name>`.
2. Rename metadata/constants and implement product node/devices/services/params.
3. Add `spec/vectors/valid/node-config-<product>.json`.
4. Add/update `firmware/host_tests` checks for the new vector.
5. Add product/target in firmware CI matrix.
6. Build and run host tests.

## Non-negotiable standards

1. `spec/` is the wire contract.
   - If wire semantics change, update `spec/protocol.md`, `spec/types.md`, schemas, vectors, and all impacted consumers/tests in Rust/C/Dart/TypeScript.
2. Safety-critical irrigation behavior remains enforced in firmware.
3. Product folders are named by function, never board/chip.
4. Chip selection is done via `idf.py set-target`.
5. Do not commit secrets or device material (`*.pem`, `*.key`, `*.csr`, `*.crt`, `factory*.bin`, `*.tfvars`).

<details>
<summary><strong>Critical detail: first boot identity requirements</strong></summary>

Each board needs a factory identity in `factory_nvs` before normal boot:

```bash
gm admin node create --model <product>
gm admin node flash <node-id> --port /dev/ttyUSB0
```

Factory `model` must match firmware product model.
</details>

<details>
<summary><strong>Critical detail: contract-change trigger checklist</strong></summary>

Treat as contract-changing if any of these change:

- param names, types, ranges, enum values, read/write permissions
- node config JSON shape
- to-cloud/from-cloud payload semantics
- schedule or OTA message semantics

If any item is true, update spec, vectors, firmware, and cross-stack consumers in one change.
</details>

<details>
<summary><strong>Critical detail: CI parity baseline</strong></summary>

Expected firmware matrix:

- grow-light / esp32
- plant-monitor / esp32c6
- irrigation-controller / esp32s3
- light-meter / esp32s3

If product/target support changes, update `.github/workflows/ci.yml` in the same change.
</details>

<details>
<summary><strong>Critical detail: OTA versioning guardrail</strong></summary>

Before producing OTA images:

- bump `FW_VERSION` for new firmware behavior
- bump `CONFIG_VERSION` when node config shape changes
- keep vectors and tests aligned with version/config behavior
</details>

## Handoff format

Report:

- scope path used (A/B/C)
- product and target(s)
- contract changed: yes/no
- files updated in `spec/` and consumers
- exact validation commands run and pass/fail results
