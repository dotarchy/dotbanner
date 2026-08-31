//! Rasterization and image-space effects (ADR-300).
//!
//! Everything here works on pixels. Text becomes a bi-level mask, effects
//! derive further masks from it, and each layer carries the paint that
//! colors it. The symbolizer is the only crossing into cell space.

use ab_glyph::{Font as _, FontRef, Glyph, PxScale, ScaleFont as _};

use crate::color::Rgb;
use crate::recipe::{Fill, Op, Recipe};
use crate::symbolizer::Mask;

/// How a layer's pixels are colored.
#[derive(Debug, Clone, PartialEq)]
pub enum Paint {
    Solid(Rgb),
    /// Vertical gradient across the mask's height, optionally quantized.
    Bands {
        stops: Vec<Rgb>,
        steps: Option<u32>,
    },
}

impl Paint {
    /// Color at a vertical position, `t` running 0.0 (top) to 1.0 (bottom).
    pub fn color_at(&self, t: f32) -> Rgb {
        match self {
            Paint::Solid(c) => *c,
            Paint::Bands { stops, steps } => {
                if stops.is_empty() {
                    return Rgb::new(0xff, 0xff, 0xff);
                }
                if stops.len() == 1 {
                    return stops[0];
                }
                let t = t.clamp(0.0, 1.0);
                let t = match steps {
                    // Quantize into hard bands: the omarchy stepped look.
                    Some(n) if *n > 1 => {
                        let n = *n as f32;
                        (t * n).floor().min(n - 1.0) / (n - 1.0)
                    }
                    _ => t,
                };
                let span = (stops.len() - 1) as f32;
                let pos = t * span;
                let i = pos.floor().min(span - 1.0) as usize;
                stops[i].lerp(stops[i + 1], pos - i as f32)
            }
        }
    }
}

/// A painted region of the render: which pixels, how they're colored, which
/// symbol register draws them (`None` follows the recipe's body), and whether
/// it draws over whatever it overlaps.
#[derive(Debug, Clone)]
pub struct Layer {
    pub mask: Mask,
    pub paint: Paint,
    pub register: Option<crate::symbolizer::SymbolSet>,
    /// When set, this layer draws the glyph in any cell it touches and pushes
    /// the layer it overlaps into the cell background. A braille layer marked
    /// this way stipples its dots over a solid body, the body's own color
    /// showing through as the ground.
    pub on_top: bool,
}

#[derive(Debug)]
pub enum EngineError {
    /// No family matched; carries the closest names so a caller can suggest
    /// something quotable.
    FontNotFound {
        query: String,
        near: Vec<String>,
    },
    /// Several families matched loosely; the caller should ask which.
    FontAmbiguous {
        query: String,
        matches: Vec<String>,
    },
    FontUnreadable(String),
    EmptyRender,
    /// The family exists but has no face in the requested style.
    StyleNotFound {
        family: String,
        style: String,
        available: Vec<String>,
    },
    /// The requested size would need more pixels than is sane to allocate.
    TooLarge {
        rows: usize,
        pixels: u64,
    },
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::FontNotFound { query, .. } => write!(f, "no font matched '{query}'"),
            EngineError::FontAmbiguous { query, .. } => {
                write!(f, "'{query}' matches several families")
            }
            EngineError::StyleNotFound { family, style, .. } => {
                write!(f, "'{family}' has no '{style}' face")
            }
            EngineError::FontUnreadable(p) => write!(f, "could not read font: {p}"),
            EngineError::EmptyRender => write!(f, "text rendered to nothing"),
            EngineError::TooLarge { rows, pixels } => {
                write!(f, "{rows} rows needs {pixels} pixels — too large to render")
            }
        }
    }
}

impl std::error::Error for EngineError {}

