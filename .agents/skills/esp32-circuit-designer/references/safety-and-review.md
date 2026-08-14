# ESP32 circuit safety and review reference

Use this checklist with Bancada's machine validation. Automated checks catch declared conflicts; they cannot prove that an unidentified module, breadboard, supply, or physical wire matches the declaration.

## Review order

Classify findings as `STOP — unsafe to power`, `Must fix`, `Likely issue`, `Improvement`, or `Looks correct`. Review in this order:

1. Conflicting power sources, reversed polarity, shorts, and overvoltage at GPIOs.
2. Regulator/supply current budget and loads driven directly by GPIOs.
3. Common-ground requirements and intentional isolation.
4. Drivers and flyback protection for relays, motors, solenoids, pumps, and other inductive loads.
5. Exact-board reserved, strapping, USB/JTAG, flash, PSRAM, camera, and onboard-peripheral pins.
6. Pull-ups/pull-downs, protocol wiring, analog limits, and firmware pin mapping.

Put any `STOP — unsafe to power` finding before all other content.

## Compatibility evidence

For each board/module/part, prefer its manufacturer schematic or datasheet. Record the exact part number, voltage, current, pin names, and whether the evidence is `Verified`, `Likely — verify on the module`, or `Unknown — do not connect until verified`.

Never infer a MOSFET/transistor lead order from package shape, a wire's role from color, or hidden photo connections.

## Power and logic

- Treat ESP32 GPIOs as 3.3 V logic. A 5 V peripheral supply does not imply its output is GPIO-safe.
- Distinguish USB/VBUS/5V/VIN from regulated 3V3 according to the exact board.
- Do not connect regulated outputs together merely because their nominal voltages match.
- Show common ground when separate non-isolated supplies exchange signals.
- Sum loads with margin for startup spikes. Use a rated external supply when the board regulator is insufficient.
- For analog input, verify the ADC-capable pin, source impedance, attenuation, and maximum voltage.

## Loads and protection

GPIOs are control signals, not load supplies. Motors, pumps, solenoids, relay coils, servos, heaters, speakers, and large LED loads need an appropriate MOSFET, transistor, driver IC, or compatible module. Add flyback protection unless the driver verifiably includes it. Check gate/base resistance, reset state, ratings, dissipation, and supply polarity.

Every bare LED needs current limiting. I²C SDA/SCL need pull-ups to a compatible rail; verify whether breakout boards already include them. Add decoupling according to device datasheets and load transients.

## Complete wiring declaration

Account for power, ground, signal, enable/reset, chip-select, interrupt, pull-up/down, driver gate/base, flyback diode, and external supply pins. Add a manifest connection for each wire and note intentionally unconnected pins.

For UART, cross TX/RX and share ground. For SPI, declare SCK, MOSI, MISO, every CS, and auxiliary pins. For I²C, show devices in parallel and verify addresses.

## Pre-power and bring-up

Disconnect power while rewiring. Compare every wire against `hardware/wiring.md` and `hardware/circuit.svg`. Confirm polarity, rail voltage, shared grounds, resistors, drivers, protection, split breadboard rails, adjacent-pin shorts, GPIO voltage, and load-current paths.

Bring up the smallest subsystem first: measure rails, scan I²C, read raw ADC, blink one limited LED, or toggle a driver with its final load disconnected. Confirm that result before application logic.
