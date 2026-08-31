//! The pixels-to-cells symbolizer (ADR-400).
//!
//! Coverage mapping only: each output cell samples a fixed pixel window from
//! a bi-level mask and looks the symbol up by bit pattern. Identical mask +
//! spec produces identical cells on every platform — baked fonts and shared
//! recipes depend on it. Perceptual heuristics require a symbolizer-domain
//! ADR before they enter this module.

/// A bi-level pixel mask. Out-of-bounds reads are `false`, so masks need no
/// padding to symbolize cleanly at any cell boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mask {
    width: usize,
    height: usize,
    bits: Vec<bool>,
}

impl Mask {
    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height, bits: vec![false; width * height] }
    }

    /// Build from per-pixel luminance (row-major) against a threshold.
    /// `luma.len()` must equal `width * height`.
    pub fn from_luma(width: usize, height: usize, luma: &[u8], threshold: u8) -> Self {
        assert_eq!(luma.len(), width * height, "luma buffer size mismatch");
        let bits = luma.iter().map(|&v| v >= threshold).collect();
        Self { width, height, bits }
    }

    /// Build from a text sketch: `'#'` is set, anything else is clear.
    /// Lines may be ragged; short lines read as clear.
    pub fn from_sketch(sketch: &str) -> Self {
        let lines: Vec<&str> = sketch.lines().collect();
        let height = lines.len();
        let width = lines.iter().map(|l| l.len()).max().unwrap_or(0);
        let mut mask = Self::new(width, height);
        for (y, line) in lines.iter().enumerate() {
            for (x, ch) in line.bytes().enumerate() {
                if ch == b'#' {
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

    pub fn set(&mut self, x: usize, y: usize, value: bool) {
        assert!(x < self.width && y < self.height, "set out of bounds");
        self.bits[y * self.width + x] = value;
    }
}

/// Which symbol repertoire a region renders with. Each register of a recipe's
/// symbolizer spec (body, rim, cast) names one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolSet {
    /// Quadrant blocks (2×2 pixels per cell): ` ▘▝▀▖▌▞▛▗▚▐▜▄▙▟█`.
    Blocks,
    /// Braille patterns (2×4 pixels per cell): U+2800..U+28FF.
    Braille,
}

impl SymbolSet {
    /// Pixel window one output cell covers, as (width, height).
    pub fn cell_size(self) -> (usize, usize) {
        match self {
            SymbolSet::Blocks => (2, 2),
            SymbolSet::Braille => (2, 4),
        }
    }
}

/// Quadrant lookup: bit 0 = upper-left, 1 = upper-right, 2 = lower-left,
/// 3 = lower-right.
const QUADS: [char; 16] = [
    ' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█',
];

fn quad_char(mask: &Mask, cx: usize, cy: usize) -> char {
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
    QUADS[bits]
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

/// Map a mask to lines of character art. Output dimensions are the mask's,
/// divided by the set's cell size, rounded up; partial edge cells read
/// out-of-bounds pixels as clear.
pub fn symbolize(mask: &Mask, set: SymbolSet) -> Vec<String> {
    let (cw, ch) = set.cell_size();
    let cols = mask.width().div_ceil(cw);
    let rows = mask.height().div_ceil(ch);
    let mut lines = Vec::with_capacity(rows);
    for cy in 0..rows {
        let mut line = String::with_capacity(cols);
        for cx in 0..cols {
            line.push(match set {
                SymbolSet::Blocks => quad_char(mask, cx, cy),
                SymbolSet::Braille => braille_char(mask, cx, cy),
            });
        }
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_quad_pattern_maps_to_its_symbol() {
        for bits in 0..16usize {
            let mut mask = Mask::new(2, 2);
            mask.set(0, 0, bits & 1 != 0);
            mask.set(1, 0, bits & 2 != 0);
            mask.set(0, 1, bits & 4 != 0);
            mask.set(1, 1, bits & 8 != 0);
            let out = symbolize(&mask, SymbolSet::Blocks);
            assert_eq!(out, vec![QUADS[bits].to_string()], "pattern {bits:04b}");
        }
    }

    #[test]
    fn braille_dot_positions() {
        // dot 1 (top-left) alone
        let mut mask = Mask::new(2, 4);
        mask.set(0, 0, true);
        assert_eq!(symbolize(&mask, SymbolSet::Braille), vec!["⠁".to_string()]);
        // dot 8 (bottom-right) alone
        let mut mask = Mask::new(2, 4);
        mask.set(1, 3, true);
        assert_eq!(symbolize(&mask, SymbolSet::Braille), vec!["⢀".to_string()]);
        // all dots
        let mut mask = Mask::new(2, 4);
        for y in 0..4 {
            for x in 0..2 {
                mask.set(x, y, true);
            }
        }
        assert_eq!(symbolize(&mask, SymbolSet::Braille), vec!["⣿".to_string()]);
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
        let out = symbolize(&mask, SymbolSet::Blocks);
        assert_eq!(out, vec!["█▌".to_string(), "▀▘".to_string()]);
    }

    #[test]
    fn sketch_round_trip() {
        let mask = Mask::from_sketch("##  ##\n##  ##\n######\n######");
        let out = symbolize(&mask, SymbolSet::Blocks);
        assert_eq!(out, vec!["█ █".to_string(), "███".to_string()]);
    }

    #[test]
    fn luma_threshold_is_inclusive() {
        let mask = Mask::from_luma(2, 1, &[127, 128], 128);
        assert!(!mask.get(0, 0));
        assert!(mask.get(1, 0));
    }

    #[test]
    fn deterministic_across_runs() {
        let mask = Mask::from_sketch("#.#.#\n.#.#.\n#.#.#\n.#.#.");
        let a = symbolize(&mask, SymbolSet::Braille);
        let b = symbolize(&mask, SymbolSet::Braille);
        assert_eq!(a, b);
    }
}
