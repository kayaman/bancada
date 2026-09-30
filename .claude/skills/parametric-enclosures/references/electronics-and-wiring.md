# Electronics and wiring inside the enclosure

A box that fits the board can still fail as a product: the ESP32 resets every time the
pump starts, a Dupont lead drops off inside a closed lid, the lid pinches the display
ribbon, one pull on a cable rips its solder joint. This file covers the electrical side
of the brief and the room the wiring needs. Work through it before sizing the cavity:
the wiring bay, the connector heights and `heat_w` all come out of it.

## Contents
1. The electrical brief
2. Power budget and supplies
3. Protection
4. Choosing pins
5. Driving loads and reading sensors
6. Wires and connectors
7. Laying out the harness
8. Using the template's wiring bay
9. What to deliver

## 1. The electrical brief

Extract what the conversation gives; assume and tag the rest.

- **Source**: USB (port or charger), wall adapter (voltage, current, plug), battery
  (chemistry, capacity), or mains. [USB 5 V from a 1 A+ charger]
- **Every module and load**, with its supply voltage and its typical and peak current:
  MCU board, sensors, display, relays, motors, servos, LEDs, radios.
- **Connections**: which bus or pin each module uses, and whether modules plug into a
  carrier board or hang on wires.
- **Cables that leave the box**: what is at the other end, cable Ø, length, and whether
  it is ever unplugged.

## 2. Power budget and supplies

Budget every rail on its peak, not its average. Wi-Fi TX bursts, a servo stall and an
LED strip at full white are short, they coincide, and they are why boards reset.

| Load | Typical | Peak or note |
|---|---|---|
| ESP32, S3, C3, C6 with Wi-Fi | 80–150 mA | plan 500 mA for TX and calibration bursts |
| ESP32 DevKit in deep sleep | several mA | regulator, USB-UART chip and power LED; the chip alone is ≈ 10 µA |
| Arduino Uno R3 / Nano | ≈ 50 / 20 mA | |
| Raspberry Pi 4 / 5 | | official 5.1 V 3 A / 5 A supply; a Pi 5 on 3 A limits USB devices to 600 mA |
| Raspberry Pi Zero 2 W | | 5.1 V 2.5 A supply |
| 5 V relay module | 70–90 mA per channel | coil plus LEDs |
| SG90 / MG996R servo | 100–250 mA / 0.5–0.9 A moving | stall ≈ 0.65 A / 2.5 A |
| WS2812B pixel | ≈ 20 mA in mixed colours | 60 mA at full white |
| MQ-x gas sensor | ≈ 150 mA at 5 V | continuous heater, and a heat source |
| PMS5003-class particulate sensor | ≈ 100 mA | fan |
| SIM800L GSM module | | 2 A bursts at 3.4–4.4 V; big capacitor at the module |
| I2C sensors, small OLEDs | < 1–20 mA | |

- Sum the peaks that can happen together on each rail. The source must cover that sum
  with ≥ 25 % margin. A USB 2.0 port gives 500 mA, USB 3 gives 900 mA, a phone charger
  1–3 A.
- A dev board's 5 V and 3V3 pins go through the board's own fuse, diode and regulator.
  They are fine for sensors, a display and a relay coil or two. Servos, motors, LED
  strips and heaters get a feed from the supply, in parallel with the board, on a
  common ground.
- A linear regulator turns (Vin − Vout) × I into heat. An Uno on a 12 V jack puts
  7 V × 0.15 A ≈ 1 W into its regulator for 150 mA drawn from the 5 V pin, and a
  SOT-223 regulator on a small board runs hot above roughly 0.5–1 W. A buck converter
  is 85–90 % efficient: input current ≈ Vout × Iout / (0.85 × Vin).
- The AMS1117-3.3 on most ESP32 DevKits drops out around 1.1 V, so it needs ≥ 4.4 V in.
  A long thin USB lead plus a Wi-Fi burst browns the board out. Keep the 5 V feed short
  and thick and add 100–470 µF near the module.
