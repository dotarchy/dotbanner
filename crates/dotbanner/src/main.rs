//! dotbanner CLI — render banners from recipes or flags.

use std::io::{self, Read, Write};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

mod style;
mod tui;
use style::{bad, cmd, heading, hint, name, quoted};

use dotbanner_core::{
    color::Rgb,
    engine,
    formats::ansi,
    presets,
    recipe::{Fit, Font, Recipe, Register, SymbolizerSpec},
    scheme,
};

/// The three-rung model, rendered with the chrome roles. Built at run time
/// because colour depends on whether stdout is a terminal.
fn ladder() -> String {
    format!(
        "{}\n\n  1  {}  {}\n  2  {}  {}\n{}\n  3  {}  {}\n{}\n\n{}\n  {}",
        heading("Three rungs, each one step further in:"),
        name("flags   "),
        cmd("dotbanner render \"hello\" --style band --colors fire"),
        name("see it  "),
        cmd("dotbanner recipe \"hello\" --style band --colors fire"),
        hint("                    prints the recipe those flags built, as JSON"),
        name("edit it "),
        cmd("dotbanner render --recipe my.json \"other text\""),
        hint("                    every effect, colour and register, without flags"),
        heading("Discover what the flags accept:"),
        cmd("dotbanner show styles · colors · fonts [filter] · registers"),
    )
}

#[derive(Parser)]
#[command(
    name = "dotbanner",
    version,
    about = "Terminal banners and figlet-family fonts from real TTF outlines",
    after_help = ladder(),
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Render a banner to the terminal
    Render(RenderArgs),
    /// Print the recipe a set of flags produces, without rendering
    Recipe(RenderArgs),
    /// Edit a recipe interactively, with a live preview
    Tui(RenderArgs),
    /// Show what's available: styles, gradients, fonts, registers
    Show {
        /// styles | gradients | fonts | registers  (omit to list the topics)
        what: Option<String>,
        /// Sample text for style and gradient previews; a filter for fonts
        text: Option<String>,
    },
}

#[derive(Parser, Clone)]
struct RenderArgs {
    /// Text to render (omit when using --recipe)
    text: Option<String>,
    /// Font family, e.g. "JetBrains Mono"
    #[arg(short, long, default_value = "DejaVu Sans")]
    font: String,
    /// Font style to prefer, e.g. "Bold"
    #[arg(long)]
    style_name: Option<String>,
    /// Output height in terminal rows
    #[arg(short, long, default_value_t = 8)]
    rows: usize,
    /// Shrink until the banner fits: a column count, or "terminal"
    #[arg(long, value_name = "COLS|terminal")]
    fit: Option<String>,
    /// Space between glyphs as a fraction of the em (0 for none)
    #[arg(long, value_name = "EM")]
    tracking: Option<f32>,
    /// Effect style; see: dotbanner show styles
    #[arg(short = 's', long, default_value = "plain")]
    style: String,
    /// Palette name or comma-separated hex colors; see: dotbanner show colors
    #[arg(short, long, default_value = "omarchy")]
    colors: String,
    /// Glyph repertoire: blocks, braille, sextants, facets
    #[arg(long, value_name = "REGISTER")]
    register: Option<String>,
    /// How thick the style's edge treatment is, in mask pixels (0-32)
    #[arg(
        long,
        default_value_t = presets::DEFAULT_WEIGHT,
        value_parser = clap::value_parser!(u32).range(0..=32)
    )]
    weight: u32,
    /// Load a recipe JSON file ("-" for stdin); flags override its fields
    #[arg(long)]
    recipe: Option<String>,
}

