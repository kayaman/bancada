# Resin (SLA / MSLA)

Use resin when the feature is smaller than one FDM extrusion, the part is small and must look smooth, or a fit must be tighter than desktop FDM can hold. Say so if the user only has the A1 Mini: they would be ordering the part or using another printer.

Consumer MSLA printers (the common LCD machines) are less accurate and more warped on large flat parts than a Formlabs Form 4. The numbers below are Formlabs Form 4 Grey at 50 µm, from Formlabs' design specifications, and they are a best case. On a consumer machine, thicken toward the "use this" column.

| Feature | Form 4 spec | Use this unless they named a Form printer |
|---|---|---|
| Supported or unsupported wall | 0.2 mm | 0.8–1.0 mm. Thin walls warp from peel forces and swell in the wash |
| Hole | 0.5 mm | 0.8 mm, and drill anything that locates a shaft |
| Drain / vent | 0.75 mm | ≥ 3 mm, at least two, on a hollow part |
| Clearance between moving parts | 0.4 mm | 0.5 mm |
| Unsupported overhang length | 5 mm, and they still want supports in practice | Assume supports. Resin does not bridge the way FDM does |
| Embossed detail | 0.1 mm | 0.2 mm, bold type |

## Rules that keep a resin print alive

- Orient the part so cups (concave faces that hold a pool of resin against the film) do not point at the vat. A cup blows out or stalls the peel. Add a drain or tilt the part.
- Hollow thick parts. A solid block wastes resin, holds heat, and traps uncured resin inside. Every cavity needs a drain large enough to flush, not just to drip.
- Support the part. Unsupported islands and long overhangs fail. Keep support tips off faces that must be optically clear or accurately flat; those faces go toward the build plate or get oriented so supports land on a back face.
- Large flat areas parallel to the screen increase peel force. Tilt them 30–45° unless the user is printing a dental-style flat-on-plate workflow on purpose.
- Leave walls thick enough to survive the wash. A long soak in alcohol softens thin sections and they warp before the cure.

## After the print

Wash, then UV-cure per the resin maker. Uncured resin on skin sensitizes people over time; gloves, not bare fingers, and do not pour used wash down a drain. A part that is still tacky is not finished and will not hold a dimension.

Standard hobby resins are brittle. A snap fit, a press-in bearing, or a screw boss that would be marginal in PLA will crack in basic resin. Tough or ABS-like resins exist; name that requirement instead of assuming the default grey resin can take a load.

Resin threads and inserts: small machine threads (around M3 and up) can work if the hole is accurate and the resin is tough. Heat-set inserts need a resin that actually melts and grips; many do not. A nut trap or a clearance hole for a screw is the safer joint.

## When to stay with FDM

Anything bigger than the resin build volume, anything that must be tough, a jig that will be dropped, and any part the user intends to print today on the A1 Mini. Do not send a 150 mm bracket to resin because the FDM version needed a chamfer.
