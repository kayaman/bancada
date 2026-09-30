# PCBs, boards and connector cutouts

## Contents
1. Getting the board geometry
2. Vertical stack-up
3. Holding the board
4. Known board footprints
5. Connector cutouts (and the USB plug trap)
6. Antenna and keepouts

## 1. Getting the board geometry

In order of reliability:
1. The user's own measurements or KiCad/EasyEDA files. From KiCad, export the
   Edge.Cuts layer as DXF/SVG (1:1, mm) and `import()` it for the outline;
   export the 3D board as STL and show it with `%import("board.stl")` for fit.
2. The manufacturer's mechanical drawing (Raspberry Pi, Arduino and Espressif
   publish them).
3. The table in §4 — good starting values; clones differ. Always tag them
   `// ASSUMPTION: verify with calipers`.

What you need: outline X×Y, thickness, hole Ø and centers (from the
bottom-left corner, top view), connector positions/sizes/overhang past the
edge, tallest part above and below.

## 2. Vertical stack-up (base floor upward)

```
floor_t                     profile value (3.0 on PETG-CF; ≥ 1.2 generic)
standoff_h                  ≥ 4 mm (solder tails); template: max(standoff_min, comp_bot + 1,
                            comp_bot + 3.5 with keyholes)
pcb_t                       1.6 typical (1.0 Pico, ~1.4 Raspberry Pi)
comp_top                    measure; + 2 mm headroom (+ wire bend room if cables plug in from above)
lid split                   template: the higher of the component stack and the tallest
                            cutout (with its lead-in and roof) + 1 mm
lid                         ceil_t + tongue/groove
```

Headers with Dupont wires plugged from above need ~15–20 mm above the pins.

## 3. Holding the board

| Method | When | Notes |
|---|---|---|
| Standoffs + screws | board has holes | default; M2.5 for Raspberry Pi, M3 for most others; self-tap pilot 2.1 (M2.5) / 2.4 (M3) |
| Standoffs + heat-set inserts | board removed often | insert hole per vendor spec; standoff Ø ≥ insert hole + 3 |
| Pins (locating) + lid posts | quick, no screws | pin Ø = hole − 0.3; lid posts press the board with 0.2 mm interference |
| Floor rails + stops | board without holes (ESP32 DevKit, XIAO), top-loading box | the template's default when `mount_holes = []`: rails under the front/back edges at standoff height, a stop on each side that has a column zone; the lid limits lift to its headroom. Move or drop a rail (`rail_w`) where header pins run under the edge |
| Card-guide slots | board without holes, box that opens at one end | slot width = pcb_t + 0.3; rails 2 mm deep; a stop at the end |
| Snap clips over the board edge | holes absent, board rarely removed | see closures reference for strain |
| Header sockets on a carrier board | dev boards | the carrier gets the holes |

Keep standoff diameter clear of pads and components near the holes (check
keepout rings on the board; typically Ø 5–6 mm is safe for M3 holes).

## 4. Known board footprints (verify before printing)

Coordinates: origin at the bottom-left corner of the board in top view.

| Board | Outline (mm) | Mounting holes | Notes |
|---|---|---|---|
| Arduino Uno R3 / R4 | 68.6 × 53.3 | Ø3.2 at (14.0, 2.5), (15.3, 50.7), (66.1, 7.6), (66.1, 35.5) | USB and DC jack on the x = 0 edge and overhang it by ~6 mm; R3 USB-B is ~12 × 11 mm tall, R4 uses USB-C |
| Arduino Nano (classic/clones) | 18 × 45 | four small holes (~Ø1.8) near the corners | usually held by headers or rails; USB on a short edge (Mini-USB, micro or USB-C by clone) |
| Arduino Nano ESP32 / Every | 18 × 45 | as Nano | USB-C/micro on a short edge |
| ESP32 DevKitC (38-pin) | ≈ 54–55 × 28 | none | antenna at the short end opposite the USB; clones vary (48–58 × 25–28) |
| ESP32 30-pin DevKit clones | ≈ 51–52 × 28 | none (some have 4 × Ø2.5) | measure |
| Wemos D1 mini | 34.2 × 25.6 | none | USB on a short edge |
| Seeed XIAO (all variants) | 21 × 17.8 | none | USB-C on the short edge; castellated |
| Raspberry Pi 4 B / 3 B+ / 5 | 85 × 56 | M2.5, Ø2.7 at (3.5, 3.5), (61.5, 3.5), (3.5, 52.5), (61.5, 52.5) | USB/Ethernet stack ~16 mm tall and overhangs the x = 85 edge by ~2–3 mm; Pi 5 needs active cooling |
| Raspberry Pi Zero / Zero 2 W | 65 × 30 | M2.5, Ø2.75 at (3.5, 3.5), (61.5, 3.5), (3.5, 26.5), (61.5, 26.5) | ports on the long edge at y = 0 |
| Raspberry Pi Pico / Pico W / Pico 2 | 51 × 21 × 1.0 | Ø2.1, 47 × 11.4 pattern (2.0 mm from short edges, 4.8 from long edges) | USB on a short edge; Pico W antenna at the opposite end |

## 5. Connector cutouts

