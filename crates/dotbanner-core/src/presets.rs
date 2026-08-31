//! Built-in gradient presets and style recipes (ADR-200: presets are
//! recipes). These seed `presets/` on disk and back the CLI's named styles.

use crate::color::Rgb;
use crate::recipe::{Fill, Op, Register};

/// A named gradient: the palettes the prototype's `show gradients` offered.
pub struct Gradient {
    pub name: &'static str,
    pub stops: &'static [&'static str],
}

pub const GRADIENTS: &[Gradient] = &[
    Gradient {
        name: "omarchy",
        stops: &["#f8ffff", "#a8ecfa", "#3f7fe8", "#8a2fc8"],
    },
    Gradient {
        name: "fire",
        stops: &["#fff8d8", "#ffd21f", "#ff5f1f", "#8a0f0f"],
    },
    Gradient {
        name: "synthwave",
        stops: &["#f8f8ff", "#ff6ec7", "#8a2be2", "#1a0a3e"],
    },
    Gradient {
        name: "mint",
        stops: &["#f0fff8", "#7fffd4", "#20b2aa", "#0a4f4f"],
    },
    Gradient {
        name: "ember",
        stops: &["#2b0a02", "#8a0f0f", "#ff5f1f", "#ffd21f"],
    },
    Gradient {
        name: "steel",
        stops: &["#f8fbff", "#c8d4e0", "#5a7088", "#1a2530"],
    },
];

/// Palettes borrowed from the editor and terminal themes people already run,
/// so a banner can match the rest of a setup. Each is an ordered ramp
/// through the theme's signature colours rather than its full palette.
pub const SCHEMES: &[Gradient] = &[
    Gradient {
        name: "monokai",
        stops: &["#f92672", "#fd971f", "#e6db74", "#a6e22e", "#66d9ef"],
    },
    Gradient {
        name: "gruvbox",
        stops: &["#fbf1c7", "#fabd2f", "#fe8019", "#cc241d", "#282828"],
    },
    Gradient {
        name: "nord",
        stops: &["#eceff4", "#88c0d0", "#81a1c1", "#5e81ac", "#2e3440"],
    },
    Gradient {
        name: "dracula",
        stops: &["#f8f8f2", "#8be9fd", "#bd93f9", "#ff79c6", "#282a36"],
    },
    Gradient {
        name: "catppuccin",
        stops: &["#cdd6f4", "#89b4fa", "#cba6f7", "#f5c2e7", "#1e1e2e"],
    },
    Gradient {
        name: "tokyo-night",
        stops: &["#c0caf5", "#7dcfff", "#7aa2f7", "#bb9af7", "#1a1b26"],
    },
    Gradient {
        name: "solarized",
        stops: &["#fdf6e3", "#b58900", "#cb4b16", "#268bd2", "#002b36"],
    },
    Gradient {
        name: "everforest",
        stops: &["#d3c6aa", "#a7c080", "#83c092", "#7fbbb3", "#2d353b"],
    },
    Gradient {
        name: "rose-pine",
        stops: &["#e0def4", "#ebbcba", "#eb6f92", "#c4a7e7", "#191724"],
    },
    Gradient {
        name: "kanagawa",
        stops: &["#dcd7ba", "#7e9cd8", "#957fb8", "#ffa066", "#1f1f28"],
    },
];

/// Every named palette: the designed ramps first, then the theme schemes.
pub fn all_presets() -> impl Iterator<Item = &'static Gradient> {
    GRADIENTS.iter().chain(SCHEMES.iter())
}

/// Look a gradient up by name, returning its parsed stops.
pub fn gradient(name: &str) -> Option<Vec<Rgb>> {
    all_presets().find(|g| g.name == name).map(|g| {
        g.stops
            .iter()
            .map(|s| Rgb::parse(s).expect("built-in gradients are valid hex"))
            .collect()
    })
}

/// Parse `--colors`: either a preset name or a comma-separated hex list.
pub fn resolve_colors(spec: &str) -> Option<Vec<Rgb>> {
    if let Some(stops) = gradient(spec) {
        return Some(stops);
    }
    // Any base16 scheme on disk resolves by name too, so a banner can match
    // the theme the rest of a setup already runs.
    if let Some(scheme) = crate::scheme::find(spec) {
        return Some(scheme.ramp());
    }
    spec.split(',')
        .map(|s| Rgb::parse(s.trim()).ok())
        .collect::<Option<Vec<_>>>()
        .filter(|v| !v.is_empty())
}