fn build_recipe(args: &RenderArgs) -> Result<Recipe, String> {
    let mut recipe = match &args.recipe {
        Some(path) => {
            let json = if path == "-" {
                let mut buf = String::new();
                io::stdin()
                    .read_to_string(&mut buf)
                    .map_err(|e| format!("reading stdin: {e}"))?;
                buf
            } else {
                std::fs::read_to_string(path).map_err(|e| format!("reading {path}: {e}"))?
            };
            Recipe::from_json(&json).map_err(|e| recipe_error(path, &json, &e))?
        }
        None => {
            let text = args
                .text
                .clone()
                .ok_or_else(|| "no text given (or use --recipe)".to_string())?;
            let mut r = Recipe::new(text);
            let colors = presets::resolve_colors(&args.colors)
                .ok_or_else(|| format!("unknown gradient or bad colors: {}", args.colors))?;
            let ops = {
                presets::style_pipeline_weighted(&args.style, &colors, args.weight).ok_or_else(
                    || {
                        format!(
                            "unknown style '{}' (try: {})",
                            args.style,
                            presets::STYLES.join(", ")
                        )
                    },
                )?
            };
            r.pipeline = ops.into_iter().map(Into::into).collect();
            r
        }
    };

    // Flags override recipe fields so one recipe can style many banners.
    if let Some(text) = &args.text {
        recipe.text = text.clone();
    }
    if args.font != "DejaVu Sans" || recipe.font.family.is_empty() {
        recipe.font = Font {
            family: args.font.clone(),
            style: args.style_name.clone(),
        };
    } else if args.style_name.is_some() {
        recipe.font.style = args.style_name.clone();
    }
    if args.rows != 8 {
        recipe.size.rows = args.rows;
    }
    if let Some(fit) = &args.fit {
        recipe.size.fit =
            Some(match fit.as_str() {
                "terminal" | "term" | "auto" => Fit::Terminal,
                n => Fit::Columns(n.parse().map_err(|_| {
                    format!("--fit wants a column count or \"terminal\", not '{fit}'")
                })?),
            });
    }
    if let Some(t) = args.tracking {
        recipe.size.tracking = t;
    }
    if let Some(r) = &args.register {
        let body = match r.as_str() {
            "blocks" => Register::Blocks,
            "braille" | "dots" => Register::Braille,
            "sextants" => Register::Sextants,
            "facets" => Register::Facets,
            other => {
                return Err(format!(
                    "unknown register '{other}'\n  {} {}",
                    hint("the four this build draws with:"),
                    cmd("dotbanner show registers"),
                ))
            }
        };
        recipe.symbolizer = SymbolizerSpec { body };
    }
    Ok(recipe)
}

fn font_error(e: engine::EngineError) -> String {
    match e {
        engine::EngineError::FontNotFound { query, near } if !near.is_empty() => {
            let list = near
                .iter()
                .map(|n| quoted(n))
                .collect::<Vec<_>>()
                .join("  ");
            format!(
                "no font matched '{query}'\n  did you mean:  {list}\n  \
                 browse them:  dotbanner show fonts {query}"
            )
        }
        engine::EngineError::FontNotFound { query, .. } => {
            format!("no font matched '{query}'\n  list what is installed:  dotbanner show fonts")
        }
        engine::EngineError::StyleNotFound {
            family,
            style,
            available,
        } => {
            let list = available
                .iter()
                .map(|n| format!("\n    {}", name(n)))
                .collect::<String>();
            format!("'{family}' has no '{style}' face — it has:{list}")
        }
        engine::EngineError::FontAmbiguous { query, matches } => {
            let list = matches
                .iter()
                .map(|n| format!("\n    {}", name(&quoted(n))))
                .collect::<String>();
            format!("'{query}' matches several families — pick one:{list}")
        }
        other => other.to_string(),
    }
}

/// Present a parse failure the way the file reads: what was unexpected,
/// where, and the offending line. serde carries the expectation and the
/// position; this puts them next to the text they refer to.
fn recipe_error(path: &str, json: &str, err: &dotbanner_core::serde_json::Error) -> String {
    let (line, col) = (err.line(), err.column());
    let source = if line > 0 {
        json.lines()
            .nth(line - 1)
            .map(|text| {
                let caret = " ".repeat(col.saturating_sub(1));
                format!("\n  {}\n  {}{}", text.trim_end(), caret, name("^"))
            })
            .unwrap_or_default()
    } else {
        String::new()
    };
    let where_ = if line > 0 {
        format!("{path}:{line}:{col}")
    } else {
        path.to_string()
    };
    format!(
        "{where_}: {err}{source}\n  {} {}",
        hint("a valid recipe of this shape:"),
        cmd("dotbanner recipe \"text\" --style band"),
    )
}

