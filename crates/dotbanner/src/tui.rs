//! The interactive recipe editor (ADR-200): a parameter panel beside a live
//! preview. Every control writes a field of the one recipe document, the
//! preview re-renders from it, and `s` saves it as JSON — the same file the
//! CLI's `--recipe` reads.
//!
//! The preview draws the `CellGrid` directly into ratatui spans rather than
//! round-tripping through the ANSI sink; both read the same grid, so what
//! the panel shows is what `render` will print.

use std::io;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::{DefaultTerminal, Frame};

use dotbanner_core::{
    color::Rgb,
    engine::Paint,
    presets,
    recipe::{Recipe, Register, Stage},
    scheme,
    symbolizer::CellGrid,
};

/// A register's geometry, shown beside its name in the picker.
fn register_hint(r: &Register) -> &'static str {
    match r {
        Register::Blocks => "2×2 quadrants",
        Register::Facets => "2×2, corners as triangles",
        Register::Sextants => "2×3 semigraphics",
        Register::Braille => "2×4 dots",
        Register::Unknown(_) => "",
    }
}

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
        matches!(self, Control::Text | Control::Path)
    }

    /// Whether Enter opens a picker popover on this control.
    fn has_picker(self) -> bool {
        matches!(
            self,
            Control::Font | Control::Style | Control::Palette | Control::Register
        )
    }
}

/// What keystrokes currently mean: moving between controls, or typing into
/// one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Navigate,
    Edit,
}

/// A popover for choosing a selector control's value from its items —
/// the one convention every such setting shares: type to filter, ↑↓
/// previews the highlighted item live in the document, Enter keeps it,
/// Esc restores the state the picker opened on.
struct Picker {
    control: Control,
    filter: String,
    /// Selected row in the filtered list.
    sel: usize,
    /// Computed once at open — a palette row's ramp would otherwise
    /// re-parse every scheme file per frame.
    items: Vec<PickerItem>,
    /// Document and derived control state at open; Esc restores all of it.
    undo: (Recipe, Option<String>, String),
}

/// One row a picker offers: the value's name plus what shows it — a
/// palette's ramp, a register's geometry.
struct PickerItem {
    name: String,
    ramp: Vec<Rgb>,
    hint: &'static str,
}

