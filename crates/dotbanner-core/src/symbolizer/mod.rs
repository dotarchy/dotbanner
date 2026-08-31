//! The pixels-to-cells symbolizer (ADR-400).
//!
//! Coverage mapping only: each output cell samples a fixed pixel window from
//! a bi-level mask and looks the symbol up by bit pattern. Identical mask +
//! spec produces identical cells on every platform — baked fonts and shared
//! recipes depend on it. Perceptual heuristics require a symbolizer-domain
//! ADR before they enter this module (ADR-401 is the first).

/// A bi-level pixel mask. Out-of-bounds reads are `false`, so masks need no
/// padding to symbolize cleanly at any cell boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mask {
    width: usize,
    height: usize,
    bits: Vec<bool>,
}

impl Mask {
    /// # Panics
    ///
    /// Panics if `width * height` overflows `usize`.
    pub fn new(width: usize, height: usize) -> Self {
        let len = width.checked_mul(height).expect("mask dimensions overflow");
        Self {
            width,
            height,
            bits: vec![false; len],
        }
    }

    /// Build from per-pixel luminance (row-major) against a threshold.
    /// A pixel at or above `threshold` is set.
    ///
    /// # Panics
    ///
    /// Panics if `luma.len() != width * height`, or on dimension overflow.
    pub fn from_luma(width: usize, height: usize, luma: &[u8], threshold: u8) -> Self {
        let len = width.checked_mul(height).expect("mask dimensions overflow");
        assert_eq!(luma.len(), len, "luma buffer size mismatch");
        let bits = luma.iter().map(|&v| v >= threshold).collect();
        Self {
            width,
            height,
            bits,
        }
    }

    /// Build from a text sketch: `'#'` is set, any other character is clear.
    /// Lines may be ragged; short lines read as clear. Width is counted in
    /// characters, so multi-byte filler is safe.
    pub fn from_sketch(sketch: &str) -> Self {
        let lines: Vec<&str> = sketch.lines().collect();
        let height = lines.len();
        let width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
        let mut mask = Self::new(width, height);
        for (y, line) in lines.iter().enumerate() {
            for (x, ch) in line.chars().enumerate() {
                if ch == '#' {
                    mask.set(x, y, true);
                }
            }
        }
        mask
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn get(&self, x: usize, y: usize) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        self.bits[y * self.width + x]
    }

    /// # Panics
    ///
    /// Panics if `(x, y)` is outside the mask.
    pub fn set(&mut self, x: usize, y: usize, value: bool) {
        assert!(x < self.width && y < self.height, "set out of bounds");
        self.bits[y * self.width + x] = value;
    }
}

/// Which symbol repertoire a region renders with. Each register of a recipe's
/// symbolizer spec (body, rim, cast) names one.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolSet {
    /// Quadrant blocks (2×2 pixels per cell): ` ▘▝▀▖▌▞▛▗▚▐▜▄▙▟█`.
    Blocks,
    /// Braille patterns (2×4 pixels per cell): U+2800..U+28FF.
    Braille,
    /// Sextants (2×3 pixels per cell): U+1FB00..U+1FB3B plus `▌▐█`. Fifty
    /// percent more vertical resolution than quadrants, still a pure
    /// bit-pattern lookup.
    Sextants,
    /// Faceted blocks: the same quadrant coverage as [`SymbolSet::Blocks`],
    /// but three-quadrant patterns render as large triangles `◤◥◣◢` instead
    /// of the chunky `▛▜▙▟`, so edges read as cut faces.
    Facets,
}

impl SymbolSet {
    /// Pixel window one output cell covers, as (width, height).
    pub fn cell_size(self) -> (usize, usize) {
        match self {
            SymbolSet::Blocks | SymbolSet::Facets => (2, 2),
            SymbolSet::Sextants => (2, 3),
            SymbolSet::Braille => (2, 4),
        }
    }
}

