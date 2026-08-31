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
    recipe::{Font, Recipe, Register, Stage},
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

/// Which sidebar the panel shows: the recipe's controls, or the font
/// browser — a filterable family list whose selection previews live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Recipe,
    Fonts,
}

pub struct App {
    /// The document. Every control writes into it; save serializes it.
    recipe: Recipe,
    /// Where `s` writes. Seeded from `--recipe` so an opened file saves back
    /// to itself.
    path: String,
    /// The named style the pipeline was last built from. `None` means the
    /// pipeline came from a file no preset claims: the panel shows "custom",
    /// and no control rebuilds the pipeline until the user picks a style —
    /// a weight or palette nudge must not replace effects it did not make.
    style: Option<String>,
    /// The `--colors` spec: a palette name or a hex list.
    colors: String,
    weight: u32,
    /// True once `path` names a file this session opened or wrote, so a
    /// save into it is an update rather than a surprise overwrite.
    own_file: bool,
    /// Installed families, sorted (the engine sorts for determinism).
    fonts: Vec<String>,
    focus: usize,
    mode: Mode,
    view: View,
    /// Substring the font browser filters families by.
    font_filter: String,
    /// Selected row in the browser's filtered list.
    font_sel: usize,
    /// The document's font when the browser opened; Esc restores it.
    font_entry: Font,
    status: String,
    /// Set when the document changed since the preview was last rendered.
    dirty: bool,
    preview: Result<CellGrid, String>,
    /// The serialized document as last saved (or as opened), so quitting
    /// can tell edits-with-a-home from edits-without.
    saved: String,
    /// A warning that must outlive the next keystroke: the opened file uses
    /// a newer schema or carries effects this build cannot draw.
    notice: String,
    /// An action awaiting its confirming second keystroke.
    pending: Pending,
    quit: bool,
}

/// A destructive action a first keystroke has warned about; the same
/// keystroke again carries it out, any other stands it down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    None,
    Quit,
    Save,
}

impl App {
    /// `style: None` marks a document opened from a file: its pipeline is
    /// custom until the user picks a preset. `own_file` says `path` names
    /// that opened file.
    pub fn new(
        recipe: Recipe,
        path: String,
        own_file: bool,
        style: Option<String>,
        colors: String,
        weight: u32,
    ) -> Self {
        // These conditions hold for the document's whole lifetime in the
        // editor, so they live above the one-keystroke status line.
        let mut notices = Vec::new();
        if recipe.is_newer_than_this_build() {
            notices.push(format!(
                "recipe schema v{} is newer than this build reads",
                recipe.version
            ));
        }
        let skipped = recipe.unknown_ops();
        if !skipped.is_empty() {
            notices.push(format!(
                "{} layer(s) use effects this build cannot draw: {}",
                skipped.len(),
                skipped.join(", ")
            ));
        }
        Self {
            saved: recipe.to_json(),
            recipe,
            path,
            own_file,
            style,
            colors,
            weight,
            fonts: dotbanner_core::engine::list_families(),
            focus: 0,
            mode: Mode::Navigate,
            view: View::Recipe,
            font_filter: String::new(),
            font_sel: 0,
            font_entry: Font::default(),
            status: String::new(),
            dirty: true,
            preview: Err("rendering…".into()),
            notice: notices.join(" · "),
            pending: Pending::None,
            quit: false,
        }
    }

    fn focused(&self) -> Control {
        CONTROLS[self.focus]
    }