Size the opening = connector body + `cutout_clear` (0.5 mm) per side (+0.25 more and a
1 mm 45° lead-in on user-facing ports), corner radius = body radius + clearance.

In the template each cutout is
`[edge, pos, z, w, h, r, user_facing, overmold, overhang]`, all in board coordinates
read straight off the board drawing:

- `edge` is the board edge the connector sits on: `"x0"` (the edge at x = 0), `"x1"`,
  `"y0"` or `"y1"`. `pcb_rot` turns the board, which picks the wall each edge meets.
- `pos` is the connector centre along that edge (board y on an x-edge, x on a y-edge);
  `z` is its centre above the board top.
- `overhang` is how far the body sticks out past the board edge (Uno USB-B ≈ 6.2,
  DC jack ≈ 1.8, Pi 4 USB/Ethernet ≈ 2.5). The template moves the board in by it.

The template then lays the box out from the connectors: a wall that carries a
connector sits `pcb_clear` from the board, and the column zones take the other sides
(both X ends if they are free, else front and back, else every free side). A corner
column exists where at least one of its two sides has a zone, so connectors on two
adjacent walls (Raspberry Pi) leave three columns, and on three or four walls the
assert stops you: bring one connector out through the lid, move it, or place a column
by hand. Vents default to faces without connectors.

Typical bodies (verify):

| Connector | Body opening W × H (mm) | Notes |
|---|---|---|
| USB-C receptacle | 8.9 × 3.3, r 1.6 | plug overmold ~10–13 × 5.5–7 |
| Micro-USB | 7.8 × 3.0 | plug overmold ~10 × 6 |
| Mini-USB | 7.7 × 4.0 | |
| USB-A (single) | 13.2 × 5.8 | |
| USB-B | 12.2 × 11 | |
| DC barrel jack (PCB, 5.5/2.1) | ≈ 9 × 11 | panel-mount versions use a round hole — check the datasheet thread |
| RJ45 | 16 × 13.5 | with LEDs: extend window |
| 3.5 mm audio | Ø6–6.5 | |
| microSD socket | 12 × 2 slot | add a finger notch Ø10 below |
| Toggle / rocker switches, panel pots | per datasheet (e.g. 6 mm or 7 mm shaft holes; 7 mm pots use Ø7.2) | add an anti-rotation tab slot if the part has one |

**The USB plug trap.** The plug's overmold is wider than the socket. If the
socket sits back from the outer surface by more than ~1.5 mm, the overmold
hits the wall and the plug never seats. Fix one of two ways:
1. Place the board so the socket face is within 0.5 mm of the inner wall, and
   keep the wall ≤ 1.5 mm at the socket, or
2. Recess the outer face around the opening to overmold size + tol per side,
   leaving ≤ 1 mm of wall at the socket — the template does this when a cutout's
   `overmold` is set (needed on the 2.52 mm PETG-CF wall).
Say which one you used. The template's walls with connectors already sit
`pcb_clear` from the board edge; the recess handles the thick PETG-CF wall.

Cutouts must not cross the lid split line unless you design them as notches
split between base and lid. The template raises the split above the tallest cutout
(lead-in and roof included) and asserts it.

Openings wider than the profile's bridge limit get a 45° roof (`cutout_roof`), so
their top prints without sagging into the plug's path.

Raspberry Pi 4 B, from drawing RP-008343-DS (verify against the board; heights are
body tops above the PCB):

```
cutouts = [
    ["y0", 11.2, 1.6, 9.0, 3.2, 1.6, true, [12.5, 7.0], 0],  // USB-C power
    ["y0", 26.0, 1.5, 6.5, 3.0, 0.5, true, [11.0, 7.5], 0],  // micro-HDMI 0
    ["y0", 39.5, 1.5, 6.5, 3.0, 0.5, true, [11.0, 7.5], 0],  // micro-HDMI 1
    ["y0", 54.0, 3.0, 6.2, 6.2, 3.1, true, [], 0],           // 3.5 mm AV jack
    ["x1",  9.0, 8.0, 13.5, 16.0, 0.5, true, [], 2.5],       // USB 2.0 stack
    ["x1", 27.0, 8.0, 13.5, 16.0, 0.5, true, [], 2.5],       // USB 3.0 stack
    ["x1", 45.75, 6.75, 16.0, 13.5, 0.5, true, [], 2.5]      // Ethernet
];
```

That puts connectors on two adjacent walls: the template leaves the corner between
them without a column and screws the lid at the other three.

## 6. Antennas and keepouts

- Wi-Fi/BLE modules with PCB antennas (ESP32-WROOM, Pico W, XIAO): place the
  antenna end near a wall, not in a corner packed with parts.
- Keep metal outside the profile's antenna keepout (15 mm generic, 20 mm on PETG-CF):
  screws, heat-set inserts, batteries, magnets. Carbon-fibre/metal-filled filament
  attenuates by itself. The template asserts inserts against `antenna`.
- No metal enclosures; plastic walls ≤ 3 mm are nearly transparent at 2.4 GHz.
- External antenna (u.FL → SMA): SMA bulkhead hole Ø6.5 with a D-flat if the
  connector has one.
