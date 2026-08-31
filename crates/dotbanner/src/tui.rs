//! The interactive recipe editor (ADR-200): a parameter panel beside a live
//! preview. Every control writes a field of the one recipe document, the
//! preview re-renders from it, and `s` saves it as JSON — the same file the
//! CLI's `--recipe` reads.
//!
//! The preview draws the `CellGrid` directly into ratatui spans rather than
//! round-tripping through the ANSI sink; both read the same grid, so what
//! the panel shows is what `render` will print.

use std::io;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::{DefaultTerminal, Frame};

use dotbanner_core::{
    presets,
    recipe::{Fit, Recipe, Register, Stage},
    scheme,
    symbolizer::CellGrid,
};

/// The registers a control can cycle through — the four this build draws
/// with. A recipe loaded with an unknown register keeps it until the user
/// moves the control (ADR-202).
const REGISTERS: [Register; 4] = [
    Register::Blocks,
    Register::Facets,
    Register::Sextants,
    Register::Braille,
];

/// The panel's controls, in focus order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Text,
    Font,
    Style,
    Palette,
    Register,
    Rows,
    Weight,
    Tracking,
    Path,
}

const CONTROLS: [Control; 9] = [
    Control::Text,
    Control::Font,
    Control::Style,
    Control::Palette,
    Control::Register,
    Control::Rows,
    Control::Weight,
    Control::Tracking,
    Control::Path,
];

impl Control {
    fn label(self) -> &'static str {
        match self {
            Control::Text => "text",
            Control::Font => "font",
            Control::Style => "style",
            Control::Palette => "colors",
            Control::Register => "register",
            Control::Rows => "rows",
            Control::Weight => "weight",
            Control::Tracking => "tracking",
            Control::Path => "save to",
        }
    }

    /// Whether Enter opens free-text editing on this control.
    fn takes_text(self) -> bool {
        matches!(self, Control::Text | Control::Palette | Control::Path)
    }
}

/// What keystrokes currently mean: moving between controls, or typing into
/// one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Navigate,
    Edit,
}

pub struct App {
    /// The document. Every control writes into it; save serializes it.
    recipe: Recipe,
    /// Where `s` writes. Seeded from `--recipe` so an opened file saves back
    /// to itself.
    path: String,
    /// The named style the pipeline was last built from. The document
    /// carries the pipeline itself; this remembers which preset the style
    /// control points at.
    style: String,
    /// The `--colors` spec: a palette name or a hex list.
    colors: String,
    weight: u32,
    /// Installed families, sorted (the engine sorts for determinism).
    fonts: Vec<String>,
    focus: usize,
    mode: Mode,
    status: String,
    /// Set when the document changed since the preview was last rendered.
    dirty: bool,
    /// The pane width the cached preview was rendered for; a resize
    /// invalidates it.
    preview_for: u16,
    preview: Result<CellGrid, String>,
    /// The serialized document as last saved (or as opened), so quitting
    /// can tell edits-with-a-home from edits-without.
    saved: String,
    /// Set by a first `q` with unsaved edits; the next `q` really quits.
    quit_armed: bool,
    quit: bool,
}

impl App {
    pub fn new(recipe: Recipe, path: String, style: String, colors: String, weight: u32) -> Self {
        Self {
            saved: recipe.to_json(),
            recipe,
            path,
            style,
            colors,
            weight,
            fonts: dotbanner_core::engine::list_families(),
            focus: 0,
            mode: Mode::Navigate,
            status: String::new(),
            dirty: true,
            preview_for: 0,
            preview: Err("rendering…".into()),
            quit_armed: false,
            quit: false,
        }
    }

    fn focused(&self) -> Control {
        CONTROLS[self.focus]
    }

    /// Rebuild the pipeline from the style/colors/weight controls. Stages
    /// this build cannot render are carried over, not dropped — an older
    /// build must not destroy an effect a newer one wrote (ADR-202).
    fn rebuild_pipeline(&mut self) {
        let Some(colors) = presets::resolve_colors(&self.colors) else {
            self.status = format!("unknown palette or bad colors: {}", self.colors);
            return;
        };
        let Some(ops) = presets::style_pipeline_weighted(&self.style, &colors, self.weight) else {
            self.status = format!("unknown style: {}", self.style);
            return;
        };
        let unknown: Vec<Stage> = self
            .recipe
            .pipeline
            .iter()
            .filter(|s| s.op().is_none())
            .cloned()
            .collect();
        self.recipe.pipeline = ops.into_iter().map(Into::into).chain(unknown).collect();
        self.dirty = true;
    }