- Adjustable buck modules (LM2596, MP1584, XL4015) ship at an arbitrary output voltage.
  Set it with a meter before connecting anything, then lock the trimmer. Derate the
  headline current by about a third without a heatsink.
- A board powered through USB-C needs 5.1 kΩ pull-downs on both CC pins. Without them a
  C-to-C charger supplies nothing and the board only works from A-to-C cables. Many
  cheap boards omit them, so check before designing around a USB-C input.
- `heat_w` is what dissipates inside the box: MCU, regulator and converter losses,
  driver and MOSFET losses, resistors, a gas-sensor heater. A pump or LED strip outside
  the box is not in it.

Batteries, electrical side (space, venting and child safety are in
`environment-and-safety.md` §7):

- A 1S Li-ion or LiPo cell is 4.2 V full, 3.7 V nominal and 3.0–3.3 V empty. It cannot
  feed an AMS1117. Use a low-dropout regulator rated ≥ 500 mA (AP2112K-3.3, RT9080,
  ME6211) or a buck-boost converter.
- Battery life ≈ 0.8 × capacity / average current, where the average is
  (I_active × t_active + I_sleep × t_sleep) / period. A 2000 mAh cell, 10 s at 120 mA
  every 10 min: with a DevKit's 8 mA sleep the average is ≈ 10 mA, about 7 days; with a
  20 µA sleep it is ≈ 2 mA, about 33 days. Battery nodes need a low-quiescent board or a
  bare module, not a DevKit.
- TP4056 charger boards charge at 1 A with the stock 1.2 kΩ R_PROG. For a smaller cell
  keep the charge current ≤ 1 C: 2 kΩ ≈ 580 mA, 3 kΩ ≈ 400 mA, 5 kΩ ≈ 250 mA. Use the
  variant with the DW01A protection chip and take the load from OUT+/OUT−, not from
  the B+/B− cell pads.
- JST-PH battery polarity is not standard between sellers. Check red and black against
  the board's + and − before the first plug-in; a reversed cell kills the charger or
  the board at once.
- Put the fuse, or use a protected cell or BMS, as close to the cell as possible.
  Everything between the cell and the fuse is unprotected wire.

## 3. Protection

At the power entry, in this order: fuse, reverse-polarity protection, bulk capacitor,
regulators.

- **Fuse**: rated ≥ 1.25 × the maximum continuous current and ≤ the rating of the
  thinnest wire it protects (§6). Slow-blow for motors, pumps and large capacitors. A
  polyfuse (PTC) resets itself but trips slowly, at about twice its hold current.
- **Reverse polarity** on every input a user can plug: a series Schottky (SS34, 1N5819)
  costs 0.3–0.6 V, which is fine at 12 V and too much ahead of a 5 V → 3.3 V LDO. A
  P-MOSFET "ideal diode" costs almost nothing.
- **Flyback diode** across every coil and brushed motor: relays, solenoids, pumps, DC
  motors. A 1N4007 for slow switching, a Schottky rated for the load current under PWM.
  Relay modules carry one; bare relays and MOSFET switches do not.
- **Bulk capacitors**: 100–470 µF at an ESP32's supply on long leads, 1000 µF across an
  LED strip's input, 470–1000 µF on a servo rail, 47–100 µF across a stepper driver's
  VMOT right at the driver. Voltage rating ≥ 1.5 × the rail.
- **Lines that leave the box** (buttons on long leads, external sensors, anything a user
  touches): an ESD or TVS diode at the connector and 100 Ω–1 kΩ in series into the pin.

## 4. Choosing pins

A pin that looks free on the silkscreen can stop the board booting, corrupt its flash,
or read nothing while Wi-Fi is on. Check every entry in the pin map against its board.

