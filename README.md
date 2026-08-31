# dotbanner

Terminal banners and figlet-family fonts, generated from the real fonts on
your system.

dotbanner rasterizes text with actual TTF/OTF outlines, runs it through an
image-space effects pipeline (gradients, trapped rims, glows, extrusions,
drop shadows), and maps the result to Unicode character art — block elements
for solid bodies, braille for dotted shadows. Preview live in a TUI, then bake
the result into a reusable font: toilet `.tlf`, figlet `.flf`, or cfonts JSON.
As far as we can find, it is the first TLF generator in existence — every
`.tlf` in circulation today was drawn by hand.

## Status

Scaffold. The working prototype is a bash pipeline (ImageMagick + chafa);
this repository is its Rust successor — single static binary, no runtime
dependencies, recipe-driven. The founding decisions live in
[docs/architecture/](docs/architecture/INDEX.md).

## The model

```
text + font ──► mask ──► effects (fill / rim / cast) ──► symbolizer ──► preview
                                                                    ├──► .tlf / .flf / cfonts
                                                                    └──► .ans / .sh animations
```

A render is described by a **recipe** — a small JSON document naming the font,
the pipeline of effects, and the symbolizer registers. Presets are recipes,
the TUI is a recipe editor, the CLI replays recipes, and sharing a look means
sharing a file:

```json
{
  "name": "omarchy-laser",
  "text": "dotarchy",
  "font": { "family": "Pirata One" },
  "pipeline": [
    { "op": "fill", "kind": "band", "stops": ["#f8ffff", "#a8ecfa", "#3f7fe8", "#8a2fc8"] },
    { "op": "rim", "erode": 5, "color": "#e8f6ff" }
  ],
  "symbolizer": { "body": "blocks", "cast": "braille" }
}
```

## Quick start

Not yet — the binary is under construction. Watch releases.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Decisions are recorded as ADRs
(`docs/scripts/adr list --group`); the recipe schema and the symbolizer's
determinism guarantee both have dedicated ADRs worth reading before touching
those areas.

## License

MIT