impl PickerItem {
    fn plain(name: String) -> Self {
        Self {
            name,
            ramp: Vec::new(),
            hint: "",
        }
    }
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
    /// The open picker popover, when a selector control has one up.
    picker: Option<Picker>,
    status: String,
    /// Set when the document changed since the preview was last rendered.
    dirty: bool,
    /// When the pending render may run. Each change pushes it a cooldown
    /// ahead, so a held key scrolls the panel freely and the engine runs
    /// once, when the input pauses.
    render_after: Instant,
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

/// How long input must pause before a pending render runs. Above a key's
/// autorepeat interval, below what a hand notices on a single press.
const RENDER_COOLDOWN: Duration = Duration::from_millis(150);

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
            picker: None,
            status: String::new(),
            // Due already: the opening frame renders without a cooldown.
            dirty: true,
            render_after: Instant::now(),
            preview: Err("rendering…".into()),
            notice: notices.join(" · "),
            pending: Pending::None,
            quit: false,
        }
    }

    fn focused(&self) -> Control {
        CONTROLS[self.focus]
    }

    /// The document changed: re-render once input pauses for the cooldown.
    /// Every change pushes the deadline anew, so a scroll or a held slider
    /// moves the panel per keystroke and costs one render at its end.
    fn mark_changed(&mut self) {
        self.dirty = true;
        self.render_after = Instant::now() + RENDER_COOLDOWN;
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
        self.mark_changed();
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
                self.mark_changed();
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
                self.mark_changed();
            }
            Control::Rows => {
                let rows = self.recipe.size.rows as i64 + delta;
                self.recipe.size.rows = rows.clamp(1, 64) as usize;
                self.mark_changed();
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
                self.mark_changed();
            }
        }
    }

    /// Every item a control's picker can offer, in list order.
    fn picker_items(&self, control: Control) -> Vec<PickerItem> {
        match control {
            Control::Font => self.fonts.iter().cloned().map(PickerItem::plain).collect(),
            Control::Style => presets::STYLES
                .iter()
                .map(|s| PickerItem::plain(s.to_string()))
                .collect(),
            Control::Palette => scheme::all()
                .into_iter()
                .map(|s| PickerItem {
                    ramp: s.ramp(),
                    name: s.name,
                    hint: "",
                })
                .collect(),
            Control::Register => REGISTERS
                .iter()
                .map(|r| PickerItem {
                    name: r.as_str().to_string(),
                    ramp: Vec::new(),
                    hint: register_hint(r),
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The open picker's item names after its filter.
    fn picker_list(&self) -> Vec<String> {
        let Some(p) = &self.picker else {
            return Vec::new();
        };
        filter_items(&p.items, &p.filter)
            .into_iter()
            .map(|i| i.name.clone())
            .collect()
    }

    /// The value the picker's control currently holds, as an item string.
    fn picker_current(&self, control: Control) -> String {
        match control {
            Control::Font => self.recipe.font.family.clone(),
            Control::Style => self.style.clone().unwrap_or_else(|| "custom".into()),
            Control::Palette => self.colors.clone(),
            Control::Register => self.recipe.symbolizer.body.as_str().to_string(),
            _ => String::new(),
        }
    }

    /// Open a picker on a control, selecting its current value. A current
    /// value no item carries (a custom style, a hex list, an unknown
    /// register, an uninstalled family) leaves the selection at the top,
    /// unapplied.
    fn open_picker(&mut self, control: Control) {
        let current = self.picker_current(control);
        let items = self.picker_items(control);
        let sel = items
            .iter()
            .position(|i| i.name.eq_ignore_ascii_case(&current))
            .unwrap_or(0);
        self.picker = Some(Picker {
            control,
            filter: String::new(),
            sel,
            items,
            undo: (self.recipe.clone(), self.style.clone(), self.colors.clone()),
        });
    }

    /// Write the picker's selection into the document, so the preview
    /// shows the highlighted item as it moves.
    fn picker_apply(&mut self) {
        let Some(p) = &self.picker else { return };
        let control = p.control;
        let sel = p.sel;
        let list = self.picker_list();
        if list.is_empty() {
            return;
        }
        let sel = sel.min(list.len() - 1);
        if let Some(p) = &mut self.picker {
            p.sel = sel;
        }
        let item = list[sel].clone();
        if self.picker_current(control).eq_ignore_ascii_case(&item) {
            return;
        }
        match control {
            Control::Font => {
                self.recipe.font.family = item;
                // A face name is per family (see the font control).
                self.recipe.font.style = None;
                self.mark_changed();
            }
            Control::Style => {
                self.style = Some(item);
                self.rebuild_pipeline();
            }
            // With a custom pipeline nothing paints with a palette, so
            // refuse before touching the spec — the same guard the panel
            // control has. Writing `colors` anyway would let Enter keep a
            // palette a later style pick silently adopts.
            Control::Palette if self.style.is_none() => {
                self.status = "the pipeline is custom — pick a style to use colors".into();
            }
            Control::Palette => {
                self.colors = item;
                self.rebuild_pipeline();
            }
            Control::Register => {
                if let Some(r) = REGISTERS.iter().find(|r| r.as_str() == item) {
                    self.recipe.symbolizer.body = r.clone();
                    self.mark_changed();
                }
            }
            _ => {}
        }
    }

    /// Move the picker selection by `delta`, stopping at the list's ends —
    /// a picker walks, it does not wrap. Only a selection that actually
    /// moved applies: Up at the top of the list is not a choice.
    fn picker_browse(&mut self, delta: i64) {
        let len = self.picker_list().len();
        let Some(p) = &mut self.picker else { return };
        if len == 0 {
            return;
        }
        let sel = p.sel.min(len - 1);
        let next = (sel as i64 + delta).clamp(0, len as i64 - 1) as usize;
        p.sel = next;
        if next != sel {
            self.picker_apply();
        }
    }

    /// Reselect after a filter change. A keystroke that edits the filter
    /// expresses no choice, so the document moves only when the filter
    /// excludes a value an item carries. A value no item ever carried — a
    /// custom style, a hex list, an unknown register — stays put until ↑↓
    /// or Enter chooses (ADR-202: the register comment above REGISTERS is
    /// a promise).
    fn picker_reselect(&mut self) {
        let Some(p) = &self.picker else { return };
        let current = self.picker_current(p.control);
        let known = p
            .items
            .iter()
            .any(|i| i.name.eq_ignore_ascii_case(&current));
        let list = self.picker_list();
        let at = list.iter().position(|i| i.eq_ignore_ascii_case(&current));
        let Some(p) = &mut self.picker else { return };
        match at {
            Some(at) => p.sel = at,
            None => {
                p.sel = 0;
                if known {
                    self.picker_apply();
                }
            }
        }
    }

    /// Close the picker, restoring the state it opened on.
    fn picker_revert(&mut self) {
        let Some(p) = self.picker.take() else { return };
        let (recipe, style, colors) = p.undo;
        if self.recipe != recipe || self.style != style || self.colors != colors {
            self.recipe = recipe;
            self.style = style;
            self.colors = colors;
            self.mark_changed();
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
        if self.picker.is_some() {
            match code {
                // Enter chooses the highlighted item — the one keystroke
                // that IS a choice, so a selection browsing never applied
                // (a custom style's top row, say) applies here — then
                // closes. Esc puts back everything the picker opened on.
                KeyCode::Enter => {
                    self.picker_apply();
                    self.picker = None;
                }
                KeyCode::Esc => self.picker_revert(),
                KeyCode::Up => self.picker_browse(-1),
                KeyCode::Down => self.picker_browse(1),
                KeyCode::PageUp => self.picker_browse(-10),
                KeyCode::PageDown => self.picker_browse(10),
                KeyCode::Backspace => {
                    let popped = self
                        .picker
                        .as_mut()
                        .is_some_and(|p| p.filter.pop().is_some());
                    if popped {
                        self.picker_reselect();
                    }
                }
                KeyCode::Char(c)
                    if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    if let Some(p) = &mut self.picker {
                        p.filter.push(c);
                    }
                    self.picker_reselect();
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
                        self.mark_changed();
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
                        self.mark_changed();
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
            KeyCode::Char('f') => self.open_picker(Control::Font),
            KeyCode::Char('c') => self.open_picker(Control::Palette),
            KeyCode::Enter if self.focused().has_picker() => self.open_picker(self.focused()),
            KeyCode::Enter if self.focused().takes_text() => self.mode = Mode::Edit,
            // Raw hex lists still want free-text entry, which Enter now
            // spends on the picker.
            KeyCode::Char('e') if self.focused() == Control::Palette => self.mode = Mode::Edit,
            _ => {}
        }
    }

    /// Re-render the preview when the document changed. The banner renders
    /// at the document's own size — fitting the pane is the wrap's job in
    /// `grid_text` — so the grid is exactly what `render` would print.
    fn refresh_preview(&mut self) {
        if !self.dirty || Instant::now() < self.render_after {
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

/// Where a `rows`-tall window onto a `len`-long list starts so that `sel`
/// sits inside it, near the middle where the list allows.
fn window_offset(sel: usize, len: usize, rows: usize) -> usize {
    sel.saturating_sub(rows / 2).min(len.saturating_sub(rows))
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

/// A palette ramp as a truecolor bar.
fn swatch_spans(ramp: &[Rgb]) -> Vec<Span<'static>> {
    const WIDTH: usize = 16;
    if ramp.is_empty() {
        return Vec::new();
    }
    let paint = Paint::Bands {
        stops: ramp.to_vec(),
        steps: None,
    };
    (0..WIDTH)
        .map(|i| {
            let c = paint.color_at(i as f32 / (WIDTH - 1) as f32);
            Span::styled("█", Style::default().fg(Color::Rgb(c.r, c.g, c.b)))
        })
        .collect()
}

/// The items matching a picker's filter, in list order. The list and the
/// popup both read this, so the highlighted row and the applied item
/// cannot diverge.
fn filter_items<'a>(items: &'a [PickerItem], filter: &str) -> Vec<&'a PickerItem> {
    let filter = filter.to_ascii_lowercase();
    items
        .iter()
        .filter(|i| filter.is_empty() || i.name.to_ascii_lowercase().contains(&filter))
        .collect()
}

/// The picker popover, centered over the editor: the filter line, then a
/// window of the filtered items scrolled to keep the selection visible.
/// Palette rows carry their ramp as a swatch, register rows their
/// geometry.
fn picker_popup(app: &App, frame: &mut Frame, body: Rect) {
    let Some(p) = &app.picker else { return };
    // A popup that cannot show its border, filter line, and one row has
    // nothing to draw — and clamp(4, height) would panic below 4.
    if body.height < 4 || body.width < 8 {
        return;
    }
    let items = filter_items(&p.items, &p.filter);

    let width = 46.min(body.width);
    let height = (items.len() as u16 + 3).clamp(4, body.height);
    let area = Rect {
        x: body.x + (body.width - width) / 2,
        y: body.y + (body.height - height) / 2,
        width,
        height,
    };

    let mut lines = vec![Line::from(vec![
        Span::styled("  filter   ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!(" {} ", p.filter),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    ])];
    if items.is_empty() {
        lines.push(Line::from(format!("  nothing matches '{}'", p.filter)));
    } else {
        let rows = (height as usize).saturating_sub(3).max(1);
        let sel = p.sel.min(items.len() - 1);
        let offset = window_offset(sel, items.len(), rows);
        for (i, item) in items.iter().enumerate().skip(offset).take(rows) {
            let (marker, style) = if i == sel {
                (
                    "▸ ",
                    Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED),
                )
            } else {
                ("  ", Style::default())
            };
            let mut spans = vec![
                Span::raw(marker),
                Span::styled(format!(" {:<18} ", item.name), style),
            ];
            spans.extend(swatch_spans(&item.ramp));
            if !item.hint.is_empty() {
                spans.push(Span::styled(
                    item.hint,
                    Style::default().fg(Color::DarkGray),
                ));
            }
            lines.push(Line::from(spans));
        }
    }

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(format!(
            " {} {}/{} ",
            p.control.label(),
            items.len(),
            p.items.len()
        ))),
        area,
    );
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

    app.refresh_preview();
    let inner = Block::default().borders(Borders::ALL).inner(preview);
    let bands = match &app.preview {
        Ok(grid) => band_count(grid.cols(), inner.width as usize),
        Err(_) => 1,
    };
    // While a render waits out the cooldown the pane shows the last grid;
    // the ellipsis says so.
    let pending = if app.dirty { "… " } else { " " };
    let title = if bands > 1 {
        format!(" preview · wrapped ×{bands}{pending}")
    } else {
        format!(" preview{pending}")
    };
    frame.render_widget(Block::default().borders(Borders::ALL).title(title), preview);
    match &app.preview {
        Ok(grid) => {
            let text = grid_text(grid, inner.width as usize);
            // Center when everything fits; clip the tail when it does not.
            // The count stays in usize until after the subtraction: a
            // pathological wrap can exceed u16.
            let top = (inner.height as usize).saturating_sub(text.lines.len()) / 2;
            let area = Rect {
                y: inner.y + top as u16,
                height: inner.height.saturating_sub(top as u16),
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

    picker_popup(app, frame, body);

    let help = if app.picker.is_some() {
        "type to filter · ↑↓ pick · enter choose · esc revert".to_string()
    } else {
        match app.mode {
            Mode::Navigate => {
                // Hex lists lost Enter to the picker; say where they went.
                let hex = if app.focused() == Control::Palette {
                    " · e hex"
                } else {
                    ""
                };
                format!(
                    "↑↓ control · ←→ change · enter pick/edit · f fonts · c colors{hex} · s save · q quit"
                )
            }
            Mode::Edit => "type to edit · enter/esc done".to_string(),
        }
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
        Event::Resize(..) => app.mark_changed(),
        _ => {}
    }
}

pub fn run(app: &mut App, terminal: &mut DefaultTerminal) -> io::Result<()> {
    while !app.quit {
        terminal.draw(|frame| draw(app, frame))?;
        // Wait for the next event — or, when a render is pending, only
        // until its cooldown expires, so the pause in the input is itself
        // what triggers the render. Each frame drains every queued event
        // before drawing again. The idle arm stays a long poll rather
        // than a blocking read(): one wait shape is what lets the dirty
        // deadline double as the timeout.
        let wait = if app.dirty {
            app.render_after
                .saturating_duration_since(Instant::now())
                .max(Duration::from_millis(1))
        } else {
            Duration::from_secs(3600)
        };
        if event::poll(wait)? {
            handle(app, event::read()?);
            while !app.quit && event::poll(Duration::ZERO)? {
                handle(app, event::read()?);
            }
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

    /// Force the open picker's selection to `sel` and apply it, the way a
    /// browse movement would.
    fn pick(app: &mut App, sel: usize) {
        if let Some(p) = &mut app.picker {
            p.sel = sel;
        }
        app.picker_apply();
    }

    #[test]
    fn the_font_picker_previews_keeps_and_reverts() {
        let mut app = app_with(Recipe::new("hi"));
        if app.fonts.len() < 2 {
            return; // Not enough fonts installed to browse.
        }
        let original = app.recipe.font.clone();

        // Browsing writes the highlighted family into the document; Esc
        // puts the original back.
        app.on_key(KeyCode::Char('f'), KeyModifiers::NONE);
        assert!(app.picker.is_some());
        pick(&mut app, 0);
        app.picker_browse(1);
        assert_eq!(app.recipe.font.family, app.picker_list()[1]);
        app.on_key(KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.picker.is_none());
        assert_eq!(app.recipe.font, original, "esc reverts the preview");

        // Enter keeps what the preview shows.
        app.on_key(KeyCode::Char('f'), KeyModifiers::NONE);
        pick(&mut app, 0);
        app.picker_browse(1);
        let picked = app.recipe.font.family.clone();
        app.on_key(KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.picker.is_none());
        assert_eq!(app.recipe.font.family, picked);
    }

    #[test]
    fn filter_edits_that_still_match_keep_the_document_family() {
        let mut app = app_with(Recipe::new("hi"));
        if app.fonts.len() < 2 {
            return;
        }
        // The last family, so a wrong reset would land on a different one.
        let family = app.fonts.last().unwrap().clone();
        app.recipe.font.family = family.clone();
        app.on_key(KeyCode::Char('f'), KeyModifiers::NONE);

        // Backspace on an empty filter is a no-op edit.
        app.on_key(KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(app.recipe.font.family, family);

        // Typing the family's own name matches it at every keystroke, so
        // no keystroke expresses a new choice.
        for c in family.to_ascii_lowercase().chars().take(4) {
            app.on_key(KeyCode::Char(c), KeyModifiers::NONE);
        }
        assert_eq!(app.recipe.font.family, family);
    }

    #[test]
    fn the_palette_picker_carries_swatches_and_reverts_the_pipeline() {
        let mut app = app_with(Recipe::new("hi"));
        app.rebuild_pipeline();
        let pipeline = app.recipe.pipeline.clone();

        app.on_key(KeyCode::Char('c'), KeyModifiers::NONE);
        let p = app.picker.as_ref().unwrap();
        assert_eq!(p.control, Control::Palette);
        assert!(
            p.items.iter().all(|i| i.ramp.len() >= 3),
            "every palette row carries its ramp for the swatch"
        );
        assert_eq!(
            p.items[p.sel].name, "omarchy",
            "opens on the current palette"
        );

        // Browsing repaints the pipeline; Esc restores palette and
        // pipeline both.
        app.picker_browse(1);
        assert_ne!(app.recipe.pipeline, pipeline);
        app.on_key(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.colors, "omarchy");
        assert_eq!(app.recipe.pipeline, pipeline, "esc restores the pipeline");
    }

    #[test]
    fn a_tiny_terminal_cannot_panic_the_popup() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let mut app = app_with(Recipe::new("hi"));
        app.open_picker(Control::Register);
        for (w, h) in [(10u16, 3u16), (5, 4), (80, 2), (2, 2), (46, 5)] {
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            t.draw(|f| draw(&mut app, f)).unwrap();
        }
    }

    #[test]
    fn a_filter_keystroke_never_applies_over_a_value_no_item_carries() {
        // A custom pipeline's style is a value no picker item carries, so
        // typing in the style picker must not hand the pipeline over.
        let json = r##"{"text":"x","pipeline":[
            {"op":"rim","width":5,"kind":"solid","color":"#123456"}]}"##;
        let mut app = app_with_loaded(Recipe::from_json(json).unwrap());
        let before = app.recipe.pipeline.clone();
        focus_on(&mut app, Control::Style);
        app.on_key(KeyCode::Enter, KeyModifiers::NONE);
        app.on_key(KeyCode::Char('b'), KeyModifiers::NONE);
        assert_eq!(app.recipe.pipeline, before, "a filter edit is not a choice");
        assert_eq!(app.style, None);

        // Up at the top of the list is not a choice either.
        app.on_key(KeyCode::Backspace, KeyModifiers::NONE);
        app.on_key(KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(app.recipe.pipeline, before);

        // Enter is: it chooses the highlighted item.
        app.on_key(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.style.as_deref(), Some("plain"));
        assert_ne!(app.recipe.pipeline, before);
    }

    #[test]
    fn an_unknown_register_survives_picker_filtering() {
        // The comment above REGISTERS is a promise (ADR-202): an unknown
        // register keeps its name until the user moves the control.
        let json = r##"{"text":"x","symbolizer":{"body":"hexants"}}"##;
        let mut app = app_with_loaded(Recipe::from_json(json).unwrap());
        focus_on(&mut app, Control::Register);
        app.on_key(KeyCode::Enter, KeyModifiers::NONE);
        app.on_key(KeyCode::Char('b'), KeyModifiers::NONE);
        assert_eq!(app.recipe.symbolizer.body.as_str(), "hexants");
        app.on_key(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.recipe.symbolizer.body.as_str(), "hexants");
    }

    #[test]
    fn the_palette_picker_refuses_a_custom_pipeline() {
        let json = r##"{"text":"x","pipeline":[
            {"op":"rim","width":5,"kind":"solid","color":"#123456"}]}"##;
        let mut app = app_with_loaded(Recipe::from_json(json).unwrap());
        app.on_key(KeyCode::Char('c'), KeyModifiers::NONE);
        app.picker_browse(1);
        assert_eq!(
            app.colors, "omarchy",
            "nothing paints with a palette here, so the spec must not move"
        );
        assert!(app.status.contains("custom"));
    }

    #[test]
    fn the_style_picker_hands_a_custom_pipeline_over_deliberately() {
        let json = r##"{"text":"x","pipeline":[
            {"op":"rim","width":5,"kind":"solid","color":"#123456"}]}"##;
        let mut app = app_with_loaded(Recipe::from_json(json).unwrap());
        let before = app.recipe.pipeline.clone();

        // Opening the style picker applies nothing by itself.
        focus_on(&mut app, Control::Style);
        app.on_key(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.recipe.pipeline, before);

        // Esc keeps the custom pipeline and the custom marker.
        app.picker_browse(1);
        assert_ne!(app.recipe.pipeline, before, "a browse move applies");
        app.on_key(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.recipe.pipeline, before);
        assert_eq!(app.style, None, "still custom after revert");
    }

    #[test]
    fn a_change_renders_only_after_the_cooldown() {
        let mut app = app_with(Recipe::new("hi"));
        // The opening render is due immediately.
        app.refresh_preview();
        assert!(!app.dirty);

        app.on_key(KeyCode::Down, KeyModifiers::NONE);
        assert!(!app.dirty, "a focus move does not touch the document");

        focus_on(&mut app, Control::Rows);
        app.adjust(1);
        assert!(app.dirty);
        app.refresh_preview();
        assert!(app.dirty, "within the cooldown the render waits");

        app.render_after = Instant::now()
            .checked_sub(Duration::from_millis(1))
            .unwrap_or_else(Instant::now);
        app.refresh_preview();
        assert!(!app.dirty, "past the deadline the render runs");
    }

    #[test]
    fn window_offset_keeps_the_selection_visible() {
        for (sel, len, rows) in [
            (0, 100, 10),
            (50, 100, 10),
            (99, 100, 10),
            (0, 3, 10),
            (2, 3, 1),
            (5, 6, 3),
        ] {
            let off = window_offset(sel, len, rows);
            assert!(off <= sel, "({sel},{len},{rows}): window starts past it");
            assert!(
                sel < off + rows,
                "({sel},{len},{rows}): window ends before it"
            );
        }
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
        assert!(app.picker_list().is_empty());
        assert_eq!(app.recipe.font, before, "no match, no change");
    }
}