    /// Rebuild the pipeline from the style/colors/weight controls. Stages
    /// this build cannot render are carried over, not dropped — an older
    /// build must not destroy an effect a newer one wrote (ADR-202). A
    /// custom pipeline (no style picked yet) is never rebuilt.
    fn rebuild_pipeline(&mut self) {
        let Some(style) = self.style.clone() else {
            self.status = "the pipeline is custom — pick a style to rebuild it".into();
            return;
        };
        let Some(colors) = presets::resolve_colors(&self.colors) else {
            self.status = format!("unknown palette or bad colors: {}", self.colors);
            return;
        };
        let Some(ops) = presets::style_pipeline_weighted(&style, &colors, self.weight) else {
            self.status = format!("unknown style: {style}");
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
                // Leaving "custom" is the one deliberate act that hands the
                // pipeline to a preset; either direction starts at an end.
                let next = match &self.style {
                    Some(style) => {
                        let at = presets::STYLES.iter().position(|s| s == style).unwrap_or(0);
                        cycle(at, presets::STYLES.len(), delta)
                    }
                    None if delta >= 0 => 0,
                    None => presets::STYLES.len() - 1,
                };
                self.style = Some(presets::STYLES[next].into());
                self.rebuild_pipeline();
            }
            Control::Palette => {
                if self.style.is_none() {
                    self.status = "the pipeline is custom — pick a style to use colors".into();
                    return;
                }
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
                if self.style.is_none() {
                    self.status = "the pipeline is custom — pick a style to use weight".into();
                    return;
                }
                // The same 0–32 range the CLI flag accepts.
                let weight = self.weight as i64 + delta;
                self.weight = weight.clamp(0, 32) as u32;
                self.rebuild_pipeline();
            }
            Control::Tracking => {
                // Round to the step so repeated nudges cannot leave float
                // noise in the saved JSON.
                let t = self.recipe.size.tracking + delta as f32 * 0.01;
                self.recipe.size.tracking = (t.clamp(0.0, 0.5) * 100.0).round() / 100.0;
                self.dirty = true;
            }
        }
    }

    /// Families matching the browser's filter, in list order.
    fn filtered_fonts(&self) -> Vec<String> {
        let filter = self.font_filter.to_ascii_lowercase();
        self.fonts
            .iter()
            .filter(|f| filter.is_empty() || f.to_ascii_lowercase().contains(&filter))
            .cloned()
            .collect()
    }

    /// Open the font browser on the document's current family.
    fn open_fonts(&mut self) {
        self.view = View::Fonts;
        self.font_entry = self.recipe.font.clone();
        self.font_filter.clear();
        self.font_sel = self
            .fonts
            .iter()
            .position(|f| *f == self.recipe.font.family)
            .unwrap_or(0);
    }

    /// Write the browser's selection into the document, so the preview
    /// shows the highlighted family as it moves.
    fn font_apply(&mut self) {
        let list = self.filtered_fonts();
        if list.is_empty() {
            return;
        }
        self.font_sel = self.font_sel.min(list.len() - 1);
        let family = list[self.font_sel].clone();
        if self.recipe.font.family != family {
            self.recipe.font.family = family;
            // A face name is per family (see the font control).
            self.recipe.font.style = None;
            self.dirty = true;
        }
    }

    /// Move the browser selection by `delta`, stopping at the list's ends —
    /// a browser walks, it does not wrap.
    fn font_browse(&mut self, delta: i64) {
        let len = self.filtered_fonts().len();
        if len == 0 {
            return;
        }
        self.font_sel = (self.font_sel as i64 + delta).clamp(0, len as i64 - 1) as usize;
        self.font_apply();
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
                self.own_file = true;
                self.status = format!("saved {}", self.path);
            }
            Err(e) => self.status = format!("saving {}: {e}", self.path),
        }
    }

    /// Save, after a confirming second `s` when the write would destroy
    /// something: fields of a schema newer than this build reads (the wire
    /// parser drops top-level fields it does not know), or a file this
    /// session never opened.
    fn request_save(&mut self) {
        if self.pending == Pending::Save {
            self.pending = Pending::None;
            self.save();
            return;
        }
        if self.recipe.is_newer_than_this_build() {
            self.pending = Pending::Save;
            self.status = format!(
                "recipe schema v{} is newer than this build — saving drops fields it cannot \
                 read; s again to save anyway",
                self.recipe.version
            );
            return;
        }
        if !self.own_file && std::path::Path::new(&self.path).exists() {
            self.pending = Pending::Save;
            self.status = format!("{} exists — s again to overwrite", self.path);
            return;
        }
        self.save();
    }

    /// Quit, unless there are edits no file has: those want one deliberate
    /// second `q` (or a save) first.
    fn request_quit(&mut self) {
        if self.pending == Pending::Quit || self.recipe.to_json() == self.saved {
            self.quit = true;
        } else {
            self.pending = Pending::Quit;
            self.status = "unsaved changes — q again to discard, s to save".into();
        }
    }

    fn on_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        // Any key other than the one that armed a confirmation stands it
        // down.
        match (self.pending, code) {
            (Pending::Quit, KeyCode::Char('q')) | (Pending::Save, KeyCode::Char('s')) => {}
            _ => self.pending = Pending::None,
        }
        if modifiers.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if self.view == View::Fonts {
            match code {
                // Enter keeps the selection the preview already shows; Esc
                // puts back the font the browser opened on.
                KeyCode::Enter => self.view = View::Recipe,
                KeyCode::Esc => {
                    if self.recipe.font != self.font_entry {
                        self.recipe.font = self.font_entry.clone();
                        self.dirty = true;
                    }
                    self.view = View::Recipe;
                }
                KeyCode::Up => self.font_browse(-1),
                KeyCode::Down => self.font_browse(1),
                KeyCode::PageUp => self.font_browse(-10),
                KeyCode::PageDown => self.font_browse(10),
                KeyCode::Backspace => {
                    self.font_filter.pop();
                    self.font_sel = 0;
                    self.font_apply();
                }
                KeyCode::Char(c)
                    if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.font_filter.push(c);
                    self.font_sel = 0;
                    self.font_apply();
                }
                _ => {}
            }
            return;
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
                        // Editing the path points at a file this session
                        // has not written.
                        if self.focused() == Control::Path {
                            self.own_file = false;
                        }
                        self.dirty = true;
                    }
                }
                // A chorded character is a command that means nothing here,
                // not text.
                KeyCode::Char(c)
                    if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    if let Some(buf) = self.text_buffer() {
                        buf.push(c);
                        if self.focused() == Control::Palette {
                            self.rebuild_pipeline();
                        }
                        if self.focused() == Control::Path {
                            self.own_file = false;
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
            KeyCode::Char('s') => self.request_save(),
            KeyCode::Up | KeyCode::Char('k') => {
                self.focus = cycle(self.focus, CONTROLS.len(), -1);
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                self.focus = cycle(self.focus, CONTROLS.len(), 1);
            }
            KeyCode::Left | KeyCode::Char('h') => self.adjust(-1),
            KeyCode::Right | KeyCode::Char('l') => self.adjust(1),
            KeyCode::Char('f') => self.open_fonts(),
            KeyCode::Enter if self.focused() == Control::Font => self.open_fonts(),
            KeyCode::Enter if self.focused().takes_text() => self.mode = Mode::Edit,
            _ => {}
        }
    }

    /// Re-render the preview when the document changed. The banner renders
    /// at the document's own size — fitting the pane is the wrap's job in
    /// `grid_text` — so the grid is exactly what `render` would print.
    fn refresh_preview(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        self.preview = dotbanner_core::render(&self.recipe).map_err(|e| e.to_string());
    }
}

