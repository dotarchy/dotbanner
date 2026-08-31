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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recipe {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub text: String,
    #[serde(default)]
    pub font: Font,
    #[serde(default = "default_rows")]
    pub rows: usize,
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
            rows: default_rows(),
            pipeline: vec![Op::Fill {
                kind: Fill::Solid {
                    color: Rgb::new(0xff, 0xff, 0xff),
                },
            }],
            symbolizer: SymbolizerSpec::default(),
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

/// A pipeline stage. Every named style is a composition of these (ADR-300).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Op {
    /// Paint the glyph body.
    Fill {
        #[serde(flatten)]
        kind: Fill,
    },
    /// Paint an eroded edge band in its own color.
    Rim {
        #[serde(default = "default_erode")]
        erode: u32,
        color: Rgb,
    },
}

fn default_erode() -> u32 {
    2
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
            rows: 8,
            pipeline: vec![
                Op::Fill {
                    kind: Fill::Band {
                        stops: vec![
                            Rgb::parse("#f8ffff").unwrap(),
                            Rgb::parse("#8a2fc8").unwrap(),
                        ],
                        steps: Some(10),
                    },
                },
                Op::Rim {
                    erode: 5,
                    color: Rgb::parse("#e8f6ff").unwrap(),
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
        assert_eq!(r.rows, 8);
        assert_eq!(r.symbolizer.body, Register::Blocks);
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
                kind: Fill::Solid {
                    color: Rgb::new(255, 0, 0)
                }
            }]
        );
    }
}