fn render(args: &RenderArgs) -> Result<String, String> {
    let recipe = build_recipe(args)?;
    // A layer this build cannot draw is skipped, not fatal — but say so, or
    // the banner is quietly missing an effect the recipe asked for.
    let skipped = recipe.unknown_ops();
    if !skipped.is_empty() {
        eprintln!(
            "{} this build has no effect named {} — {} layer(s) skipped",
            hint("note:"),
            skipped
                .iter()
                .map(|s| name(s))
                .collect::<Vec<_>>()
                .join(", "),
            skipped.len(),
        );
    }
    if recipe.is_newer_than_this_build() {
        eprintln!(
            "{} the recipe declares schema v{} and this build reads v{}",
            hint("note:"),
            recipe.version,
            dotbanner_core::recipe::SCHEMA_VERSION,
        );
    }
    let grid = dotbanner_core::render(&recipe).map_err(font_error)?;
    Ok(ansi::to_ansi(&grid))
}

/// `text` carries two meanings: sample text for the rendered topics, and a
/// filter for `fonts`. It stays optional so an absent one means "no filter"
/// rather than filtering for the sample.
fn show(what: &str, text: Option<&str>) -> Result<String, String> {
    const SAMPLE: &str = "dotbanner";
    let sample = text.unwrap_or(SAMPLE);
    let mut out = String::new();
    // Accept the singular and the flag's own spelling: --colors takes a
    // gradient, so `show colors` has to reach the same place.
    let topic = match what {
        "style" => "styles",
        "colors" | "colours" | "colour" | "color" | "gradient" => "gradients",
        "font" => "fonts",
        "register" => "registers",
        other => other,
    };
    match topic {
        "styles" => {
            for style in presets::STYLES {
                out.push_str(&format!("\n{}\n", hint(&format!("──── {style}"))));
                let args = RenderArgs {
                    text: Some(sample.to_string()),
                    font: "DejaVu Sans".into(),
                    style_name: None,
                    rows: 7,
                    style: (*style).into(),
                    colors: "omarchy".into(),
                    register: None,
                    weight: presets::DEFAULT_WEIGHT,
                    fit: Some("terminal".into()),
                    tracking: None,
                    recipe: None,
                };
                out.push_str(&render(&args)?);
            }
        }
        "gradients" if text.is_none() => {
            // No sample text: show every palette as a compact swatch rather
            // than a screen of banners. The grammar goes first, since the
            // named set is a convenience and any hex list is valid.
            out.push_str(&format!(
                "{}\n  {}  {}\n  {}  {}\n  {}  {}\n\n",
                heading("--colors accepts"),
                name("a hex list"),
                hint("--colors \"#f8ffff,#3f7fe8,#8a2fc8\"   any length, top to bottom"),
                name("a palette "),
                hint("--colors fire                        named below"),
                name("a file    "),
                hint("--colors gruvbox-dark-hard           any base16 or ramp file"),
            ));
            let all = scheme::all();
            let (installed, shipped): (Vec<_>, Vec<_>) = all
                .iter()
                .partition(|s| s.source == scheme::Source::Installed);
            if !installed.is_empty() {
                out.push_str(&format!("{}\n", heading("installed")));
                for sc in &installed {
                    out.push_str(&format!(
                        "  {}{}\n",
                        name(&pad(&sc.name, 18)),
                        swatch(&sc.ramp())
                    ));
                }
                out.push('\n');
            }
            out.push_str(&format!("{}\n", heading("built in")));
            for sc in &shipped {
                out.push_str(&format!(
                    "  {}{}\n",
                    name(&pad(&sc.name, 18)),
                    swatch(&sc.ramp())
                ));
            }
            out.push_str(&format!(
                "\n{}\n{}\n{}\n",
                hint("Render one:  dotbanner render \"text\" --colors monokai"),
                hint("See them rendered:  dotbanner show colors \"text\""),
                hint(&format!(
                    "Add or override: drop a base16 or ramp file into {}",
                    scheme::scheme_dirs()
                        .first()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default()
                )),
            ));
        }
        "gradients" => {
            for g in scheme::all() {
                out.push_str(&format!("\n{}\n", hint(&format!("──── {}", g.name))));
                let args = RenderArgs {
                    text: Some(sample.to_string()),
                    font: "DejaVu Sans".into(),
                    style_name: None,
                    rows: 6,
                    style: "band".into(),
                    colors: g.name.clone(),
                    register: None,
                    weight: presets::DEFAULT_WEIGHT,
                    fit: Some("terminal".into()),
                    tracking: None,
                    recipe: None,
                };
                out.push_str(&render(&args)?);
            }
        }
        "fonts" => {
            // The optional text argument filters, since the full list runs to
            // hundreds of families.
            let filter = text.unwrap_or_default().to_ascii_lowercase();
            let all = engine::list_families();
            let shown: Vec<&String> = all
                .iter()
                .filter(|f| filter.is_empty() || f.to_ascii_lowercase().contains(&filter))
                .collect();
            if shown.is_empty() {
                return Err(format!(
                    "no installed family matches '{}'\n  {} {}",
                    filter,
                    hint("list them all:"),
                    cmd("dotbanner show fonts"),
                ));
            }
            for family in &shown {
                // Quoted exactly as --font wants it.
                out.push_str(&quoted(family));
                out.push('\n');
            }
            out.push_str(&format!(
                "\n{}\n",
                hint(&format!(
                    "{} of {} families · filter with: dotbanner show fonts <text>",
                    shown.len(),
                    all.len()
                ))
            ));
        }
        "registers" => {
            out.push_str(&format!(
                "{}  which glyphs a layer draws with\n\n  {}  {}\n  {}  {}\n  {}  {}\n  {}  {}\n\n{}\n",
                heading("registers"),
                name("blocks  "),
                hint("2x2 quadrants — the default, universal font support"),
                name("facets  "),
                hint("2x2, corners as triangles — crystalline edges"),
                name("sextants"),
                hint("2x3 semigraphics — needs Legacy Computing font coverage"),
                name("braille "),
                hint("2x4 dots — finest, reads as texture"),
                hint(
                    "Set the body register in a recipe's \"symbolizer\", or per layer with a\n\
                     layer's \"register\". See it: dotbanner recipe <text> --style stipple"
                ),
            ));
        }
        other => {
            return Err(format!(
                "don't know how to show '{other}'\n  {} {}",
                hint("try:"),
                name("styles · colors · fonts · registers"),
            ))
        }
    }
    Ok(out)
}

