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
    #[serde(default = "default_tracking", deserialize_with = "finite_tracking")]
    pub tracking: f32,
}

/// Reject a non-finite tracking: serde_json writes NaN and infinity as
/// `null`, which the tool then cannot read back.
fn finite_tracking<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
    use serde::de::Error as _;
    let v = f32::deserialize(d)?;
    if v.is_finite() {
        Ok(v)
    } else {
        Err(D::Error::custom("tracking must be a finite number"))
    }
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

/// The wire shape of a recipe. It exists so the deprecated top-level `rows`
/// can be folded into `size` at load, leaving the domain type with exactly
/// one height field — a second one invites the two disagreeing.
#[derive(Deserialize)]
struct RecipeWire {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default)]
    name: Option<String>,
    text: String,
    #[serde(default)]
    font: Font,
    /// Deprecated spelling of `size.rows`, read so recipes written before
    /// `size` existed keep loading (ADR-200).
    #[serde(default)]
    rows: Option<usize>,
    #[serde(default)]
    size: Option<Size>,
    #[serde(default)]
    pipeline: Vec<Stage>,
    #[serde(default)]
    symbolizer: SymbolizerSpec,
}

impl From<RecipeWire> for Recipe {
    fn from(w: RecipeWire) -> Self {
        // An explicit `size` wins; otherwise the legacy height applies.
        let mut size = w.size.unwrap_or_default();
        if w.size.is_none() {
            if let Some(rows) = w.rows {
                size.rows = rows;
            }
        }
        Recipe {
            version: w.version,
            name: w.name,
            text: w.text,
            font: w.font,
            size,
            pipeline: w.pipeline,
            symbolizer: w.symbolizer,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "RecipeWire")]
pub struct Recipe {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub text: String,
    #[serde(default)]
    pub font: Font,
    #[serde(default)]
    pub size: Size,
    #[serde(default)]
    pub pipeline: Vec<Stage>,
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
            size: Size::default(),
            pipeline: vec![Op::Fill {
                inset: 0,
                kind: Fill::Solid {
                    color: Rgb::new(0xff, 0xff, 0xff),
                },
                register: None,
                on_top: false,
            }
            .into()],
            symbolizer: SymbolizerSpec::default(),
        }
    }

    /// The banner's height in terminal rows.
    pub fn rows(&self) -> usize {
        self.size.rows
    }

    /// The ops this build can render, in order.
    pub fn ops(&self) -> impl Iterator<Item = &Op> {
        self.pipeline.iter().filter_map(|s| s.op())
    }

    /// Names of the stages this build cannot render.
    pub fn unknown_ops(&self) -> Vec<String> {
        self.pipeline
            .iter()
            .filter_map(|s| s.unknown_name())
            .collect()
    }

    /// True when the file declares a schema newer than this build reads.
    pub fn is_newer_than_this_build(&self) -> bool {
        self.version > SCHEMA_VERSION
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

/// One entry in a recipe's pipeline: an op this build understands, or one
/// it does not.
///
/// A recipe is a shared document, so a file written by a newer dotbanner
/// reaches an older one. An unknown effect makes that one layer
/// unrenderable, not the whole banner — the raw JSON is kept so the recipe
/// survives a round-trip through a build that cannot draw it (ADR-202).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Stage {
    Known(Op),
    Unknown(serde_json::Value),
}

/// Op names this build renders. A stage naming one of these must parse as
/// that op or fail loudly; only an unrecognised name degrades.
const KNOWN_OPS: &[&str] = &["fill", "rim", "cast", "edge"];

/// Fill kinds this build paints with. A newer kind degrades the layer the
/// same way a newer op does; a misspelled *field* inside a known kind does
/// not, because that is a mistake rather than a newer schema.
const KNOWN_FILLS: &[&str] = &["solid", "band"];

impl<'de> Deserialize<'de> for Stage {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let value = serde_json::Value::deserialize(d)?;
        let tag = value.get("op").and_then(|t| t.as_str()).unwrap_or_default();
        let kind = value.get("kind").and_then(|k| k.as_str());
        let kind_known = kind.is_none_or(|k| KNOWN_FILLS.contains(&k));
        if KNOWN_OPS.contains(&tag) && kind_known {
            // This build knows the op, so a problem inside it is a mistake in
            // the recipe, not a newer schema. Report it rather than dropping
            // the layer (ADR-202).
            let op = Op::deserialize(&value).map_err(D::Error::custom)?;
            return Ok(Stage::Known(op));
        }
        Ok(Stage::Unknown(value))
    }
}

impl From<Op> for Stage {
    fn from(op: Op) -> Self {
        Stage::Known(op)
    }
}

impl Stage {
    /// The op, when this build understands it.
    pub fn op(&self) -> Option<&Op> {
        match self {
            Stage::Known(op) => Some(op),
            Stage::Unknown(_) => None,
        }
    }

