//! Built-in gradient presets and style recipes (ADR-200: presets are
//! recipes). These seed `presets/` on disk and back the CLI's named styles.

use crate::color::Rgb;
use crate::recipe::{Fill, Op, Register};

/// Look a palette up by name, returning its ramp. Built-in and installed
/// schemes resolve through the same path (ADR-201), so a file dropped into a
/// scheme directory shadows a shipped palette of the same name.
pub fn gradient(name: &str) -> Option<Vec<Rgb>> {
    crate::scheme::find(name)
        .map(|s| s.ramp())
        .filter(|r| !r.is_empty())
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

/// How thick a style's edge treatment is, in mask pixels. Each style spends
/// it on whatever edge it has — a rim's width, a cast's spread, an edge
/// band's reach — so one knob means "how strong is the effect" across all
/// of them.
pub const DEFAULT_WEIGHT: u32 = 1;

/// The pipeline for a named style at the default weight.
pub fn style_pipeline(style: &str, colors: &[Rgb]) -> Option<Vec<Op>> {
    style_pipeline_weighted(style, colors, DEFAULT_WEIGHT)
}

/// The pipeline for a named style, given resolved colors and a weight.
pub fn style_pipeline_weighted(style: &str, colors: &[Rgb], weight: u32) -> Option<Vec<Op>> {
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
        // Weight zero means no edge treatment, so trap becomes its body.
        "trap" if weight == 0 => Some(vec![Op::Fill {
            inset: 0,
            kind: Fill::Solid {
                color: colors.last().copied().unwrap_or(first),
            },
            register: None,
            on_top: false,
        }]),
        "trap" => Some(trap_pipeline(colors, weight)),
        // The prototype's braille-behind-blocks shadow, native now: a cast
        // offset down-right in braille, under a gradient body.
        "shadow" => Some(vec![
            Op::Cast {
                spread: weight,
                dx: (2 * weight).min(i32::MAX as u32) as i32,
                dy: (2 * weight).min(i32::MAX as u32) as i32,
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
                spread: 3 * weight,
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
                spread: 3 * weight,
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
                outer: weight,
                inner: 4 * weight,
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
                outer: 3 * weight,
                inner: 3 * weight,
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
    fn every_shipped_palette_resolves() {
        for s in crate::scheme::built_in() {
            assert!(
                gradient(&s.name).is_some_and(|r| r.len() >= 3),
                "{} did not resolve to a ramp",
                s.name
            );
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
    fn an_outward_style_does_not_inflate_the_requested_height() {
        // Padding for outward effects lands in the finished banner, so the
        // height search has to account for it or `rows` is only a hint.
        use crate::recipe::{Recipe, Size};
        let colors = gradient("fire").unwrap();
        for style in ["plain", "shadow", "glow", "sticker", "halo"] {
            let mut r = Recipe::new("Hello");
            r.size = Size {
                rows: 8,
                ..Size::default()
            };
            r.pipeline = style_pipeline(style, &colors)
                .unwrap()
                .into_iter()
                .map(Into::into)
                .collect();
            let grid = crate::render(&r).expect("renders");
            assert_eq!(grid.rows(), 8, "{style} rendered {} rows", grid.rows());
        }
    }

    #[test]
    fn weight_zero_means_no_edge_treatment() {
        let colors = gradient("omarchy").unwrap();
        let none = style_pipeline_weighted("trap", &colors, 0).unwrap();
        let some = style_pipeline_weighted("trap", &colors, 1).unwrap();
        assert_ne!(none, some, "weight 0 must not silently behave as 1");
        assert_eq!(none.len(), 1, "no rim at weight 0");
    }

    #[test]
    fn weight_thickens_every_style_that_has_an_edge() {
        let colors = gradient("omarchy").unwrap();
        for style in STYLES {
            let thin = style_pipeline_weighted(style, &colors, 1).unwrap();
            let thick = style_pipeline_weighted(style, &colors, 3).unwrap();
            if *style == "plain" || *style == "band" || *style == "gradient" {
                assert_eq!(thin, thick, "{style} has no edge to thicken");
            } else {
                assert_ne!(thin, thick, "{style} should respond to weight");
            }
        }
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