/// Locate a font file by family (and optional style) using the system font
/// database. Returns the file bytes and the face index within it.
/// Fold a family name for loose comparison: lowercase, no spaces, hyphens
/// or underscores. "JetBrains Mono" and "jetbrainsmono" fold alike.
fn fold(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_')
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Locate a font file by family (and optional style) using the system font
/// database. A path loads that file directly. Family matching is exact
/// first, then case- and separator-insensitive, then substring — so
/// `jetbrains` finds "JetBrains Mono" without the caller guessing the exact
/// spelling or quoting.
pub fn load_font(family: &str, style: Option<&str>) -> Result<(Vec<u8>, u32), EngineError> {
    // A path loads the file directly, so a font can be tried without being
    // installed system-wide.
    let path = std::path::Path::new(family);
    if path.is_file() {
        let bytes =
            std::fs::read(path).map_err(|_| EngineError::FontUnreadable(family.to_string()))?;
        return Ok((bytes, 0));
    }

    let mut db = fontdb::Database::new();
    db.load_system_fonts();

    // Resolve the query to one canonical family name.
    let wanted = fold(family);
    let mut exact: Option<String> = None;
    let mut folded: Vec<String> = Vec::new();
    let mut partial: Vec<String> = Vec::new();
    for face in db.faces() {
        for (name, _) in &face.families {
            if name.eq_ignore_ascii_case(family) {
                exact = Some(name.clone());
            } else if fold(name) == wanted {
                folded.push(name.clone());
            } else if fold(name).contains(&wanted) {
                partial.push(name.clone());
            }
        }
    }
    folded.sort();
    folded.dedup();
    partial.sort();
    partial.dedup();

    // When the shortest candidate is a prefix of every other, it is the base
    // family and the rest are its variants — "jetbrains" means "JetBrains
    // Mono", not an ambiguity with "JetBrains Mono NL".
    let narrow = |v: &mut Vec<String>| {
        if v.len() > 1 {
            let shortest = v.iter().min_by_key(|n| n.len()).cloned().unwrap();
            if v.iter().all(|n| fold(n).starts_with(&fold(&shortest))) {
                *v = vec![shortest];
            }
        }
    };
    narrow(&mut folded);
    narrow(&mut partial);

    let resolved = match exact {
        Some(name) => name,
        None if folded.len() == 1 => folded.remove(0),
        None if !folded.is_empty() => {
            return Err(EngineError::FontAmbiguous {
                query: family.to_string(),
                matches: folded,
            })
        }
        None if partial.len() == 1 => partial.remove(0),
        None if !partial.is_empty() => {
            return Err(EngineError::FontAmbiguous {
                query: family.to_string(),
                matches: partial.into_iter().take(12).collect(),
            })
        }
        None => {
            // Nothing contained the query; offer families sharing its first
            // few letters as a starting point.
            let head: String = wanted.chars().take(3).collect();
            let mut near: Vec<String> = db
                .faces()
                .flat_map(|f| f.families.iter().map(|(n, _)| n.clone()))
                .filter(|n| !head.is_empty() && fold(n).starts_with(&head))
                .collect();
            near.sort();
            near.dedup();
            near.truncate(8);
            return Err(EngineError::FontNotFound {
                query: family.to_string(),
                near,
            });
        }
    };

    // Sort the family's faces before choosing: fontdb yields them in
    // filesystem scan order, so an unsorted fallback would pick a different
    // face on another machine and break the reproducibility ADR-400 needs.
    let mut faces: Vec<&fontdb::FaceInfo> = db
        .faces()
        .filter(|f| f.families.iter().any(|(n, _)| n == &resolved))
        .collect();
    faces.sort_by(|a, b| a.post_script_name.cmp(&b.post_script_name));

    let want_style = style.map(str::to_ascii_lowercase);
    let chosen = match &want_style {
        Some(s) => faces.iter().find(|f| {
            let post = f.post_script_name.to_ascii_lowercase();
            post.contains(&fold(s)) || post.contains(s.as_str())
        }),
        None => faces
            .iter()
            .find(|f| f.weight == fontdb::Weight::NORMAL && f.style == fontdb::Style::Normal)
            .or_else(|| faces.first()),
    };
    // A style that matches nothing is a mistake worth reporting: rendering
    // some other face silently is how a recipe stops being reproducible.
    let face = *chosen.ok_or_else(|| EngineError::StyleNotFound {
        family: resolved.clone(),
        style: want_style.clone().unwrap_or_default(),
        available: faces.iter().map(|f| f.post_script_name.clone()).collect(),
    })?;
    match &face.source {
        fontdb::Source::File(path) => {
            let bytes = std::fs::read(path)
                .map_err(|_| EngineError::FontUnreadable(path.display().to_string()))?;
            Ok((bytes, face.index))
        }
        fontdb::Source::Binary(data) | fontdb::Source::SharedFile(_, data) => {
            Ok((data.as_ref().as_ref().to_vec(), face.index))
        }
    }
}

/// The terminal's width in columns, falling back to a conventional 80 when
/// it cannot be measured (a pipe, a dumb terminal).
pub fn terminal_columns() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or_else(|| {
            // `tput cols` consults the terminfo database and the tty, which
            // works even when COLUMNS is not exported.
            std::process::Command::new("tput")
                .arg("cols")
                .stderr(std::process::Stdio::null())
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .and_then(|s| s.trim().parse().ok())
                .filter(|n: &usize| *n > 0)
                .unwrap_or(80)
        })
}

