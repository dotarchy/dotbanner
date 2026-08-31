---
status: Accepted
date: 2026-08-31
deciders:
  - aaronsb
related:
  - ADR-100
  - ADR-300
  - ADR-400
---

# ADR-200: Recipe JSON as the canonical pipeline document

## Context

Every dotbanner render is a pipeline: text + font → mask → image-space effect
ops → symbolizer → sink (preview, font file, animation). The bash prototype
hard-coded each style as a shell function; combinations lived nowhere and died
with the invocation. The project goal is "easy and fun": experiments must be
saveable, replayable, and shareable.

## Decision

The render recipe is a JSON document, and that document is the single contract
every part of the system reads and writes:

```json
{
  "name": "omarchy-laser",
  "text": "dotarchy",
  "font": { "family": "Pirata One", "style": "Regular" },
  "size": { "rows": 8 },
  "pipeline": [
    { "op": "fill", "kind": "band", "stops": ["#f8ffff", "#a8ecfa", "#3f7fe8", "#8a2fc8"], "steps": 10 },
    { "op": "rim", "width": 5, "kind": "solid", "color": "#e8f6ff" }
  ],
  "symbolizer": { "body": "blocks", "cast": "braille" },
  "animate": { "roll": "vertical", "frames": 24 }
}
```

The op shapes above are ADR-201's; this sketch is kept current so a reader
does not take a superseded spelling for the schema.

- **Presets are shipped recipes** — the built-in gallery is a directory of
  these files, browsed with the same `show` convention users get.
- **The CLI replays recipes** — `dotbanner render -r file.json`, with `text`
  and `font` overridable at invocation so one recipe styles many banners.
- **The TUI is a live recipe editor** — it renders the pipeline as a flowchart
  that matures as the user selects inputs, effects, and outputs; every knob
  writes a recipe field and re-runs the preview. Save writes the JSON.
- **Compatibility**: the schema carries a `version` field from its first
  stable release; saved recipes must keep loading. Schema changes require a
  recipe-domain ADR.

## Consequences

### Positive

- Sharing is a file — the oh-my-posh theme culture, applied to banners.
- The TUI, CLI, and any future graph editor are views over one document; no
  surface owns private state.

### Negative

- The schema is a public contract; changing it costs an ADR and a migration
  story once recipes circulate.

### Neutral

- A `dotbanner-recipes` community repo becomes possible but is out of scope
  here.

## Alternatives Considered

- **CLI flags only** (the prototype's model) — rejected: combinations are not
  saveable or shareable.
- **A DSL / TOML** — rejected: JSON round-trips through serde and the TUI
  without a parser of our own, and recipes are machine-written more often than
  hand-written.
