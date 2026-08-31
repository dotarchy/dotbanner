//! dotbanner-core: the recipe document, render pipeline, and symbolizer.
//!
//! A recipe (JSON) describes text + font + a pipeline of image-space effect
//! ops + a symbolizer spec. The engine rasterizes, applies effects, and the
//! symbolizer maps pixels to terminal cells. Every output format is a sink
//! over the symbolized stream.

// Re-exported so a caller can inspect a recipe parse failure (line,
// column, message) without taking its own serde_json dependency.
pub use serde_json;

pub mod color;
pub mod engine;
pub mod formats;
pub mod presets;
pub mod recipe;
pub mod scheme;
pub mod symbolizer;

use recipe::{Recipe, Register};
use symbolizer::{CellGrid, SymbolSet};

/// Run a recipe end to end: rasterize, apply effects, symbolize.
pub fn render(recipe: &Recipe) -> Result<CellGrid, engine::EngineError> {
    let layers = engine::render(recipe)?;
    let set = match recipe.symbolizer.body {
        Register::Blocks => SymbolSet::Blocks,
        Register::Braille => SymbolSet::Braille,
        Register::Facets => SymbolSet::Facets,
        Register::Unknown(_) => SymbolSet::Blocks,
        Register::Sextants => SymbolSet::Sextants,
    };
    Ok(symbolizer::symbolize_layers(&layers, set))
}
