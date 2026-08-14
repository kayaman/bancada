# Bancada project instructions

## Circuit and firmware synchronization

Use the project-local `esp32-circuit-designer` skill for any work involving ESP32 GPIO assignments, physical components, power rails, buses, wiring, circuit review, or Arduino code that consumes or changes hardware pins.

`hardware/circuit.yaml` is the only editable circuit source of truth. Do not hand-edit `src/bancada_circuit_pins.h`, `hardware/circuit.svg`, `hardware/wiring.md`, `hardware/bom.csv`, or `hardware/validation.json`; regenerate them with the skill's circuit command.

Arduino C++ must include and use `src/bancada_circuit_pins.h` for manifest-declared GPIOs. After circuit manifest changes, synchronize the artifacts. After hardware-related firmware changes, and before declaring the work complete, run the circuit check with the active profile FQBN and resolve every error. Keep the manifest, generated artifacts, and firmware changes together.

Projects without `hardware/circuit.yaml` keep their existing build behavior. Once a manifest exists, do not bypass the Verify/Upload circuit guard.
