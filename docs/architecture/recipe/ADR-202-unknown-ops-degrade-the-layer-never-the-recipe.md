---
status: Accepted
date: 2026-08-31
deciders:
  - aaronsb
related:
  - ADR-200
  - ADR-201
---

# ADR-202: Unknown ops degrade the layer, never the recipe

## Context

ADR-200 committed to saved recipes continuing to load, and delivered it for
unknown *fields*: a key this build does not know is ignored. Unknown
*variants* were a different story. A recipe naming an effect, fill kind or
register this build lacks failed to parse at all:

```
dotbanner: parsing recipe: unknown variant `warp`, expected one of
`fill`, `rim`, `cast`, `edge`
```

One unknown effect made the whole document unusable — no banner, and a
round-trip through the older build would have destroyed the effect it could
not name. For a format meant to be shared, that turns every new effect into
a breaking change for everyone who has not upgraded.

## Decision

Extensibility is defined per level, and the blast radius never exceeds the
level that failed.

| Unknown | Consequence |
|---------|-------------|
| a field | ignored (ADR-200) |
| a `register` | the layer paints in the default register |
| an `op` or a `fill` kind | that one layer is skipped; the rest renders |
| the document's shape | a parse error naming what was unexpected and where |

- **A pipeline entry is a `Stage`**, either an op this build understands or
  the raw JSON of one it does not. The raw form is kept verbatim, so a
  recipe survives a round-trip through a build that cannot draw it — an old
  dotbanner reading and re-writing a new recipe does not silently delete the
  effect.
- **Skipping is reported, not silent.** A render whose recipe named an
  unknown effect says which and how many layers it dropped, because a
  quietly missing effect reads as a bug in the effect.
- **`version` is advisory.** A recipe declaring a schema newer than the
  build reads still renders what it can, and says so.
- **A genuine parse failure names the position and the expectation**,
  quoting the offending line, because the remaining failure mode is a
  malformed document and the reader needs to find it.

## Consequences

### Positive

- A new effect is no longer a breaking change: older builds render the
  layers they know and preserve the ones they do not.
- The schema can grow variants — new ops, fills, registers — without a
  version bump, so `version` stays a coarse signal rather than a treadmill.

### Negative

- A typo in an op name now renders a banner missing a layer rather than
  refusing, so the warning is the only thing standing between a typo and a
  confusing result.
- The raw JSON of an unknown stage is carried in memory and re-serialized;
  a recipe with many unknown stages costs more than one without.

### Neutral

- Unknown-variant tolerance uses serde's untagged fallback for stages and
  `#[serde(other)]` for registers; neither needs a hand-written
  deserializer.

## Alternatives Considered

- **Strict parsing, bump `version` per effect** — rejected: it makes every
  new effect a breaking change and pushes the cost onto everyone sharing
  recipes.
- **Drop unknown stages on load** — rejected: an older build would then
  destroy an effect it merely could not draw, which is worse than failing.
- **Render a placeholder for an unknown effect** — rejected: inventing
  output for an effect whose semantics are unknown misrepresents the recipe.
