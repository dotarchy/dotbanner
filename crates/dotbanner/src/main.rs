//! dotbanner CLI — render banners from recipes or flags.

use std::io::{self, Read, Write};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use dotbanner_core::{
    engine,
    formats::ansi,
    presets,
    recipe::{Font, Recipe, Register, SymbolizerSpec},
};

const LADDER: &str = "\
Three rungs, each one step further in:

  1  flags        dotbanner render \"hello\" --style band --colors fire
  2  see it       dotbanner recipe \"hello\" --style band --colors fire
                  prints the recipe those flags built — the whole render as JSON
  3  edit it      dotbanner render --recipe my.json \"other text\"
                  every effect, colour and register, composable without flags

Discover what the flags accept:
  dotbanner show styles · gradients · fonts [filter] · registers";

#[derive(Parser)]
#[command(
    name = "dotbanner",
    version,
    about = "Terminal banners and figlet-family fonts from real TTF outlines",
    after_help = LADDER,
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
        recipe.rows = args.rows;
    }
    if args.dots {
        recipe.symbolizer = SymbolizerSpec {
            body: Register::Braille,
        };
    }
    Ok(recipe)
}

/// Wrap a family in quotes when it contains spaces, so a suggestion can be
/// pasted straight back onto the command line.
fn quoted(name: &str) -> String {
    if name.contains(' ') {
        format!("\"{name}\"")
    } else {
        name.to_string()
    }
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
                .map(|n| format!("\n    {}", quoted(n)))
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
    match what {
        "styles" => {
            for style in presets::STYLES {
                out.push_str(&format!("\n\x1b[2m──── {style}\x1b[0m\n"));
                let args = RenderArgs {
                    text: Some(sample.to_string()),
                    font: "DejaVu Sans".into(),
                    style_name: None,
                    rows: 7,
                    style: (*style).into(),
                    colors: "omarchy".into(),
                    dots: false,
                    trap_width: 1,
                    recipe: None,
                };
                out.push_str(&render(&args)?);
            }
        }
        "gradients" => {
            for g in presets::GRADIENTS {
                out.push_str(&format!("\n\x1b[2m──── {}\x1b[0m\n", g.name));
                let args = RenderArgs {
                    text: Some(sample.to_string()),
                    font: "DejaVu Sans".into(),
                    style_name: None,
                    rows: 6,
                    style: "band".into(),
                    colors: g.name.into(),
                    dots: false,
                    trap_width: 1,
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
                    "no installed family matches '{}'\n  \
                     list them all:  dotbanner show fonts",
                    filter
                ));
            }
            for family in &shown {
                // Quoted exactly as --font wants it.
                out.push_str(&quoted(family));
                out.push('\n');
            }
            out.push_str(&format!(
                "\n\x1b[2m{} of {} families · filter with: dotbanner show fonts <text>\x1b[0m\n",
                shown.len(),
                all.len()
            ));
        }
        "registers" => {
            out.push_str(
                "\x1b[1mregisters\x1b[0m  which glyphs a layer draws with\n\n\
                 \x1b[1mblocks\x1b[0m    2x2 quadrants — the default, universal font support\n\
                 \x1b[1mfacets\x1b[0m    2x2, corners as triangles — crystalline edges\n\
                 \x1b[1msextants\x1b[0m  2x3 semigraphics — finer, needs Legacy Computing coverage\n\
                 \x1b[1mbraille\x1b[0m   2x4 dots — finest, reads as texture\n\n\
                 \x1b[2mSet the body register in a recipe's \"symbolizer\", or per layer\n\
                 with a layer's \"register\". See: dotbanner recipe <text> --style stipple\x1b[0m\n",
            );
        }
        other => {
            return Err(format!(
                "don't know how to show '{other}'\n  \
                 try:  styles · gradients · fonts · registers"
            ))
        }
    }
    Ok(out)
}

/// What a bare `dotbanner` prints: what it is, and the shortest thing that
/// works, before any flag reference.
fn overview() -> String {
    format!(
        "\x1b[1mdotbanner\x1b[0m — terminal banners from the real fonts on your system\n\n\
         Start here:\n  \
         dotbanner render \"hello\"\n  \
         dotbanner render \"hello\" --style band --colors fire\n  \
         dotbanner show styles\n\n{LADDER}\n\n\
         \x1b[2mFull flag reference: dotbanner render --help\x1b[0m\n"
    )
}

/// clap's own errors are terse ("a value is required for '--font <FONT>'"),
/// so add the one thing the reader needs next: where to find valid values.
fn hint_for(err: &clap::Error) -> Option<&'static str> {
    let text = err.to_string();
    if text.contains("--font") || text.contains("-f") {
        Some("\n  font names, quoted and ready to paste:  dotbanner show fonts [filter]")
    } else if text.contains("--style") {
        Some("\n  every style, rendered:  dotbanner show styles")
    } else if text.contains("--colors") {
        Some("\n  every gradient, rendered:  dotbanner show gradients")
    } else if text.contains("--recipe") {
        Some("\n  build one from flags first:  dotbanner recipe \"text\" --style band")
    } else {
        None
    }
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
        "\x1b[1mdotbanner show\x1b[0m <topic>\n\n  \
         \x1b[1mstyles\x1b[0m     every effect, rendered           {}\n  \
         \x1b[1mgradients\x1b[0m  every colour preset, rendered    {}\n  \
         \x1b[1mfonts\x1b[0m      installed families, quoted for --font\n  \
         \x1b[1mregisters\x1b[0m  which glyphs a layer can draw with\n\n\
         \x1b[2mEach takes optional text: dotbanner show styles \"my text\"\x1b[0m\n",
        presets::STYLES.join(", "),
        presets::GRADIENTS
            .iter()
            .map(|g| g.name)
            .collect::<Vec<_>>()
            .join(", ")
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
            eprintln!("dotbanner: {msg}");
            ExitCode::FAILURE
        }
    }
}