/// Step `at` by `delta` around a ring of `len` entries.
fn cycle(at: usize, len: usize, delta: i64) -> usize {
    (at as i64 + delta).rem_euclid(len as i64) as usize
}

/// How many bands of `width` columns a grid wraps into.
fn band_count(cols: usize, width: usize) -> usize {
    if width == 0 || cols == 0 {
        1
    } else {
        cols.div_ceil(width)
    }
}

/// A `CellGrid` as ratatui text: each cell's glyph with its truecolor
/// foreground and background. A grid wider than `width` wraps into bands —
/// each further `width` columns continue below, separated by a blank row —
/// so a banner that runs off the pane reads on instead of clipping.
fn grid_text(grid: &CellGrid, width: usize) -> Text<'static> {
    let bands = band_count(grid.cols(), width);
    let mut lines = Vec::with_capacity(bands * (grid.rows() + 1));
    for band in 0..bands {
        if band > 0 {
            lines.push(Line::default());
        }
        let start = band * width;
        let end = if width == 0 {
            grid.cols()
        } else {
            (start + width).min(grid.cols())
        };
        for row in 0..grid.rows() {
            let mut spans = Vec::with_capacity(end - start);
            for col in start..end {
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
        Control::Style => app.style.clone().unwrap_or_else(|| "custom".into()),
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

/// The font browser panel: the filter line, then a window of the filtered
/// families scrolled to keep the selection visible.
fn fonts_panel(app: &App, height: usize) -> Vec<Line<'static>> {
    let list = app.filtered_fonts();
    let mut lines = vec![Line::from(vec![
        Span::styled("  filter   ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!(" {} ", app.font_filter),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    ])];
    if list.is_empty() {
        lines.push(Line::from(format!(
            "  no family matches '{}'",
            app.font_filter
        )));
        return lines;
    }
    let rows = height.saturating_sub(1).max(1);
    let sel = app.font_sel.min(list.len() - 1);
    // Scroll the window so the selection sits inside it.
    let offset = sel
        .saturating_sub(rows / 2)
        .min(list.len().saturating_sub(rows));
    for (i, family) in list.iter().enumerate().skip(offset).take(rows) {
        let (marker, style) = if i == sel {
            (
                "▸ ",
                Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED),
            )
        } else {
            ("  ", Style::default())
        };
        lines.push(Line::from(vec![
            Span::raw(marker),
            Span::styled(format!(" {family} "), style),
        ]));
    }
    lines
}

fn draw(app: &mut App, frame: &mut Frame) {
    let [body, status_bar] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());
    let [panel, preview] =
        Layout::horizontal([Constraint::Length(34), Constraint::Min(10)]).areas(body);

    let (panel_title, lines) = match app.view {
        View::Recipe => (
            " recipe ".to_string(),
            CONTROLS.iter().map(|c| control_line(app, *c)).collect(),
        ),
        View::Fonts => (
            format!(" fonts {}/{} ", app.filtered_fonts().len(), app.fonts.len()),
            fonts_panel(app, panel.height.saturating_sub(2) as usize),
        ),
    };
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(panel_title)),
        panel,
    );

    app.refresh_preview();
    let inner_width = preview.width.saturating_sub(2) as usize;
    let bands = match &app.preview {
        Ok(grid) => band_count(grid.cols(), inner_width),
        Err(_) => 1,
    };
    let title = if bands > 1 {
        format!(" preview · wrapped ×{bands} ")
    } else {
        " preview ".to_string()
    };
    let preview_block = Block::default().borders(Borders::ALL).title(title);
    let inner = preview_block.inner(preview);
    frame.render_widget(preview_block, preview);
    match &app.preview {
        Ok(grid) => {
            let text = grid_text(grid, inner.width as usize);
            // Center when everything fits; clip the tail when it does not.
            let top = inner
                .height
                .saturating_sub(text.lines.len() as u16)
                .saturating_div(2);
            let area = Rect {
                y: inner.y + top,
                height: inner.height.saturating_sub(top),
                ..inner
            };
            frame.render_widget(Clear, inner);
            frame.render_widget(Paragraph::new(text), area);
        }
        Err(msg) => {
            frame.render_widget(
                Paragraph::new(msg.clone()).style(Style::default().fg(Color::Red)),
                inner,
            );
        }
    }

    let help = match (app.view, app.mode) {
        (View::Fonts, _) => "type to filter · ↑↓ pick · enter keep · esc revert",
        (_, Mode::Navigate) => "↑↓ control · ←→ change · enter edit · f fonts · s save · q quit",
        (_, Mode::Edit) => "type to edit · enter/esc done",
    };
    // The transient message, then the standing conditions: a half-typed
    // palette name (the pipeline still paints with the last valid one), and
    // the notices the opened file arrived with.
    let mut parts = Vec::new();
    if !app.status.is_empty() {
        parts.push(app.status.clone());
    }
    if app.style.is_some() && presets::resolve_colors(&app.colors).is_none() {
        parts.push(format!("colors '{}' not resolvable yet", app.colors));
    }
    if !app.notice.is_empty() {
        parts.push(app.notice.clone());
    }
    parts.push(help.to_string());
    frame.render_widget(
        Paragraph::new(parts.join("   ")).style(Style::default().fg(Color::DarkGray)),
        status_bar,
    );
}

fn handle(app: &mut App, event: Event) {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            // A new keystroke supersedes the message about the last one.
            app.status.clear();
            app.on_key(key.code, key.modifiers);
        }
        // A recipe with `fit: terminal` measures at render time.
        Event::Resize(..) => app.dirty = true,
        _ => {}
    }
}