/// One terminal cell of symbolized output: the glyph, the foreground it
/// paints with, and the background behind it.
///
/// A cell holds two colors, so two layers can share it: the layer with the
/// most coverage draws the glyph in `fg`, and the layer beneath it fills
/// `bg`. That is how a braille glow stays visible where a block body sits on
/// top of it, instead of being overwritten.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub fg: Option<crate::color::Rgb>,
    pub bg: Option<crate::color::Rgb>,
}

impl Cell {
    pub fn new(ch: char) -> Self {
        Self {
            ch,
            fg: None,
            bg: None,
        }
    }

    pub fn with_fg(ch: char, fg: crate::color::Rgb) -> Self {
        Self {
            ch,
            fg: Some(fg),
            bg: None,
        }
    }

    pub fn with_bg(mut self, bg: crate::color::Rgb) -> Self {
        self.bg = Some(bg);
        self
    }
}

/// A row-major grid of symbolized cells — what every output sink consumes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellGrid {
    cols: usize,
    rows: usize,
    cells: Vec<Cell>,
}

impl CellGrid {
    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn get(&self, col: usize, row: usize) -> Option<Cell> {
        if col >= self.cols || row >= self.rows {
            return None;
        }
        Some(self.cells[row * self.cols + col])
    }

    /// Render the grid as plain lines, dropping any per-cell attributes —
    /// the mono convenience view for tests and uncolored sinks.
    pub fn lines(&self) -> Vec<String> {
        (0..self.rows)
            .map(|r| {
                let mut line = String::with_capacity(self.cols * 4);
                for c in 0..self.cols {
                    line.push(self.cells[r * self.cols + c].ch);
                }
                line
            })
            .collect()
    }
}

/// Quadrant lookup: bit 0 = upper-left, 1 = upper-right, 2 = lower-left,
/// 3 = lower-right.
const QUADS: [char; 16] = [
    ' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█',
];

/// Faceted variant of [`QUADS`]: identical except that the four
/// three-quadrant patterns become large triangles, turning every corner
/// into a cut face rather than a step.
const FACETS: [char; 16] = [
    ' ', '▘', '▝', '▀', '▖', '▌', '▞', '◤', '▗', '▚', '▐', '◥', '▄', '◣', '◢', '█',
];

fn quad_char(mask: &Mask, cx: usize, cy: usize) -> char {
    QUADS[quad_bits(mask, cx, cy)]
}

fn facet_char(mask: &Mask, cx: usize, cy: usize) -> char {
    FACETS[quad_bits(mask, cx, cy)]
}

fn quad_bits(mask: &Mask, cx: usize, cy: usize) -> usize {
    let x = cx * 2;
    let y = cy * 2;
    let mut bits = 0usize;
    if mask.get(x, y) {
        bits |= 1;
    }
    if mask.get(x + 1, y) {
        bits |= 2;
    }
    if mask.get(x, y + 1) {
        bits |= 4;
    }
    if mask.get(x + 1, y + 1) {
        bits |= 8;
    }
    bits
}

/// Sextant bit order (Unicode): 1 = top-left, 2 = top-right, 3 = middle-left,
/// 4 = middle-right, 5 = bottom-left, 6 = bottom-right. The block omits the
/// three patterns that already exist elsewhere — left half, right half and
/// full block — so the codepoint index skips them.
fn sextant_char(mask: &Mask, cx: usize, cy: usize) -> char {
    let (x, y) = (cx * 2, cy * 3);
    let mut bits = 0usize;
    for row in 0..3 {
        for col in 0..2 {
            if mask.get(x + col, y + row) {
                bits |= 1 << (row * 2 + col);
            }
        }
    }
    sextant_glyph(bits)
}

fn sextant_glyph(bits: usize) -> char {
    match bits {
        0 => ' ',
        21 => '\u{258C}', // left column: LEFT HALF BLOCK
        42 => '\u{2590}', // right column: RIGHT HALF BLOCK
        63 => '\u{2588}', // FULL BLOCK
        b => {
            // The block skips the three patterns above, so shift the index
            // past each one already passed.
            let idx = b - 1 - usize::from(b > 21) - usize::from(b > 42);
            char::from_u32(0x1FB00 + idx as u32).expect("sextant index in range")
        }
    }
}

