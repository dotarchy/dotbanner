---
status: Proposed
date: 2026-08-31
deciders:
  - aaronsb
related:
  - ADR-300
---

# ADR-400: Own the pixels-to-cells symbolizer instead of wrapping chafa

## Context

chafa served the prototype well, but three costs surfaced. Its terminal
auto-detection emitted sixel through pipes until forced to symbols mode. Its
`-c none` threshold silently dropped mid-gray content (patterned backgrounds
vanished). And mixed-register output — block-glyph body with a braille shadow
— required rendering twice and merging cell grids in Python. It also has no
stable Rust binding.

Our actual use of chafa is narrow: thresholded coverage mapping. Pick, for
each 2×2 (quads/halves) or 2×4 (braille) pixel cell, the symbol whose dot
pattern matches the thresholded pixels, and carry foreground/background color
per cell for ANSI output.

## Decision

Write the symbolizer in `dotbanner-core` (~200 lines of scope):

- **Coverage mapping, not perceptual matching** — threshold the cell, look up
  the symbol by bit pattern. No error diffusion, no work-factor heuristics.
- **Deterministic** — identical input pixels and symbolizer spec produce
  identical cells, on every platform. This is a hard invariant: baked fonts
  and shared recipes must reproduce exactly.
- **Per-region symbol sets** — the recipe's symbolizer spec assigns registers
  (e.g. `body: blocks`, `cast: braille`); regions arrive as separate layers
  from the engine and symbolize in one pass, replacing the render-twice-merge
  dance.
- **Color modes**: none (mono glyph art for .flf/.tlf), indexed slots (cfonts
  `<cN>`), truecolor ANSI (preview, .ans).

Adding any perceptual heuristic later requires a symbolizer-domain ADR,
because it trades away determinism.

## Consequences

### Positive

- The one component with no prior art anywhere becomes ours and testable;
  golden-file tests pin cell output exactly.

### Negative

- We forgo chafa's quality machinery for photographic content — irrelevant
  for high-contrast glyph masks, but a real limit if inputs ever generalize.

### Neutral

- The prototype's chafa renders become reference fixtures: the Rust
  symbolizer should match or beat them on the same masks.

## Alternatives Considered

- **chafa via FFI** — rejected: C dependency, no maintained binding, and the
  auto-detection/threshold behaviors we'd be working around live below the
  API.
- **Port chafa's full matcher** — rejected: scope without benefit for
  bi-level masks.
