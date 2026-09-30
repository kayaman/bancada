# Environment, thermal and safety

## Contents
1. Heat: estimate before you seal
2. Ventilation design
3. Sensors that sample the outside
4. Dust, water, sealing (IP)
5. Outdoor and UV
6. RF and antennas
7. Batteries
8. Mains voltage
9. Children and pets

## 1. Heat

Rough dissipation (steady state):

| Source | Typical |
|---|---|
| Arduino Uno/Nano idle | 0.2–0.3 W |
| ESP32 with Wi-Fi active | 0.3–0.8 W average (peaks ~1 W) |
| ESP32 deep-sleeping node | ≈ 0 |
| Raspberry Pi Zero 2 W | 0.6–2 W |
| Raspberry Pi 4 | 3–6 W |
| Raspberry Pi 5 | 4–10+ W — needs a fan/active cooler |
| Linear regulator | (Vin − Vout) × I, e.g. 12→5 V at 0.5 A = 3.5 W |
| Motor drivers, LED strips | per datasheet — often the dominant load |

Sealed box temperature rise ≈ P / (h · A), with h ≈ 5–10 W/m²K for natural
convection on plastic and A = outer surface area in m². Example: 1 W in a
100 × 70 × 40 mm box (A ≈ 0.028 m²) → ΔT ≈ 4–7 K. 5 W in the same box → 20–35 K:
vent it or add a fan.

If internal temperature can exceed the material's softening point minus a
margin (PLA ~50 °C!), switch material or ventilate.

## 2. Ventilation

- Needed above ~0.3 W or when the brief names a heat source (template `vents = "auto"`;
  the vent faces default to walls without connectors, and an assert stops a vent band
  that would overlap a cutout).
- **Chimney**: inlets in the lowest 25 % of one wall, outlets in the top 25 % of the
  opposite wall or the lid, so air crosses the heat source.
- Outlet area ≥ inlet area; above 1 W aim for ≥ 25 % open area on the vented wall,
  or add a fan.
- Slots 1.5–2.5 mm wide, ≤ 25 mm long, pitch ≥ slot + one wall (webs are structure).
  ≤ 1 mm (or mesh/fabric behind) keeps insects and most dust out.
- Slots on vertical walls print cleanly with rounded tops (library
  `slot_pattern_2d`). Honeycomb (`hex_pattern_2d`, pointy-top) works on walls
  and lids.
- Rain/splash: vents on the underside or behind downward louvers (45°), never
  on top.
- Fans: 25/30/40 mm fans use 20/24/32 mm hole spacing (M2.5/M3 self-tap into
  plastic); cut a round grille, not a solid hole; mount so air exits past the
  hottest part. Leave ≥ 5 mm plenum in front of the fan.

## 3. Sensors that sample the outside

Per-sensor geometry (T/RH, PIR, ToF, microphones, light/UV, gas, camera, IMU, load
cells, pressure, particulate, ultrasonic) is in `sensors.md`. The rule that matters
most: own chamber, ≥ 15 mm from heat, vented bottom and top.

## 4. Dust, water, sealing

FDM parts are not inherently watertight (layer gaps, porous seams). For splash
resistance:
- Walls ≥ 3 perimeters; slightly higher flow; or a coating (epoxy, spray) on
  the inside.
- Gasket in the lid joint: either a printed **TPU gasket**, or an **O-ring /
  cord** in a groove. Groove depth ≈ 75–80 % of cord Ø (20–25 % compression),
  groove width ≈ 1.3–1.5 × cord Ø. Screws around the perimeter ≤ 50 mm apart.
- Tongue-and-groove joint + gasket beats a flat joint.
- Cable entry: cable glands (PG7 ≈ Ø12.5 mm hole, M12 ≈ Ø12.2–12.5; PG9 ≈
  Ø15.2) instead of open notches.
- Buttons: TPU membrane or IP-rated panel switches.
- Condensation: a small drain/breather hole (Ø2) at the lowest point or a
  vent membrane; conformal-coat the board.
- Be honest about IP claims — a printed box with a gasket may be "splash
  resistant"; don't promise IP67 without testing.

## 5. Outdoor and UV

- ASA (best), or PETG with UV-resistant paint. Not PLA (creeps, softens in the
  sun, degrades).
- Light colors run cooler in direct sun; a sun shade/roof over the lid helps a
  lot for sensors.
- Stainless steel screws; brass inserts are fine.
- Mounting ears/brackets sized for wind load; drip edge on the lid.

## 6. RF and antennas

- Keep screws, inserts, magnets, batteries and carbon/metal-filled filaments
  outside the profile's antenna keepout (15 mm generic, 20 mm PETG-CF) of PCB antennas; antenna end near a plastic wall.
- No metallic paint or foil shielding near the antenna.
- For range-critical nodes, use an external antenna through an SMA bulkhead
  (Ø6.5 hole, flat if keyed).

## 7. Batteries

| Battery | Space (approx) | Notes |
|---|---|---|
| LiPo pouch | measured size + 10 % on thickness | allow swelling, no pressure, no sharp edges nearby, wires strain-relieved; must have a protection circuit |
| 18650 | Ø18.5 × 65 (69 with button top) | use a commercial holder; add holder size, not cell size |
| AA / AAA | Ø14.5 × 50.5 / Ø10.5 × 44.5 | use holders or springs; +1 mm length per cell for springs |
| CR2032 | Ø20 × 3.2 | coin-cell holders |

- Battery doors that are easy to open are fine for adults; see §9 for
  children/pets.
- Charging circuits (TP4056 etc.) get warm — ventilate, don't sandwich them
  against the cell.
- Never fully seal a lithium cell airtight in a tiny volume; if it vents,
  gas must escape.

## 8. Mains voltage (110–240 V AC)

Tell the user clearly: common filaments are not flame-rated, printed walls may
have gaps, filled grades may conduct, and creepage/clearance rules apply (≥ 3 mm
from any mains/HV net to the wall; never rest a bare conductor on plastic). The safe path is a certified
AC/DC module or wall adapter so the printed enclosure only ever sees low
voltage. If the user insists on mains inside: separate compartment with an
internal wall, strain-relieved cable with a proper gland, no exposed metal
(or grounded), flame-retardant material (V-0 rated filament), fuse, and
qualified review. Don't present a printed mains enclosure as safe.

## 9. Children and pets

For toys, pet devices, and anything left within reach:
- No small parts that can come loose (choking): captive screws, no glued-on
  small pieces, fully enclosed magnets.
- **Button/coin cells**: compartment must need a tool (screw) to open;
  many jurisdictions regulate this (e.g. the US Reese's Law). Recommend it for
  any battery type in these products.
- No opening larger than 4 mm in any direction (vents included), and none
  directly above the PCB — use offset louvres so there is no straight line from
  outside to electronics. Template `child_pet_safe` caps slot width and length at
  4 mm; the louvres are yours to add. Say that connector openings are the exception
  and should face away from reach.
- Round all exposed edges (r ≥ 1 mm), no pinch points in moving parts.
- Chewing: PETG/nylon/TPU survive better than PLA; thick walls (≥ 2.4 mm);
  no loose cables reachable — route them internally or armor them.
- Pets: keep LEDs/lasers below safe levels (don't design laser pointers aimed
  at eye level), and avoid toxic coatings; say that FDM surfaces harbor
  bacteria if the device touches food or mouths.
- Motors: guard gears and propellers; fully enclosed drivetrains.