/// List available font families, sorted and deduplicated.
pub fn list_families() -> Vec<String> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let mut names: Vec<String> = db
        .faces()
        .filter_map(|f| f.families.first().map(|(n, _)| n.clone()))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Rasterize a line of text into a bi-level mask, trimmed to its ink.
///
/// `px` is the em size in pixels; coverage at or above `threshold` (0.0–1.0)
/// counts as set. `tracking` adds extra pixels between glyphs — banner text
/// needs more air than body text once quantized to cells.
pub fn rasterize_tracked(
    font_bytes: &[u8],
    face_index: u32,
    text: &str,
    px: f32,
    threshold: f32,
    tracking: f32,
) -> Result<Mask, EngineError> {
    let font = FontRef::try_from_slice_and_index(font_bytes, face_index)
        .map_err(|_| EngineError::FontUnreadable("invalid font data".into()))?;
    let scaled = font.as_scaled(PxScale::from(px));

    // Lay out glyphs on a baseline, accumulating coverage into a float buffer.
    let ascent = scaled.ascent();
    let width = {
        let mut w = 0.0f32;
        let mut prev: Option<char> = None;
        for c in text.chars() {
            if let Some(p) = prev {
                w += scaled.kern(font.glyph_id(p), font.glyph_id(c)) + tracking;
            }
            w += scaled.h_advance(font.glyph_id(c));
            prev = Some(c);
        }
        w.ceil() as usize + 2
    };
    let height = (scaled.height().ceil() as usize) + 2;
    // The empty case is caught after thresholding, where an uninked mask is
    // actually detectable; width and height here are always at least 2.
    // Refuse before allocating: an unbounded row count otherwise aborts the
    // process on a failed allocation rather than returning an error.
    const MAX_PIXELS: u64 = 64 << 20;
    let pixels = width as u64 * height as u64;
    if pixels > MAX_PIXELS {
        return Err(EngineError::TooLarge {
            rows: (px * 0.72 / 12.0).round() as usize,
            pixels,
        });
    }
    let mut cov = vec![0.0f32; width * height];

    let mut caret = 1.0f32;
    let mut prev: Option<char> = None;
    for c in text.chars() {
        let id = font.glyph_id(c);
        if let Some(p) = prev {
            caret += scaled.kern(font.glyph_id(p), id) + tracking;
        }
        let glyph: Glyph =
            id.with_scale_and_position(PxScale::from(px), ab_glyph::point(caret, ascent + 1.0));
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, c| {
                let x = bounds.min.x as i32 + gx as i32;
                let y = bounds.min.y as i32 + gy as i32;
                if x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height {
                    let i = y as usize * width + x as usize;
                    cov[i] = cov[i].max(c);
                }
            });
        }
        caret += scaled.h_advance(id);
        prev = Some(c);
    }

    // Threshold, then trim to the inked bounding box.
    let set = |x: usize, y: usize| cov[y * width + x] >= threshold;
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
    for y in 0..height {
        for x in 0..width {
            if set(x, y) {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    if x0 == usize::MAX {
        return Err(EngineError::EmptyRender);
    }
    let mut mask = Mask::new(x1 - x0 + 1, y1 - y0 + 1);
    for y in y0..=y1 {
        for x in x0..=x1 {
            if set(x, y) {
                mask.set(x - x0, y - y0, true);
            }
        }
    }
    Ok(mask)
}

/// Rasterize with default tracking.
pub fn rasterize(
    font_bytes: &[u8],
    face_index: u32,
    text: &str,
    px: f32,
    threshold: f32,
) -> Result<Mask, EngineError> {
    rasterize_tracked(font_bytes, face_index, text, px, threshold, 0.0)
}

/// Erode a mask by `radius` using a square structuring element: a pixel
/// survives only if every pixel within the radius is set.
pub fn erode(mask: &Mask, radius: u32) -> Mask {
    if radius == 0 {
        return mask.clone();
    }
    let r = radius as i64;
    let mut out = Mask::new(mask.width(), mask.height());
    for y in 0..mask.height() {
        for x in 0..mask.width() {
            let mut keep = true;
            'window: for dy in -r..=r {
                for dx in -r..=r {
                    let nx = x as i64 + dx;
                    let ny = y as i64 + dy;
                    if nx < 0
                        || ny < 0
                        || nx >= mask.width() as i64
                        || ny >= mask.height() as i64
                        || !mask.get(nx as usize, ny as usize)
                    {
                        keep = false;
                        break 'window;
                    }
                }
            }
            if keep {
                out.set(x, y, true);
            }
        }
    }
    out
}

/// Dilate a mask by `radius`: a pixel is set if any pixel within the radius
/// is set. Optionally shifts the result by (`dx`, `dy`) pixels, which is how
/// a cast becomes an offset drop shadow rather than a centered glow.
pub fn dilate_offset(mask: &Mask, radius: u32, dx: i32, dy: i32) -> Mask {
    let r = radius as i64;
    let mut out = Mask::new(mask.width(), mask.height());
    for y in 0..mask.height() {
        for x in 0..mask.width() {
            let sx = x as i64 - dx as i64;
            let sy = y as i64 - dy as i64;
            let mut on = false;
            'window: for wy in -r..=r {
                for wx in -r..=r {
                    let (nx, ny) = (sx + wx, sy + wy);
                    if nx >= 0
                        && ny >= 0
                        && nx < mask.width() as i64
                        && ny < mask.height() as i64
                        && mask.get(nx as usize, ny as usize)
                    {
                        on = true;
                        break 'window;
                    }
                }
            }
            if on {
                out.set(x, y, true);
            }
        }
    }
    out
}

