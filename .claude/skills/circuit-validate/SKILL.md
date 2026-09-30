---
name: circuit-validate
description: Validate a GPIO wiring plan against board pin safety rules and basic electrical rules
---

# Circuit Validate

Given a board identifier and a list of GPIO→role→component assignments, validate the circuit against:

1. **Level 1 — pin safety** from `core/src/boardprofile.rs` (`check_pin` / `PinVerdict` / `Caveat`)
2. **Level 2 — electrical rules** from `core/src/circuit.rs` (`validate_circuit`)

> **Inside a Bancada agent session**: the `mcp__bancada__validate_circuit` tool runs this automatically. Call it with `board_id` (optional) and `connections` (required). The result is a Markdown report; `isError` is `false` even when issues are found — a report with findings is still a valid tool result.

---

## Usage (contributor / Claude Code)

1. **Identify the board.** Look up its id in `core/src/boardprofile.rs` (the `KNOWN_BOARDS` slice), e.g. `"esp32-s3-devkitc-1"`. Or use `Board::from_id()`.

2. **Build a `CircuitSpec`.**
   ```rust
   use bancada_core::circuit::{CircuitSpec, Connection, ConnectionRole};
   use bancada_core::boardprofile::Board;

   let board = Board::from_id("esp32-s3-devkitc-1").unwrap();
   let spec = CircuitSpec {
       board,
       connections: vec![
           Connection { gpio: 4,  role: ConnectionRole::DigitalOutput, component: Some("LED + 220Ω".into()) },
           Connection { gpio: 8,  role: ConnectionRole::I2cSda,        component: Some("SHT31".into()) },
           Connection { gpio: 9,  role: ConnectionRole::I2cScl,        component: Some("SHT31".into()) },
           Connection { gpio: 8,  role: ConnectionRole::DigitalInput,  component: Some("4.7kΩ pull-up".into()) },
       ],
   };
   ```

3. **Run validation and format the report.**
   ```rust
   let report = bancada_core::circuit::validate_circuit(&spec);
   println!("{}", report.to_markdown());
   ```

---

## ConnectionRole variants

Accept kebab-case (`"digital-output"`) or snake_case (`"digital_output"`):

| Variant | String |
|---------|--------|
| DigitalOutput | `digital-output` |
| DigitalInput  | `digital-input`  |
| AnalogInput   | `analog-input`   |
| I2cSda        | `i2c-sda`        |
| I2cScl        | `i2c-scl`        |
| SpiMosi       | `spi-mosi`       |
| SpiMiso       | `spi-miso`       |
| SpiClk        | `spi-clk`        |
| SpiCs         | `spi-cs`         |
| UartTx        | `uart-tx`        |
| UartRx        | `uart-rx`        |
| Pwm           | `pwm`            |
| Neopixel      | `neopixel`       |

---

## Level 1 — pin safety rules

| Caveat | Condition | Severity |
|--------|-----------|----------|
| `InputOnly` | output role | **Error** |
| `FlashOrPsram` | any role | **Error** |
| `UsbSerialJtag` | any role | **Warn** |
| `Strapping` | output role | **Warn** |
| `Adc2WifiConflict` | `analog-input` role | **Warn** |
| `NotBrokenOut` (verdict) | any role | **Warn** |
| All other caveats | any role | **Info** |

Output roles: `DigitalOutput`, `I2cScl`, `SpiMosi`, `SpiClk`, `SpiCs`, `UartTx`, `Pwm`, `Neopixel`.

---

## Level 2 — electrical rules

| Rule | Condition | Severity |
|------|-----------|----------|
| Duplicate GPIO | same `gpio` used twice | **Error** |
| I2C pull-up missing | any `i2c-sda`/`i2c-scl`, but no component mentions `pull`, `kΩ`, `kohm`, or `resistor` | **Warn** |
| LED without resistor | `digital-output` + component contains `led` (case-insensitive) but not `Ω`, `ohm`, or `resistor` | **Warn** |

Note: Ω detection uses the **original** (non-lowercased) component string because `to_lowercase()` maps `Ω`→`ω`.

---

## Adding a new electrical rule

1. Edit `core/src/circuit.rs` — add logic in `validate_circuit()`.
2. Add a unit test in `circuit::tests`.
3. Run `cargo test -p bancada-core --lib circuit` — watch it pass.
4. Run `cargo check -p bancada` to verify the Tauri layer still compiles.

No changes to `src/api.ts`, `src-tauri/src/lib.rs`, or `core/src/mcp.rs` are needed for a pure logic change.
