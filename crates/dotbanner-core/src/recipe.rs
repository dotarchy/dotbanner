//! The recipe document (ADR-200): the one contract the TUI, CLI, presets,
//! and output sinks all read and write.
//!
//! Compatibility rule: saved recipes must keep loading. Unknown fields are
//! ignored rather than rejected, and `version` records the schema the file
//! was written against.

use serde::{Deserialize, Serialize};

use crate::color::Rgb;

/// Current recipe schema version.
pub const SCHEMA_VERSION: u32 = 1;

fn default_version() -> u32 {
    SCHEMA_VERSION
}

fn default_rows() -> usize {
    8
}

fn default_tracking() -> f32 {
    0.06
}

/// How a banner is sized. `rows` is the height in terminal rows; `fit`
/// optionally caps the width, shrinking `rows` until the render fits.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Size {
    #[serde(default = "default_rows")]
    pub rows: usize,
    /// Maximum width in terminal columns. `Fit::Terminal` measures the
    /// current terminal; `Fit::Columns(n)` pins a number; absent means the
    /// banner is whatever width the text comes out as.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit: Option<Fit>,
    /// Extra space between glyphs, as a fraction of the em. Banner text
    /// needs more air than body text once quantized to cells; condensed and
    /// monospace faces want less than the default.
    #[serde(default = "default_tracking")]
    pub tracking: f32,
}

impl Default for Size {
    fn default() -> Self {
        Self {
            rows: default_rows(),
            fit: None,
            tracking: default_tracking(),
        }
    }
}

/// A width limit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    /// Measure the terminal at render time.
    Terminal,
    /// A fixed number of columns.
    Columns(usize),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recipe {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub text: String,
    #[serde(default)]
    pub font: Font,
    /// Deprecated spelling of `size.rows`, still read so recipes written
    /// before `size` existed keep loading (ADR-200).
    #[serde(default, skip_serializing)]
    pub rows: Option<usize>,
    #[serde(default)]
    pub size: Size,
    #[serde(default)]
    pub pipeline: Vec<Op>,
    #[serde(default)]
    pub symbolizer: SymbolizerSpec,
}

impl Recipe {
    /// A plain white banner in the default font — the starting point every
    /// preset and TUI session edits.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            version: SCHEMA_VERSION,
            name: None,
            text: text.into(),
            font: Font::default(),
            rows: None,
            size: Size::default(),
            pipeline: vec![Op::Fill {
                inset: 0,
                kind: Fill::Solid {
                    color: Rgb::new(0xff, 0xff, 0xff),
                },
                register: None,
                on_top: false,
            }],
            symbolizer: SymbolizerSpec::default(),
        }
    }

    /// The effective height, preferring `size.rows` and falling back to the
    /// legacy top-level `rows`.
    pub fn rows(&self) -> usize {
        match self.rows {
            Some(n) if self.size.rows == default_rows() => n,
            _ => self.size.rows,
        }
    }

    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("recipe serializes")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Font {
    pub family: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
}

impl Default for Font {
    fn default() -> Self {
        Self {
            family: "DejaVu Sans".into(),
            style: None,
        }
    }
}

/// A pipeline stage: a region, a paint, and optionally its own symbol
/// register (ADR-201).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Op {
    /// Paint the glyph body, optionally pulled in from the edge by `inset`
    /// mask pixels. An inset layer never touches the silhouette, so the
    /// layer beneath keeps ownership of the letterform's outline.
    Fill {
        #[serde(default, skip_serializing_if = "is_zero")]
        inset: u32,
        #[serde(flatten)]
        kind: Fill,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        register: Option<Register>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        on_top: bool,
    },
    /// Paint an inner edge band: the body minus its eroded core. `width` is
    /// in mask pixels, so values below one cell are subpixel traps.
    Rim {
        #[serde(default = "default_width", alias = "erode")]
        width: u32,
        #[serde(flatten)]
        kind: Fill,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        register: Option<Register>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        on_top: bool,
    },
    /// Paint outside the body: the glyph dilated by `spread` and offset by
    /// (`dx`, `dy`), minus the body itself — a drop shadow, outline, or glow.
    Cast {
        #[serde(default = "default_width")]
        spread: u32,
        #[serde(default)]
        dx: i32,
        #[serde(default)]
        dy: i32,
        #[serde(flatten)]
        kind: Fill,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        register: Option<Register>,
        /// Draw over whatever this overlaps instead of contesting by
        /// coverage; the overlapped layer becomes the cell background.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        on_top: bool,
    },
    /// Paint a band straddling the glyph edge: `outer` pixels beyond it and
    /// `inner` pixels within. With a gradient paint it fades in both
    /// directions at once across the letterform boundary.
    Edge {
        #[serde(default = "default_width")]
        outer: u32,
        #[serde(default = "default_width")]
        inner: u32,
        #[serde(flatten)]
        kind: Fill,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        register: Option<Register>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        on_top: bool,
    },
}

