//! dotbanner CLI — render banners from recipes or flags.

use std::io::{self, Read, Write};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

mod style;
use style::{bad, cmd, heading, hint, name, quoted};

use dotbanner_core::{
    engine,
    formats::ansi,
    presets,
    recipe::{Fit, Font, Recipe, Register, SymbolizerSpec},
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
    /// Effect style: plain, band, gradient, trap
    #[arg(short = 's', long, default_value = "plain")]
    style: String,
    /// Gradient preset name or comma-separated hex colors
    #[arg(short, long, default_value = "omarchy")]
    colors: String,
    /// Render with braille instead of block glyphs
    #[arg(long)]
    dots: bool,
    /// Trap rim width in mask pixels (2 px = 1 block row, 4 px = 1 braille
    /// row, so 1 is a subpixel trap)
    #[arg(long, default_value_t = 1)]
    trap_width: u32,
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
            Recipe::from_json(&json).map_err(|e| format!("parsing recipe: {e}"))?
        }
        None => {
            let text = args
                .text
                .clone()
                .ok_or_else(|| "no text given (or use --recipe)".to_string())?;
            let mut r = Recipe::new(text);
            let colors = presets::resolve_colors(&args.colors)
                .ok_or_else(|| format!("unknown gradient or bad colors: {}", args.colors))?;
            r.pipeline = if args.style == "trap" {
                presets::trap_pipeline(&colors, args.trap_width)
            } else {
                presets::style_pipeline(&args.style, &colors).ok_or_else(|| {
                    format!(
                        "unknown style '{}' (try: {})",
                        args.style,
                        presets::STYLES.join(", ")
                    )
                })?
            };
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
    if args.dots {
        recipe.symbolizer = SymbolizerSpec {
            body: Register::Braille,
        };
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

fn render(args: &RenderArgs) -> Result<String, String> {
    let recipe = build_recipe(args)?;
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
                    dots: false,
                    trap_width: 1,
                    fit: Some("terminal".into()),
                    tracking: None,
                    recipe: None,
                };
                out.push_str(&render(&args)?);
            }
        }
        "gradients" => {
            for g in presets::GRADIENTS {
                out.push_str(&format!("\n{}\n", hint(&format!("──── {}", g.name))));
                let args = RenderArgs {
                    text: Some(sample.to_string()),
                    font: "DejaVu Sans".into(),
                    style_name: None,
                    rows: 6,
                    style: "band".into(),
                    colors: g.name.into(),
                    dots: false,
                    trap_width: 1,
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
        Some(Command::Show { what, text }) => match what.as_deref() {
            None => Ok(show_topics()),
            Some(w) => show(w, text.as_deref()),
        },
    }
}

/// `show` with no topic lists the topics rather than erroring.
fn show_topics() -> String {
    format!(
        "{} <topic>\n\n  {}  every effect, rendered        {}\n  {}  every colour preset, rendered {}\n  \
         {}  installed families, quoted for --font\n  {}  which glyphs a layer can draw with\n\n{}\n",
        heading("dotbanner show"),
        name("styles   "),
        hint(&presets::STYLES.join(", ")),
        name("colors   "),
        hint(
            &presets::GRADIENTS
                .iter()
                .map(|g| g.name)
                .collect::<Vec<_>>()
                .join(", ")
        ),
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