/// Braille dot numbering (Unicode): dots 1–3 run down the left column, 4–6
/// down the right, dots 7 and 8 are the bottom row left and right.
fn braille_char(mask: &Mask, cx: usize, cy: usize) -> char {
    let x = cx * 2;
    let y = cy * 4;
    let mut bits = 0u32;
    if mask.get(x, y) {
        bits |= 0x01; // dot 1
    }
    if mask.get(x, y + 1) {
        bits |= 0x02; // dot 2
    }
    if mask.get(x, y + 2) {
        bits |= 0x04; // dot 3
    }
    if mask.get(x + 1, y) {
        bits |= 0x08; // dot 4
    }
    if mask.get(x + 1, y + 1) {
        bits |= 0x10; // dot 5
    }
    if mask.get(x + 1, y + 2) {
        bits |= 0x20; // dot 6
    }
    if mask.get(x, y + 3) {
        bits |= 0x40; // dot 7
    }
    if mask.get(x + 1, y + 3) {
        bits |= 0x80; // dot 8
    }
    char::from_u32(0x2800 + bits).expect("braille block covers all 8-bit patterns")
}

/// Map a mask to a cell grid. Output dimensions are the mask's, divided by
/// the set's cell size, rounded up; partial edge cells read out-of-bounds
/// pixels as clear.
pub fn symbolize(mask: &Mask, set: SymbolSet) -> CellGrid {
    let (cw, ch) = set.cell_size();
    let cols = mask.width().div_ceil(cw);
    let rows = mask.height().div_ceil(ch);
    let mut cells = Vec::with_capacity(cols * rows);
    for cy in 0..rows {
        for cx in 0..cols {
            cells.push(Cell::new(match set {
                SymbolSet::Blocks => quad_char(mask, cx, cy),
                SymbolSet::Facets => facet_char(mask, cx, cy),
                SymbolSet::Sextants => sextant_char(mask, cx, cy),
                SymbolSet::Braille => braille_char(mask, cx, cy),
            }));
        }
    }
    CellGrid { cols, rows, cells }
}

