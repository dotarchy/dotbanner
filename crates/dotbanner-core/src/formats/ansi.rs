//! Truecolor ANSI output — the terminal preview sink.

use crate::symbolizer::CellGrid;

/// Render the grid with 24-bit foreground escapes, resetting at each line's
/// end. Cells without a foreground print bare.
pub fn to_ansi(grid: &CellGrid) -> String {
    let mut out = String::new();
    for row in 0..grid.rows() {
        let mut current: Option<crate::color::Rgb> = None;
        for col in 0..grid.cols() {
            let cell = match grid.get(col, row) {
                Some(c) => c,
                None => continue,
            };
            // Blanks carry no ink, so they inherit whatever color is active
            // rather than forcing a reset — that keeps runs unbroken.
            if cell.ch != ' ' {
                if let Some(rgb) = cell.fg {
                    if current != Some(rgb) {
                        out.push_str(&format!("\x1b[38;2;{};{};{}m", rgb.r, rgb.g, rgb.b));
                        current = Some(rgb);
                    }
                }
            }
            out.push(cell.ch);
        }
        if current.is_some() {
            out.push_str("\x1b[0m");
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgb;
    use crate::symbolizer::{symbolize, Mask, SymbolSet};

    #[test]
    fn mono_grid_has_no_escapes() {
        let mask = Mask::from_sketch("##\n##");
        let ansi = to_ansi(&symbolize(&mask, SymbolSet::Blocks));
        assert_eq!(ansi, "█\n");
    }

    #[test]
    fn colored_cells_emit_and_reset() {
        use crate::engine::{Layer, Paint};
        use crate::symbolizer::symbolize_layers;
        let layers = vec![Layer {
            mask: Mask::from_sketch("##\n##"),
            paint: Paint::Solid(Rgb::new(255, 0, 0)),
        }];
        let ansi = to_ansi(&symbolize_layers(&layers, SymbolSet::Blocks));
        assert_eq!(ansi, "\x1b[38;2;255;0;0m█\x1b[0m\n");
    }
}