| Board or chip | Avoid, or use with care | Also |
|---|---|---|
| ESP32 (WROOM, WROVER, DevKitC, DOIT) | 6–11 flash; strapping 0, 2, 5, 12, 15 (12 high at reset selects 1.8 V flash and the board boot-loops); 1/3 UART0 console; 16/17 on WROVER (PSRAM) | 34–39 input only, no internal pulls; ADC2 (0, 2, 4, 12–15, 25–27) reads nothing with Wi-Fi on, so analog goes on ADC1 (32–39); 1, 3, 5, 14, 15 pulse or go high at boot |
| ESP32-S3 | strapping 0, 3, 45, 46; 26–32 flash, and 33–37 on octal-PSRAM modules (R8, R8V); 19/20 native USB; 43/44 UART0 | ADC2 (11–20) not with Wi-Fi |
| ESP32-C3 | strapping 2, 8, 9; 12–17 flash; 18/19 native USB; 20/21 UART0 | analog on ADC1 (0–4) only |
| ESP32-C6 | strapping 4, 5, 8, 9, 15; 24–30 flash; 12/13 native USB; 16/17 UART0 | ADC1 only |
| Pico / Pico W (RP2040) | Pico W: 23, 24, 25, 29 belong to the radio, and its LED is on the radio chip | ADC on 26–28; default drive 4 mA (up to 12 mA); keep 3V3 OUT under 300 mA |
| Pico 2 (RP2350) | as Pico | erratum E9: internal pull-downs latch. Buttons to GND with pull-ups, or an external pull-down ≤ 8.2 kΩ |
| Uno R3 / Nano (ATmega328P) | D0/D1 are the USB serial; D13 carries the LED | 5 V logic; 20 mA per pin, 200 mA per chip; A6/A7 (Nano) analog input only; interrupts on D2/D3 |
| Uno R4 (RA4M1) | | 5 V logic but 8 mA per pin: drive LEDs through a transistor |
| Raspberry Pi 40-pin header | GPIO2/3 have 1.8 kΩ pull-ups on board (I2C); 14/15 UART | 3.3 V logic; ~16 mA per pin; back-powering through a 5 V pin bypasses the input protection |

- ESP32, RP2040/RP2350 and Raspberry Pi pins are 3.3 V only. A 5 V signal into one goes
  through a divider or a 74LVC buffer powered from 3.3 V; I2C to a 5 V device goes
  through a BSS138 shifter; 3.3 V driving 5 V logic that needs ≥ 3.5 V (WS2812) goes
  through a 74AHCT125 powered from 5 V.
- A button never sits on a strapping pin: held during a reset, it changes the boot
  mode. On ESP32 34–39 it needs an external 10 kΩ pull-up.
- A load that must stay off while the board boots (relay, MOSFET gate, heater) never
  sits on a pin that pulses at boot, and its input gets a pull-down so a floating pin
  cannot switch it.
- Read the board in hand, not memory: clones move pins. Some 0.96" OLED modules swap
  VCC and GND, and relay and sensor boards reorder their headers.
- A GPIO is a signal, not a supply. Past an indicator LED, switch through a transistor.

## 5. Driving loads and reading sensors

- **LEDs**: R = (Vsupply − Vf) / I, with Vf ≈ 2.0 V for red and yellow, 3.0 V for blue,
  green and white. From 3.3 V a white LED barely lights; switch it from 5 V.
- **Relay modules**: many "5 V" modules with an active-low optocoupler input chatter or
  never release on 3.3 V logic. Use a module rated for 3.3 V input, or one with a
  JD-VCC jumper (VCC at 3.3 V, JD-VCC at 5 V), and test that it switches off. The
  "10 A 250 VAC" on the relay is for resistive loads, and cheap modules have neither the
  traces nor the creepage to carry it. Mains-side wiring follows
  `environment-and-safety.md` §8, and a certified smart plug or contactor is usually
  the better answer.
