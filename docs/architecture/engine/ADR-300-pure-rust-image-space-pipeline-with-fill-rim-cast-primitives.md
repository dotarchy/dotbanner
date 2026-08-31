---
status: Proposed
date: 2026-08-31
deciders:
  - aaronsb
related:
  - ADR-200
  - ADR-400
---

# ADR-300: Pure-Rust image-space pipeline with fill/rim/cast primitives

## Context

The bash prototype leans on four runtime tools: ImageMagick (rasterize +
morphology + composite), chafa (pixels → cells), toilet (render baked fonts),
python3 (cell merges, cfonts JSON). Effect research showed every style built
so far decomposes into three image-space primitives applied to the glyph mask:

- **fill** — what paints the body (flat split, banded gradient, scanline,
  shade/bevel)
- **rim** — treatment of an eroded-edge region (trap, neon tube, sticker
  outline)
- **cast** — what the glyph throws (drop shadow, glow halo, extrusion)

## Decision

The engine is pure Rust, and effects are compositions of the three primitives
rather than a flat style list:

- **Font discovery**: `fontdb` (replaces fc-match).
- **Rasterization**: `ab_glyph` (replaces `magick label:`).
- **Morphology, blur, gradients, composites**: `image` + `imageproc`
  (replaces the rest of ImageMagick).
- **Pipeline ops are recipe nodes** (ADR-200): `mask`, `fill`, `rim`, `cast`,
  each with a `kind` and parameters. Named styles (`chrome`, `neon`,
  `extrude`) are shipped recipes, not code paths.
- Everything upstream of the symbolizer operates on images; the symbolizer
  (ADR-400) is the single crossing into character space.

## Consequences

### Positive

- Single static binary; `cargo install dotbanner` is the whole setup.
- New effects are recipe compositions first, new primitive kinds second, new
  primitives rarely — the blast radius shrinks in that order.

### Negative

- Reimplementing morphology/composite behavior ImageMagick gave us free;
  prototype parity (erode/difference trap, banded gradients, blur glow,
  stepped extrusion) must be verified against the bash renders.

### Neutral

- Animation (rolled gradients) is a frame loop over the same pipeline; it
  lives in the recipe's `animate` block, not in the engine's model.

## Alternatives Considered

- **Shell out to magick/chafa from Rust** — rejected: keeps the dependency
  problem the rewrite exists to remove.
- **Flat style enum** (prototype model) — rejected: the seven researched
  effects already repeat the three primitives; composition is the smaller
  surface.
