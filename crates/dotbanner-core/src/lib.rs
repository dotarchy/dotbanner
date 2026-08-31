//! dotbanner-core: the recipe document, render pipeline, and symbolizer.
//!
//! A recipe (JSON) describes text + font + a pipeline of image-space effect
//! ops + a symbolizer spec. The engine rasterizes, applies effects, and the
//! symbolizer maps pixels to terminal cells. Every output format is a sink
//! over the symbolized stream.

pub mod symbolizer;
