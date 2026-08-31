# dotbanner — session handoff

Written 2026-08-31 at the end of the founding session. That session ran from
`~/.dotfiles`, so this file exists to carry its state into a session started
from `~/Projects/app/dotbanner`. Paste the block below to resume cold.

---

```
Continue work on dotbanner (~/Projects/app/dotbanner, repo dotarchy/dotbanner,
public, MIT) — a terminal banner and figlet-family font generator: it renders
text with real TTF/OTF outlines, runs an image-space effects pipeline, and maps
the result to Unicode character art. Prior-art research found no existing TLF
generator of any kind, so this is plausibly the first.

BUILD: `make check` = clippy -D warnings + fmt --check + cargo test + adr lint.
`make run ARGS='render "hello" --style band --colors fire'`. main is green:
67 tests. CI runs the same gate on every PR; merging to main also builds and
uploads a dotbanner-linux-x86_64 artifact.

LANDED — everything from the founding session is merged to main and green:
  - PR #1 scaffold: cargo workspace (dotbanner-core lib + dotbanner bin), ADR
    tooling with five domains (meta/recipe/engine/symbolizer/surfaces),
    annotated CODEOWNERS, CI + artifact workflows, Makefile.
  - PR #2 symbolizer: coverage-mapping pixels→cells, blocks + braille.
  - PR #4 the big one (45 files, +4369): working render path (fontdb +
    ab_glyph rasterization, erode/dilate/difference morphology, banded
    gradients), fill/rim/cast/edge op vocabulary, two-color cell compositing
    with silhouette trapping, four symbol registers, base16 palettes, CLI
    ladder with semantic chrome, size/fit control, schema tolerant of unknown
    effects. Two adversarial reviews found 17 defects (6 blocking) — all
    remediated before merge.
  - PR #5 control consistency: --dots→--register, --trap-width→--weight
    (scales whichever edge treatment a style has), and size.rows became an
    exact height contract via em-size bisection. One review, 8 findings, 2
    blocking, all fixed.
  - PR #3 is STILL OPEN: ADR-401 (corner smoothing) as Draft, docs only.

STATE — IN FLIGHT — nothing. Both repos clean, no unmerged commits, no
uncommitted files. dotfiles-side work (dotfonts Google Fonts support, dotbanner
bash prototype) is committed and pushed via `dotfiles push`.

INVARIANTS / GOTCHAS — not recoverable from git:
  - DETERMINISM IS A HARD INVARIANT (ADR-400). The symbolizer is coverage
    mapping plus fixed lookup, no perceptual heuristics. Baked fonts and shared
    recipes depend on identical output across machines. Anything that would
    trade it away needs a symbolizer-domain ADR first. This is why font face
    selection sorts before choosing — fontdb yields faces in filesystem scan
    order, which differs per machine.
  - THE SHARED CELL FOOTPRINT IS 6×12 PIXELS. Twelve rows is the LCM of every
    register's sub-row count (blocks 2, sextants 3, braille 4) so each samples
    whole sub-blocks; six columns preserves a terminal cell's 1:2 aspect.
    Changing either breaks every register at once.
  - PADDING FOR OUTWARD EFFECTS LANDS IN THE FINISHED BANNER. Both the height
    and fit solves must measure the padded size. Forgetting this made --rows 8
    render 9 for four of nine styles — it was a blocking review finding, and
    the regression test `an_outward_style_does_not_inflate_the_requested_height`
    guards it.
  - ADR-202 TOLERANCE IS ASYMMETRIC ON PURPOSE. An unrecognised op or fill kind
    degrades that one layer and its raw JSON is preserved verbatim; a mistake
    inside a KNOWN op (typo'd field, bad hex) is a hard error. Any code that
    writes recipes must preserve Stage::Unknown entries or it destroys effects
    a newer build wrote.
  - CHROME COLOUR VS CONTENT COLOUR. A rendered banner's colour is content and
    always survives a pipe; the tool's own chrome is presentation and drops
    when stdout is not a tty or NO_COLOR is set. crates/dotbanner/src/style.rs
    owns the four roles (heading/name/cmd/hint/bad).
  - `dotfiles push` in ~/.dotfiles STAGES THE WHOLE TREE (git add -A). Check
    `git status` first; never use raw git there.

TASKS — recreate these with the task list tool before starting; they are the
working set.

  #16 [IN PROGRESS] Build the interactive TUI (recipe editor)
      THE next increment. Design settled in ADR-200/201: the TUI is a live
      recipe editor, not a separate surface. Every control writes a recipe
      field, the preview re-renders, save writes JSON. Tier 1 = parameter panel
      (font picker, style/register/palette selectors, rows + weight sliders,
      text input) beside a live preview. Tier 2, later = node-graph canvas over
      the same document; docs/architecture/context.mmd is the mental model.
      STACK: ratatui. Prefer a `tui` subcommand in crates/dotbanner over a new
      crate — one binary, and the CLI already owns style.rs.
      API: dotbanner_core::render(&Recipe) -> Result<CellGrid, EngineError> is
      the whole render call; formats::ansi::to_ansi(&grid) renders it;
      scheme::all() lists palettes with .ramp(); engine::list_families() lists
      fonts; presets::STYLES lists styles;
      presets::style_pipeline_weighted(style, colors, weight) builds a pipeline.
      CONTROLS MAP 1:1 TO SCHEMA (finished in PR #5, do not re-derive):
      font/style-name → recipe.font; rows/fit/tracking → recipe.size; style →
      pipeline; colors → palette; register → symbolizer.body; weight → edge
      thickness of whatever edge the style has.
      WATCH: preserve Stage::Unknown on save (see ADR-202 invariant above). The
      TUI owns the screen, so style unconditionally rather than through
      style::colored(), which is false when stdout is not a tty.

  #17 [OPEN, blocked by #16] Implement ADR-401 corner smoothing
      ADR-401 is written and sits at Draft on PR #3, open and unmerged. Decide:
      merge the ADR as Draft to get it onto main, or implement then merge.
      WHAT: a deterministic post-symbolizer pass that classifies staircase
      patterns and substitutes slope glyphs, MLAA-style. Two payoffs — smoother
      diagonals, and fine detail (serifs, thin diagonals) surviving
      quantization by carrying sub-cell coverage across instead of discarding
      it at the threshold.
      GROUNDWORK: symbolize_layers already computes per-cell coverage counts —
      exactly the signal the substitution needs. The ADR-402 trapping rule also
      guarantees the BODY's block glyph owns every silhouette boundary cell,
      which is precisely where substitution applies.
      GLYPH TIERS (researched in-session): ◢◣◤◥ (U+25E2-5, Geometric Shapes)
      are inset standalone symbols and do NOT tile — that is why the facets
      register floats in fonts like Monoid. The tiling set is Symbols for
      Legacy Computing smooth mosaics U+1FB3C-1FB6F, whose Unicode names encode
      their diagonal endpoints; that table IS the substitution lookup. Font
      coverage is the catch: on this machine only Adwaita Mono and Noto Sans
      Symbols 2 cover the block, so the ADR's tiered safe/extended repertoires
      matter and the tier should probably be a recipe field.
      CONSTRAINT: determinism (see invariants). Golden-file tests required.

  #18 [OPEN] Retire the bash dotbanner prototype from the dotfiles store
      ~/.dotfiles/dotbanner/dotbanner is the original bash prototype (magick +
      chafa + toilet + python3), still deployed to ~/.local/bin/dotbanner. It is
      TD-1 in docs/tech-debt.md.
      BLOCKING GAP: the Rust binary does not yet do two things the bash tool
      does — `font` (bake .tlf/.flf/cfonts font files) and `animate`
      (rolled-gradient ANSImations as .ans or self-contained .sh players).
      Those are the whole "first TLF generator" claim, so they matter. Until
      they exist, both coexist.
      ALSO: ~/.dotfiles/dotbanner/fonts/ holds five disposable smoke-test fonts
      swept in by `dotfiles push`. Delete when convenient.
      WHEN RETIRING: `dotfiles disable dotbanner`, remove both manifest entries
      (dotbanner + dotbanner-fonts), commit with `dotfiles push -m "..."`.

  #1-15 [DONE] scaffold, symbolizer, MVP render path, build pipeline, CLI UX,
      base16 palettes, schema extensibility, control consistency.

DO NEXT — start #16, the TUI. Concretely: add a `tui` subcommand to
crates/dotbanner, `cargo add ratatui crossterm`, and build the Tier 1 layout —
left panel of controls bound to Recipe fields, right panel showing
to_ansi(render(&recipe)) re-rendered on every change, `s` to save the recipe as
JSON. Get one control (text input) driving a live preview before adding the
rest; that proves the loop.
  Alternatives if the TUI is not the appetite: (a) port `font` baking from the
bash prototype into the Rust binary, which unblocks #18 and is the project's
distinctive claim; (b) port `animate`; (c) take #17's smoothing pass, the most
technically interesting piece.

CONVENTIONS —
  - Branch → commit → PR → review → remediate → merge (regular merge commit,
    never squash unless the branch is genuinely noise) → delete branch → pull.
  - REVIEWS FIND REAL DEFECTS HERE. Three reviews this session found 25
    findings, 8 blocking, including clipped shadows, vanishing thin strokes,
    silently shrinking presets, and a process abort. Dispatch a code-reviewer
    subagent before merging anything touching engine/symbolizer/schema, and
    remediate before the gate rather than merging over findings.
  - ADRs: `docs/scripts/adr new <domain> "Title"`, `docs/scripts/adr list
    --group`. Flip Proposed→Accepted when the implementing PR merges — the
    merge is when the claim becomes true.
  - Recipe schema changes require a recipe-domain ADR (the project way in
    .claude/ways/recipe/ enforces this). Symbolizer changes that alter cell
    output require a symbolizer-domain ADR and golden-file justification.
  - Commit messages: what changed, why, and the impact. Long-form is normal
    here; several commits this session run 15+ lines and that is the house
    style.

KEY FILES —
  crates/dotbanner-core/src/
    recipe.rs      the schema: Recipe, Stage, Op, Fill, Register, Size, Fit
    engine.rs      rasterization, morphology, the height/fit solves, Paint
    symbolizer/    pixels→cells: Mask, SymbolSet, Cell, CellGrid, composition
    scheme.rs      base16 + ramp palettes, built-in and installed
    presets.rs     named styles → pipelines, weight scaling
    formats/ansi.rs the truecolor sink
    schemes/*.yaml the 16 shipped palettes, embedded with include_str!
  crates/dotbanner/src/
    main.rs        CLI: render / recipe / show, the three-rung ladder
    style.rs       chrome roles (heading/name/cmd/hint/bad), NO_COLOR aware
  docs/architecture/  seven ADRs; INDEX.md lists them
  presets/*.json      five shipped recipes (chrome-stipple, chrome-overlay,
                      chrome-fade, chrome-full, crystal)
  .claude/ways/       recipe + symbolizer project ways
```
