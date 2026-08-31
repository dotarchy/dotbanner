//! Truecolor ANSI output — the terminal preview sink.

use crate::symbolizer::CellGrid;

/// Render the grid with 24-bit escapes, resetting at each line's end. Each
/// cell can carry both a foreground (its glyph) and a background (the layer
/// composited beneath it), so overlapping layers both stay visible.
pub fn to_ansi(grid: &CellGrid) -> String {
    let mut out = String::new();
    for row in 0..grid.rows() {
        let mut fg: Option<crate::color::Rgb> = None;
        let mut bg: Option<crate::color::Rgb> = None;
        for col in 0..grid.cols() {
            let cell = match grid.get(col, row) {
                Some(c) => c,
                None => continue,
            };
            // A blank carries no ink, so it inherits the active foreground
            // rather than forcing a reset — that keeps runs unbroken. Its
            // background still applies: that is the cell being painted.
            if cell.ch != ' ' {
                if let Some(rgb) = cell.fg {
                    if fg != Some(rgb) {
                        out.push_str(&format!("\x1b[38;2;{};{};{}m", rgb.r, rgb.g, rgb.b));
                        fg = Some(rgb);
                    }
                }
            }
            if cell.bg != bg {
                match cell.bg {
                    Some(rgb) => out.push_str(&format!("\x1b[48;2;{};{};{}m", rgb.r, rgb.g, rgb.b)),
                    None => out.push_str("\x1b[49m"),
                }
                bg = cell.bg;
            }
            out.push(cell.ch);
        }
        if fg.is_some() || bg.is_some() {
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

    /// One full cell of the shared 6×12 layer footprint.
    fn full_cell() -> Mask {
        Mask::from_sketch(&(["######"].repeat(12)).join("\n"))
    }

    /// The top half of one cell — a partially covered edge cell.
    fn half_cell() -> Mask {
        let mut rows = ["######"].repeat(6);
        rows.extend(["      "].repeat(6));
        Mask::from_sketch(&rows.join("\n"))
    }

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
            mask: full_cell(),
            paint: Paint::Solid(Rgb::new(255, 0, 0)),
            register: None,
            on_top: false,
        }];
        let ansi = to_ansi(&symbolize_layers(&layers, SymbolSet::Blocks));
        assert_eq!(ansi, "\x1b[38;2;255;0;0m█\x1b[0m\n");
    }

    #[test]
    fn two_layers_share_a_cell_as_foreground_and_background() {
        use crate::engine::{Layer, Paint};
        use crate::symbolizer::symbolize_layers;
        // Equal coverage: the later layer wins the glyph, the earlier one
        // paints the background.
        let layers = vec![
            Layer {
                mask: full_cell(),
                paint: Paint::Solid(Rgb::new(0, 0, 255)),
                register: Some(SymbolSet::Braille),
                on_top: false,
            },
            Layer {
                mask: full_cell(),
                paint: Paint::Solid(Rgb::new(255, 0, 0)),
                register: Some(SymbolSet::Blocks),
                on_top: false,
            },
        ];
        let grid = symbolize_layers(&layers, SymbolSet::Blocks);
        let cell = grid.get(0, 0).unwrap();
        assert_eq!(cell.ch, '█');
        assert_eq!(cell.fg, Some(Rgb::new(255, 0, 0)));
        assert_eq!(cell.bg, Some(Rgb::new(0, 0, 255)));
    }

    #[test]
    fn an_overlay_is_trapped_to_fully_covered_cells() {
        use crate::engine::{Layer, Paint};
        use crate::symbolizer::symbolize_layers;
        for (body, expect_overlay) in [(full_cell(), true), (half_cell(), false)] {
            let layers = vec![
                Layer {
                    mask: body,
                    paint: Paint::Solid(Rgb::new(10, 10, 10)),
                    register: Some(SymbolSet::Blocks),
                    on_top: false,
                },
                Layer {
                    mask: full_cell(),
                    paint: Paint::Solid(Rgb::new(200, 200, 200)),
                    register: Some(SymbolSet::Braille),
                    on_top: true,
                },
            ];
            let grid = symbolize_layers(&layers, SymbolSet::Blocks);
            let cell = grid.get(0, 0).unwrap();
            if expect_overlay {
                assert_eq!(cell.ch, '⣿', "overlay takes a full cell");
                assert_eq!(cell.fg, Some(Rgb::new(200, 200, 200)));
            } else {
                // Partly covered: the body keeps its own glyph so the
                // silhouette stays crisp.
                assert_eq!(cell.ch, '▀', "body keeps a partial cell");
                assert_eq!(cell.fg, Some(Rgb::new(10, 10, 10)));
            }
        }
    }

    #[test]
    fn a_braille_layer_draws_in_its_own_register() {
        use crate::engine::{Layer, Paint};
        use crate::symbolizer::symbolize_layers;
        let layers = vec![Layer {
            mask: full_cell(),
            paint: Paint::Solid(Rgb::new(0, 255, 0)),
            register: Some(SymbolSet::Braille),
            on_top: false,
        }];
        let ansi = to_ansi(&symbolize_layers(&layers, SymbolSet::Blocks));
        assert_eq!(ansi, "\x1b[38;2;0;255;0m⣿\x1b[0m\n");
    }
}