fn default_width() -> u32 {
    1
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Fill {
    Solid {
        color: Rgb,
    },
    /// Vertical multi-stop gradient, optionally quantized into hard bands.
    Band {
        stops: Vec<Rgb>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        steps: Option<u32>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Register {
    #[default]
    Blocks,
    Braille,
    /// Sextants — 2×3 semigraphics (U+1FB00), finer vertical resolution.
    /// Needs a font with Symbols for Legacy Computing coverage.
    Sextants,
    /// Faceted blocks — three-quadrant patterns render as large triangles,
    /// giving edges a cut-face read rather than a step.
    Facets,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolizerSpec {
    #[serde(default)]
    pub body: Register,
}

impl Default for SymbolizerSpec {
    fn default() -> Self {
        Self {
            body: Register::Blocks,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json() {
        let r = Recipe {
            version: SCHEMA_VERSION,
            name: Some("omarchy-laser".into()),
            text: "dotarchy".into(),
            font: Font {
                family: "Pirata One".into(),
                style: Some("Regular".into()),
            },
            rows: None,
            size: Size {
                rows: 8,
                fit: None,
                tracking: default_tracking(),
            },
            pipeline: vec![
                Op::Fill {
                    inset: 0,
                    kind: Fill::Band {
                        stops: vec![
                            Rgb::parse("#f8ffff").unwrap(),
                            Rgb::parse("#8a2fc8").unwrap(),
                        ],
                        steps: Some(10),
                    },
                    register: None,
                    on_top: false,
                },
                Op::Rim {
                    width: 5,
                    kind: Fill::Solid {
                        color: Rgb::parse("#e8f6ff").unwrap(),
                    },
                    register: None,
                    on_top: false,
                },
                Op::Cast {
                    spread: 2,
                    dx: 2,
                    dy: 2,
                    kind: Fill::Solid {
                        color: Rgb::parse("#101020").unwrap(),
                    },
                    register: Some(Register::Braille),
                    on_top: false,
                },
            ],
            symbolizer: SymbolizerSpec {
                body: Register::Blocks,
            },
        };
        let back = Recipe::from_json(&r.to_json()).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn minimal_recipe_fills_defaults() {
        let r = Recipe::from_json(r#"{"text":"hi"}"#).unwrap();
        assert_eq!(r.text, "hi");
        assert_eq!(r.version, SCHEMA_VERSION);
        assert_eq!(r.rows(), 8);
        assert_eq!(r.symbolizer.body, Register::Blocks);
    }

    #[test]
    fn legacy_top_level_rows_still_loads() {
        // Recipes written before `size` existed keep working (ADR-200).
        let r = Recipe::from_json(r#"{"text":"hi","rows":14}"#).unwrap();
        assert_eq!(r.rows(), 14);
    }

    #[test]
    fn size_block_wins_and_carries_fit() {
        let json = r#"{"text":"hi","size":{"rows":6,"fit":"terminal","tracking":0.1}}"#;
        let r = Recipe::from_json(json).unwrap();
        assert_eq!(r.rows(), 6);
        assert_eq!(r.size.fit, Some(Fit::Terminal));
        assert!((r.size.tracking - 0.1).abs() < f32::EPSILON);
    }

    #[test]
    fn fit_accepts_a_column_count() {
        let r = Recipe::from_json(r#"{"text":"hi","size":{"fit":{"columns":40}}}"#).unwrap();
        assert_eq!(r.size.fit, Some(Fit::Columns(40)));
    }

    #[test]
    fn unknown_fields_are_ignored_not_rejected() {
        // Forward compatibility: a recipe written by a newer dotbanner still
        // loads here (ADR-200).
        let r = Recipe::from_json(r#"{"text":"hi","futureField":{"a":1}}"#).unwrap();
        assert_eq!(r.text, "hi");
    }

    #[test]
    fn op_tagging_is_stable() {
        let json = r##"{"text":"x","pipeline":[{"op":"fill","kind":"solid","color":"#ff0000"}]}"##;
        let r = Recipe::from_json(json).unwrap();
        assert_eq!(
            r.pipeline,
            vec![Op::Fill {
                inset: 0,
                kind: Fill::Solid {
                    color: Rgb::new(255, 0, 0)
                },
                register: None,
                on_top: false,
            }]
        );
    }

    #[test]
    fn rim_accepts_the_legacy_erode_alias() {
        // The field was named `erode` before ADR-201 renamed it `width`.
        let json = r##"{"text":"x","pipeline":[
            {"op":"rim","erode":3,"kind":"solid","color":"#ffffff"}]}"##;
        let r = Recipe::from_json(json).unwrap();
        assert!(matches!(r.pipeline[0], Op::Rim { width: 3, .. }));
    }

    #[test]
    fn cast_defaults_to_no_offset() {
        let json = r##"{"text":"x","pipeline":[
            {"op":"cast","kind":"solid","color":"#000000","register":"braille"}]}"##;
        let r = Recipe::from_json(json).unwrap();
        assert!(matches!(
            r.pipeline[0],
            Op::Cast {
                spread: 1,
                dx: 0,
                dy: 0,
                register: Some(Register::Braille),
                ..
            }
        ));
    }
}