/// What a bare `dotbanner` prints: what it is, and the shortest thing that
/// works, before any flag reference.
fn overview() -> String {
    format!(
        "{} — terminal banners from the real fonts on your system\n\n{}\n  {}\n  {}\n  {}\n\n{}\n\n{}\n",
        heading("dotbanner"),
        heading("Start here:"),
        cmd("dotbanner render \"hello\""),
        cmd("dotbanner render \"hello\" --style band --colors fire"),
        cmd("dotbanner show styles"),
        ladder(),
        hint("Full flag reference: dotbanner render --help"),
    )
}

/// clap's own errors are terse ("a value is required for '--font <FONT>'"),
/// so add the one thing the reader needs next: where to find valid values.
fn hint_for(err: &clap::Error) -> Option<String> {
    // --help and --version arrive as "errors" too; they are not failures and
    // want no hint appended.
    use clap::error::ErrorKind;
    if matches!(
        err.kind(),
        ErrorKind::DisplayHelp
            | ErrorKind::DisplayVersion
            | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    ) {
        return None;
    }
    // Match whole flag tokens: a bare substring search finds "-f" inside
    // prose like "figlet-family".
    let text = err.to_string();
    let mentions = |flag: &str| {
        text.split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_'))
            .any(|tok| tok == flag)
    };
    let (label, command) = if mentions("--fit") {
        (
            "a column count, or:",
            "dotbanner render \"text\" --fit terminal",
        )
    } else if mentions("--font") || mentions("-f") {
        (
            "font names, quoted and ready to paste:",
            "dotbanner show fonts [filter]",
        )
    } else if mentions("--style") || mentions("-s") {
        ("every style, rendered:", "dotbanner show styles")
    } else if mentions("--register") {
        ("the four registers:", "dotbanner show registers")
    } else if mentions("--weight") {
        (
            "how thick an edge treatment is, in mask pixels:",
            "dotbanner show styles",
        )
    } else if mentions("--colors") || mentions("-c") {
        ("every colour preset, rendered:", "dotbanner show colors")
    } else if mentions("--recipe") {
        (
            "build one from flags first:",
            "dotbanner recipe \"text\" --style band",
        )
    } else {
        return None;
    };
    Some(format!("  {} {}", hint(label), cmd(command)))
}