/// Symbolize painted layers into one colored grid.
///
/// Each cell is contested by every layer covering it. The layer with the
/// most covered pixels draws the glyph in its own register and paints the
/// foreground; ties go to the later layer, preserving draw order. Majority
/// coverage is what keeps a one-pixel trap from flooding every edge cell.
///
/// The runner-up paints the **background**, so two layers share the cell
/// rather than one erasing the other — a braille glow keeps showing through
/// where a block body covers it, and both keep their own paint. A layer that
/// loses every cell it touches still contributes color this way.
pub fn symbolize_layers(layers: &[crate::engine::Layer], default_set: SymbolSet) -> CellGrid {
    // Every register shares this pixel footprint per cell, so a braille cast
    // and a block body can occupy one grid (ADR-201).
    // A terminal cell is about twice as tall as it is wide, so the footprint
    // keeps that 1:2 ratio.
    const CELL_W: usize = 6;
    // Twelve pixel rows per cell is the least common multiple of every
    // register's sub-row count (blocks 2, sextants 3, braille 4), so each
    // one samples whole sub-blocks with nothing left over.
    const CELL_H: usize = 12;

    let width = layers.iter().map(|l| l.mask.width()).max().unwrap_or(0);
    let height = layers.iter().map(|l| l.mask.height()).max().unwrap_or(0);
    let cols = width.div_ceil(CELL_W);
    let rows = height.div_ceil(CELL_H);

    let mut cells = Vec::with_capacity(cols * rows);
    for row in 0..rows {
        for col in 0..cols {
            // Score every layer's coverage of this cell, keeping the two
            // best: the winner draws, the runner-up backs it. A layer marked
            // `on_top` skips the contest and takes the glyph outright,
            // demoting the best coverage-scored layer to the background.
            let mut owner: Option<(&crate::engine::Layer, usize, usize)> = None;
            let mut under: Option<(&crate::engine::Layer, usize, usize)> = None;
            let mut overlay: Option<(&crate::engine::Layer, usize, usize)> = None;
            for layer in layers {
                let mut sum_y = 0usize;
                let mut count = 0usize;
                for dy in 0..CELL_H {
                    for dx in 0..CELL_W {
                        if layer.mask.get(col * CELL_W + dx, row * CELL_H + dy) {
                            sum_y += row * CELL_H + dy;
                            count += 1;
                        }
                    }
                }
                if count == 0 {
                    continue;
                }
                if layer.on_top {
                    overlay = Some((layer, count, sum_y));
                    continue;
                }
                if owner.map(|(_, best, _)| count >= best).unwrap_or(true) {
                    under = owner;
                    owner = Some((layer, count, sum_y));
                } else if under.map(|(_, second, _)| count >= second).unwrap_or(true) {
                    under = Some((layer, count, sum_y));
                }
            }
            // An overlay is trapped to the interior: it takes the cell only
            // where the layer beneath fills every pixel. At the silhouette
            // the body keeps its own crisp glyph, so a sparse overlay never
            // punches holes in the outline or squares it off.
            if let Some(top) = overlay {
                if let Some((_, base_count, _)) = owner {
                    if base_count == CELL_W * CELL_H {
                        under = owner;
                        owner = Some(top);
                    }
                } else {
                    owner = Some(top);
                }
            }

            let Some((layer, count, sum_y)) = owner else {
                cells.push(Cell::new(' '));
                continue;
            };
            let paint_at = |l: &crate::engine::Layer, count: usize, sum_y: usize| {
                let mid = sum_y as f32 / count as f32;
                let t = if height > 1 {
                    mid / (height - 1) as f32
                } else {
                    0.0
                };
                l.paint.color_at(t)
            };
            let set = layer.register.unwrap_or(default_set);
            let glyph = cell_glyph(&layer.mask, col, row, set, CELL_W, CELL_H);
            let mut cell = Cell::with_fg(glyph, paint_at(layer, count, sum_y));
            // A background paints the whole cell rectangle, so it may only
            // come from a layer that genuinely fills the cell. Painting one
            // from a partially covered edge cell would spill past the
            // silhouette and square the letterform off.
            if let Some((below, bc, bsum)) = under {
                if bc == CELL_W * CELL_H {
                    cell = cell.with_bg(paint_at(below, bc, bsum));
                }
            }
            cells.push(cell);
        }
    }
    CellGrid { cols, rows, cells }
}

