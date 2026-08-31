# Tech Debt Register

Known debts and prototype learnings carried into the rewrite. Retire rows by
linking the PR that resolves them.

| ID | Description | Context | Impact | Effort | When to address |
|----|-------------|---------|--------|--------|-----------------|
| TD-1 | Bash prototype still deployed from the dotfiles store | The `dotbanner` script (magick+chafa+toilet+python3) remains the working tool until the binary reaches parity | Two implementations to keep honest | — | Retire when `cargo install dotbanner` covers render/font/animate |
| TD-2 | Mid-gray content vanishes under bi-level thresholding | chafa's `-c none` dropped 35–55% gray backgrounds silently; the prototype's pattern-fill styles never worked | Any "background texture" effect needs cell-space compositing, not luminance tricks | Medium | When background/texture fills are designed (engine ADR) |
| TD-3 | Shadow/trap cell merges were bolted on in Python | Two chafa passes merged cell-wise because one pass couldn't mix symbol registers | The per-region register design (ADR-400) exists to delete this; verify it actually covers all three prototype merge cases | Small | During symbolizer implementation |
| TD-4 | TLF color fonts unexplored | TLF supports ANSI color in glyphs; the prototype only baked mono fonts, so gradient/chrome styles can't be saved as fonts yet | A whole output category (colored toilet fonts) is designed around but unproven | Medium | Surfaces-domain ADR after mono formats work |
| TD-5 | Glyph metrics are guessed, not measured | Prototype fonts used chafa's padded widths; real advance widths, kerning classes, and figlet smushing rules were ignored (full-width layout only) | Baked fonts are wider than they should be; no smushing support | Medium | Engine implementation — ab_glyph exposes real metrics |