- **DC loads** (pumps, solenoids, 12 V strips, fans): a logic-level N-MOSFET on the low
  side instead of a relay. Pick one whose R_DS(on) is specified at V_GS = 2.5 V for
  3.3 V logic (AO3400A in SOT-23, to about 2 A). IRF520 modules need about 10 V on the
  gate and barely conduct from an MCU; an IRLZ44N is marginal at 3.3 V. Fit 100 Ω in
  series with the gate, 10–100 kΩ from gate to source, and a flyback diode on inductive
  loads. MOSFET loss = I² × R_DS(on), and it goes into `heat_w`.
- **Motors**: TB6612FNG or DRV8833 rather than an L298N, which drops 2–3 V and turns it
  into heat. Stepper drivers: set the current limit, and never plug or unplug a motor
  while the driver is powered.
- **Servos**: their own 5–6 V supply, sized for the servos that can stall together; a
  common ground with the MCU; 470–1000 µF across the servo rail near the servos. Leads:
  brown or black ground, red V+, orange, yellow or white signal.
- **WS2812 strips**: fed from the supply, not through the board, with 1000 µF across the
  strip input and 300–500 Ω in the data line at the first pixel. From a 3.3 V MCU, add
  the 74AHCT125. Budget 60 mA per pixel at full white, or cap the brightness in firmware
  and say so. Connect ground first. Feed long or dense strips at both ends; a starved
  far end turns yellow.
- **I2C**: one set of pull-ups per bus, 2.2–4.7 kΩ to the bus voltage. Breakouts often
  carry their own, and paralleled boards add up; stay above ~1 kΩ at 3.3 V. The bus is
  for inside the box: keep it under about 1 m and never twist SDA with SCL. For a sensor
  on a longer cable, drop to 100 kHz and add a differential extender (PCA9615), or pick
  a sensor with a long-cable interface (1-Wire, RS-485, 4–20 mA). Qwiic / STEMMA QT is a
  4-pin JST-SH at 3.3 V: black GND, red 3.3 V, blue SDA, yellow SCL. A 5 V module such as
  a PCF8574 LCD backpack pulls the bus to 5 V: run it at 3.3 V if it allows, or shift.
- **1-Wire (DS18B20)**: one 4.7 kΩ pull-up at the MCU end, three-wire powered mode, a
  daisy chain rather than a star. Tens of metres work on twisted pair.
- **UART**: TX to RX, RX to TX, shared ground. A 5 V TX into a 3.3 V RX goes through a
  divider (1 kΩ series, 2 kΩ to ground).
- **SPI displays**: leads under about 15 cm, or a lower clock; each device its own CS.
- **Buttons, pots and analog inputs**: buttons to GND with pull-ups, debounced in
  firmware; on leads over ~30 cm add 1 kΩ in series and 100 nF to ground at the pin.
  Pots across 3V3 and GND (never 5 V into a 3.3 V ADC), wiper to an ADC1 pin with
  100 nF. The ESP32 ADC is non-linear near both rails: calibrate, or stay mid-range.

## 6. Wires and connectors

Size wire by current and by voltage drop. On 3.3 V and 5 V rails the drop usually
decides.

| AWG | mm² | mΩ per m | Max in a closed box | Typical use |
|---|---|---|---|---|
| 30 | 0.05 | 339 | 0.4 A | signals only |
| 28 | 0.08 | 213 | 0.7 A | ribbon, Dupont jumpers, signals |
| 26 | 0.13 | 134 | 1.1 A | signals, sensor and module power |
| 24 | 0.20 | 84 | 1.8 A | board power to about 1.5 A |
| 22 | 0.33 | 53 | 3.5 A | 5 V feeds, servos |
| 20 | 0.52 | 33 | 5.5 A | LED strip feeds, pumps |
| 18 | 0.82 | 21 | 8 A | |
| 16 | 1.31 | 13 | 11 A | battery packs, heaters |

The current column is about half the free-air chassis-wiring figure: bundled PVC wire in
a closed plastic box runs hot, and PETG-CF softens at 75–85 °C (PLA near 55 °C).