/// The glyph for one cell of a layer's mask, in the given register. Blocks
/// downsample the shared 2×4 footprint into quadrants — a quadrant is set
/// when either of its two pixel rows is.
fn cell_glyph(
    mask: &Mask,
    col: usize,
    row: usize,
    set: SymbolSet,
    cell_w: usize,
    cell_h: usize,
) -> char {
    let (x0, y0) = (col * cell_w, row * cell_h);
    let (sub_w, sub_h) = set.cell_size();
    let (bw, bh) = (cell_w / sub_w, cell_h / sub_h);
    // A sub-cell is set when at least half its pixels are — majority keeps
    // thin strokes without letting a single stray pixel fill a sub-cell.
    let mut sub = Mask::new(sub_w, sub_h);
    for sy in 0..sub_h {
        for sx in 0..sub_w {
            let mut on = 0usize;
            for dy in 0..bh {
                for dx in 0..bw {
                    if mask.get(x0 + sx * bw + dx, y0 + sy * bh + dy) {
                        on += 1;
                    }
                }
            }
            sub.set(sx, sy, on * 2 >= bw * bh);
        }
    }
    match set {
        SymbolSet::Blocks => quad_char(&sub, 0, 0),
        SymbolSet::Facets => facet_char(&sub, 0, 0),
        SymbolSet::Sextants => sextant_char(&sub, 0, 0),
        SymbolSet::Braille => braille_char(&sub, 0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_quad_pattern_maps_to_its_symbol() {
        for (bits, expected) in QUADS.iter().enumerate() {
            let mut mask = Mask::new(2, 2);
            mask.set(0, 0, bits & 1 != 0);
            mask.set(1, 0, bits & 2 != 0);
            mask.set(0, 1, bits & 4 != 0);
            mask.set(1, 1, bits & 8 != 0);
            let out = symbolize(&mask, SymbolSet::Blocks).lines();
            assert_eq!(out, vec![expected.to_string()], "pattern {bits:04b}");
        }
    }

    #[test]
    fn facets_replace_three_quadrant_blocks_with_triangles() {
        // Three quadrants set, missing the lower-right: blocks give the
        // chunky corner, facets give a cut face.
        let mut mask = Mask::new(2, 2);
        mask.set(0, 0, true);
        mask.set(1, 0, true);
        mask.set(0, 1, true);
        assert_eq!(symbolize(&mask, SymbolSet::Blocks).lines(), vec!["▛"]);
        assert_eq!(symbolize(&mask, SymbolSet::Facets).lines(), vec!["◤"]);
        // Everything else agrees with the block repertoire.
        let full = Mask::from_sketch("##\n##");
        assert_eq!(
            symbolize(&full, SymbolSet::Blocks).lines(),
            symbolize(&full, SymbolSet::Facets).lines()
        );
    }

    #[test]
    fn braille_dot_positions() {
        // dot 1 (top-left) alone
        let mut mask = Mask::new(2, 4);
        mask.set(0, 0, true);
        assert_eq!(symbolize(&mask, SymbolSet::Braille).lines(), vec!["⠁"]);
        // dot 8 (bottom-right) alone
        let mut mask = Mask::new(2, 4);
        mask.set(1, 3, true);
        assert_eq!(symbolize(&mask, SymbolSet::Braille).lines(), vec!["⢀"]);
        // all dots
        let mut mask = Mask::new(2, 4);
        for y in 0..4 {
            for x in 0..2 {
                mask.set(x, y, true);
            }
        }
        assert_eq!(symbolize(&mask, SymbolSet::Braille).lines(), vec!["⣿"]);
    }

    #[test]
    fn partial_edge_cells_read_clear() {
        // 3×3 mask, all set: right and bottom cells are partial.
        let mut mask = Mask::new(3, 3);
        for y in 0..3 {
            for x in 0..3 {
                mask.set(x, y, true);
            }
        }
        let out = symbolize(&mask, SymbolSet::Blocks).lines();
        assert_eq!(out, vec!["█▌", "▀▘"]);
    }

    #[test]
    fn sketch_accepts_multibyte_filler() {
        let mask = Mask::from_sketch("·#\n#·");
        assert_eq!(mask.width(), 2);
        assert!(!mask.get(0, 0) && mask.get(1, 0));
        assert!(mask.get(0, 1) && !mask.get(1, 1));
    }

    #[test]
    fn luma_threshold_is_inclusive() {
        let mask = Mask::from_luma(2, 1, &[127, 128], 128);
        assert!(!mask.get(0, 0));
        assert!(mask.get(1, 0));
    }

    #[test]
    fn cell_grid_indexing() {
        let mask = Mask::from_sketch("##\n##");
        let grid = symbolize(&mask, SymbolSet::Blocks);
        assert_eq!((grid.cols(), grid.rows()), (1, 1));
        assert_eq!(grid.get(0, 0).map(|c| c.ch), Some('█'));
        assert_eq!(grid.get(1, 0), None);
    }

    /// Golden fixture: a small glyph-like shape pinned exactly in both sets.
    /// A diff here is a symbolizer behavior change and needs ADR-level
    /// justification (ADR-400).
    #[test]
    fn golden_arrow_fixture() {
        let sketch = "\
....##....
...####...
..######..
.########.
....##....
....##....
....##....
....##....";
        let mask = Mask::from_sketch(sketch);
        assert_eq!(
            symbolize(&mask, SymbolSet::Blocks).lines(),
            vec![" ▗█▖ ", "▗███▖", "  █  ", "  █  "],
        );
        assert_eq!(
            symbolize(&mask, SymbolSet::Braille).lines(),
            vec!["⢀⣴⣿⣦⡀", "⠀⠀⣿⠀⠀"],
        );
    }
}
