---
status: Accepted
date: 2026-08-31
deciders:
  - aaronsb
related:
  - ADR-200
  - ADR-300
  - ADR-400
  - ADR-401
---

# ADR-201: Pipeline op vocabulary — fill, rim, cast with paints and registers

## Context

The first pipeline implementation gave `fill` a full paint (solid or banded
gradient) but limited `rim` to a flat color, and offered no way to paint
anything outside the glyph. Two capabilities were missing:

- A trapped rim could not carry a gradient, so body and rim could not both
  be styled — the trap was a color accent rather than a design element.
- Drop shadows and glows, which the bash prototype produced by compositing a
  braille render behind a block render, had no representation at all.

ADR-400 named per-region symbol registers as the design center that would
replace that render-twice-merge, but nothing exercised it.

## Decision

Three ops, uniform in shape: each names a **region**, a **paint**, and
optionally a **register**.

| Op | Region | Derived by |
|----|--------|-----------|
| `fill` | the glyph body | the mask itself |
| `rim` | an inner edge band | mask minus erode(mask, width) |
| `cast` | an outer band or offset shape | dilate(mask, spread), offset by (dx, dy), minus the mask |

- **Every op takes a `Fill`** — solid or multi-stop gradient, banded or
  smooth. Rim gradients and gradient glows follow with no new machinery.
- **Every op may name a `register`** (`blocks` or `braille`). A cast in
  braille against a block body is the prototype's drop shadow, produced in
  one pass instead of two renders merged in Python (tech-debt TD-3).
- **Registers share one cell footprint.** The mask rasterizes at braille
  resolution — 2×4 pixels per output cell — and block layers downsample into
  the same grid. Without a shared footprint, mixed registers could not
  coexist in one cell grid at all.

This supersedes the initial `rim: { erode, color }` shape from ADR-200's
sketch. Recipes are pre-release and not yet circulating, so no migration is
owed; the compatibility rule takes effect from the first tagged release.

## Consequences

### Positive

- Body, rim, and cast are stylable with the same expressive paint, so the
  effects catalog researched in the prototype (chrome, neon, sticker,
  extrude, glow) becomes recipe composition rather than new code.
- The braille-behind-blocks shadow is native, and TD-3 can retire.

### Negative

- Rasterizing at 2×4 always costs ~2× the pixels for block-only renders.
- Three ops with a shared shape is a wider schema surface to keep
  compatible once recipes circulate.

### Neutral

- Cast's offset makes extrusion expressible as repeated casts; whether to
  add a dedicated op for it is left open.
- The coverage counts these ops produce per cell are the same signal
  ADR-401's smoothing pass needs.

## Alternatives Considered

- **Keep rim flat-colored, add a separate `gradient-rim` op** — rejected:
  duplicates the paint vocabulary per region.
- **Separate cell grids per register, composited afterward** — rejected:
  that is the render-twice-merge ADR-400 set out to eliminate.
- **Cast as a post-symbolizer cell effect** — rejected: shadows are a shape
  derived from the mask, and shape work belongs in image space (ADR-300).