/// The pipeline for a named style, given resolved colors.
pub fn style_pipeline(style: &str, colors: &[Rgb]) -> Option<Vec<Op>> {
    let first = colors
        .first()
        .copied()
        .unwrap_or(Rgb::new(0xff, 0xff, 0xff));
    let last = colors.last().copied().unwrap_or(first);
    match style {
        "plain" => Some(vec![Op::Fill {
            inset: 0,
            kind: Fill::Solid { color: first },
            register: None,
            on_top: false,
        }]),
        "band" => Some(vec![Op::Fill {
            inset: 0,
            kind: banded(colors, Some(10)),
            register: None,
            on_top: false,
        }]),
        "gradient" => Some(vec![Op::Fill {
            inset: 0,
            kind: banded(colors, None),
            register: None,
            on_top: false,
        }]),
        "trap" => Some(trap_pipeline(colors, 1)),
        // The prototype's braille-behind-blocks shadow, native now: a cast
        // offset down-right in braille, under a gradient body.
        "shadow" => Some(vec![
            Op::Cast {
                spread: 1,
                dx: 2,
                dy: 2,
                kind: Fill::Solid { color: last },
                register: Some(Register::Braille),
                on_top: false,
            },
            Op::Fill {
                inset: 0,
                kind: banded(colors, None),
                register: None,
                on_top: false,
            },
        ]),
        // A centered braille halo: same mechanism, no offset, wider spread.
        "glow" => Some(vec![
            Op::Cast {
                spread: 3,
                dx: 0,
                dy: 0,
                kind: Fill::Solid { color: last },
                register: Some(Register::Braille),
                on_top: false,
            },
            Op::Fill {
                inset: 0,
                kind: Fill::Solid { color: first },
                register: None,
                on_top: false,
            },
        ]),
        // Thick outer outline in the body's own register.
        "sticker" => Some(vec![
            Op::Cast {
                spread: 3,
                dx: 0,
                dy: 0,
                kind: Fill::Solid { color: first },
                register: None,
                on_top: false,
            },
            Op::Fill {
                inset: 0,
                kind: banded(colors, None),
                register: None,
                on_top: false,
            },
        ]),
        // Braille stippled over the body: the dots draw on top and the body
        // colour shows through as the cell background. Close fg/bg values
        // read as texture rather than as a second shape.
        "stipple" => Some(vec![
            Op::Fill {
                inset: 0,
                kind: banded(colors, None),
                register: None,
                on_top: false,
            },
            Op::Edge {
                outer: 1,
                inner: 4,
                kind: Fill::Solid {
                    color: first.lerp(last, 0.25),
                },
                register: Some(Register::Braille),
                on_top: true,
            },
        ]),
        // A band straddling the letterform edge, fading through the palette
        // in both directions at once — outward into the ground, inward into
        // the body.
        "halo" => Some(vec![
            Op::Fill {
                inset: 0,
                kind: Fill::Solid { color: last },
                register: None,
                on_top: false,
            },
            Op::Edge {
                outer: 3,
                inner: 3,
                kind: banded(colors, None),
                register: Some(Register::Braille),
                on_top: true,
            },
        ]),
        _ => None,
    }
}

fn banded(colors: &[Rgb], steps: Option<u32>) -> Fill {
    Fill::Band {
        stops: colors.to_vec(),
        steps,
    }
}

/// Trap with an explicit width in mask pixels. Because the mask is
/// rasterized at 2 (blocks) or 4 (braille) pixels per output row, a width
/// below one cell is a genuine subpixel trap: the rim occupies part of a
/// cell's coverage rather than a whole cell.
///
/// Body first, rim painted over it: the rim's cells are a subset of the
/// body's, so it wins only at the edge. Width is clamped to at least 1 —
/// zero would erode nothing and leave no rim at all.
pub fn trap_pipeline(colors: &[Rgb], width: u32) -> Vec<Op> {
    let rim = colors
        .first()
        .copied()
        .unwrap_or(Rgb::new(0xff, 0x2d, 0x55));
    let core = colors.last().copied().unwrap_or(Rgb::new(0xff, 0xd2, 0x1f));
    vec![
        Op::Fill {
            inset: 0,
            kind: Fill::Solid { color: core },
            register: None,
            on_top: false,
        },
        Op::Rim {
            width: width.max(1),
            kind: Fill::Solid { color: rim },
            register: None,
            on_top: false,
        },
    ]
}

/// Style names the CLI accepts.
pub const STYLES: &[&str] = &[
    "plain", "band", "gradient", "trap", "shadow", "glow", "sticker", "stipple", "halo",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_gradient_parses() {
        for g in all_presets() {
            let stops = gradient(g.name).expect("named gradient resolves");
            assert_eq!(stops.len(), g.stops.len());
        }
    }

    #[test]
    fn preset_names_are_unique() {
        let mut names: Vec<&str> = all_presets().map(|g| g.name).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "two presets share a name");
    }

    #[test]
    fn schemes_resolve_like_gradients() {
        assert_eq!(resolve_colors("monokai").unwrap().len(), 5);
        assert_eq!(resolve_colors("tokyo-night").unwrap().len(), 5);
    }

    #[test]
    fn resolve_accepts_presets_and_lists() {
        assert_eq!(resolve_colors("fire").unwrap().len(), 4);
        assert_eq!(
            resolve_colors("#ff0000,#00ff00").unwrap(),
            vec![Rgb::new(255, 0, 0), Rgb::new(0, 255, 0)]
        );
        assert!(resolve_colors("not-a-color").is_none());
    }

    #[test]
    fn every_style_builds_a_pipeline() {
        let colors = gradient("omarchy").unwrap();
        for style in STYLES {
            assert!(style_pipeline(style, &colors).is_some(), "style {style}");
        }
        assert!(style_pipeline("nope", &colors).is_none());
    }

    #[test]
    fn trap_layers_rim_over_core() {
        let colors = vec![Rgb::new(255, 0, 0), Rgb::new(255, 210, 31)];
        let ops = style_pipeline("trap", &colors).unwrap();
        assert!(matches!(ops[0], Op::Fill { .. }));
        assert!(matches!(ops[1], Op::Rim { .. }));
    }
}