    /// Move a selector or slider by `delta` steps.
    fn adjust(&mut self, delta: i64) {
        match self.focused() {
            Control::Text | Control::Path => {}
            Control::Font => {
                if self.fonts.is_empty() {
                    return;
                }
                let at = self
                    .fonts
                    .iter()
                    .position(|f| *f == self.recipe.font.family)
                    .unwrap_or(0);
                let next = cycle(at, self.fonts.len(), delta);
                self.recipe.font.family = self.fonts[next].clone();
                // A face name is per family; carrying one across families
                // would ask the next font for a style it may not have.
                self.recipe.font.style = None;
                self.dirty = true;
            }
            Control::Style => {
                let at = presets::STYLES
                    .iter()
                    .position(|s| *s == self.style)
                    .unwrap_or(0);
                self.style = presets::STYLES[cycle(at, presets::STYLES.len(), delta)].into();
                self.rebuild_pipeline();
            }
            Control::Palette => {
                let names: Vec<String> = scheme::all().into_iter().map(|s| s.name).collect();
                if names.is_empty() {
                    return;
                }
                let at = names.iter().position(|n| *n == self.colors).unwrap_or(0);
                self.colors = names[cycle(at, names.len(), delta)].clone();
                self.rebuild_pipeline();
            }
            Control::Register => {
                let at = REGISTERS
                    .iter()
                    .position(|r| *r == self.recipe.symbolizer.body)
                    .unwrap_or(0);
                self.recipe.symbolizer.body = REGISTERS[cycle(at, REGISTERS.len(), delta)].clone();
                self.dirty = true;
            }
            Control::Rows => {
                let rows = self.recipe.size.rows as i64 + delta;
                self.recipe.size.rows = rows.clamp(1, 64) as usize;
                self.dirty = true;
            }
            Control::Weight => {
                // The same 0–32 range the CLI flag accepts.
                let weight = self.weight as i64 + delta;
                self.weight = weight.clamp(0, 32) as u32;
                self.rebuild_pipeline();
            }
            Control::Tracking => {
                let t = self.recipe.size.tracking + delta as f32 * 0.01;
                self.recipe.size.tracking = t.clamp(0.0, 0.5);
                self.dirty = true;
            }
        }
    }

    /// The text buffer the Edit mode types into.
    fn text_buffer(&mut self) -> Option<&mut String> {
        match self.focused() {
            Control::Text => Some(&mut self.recipe.text),
            Control::Palette => Some(&mut self.colors),
            Control::Path => Some(&mut self.path),
            _ => None,
        }
    }

    fn save(&mut self) {
        match std::fs::write(&self.path, self.recipe.to_json() + "\n") {
            Ok(()) => {
                self.saved = self.recipe.to_json();
                self.status = format!("saved {}", self.path);
            }
            Err(e) => self.status = format!("saving {}: {e}", self.path),
        }
    }

    /// Quit, unless there are edits no file has: those want one deliberate
    /// second `q` (or a save) first.
    fn request_quit(&mut self) {
        if self.quit_armed || self.recipe.to_json() == self.saved {
            self.quit = true;
        } else {
            self.quit_armed = true;
            self.status = "unsaved changes — q again to discard, s to save".into();
        }
    }

    fn on_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        // Any key that is not the quit key stands down the quit warning.
        if code != KeyCode::Char('q') {
            self.quit_armed = false;
        }
        if self.mode == Mode::Edit {
            match code {
                KeyCode::Esc | KeyCode::Enter => self.mode = Mode::Navigate,
                KeyCode::Backspace => {
                    if let Some(buf) = self.text_buffer() {
                        buf.pop();
                        // A palette edit changes what the pipeline paints
                        // with, not just a field.
                        if self.focused() == Control::Palette {
                            self.rebuild_pipeline();
                        }
                        self.dirty = true;
                    }
                }
                KeyCode::Char(c) => {
                    if let Some(buf) = self.text_buffer() {
                        buf.push(c);
                        if self.focused() == Control::Palette {
                            self.rebuild_pipeline();
                        }
                        self.dirty = true;
                    }
                }
                _ => {}
            }
            return;
        }
        match code {
            KeyCode::Char('q') => self.request_quit(),
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => self.quit = true,
            KeyCode::Char('s') => self.save(),
            KeyCode::Up | KeyCode::Char('k') => {
                self.focus = cycle(self.focus, CONTROLS.len(), -1);
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                self.focus = cycle(self.focus, CONTROLS.len(), 1);
            }
            KeyCode::Left | KeyCode::Char('h') => self.adjust(-1),
            KeyCode::Right | KeyCode::Char('l') => self.adjust(1),
            KeyCode::Enter if self.focused().takes_text() => self.mode = Mode::Edit,
            _ => {}
        }
    }

    /// Render the preview for a pane `width` columns wide, reusing the cache
    /// when neither the document nor the pane changed.
    fn refresh_preview(&mut self, width: u16) {
        if !self.dirty && self.preview_for == width {
            return;
        }
        self.dirty = false;
        self.preview_for = width;
        if width == 0 {
            self.preview = Err("no room".into());
            return;
        }
        // The preview always fits its pane; the document's own `fit` is a
        // property of the saved recipe, not of this window.
        let mut preview = self.recipe.clone();
        preview.size.fit = Some(Fit::Columns(width as usize));
        self.preview = dotbanner_core::render(&preview).map_err(|e| e.to_string());
    }
}