    /// The `op` name of a stage this build cannot render, for a warning.
    pub fn unknown_name(&self) -> Option<String> {
        match self {
            Stage::Unknown(v) => Some(
                v.get("op")
                    .and_then(|t| t.as_str())
                    .unwrap_or("(no op field)")
                    .to_string(),
            ),
            Stage::Known(_) => None,
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
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
    /// A register this build does not know, keeping its name so saving the
    /// recipe does not rewrite it (ADR-202). The layer still paints, in the
    /// default register.
    Unknown(String),
}

impl Register {
    /// The name as it appears on the wire.
    pub fn as_str(&self) -> &str {
        match self {
            Register::Blocks => "blocks",
            Register::Braille => "braille",
            Register::Sextants => "sextants",
            Register::Facets => "facets",
            Register::Unknown(name) => name,
        }
    }
}

impl Serialize for Register {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Register {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let name = String::deserialize(d)?;
        Ok(match name.as_str() {
            "blocks" => Register::Blocks,
            "braille" => Register::Braille,
            "sextants" => Register::Sextants,
            "facets" => Register::Facets,
            _ => Register::Unknown(name),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
            size: Size {
                rows: 8,
                fit: None,
                tracking: default_tracking(),
            },
            pipeline: vec![
                Stage::Known(Op::Fill {
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
                }),
                Stage::Known(Op::Rim {
                    width: 5,
                    kind: Fill::Solid {
                        color: Rgb::parse("#e8f6ff").unwrap(),
                    },
                    register: None,
                    on_top: false,
                }),
                Stage::Known(Op::Cast {
                    spread: 2,
                    dx: 2,
                    dy: 2,
                    kind: Fill::Solid {
                        color: Rgb::parse("#101020").unwrap(),
                    },
                    register: Some(Register::Braille),
                    on_top: false,
                }),
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
    fn an_unknown_op_loses_its_layer_not_the_recipe() {
        // A newer dotbanner's effect reaches an older build (ADR-202).
        let json = r##"{"text":"x","pipeline":[
            {"op":"fill","kind":"solid","color":"#ffffff"},
            {"op":"warp","amplitude":3}]}"##;
        let r = Recipe::from_json(json).expect("the recipe still loads");
        assert_eq!(r.ops().count(), 1, "the known op renders");
        assert_eq!(r.unknown_ops(), vec!["warp"], "the unknown one is named");
    }

    #[test]
    fn an_unknown_op_survives_a_round_trip() {
        // An older build must not destroy an effect it cannot draw.
        let json = r##"{"text":"x","pipeline":[{"op":"warp","amplitude":3}]}"##;
        let r = Recipe::from_json(json).unwrap();
        let back = Recipe::from_json(&r.to_json()).unwrap();
        assert_eq!(back.unknown_ops(), vec!["warp"]);
        assert!(
            r.to_json().contains("amplitude"),
            "the op's fields are kept"
        );
    }

    #[test]
    fn an_unknown_fill_kind_degrades_the_same_way() {
        let json = r##"{"text":"x","pipeline":[{"op":"fill","kind":"radial","color":"#ffffff"}]}"##;
        let r = Recipe::from_json(json).expect("still loads");
        assert_eq!(r.ops().count(), 0);
    }

    #[test]
    fn a_mistake_inside_a_known_op_is_an_error_not_a_shrug() {
        // A misspelled field must not silently drop the layer and blame an
        // op this build plainly has (ADR-202).
        let json = r##"{"text":"x","pipeline":[{"op":"fill","kind":"solid","colour":"#ff0000"}]}"##;
        assert!(
            Recipe::from_json(json).is_err(),
            "the typo must be reported"
        );
    }

    #[test]
    fn a_bad_colour_inside_a_known_op_is_an_error() {
        let json = r##"{"text":"x","pipeline":[{"op":"fill","kind":"solid","color":"#gggggg"}]}"##;
        assert!(Recipe::from_json(json).is_err());
    }

    #[test]
    fn an_unknown_register_keeps_its_name_on_save() {
        let json = r##"{"text":"x","pipeline":[
            {"op":"fill","kind":"solid","color":"#ffffff","register":"hexants"}]}"##;
        let r = Recipe::from_json(json).unwrap();
        assert!(
            r.to_json().contains("hexants"),
            "saving must not rewrite a register this build cannot draw"
        );
    }

    #[test]
    fn a_legacy_rows_survives_a_round_trip() {
        // Every shipped preset uses the top-level spelling.
        let r = Recipe::from_json(r#"{"text":"x","rows":12}"#).unwrap();
        assert_eq!(r.rows(), 12);
        assert_eq!(Recipe::from_json(&r.to_json()).unwrap().rows(), 12);
    }

    #[test]
    fn an_explicit_size_wins_over_legacy_rows() {
        let r = Recipe::from_json(r#"{"text":"x","rows":14,"size":{"rows":6}}"#).unwrap();
        assert_eq!(r.rows(), 6);
    }

    #[test]
    fn a_non_finite_tracking_is_rejected() {
        assert!(Recipe::from_json(r#"{"text":"x","size":{"tracking":null}}"#).is_err());
    }

    #[test]
    fn an_unknown_register_keeps_the_layer() {
        let json = r##"{"text":"x","pipeline":[
            {"op":"fill","kind":"solid","color":"#ffffff","register":"hexants"}]}"##;
        let r = Recipe::from_json(json).expect("still loads");
        assert_eq!(r.ops().count(), 1, "the layer still paints");
    }

    #[test]
    fn a_newer_schema_version_is_detectable() {
        let r = Recipe::from_json(r#"{"version":99,"text":"x"}"#).unwrap();
        assert!(r.is_newer_than_this_build());
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
            r.ops().collect::<Vec<_>>(),
            vec![&Op::Fill {
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
        assert!(matches!(r.ops().next(), Some(Op::Rim { width: 3, .. })));
    }

    #[test]
    fn cast_defaults_to_no_offset() {
        let json = r##"{"text":"x","pipeline":[
            {"op":"cast","kind":"solid","color":"#000000","register":"braille"}]}"##;
        let r = Recipe::from_json(json).unwrap();
        assert!(matches!(
            r.ops().next(),
            Some(Op::Cast {
                spread: 1,
                dx: 0,
                dy: 0,
                register: Some(Register::Braille),
                ..
            })
        ));
    }
}
