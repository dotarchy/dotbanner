---
status: Accepted
date: 2026-08-31
deciders:
  - aaronsb
related:
  - ADR-201
  - ADR-400
  - ADR-401
---

# ADR-402: Two-color cells — foreground glyph over composited background

## Context

With per-region registers in place (ADR-201), a braille cast and a block
body could share a grid but not a *cell*: whichever layer covered more
pixels took the cell entirely and the other vanished there. A glow died
wherever the letterform sat on top of it.

A terminal cell carries two colors, not one. Classic ANSI art built its
entire depth vocabulary on exactly that — a small CP437 block repertoire
plus a foreground and a background per cell. dotbanner has the same two
colors available, with the full Unicode glyph repertoire and 24-bit color
instead of sixteen.

## Decision

A `Cell` carries `ch`, `fg`, and `bg`, and layer composition fills all
three:

- **The winner draws.** The layer covering the most pixels in a cell
  contributes the glyph (in its own register) and the foreground color.
  Ties go to the later layer, preserving draw order.
- **The runner-up backs it.** The next-best layer paints the cell
  background, so two layers share the cell instead of one erasing the
  other. A layer that loses every cell it touches still contributes color.
- **`on_top` inverts the contest.** A layer marked so takes the glyph
  outright regardless of coverage and demotes the coverage winner to the
  background. A braille layer marked `on_top` stipples its dots *over* a
  solid body, the body's own color showing through as the ground.
- **A new `edge` region** spans both sides of the letterform boundary
  (`outer` pixels beyond, `inner` pixels within), so a paint can fade
  outward into the ground and inward into the body in one op.

Determinism is unaffected: coverage counts and draw order are exact
integers, and the two-color result is a pure function of the layer set
(ADR-400).

## Consequences

### Positive

- The ANSI-art technique — glyph plus two colors per cell — is available
  with the whole Unicode repertoire behind it.
- Low-contrast pairings (near-identical fg and bg) read as texture rather
  than as a second shape, a register the single-color model could not
  express at all.
- Shadows, glows and halos survive overlap with the body.

### Negative

- Only two layers can be represented in any one cell; a third contending
  layer is dropped there.
- Backgrounds paint the full cell rectangle, so a cast's background can
  square off the silhouette where the glyph is sparse.

### Neutral

- Font sinks (.tlf/.flf) carry no background, so they take the foreground
  only; the cfonts sink can map fg/bg onto its color slots.
- ADR-401's smoothing pass reads the same per-cell coverage counts this
  composition already computes.

## Alternatives Considered

- **Blend the two layers' colors into one foreground** — rejected: loses
  the glyph distinction that makes a braille overlay legible as texture.
- **Composite in image space before symbolizing** — rejected: the two
  layers must keep separate registers, which only exists in cell space.
- **Let every layer own its own grid, composited at the sink** — rejected:
  that is the render-twice-merge ADR-400 removed.
