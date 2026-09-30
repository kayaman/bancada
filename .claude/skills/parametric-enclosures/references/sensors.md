# Sensor compartment rules

A sensor in the wrong chamber produces confident, wrong readings — the worst failure
mode, because nothing looks broken. Read the entry for every sensor in the brief.

Contents:
- [Temperature / humidity](#temperature--humidity)
- [PIR motion](#pir-motion)
- [ToF and IR proximity](#tof-and-ir-proximity)
- [Microphone](#microphone)
- [Ambient light and UV](#ambient-light-and-uv)
- [Gas, VOC, CO2](#gas-voc-co2)
- [Camera](#camera)
- [Accelerometer / IMU](#accelerometer--imu)
- [Load cell / strain](#load-cell--strain)
- [Barometric pressure](#barometric-pressure)
- [Particulate matter](#particulate-matter)
- [Ultrasonic ranging](#ultrasonic-ranging)
- [Using the template's chamber](#using-the-templates-chamber)

---

## Temperature / humidity

BME280, BMP280, SHT3x/SHT4x, DHT22, AHT20, DS18B20.

The enemy is self-heating. A regulator, an ESP32 in Wi-Fi TX, or an LED driver will
raise a nearby sensor several degrees and the reading will look plausible.

- Own chamber, full-height divider, ≥ 15 mm from any heat source.
- Divider must not conduct: either a ≥ 3 mm air gap behind it or a slot cut through it.
- **Vent at the bottom and at the top of the chamber.** A chamber vented only at the top
  traps a warm column and measures itself. Bottom-in, top-out gives convection that
  tracks ambient.
- Sensor aperture faces the airflow path, not a wall.
- No dead pockets — round or chamfer the chamber corners so air does not stall.
- Keep the chamber out of direct sunlight, and behind an opaque wall if the enclosure is
  translucent.
- Humidity sensors need the same airflow plus no condensation trap: no horizontal ledge
  under the sensor where water can pool.
- DS18B20 in a probe body: strain-relieve the cable, and thermally isolate the entry
  point so the cable does not conduct enclosure heat to the probe.

## PIR motion

HC-SR501, AM312, and bare fresnel-lens modules.

- The lens sits **flush** in a round cutout of Ø lens + 0.3 mm. Never recess a fresnel
  lens behind a wall — the wall clips the detection cone and the sensor goes half-blind
  in a way that only shows up as "it sometimes misses people".
- Light-tight collar behind the lens so internal LEDs cannot reach the element.
- The entire detection cone must be unobstructed — check it against the module's stated
  angle, not the lens diameter.
- Thermal isolation matters here too: a warm body inside the enclosure drifting across
  the element causes false triggers. Keep it away from regulators and give it its own
  chamber.
- No airflow across the element. This one wants to be sealed, unlike the T/RH sensors.

## ToF and IR proximity

VL53L0X/L1X, VL6180X, APDS-9960, IR reflective pairs.

- Open aperture sized from the datasheet field of view:
  `aperture ≥ 2 × depth × tan(FoV / 2)`, where `depth` is the wall thickness plus any
  standoff between sensor face and aperture. Undersizing this clips the FoV and shortens
  usable range.
- **Opaque divider between emitter and receiver**, running right up to the wall.
  Internal crosstalk is the number-one cause of a ToF sensor reporting a constant short
  distance.
- No window material unless the datasheet explicitly qualifies one — printed plastic,
  even thin, scatters IR unpredictably. Leave the aperture open.
- Matte black internal surfaces around the aperture if the user can print in black;
  otherwise note it.

## Microphone

MEMS (INMP441, SPH0645, ICS-43434) and electret.

- Port Ø 1.0–1.5 mm, port length ≤ 2 × Ø. A long narrow port is a Helmholtz resonator
  and will colour the response.
- Gasket recess against the PCB around the port so sound arrives through the port and
  not through the enclosure cavity.
- Keep the port away from fans, vents and anything that vibrates.
- MEMS mics are bottom- or top-ported — check which, because the port must align with
  the correct face of the package.

## Ambient light and UV

BH1750, TSL2591, VEML6075.

- Window or open aperture, plus an internal baffle so status LEDs cannot bleed into the
  sensor. LED bleed is the standard failure: the lamp turns on, the sensor sees light,
  the lamp turns off.
- If a window is printed, note that layer lines diffuse and attenuate; a flat acrylic
  disc in a 1 mm recess is usually better and worth calling out.
- UV sensors need a UV-transmissive window — most plastics block it. Say so rather than
  designing a window that guarantees a zero reading.

## Gas, VOC, CO2

MQ-x, SGP30/SGP40, SCD4x, MH-Z19.

- Generous airflow, own chamber, same bottom-in/top-out convection as T/RH.
- **Far from any hot component** — these are temperature-compensated and self-heating
  corrupts the compensation.
- MQ-x sensors have their own heater (hundreds of mW). Give that heater its own vented
  chamber and treat it as a heat source for everything else in the enclosure.
- No adhesives, no fresh silicone, no solvent near VOC sensors — outgassing poisons the
  reading for weeks. Note this in the build notes.
- NDIR CO2 (SCD4x, MH-Z19) needs diffusion, not draught: vents, but not in a direct
  wind path.

## Camera

OV2640, IMX219, and similar modules.

- Aperture sized from the lens FoV with margin; an undersized hole vignettes the corners.
- Recess the lens barrel so the wall does not sit in the light path, and add a short
  internal shroud to kill reflections off the inside of the enclosure.
- Lens must be positionable for focus — either a clamp or a boss the module screws to,
  not a pocket it is trapped in.
- Keep the IR LEDs (if any) optically separated from the lens or you get a white haze.

## Accelerometer / IMU

MPU6050, BMI160, LSM6DS3.

- Rigid mount. The IMU must be mechanically coupled to the enclosure, not floating on a
  flexing board — an unsupported PCB centre turns every bump into a resonance.
- Add a standoff directly under the IMU footprint if the PCB spans more than ~50 mm.
- Document the sensor axes relative to the enclosure in the build notes; getting this
  wrong is a firmware bug that looks like a hardware bug.

## Load cell / strain

- The load path must go through metal, never through printed plastic in the measurement
  path — PETG-CF creeps under sustained load and the zero drifts over hours.
- The enclosure holds and protects; it does not carry the measured force.
- Provide a defined mechanical ground: a flat, bolted mounting face, not feet.

## Barometric pressure

BMP280/BMP388/LPS22 (and the pressure half of a BME280).

- A small vent is enough; pressure equalises through any gap. Keep wind from blowing
  straight onto the port (gusts read as pressure noise).
- Sealed enclosures need a breather or vent membrane, otherwise the sensor measures
  the box, which also swells and shrinks with temperature.

## Particulate matter

PMS5003/7003, SPS30, SDS011.

- These have a fan and defined inlet/outlet ports. Align enclosure openings with both,
  don't block either, and keep inlet and outlet apart so exhaust isn't re-ingested.
- Mount the sensor with the manufacturer's recommended orientation (usually ports
  vertical) and no gap between port and enclosure opening — a short duct or gasket.
- The fan is also a (small) vibration and heat source for neighbouring sensors.

## Ultrasonic ranging

HC-SR04, JSN-SR04T.

- Two round holes for the transducers (≈ Ø16 mm on HC-SR04 — measure the centre
  spacing), transducer faces flush with or slightly proud of the outer surface.
- A recessed transducer sees the hole's edge as an echo at very short range.
- Soft mount (TPU ring) to stop the enclosure ringing into the receiver.

## Using the template's chamber

`sensor_chamber = true` adds a two-skin divider (air gap `chamber_gap`, a wire notch at
the top) and a chamber of `chamber_len` at the +X end, vented low and high on the +X
face. No connector may face +X (turn the board with `pcb_rot`), and the main cavity's
chimney moves to the free faces. List the heat sources in `heat_sources` (board
coordinates) so the 15 mm rule is asserted. Sensors that must be sealed (PIR) or aimed
(ToF, camera) don't use this chamber — build their aperture per the sections above.