Drop over a supply-and-return pair: ΔV = I × R_per_m × 2 × L. Keep it ≤ 3–5 % of the
rail. A 3 A LED strip 0.5 m away loses 3 × 0.053 × 1.0 = 0.16 V (3 %) on 22 AWG and
0.40 V (8 %) on 26 AWG.

- Stranded wire for anything that moves or gets handled: lid harnesses, cables.
  Silicone insulation stays flexible and survives 150–200 °C. Solid core is for
  breadboards; it cracks when flexed. PVC hookup wire (UL1007) is rated 80 °C, 300 V.
- Red for each positive rail (a second colour per extra rail), black for ground, and
  the colours recorded in the wiring table. Mains follows the local code.

| Family | Pitch mm | Per contact | Wire | Use |
|---|---|---|---|---|
| Dupont 2.54 mm housings | 2.54 | ~1–3 A, varies | 22–28 AWG | bench only: friction fit, unkeyed, shakes loose |
| JST-XH | 2.5 | 3 A | 22–30 AWG | board-to-wire inside the box |
| JST-PH | 2.0 | 2 A | 24–32 AWG | LiPo cells, small modules |
| JST-SH (Qwiic, STEMMA QT) | 1.0 | 1 A | 28–32 AWG | I2C modules |
| JST-VH | 3.96 | 10 A | 16–22 AWG | power inside the box |
| XT30 / XT60 | | 15 / 30 A | 16 / 12–14 AWG | battery packs |
| Screw terminal | 5.0–5.08 | 10–16 A by make | 12–26 AWG | field wiring |
| Lever connector (WAGO 221) | | per rating | 0.2–4 mm² | joining supply leads |

- Every connection that carries power is keyed and latched or screwed. Nothing that
  carries power goes on a reversible 2-pin header.
- One connector type per voltage in a box. Two identical 2-pin plugs for 5 V and 12 V
  will eventually meet the wrong socket.
- Crimp with the tool made for the contact family, tug-test every crimp, and don't
  solder a crimp contact.
- Ferrules on stranded wire in screw terminals. A solder-tinned end creeps under the
  screw and works loose.
- Heat-shrink every solder joint and splice, and stagger splices along a bundle.
- Label both ends of every wire with its ID from the wiring table.

Past about ten leads between modules, or once power runs through jumpers, move the
modules onto a carrier board: stripboard or perfboard with female headers and screw
terminals or JST headers for field wiring, or a small KiCad board. The enclosure then
holds one board with mounting holes, which the template already handles. On a carrier
you lay out yourself: M3 holes (Ø 3.2) about 3.5 mm in from each corner, connectors at
the board edge with the socket face on the edge, tall parts away from the corners where
the lid columns go, and no copper under the module's antenna.

## 7. Laying out the harness

List every run with its gauge before placing anything, then place modules so the runs
are short and don't cross.

- **Flow**: power entry, fuse and protection, regulator, loads, in one direction across
  the box. Switching and hot parts (buck converters, relays, motor drivers) on one side;
  sensors, analog leads and the antenna on the other. Mains in its own compartment
  behind a full-height wall (`environment-and-safety.md` §8).
- **Room**: wires need floor and height that the board outline doesn't show. Give them a
  wiring bay beside the board (§8). Above a top-entry connector, allow roughly 20 mm for
  Dupont, 15 mm for JST-XH, 10 mm for JST-PH and 8 mm for JST-SH, plug plus wire bend,
  in `comp_top`. Side-entry headers save most of that.
- **Bends**: ≥ 3 × Ø for single hookup wire, ≥ 5–10 × Ø for jacketed cable, and the first
  5–10 mm behind a connector or solder joint straight.
- **Tie points**: a zip-tie anchor 10–20 mm inside every cable exit, so a pull stops at
  the anchor and never reaches a solder joint or a connector, and one every ~50–80 mm
  along a long run. A printed clip is an elastic feature: on the default PETG-CF
  profile use anchors and ties.
