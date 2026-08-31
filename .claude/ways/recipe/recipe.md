---
description: the recipe JSON contract — schema fields, presets, saved-recipe compatibility
vocabulary: recipe schema preset json field version compat serde pipeline stops symbolizer animate
files: recipe|preset
scope: agent, subagent
---
<!-- epistemic: convention -->
# Recipe Way

The recipe document is dotbanner's one contract (ADR-200): the TUI edits it,
the CLI replays it, presets ship as it, users trade it as files.

- **Schema changes require a recipe-domain ADR.** A field added, renamed, or
  retyped changes what other people's saved files mean.
- **Saved recipes must keep loading.** The schema carries `version`; loaders
  accept every version ever released. Breaking a circulating recipe is a bug.
- **Presets are recipes.** A new built-in style is a JSON file in `presets/`,
  never a code path. If it can't be expressed as a recipe, the schema is
  missing something — that's the ADR conversation.
- **Serde round-trip is an invariant**: parse → serialize → parse must be
  identity. Unknown fields warn, never fail.
