# User interface, labels, mounting and cables

## Contents
1. Buttons
2. LEDs and light pipes
3. Displays
4. Labels and text
5. Mounting options
6. Feet
7. Cable entry and strain relief

## 1. Buttons

Board-mounted tactile switches (6×6 mm, 12×12 mm) under a wall or lid:

- **Flexure button (one-piece)** — only if the profile allows elastic features
  (not on PETG-CF; use a captured cap there): cut a U-shaped slot (0.8–1.0 mm wide) in the
  lid around a tab ~8–10 mm wide, 12–15 mm long; a plunger under the tab
  reaches to 0.3–0.5 mm above the switch actuator. Print the lid flat so the
  tab bends in XY-layer-friendly direction (it does, when printed flat).
  Works in PETG/ABS; in PLA keep the tab ≥ 12 mm long.
- **Captured cap**: separate button with a flange larger than the hole; hole
  = cap Ø + 0.3–0.4; flange 1 mm wider than the hole all round; stem length =
  distance to the actuator − 0.3. Cap prints separately (top on the bed).
- **Panel-mount button**: round hole per datasheet (e.g. Ø12 / Ø16 / Ø19)
  plus anti-rotation flat or key.
- Measure the gap from lid underside to the switch top; state the assumed
  value — it is the most common mistake.
- Add a recess or raised ring so the button can be found by touch.

## 2. LEDs and light pipes

- 3 mm / 5 mm THT LEDs: hole Ø3.1 / Ø5.1 (press fit) or bezel clip size per
  part.
- SMD LEDs on the board:
  - thin window: leave 0.4–0.6 mm of wall over the LED (white/natural PETG
    glows nicely), or
  - light pipe: a hole down to the LED with a clear filament rod/plug; the pipe
    within 1 mm of the LED; opaque walls around it to stop bleed into
    neighbours.
- Separate multiple indicators with internal ribs (light baffles).
- A small hole (Ø1–2) directly over an LED is simple and readable.

## 3. Displays

- Window = active area + 0.5 mm per side; module outline pocket behind it =
  module PCB + 0.3 mm per side.
- Chamfer the window edges 45° on the outside so the bezel doesn't shadow the
  viewing angle.
- Common modules (measure — clones vary): 0.96" SSD1306 OLED module ≈ 27 ×
  27 mm PCB, active ≈ 22 × 11 mm; 1.3" OLED ≈ 35 × 33 mm PCB; 16×2 LCD
  (1602) ≈ 80 × 36 mm PCB, window ≈ 64.5 × 14.5 mm, holes 75 × 31 mm;
  2.8" SPI TFT (ILI9341) ≈ 86 × 50 mm PCB.
- Hold the display with lid posts that clamp the PCB (0.2 mm interference) or
  screw bosses matching its holes; the glass must not carry load.
- Optional protective window: acrylic sheet in a 0.3 mm clearance pocket.
- Touchscreens: glass flush with or slightly proud of the bezel.

## 4. Labels and text

- Library: `label_cut()` (deboss) and `label()` (emboss). Default font
  "Liberation Sans:style=Bold" (bundled with OpenSCAD).
- Always deboss part name + version (0.4 mm deep, ≥ 3 mm tall) on an internal face
  — the template puts it on the base floor (not with keyholes, which use that floor).
  Future-you will hold three similar lids.
- Top faces (lid printed face-down on the bed): **debossed** 0.4–0.6 mm
  reads well and needs no supports. Model text on the outer face in the
  assembled frame; the print flip is a rotation, not a mirror, so the text
  still reads correctly. Never `mirror()` text.
- Side walls: emboss 0.6 mm or deboss 0.6 mm; size ≥ 5 mm, bold.
- Two-color: raise labels by a multiple of layer height and tell the user
  the Z to switch filament, or split into a separate body for multi-material
  printers.
- Port labels ("USB", "5V", "RESET") next to cutouts help users; icons can be
  simple 2D shapes or imported SVGs.

## 5. Mounting

| Method | Geometry |
|---|---|
| Keyhole slots (wall hanging) | Ø7 head / Ø3.5 slot / 8 mm travel for small screws (Ø8.5 / 4.5 / 9 for M4/#8); head space behind the slot; two slots ≥ 60 mm apart, level. Template `wall_mount = "keyholes"`: on the board centreline with the slot toward the back face, so the front face and its cables hang down; it raises the standoffs so the heads fit under the board, skips the floor label, and asserts clearance to the standoffs. Needs a board ≥ ~30 mm deep — on smaller boards use ears |
| Mounting ears/tabs | tabs extending from the base, hole Ø4.5 (M4/#8 screws), tab thickness ≥ 3 mm with a gusset or root fillet. Template `wall_mount = "ears"`: 4 mm tabs on both X ends at floor level, countersunk, filleted to the wall; nothing inside can collide with them, so it is the safe choice for small boxes |
| DIN rail (TS35) | clip gripping a 35 mm rail, 7.5 mm (or 15 mm) deep, 1 mm flanges; one fixed hook + one flexible snap side; print with the snap in XY |
| Magnets | pockets per closures reference; for steel surfaces (fridges, cabinets) |
| Zip-tie slots | two slots 5 × 2 mm on the base, ≥ 10 mm apart |
| Camera thread 1/4"-20 | hex nut trap 11.2 mm AF (7/16") + Ø6.5 hole |
| GoPro-style | 3 prongs 3 mm thick with 3.2 mm gaps, Ø5 hole — copy exact dims if the user needs compatibility |
| Screw-in through the floor | countersunk holes accessible with the board removed or a hole through the board clearance |
| Adhesive (VHB) | flat recessed pad on the base, 0.3 mm deep |

## 6. Feet

- Rubber bumpon recesses: default Ø8 × 1 mm (template `feet`); generally Ø+0.5,
  depth ≈ ⅓ of bumpon height.
- Printed TPU feet or 1 mm raised pads at the corners.
- If the base has bottom vents, feet must lift it ≥ 5 mm.

## 7. Cable entry and strain relief

- Notch split between base and lid (half-circle in each) for a cable that
  shouldn't be unplugged; Ø = cable + 0.3.
- Strain relief: a zig-zag channel, a zip-tie anchor bar inside, or a clamp
  plate with two screws. The connector on the board must not carry the pull.
- TPU grommet in a round hole for a clean look.
- Cable glands for sealed boxes (environment-and-safety.md §4).
- Leave room inside for connector bodies and wire bend radius (≥ 5 × cable Ø).