- **Keep wires out of**: the lid seam (a lead over the tongue-and-groove gets pinched
  and stops the lid seating), the screw columns, the antenna keepout (copper detunes it
  as a screw does), the air around hot parts (≥ 5 mm from regulators, heatsinks and
  power resistors), and the sensor chamber (wires conduct heat in).
- **Parts on the lid** (display, buttons, LEDs, fan): either a keyed connector at the
  board so the lid lifts off completely, or a service loop long enough to lay the open
  lid beside the base: the path from the connector over the base wall to the part, plus
  about 50 mm. Anchor the loop at the base so closing folds it into the bay, not into
  the seam. Use stranded or silicone wire or ribbon. An FPC or ribbon gets no crease
  within 5 mm of its connector and room to open the latch.
- **Pass-throughs**: through the sensor chamber divider route wires, not connectors. The
  notch (template `wire_pass`) is sized for the bundle, so crimp after passing or keep
  the connector on the chamber side. Plug the notch with foam when the chamber must stay
  thermally or optically isolated.
- **Cable exits**: hole = cable Ø + 0.3–0.5 mm with an anti-chafe chamfer on both faces
  (`cable_exit_cut()`); a gland where it is damp or outdoors; a drip loop below the
  entry outdoors. A cable whose moulded plug is bigger than the hole needs a notch split
  across the lid seam or a slotted grommet.
- **Noise**: run each supply as a pair, + and − together (twisted when long); keep signal
  leads away from buck inductors and relay coils; join separate supplies' grounds at one
  point.
- **Assembly order** goes in BUILD NOTES: what plugs in before the board goes in, which
  terminals a screwdriver must reach with the board fitted (screws up or toward the bay,
  about 10 mm clear above them), and when the lid harness connects.

## 8. Using the template's wiring bay

`bay_front` and `bay_back` set the floor between the board and the front or back wall,
along its whole length; a column zone on that side already counts toward it. Put the bay
on a side without connector cutouts, turning the board with `pcb_rot` if needed: a
socket behind a bay is out of reach, and the template asserts it.

`cable_exits` entries are `[face, pos, z, cable_d]`. The face is `"front"` or `"back"`,
`pos` is the X of the exit centre from the board's left edge (board x when
`pcb_rot = 0`), and `z` is the centre height above the floor. Each exit gets a hole of
`cable_d + exit_clear` with anti-chafe chamfers (a teardrop roof from `teardrop_min` up)
and a `tie_anchor()` on the floor `tie_offset` inside the wall, its tunnel along X, for
a `tie_w` strap. The template asserts that the floor on that side fits the anchor, and
that every exit stays clear of the floor, the lid split, the corner columns and the ribs
that run along the wall, the board stops, the chamber divider, the vent bands and the
antenna keepout. It echoes the exit height for a straight run over the anchor, and a
cable tie per exit in the BOM.

For more tie points or a channel along a run, place `tie_anchor()` or `cable_channel()`
in `base()` the same way, outside the board footprint and at least 1 mm from the PCB
edge. Add one zip tie per anchor to the fastener BOM.

## 9. What to deliver

A WIRING section in the reply, for any design with more than one module, a battery, a
switched load, or a cable leaving the box:

1. **Power budget**: per rail, the source and its rating, each load's typical and peak
   current, the simultaneous peak, the margin, and the dissipation that went into
   `heat_w`.
2. **Pin map**: MCU pin, function, net, and the §4 check it passed.
3. **Wiring table**: ID, from (module.pin), to, signal, AWG, colour, length,
   termination.
4. **Protection**: fuse rating and position, polarity protection, flyback diodes, bulk
   capacitors, pull-ups, level shifting.
5. **Harness notes**: where each run goes in the box, tie points, service loops, exits,
   assembly order.

For a wiring or power question with no box in it, give this section alone, with the
diagnosis first when something is failing, and don't design an enclosure nobody asked
for. If the user wants a harness drawing, offer WireViz, which renders a YAML list of
connectors, cables and connections to SVG.