/// Step `at` by `delta` around a ring of `len` entries.
fn cycle(at: usize, len: usize, delta: i64) -> usize {
    (at as i64 + delta).rem_euclid(len as i64) as usize
}

/// A `CellGrid` as ratatui text: each cell's glyph with its truecolor
/// foreground and background.
fn grid_text(grid: &CellGrid) -> Text<'static> {
    let mut lines = Vec::with_capacity(grid.rows());
    for row in 0..grid.rows() {
        let mut spans = Vec::with_capacity(grid.cols());
        for col in 0..grid.cols() {
            let Some(cell) = grid.get(col, row) else {
                continue;
            };
            let mut style = Style::default();
            if let Some(fg) = cell.fg {
                style = style.fg(Color::Rgb(fg.r, fg.g, fg.b));
            }
            if let Some(bg) = cell.bg {
                style = style.bg(Color::Rgb(bg.r, bg.g, bg.b));
            }
            spans.push(Span::styled(cell.ch.to_string(), style));
        }
        lines.push(Line::from(spans));
    }
    Text::from(lines)
}

/// One row of the parameter panel: a label, the current value, and where
/// focus sits, styled to say which control the keys act on.
fn control_line(app: &App, control: Control) -> Line<'static> {
    let focused = app.focused() == control;
    let value = match control {
        Control::Text => app.recipe.text.clone(),
        Control::Font => app.recipe.font.family.clone(),
        Control::Style => app.style.clone(),
        Control::Palette => app.colors.clone(),
        Control::Register => app.recipe.symbolizer.body.as_str().to_string(),
        Control::Rows => app.recipe.size.rows.to_string(),
        Control::Weight => app.weight.to_string(),
        Control::Tracking => format!("{:.2}", app.recipe.size.tracking),
        Control::Path => app.path.clone(),
    };
    let editing = focused && app.mode == Mode::Edit;
    let marker = if editing {
        "✎ "
    } else if focused {
        "▸ "
    } else {
        "  "
    };
    let label_style = if focused {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let value_style = if editing {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else if focused {
        Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        Style::default()
    };
    Line::from(vec![
        Span::raw(marker.to_string()),
        Span::styled(format!("{:<9}", control.label()), label_style),
        Span::styled(format!(" {value} "), value_style),
    ])
}

fn draw(app: &mut App, frame: &mut Frame) {
    let [body, status_bar] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());
    let [panel, preview] =
        Layout::horizontal([Constraint::Length(34), Constraint::Min(10)]).areas(body);

    let lines: Vec<Line> = CONTROLS.iter().map(|c| control_line(app, *c)).collect();
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" recipe ")),
        panel,
    );

    let preview_block = Block::default().borders(Borders::ALL).title(" preview ");
    let inner = preview_block.inner(preview);
    frame.render_widget(preview_block, preview);
    app.refresh_preview(inner.width);
    match &app.preview {
        Ok(grid) => {
            // Center a banner shorter than the pane; a taller one clips.
            let top = inner
                .height
                .saturating_sub(grid.rows() as u16)
                .saturating_div(2);
            let area = Rect {
                y: inner.y + top,
                height: inner.height.saturating_sub(top),
                ..inner
            };
            frame.render_widget(Clear, inner);
            frame.render_widget(Paragraph::new(grid_text(grid)), area);
        }
        Err(msg) => {
            frame.render_widget(
                Paragraph::new(msg.clone()).style(Style::default().fg(Color::Red)),
                inner,
            );
        }
    }

    let help = match app.mode {
        Mode::Navigate => "↑↓ control · ←→ change · enter edit · s save · q quit",
        Mode::Edit => "type to edit · enter/esc done",
    };
    let status = if app.status.is_empty() {
        help.to_string()
    } else {
        format!("{}   {help}", app.status)
    };
    frame.render_widget(
        Paragraph::new(status).style(Style::default().fg(Color::DarkGray)),
        status_bar,
    );
}