/// Pixels in `a` that are not in `b`.
pub fn difference(a: &Mask, b: &Mask) -> Mask {
    let mut out = Mask::new(a.width(), a.height());
    for y in 0..a.height() {
        for x in 0..a.width() {
            if a.get(x, y) && !b.get(x, y) {
                out.set(x, y, true);
            }
        }
    }
    out
}

/// Run a recipe's pipeline, producing painted layers in draw order (later
/// layers paint over earlier ones).
pub fn render(recipe: &Recipe) -> Result<Vec<Layer>, EngineError> {
    let (bytes, index) = load_font(&recipe.font.family, recipe.font.style.as_deref())?;
    // Registers share a 2×12 pixel cell footprint (ADR-201), so the mask
    // rasterizes at 12 pixel rows per output row. Em size overshoots ink
    // height, so scale by a typical cap-height ratio.
    // Outward ops paint beyond the letterform, and the rasterized mask is
    // trimmed to its ink, so the canvas is padded by the furthest any op
    // reaches before the pipeline runs.
    let pad = recipe
        .ops()
        .map(|op| match op {
            Op::Cast { spread, dx, dy, .. } => {
                *spread as usize + dx.unsigned_abs() as usize + dy.unsigned_abs() as usize
            }
            Op::Edge { outer, .. } => *outer as usize,
            _ => 0,
        })
        .max()
        .unwrap_or(0);

    let requested = recipe.rows().max(1);
    let limit = match recipe.size.fit {
        Some(crate::recipe::Fit::Columns(n)) => Some(n.max(1)),
        Some(crate::recipe::Fit::Terminal) => Some(terminal_columns()),
        None => None,
    };

    let raster = |rows: usize| -> Result<Mask, EngineError> {
        let px = (rows as f32 * 12.0) / 0.72;
        // Banner text needs air between glyphs: natural side bearings
        // quantize away at these sizes and letters collide.
        rasterize_tracked(
            &bytes,
            index,
            &recipe.text,
            px,
            0.5,
            px * recipe.size.tracking,
        )
    };

    let mut base = raster(requested)?;
    if let Some(cols) = limit {
        // Width scales with the row count, so solve for the rows that fit
        // rather than stepping down one at a time. One correction pass
        // absorbs the rounding.
        let mut rows = requested;
        for _ in 0..2 {
            let have = base.width().div_ceil(6);
            if have <= cols {
                break;
            }
            let scaled = (rows * cols) / have.max(1);
            let next = scaled.clamp(1, rows.saturating_sub(1));
            if next == rows {
                break;
            }
            rows = next;
            base = raster(rows)?;
        }
        // The estimate can still land a column or two over; walk the
        // remainder down, which is now a step or two rather than hundreds.
        while base.width().div_ceil(6) > cols && rows > 1 {
            rows -= 1;
            base = raster(rows)?;
        }
    }
    let base = base.padded(pad);

    let mut layers = Vec::new();
    for op in recipe.ops() {
        let (mask, kind, register, on_top) = match op {
            Op::Fill {
                inset,
                kind,
                register,
                on_top,
            } => (erode(&base, *inset), kind, register, *on_top),
            Op::Rim {
                width,
                kind,
                register,
                on_top,
            } => {
                // The body minus its eroded core. A fill painted before this
                // stays visible inside, leaving the rim proud at the edges.
                let core = erode(&base, *width);
                (difference(&base, &core), kind, register, *on_top)
            }
            Op::Cast {
                spread,
                dx,
                dy,
                kind,
                register,
                on_top,
            } => {
                // Outside the glyph only: the dilated, offset shape minus
                // the body, so a cast never paints over its own letterform.
                let spread_mask = dilate_offset(&base, *spread, *dx, *dy);
                (difference(&spread_mask, &base), kind, register, *on_top)
            }
            Op::Edge {
                outer,
                inner,
                kind,
                register,
                on_top,
            } => {
                // A band straddling the boundary: everything within `outer`
                // pixels outside, minus everything deeper than `inner`
                // pixels inside.
                let grown = dilate_offset(&base, *outer, 0, 0);
                let core = erode(&base, *inner);
                (difference(&grown, &core), kind, register, *on_top)
            }
        };
        let paint = match kind {
            Fill::Solid { color } => Paint::Solid(*color),
            Fill::Band { stops, steps } => Paint::Bands {
                stops: stops.clone(),
                steps: *steps,
            },
        };
        layers.push(Layer {
            mask,
            paint,
            register: register.as_ref().map(register_to_set),
            on_top,
        });
    }
    if layers.is_empty() {
        layers.push(Layer {
            mask: base,
            paint: Paint::Solid(Rgb::new(0xff, 0xff, 0xff)),
            register: None,
            on_top: false,
        });
    }
    Ok(layers)
}

