# Contributing

## Build and test

```bash
cargo build
cargo test
```

Rust stable; no system dependencies.

## How changes land

Every change goes through a PR, including from maintainers. The PR template
asks for the governing ADR — most changes don't need a new one, but two areas
always do:

- **Recipe schema** (fields, types, versioning) — recipe-domain ADR, because
  saved recipes circulate and must keep loading.
- **Symbolizer behavior** — symbolizer-domain ADR when a change alters cell
  output; golden-file tests pin the current output exactly.

`docs/scripts/adr list --group` shows the decision record; `docs/scripts/adr
new <domain> "Title"` starts a new one.

## Recipes and presets

New built-in looks are JSON recipes in `presets/`, not code. A preset PR needs
the recipe file and a screenshot of the render.

## Bug reports

A recipe JSON plus the font name is a complete repro — include both.