pub fn run(app: &mut App, terminal: &mut DefaultTerminal) -> io::Result<()> {
    while !app.quit {
        terminal.draw(|frame| draw(app, frame))?;
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                // A new keystroke supersedes the message about the last one.
                app.status.clear();
                app.on_key(key.code, key.modifiers);
            }
            Event::Resize(..) => {}
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with(recipe: Recipe) -> App {
        App::new(
            recipe,
            "out.json".into(),
            "plain".into(),
            "omarchy".into(),
            presets::DEFAULT_WEIGHT,
        )
    }

    #[test]
    fn rebuilding_the_pipeline_keeps_unknown_stages() {
        // The editor must not destroy an effect this build cannot draw
        // (ADR-202), even when the style control replaces the pipeline.
        let json = r##"{"text":"x","pipeline":[
            {"op":"fill","kind":"solid","color":"#ffffff"},
            {"op":"warp","amplitude":3}]}"##;
        let mut app = app_with(Recipe::from_json(json).unwrap());
        app.style = "trap".into();
        app.rebuild_pipeline();
        assert_eq!(app.recipe.unknown_ops(), vec!["warp"]);
        assert!(app.recipe.ops().count() >= 2, "the trap pipeline landed");
    }

    #[test]
    fn saving_round_trips_the_document() {
        let mut app = app_with(Recipe::new("hi"));
        app.rebuild_pipeline();
        let back = Recipe::from_json(&app.recipe.to_json()).unwrap();
        assert_eq!(app.recipe, back);
    }

    #[test]
    fn quitting_with_unsaved_edits_takes_a_second_q() {
        let mut app = app_with(Recipe::new("hi"));
        app.on_key(KeyCode::Char('q'), KeyModifiers::NONE);
        assert!(app.quit, "an untouched document quits at once");

        let mut app = app_with(Recipe::new("hi"));
        app.recipe.text.push('!');
        app.on_key(KeyCode::Char('q'), KeyModifiers::NONE);
        assert!(!app.quit, "the first q only warns");
        app.on_key(KeyCode::Char('q'), KeyModifiers::NONE);
        assert!(app.quit, "the second q discards");

        let mut app = app_with(Recipe::new("hi"));
        app.recipe.text.push('!');
        app.on_key(KeyCode::Char('q'), KeyModifiers::NONE);
        app.on_key(KeyCode::Down, KeyModifiers::NONE);
        app.on_key(KeyCode::Char('q'), KeyModifiers::NONE);
        assert!(!app.quit, "any other key stands the warning down");
    }

    #[test]
    fn cycle_wraps_both_ways() {
        assert_eq!(cycle(0, 3, -1), 2);
        assert_eq!(cycle(2, 3, 1), 0);
        assert_eq!(cycle(1, 3, 1), 2);
    }

    #[test]
    fn adjust_clamps_the_sliders() {
        let mut app = app_with(Recipe::new("hi"));
        app.focus = CONTROLS.iter().position(|c| *c == Control::Rows).unwrap();
        for _ in 0..100 {
            app.adjust(-1);
        }
        assert_eq!(app.recipe.size.rows, 1);
        app.focus = CONTROLS.iter().position(|c| *c == Control::Weight).unwrap();
        for _ in 0..100 {
            app.adjust(1);
        }
        assert_eq!(app.weight, 32, "the CLI flag's own upper bound");
    }

    #[test]
    fn changing_font_drops_the_face_name() {
        let mut app = app_with(Recipe::new("hi"));
        app.recipe.font.style = Some("Bold".into());
        app.focus = CONTROLS.iter().position(|c| *c == Control::Font).unwrap();
        if app.fonts.is_empty() {
            return; // No fonts installed in this environment.
        }
        app.adjust(1);
        assert_eq!(app.recipe.font.style, None);
    }

    #[test]
    fn the_preview_never_writes_fit_into_the_document() {
        let mut app = app_with(Recipe::new("hi"));
        app.rebuild_pipeline();
        app.refresh_preview(40);
        assert_eq!(app.recipe.size.fit, None, "fit belongs to the pane");
    }
}