fn register_to_set(r: &crate::recipe::Register) -> crate::symbolizer::SymbolSet {
    match r {
        crate::recipe::Register::Blocks => crate::symbolizer::SymbolSet::Blocks,
        crate::recipe::Register::Braille => crate::symbolizer::SymbolSet::Braille,
        crate::recipe::Register::Facets => crate::symbolizer::SymbolSet::Facets,
        crate::recipe::Register::Sextants => crate::symbolizer::SymbolSet::Sextants,
        // A register this build does not know falls back to the default, so
        // the layer still paints (ADR-202).
        crate::recipe::Register::Unknown(_) => crate::symbolizer::SymbolSet::Blocks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbolizer::Mask;

    #[test]
    fn solid_paint_is_position_independent() {
        let p = Paint::Solid(Rgb::new(1, 2, 3));
        assert_eq!(p.color_at(0.0), p.color_at(1.0));
    }

    #[test]
    fn band_paint_interpolates_between_stops() {
        let p = Paint::Bands {
            stops: vec![Rgb::new(0, 0, 0), Rgb::new(255, 255, 255)],
            steps: None,
        };
        assert_eq!(p.color_at(0.0), Rgb::new(0, 0, 0));
        assert_eq!(p.color_at(1.0), Rgb::new(255, 255, 255));
        assert_eq!(p.color_at(0.5), Rgb::new(128, 128, 128));
    }

    #[test]
    fn stepped_bands_quantize() {
        let p = Paint::Bands {
            stops: vec![Rgb::new(0, 0, 0), Rgb::new(255, 255, 255)],
            steps: Some(2),
        };
        // Two bands: everything below the midpoint is the first stop.
        assert_eq!(p.color_at(0.1), Rgb::new(0, 0, 0));
        assert_eq!(p.color_at(0.4), Rgb::new(0, 0, 0));
        assert_eq!(p.color_at(0.6), Rgb::new(255, 255, 255));
    }

    #[test]
    fn empty_stops_do_not_panic() {
        let p = Paint::Bands {
            stops: vec![],
            steps: None,
        };
        assert_eq!(p.color_at(0.5), Rgb::new(255, 255, 255));
    }

    #[test]
    fn erode_shrinks_a_solid_square() {
        let mask = Mask::from_sketch("#####\n#####\n#####\n#####\n#####");
        let out = erode(&mask, 1);
        // Only the center 3×3 survives a radius-1 erosion of a 5×5 square.
        assert!(out.get(2, 2));
        assert!(!out.get(0, 0));
        assert!(!out.get(4, 4));
        assert!(out.get(1, 1) && out.get(3, 3));
    }

    #[test]
    fn erode_zero_is_identity() {
        let mask = Mask::from_sketch("##\n##");
        assert_eq!(erode(&mask, 0), mask);
    }

    #[test]
    fn difference_leaves_the_rim() {
        let mask = Mask::from_sketch("###\n###\n###");
        let core = erode(&mask, 1);
        let rim = difference(&mask, &core);
        assert!(rim.get(0, 0));
        assert!(!rim.get(1, 1));
    }
}
