# Materials

Pick the material for the environment and the load, then check that the user's machine can print it. The house default is PETG-CF on an open A1 Mini. Switching material means switching the profile, including whether clips are allowed.

Temperatures below are approximate heat-deflection or softening ranges, not a promise from one spool. Use the spool's data sheet when the user has it. None of these are flame-rated unless the spool says UL94, and none are a food-contact surface: layer grooves hold residue, and a brass nozzle can contaminate the part.

| Material | Softens around | Use it for | Do not use it for |
|---|---|---|---|
| PLA | 55–60 °C | Crisp prototypes, jigs that stay indoors and cool | Cars, windows, anything near a regulator, screws that stay loaded (it creeps), snaps that must survive many cycles |
| PETG | 70–80 °C | Functional parts, the default unfilled plastic when a clip or a tougher layer bond is needed | High heat, fine press fits (it strings and can fuse to smooth PEI) |
| PETG-CF | 75–85 °C | Stiff, stable shells on the house printer | Snaps, hinges, high heat, food, anything that needs toughness. Hardened nozzle only |
| ABS | 95–100 °C | Warm parts, when an enclosed printer and ventilation exist | The open A1 Mini. It warps and the fumes need an enclosure that vents |
| ASA | 95–100 °C | The outdoor version of ABS: heat plus UV | The open A1 Mini, same warp and fume limits |
| PC and PC blends | 110–140 °C | Hot, tough parts | Casual printing. Dry it, enclose the printer, expect warp |
| TPU (95A is the useful default) | stays flexible | Seals, feet, bumpers, grips, button membranes | Fine tolerances, thin tall walls, the house profile's fits. Print slow, little retraction drama if dried |
| Nylon (PA6, PA12) | wide range; dry it | Clips, hinges, wear parts | Printing it wet. It absorbs water, pops, and goes brittle. Dry hotter than PETG, often around 70–80 °C, and keep it dry during the print |
| Higher-temp filled (PA-CF, PET-CF, not PETG-CF) | well above PETG-CF | Stiff hot parts | Open-frame printers. Bambu's own guide puts several of these on enclosed machines only, with a hardened nozzle ≥ 0.4 mm and often 0.6 mm |

Shrinkage is small next to a sane clearance, and it is not uniform. Order of magnitude: PLA and PETG a few tenths of a percent, ABS and ASA closer to 0.5–0.8%. Do not scale a model by a textbook percentage. If a long part must hit a length, measure a bar in that material and compensate that axis.

## How to choose

1. Temperature first. If the part sees more than about 50 °C, PLA is out. More than about 70 °C, house PETG-CF is out.
2. Then toughness. A part that must bend, clip, or survive a drop is unfilled PETG, nylon, or TPU. Fiber fill makes a part stiffer and usually more brittle.
3. Then the machine. ABS, ASA, PC, and the hotter filled nylons want a closed chamber. Recommending them on the A1 Mini without saying the printer is wrong is a bad answer.
4. Then UV and appearance. ASA for sun. PETG-CF hides layer lines and stays matte. PLA looks the cleanest and fails outdoors.
5. Abrasive fills (carbon, glass) require a hardened nozzle. A brass nozzle wears, the orifice grows, and every tuned clearance drifts.

## Drying

Wet filament pops, strings, and bonds poorly between layers. Dry before blaming the model.

| Family | Typical dry |
|---|---|
| PLA | 45–50 °C, several hours, if it has been open a long time. Less hungry than the others |
| PETG, PETG-CF | 65 °C, about 8 h. Bambu's PETG-CF page: 65 °C for 8 h in a blast oven |
| Nylon, PA-CF | Often 70–80 °C or higher. Follow the spool. It will re-absorb water during a long print if left open |
| TPU | Around 50–60 °C. Wet TPU strings heavily |

Store the spool sealed with desiccant after drying. Drying is not permanent.

## Sanding and dust

Sanding carbon- or glass-filled parts makes fine dust. A dust mask, wet sanding, and no casual blowing of the dust around a room. Ordinary PLA and PETG dust is still not something to breathe in volume.
