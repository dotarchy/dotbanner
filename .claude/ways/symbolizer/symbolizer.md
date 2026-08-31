---
description: the pixels-to-cells symbolizer — coverage mapping, determinism, symbol sets
vocabulary: symbolizer cell braille quad block coverage threshold deterministic golden register glyph
files: symbolizer
scope: agent, subagent
---
<!-- epistemic: convention -->
# Symbolizer Way

The symbolizer (ADR-400) is the single crossing from image space to character
space, and the one component with no prior art — treat it as load-bearing.

- **Determinism is a hard invariant.** Identical pixels + spec = identical
  cells, on every platform. Baked fonts and shared recipes reproduce exactly.
- **Coverage mapping only** — threshold the cell, look up the symbol by bit
  pattern. Any perceptual heuristic (error diffusion, work factors) trades
  determinism away and requires a symbolizer-domain ADR first.
- **Golden-file tests gate changes.** Cell output is pinned exactly; a diff
  in golden files is either a bug or a deliberate, ADR-justified regeneration
  named in the PR.
- **Per-region registers** (body/rim/cast each with their own symbol set) are
  the design center — resist collapsing them into a single global set.
- The bash prototype's chafa renders are reference fixtures: match or beat
  them on the same masks.
