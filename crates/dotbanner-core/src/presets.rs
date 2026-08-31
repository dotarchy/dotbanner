//! Built-in gradient presets and style recipes (ADR-200: presets are
//! recipes). These seed `presets/` on disk and back the CLI's named styles.

use crate::color::Rgb;
use crate::recipe::{Fill, Op};

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

/// Look a gradient up by name, returning its parsed stops.
pub fn gradient(name: &str) -> Option<Vec<Rgb>> {
    GRADIENTS.iter().find(|g| g.name == name).map(|g| {
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
    match style {
        "plain" => Some(vec![Op::Fill {
            kind: Fill::Solid { color: first },
        }]),
        "band" => Some(vec![Op::Fill {
            kind: Fill::Band {
                stops: colors.to_vec(),
                steps: Some(10),
            },
        }]),
        "gradient" => Some(vec![Op::Fill {
            kind: Fill::Band {
                stops: colors.to_vec(),
                steps: None,
            },
        }]),
        "trap" => Some(trap_pipeline(colors, 1)),
        _ => None,
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
            kind: Fill::Solid { color: core },
        },
        Op::Rim {
            erode: width.max(1),
            color: rim,
        },
    ]
}

/// Style names the CLI accepts.
pub const STYLES: &[&str] = &["plain", "band", "gradient", "trap"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_gradient_parses() {
        for g in GRADIENTS {
            let stops = gradient(g.name).expect("named gradient resolves");
            assert_eq!(stops.len(), g.stops.len());
        }
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
