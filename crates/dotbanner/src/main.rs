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

#[derive(Parser)]
#[command(
    name = "dotbanner",
    version,
    about = "Terminal banners and figlet-family fonts from real TTF outlines",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Render a banner to the terminal
    Render(RenderArgs),
    /// Print the recipe a set of flags produces, without rendering
    Recipe(RenderArgs),
    /// Show what's available: styles, gradients, fonts
    Show {
        /// styles | gradients | fonts
        what: String,
        /// Sample text for style and gradient previews
        #[arg(default_value = "Aaron")]
        text: String,
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

fn render(args: &RenderArgs) -> Result<String, String> {
    let recipe = build_recipe(args)?;
    let grid = dotbanner_core::render(&recipe).map_err(|e| match e {
        engine::EngineError::FontNotFound(f) => {
            format!("no font matched '{f}' — try: dotbanner show fonts")
        }
        other => other.to_string(),
    })?;
    Ok(ansi::to_ansi(&grid))
}

fn show(what: &str, text: &str) -> Result<String, String> {
    let mut out = String::new();
    match what {
        "styles" => {
            for style in presets::STYLES {
                out.push_str(&format!("\n\x1b[2m──── {style}\x1b[0m\n"));
                let args = RenderArgs {
                    text: Some(text.to_string()),
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
                    text: Some(text.to_string()),
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
            for family in engine::list_families() {
                out.push_str(&family);
                out.push('\n');
            }
        }
        other => {
            return Err(format!(
                "don't know how to show '{other}' (styles, gradients, fonts)"
            ))
        }
    }
    Ok(out)
}

fn run() -> Result<String, String> {
    let cli = Cli::parse();
    match &cli.command {
        Command::Render(args) => render(args),
        Command::Recipe(args) => Ok(build_recipe(args)?.to_json() + "\n"),
        Command::Show { what, text } => show(what, text),
    }
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
