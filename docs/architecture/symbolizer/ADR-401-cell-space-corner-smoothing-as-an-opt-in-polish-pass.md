---
status: Draft
date: 2026-08-31
deciders:
  - aaronsb
related:
  - ADR-400
  - ADR-200
---

# ADR-401: Cell-space corner smoothing as an opt-in polish pass

## Context

Coverage mapping (ADR-400) quantizes diagonal glyph edges into staircases, and
fine typographic detail — serifs, thin strokes, small counters — aliases away
entirely below one cell. The bash prototype showed both: heavy weights lost
their counters at 8 rows, and every slope rendered as steps.

MLAA (morphological antialiasing) addresses the same problem in pixel
rendering by classifying edge discontinuity patterns (L/Z/U shapes) and
substituting blended coverage. The terminal-cell analog substitutes *slope
glyphs* instead of blends. Unicode offers two repertoires:

- **Safe**: `◢ ◣ ◤ ◥` triangles plus the existing quads — near-universal
  font coverage.
- **Extended**: Legacy Computing smooth mosaics (U+1FB3C–1FB6F) — many slope
  angles, patchy font coverage.

Whether a smoothed result *fits* — especially combined with color styles — is
a judgment call, and per the prototype sessions the judgment differs per font,
weight, and effect.

## Decision

Smoothing is a **post-symbolizer polish pass**, opt-in per recipe:

- **Deterministic pattern substitution.** The pass reads the mask at sub-cell
  resolution around each staircase cell, classifies the local edge pattern,
  and substitutes from a fixed lookup table. No blending, no randomness —
  ADR-400's determinism invariant holds; this ADR is the gate that clause
  requires.
- **Detail preservation, not only smoothing.** The same sub-cell read lets a
  cell that quantized to empty-or-full recover a slope glyph that encodes its
  true partial coverage — serifs and thin diagonals survive that would
  otherwise vanish.
- **Tiered repertoires** in the recipe: `polish: { smooth: "safe" | "extended" }`
  (absent = off). The extended tier's font-coverage risk is the user's
  knowing choice.
- **The human judges fit in the TUI.** Polish toggles live in the preview
  loop with instant before/after; the accepted combination saves into the
  recipe like every other choice (ADR-200).

## Consequences

### Positive

- Baked fonts gain a quality tier no existing figlet-family generator offers.
- Fine details survive low row counts, widening the usable font range.

### Negative

- The substitution table is a new correctness surface; golden-file tests
  must cover each classified pattern.
- Extended-tier output renders as tofu in terminals whose fonts lack Legacy
  Computing coverage.

### Neutral

- Colored output must carry the substituted cell's foreground unchanged;
  smoothing alters shape only.

## Alternatives Considered

- **Smooth in image space (raster antialiasing before symbolize)** —
  rejected: gray edge pixels die at the bi-level threshold (the prototype's
  mid-gray losses, tech-debt TD-2); the information must cross into cell
  space, not be discarded before it.
- **Full MLAA blending with shade characters (░▒▓)** — deferred: shade
  glyphs read as texture, not edge, at banner sizes.
- **Always-on smoothing** — rejected: fit is subjective per font and effect;
  the recipe records the human's call.