fn run() -> Result<String, String> {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            let hint = hint_for(&e);
            e.print().ok();
            if let Some(h) = hint {
                eprintln!("{h}");
            }
            std::process::exit(e.exit_code());
        }
    };
    match &cli.command {
        None => Ok(overview()),
        Some(Command::Render(args)) => render(args),
        Some(Command::Recipe(args)) => Ok(build_recipe(args)?.to_json() + "\n"),
        Some(Command::Tui(args)) => run_tui(args),
        Some(Command::Show { what, text }) => match what.as_deref() {
            None => Ok(show_topics()),
            Some(w) => show(w, text.as_deref()),
        },
    }
}

/// Build the starting document from the same flags `render` takes, then
/// hand the screen to the editor. Stdin recipes are refused: the terminal
/// the TUI reads keys from is the terminal "-" would consume.
fn run_tui(args: &RenderArgs) -> Result<String, String> {
    if args.recipe.as_deref() == Some("-") {
        return Err("the editor cannot read a recipe from stdin — pass a file path".into());
    }
    let mut args = args.clone();
    if args.text.is_none() && args.recipe.is_none() {
        args.text = Some("dotbanner".into());
    }
    let recipe = build_recipe(&args)?;
    let path = args
        .recipe
        .clone()
        .unwrap_or_else(|| "banner.json".to_string());
    let mut app = tui::App::new(
        recipe,
        path,
        args.style.clone(),
        args.colors.clone(),
        args.weight,
    );
    ratatui::run(|terminal| tui::run(&mut app, terminal)).map_err(|e| e.to_string())?;
    Ok(String::new())
}

/// `show` with no topic lists the topics rather than erroring.
/// Pad to a column width before styling: escape sequences count toward a
/// format width but occupy no columns.
fn pad(text: &str, width: usize) -> String {
    format!("{text:<width$}")
}

/// A continuous bar of a ramp's colours, so a palette reads at a glance
/// without rendering a whole banner.
fn swatch(stops: &[Rgb]) -> String {
    use dotbanner_core::engine::Paint;
    if stops.is_empty() {
        return String::new();
    }
    const WIDTH: usize = 32;
    let paint = Paint::Bands {
        stops: stops.to_vec(),
        steps: None,
    };
    if !style::colored() {
        // Without colour a bar says nothing; list the stops instead.
        return stops
            .iter()
            .map(|c| c.to_hex())
            .collect::<Vec<_>>()
            .join(" ");
    }
    (0..WIDTH)
        .map(|i| {
            let c = paint.color_at(i as f32 / (WIDTH - 1) as f32);
            format!("\x1b[38;2;{};{};{}m█", c.r, c.g, c.b)
        })
        .collect::<String>()
        + "\x1b[0m"
}

fn show_topics() -> String {
    format!(
        "{} <topic>\n\n  {}  every effect, rendered        {}\n  {}  every colour preset, rendered {}\n  \
         {}  installed families, quoted for --font\n  {}  which glyphs a layer can draw with\n\n{}\n",
        heading("dotbanner show"),
        name("styles   "),
        hint(&presets::STYLES.join(", ")),
        name("colors   "),
        hint(&format!(
            "{} palettes, any hex list, any base16 file",
            scheme::all().len()
        )),
        name("fonts    "),
        name("registers"),
        hint("Each takes optional text: dotbanner show styles \"my text\""),
    )
}

fn main() -> ExitCode {
    match run() {
        Ok(out) => {
            print!("{out}");
            let _ = io::stdout().flush();
            ExitCode::SUCCESS
        }
        Err(msg) => {
            eprintln!("{} {msg}", bad("dotbanner:"));
            ExitCode::FAILURE
        }
    }
}