pub fn run(app: &mut App, terminal: &mut DefaultTerminal) -> io::Result<()> {
    while !app.quit {
        terminal.draw(|frame| draw(app, frame))?;
        // Block for the first event, then drain whatever arrived while the
        // frame rendered: a typing burst costs one render at its end, not
        // one per key queued behind the last.
        handle(app, event::read()?);
        while !app.quit && event::poll(std::time::Duration::ZERO)? {
            handle(app, event::read()?);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An App as `tui <text>` builds it: a flag-built document a preset
    /// describes.
    fn app_with(recipe: Recipe) -> App {
        App::new(
            recipe,
            "out.json".into(),
            false,
            Some("plain".into()),
            "omarchy".into(),
            presets::DEFAULT_WEIGHT,
        )
    }

    /// An App as `tui --recipe file.json` builds it: a custom pipeline.
    fn app_with_loaded(recipe: Recipe) -> App {
        App::new(
            recipe,
            "out.json".into(),
            true,
            None,
            "omarchy".into(),
            presets::DEFAULT_WEIGHT,
        )
    }

    fn focus_on(app: &mut App, control: Control) {
        app.focus = CONTROLS.iter().position(|c| *c == control).unwrap();
    }

    #[test]
    fn rebuilding_the_pipeline_keeps_unknown_stages() {
        // The editor must not destroy an effect this build cannot draw
        // (ADR-202), even when the style control replaces the pipeline.
        let json = r##"{"text":"x","pipeline":[
            {"op":"fill","kind":"solid","color":"#ffffff"},
            {"op":"warp","amplitude":3}]}"##;
        let mut app = app_with(Recipe::from_json(json).unwrap());
        app.style = Some("trap".into());
        app.rebuild_pipeline();
        assert_eq!(app.recipe.unknown_ops(), vec!["warp"]);
        assert!(app.recipe.ops().count() >= 2, "the trap pipeline landed");
    }

    #[test]
    fn a_loaded_pipeline_survives_weight_and_palette_touches() {
        // A file's pipeline is custom: nudging weight or colors must not
        // replace effects the controls did not make (ADR-202).
        let json = r##"{"text":"x","pipeline":[
            {"op":"rim","width":5,"kind":"solid","color":"#123456"}]}"##;
        let mut app = app_with_loaded(Recipe::from_json(json).unwrap());
        let before = app.recipe.pipeline.clone();
        focus_on(&mut app, Control::Weight);
        app.adjust(1);
        focus_on(&mut app, Control::Palette);
        app.adjust(1);
        assert_eq!(app.recipe.pipeline, before, "custom pipeline untouched");

        // Picking a style is the deliberate hand-over.
        focus_on(&mut app, Control::Style);
        app.adjust(1);
        assert_ne!(app.recipe.pipeline, before, "a chosen preset rebuilds");
    }

    #[test]
    fn a_newer_schema_recipe_saves_only_on_a_second_s() {
        // The wire parser drops top-level fields it does not know, so a
        // save of a newer file is lossy and wants a deliberate confirm.
        let dir = std::env::temp_dir().join(format!("dotbanner-tui-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("newer.json");
        let recipe = Recipe::from_json(r#"{"version":99,"text":"x"}"#).unwrap();
        let mut app = App::new(
            recipe,
            path.to_string_lossy().into_owned(),
            true,
            None,
            "omarchy".into(),
            1,
        );
        app.on_key(KeyCode::Char('s'), KeyModifiers::NONE);
        assert!(!path.exists(), "the first s only warns");
        app.on_key(KeyCode::Char('s'), KeyModifiers::NONE);
        assert!(path.exists(), "the second s saves");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saving_over_a_foreign_file_takes_a_second_s() {
        let dir = std::env::temp_dir().join(format!("dotbanner-tui-clob-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("existing.json");
        std::fs::write(&path, "precious").unwrap();
        let mut app = app_with(Recipe::new("hi"));
        app.path = path.to_string_lossy().into_owned();
        app.on_key(KeyCode::Char('s'), KeyModifiers::NONE);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "precious",
            "the first s only warns"
        );
        app.on_key(KeyCode::Char('s'), KeyModifiers::NONE);
        assert!(
            std::fs::read_to_string(&path).unwrap().contains("\"text\""),
            "the second s overwrites"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_chorded_character_is_not_text() {
        let mut app = app_with(Recipe::new("hi"));
        app.mode = Mode::Edit;
        app.on_key(KeyCode::Char('x'), KeyModifiers::CONTROL);
        assert_eq!(app.recipe.text, "hi");
        app.on_key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(app.quit, "ctrl-c quits even while editing");
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
        app.refresh_preview();
        assert_eq!(app.recipe.size.fit, None, "fit belongs to the pane");
    }

    #[test]
    fn a_wide_grid_wraps_into_bands() {
        use dotbanner_core::symbolizer::{symbolize, Mask, SymbolSet};
        // 12×12 pixels at the blocks register's 2×2 sub-blocks per cell:
        // a 6×6 cell grid.
        let mask = Mask::from_sketch(&["############"; 12].join("\n"));
        let grid = symbolize(&mask, SymbolSet::Blocks);
        assert_eq!((grid.cols(), grid.rows()), (6, 6));

        let wrapped = grid_text(&grid, 3);
        assert_eq!(wrapped.lines.len(), 13, "two bands and a separator row");
        let flat = grid_text(&grid, 10);
        assert_eq!(flat.lines.len(), 6, "a fitting grid does not wrap");

        assert_eq!(band_count(0, 5), 1);
        assert_eq!(band_count(5, 0), 1, "a zero-width pane must not divide");
        assert_eq!(band_count(10, 4), 3);
    }

    #[test]
    fn the_font_browser_previews_keeps_and_reverts() {
        let mut app = app_with(Recipe::new("hi"));
        if app.fonts.len() < 2 {
            return; // Not enough fonts installed to browse.
        }
        let original = app.recipe.font.clone();

        // Browsing writes the highlighted family into the document; Esc
        // puts the original back.
        app.on_key(KeyCode::Char('f'), KeyModifiers::NONE);
        assert_eq!(app.view, View::Fonts);
        app.font_sel = 0;
        app.font_apply();
        app.font_browse(1);
        assert_eq!(app.recipe.font.family, app.filtered_fonts()[1]);
        app.on_key(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.view, View::Recipe);
        assert_eq!(app.recipe.font, original, "esc reverts the preview");

        // Enter keeps what the preview shows.
        app.on_key(KeyCode::Char('f'), KeyModifiers::NONE);
        app.font_sel = 0;
        app.font_apply();
        app.font_browse(1);
        let picked = app.recipe.font.family.clone();
        app.on_key(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.view, View::Recipe);
        assert_eq!(app.recipe.font.family, picked);
    }

    #[test]
    fn an_unmatched_font_filter_keeps_the_document() {
        let mut app = app_with(Recipe::new("hi"));
        if app.fonts.is_empty() {
            return;
        }
        app.on_key(KeyCode::Char('f'), KeyModifiers::NONE);
        let before = app.recipe.font.clone();
        // '@' appears in no family name, so no intermediate keystroke can
        // match either.
        for _ in 0..4 {
            app.on_key(KeyCode::Char('@'), KeyModifiers::NONE);
        }
        assert!(app.filtered_fonts().is_empty());
        assert_eq!(app.recipe.font, before, "no match, no change");
    }
}
