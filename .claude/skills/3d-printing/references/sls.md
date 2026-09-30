# SLS and MJF

Powder-bed nylon (SLS, and the closely related MJF service) is the process for parts with internal channels, print-in-place mechanisms, and shapes that have no legal FDM orientation. It is usually a bureau, not the user's A1 Mini. Say that when you recommend it.

Baseline numbers are Formlabs Fuse with Nylon 12, from the Fuse Series design guide. A general bureau often wants slightly thicker walls (Protolabs publishes about 0.8–1.0 mm). Use the "bureau" column unless the user named a Fuse.

| Feature | Fuse Nylon 12 | Safer bureau default |
|---|---|---|
| Wall | 0.6 mm vertical, 0.3 mm horizontal | 1.0 mm, 1.5 mm if it is structural |
| Pin | 0.8 mm | 1.0 mm, filleted where it meets the body |
| Hole | 1.0 mm, and thick walls make holes less accurate | 1.5 mm, or a pilot to ream |
| Escape holes | ≥ 3.5 mm, at least two per cavity | Same. Bigger and more of them. Line of sight for the blast |
| Integrated mechanism clearance | 0.3 mm under 20 mm², 0.6 mm above | 0.5 mm per side if the user cannot iterate with the bureau |
| Gap between separate parts in the build | ≥ 1 mm, 5 mm recommended | Leave packing to the bureau |

## Rules that matter more than the minimums

- Unsintered powder is trapped in every closed void. Two escape holes, large enough to flush, placed so powder and blast media can actually leave. One hole makes a cavity that will not empty. Blind screw holes get a small continuation through the bottom, or they pack solid.
- Keep wall thickness even, roughly in the 1–3 mm band for production-like parts. A thick block next to a thin wall cools at different rates and warps. Hollow the thick region instead of leaving a 10 mm slab.
- SLS nylon is not injection-molded nylon. Elongation is much lower. A living hinge that would work in molded PP is not a default SLS feature. Ask the bureau which powder if the part must flex a lot.
- Holes in thick sections shrink more ("hoop shrink"). Design them slightly large or ream them. Do not trust a press fit for a bearing until a sample exists.
- There are no dedicated supports, which is the point. There is still heat, so a long thin wall can warp during cooldown. Shorten it, thicken it, or rib it.
- Glass-filled powders leave a tougher skin and need larger escape holes and line of sight to clean.

## Fits

Printed-together mechanisms need the integrated clearance above or they sinter into one piece. Assembled-after printing can be tighter, but powder still leaves a grainy surface, so a sliding fit wants more clearance than a machined nylon part, not less.

Threads: coarse threads print. Fine metal-screw threads should be a heat-set or a self-tapping insert specified by the bureau, or a tapped hole in a boss with enough wall (on the order of 2 mm of material around the insert, same idea as FDM).

## When not to bother

A bracket the A1 Mini can print this afternoon. SLS wins on geometry that FDM cannot orient, on batches of complex parts, and on nylon toughness without supports. It loses on price and turnaround for a one-off jig.
