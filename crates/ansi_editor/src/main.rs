#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cli;
mod font_library;
mod glyph;
mod image_export;
#[cfg(target_os = "macos")]
mod macos_open;
mod minimap;
mod updates;

use ansi_core::{
    ans_io::{self, Document},
    canvas::{Canvas, Cell, Color, History, DEFAULT_HEIGHT, DEFAULT_WIDTH},
    encoding::{decode_cp437, Encoding},
    gradient::{gradient_at, GradientPalette},
    sauce::Sauce,
    tdf,
};
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Sense, Vec2};
use egui_material_icons::icons::{
    ICON_ADD, ICON_APPS, ICON_BRUSH, ICON_CENTER_FOCUS_STRONG, ICON_CHEVRON_RIGHT, ICON_COLORIZE,
    ICON_CONTENT_COPY, ICON_CONTENT_CUT, ICON_DRAG_HANDLE, ICON_EAST, ICON_EXPAND_MORE,
    ICON_FLASH_ON, ICON_FORMAT_BOLD, ICON_FORMAT_UNDERLINED, ICON_GRID_VIEW, ICON_NORTH,
    ICON_NORTH_WEST, ICON_PALETTE, ICON_SEARCH, ICON_SELECT_ALL, ICON_SOUTH, ICON_SOUTH_EAST,
    ICON_SWAP_HORIZ, ICON_SYNC_ALT, ICON_WEST, ICON_ZOOM_IN, ICON_ZOOM_OUT,
};
use egui_material_icons::MaterialIcon;
use std::path::PathBuf;

const MINIMAP_DEFAULT_WIDTH: f32 = 180.0;
const DEFAULT_WINDOW_SIZE: Vec2 = Vec2::new(1006.0 + MINIMAP_DEFAULT_WIDTH, 692.0);
const SIDEBAR_ORDER_KEY: &str = "sidebar_order_v1";
const SIDEBAR_COLLAPSED_KEY: &str = "sidebar_collapsed_v1";
const MINIMAP_VISIBLE_KEY: &str = "minimap_visible_v1";
const APP_ICON_PNG: &[u8] = include_bytes!("../assets/adraw.png");
const GITHUB_PAGE: &str = "https://github.com/mikehodgson/adraw";
const ZOOM_LEVELS: [u16; 8] = [50, 75, 100, 125, 150, 200, 300, 400];
const DEFAULT_ZOOM_PERCENT: f32 = 100.0;
const REDO_SHORTCUT: egui::KeyboardShortcut = if cfg!(target_os = "macos") {
    egui::KeyboardShortcut::new(
        egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT),
        egui::Key::Z,
    )
} else {
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Y)
};

fn display_color(color: Color) -> Color32 {
    let [r, g, b] = color.rgb();
    Color32::from_rgb(r, g, b)
}

struct Editor {
    update_check: updates::UpdateCheck,
    about_open: bool,
    about_logo: Option<egui::TextureHandle>,
    sidebar_order: Vec<SidebarSection>,
    sidebar_collapsed: Vec<SidebarSection>,
    minimap_visible: bool,
    minimap: minimap::Minimap,
    doc: Document,
    history: History,
    brush: Cell,
    encoding: Encoding,
    width: usize,
    height: usize,
    zoom_percent: f32,
    path: Option<PathBuf>,
    message: Option<String>,
    dirty: bool,
    pending: Option<PendingAction>,
    cursor: (usize, usize),
    picker_open: bool,
    selecting: bool,
    selection: Option<Selection>,
    selection_drag: Option<SelectionDrag>,
    /// RGB picker target and history revision of its latest selection recolor.
    recolor_merge: Option<(RgbTarget, u64)>,
    gradient_direction: GradientDirection,
    gradient_reversed: bool,
    last_gradient: Option<LastGradient>,
    /// Colors picked this session, listed under Used colors.
    picked_colors: Vec<Color>,
    /// Brush colors as of the last remembered pick.
    remembered_brush: (Color, Color),
    rgb_open: bool,
    pending_gradient_end: Option<Color>,
    eyedropper: bool,
    clipboard: Option<CellClipboard>,
    tdf_open: bool,
    tdf_library: font_library::FontLibrary,
    tdf_font: usize,
    tdf_text: String,
    tdf_spacing: usize,
    tdf_filter: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum SidebarSection {
    Brush,
    Zoom,
    Colors,
    Attributes,
    Sauce,
}

impl SidebarSection {
    const ALL: [Self; 5] = [
        Self::Brush,
        Self::Zoom,
        Self::Colors,
        Self::Attributes,
        Self::Sauce,
    ];

    fn id(self) -> &'static str {
        match self {
            Self::Brush => "brush",
            Self::Zoom => "zoom",
            Self::Colors => "colors",
            Self::Attributes => "attributes",
            Self::Sauce => "sauce",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Brush => "Brush",
            Self::Zoom => "Zoom",
            Self::Colors => "Colors",
            Self::Attributes => "Attributes",
            Self::Sauce => "SAUCE metadata",
        }
    }

    fn known(saved: &str) -> Vec<Self> {
        let mut order = Vec::new();
        for id in saved.lines() {
            if let Some(section) = Self::ALL.iter().copied().find(|section| section.id() == id) {
                if !order.contains(&section) {
                    order.push(section);
                }
            }
        }
        order
    }

    fn restore(saved: &str) -> Vec<Self> {
        let mut order = Self::known(saved);
        for section in Self::ALL {
            if !order.contains(&section) {
                order.push(section);
            }
        }
        order
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Selection {
    start: (usize, usize),
    end: (usize, usize),
}

impl Selection {
    fn bounds(self) -> (usize, usize, usize, usize) {
        (
            self.start.0.min(self.end.0),
            self.start.1.min(self.end.1),
            self.start.0.max(self.end.0),
            self.start.1.max(self.end.1),
        )
    }

    fn contains(self, x: usize, y: usize) -> bool {
        let (left, top, right, bottom) = self.bounds();
        x >= left && x <= right && y >= top && y <= bottom
    }
}

/// Which way selection gradients run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum GradientDirection {
    /// From where the selection drag began to where it was released.
    #[default]
    Drag,
    LeftToRight,
    TopToBottom,
}

/// The most recent selection gradient, so changing its direction can
/// replace it within the same undo step.
#[derive(Clone, Copy, Debug)]
struct LastGradient {
    pick: GradientPick,
    start: Color,
    selection: Selection,
    revision: u64,
}

struct SelectionDrag {
    source: Selection,
    destination: Selection,
    anchor: (usize, usize),
    cells: Vec<Cell>,
}

impl SelectionDrag {
    fn preview_cell(&self, x: usize, y: usize) -> Option<Cell> {
        if self.destination.contains(x, y) {
            let (left, top, right, _) = self.destination.bounds();
            Some(self.cells[(y - top) * (right - left + 1) + x - left])
        } else if self.source.contains(x, y) {
            Some(Cell::default())
        } else {
            None
        }
    }
}

struct CellClipboard {
    width: usize,
    height: usize,
    cells: Vec<Cell>,
    text: String,
}

#[derive(Clone)]
enum PendingAction {
    New,
    Open,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    OpenPath(PathBuf),
    Close,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            update_check: updates::UpdateCheck::default(),
            about_open: false,
            about_logo: None,
            sidebar_order: SidebarSection::ALL.to_vec(),
            sidebar_collapsed: Vec::new(),
            minimap_visible: false,
            minimap: minimap::Minimap::default(),
            doc: Document {
                canvas: Canvas::new(DEFAULT_WIDTH, DEFAULT_HEIGHT).unwrap(),
                sauce: Some(Sauce::default()),
            },
            history: History::default(),
            brush: Cell {
                ch: '█',
                ..Cell::default()
            },
            encoding: Encoding::Cp437,
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            zoom_percent: DEFAULT_ZOOM_PERCENT,
            path: None,
            message: None,
            dirty: false,
            pending: None,
            cursor: (0, 0),
            picker_open: false,
            selecting: false,
            selection: None,
            selection_drag: None,
            recolor_merge: None,
            gradient_direction: GradientDirection::default(),
            gradient_reversed: false,
            last_gradient: None,
            picked_colors: Vec::new(),
            remembered_brush: (Cell::default().fg, Cell::default().bg),
            rgb_open: false,
            pending_gradient_end: None,
            eyedropper: false,
            clipboard: None,
            tdf_open: false,
            tdf_library: font_library::FontLibrary::default(),
            tdf_font: 0,
            tdf_text: "ANSI".into(),
            tdf_spacing: 1,
            tdf_filter: String::new(),
        }
    }
}

impl Editor {
    fn new(storage: Option<&dyn eframe::Storage>) -> Self {
        Self::with_font_directory(
            storage,
            eframe::storage_dir("ADraw").map(|directory| directory.join("fonts")),
        )
    }

    fn with_font_directory(
        storage: Option<&dyn eframe::Storage>,
        font_directory: Option<PathBuf>,
    ) -> Self {
        let mut editor = Self::default();
        if let Some(storage) = storage {
            if let Some(saved) = storage.get_string(SIDEBAR_ORDER_KEY) {
                editor.sidebar_order = SidebarSection::restore(&saved);
            }
            if let Some(saved) = storage.get_string(SIDEBAR_COLLAPSED_KEY) {
                editor.sidebar_collapsed = SidebarSection::known(&saved);
            }
            if let Some(saved) = storage.get_string(MINIMAP_VISIBLE_KEY) {
                match saved.parse::<bool>() {
                    Ok(visible) => editor.minimap_visible = visible,
                    Err(_) => {
                        editor.message = Some(
                            "The saved minimap preference is invalid. Choose View > Show minimap to set it again."
                                .into(),
                        );
                    }
                }
            }
        } else {
            editor.message = Some(
                "Sidebar and minimap preferences cannot be saved: application settings storage is unavailable. Check access to your user profile's application data folder."
                    .into(),
            );
        }
        editor.tdf_library = font_library::FontLibrary::new(font_directory);
        let errors = editor.tdf_library.load();
        if !errors.is_empty() {
            let error = format!(
                "Some stored fonts could not be loaded:\n{}",
                errors.join("\n")
            );
            editor.message = Some(match editor.message.take() {
                Some(message) => format!("{message}\n\n{error}"),
                None => error,
            });
        }
        if let Some(entry) = editor.tdf_library.fonts.first() {
            editor.tdf_spacing = entry.font.spacing;
        }
        editor
    }

    fn move_sidebar_section(&mut self, section: SidebarSection, insertion: usize) {
        let source = self
            .sidebar_order
            .iter()
            .position(|&item| item == section)
            .expect("dragged sidebar section is registered");
        let destination = insertion - usize::from(source < insertion);
        if source != destination {
            self.sidebar_order.remove(source);
            self.sidebar_order.insert(destination, section);
        }
    }

    fn change_zoom(&mut self, direction: isize) {
        let next = if direction > 0 {
            ZOOM_LEVELS
                .iter()
                .copied()
                .find(|&zoom| f32::from(zoom) > self.zoom_percent)
        } else {
            ZOOM_LEVELS
                .iter()
                .rev()
                .copied()
                .find(|&zoom| f32::from(zoom) < self.zoom_percent)
        };
        if let Some(next) = next {
            self.zoom_percent = f32::from(next);
        }
    }

    fn zoom_controls(&mut self, ui: &mut egui::Ui) {
        ui.add_enabled_ui(self.selection_drag.is_none(), |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add_enabled_ui(self.zoom_percent > 50.0, |ui| {
                        icon_button(
                            ui,
                            ICON_ZOOM_OUT,
                            &format!("Zoom out ({})", shortcut_text(ui.ctx(), egui::Key::Minus)),
                            false,
                        )
                    })
                    .inner
                    .clicked()
                {
                    self.change_zoom(-1);
                }
                let picker = icon_button(ui, ICON_SEARCH, "Drag to zoom", false);
                let popup_id = ui.make_persistent_id("zoom_slider");
                if picker.clicked() {
                    egui::Popup::toggle_id(ui.ctx(), popup_id);
                }
                if ui
                    .add_enabled_ui(self.zoom_percent < 400.0, |ui| {
                        icon_button(
                            ui,
                            ICON_ZOOM_IN,
                            &format!("Zoom in ({})", shortcut_text(ui.ctx(), egui::Key::Plus)),
                            false,
                        )
                    })
                    .inner
                    .clicked()
                {
                    self.change_zoom(1);
                }
                if icon_button(
                    ui,
                    ICON_CENTER_FOCUS_STRONG,
                    &format!(
                        "Actual size (100%, {})",
                        shortcut_text(ui.ctx(), egui::Key::Num0)
                    ),
                    false,
                )
                .clicked()
                {
                    self.zoom_percent = DEFAULT_ZOOM_PERCENT;
                }
                ui.label(format!("{:.0}%", self.zoom_percent));
                egui::Popup::from_response(&picker)
                    .id(popup_id)
                    .open_memory(None)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        ui.set_width(240.0);
                        ui.spacing_mut().slider_width = 240.0;
                        ui.strong(format!("Zoom: {:.0}%", self.zoom_percent));
                        // Unequal zoom ranges use separate logarithmic halves to center 100%.
                        let logarithm = (self.zoom_percent / DEFAULT_ZOOM_PERCENT).log2();
                        let mut position = if logarithm < 0.0 {
                            logarithm
                        } else {
                            logarithm / 2.0
                        };
                        if ui
                            .add(egui::Slider::new(&mut position, -1.0..=1.0).show_value(false))
                            .changed()
                        {
                            let exponent = if position < 0.0 {
                                position
                            } else {
                                position * 2.0
                            };
                            self.zoom_percent = DEFAULT_ZOOM_PERCENT * 2.0_f32.powf(exponent);
                        }
                        ui.columns(3, |columns| {
                            columns[0].label("50%");
                            columns[1].with_layout(
                                egui::Layout::top_down(egui::Align::Center),
                                |ui| {
                                    ui.label("100%");
                                },
                            );
                            columns[2].with_layout(
                                egui::Layout::top_down(egui::Align::Max),
                                |ui| {
                                    ui.label("400%");
                                },
                            );
                        });
                    });
            });
        });
    }

    fn import_tdf(&mut self) {
        let Some(paths) = rfd::FileDialog::new()
            .add_filter("TheDraw fonts", &["tdf"])
            .pick_files()
        else {
            return;
        };
        let mut added = 0;
        let mut selected = None;
        let mut errors = Vec::new();
        for path in paths {
            match self.tdf_library.import(&path) {
                Ok(result) => {
                    added += result.added;
                    selected.get_or_insert(result.first);
                }
                Err(error) => errors.push(format!("{}: {error}", path.display())),
            }
        }
        if let Some(index) = selected {
            self.tdf_font = index;
            self.tdf_spacing = self.tdf_library.fonts[index].font.spacing;
            self.tdf_filter.clear();
        }
        if !errors.is_empty() {
            self.message = Some(format!(
                "Imported {} fonts. Some files could not be imported:\n{}",
                added,
                errors.join("\n")
            ));
        }
    }

    fn place_tdf_text(&mut self, text: &tdf::RenderedText) -> Result<(), String> {
        let (x, y) = self.cursor;
        if text.width > self.doc.canvas.width().saturating_sub(x)
            || text.height > self.doc.canvas.height().saturating_sub(y)
        {
            return Err(
                "Font text does not fit at the cursor; move it or resize the canvas".into(),
            );
        }
        self.dirty |= self.history.paint_many(
            &mut self.doc.canvas,
            text.cells
                .iter()
                .map(|&(dx, dy, cell)| (x + dx, y + dy, cell)),
        );
        self.selection = Some(Selection {
            start: (x, y),
            end: (x + text.width - 1, y + text.height - 1),
        });
        Ok(())
    }

    fn undo(&mut self) {
        if self.history.undo(&mut self.doc.canvas) {
            self.dirty = true;
            self.width = self.doc.canvas.width();
            self.height = self.doc.canvas.height();
            self.clamp_cursor();
        }
    }

    fn redo(&mut self) {
        if self.history.redo(&mut self.doc.canvas) {
            self.dirty = true;
            self.width = self.doc.canvas.width();
            self.height = self.doc.canvas.height();
            self.clamp_cursor();
        }
    }

    fn new_image(&mut self) {
        match Canvas::new(DEFAULT_WIDTH, DEFAULT_HEIGHT) {
            Ok(canvas) => {
                self.doc = Document {
                    canvas,
                    sauce: Some(Sauce::default()),
                };
                self.path = None;
                self.width = DEFAULT_WIDTH;
                self.height = DEFAULT_HEIGHT;
                self.history = History::default();
                self.minimap.invalidate();
                self.dirty = false;
                self.cursor = (0, 0);
                self.selection = None;
                self.selection_drag = None;
            }
            Err(error) => self.message = Some(error.to_string()),
        }
    }

    fn open(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("ANSI art", &["ans", "asc"])
            .pick_file()
        else {
            return;
        };
        self.open_path(path);
    }

    fn open_path(&mut self, path: PathBuf) {
        let result = std::fs::read(&path)
            .map_err(|e| format!("Could not read file: {e}"))
            .and_then(|bytes| ans_io::load_auto(&bytes, self.encoding));
        match result {
            Ok(doc) => {
                self.width = doc.canvas.width();
                self.height = doc.canvas.height();
                self.doc = doc;
                self.history = History::default();
                self.minimap.invalidate();
                self.path = Some(path);
                self.dirty = false;
                self.cursor = (0, 0);
                self.selection = None;
                self.selection_drag = None;
            }
            Err(error) => self.message = Some(error),
        }
    }

    fn save(&mut self, save_as: bool) {
        let path = if save_as || self.path.is_none() {
            rfd::FileDialog::new()
                .add_filter("ANSI art", &["ans"])
                .set_file_name("art.ans")
                .save_file()
        } else {
            self.path.clone()
        };
        let Some(path) = path else { return };
        let result = ans_io::save(&self.doc, self.encoding).and_then(|bytes| {
            std::fs::write(&path, bytes).map_err(|e| format!("Could not save file: {e}"))
        });
        match result {
            Ok(()) => {
                self.path = Some(path);
                self.dirty = false;
            }
            Err(error) => self.message = Some(error),
        }
    }

    fn selection_bytes(&self) -> Result<Vec<u8>, String> {
        let (left, top, right, bottom) = self.selection.ok_or("Select an area first")?.bounds();
        if right >= self.doc.canvas.width() || bottom >= self.doc.canvas.height() {
            return Err("Selection is outside the canvas".into());
        }
        ans_io::save_region(
            &self.doc,
            self.encoding,
            left,
            top,
            right - left + 1,
            bottom - top + 1,
        )
    }

    fn save_selection(&mut self) {
        let bytes = match self.selection_bytes() {
            Ok(bytes) => bytes,
            Err(error) => {
                self.message = Some(error);
                return;
            }
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("ANSI art", &["ans"])
            .set_file_name("selection.ans")
            .save_file()
        else {
            return;
        };
        if let Err(error) = std::fs::write(path, bytes) {
            self.message = Some(format!("Could not save selection: {error}"));
        }
    }

    fn export_image(&mut self, ctx: &egui::Context, format: image_export::Format) {
        let name = if self.selection.is_some() {
            "selection"
        } else {
            "art"
        };
        let Some(mut path) = rfd::FileDialog::new()
            .add_filter(format.label(), format.extensions())
            .set_file_name(format!("{name}.{}", format.extension()))
            .save_file()
        else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension(format.extension());
        }
        let result = self
            .rendered_image(ctx)
            .and_then(|image| image_export::save(&path, image, format));
        if let Err(error) = result {
            self.message = Some(error);
        }
    }

    fn rendered_image(&self, ctx: &egui::Context) -> Result<image::RgbImage, String> {
        let definitions = ctx.fonts(|fonts| fonts.definitions().clone());
        image_export::render(&self.doc, self.selection, definitions)
    }

    fn request(&mut self, action: PendingAction, ctx: &egui::Context) {
        if self.dirty {
            self.pending = Some(action);
        } else {
            self.execute(action, ctx);
        }
    }

    fn execute(&mut self, action: PendingAction, ctx: &egui::Context) {
        match action {
            PendingAction::New => self.new_image(),
            PendingAction::Open => self.open(),
            PendingAction::OpenPath(path) => self.open_path(path),
            PendingAction::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    fn clamp_cursor(&mut self) {
        self.cursor.0 = self.cursor.0.min(self.doc.canvas.width() - 1);
        self.cursor.1 = self.cursor.1.min(self.doc.canvas.height() - 1);
        self.selection = None;
        self.selection_drag = None;
    }

    fn selection_cells(&self, selection: Selection) -> Result<Vec<Cell>, String> {
        let (left, top, right, bottom) = selection.bounds();
        let mut cells = Vec::new();
        for y in top..=bottom {
            for x in left..=right {
                cells.push(
                    *self
                        .doc
                        .canvas
                        .get(x, y)
                        .ok_or("Selection is outside the canvas")?,
                );
            }
        }
        Ok(cells)
    }

    fn begin_selection_drag(&mut self, anchor: (usize, usize)) -> Result<(), String> {
        let source = self.selection.ok_or("Select an area first")?;
        let cells = self.selection_cells(source)?;
        self.selection_drag = Some(SelectionDrag {
            source,
            destination: source,
            anchor,
            cells,
        });
        Ok(())
    }

    fn update_selection_drag(&mut self, point: (usize, usize)) {
        if let Some(drag) = &mut self.selection_drag {
            let (left, top, right, bottom) = drag.source.bounds();
            let width = right - left + 1;
            let height = bottom - top + 1;
            let move_axis = |origin: usize, anchor: usize, point: usize, maximum: usize| {
                (origin as isize + point as isize - anchor as isize).clamp(0, maximum as isize)
                    as usize
            };
            let x = move_axis(
                left,
                drag.anchor.0,
                point.0,
                self.doc.canvas.width() - width,
            );
            let y = move_axis(
                top,
                drag.anchor.1,
                point.1,
                self.doc.canvas.height() - height,
            );
            // Translate the corners so the selection keeps its drag direction.
            let shift = |corner: (usize, usize)| (corner.0 - left + x, corner.1 - top + y);
            drag.destination = Selection {
                start: shift(drag.source.start),
                end: shift(drag.source.end),
            };
        }
    }

    fn finish_selection_drag(&mut self) {
        if let Some(drag) = self.selection_drag.take() {
            let (left, top, right, bottom) = drag.source.bounds();
            let (x, y, _, _) = drag.destination.bounds();
            if drag.source.bounds() != drag.destination.bounds() {
                // Snapshot cells before clearing so overlapping moves retain every cell.
                let clear = (top..=bottom)
                    .flat_map(|y| (left..=right).map(move |x| (x, y, Cell::default())));
                let width = right - left + 1;
                let place = drag
                    .cells
                    .into_iter()
                    .enumerate()
                    .map(|(index, cell)| (x + index % width, y + index / width, cell));
                self.dirty |= self
                    .history
                    .paint_many(&mut self.doc.canvas, clear.chain(place));
            }
            self.selection = Some(drag.destination);
            self.cursor = (x, y);
        }
    }

    fn recolor_selection(
        &mut self,
        fg: Option<Color>,
        bg: Option<Color>,
        merge: Option<u64>,
    ) -> bool {
        let Some(selection) = self.selection.filter(|_| self.selection_drag.is_none()) else {
            return false;
        };
        let (left, top, right, bottom) = selection.bounds();
        let canvas = &self.doc.canvas;
        let cells: Vec<_> = (top..=bottom)
            .flat_map(|y| (left..=right).map(move |x| (x, y)))
            .filter_map(|(x, y)| {
                let mut cell = *canvas.get(x, y)?;
                cell.fg = fg.unwrap_or(cell.fg);
                cell.bg = bg.unwrap_or(cell.bg);
                Some((x, y, cell))
            })
            .collect();
        let changed = match merge {
            Some(revision) => self
                .history
                .paint_many_merged(&mut self.doc.canvas, cells, revision),
            None => self.history.paint_many(&mut self.doc.canvas, cells),
        };
        self.dirty |= changed;
        changed
    }

    /// Fades the selection's foreground or background from the brush color to
    /// `pick.end` in the chosen gradient direction (by default, from the cell
    /// where the selection drag began to where it was released). The output
    /// palette is the richest of the two colors' palettes.
    fn apply_gradient(&mut self, pick: GradientPick, merge: Option<u64>) -> Result<bool, String> {
        let selection = self.selection.ok_or_else(|| {
            format!(
                "Select an area first, then {}+click a color to fade it",
                if cfg!(target_os = "macos") {
                    "Cmd"
                } else {
                    "Ctrl"
                }
            )
        })?;
        let start = if pick.background {
            self.brush.bg
        } else {
            self.brush.fg
        };
        self.paint_gradient(pick, start, selection, merge)
    }

    /// Changes the gradient direction. If the selection still holds the most
    /// recent gradient, it is redrawn in place as part of the same undo step.
    fn set_gradient_direction(
        &mut self,
        direction: GradientDirection,
        reversed: bool,
    ) -> Result<bool, String> {
        self.gradient_direction = direction;
        self.gradient_reversed = reversed;
        let Some(last) = self.last_gradient.filter(|last| {
            Some(last.selection) == self.selection && last.revision == self.history.revision()
        }) else {
            return Ok(false);
        };
        self.paint_gradient(last.pick, last.start, last.selection, Some(last.revision))
    }

    fn gradient_line(&self, selection: Selection) -> ((usize, usize), (usize, usize)) {
        let (left, top, right, bottom) = selection.bounds();
        let (from, to) = match self.gradient_direction {
            GradientDirection::Drag => (selection.start, selection.end),
            GradientDirection::LeftToRight => ((left, top), (right, top)),
            GradientDirection::TopToBottom => ((left, top), (left, bottom)),
        };
        if self.gradient_reversed {
            (to, from)
        } else {
            (from, to)
        }
    }

    fn paint_gradient(
        &mut self,
        pick: GradientPick,
        start: Color,
        selection: Selection,
        merge: Option<u64>,
    ) -> Result<bool, String> {
        if self.selection_drag.is_some() {
            return Err("Finish moving the selection first".into());
        }
        let palette = GradientPalette::for_endpoints(start, pick.end);
        let (from, to) = self.gradient_line(selection);
        let (left, top, right, _) = selection.bounds();
        let width = right - left + 1;
        let cells = self.selection_cells(selection)?;
        let changes: Vec<_> = cells
            .into_iter()
            .enumerate()
            .map(|(index, mut cell)| {
                let point = (left + index % width, top + index / width);
                let color = gradient_at(start, pick.end, from, to, point, palette);
                if pick.background {
                    cell.bg = color;
                } else {
                    cell.fg = color;
                }
                (point.0, point.1, cell)
            })
            .collect();
        let changed = match merge {
            Some(revision) => {
                self.history
                    .paint_many_merged(&mut self.doc.canvas, changes, revision)
            }
            None => self.history.paint_many(&mut self.doc.canvas, changes),
        };
        self.dirty |= changed;
        self.last_gradient = Some(LastGradient {
            pick,
            start,
            selection,
            revision: self.history.revision(),
        });
        Ok(changed)
    }

    fn gradient_direction_controls(&mut self, ui: &mut egui::Ui) {
        let (mut direction, mut reversed) = (self.gradient_direction, self.gradient_reversed);
        ui.horizontal(|ui| {
            ui.small("Gradient");
            for (option, icons, label) in [
                (
                    GradientDirection::Drag,
                    [ICON_SOUTH_EAST, ICON_NORTH_WEST],
                    "Gradient follows the selection drag",
                ),
                (
                    GradientDirection::LeftToRight,
                    [ICON_EAST, ICON_WEST],
                    "Gradient runs left to right",
                ),
                (
                    GradientDirection::TopToBottom,
                    [ICON_SOUTH, ICON_NORTH],
                    "Gradient runs top to bottom",
                ),
            ] {
                let icon = icons[usize::from(reversed)];
                if icon_button(ui, icon, label, direction == option).clicked() {
                    direction = option;
                }
            }
            if icon_button(ui, ICON_SYNC_ALT, "Reverse gradient direction", reversed).clicked() {
                reversed = !reversed;
            }
        });
        if (direction, reversed) != (self.gradient_direction, self.gradient_reversed) {
            if let Err(error) = self.set_gradient_direction(direction, reversed) {
                self.message = Some(error);
            }
        }
    }

    fn copy_selection(&mut self, cut: bool) -> Result<String, String> {
        let selection = self.selection.ok_or("Select an area first")?;
        let (left, top, right, bottom) = selection.bounds();
        let cells = self.selection_cells(selection)?;
        let width = right - left + 1;
        let mut text = String::new();
        for (index, cell) in cells.iter().enumerate() {
            if index > 0 && index % width == 0 {
                text.push('\n');
            }
            text.push(cell.ch);
        }
        self.clipboard = Some(CellClipboard {
            width,
            height: bottom - top + 1,
            cells,
            text: text.clone(),
        });
        if cut {
            let clear =
                (top..=bottom).flat_map(|y| (left..=right).map(move |x| (x, y, Cell::default())));
            self.dirty |= self.history.paint_many(&mut self.doc.canvas, clear);
        }
        Ok(text)
    }

    fn paste(&mut self, text: &str) -> Result<(), String> {
        let text = text.replace("\r\n", "\n");
        let mut changes = Vec::new();
        let (width, height) =
            if let Some(clipboard) = self.clipboard.as_ref().filter(|clip| clip.text == text) {
                for (index, &cell) in clipboard.cells.iter().enumerate() {
                    changes.push((index % clipboard.width, index / clipboard.width, cell));
                }
                (clipboard.width, clipboard.height)
            } else {
                let rows: Vec<_> = text.split('\n').collect();
                let mut width = 0;
                for (y, row) in rows.iter().enumerate() {
                    let mut count = 0;
                    for (x, ch) in row.chars().enumerate() {
                        if ch.is_control() {
                            return Err("Clipboard contains unsupported control characters".into());
                        }
                        changes.push((x, y, Cell { ch, ..self.brush }));
                        count += 1;
                    }
                    width = width.max(count);
                }
                (width, rows.len())
            };
        if width == 0 {
            return Err("Clipboard contains no characters".into());
        }
        let (x, y) = self.cursor;
        if width > self.doc.canvas.width().saturating_sub(x)
            || height > self.doc.canvas.height().saturating_sub(y)
        {
            return Err(
                "Clipboard does not fit at the cursor; move it or resize the canvas".into(),
            );
        }
        self.dirty |= self.history.paint_many(
            &mut self.doc.canvas,
            changes
                .into_iter()
                .map(|(dx, dy, cell)| (x + dx, y + dy, cell)),
        );
        self.selection = Some(Selection {
            start: (x, y),
            end: (x + width - 1, y + height - 1),
        });
        Ok(())
    }

    fn canvas_input(&mut self, events: &[egui::Event]) -> bool {
        let width = self.doc.canvas.width();
        let height = self.doc.canvas.height();
        let mut moved = false;
        for event in events {
            match event {
                egui::Event::Text(text) | egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                    if !text.is_empty() {
                        self.selection = None;
                    }
                    for ch in text.chars().filter(|ch| !ch.is_control()) {
                        if self.cursor.0 == width {
                            break;
                        }
                        let cell = Cell { ch, ..self.brush };
                        self.dirty |= self.history.paint(
                            &mut self.doc.canvas,
                            self.cursor.0,
                            self.cursor.1,
                            cell,
                        );
                        self.cursor.0 += 1;
                        if self.cursor.0 == width && self.cursor.1 + 1 < height {
                            self.cursor = (0, self.cursor.1 + 1);
                        }
                        moved = true;
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if !modifiers.ctrl && !modifiers.alt && !modifiers.mac_cmd => {
                    match key {
                        egui::Key::ArrowLeft => {
                            if self.cursor.0 > 0 {
                                self.cursor.0 -= 1;
                            } else if self.cursor.1 > 0 {
                                self.cursor = (width - 1, self.cursor.1 - 1);
                            }
                        }
                        egui::Key::ArrowRight => {
                            if self.cursor.0 + 1 < width {
                                self.cursor.0 += 1;
                            } else if self.cursor.1 + 1 < height {
                                self.cursor = (0, self.cursor.1 + 1);
                            }
                        }
                        egui::Key::ArrowUp => {
                            self.cursor.0 = self.cursor.0.min(width - 1);
                            self.cursor.1 = self.cursor.1.saturating_sub(1);
                        }
                        egui::Key::ArrowDown => {
                            self.cursor.0 = self.cursor.0.min(width - 1);
                            self.cursor.1 = (self.cursor.1 + 1).min(height - 1);
                        }
                        egui::Key::Home => self.cursor.0 = 0,
                        egui::Key::End => self.cursor.0 = width - 1,
                        egui::Key::Enter => {
                            if self.cursor.1 + 1 < height {
                                self.cursor = (0, self.cursor.1 + 1);
                            }
                        }
                        egui::Key::Backspace => {
                            if self.cursor.0 > 0 {
                                self.cursor.0 -= 1;
                            } else if self.cursor.1 > 0 {
                                self.cursor = (width - 1, self.cursor.1 - 1);
                            } else {
                                continue;
                            }
                            self.dirty |= self.history.paint(
                                &mut self.doc.canvas,
                                self.cursor.0,
                                self.cursor.1,
                                Cell {
                                    ch: ' ',
                                    ..self.brush
                                },
                            );
                        }
                        egui::Key::Delete => {
                            if self.cursor.0 < width {
                                self.dirty |= self.history.paint(
                                    &mut self.doc.canvas,
                                    self.cursor.0,
                                    self.cursor.1,
                                    Cell {
                                        ch: ' ',
                                        ..self.brush
                                    },
                                );
                            }
                        }
                        _ => continue,
                    }
                    self.selection = None;
                    moved = true;
                }
                _ => {}
            }
        }
        moved
    }
}

fn material_fonts(fonts: &mut egui::FontDefinitions) {
    let font = egui_material_icons::font_insert();
    let name = font.name;
    fonts.font_data.insert(name.clone(), font.data.into());
    fonts
        .families
        .insert(egui::FontFamily::Name(name.clone().into()), vec![name]);
}

fn icon_button(
    ui: &mut egui::Ui,
    icon: MaterialIcon,
    label: &str,
    selected: bool,
) -> egui::Response {
    let button = egui::Button::new(icon.rich_text().size(20.0)).selected(selected);
    let response = ui
        .scope(|ui| {
            ui.spacing_mut().button_padding.x = 4.0;
            ui.add_sized([28.0, 28.0], button)
        })
        .inner;
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, label)
    });
    response.on_hover_text(label)
}

fn cell_colors(cell: Cell, ice: bool) -> (Color, Color) {
    let mut fg = cell.fg;
    let mut bg = cell.bg;
    if cell.attrs.bold {
        if let Color::Ansi(n @ 0..=7) = fg {
            fg = Color::Ansi(n + 8);
        }
    }
    if ice && cell.attrs.blink {
        if let Color::Ansi(n @ 0..=7) = bg {
            bg = Color::Ansi(n + 8);
        }
    }
    if cell.attrs.inverse {
        std::mem::swap(&mut fg, &mut bg);
    }
    (fg, bg)
}

fn color_key(color: Color) -> [u8; 3] {
    let rgb = display_color(color);
    [rgb.r(), rgb.g(), rgb.b()]
}

impl Editor {
    /// Adds a color the user picked to the session's Used colors, once.
    fn remember_color(&mut self, color: Color) {
        if !self
            .picked_colors
            .iter()
            .any(|&picked| color_key(picked) == color_key(color))
        {
            self.picked_colors.push(color);
        }
    }

    /// Remembers brush colors chosen since the last frame. Colors from an RGB
    /// picker are remembered once it closes, not at every step of a drag.
    fn remember_picked_colors(&mut self) {
        if self.rgb_open {
            return;
        }
        if let Some(end) = self.pending_gradient_end.take() {
            self.remember_color(end);
        }
        let (fg, bg) = (self.brush.fg, self.brush.bg);
        if fg != self.remembered_brush.0 {
            self.remember_color(fg);
        }
        if bg != self.remembered_brush.1 {
            self.remember_color(bg);
        }
        self.remembered_brush = (fg, bg);
    }

    fn ice_colors(&self) -> bool {
        self.doc.sauce.as_ref().is_some_and(|s| s.flags() & 1 != 0)
    }

    /// Picks up the displayed foreground or background color of a cell.
    fn pick_color(&mut self, x: usize, y: usize, background: bool) {
        let Some(&cell) = self.doc.canvas.get(x, y) else {
            return;
        };
        let (fg, bg) = cell_colors(cell, self.ice_colors());
        let color = if background { bg } else { fg };
        if background {
            self.brush.bg = color;
        } else {
            self.brush.fg = color;
        }
        self.remember_color(color);
    }
}

fn paint_cell(painter: &egui::Painter, cell: Cell, rect: Rect, ice: bool, zoom: f32) {
    let (fg, bg) = cell_colors(cell, ice);
    painter.rect_filled(rect, 0.0, display_color(bg));
    if !glyph::paint(painter, cell.ch, rect, display_color(fg)) {
        painter.with_clip_rect(rect).text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            cell.ch,
            FontId::monospace(16.0 * zoom),
            display_color(fg),
        );
    }
    if cell.attrs.underline {
        let underline = Rect::from_min_max(
            Pos2::new(rect.left(), rect.bottom() - 2.0 * zoom),
            Pos2::new(rect.right(), rect.bottom() - zoom),
        );
        painter.rect_filled(underline, 0.0, display_color(fg));
    }
}

/// A modifier-click request to fade the selection's foreground or background
/// from the current brush color to `end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GradientPick {
    background: bool,
    end: Color,
}

/// What the RGB picker edits: `(background, gradient end)`.
type RgbTarget = (bool, bool);

#[derive(Default)]
struct ColorEvents {
    gradient: Option<GradientPick>,
    /// The open RGB picker's target, or `None` when it is closed.
    rgb: Option<RgbTarget>,
    /// A color clicked in a palette for the brush, even if it was already
    /// the current color: `(background, color)`.
    picked: Option<(bool, Color)>,
}

fn gradient_modifier(ctx: &egui::Context) -> &'static str {
    if ctx.os() == egui::os::OperatingSystem::Mac {
        "Cmd"
    } else {
        "Ctrl"
    }
}

fn palette_button(
    ui: &mut egui::Ui,
    label: &str,
    color: Color,
    foreground: &mut Color,
    background: &mut Color,
    gradient: &mut Option<GradientPick>,
    picked: &mut Option<(bool, Color)>,
) -> egui::Response {
    let selected = display_color(*foreground) == display_color(color)
        || display_color(*background) == display_color(color);
    let button = egui::Button::new("")
        .fill(display_color(color))
        .stroke(ui.visuals().widgets.inactive.bg_stroke);
    let response = ui.add_sized([24.0, 20.0], button);
    if selected {
        ui.painter().rect_stroke(
            response.rect,
            ui.visuals().widgets.inactive.corner_radius,
            egui::Stroke::new(2.0, Color32::WHITE),
            egui::StrokeKind::Inside,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, label)
    });
    let modified = ui.input(|input| input.modifiers.command);
    for (clicked, is_background) in [
        (response.clicked(), false),
        (response.secondary_clicked(), true),
    ] {
        if !clicked {
            continue;
        }
        if modified {
            *gradient = Some(GradientPick {
                background: is_background,
                end: color,
            });
        } else {
            *picked = Some((is_background, color));
            if is_background {
                *background = color;
            } else {
                *foreground = color;
            }
        }
    }
    let modifier = gradient_modifier(ui.ctx());
    response.on_hover_text(format!(
        "{label}. Left-click: foreground; right-click: background. \
         {modifier}+click: fade the selection from the current color to this one."
    ))
}

fn used_palette(
    ui: &mut egui::Ui,
    colors: &[Color],
    foreground: &mut Color,
    background: &mut Color,
    gradient: &mut Option<GradientPick>,
    picked: &mut Option<(bool, Color)>,
) {
    ui.push_id("artwork_palette", |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.small("Used colors").on_hover_text(
            "Colors you have picked this session. Left-click for foreground; right-click for background.",
        );
        if colors.is_empty() {
            ui.weak("Colors you pick appear here.");
        }
        egui::ScrollArea::vertical()
            .id_salt("used_colors")
            .max_height(68.0)
            // Keep the last pixel-snapped row inside the scroll viewport.
            .content_margin(egui::Margin {
                bottom: 1,
                ..Default::default()
            })
            .show_rows(ui, 20.0, colors.len().div_ceil(8), |ui, rows| {
                egui::Grid::new("used_palette")
                    .spacing(Vec2::new(4.0, 2.0))
                    .min_col_width(24.0)
                    .show(ui, |ui| {
                        for row in rows {
                            for &used in &colors[row * 8..colors.len().min((row + 1) * 8)] {
                                let rgb = display_color(used);
                                ui.push_id([rgb.r(), rgb.g(), rgb.b()], |ui| {
                                    palette_button(
                                        ui,
                                        &format!("Used color {used:?}"),
                                        used,
                                        foreground,
                                        background,
                                        gradient,
                                        picked,
                                    )
                                });
                            }
                            ui.end_row();
                        }
                    });
            });
    });
}

fn current_color_swatch(ui: &mut egui::Ui, label: &str, color: Color) -> egui::Response {
    let response = ui.add_sized(
        [40.0, 28.0],
        egui::Button::new("").fill(display_color(color)),
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::Role::ColorWell,
            ui.is_enabled(),
            false,
            format!("{label} current color"),
        )
    });
    response.on_hover_text(format!(
        "{label}: {color:?}. Left-click to edit foreground RGB; right-click to edit background RGB."
    ))
}

fn color_controls(
    ui: &mut egui::Ui,
    foreground: &mut Color,
    background: &mut Color,
    used_colors: &[Color],
) -> ColorEvents {
    let mut gradient = None;
    let mut picked = None;
    let modifier = gradient_modifier(ui.ctx());
    let hint = format!(
        "Left-click a color for foreground, right-click for background. \
         With a selection, {modifier}+left/right-click fades its foreground/background \
         from the current color to the clicked one."
    );
    ui.small("Left: FG / Right: BG").on_hover_text(&hint);
    ui.small(format!("{modifier}+click: selection gradient"))
        .on_hover_text(hint);
    let rgb = ui
        .push_id("colors", |ui| {
            egui::Grid::new("palette")
                .spacing(Vec2::splat(4.0))
                .min_col_width(24.0)
                .show(ui, |ui| {
                    for n in 0..16 {
                        palette_button(
                            ui,
                            &format!("ANSI color {n}"),
                            Color::Ansi(n),
                            foreground,
                            background,
                            &mut gradient,
                            &mut picked,
                        );
                        if n % 8 == 7 {
                            ui.end_row();
                        }
                    }
                });
            used_palette(
                ui,
                used_colors,
                foreground,
                background,
                &mut gradient,
                &mut picked,
            );
            ui.horizontal(|ui| {
                let indexed_id = ui.make_persistent_id("indexed_palette");
                let rgb_id = ui.make_persistent_id("rgb_palette");
                let rgb_target_id = ui.make_persistent_id("rgb_target");
                let rgb_end_id = ui.make_persistent_id("rgb_gradient_end");
                let indexed = icon_button(
                    ui,
                    ICON_GRID_VIEW,
                    "256-color palette",
                    matches!(foreground, Color::Indexed(_))
                        || matches!(background, Color::Indexed(_)),
                );
                if indexed.clicked() || indexed.secondary_clicked() {
                    egui::Popup::toggle_id(ui.ctx(), indexed_id);
                }
                egui::Popup::from_response(&indexed)
                    .id(indexed_id)
                    .open_memory(None)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        ui.label("256-color palette");
                        ui.small("Left: foreground / Right: background");
                        egui::Grid::new("indexed_colors")
                            .spacing(Vec2::splat(2.0))
                            .min_col_width(24.0)
                            .show(ui, |ui| {
                                for n in 0..=255 {
                                    let response = palette_button(
                                        ui,
                                        &format!("Indexed color {n}"),
                                        Color::Indexed(n),
                                        foreground,
                                        background,
                                        &mut gradient,
                                        &mut picked,
                                    );
                                    if response.clicked() || response.secondary_clicked() {
                                        egui::Popup::close_id(ui.ctx(), indexed_id);
                                    }
                                    if n % 16 == 15 {
                                        ui.end_row();
                                    }
                                }
                            });
                    });
                let rgb = icon_button(
                    ui,
                    ICON_PALETTE,
                    &format!(
                        "RGB color: left-click foreground, right-click background; \
                     {modifier}+click picks a gradient end color"
                    ),
                    matches!(foreground, Color::Rgb(..)) || matches!(background, Color::Rgb(..)),
                );
                ui.small("FG");
                let current_fg = current_color_swatch(ui, "Foreground", *foreground);
                ui.small("BG");
                let current_bg = current_color_swatch(ui, "Background", *background);
                let opened_background = if rgb.secondary_clicked()
                    || current_fg.secondary_clicked()
                    || current_bg.secondary_clicked()
                {
                    Some(true)
                } else if rgb.clicked() || current_fg.clicked() || current_bg.clicked() {
                    Some(false)
                } else {
                    None
                };
                if let Some(is_background) = opened_background {
                    let target: RgbTarget = (is_background, ui.input(|i| i.modifiers.command));
                    let start = if is_background {
                        *background
                    } else {
                        *foreground
                    };
                    ui.data_mut(|data| {
                        data.insert_temp(rgb_target_id, target);
                        data.insert_temp(rgb_end_id, start);
                    });
                    egui::Popup::open_id(ui.ctx(), rgb_id);
                }
                let target: RgbTarget =
                    ui.data(|data| data.get_temp(rgb_target_id).unwrap_or_default());
                egui::Popup::from_response(&rgb)
                    .id(rgb_id)
                    .open_memory(None)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        ui.spacing_mut().slider_width = 275.0;
                        let (is_background, is_gradient) = target;
                        let name = if is_background {
                            "Background"
                        } else {
                            "Foreground"
                        };
                        let mut end: Color = ui
                            .data(|data| data.get_temp(rgb_end_id).unwrap_or(Color::Rgb(0, 0, 0)));
                        let color = if is_gradient {
                            ui.label(format!("{name} gradient end RGB color"));
                            &mut end
                        } else {
                            ui.label(format!("{name} RGB color"));
                            if is_background {
                                &mut *background
                            } else {
                                &mut *foreground
                            }
                        };
                        let mut selected = display_color(*color);
                        if egui::color_picker::color_picker_color32(
                            ui,
                            &mut selected,
                            egui::color_picker::Alpha::Opaque,
                        ) {
                            let picked = Color::Rgb(selected.r(), selected.g(), selected.b());
                            if !is_gradient && ui.input(|input| input.modifiers.command) {
                                // A modified pick inside an open picker switches it to
                                // choosing the gradient end, leaving the brush unchanged.
                                ui.data_mut(|data| {
                                    data.insert_temp(rgb_target_id, (is_background, true));
                                    data.insert_temp(rgb_end_id, picked);
                                });
                                gradient = Some(GradientPick {
                                    background: is_background,
                                    end: picked,
                                });
                                return;
                            }
                            *color = picked;
                            if is_gradient {
                                ui.data_mut(|data| data.insert_temp(rgb_end_id, end));
                                gradient = Some(GradientPick {
                                    background: is_background,
                                    end,
                                });
                            }
                        }
                    });
                egui::Popup::is_id_open(ui.ctx(), rgb_id)
                    .then(|| ui.data(|data| data.get_temp(rgb_target_id).unwrap_or(target)))
            })
            .inner
        })
        .inner;
    ColorEvents {
        gradient,
        rgb,
        picked,
    }
}

impl eframe::App for Editor {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        for path in macos_open::take() {
            self.request(PendingAction::OpenPath(path), ui.ctx());
        }
        self.ui(ui);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(
            SIDEBAR_ORDER_KEY,
            self.sidebar_order
                .iter()
                .map(|section| section.id())
                .collect::<Vec<_>>()
                .join("\n"),
        );
        storage.set_string(
            SIDEBAR_COLLAPSED_KEY,
            self.sidebar_collapsed
                .iter()
                .map(|section| section.id())
                .collect::<Vec<_>>()
                .join("\n"),
        );
        storage.set_string(MINIMAP_VISIBLE_KEY, self.minimap_visible.to_string());
    }

    fn persist_egui_memory(&self) -> bool {
        false
    }
}

impl Editor {
    fn sidebar_section(
        &mut self,
        ui: &mut egui::Ui,
        section: SidebarSection,
        used_colors: &[Color],
        clipboard_action: &mut Option<bool>,
    ) {
        match section {
            SidebarSection::Brush => {
                ui.horizontal(|ui| {
                    let tool = (self.selecting, self.eyedropper);
                    if icon_button(ui, ICON_BRUSH, "Draw", tool == (false, false)).clicked() {
                        (self.selecting, self.eyedropper) = (false, false);
                    }
                    if icon_button(ui, ICON_SELECT_ALL, "Select a rectangle", tool == (true, false))
                        .clicked()
                    {
                        (self.selecting, self.eyedropper) = (true, false);
                    }
                    if icon_button(
                        ui,
                        ICON_COLORIZE,
                        "Eyedropper: left-click picks up a foreground color, right-click a background color",
                        self.eyedropper,
                    )
                    .clicked()
                    {
                        self.eyedropper = true;
                    }
                    ui.separator();
                    if icon_button(
                        ui,
                        ICON_CONTENT_COPY,
                        &format!("Copy ({})", shortcut_text(ui.ctx(), egui::Key::C)),
                        false,
                    )
                    .clicked()
                    {
                        *clipboard_action = Some(false);
                    }
                    if icon_button(
                        ui,
                        ICON_CONTENT_CUT,
                        &format!("Cut ({})", shortcut_text(ui.ctx(), egui::Key::X)),
                        false,
                    )
                    .clicked()
                    {
                        *clipboard_action = Some(true);
                    }
                    let mut character = self.brush.ch.to_string();
                    if ui
                        .add_sized(
                            [40.0, 28.0],
                            egui::TextEdit::singleline(&mut character)
                                .font(FontId::new(
                                    20.0,
                                    egui::FontFamily::Name(glyph::FONT_NAME.into()),
                                ))
                                .desired_width(40.0),
                        )
                        .changed()
                    {
                        if let Some(ch) = character.chars().last() {
                            self.brush.ch = ch;
                        }
                    }
                    if icon_button(ui, ICON_APPS, "CP437 character picker", self.picker_open)
                        .clicked()
                    {
                        self.picker_open = true;
                    }
                })
                .response
                .on_hover_text(format!(
                    "Paste at the canvas cursor with {}",
                    shortcut_text(ui.ctx(), egui::Key::V)
                ));
            }
            SidebarSection::Zoom => self.zoom_controls(ui),
            SidebarSection::Colors => {
                let before = (self.brush.fg, self.brush.bg);
                let events =
                    color_controls(ui, &mut self.brush.fg, &mut self.brush.bg, used_colors);
                self.rgb_open = events.rgb.is_some();
                if let Some((_, color)) = events.picked {
                    self.remember_color(color);
                }
                // A palette click recolors the selection even when the brush
                // already has that color.
                let picked = |background| events.picked.is_some_and(|(b, _)| b == background);
                let fg = (self.brush.fg != before.0 || picked(false)).then_some(self.brush.fg);
                let bg = (self.brush.bg != before.1 || picked(true)).then_some(self.brush.bg);
                if events.rgb.is_none() {
                    self.recolor_merge = None;
                }
                let merge = self
                    .recolor_merge
                    .filter(|&(target, _)| Some(target) == events.rgb)
                    .map(|(_, revision)| revision);
                let changed = if let Some(pick) = events.gradient {
                    match self.apply_gradient(pick, merge) {
                        Ok(changed) => {
                            self.pending_gradient_end = Some(pick.end);
                            changed
                        }
                        Err(error) => {
                            self.message = Some(error);
                            false
                        }
                    }
                } else if fg.is_some() || bg.is_some() {
                    self.recolor_selection(fg, bg, merge)
                } else {
                    false
                };
                if changed {
                    self.recolor_merge = events.rgb.map(|target| (target, self.history.revision()));
                }
                if self.selection.is_some() {
                    self.gradient_direction_controls(ui);
                }
            }
            SidebarSection::Attributes => {
                ui.horizontal(|ui| {
                    if icon_button(ui, ICON_FORMAT_BOLD, "Bold", self.brush.attrs.bold).clicked() {
                        self.brush.attrs.bold = !self.brush.attrs.bold;
                    }
                    if icon_button(ui, ICON_FLASH_ON, "Blink / iCE", self.brush.attrs.blink)
                        .clicked()
                    {
                        self.brush.attrs.blink = !self.brush.attrs.blink;
                    }
                    if icon_button(
                        ui,
                        ICON_FORMAT_UNDERLINED,
                        "Underline",
                        self.brush.attrs.underline,
                    )
                    .clicked()
                    {
                        self.brush.attrs.underline = !self.brush.attrs.underline;
                    }
                    if icon_button(
                        ui,
                        ICON_SWAP_HORIZ,
                        "Inverse colors",
                        self.brush.attrs.inverse,
                    )
                    .clicked()
                    {
                        self.brush.attrs.inverse = !self.brush.attrs.inverse;
                    }
                });
            }
            SidebarSection::Sauce => {
                if self.doc.sauce.is_none()
                    && icon_button(ui, ICON_ADD, "Add SAUCE metadata", false).clicked()
                {
                    self.doc.sauce = Some(Sauce::default());
                    self.dirty = true;
                }
                if let Some(sauce) = &mut self.doc.sauce {
                    egui::Grid::new("sauce_fields")
                        .min_col_width(44.0)
                        .spacing(Vec2::new(6.0, 6.0))
                        .show(ui, |ui| {
                            for (label, value) in [
                                ("Title", &mut sauce.title),
                                ("Author", &mut sauce.author),
                                ("Group", &mut sauce.group),
                            ] {
                                ui.label(label);
                                self.dirty |= ui
                                    .add_sized([170.0, 20.0], egui::TextEdit::singleline(value))
                                    .changed();
                                ui.end_row();
                            }
                        });
                    ui.label("Comments (one per line)");
                    let mut comments = sauce.comments.join("\n");
                    if ui
                        .add(
                            egui::TextEdit::multiline(&mut comments)
                                .desired_width(ui.available_width()),
                        )
                        .changed()
                    {
                        self.dirty = true;
                        sauce.comments = if comments.is_empty() {
                            vec![]
                        } else {
                            comments.split('\n').map(str::to_owned).collect()
                        };
                    }
                    ui.label(format!(
                        "Dimensions: {} x {}",
                        self.doc.canvas.width(),
                        self.doc.canvas.height(),
                    ));
                }
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        let context = ui.ctx().clone();
        let ctx = &context;
        if !self.about_open && self.selection_drag.is_none() {
            if ctx.input_mut(|input| {
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::Plus)
                    || input.consume_key(egui::Modifiers::COMMAND, egui::Key::Equals)
                    || input.consume_key(
                        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                        egui::Key::Plus,
                    )
                    || input.consume_key(
                        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                        egui::Key::Equals,
                    )
            }) {
                self.change_zoom(1);
            }
            if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Minus))
            {
                self.change_zoom(-1);
            }
            if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Num0)) {
                self.zoom_percent = DEFAULT_ZOOM_PERCENT;
            }
        }
        if ctx.input(|input| input.viewport().close_requested()) && self.dirty {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending = Some(PendingAction::Close);
        }
        if !self.about_open && self.selection_drag.is_none() {
            if ctx.input_mut(|input| {
                input.consume_key(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::Z,
                ) || input.consume_key(egui::Modifiers::COMMAND, egui::Key::Y)
            }) {
                self.redo();
            } else if ctx
                .input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Z))
            {
                self.undo();
            }
        }
        let mut clipboard_action = None;
        let mut focus_canvas = false;
        egui::Panel::top("toolbar").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui
                        .button("New image")
                        .on_hover_text(format!(
                            "Create a {DEFAULT_WIDTH} x {DEFAULT_HEIGHT} image; use Resize canvas for custom dimensions"
                        ))
                        .clicked()
                    {
                        ui.close();
                        self.request(PendingAction::New, ctx);
                    }
                    if ui
                        .button("Open...")
                        .on_hover_text("Use SAUCE dimensions, or 80 columns and the artwork's inferred height")
                        .clicked()
                    {
                        ui.close();
                        self.request(PendingAction::Open, ctx);
                    }
                    ui.separator();
                    if ui.button("Save").clicked() {
                        ui.close();
                        self.save(false);
                    }
                    if ui.button("Save as...").clicked() {
                        ui.close();
                        self.save(true);
                    }
                    if ui
                        .add_enabled(
                            self.selection.is_some(),
                            egui::Button::new("Save selection as..."),
                        )
                        .on_hover_text(
                            "Export the selected rectangle without changing this document",
                        )
                        .clicked()
                    {
                        ui.close();
                        self.save_selection();
                    }
                    for (label, format) in [
                        ("Export PNG...", image_export::Format::Png),
                        ("Export JPEG...", image_export::Format::Jpeg),
                    ] {
                        if ui.button(label)
                            .on_hover_text("Render the selection, or the full artwork when nothing is selected")
                            .clicked()
                        {
                            ui.close();
                            self.export_image(ctx, format);
                        }
                    }
                    ui.separator();
                    ui.menu_button("Encoding", |ui| {
                        for (encoding, label) in
                            [(Encoding::Cp437, "CP437"), (Encoding::Utf8, "UTF-8")]
                        {
                            if ui
                                .selectable_value(&mut self.encoding, encoding, label)
                                .clicked()
                            {
                                ui.close();
                            }
                        }
                    });
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        ui.close();
                        self.request(PendingAction::Close, ctx);
                    }
                });
                ui.menu_button("Edit", |ui| {
                    if ui
                        .add(egui::Button::new("Undo").shortcut_text(shortcut_text(ctx, egui::Key::Z)))
                        .clicked()
                    {
                        ui.close();
                        self.undo();
                    }
                    if ui
                        .add(egui::Button::new("Redo").shortcut_text(ctx.format_shortcut(&REDO_SHORTCUT)))
                        .clicked()
                    {
                        ui.close();
                        self.redo();
                    }
                    ui.separator();
                    for (cut, label, shortcut) in
                        [(true, "Cut", egui::Key::X), (false, "Copy", egui::Key::C)]
                    {
                        if ui
                            .add_enabled(
                                self.selection.is_some(),
                                egui::Button::new(label).shortcut_text(shortcut_text(ctx, shortcut)),
                            )
                            .clicked()
                        {
                            ui.close();
                            clipboard_action = Some(cut);
                        }
                    }
                    if ui
                        .add(egui::Button::new("Paste").shortcut_text(shortcut_text(ctx, egui::Key::V)))
                        .clicked()
                    {
                        ui.close();
                        focus_canvas = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                    }
                    ui.separator();
                    if ui
                        .selectable_label(!self.selecting && !self.eyedropper, "Draw mode")
                        .clicked()
                    {
                        (self.selecting, self.eyedropper) = (false, false);
                        ui.close();
                        focus_canvas = true;
                    }
                    if ui.selectable_label(self.eyedropper, "Eyedropper").clicked() {
                        self.eyedropper = true;
                        ui.close();
                        focus_canvas = true;
                    }
                    if ui.button("CP437 character picker...").clicked() {
                        ui.close();
                        self.picker_open = true;
                    }
                    if ui.button("TheDraw fonts...").clicked() {
                        ui.close();
                        self.tdf_open = true;
                    }
                });
                ui.menu_button("Selection", |ui| {
                    if ui
                        .selectable_label(
                            self.selecting && !self.eyedropper,
                            "Rectangular selection",
                        )
                        .clicked()
                    {
                        (self.selecting, self.eyedropper) = (true, false);
                        ui.close();
                        focus_canvas = true;
                    }
                    ui.separator();
                    if ui.button("Select all").clicked() {
                        ui.close();
                        (self.selecting, self.eyedropper) = (true, false);
                        self.selection = Some(Selection {
                            start: (0, 0),
                            end: (self.doc.canvas.width() - 1, self.doc.canvas.height() - 1),
                        });
                        self.cursor = (0, 0);
                        focus_canvas = true;
                    }
                    if ui
                        .add_enabled(self.selection.is_some(), egui::Button::new("Deselect"))
                        .clicked()
                    {
                        ui.close();
                        self.selection = None;
                        focus_canvas = true;
                    }
                });
                ui.menu_button("View", |ui| {
                    ui.label(format!("Zoom: {:.0}%", self.zoom_percent));
                    if ui
                        .add_enabled(
                            self.selection_drag.is_none() && self.zoom_percent < 400.0,
                            egui::Button::new("Zoom in").shortcut_text(shortcut_text(ctx, egui::Key::Plus)),
                        )
                        .clicked()
                    {
                        self.change_zoom(1);
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            self.selection_drag.is_none() && self.zoom_percent > 50.0,
                            egui::Button::new("Zoom out").shortcut_text(shortcut_text(ctx, egui::Key::Minus)),
                        )
                        .clicked()
                    {
                        self.change_zoom(-1);
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            self.selection_drag.is_none(),
                            egui::Button::new("Actual size (100%)").shortcut_text(shortcut_text(ctx, egui::Key::Num0)),
                        )
                        .clicked()
                    {
                        self.zoom_percent = DEFAULT_ZOOM_PERCENT;
                        ui.close();
                    }
                    ui.separator();
                    if ui.checkbox(&mut self.minimap_visible, "Show minimap").changed() {
                        ui.close();
                    }
                });
                ui.menu_button("Canvas", |ui| {
                    ui.strong("Dimensions").on_hover_text("Set dimensions for Resize canvas");
                    ui.horizontal(|ui| {
                        ui.label("Width");
                        ui.add(egui::DragValue::new(&mut self.width).range(1..=65535));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Height");
                        ui.add(egui::DragValue::new(&mut self.height).range(1..=65535));
                    });
                    ui.separator();
                    if ui.button("Resize canvas").clicked() {
                        ui.close();
                        match self
                            .history
                            .resize(&mut self.doc.canvas, self.width, self.height)
                        {
                            Ok(changed) => {
                                self.dirty |= changed;
                                if changed {
                                    self.clamp_cursor();
                                }
                            }
                            Err(error) => self.message = Some(error.to_string()),
                        }
                    }
                });
                if ui.button("About").clicked() {
                    self.about_open = true;
                }
            });
        });
        self.rgb_open = false;
        let used_colors = self.picked_colors.clone();
        egui::Panel::left("tools")
            .resizable(true)
            .default_size(260.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(4.0, 6.0);
                    let (press, release) = ctx.input(|input| {
                        let canceled = input.events.iter().any(|event| {
                            matches!(
                                event,
                                egui::Event::Key {
                                    key: egui::Key::Escape,
                                    pressed: true,
                                    ..
                                }
                            )
                        });
                        if !canceled {
                            let position = |pressed| {
                                input.events.iter().find_map(|event| match event {
                                    egui::Event::PointerButton {
                                        pos,
                                        button: egui::PointerButton::Primary,
                                        pressed: event_pressed,
                                        ..
                                    } if *event_pressed == pressed => Some(*pos),
                                    _ => None,
                                })
                            };
                            (position(true), position(false))
                        } else {
                            (None, None)
                        }
                    });
                    let mut targets = Vec::new();
                    for (index, section) in self.sidebar_order.clone().into_iter().enumerate() {
                        if index > 0 {
                            ui.separator();
                        }
                        let response = ui
                            .push_id(section.id(), |ui| {
                                ui.set_min_width(ui.available_width());
                                egui::Frame::new()
                                    .fill(ui.visuals().widgets.inactive.bg_fill)
                                    .inner_margin(egui::Margin::symmetric(4, 2))
                                    .corner_radius(3)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            let collapsed =
                                                self.sidebar_collapsed.contains(&section);
                                            let toggle = ui.add_sized(
                                                [18.0, 18.0],
                                                egui::Button::new(
                                                    egui::RichText::new(if collapsed {
                                                        ICON_CHEVRON_RIGHT
                                                    } else {
                                                        ICON_EXPAND_MORE
                                                    })
                                                    .font(FontId::new(
                                                        16.0,
                                                        egui::FontFamily::Name(
                                                            "material-icons".into(),
                                                        ),
                                                    )),
                                                )
                                                .frame(false),
                                            );
                                            let label = format!(
                                                "{} {} section",
                                                if collapsed { "Expand" } else { "Collapse" },
                                                section.label()
                                            );
                                            toggle.widget_info(|| {
                                                egui::WidgetInfo::selected(
                                                    egui::Role::Button,
                                                    ui.is_enabled(),
                                                    !collapsed,
                                                    &label,
                                                )
                                            });
                                            if toggle.on_hover_text(label).clicked() {
                                                if collapsed {
                                                    self.sidebar_collapsed
                                                        .retain(|&item| item != section);
                                                } else {
                                                    self.sidebar_collapsed.push(section);
                                                    egui::Popup::close_all(ui.ctx());
                                                }
                                                ctx.request_repaint();
                                            }
                                            ui.strong(section.label());
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    let handle = ui.add_sized(
                                                        [18.0, 18.0],
                                                        egui::Button::new(
                                                            egui::RichText::new(ICON_DRAG_HANDLE)
                                                                .font(FontId::new(
                                                                    16.0,
                                                                    egui::FontFamily::Name(
                                                                        "material-icons".into(),
                                                                    ),
                                                                )),
                                                        )
                                                        .frame(false)
                                                        .sense(Sense::drag()),
                                                    );
                                                    let label =
                                                        format!("Drag {} section", section.label());
                                                    handle.widget_info(|| {
                                                        egui::WidgetInfo::selected(
                                                            egui::Role::Button,
                                                            ui.is_enabled(),
                                                            false,
                                                            &label,
                                                        )
                                                    });
                                                    // Use event positions even when native focus changes clear hover.
                                                    let pressed = press.is_some_and(|pos| {
                                                        handle.rect.contains(pos)
                                                            && ui.clip_rect().contains(pos)
                                                            && ctx.layer_id_at(pos)
                                                                == Some(ui.layer_id())
                                                    });
                                                    if pressed {
                                                        egui::DragAndDrop::set_payload(
                                                            ctx, section,
                                                        );
                                                        handle.request_focus();
                                                    }
                                                    handle
                                                        .on_hover_text(label)
                                                        .on_hover_cursor(egui::CursorIcon::Grab);
                                                },
                                            );
                                        });
                                    });
                                if !self.sidebar_collapsed.contains(&section) {
                                    self.sidebar_section(
                                        ui,
                                        section,
                                        &used_colors,
                                        &mut clipboard_action,
                                    );
                                }
                            })
                            .response;
                        let pointer = ctx.input(|input| input.pointer.hover_pos()).or(release);
                        if pointer.is_some_and(|pos| {
                            response.rect.contains(pos)
                                && ui.clip_rect().contains(pos)
                                && ctx.layer_id_at(pos) == Some(ui.layer_id())
                        }) {
                            if let Some(source) = egui::DragAndDrop::payload::<SidebarSection>(ctx)
                            {
                                if *source != section {
                                    let after =
                                        pointer.is_some_and(|pos| pos.y > response.rect.center().y);
                                    let y = if after {
                                        response.rect.bottom() + 2.0
                                    } else {
                                        response.rect.top() - 2.0
                                    };
                                    ui.painter().hline(
                                        response.rect.x_range(),
                                        y,
                                        egui::Stroke::new(2.0_f32, ui.visuals().selection.bg_fill),
                                    );
                                }
                            }
                        }
                        targets.push((index, response));
                    }
                    let mut drop = None;
                    for (index, response) in targets {
                        if let Some(pos) = release.filter(|pos| {
                            response.rect.contains(*pos)
                                && ui.clip_rect().contains(*pos)
                                && ctx.layer_id_at(*pos) == Some(ui.layer_id())
                        }) {
                            if let Some(source) =
                                egui::DragAndDrop::take_payload::<SidebarSection>(ctx)
                            {
                                let after = pos.y > response.rect.center().y;
                                drop = Some((*source, index + usize::from(after)));
                            }
                        }
                    }
                    if let Some((section, insertion)) = drop {
                        self.move_sidebar_section(section, insertion);
                        ctx.request_repaint();
                    }
                    if egui::DragAndDrop::has_payload_of_type::<SidebarSection>(ctx) {
                        ctx.request_repaint();
                        if let Some(pos) = ctx.input(|input| input.pointer.hover_pos()) {
                            let clip = ui.clip_rect();
                            if clip.contains(pos) {
                                let delta = ctx.input(|input| input.stable_dt.min(0.05)) * 240.0;
                                if pos.y < clip.top() + 20.0 {
                                    ui.scroll_with_delta(Vec2::new(0.0, delta));
                                } else if pos.y > clip.bottom() - 20.0 {
                                    ui.scroll_with_delta(Vec2::new(0.0, -delta));
                                }
                            }
                        }
                    }
                });
            });
        let minimap = if self.minimap_visible {
            egui::Panel::right("minimap")
                .resizable(true)
                .default_size(MINIMAP_DEFAULT_WIDTH)
                .size_range(112.0..=minimap::MAX_PANEL_WIDTH)
                .show(ui, |ui| {
                    ui.strong("Minimap");
                    minimap::Minimap::layout(ui, &self.doc.canvas)
                })
                .inner
        } else {
            self.minimap.hide();
            None
        };
        let navigation = minimap
            .as_ref()
            .and_then(|widget| self.minimap.navigate(widget));
        if let Some(navigation) = &navigation {
            if navigation.reset {
                egui::scroll_area::State::default().store(ctx, navigation.scroll_id);
            }
        }
        let mut canvas_id = None;
        let canvas_scroll = egui::CentralPanel::default()
            .show(ui, |ui| {
                ui.style_mut().spacing.scroll = egui::style::ScrollStyle::solid();
                ui.style_mut().spacing.scroll.fade.strength = 0.0;
                let zoom = self.zoom_percent / DEFAULT_ZOOM_PERCENT;
                let max_height = if self.zoom_percent == DEFAULT_ZOOM_PERCENT {
                    50.0 * glyph::CELL_SIZE.y
                } else {
                    f32::INFINITY
                };
                let mut scroll = egui::ScrollArea::both().max_height(max_height);
                if let Some(navigation) = &navigation {
                    scroll = scroll.scroll_offset(navigation.offset);
                    if navigation.reset {
                        // Keep the existing scrollbar layout while clearing scroll momentum and targets.
                        scroll = scroll.scroll(navigation.scrollable).scroll_bar_visibility(
                            egui::scroll_area::ScrollBarVisibility::AlwaysVisible,
                        );
                    }
                }
                scroll.show_viewport(ui, |ui, viewport| {
                    let size = glyph::CELL_SIZE * zoom;
                    let (response, painter) = ui.allocate_painter(
                        Vec2::new(
                            self.doc.canvas.width() as f32 * size.x,
                            self.doc.canvas.height() as f32 * size.y,
                        ),
                        Sense::click_and_drag(),
                    );
                    canvas_id = Some(response.id);
                    if focus_canvas {
                        response.request_focus();
                    }
                    let point = |p: Pos2| {
                        let relative = p - response.rect.min;
                        (
                            (relative.x.max(0.0) / size.x) as usize,
                            (relative.y.max(0.0) / size.y) as usize,
                        )
                    };
                    let eyedropper = self.eyedropper && !self.tdf_open;
                    if eyedropper {
                        if response.hovered() {
                            ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
                        }
                        if response.secondary_clicked() {
                            if let Some((x, y)) = response.interact_pointer_pos().map(point) {
                                self.pick_color(x, y, true);
                                response.request_focus();
                            }
                        }
                    }
                    let bounded_point = |p: Pos2| {
                        let (x, y) = point(p);
                        (
                            x.min(self.doc.canvas.width() - 1),
                            y.min(self.doc.canvas.height() - 1),
                        )
                    };
                    if !self.tdf_open
                        && !eyedropper
                        && response.drag_started_by(egui::PointerButton::Primary)
                    {
                        if let Some(anchor) = ctx
                            .input(|input| input.pointer.press_origin())
                            .map(bounded_point)
                        {
                            if self
                                .selection
                                .is_some_and(|selection| selection.contains(anchor.0, anchor.1))
                            {
                                if let Err(error) = self.begin_selection_drag(anchor) {
                                    self.message = Some(error);
                                }
                            }
                        }
                    }
                    let moving_selection = self.selection_drag.is_some();
                    if moving_selection {
                        if let Some(pointer) = response
                            .interact_pointer_pos()
                            .or_else(|| ctx.input(|input| input.pointer.latest_pos()))
                        {
                            let (x, y) = point(pointer);
                            self.update_selection_drag((
                                x.min(self.doc.canvas.width() - 1),
                                y.min(self.doc.canvas.height() - 1),
                            ));
                        }
                        response.request_focus();
                        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                        if response.drag_stopped_by(egui::PointerButton::Primary) {
                            self.finish_selection_drag();
                        }
                    } else if response.clicked() || response.dragged() {
                        if let Some(pointer) = response.interact_pointer_pos() {
                            let (x, y) = point(pointer);
                            if self.tdf_open {
                                self.cursor = (
                                    x.min(self.doc.canvas.width() - 1),
                                    y.min(self.doc.canvas.height() - 1),
                                );
                                response.request_focus();
                            } else if eyedropper {
                                self.pick_color(x, y, false);
                                response.request_focus();
                            } else if response.clicked()
                                && self
                                    .selection
                                    .is_some_and(|selection| selection.contains(x, y))
                            {
                                let (left, top, _, _) = self.selection.unwrap().bounds();
                                self.cursor = (left, top);
                                response.request_focus();
                            } else if self.selecting {
                                let end = (
                                    x.min(self.doc.canvas.width() - 1),
                                    y.min(self.doc.canvas.height() - 1),
                                );
                                if response.clicked() || response.drag_started() {
                                    let start = if response.drag_started() {
                                        ctx.input(|input| input.pointer.press_origin())
                                            .map(point)
                                            .unwrap_or(end)
                                    } else {
                                        end
                                    };
                                    let start = (
                                        start.0.min(self.doc.canvas.width() - 1),
                                        start.1.min(self.doc.canvas.height() - 1),
                                    );
                                    self.selection = Some(Selection { start, end });
                                    response.request_focus();
                                } else if let Some(selection) = &mut self.selection {
                                    selection.end = end;
                                }
                                if let Some(selection) = self.selection {
                                    let (left, top, _, _) = selection.bounds();
                                    self.cursor = (left, top);
                                }
                            } else if self.doc.canvas.get(x, y).is_some() {
                                self.cursor = (x, y);
                                self.selection = None;
                                response.request_focus();
                                self.dirty |=
                                    self.history.paint(&mut self.doc.canvas, x, y, self.brush);
                            }
                        }
                        if !self.tdf_open && !eyedropper && response.hovered() && !moving_selection
                        {
                            if let Some((x, y)) = response.hover_pos().map(point) {
                                if self
                                    .selection
                                    .is_some_and(|selection| selection.contains(x, y))
                                {
                                    ctx.set_cursor_icon(egui::CursorIcon::Grab);
                                }
                            }
                        }
                    }
                    if let Some(cut) = clipboard_action {
                        match self.copy_selection(cut) {
                            Ok(text) => {
                                ctx.copy_text(text);
                                response.request_focus();
                            }
                            Err(error) => self.message = Some(error),
                        }
                    }
                    // Arrow keys move the canvas cursor. Cancel egui's arrow-key
                    // focus navigation so focus doesn't jump to sidebar widgets.
                    if response.has_focus()
                        && ctx.input(|input| {
                            input.events.iter().any(|event| {
                                matches!(
                                    event,
                                    egui::Event::Key {
                                        key: egui::Key::ArrowLeft
                                            | egui::Key::ArrowRight
                                            | egui::Key::ArrowUp
                                            | egui::Key::ArrowDown,
                                        pressed: true,
                                        modifiers,
                                        ..
                                    } if !modifiers.any()
                                )
                            })
                        })
                    {
                        ctx.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
                    }
                    let focused = response.has_focus()
                        && !self.about_open
                        && !self.picker_open
                        && !self.tdf_open
                        && !moving_selection
                        && self.pending.is_none()
                        && self.message.is_none();
                    if focused {
                        let events = ctx.input(|input| input.events.clone());
                        let mut moved = false;
                        for event in &events {
                            match event {
                                egui::Event::Copy | egui::Event::Cut => {
                                    match self.copy_selection(matches!(event, egui::Event::Cut)) {
                                        Ok(text) => ctx.copy_text(text),
                                        Err(error) => self.message = Some(error),
                                    }
                                }
                                egui::Event::Paste(text) => {
                                    if let Err(error) = self.paste(text) {
                                        self.message = Some(error);
                                    } else {
                                        moved = true;
                                    }
                                }
                                _ => moved |= self.canvas_input(std::slice::from_ref(event)),
                            }
                        }
                        if moved {
                            let pos = response.rect.min
                                + Vec2::new(
                                    self.cursor.0.min(self.doc.canvas.width() - 1) as f32 * size.x,
                                    self.cursor.1 as f32 * size.y,
                                );
                            ui.scroll_to_rect(Rect::from_min_size(pos, size), None);
                        }
                    }
                    let canvas = &self.doc.canvas;
                    let x0 = (viewport.min.x / size.x).max(0.0) as usize;
                    let y0 = (viewport.min.y / size.y).max(0.0) as usize;
                    let x1 = ((viewport.max.x / size.x).ceil() as usize + 1).min(canvas.width());
                    let y1 = ((viewport.max.y / size.y).ceil() as usize + 1).min(canvas.height());
                    let ice = self.doc.sauce.as_ref().is_some_and(|s| s.flags() & 1 != 0);
                    for y in y0..y1 {
                        for x in x0..x1 {
                            let cell = self
                                .selection_drag
                                .as_ref()
                                .and_then(|drag| drag.preview_cell(x, y))
                                .unwrap_or(*canvas.get(x, y).unwrap());
                            let rect = glyph::cell_rect(
                                response.rect.min,
                                x,
                                y,
                                size,
                                ctx.pixels_per_point(),
                            );
                            paint_cell(&painter, cell, rect, ice, zoom);
                        }
                    }
                    if focused {
                        let x = self.cursor.0.min(canvas.width() - 1);
                        let pos = response.rect.min
                            + Vec2::new(x as f32 * size.x, self.cursor.1 as f32 * size.y);
                        painter.rect_stroke(
                            Rect::from_min_size(pos, size),
                            0.0,
                            (1.0, Color32::WHITE),
                            egui::StrokeKind::Inside,
                        );
                    }
                    if let Some(selection) = self
                        .selection_drag
                        .as_ref()
                        .map(|drag| drag.destination)
                        .or(self.selection)
                    {
                        let (left, top, right, bottom) = selection.bounds();
                        let pos = response.rect.min
                            + Vec2::new(left as f32 * size.x, top as f32 * size.y);
                        let rect = Rect::from_min_size(
                            pos,
                            Vec2::new(
                                (right - left + 1) as f32 * size.x,
                                (bottom - top + 1) as f32 * size.y,
                            ),
                        );
                        painter.rect_stroke(
                            rect,
                            0.0,
                            (2.0, Color32::LIGHT_BLUE),
                            egui::StrokeKind::Inside,
                        );
                    }
                })
            })
            .inner;
        if let Some(widget) = minimap {
            let zoom = self.zoom_percent / DEFAULT_ZOOM_PERCENT;
            let canvas_size = Vec2::new(
                self.doc.canvas.width() as f32 * glyph::CELL_SIZE.x * zoom,
                self.doc.canvas.height() as f32 * glyph::CELL_SIZE.y * zoom,
            );
            self.minimap.paint(
                widget,
                &self.doc,
                self.history.revision(),
                self.selection_drag.as_ref(),
                canvas_size,
                &canvas_scroll,
            );
        }
        if self.tdf_open {
            self.tdf_window(ctx);
        }
        if self.picker_open {
            let mut selected = None;
            egui::Window::new("CP437 characters")
                .open(&mut self.picker_open)
                .resizable(true)
                .show(ctx, |ui| {
                    ui.label("Choose a character for the brush.");
                    ui.horizontal(|ui| {
                        ui.hyperlink_to(
                            "IBM VGA 9x16 - VileR (c) 2016-2020",
                            "https://int10h.org/oldschool-pc-fonts/fontlist/font?ibm_vga_9x16",
                        );
                        ui.hyperlink_to(
                            "CC BY-SA 4.0",
                            "https://creativecommons.org/licenses/by-sa/4.0/",
                        );
                    });
                    egui::ScrollArea::vertical()
                        .max_height(400.0)
                        .show(ui, |ui| {
                            egui::Grid::new("cp437_grid")
                                .min_col_width(32.0)
                                .spacing(Vec2::splat(2.0))
                                .show(ui, |ui| {
                                    for byte in 1u8..=255 {
                                        let ch = decode_cp437(byte);
                                        let response = ui.add_sized(
                                            [32.0, 32.0],
                                            egui::Button::new("").selected(self.brush.ch == ch),
                                        );
                                        response.widget_info(|| {
                                            egui::WidgetInfo::selected(
                                                egui::Role::Button,
                                                ui.is_enabled(),
                                                self.brush.ch == ch,
                                                ch.to_string(),
                                            )
                                        });
                                        glyph::paint(
                                            ui.painter(),
                                            ch,
                                            Rect::from_center_size(
                                                response.rect.center(),
                                                glyph::CELL_SIZE * 1.5,
                                            ),
                                            ui.visuals().text_color(),
                                        );
                                        if response.on_hover_text(format!("0x{byte:02X}")).clicked()
                                        {
                                            selected = Some(ch);
                                        }
                                        if byte % 16 == 0 {
                                            ui.end_row();
                                        }
                                    }
                                });
                        });
                });
            if let Some(ch) = selected {
                self.brush.ch = ch;
                self.picker_open = false;
                if let Some(id) = canvas_id {
                    ctx.memory_mut(|memory| memory.request_focus(id));
                }
            }
        }
        self.update_check.show(ctx);
        if let Some(message) = self.message.clone() {
            egui::Window::new("Error")
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label(message);
                    if ui.button("OK").clicked() {
                        self.message = None;
                    }
                });
        }
        if let Some(action) = self.pending.clone() {
            egui::Window::new("Unsaved changes")
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label("Discard unsaved changes?");
                    ui.horizontal(|ui| {
                        if ui.button("Discard").clicked() {
                            self.pending = None;
                            if matches!(action, PendingAction::Close) {
                                self.dirty = false;
                            }
                            self.execute(action, ctx);
                        }
                        if ui.button("Cancel").clicked() {
                            self.pending = None;
                        }
                    });
                });
        }
        self.about_window(ctx);
        self.remember_picked_colors();
        // Canvas edits follow the sidebar, so refresh its used swatches on the next frame.
        if ctx.input(|input| !input.events.is_empty()) {
            ctx.request_repaint();
        }
    }
}

impl Editor {
    fn about_window(&mut self, ctx: &egui::Context) {
        if !self.about_open {
            return;
        }
        if self.about_logo.is_none() {
            match eframe::icon_data::from_png_bytes(APP_ICON_PNG) {
                Ok(icon) => {
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [icon.width as usize, icon.height as usize],
                        &icon.rgba,
                    );
                    self.about_logo = Some(ctx.load_texture(
                        "adraw-about-logo",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
                Err(error) => {
                    self.message = Some(format!("Could not display the application logo: {error}"));
                    self.about_open = false;
                    return;
                }
            }
        }
        let mut open = self.about_open;
        egui::Window::new("About ADraw")
            .open(&mut open)
            .default_width(320.0)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    if let Some(logo) = &self.about_logo {
                        ui.add(egui::Image::new(logo).fit_to_exact_size(Vec2::splat(96.0)));
                    }
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                    ui.hyperlink_to("GitHub repository", GITHUB_PAGE);
                });
                ui.separator();
                self.update_check.controls(ui);
            });
        self.about_open = open;
    }

    fn tdf_window(&mut self, ctx: &egui::Context) {
        let mut open = self.tdf_open;
        egui::Window::new("TheDraw font text")
            .open(&mut open)
            .default_width(800.0)
            .default_pos(Pos2::new(280.0, 70.0))
            .show(ctx, |ui| {
                if ui.button("Import TDF files...").clicked() {
                    self.import_tdf();
                }
                if let Some(directory) = self.tdf_library.directory() {
                    ui.small("Imported fonts are saved automatically and available after restart.")
                        .on_hover_text(directory.display().to_string());
                }
                if self.tdf_library.fonts.is_empty() {
                    ui.label("Import .tdf files to use block, color, or outline fonts.");
                    return;
                }
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        Vec2::new(230.0, 340.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.label("Imported fonts");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.tdf_filter)
                                    .hint_text("Search fonts")
                                    .desired_width(230.0),
                            );
                            let previous = self.tdf_font;
                            let filter = self.tdf_filter.trim().to_lowercase();
                            let mut matches: Vec<_> = self
                                .tdf_library
                                .fonts
                                .iter()
                                .enumerate()
                                .filter(|(_, entry)| {
                                    entry.font.name.to_lowercase().contains(&filter)
                                })
                                .map(|(index, _)| index)
                                .collect();
                            matches.sort_by_key(|&index| {
                                self.tdf_library.fonts[index].font.name.to_lowercase()
                            });
                            ui.small(format!(
                                "{} matching / {} imported",
                                matches.len(),
                                self.tdf_library.fonts.len()
                            ));
                            if matches.is_empty() {
                                ui.label("No matching fonts.");
                            }
                            ui.scope(|ui| {
                                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                                egui::ScrollArea::vertical()
                                    .id_salt("imported_font_list")
                                    .max_height(280.0)
                                    .show_rows(
                                        ui,
                                        ui.spacing().interact_size.y,
                                        matches.len(),
                                        |ui, rows| {
                                            for row in rows {
                                                let index = matches[row];
                                                let entry = &self.tdf_library.fonts[index];
                                                ui.selectable_value(
                                                    &mut self.tdf_font,
                                                    index,
                                                    format!(
                                                        "{} ({:?})",
                                                        entry.font.name, entry.font.kind
                                                    ),
                                                )
                                                .on_hover_text(entry.source.display().to_string());
                                            }
                                        },
                                    );
                            });
                            if previous != self.tdf_font {
                                self.tdf_spacing =
                                    self.tdf_library.fonts[self.tdf_font].font.spacing;
                            }
                        },
                    );
                    ui.separator();
                    ui.vertical(|ui| {
                        let font = &self.tdf_library.fonts[self.tdf_font].font;
                        ui.heading(&font.name);
                        ui.horizontal(|ui| {
                            ui.label(format!("{:?} font", font.kind));
                            ui.label("Letter spacing");
                            ui.add(egui::DragValue::new(&mut self.tdf_spacing).range(0..=40));
                        });
                        ui.label("Text (ASCII; Enter for another line)");
                        egui::ScrollArea::vertical()
                            .id_salt("tdf_text_input")
                            .max_height(68.0)
                            .show(ui, |ui| {
                                ui.add(egui::TextEdit::multiline(&mut self.tdf_text).desired_rows(2));
                            });
                        ui.label("Click the canvas to choose a destination without painting.");
                        ui.label("Preview");
                        let result = font.render(&self.tdf_text, self.brush, self.tdf_spacing);
                        match result {
                            Ok(text) => {
                                ui.horizontal(|ui| {
                                    ui.label(format!(
                                        "{} x {} cells at column {}, row {}",
                                        text.width,
                                        text.height,
                                        self.cursor.0 + 1,
                                        self.cursor.1 + 1
                                    ));
                                    if ui.button("Place text").clicked() {
                                        if let Err(error) = self.place_tdf_text(&text) {
                                            self.message = Some(error);
                                        }
                                    }
                                });
                                egui::ScrollArea::both().id_salt("tdf_text_preview").max_height(220.0).show_viewport(
                                    ui,
                                    |ui, viewport| {
                                        let (response, painter) = ui.allocate_painter(
                                            Vec2::new(
                                                text.width as f32 * glyph::CELL_SIZE.x,
                                                text.height as f32 * glyph::CELL_SIZE.y,
                                            ),
                                            Sense::hover(),
                                        );
                                        painter.rect_filled(response.rect, 0.0, Color32::BLACK);
                                        let ice = self
                                            .doc
                                            .sauce
                                            .as_ref()
                                            .is_some_and(|s| s.flags() & 1 != 0);
                                        for &(x, y, cell) in &text.cells {
                                            let rect = glyph::cell_rect(
                                                response.rect.min,
                                                x,
                                                y,
                                                glyph::CELL_SIZE,
                                                ctx.pixels_per_point(),
                                            );
                                            if viewport.intersects(
                                                rect.translate(-response.rect.min.to_vec2()),
                                            ) {
                                                paint_cell(&painter, cell, rect, ice, 1.0);
                                            }
                                        }
                                    },
                                );
                            }
                            Err(error) => {
                                ui.colored_label(Color32::LIGHT_RED, error);
                            }
                        }
                        ui.label(
                            "Color fonts keep their colors; block and outline fonts use the brush style.",
                        );
                    });
                });
            });
        self.tdf_open = open;
    }
}

fn shortcut_text(ctx: &egui::Context, key: egui::Key) -> String {
    ctx.format_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, key))
}

fn font_definitions() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    #[cfg(target_os = "windows")]
    for (name, path) in [
        ("consolas", r"C:\Windows\Fonts\consola.ttf"),
        ("symbols", r"C:\Windows\Fonts\seguisym.ttf"),
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert(name.into(), egui::FontData::from_owned(bytes).into());
            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
                if name == "consolas" {
                    family.insert(0, name.into());
                } else {
                    family.push(name.into());
                }
            }
            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                family.push(name.into());
            }
        }
    }
    glyph::fonts(&mut fonts);
    material_fonts(&mut fonts);
    fonts
}

fn main() -> eframe::Result {
    // Pre-Mavericks Finder launches add a process serial number argument.
    let args = std::env::args_os()
        .skip(1)
        .filter(|arg| !(cfg!(target_os = "macos") && arg.to_string_lossy().starts_with("-psn_")));
    let (path, encoding) = match cli::parse(args) {
        Ok(cli::Command::Edit { path, encoding }) => (path, encoding),
        command => {
            #[cfg(windows)]
            cli::attach_parent_console();
            std::process::exit(cli::run(command, font_definitions()));
        }
    };
    #[cfg(target_os = "macos")]
    macos_open::install();
    let icon = eframe::icon_data::from_png_bytes(APP_ICON_PNG)
        .map_err(|error| eframe::Error::AppCreation(Box::new(error)))?;
    eframe::run_native(
        "ADraw",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            persist_window: false,
            viewport: egui::ViewportBuilder::default()
                .with_inner_size(DEFAULT_WINDOW_SIZE)
                .with_icon(icon),
            ..Default::default()
        },
        Box::new(move |cc| {
            cc.egui_ctx.set_fonts(font_definitions());
            let mut editor = Editor::new(cc.storage);
            editor.update_check = updates::UpdateCheck::start(cc.egui_ctx.clone());
            editor.encoding = encoding;
            if let Some(path) = path {
                editor.open_path(path);
            }
            #[cfg(target_os = "macos")]
            macos_open::set_context(cc.egui_ctx.clone());
            Ok(Box::new(editor))
        }),
    )
}

#[cfg(test)]
fn run_test_frame(
    ctx: &egui::Context,
    input: egui::RawInput,
    run: impl FnMut(&egui::Context),
) -> egui::FullOutput {
    let mut output = ctx.run_pass(input, run);
    output.textures_delta.clear();
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::Storage;
    use std::collections::BTreeMap;

    #[test]
    fn window_icon_matches_the_supplied_executable_icon_and_decodes() {
        let ico = include_bytes!("../assets/adraw.ico");
        let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
        let entry = ico[6..]
            .as_chunks::<16>()
            .0
            .iter()
            .take(count)
            .find(|entry| entry[0] == 0 && entry[1] == 0)
            .expect("ICO must include a 256x256 image");
        let length = u32::from_le_bytes(entry[8..12].try_into().unwrap()) as usize;
        let offset = u32::from_le_bytes(entry[12..16].try_into().unwrap()) as usize;
        assert_eq!(APP_ICON_PNG, &ico[offset..offset + length]);
        let icon = eframe::icon_data::from_png_bytes(APP_ICON_PNG).unwrap();
        assert_eq!((icon.width, icon.height), (256, 256));
        assert_eq!(icon.rgba.len(), 256 * 256 * 4);
        assert!(icon
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] != 0));
    }

    fn context() -> egui::Context {
        let ctx = egui::Context::default();
        let mut fonts = egui::FontDefinitions::default();
        glyph::fonts(&mut fonts);
        material_fonts(&mut fonts);
        ctx.set_fonts(fonts);
        ctx
    }

    #[derive(Default)]
    struct TestStorage(BTreeMap<String, String>);

    impl eframe::Storage for TestStorage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }

        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.to_owned(), value);
        }

        fn remove_string(&mut self, key: &str) {
            self.0.remove(key);
        }

        fn flush(&mut self) {}
    }

    #[test]
    fn sidebar_order_survives_save_reload_and_appends_new_sections() {
        let mut storage = TestStorage::default();
        let fonts = tempfile::tempdir().unwrap();
        let new_editor = |storage: Option<&dyn eframe::Storage>| {
            Editor::with_font_directory(storage, Some(fonts.path().join("fonts")))
        };
        let mut editor = new_editor(Some(&storage));
        editor.move_sidebar_section(SidebarSection::Colors, 0);
        editor.sidebar_collapsed = vec![SidebarSection::Colors, SidebarSection::Sauce];
        let expected = editor.sidebar_order.clone();
        eframe::App::save(&mut editor, &mut storage);
        let mut restored = new_editor(Some(&storage));
        assert_eq!(restored.sidebar_order, expected);
        assert_eq!(restored.sidebar_collapsed, editor.sidebar_collapsed);
        let output = frame(&context(), &mut restored, vec![]);
        let mut visible_order: Vec<_> = expected
            .iter()
            .map(|&section| (sidebar_handle(&output, section).y, section))
            .collect();
        visible_order.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(
            visible_order
                .into_iter()
                .map(|(_, section)| section)
                .collect::<Vec<_>>(),
            expected
        );
        assert!(!restored.dirty);

        storage.set_string(
            SIDEBAR_ORDER_KEY,
            "sauce\nbrush\nsauce\nretired-section".into(),
        );
        storage.set_string(
            SIDEBAR_COLLAPSED_KEY,
            "sauce\nsauce\nretired-section".into(),
        );
        let mut upgraded = new_editor(Some(&storage));
        assert_eq!(upgraded.sidebar_collapsed, [SidebarSection::Sauce]);
        assert_eq!(
            upgraded.sidebar_order,
            [
                SidebarSection::Sauce,
                SidebarSection::Brush,
                SidebarSection::Zoom,
                SidebarSection::Colors,
                SidebarSection::Attributes,
            ]
        );
        eframe::App::save(&mut upgraded, &mut storage);
        assert_eq!(
            new_editor(Some(&storage)).sidebar_order,
            upgraded.sidebar_order
        );
        assert_eq!(
            new_editor(Some(&storage)).sidebar_collapsed,
            upgraded.sidebar_collapsed
        );
        assert!(new_editor(None)
            .message
            .unwrap()
            .contains("cannot be saved"));
    }

    fn sidebar_handle(output: &egui::FullOutput, section: SidebarSection) -> Pos2 {
        sidebar_icon(output, section, ICON_DRAG_HANDLE)
    }

    fn sidebar_icon(
        output: &egui::FullOutput,
        section: SidebarSection,
        icon: MaterialIcon,
    ) -> Pos2 {
        let header = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) if shape.galley.text() == section.label() => {
                    Some(shape.galley.rect.translate(shape.pos.to_vec2()).center())
                }
                _ => None,
            })
            .expect("sidebar section has a heading");
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) if shape.galley.text() == icon.codepoint => {
                    Some(shape.galley.rect.translate(shape.pos.to_vec2()).center())
                }
                _ => None,
            })
            .min_by(|a, b| (a.y - header.y).abs().total_cmp(&(b.y - header.y).abs()))
            .expect("sidebar section has the requested header icon")
    }

    #[test]
    fn sidebar_sections_collapse_to_distinct_headers_and_remain_draggable() {
        for section in SidebarSection::ALL {
            let ctx = context();
            let mut editor = Editor::default();
            editor.doc.sauce.as_mut().unwrap().title = "Example title".into();
            let canvas = editor.doc.canvas.clone();
            let brush = editor.brush;
            let order = editor.sidebar_order.clone();
            let marker = match section {
                SidebarSection::Brush => ICON_BRUSH.codepoint,
                SidebarSection::Zoom => ICON_ZOOM_IN.codepoint,
                SidebarSection::Colors => ICON_PALETTE.codepoint,
                SidebarSection::Attributes => ICON_FORMAT_BOLD.codepoint,
                SidebarSection::Sauce => "Example title",
            };
            let has_body = |output: &egui::FullOutput| {
                output.shapes.iter().any(|clipped| {
                matches!(&clipped.shape, egui::Shape::Text(shape) if shape.galley.text() == marker)
            })
            };
            frame(&ctx, &mut editor, vec![]);
            let output = frame(&ctx, &mut editor, vec![]);
            assert!(has_body(&output));
            let toggle = sidebar_icon(&output, section, ICON_EXPAND_MORE);
            frame(
                &ctx,
                &mut editor,
                vec![
                    egui::Event::PointerMoved(toggle),
                    pointer_button(toggle, true),
                ],
            );
            frame(&ctx, &mut editor, vec![pointer_button(toggle, false)]);
            let output = frame(&ctx, &mut editor, vec![]);
            assert!(editor.sidebar_collapsed.contains(&section));
            assert_eq!(
                editor.sidebar_order, order,
                "the toggle does not reorder sections"
            );
            assert!(
                !has_body(&output),
                "collapsed {section:?} has no body controls"
            );
            let heading = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(shape) if shape.galley.text() == section.label() => {
                        Some(shape.galley.rect.translate(shape.pos.to_vec2()))
                    }
                    _ => None,
                })
                .unwrap();
            let header = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Rect(shape)
                        if shape.fill == ctx.global_style().visuals.widgets.inactive.bg_fill
                            && shape.rect.contains_rect(heading) =>
                    {
                        Some(shape.rect)
                    }
                    _ => None,
                })
                .expect("the header has its own shaded background");
            assert!(
                header.height() <= 26.0,
                "collapsed sections contain only a heading row"
            );
            assert!(
                output.shapes.iter().any(|clipped| {
                    matches!(&clipped.shape, egui::Shape::Rect(shape)
                    if (250.0..=270.0).contains(&shape.rect.width()) && shape.rect.height() > 300.0
                        && shape.rect.contains_rect(header)
                        && shape.fill != ctx.global_style().visuals.widgets.inactive.bg_fill)
                }),
                "header and body backgrounds are distinct"
            );
            let toggle = sidebar_icon(&output, section, ICON_CHEVRON_RIGHT);
            let handle = sidebar_handle(&output, section);
            assert!(toggle.x < heading.left());
            assert!(handle.x > heading.right());
            assert!(
                (header.right() - handle.x) <= 16.0,
                "drag handles align to the right edge"
            );
            let target = if section == SidebarSection::Brush {
                sidebar_handle(&output, SidebarSection::Attributes) + Vec2::new(-100.0, 38.0)
            } else {
                sidebar_handle(&output, SidebarSection::Brush)
            };
            frame(
                &ctx,
                &mut editor,
                vec![
                    egui::Event::PointerMoved(handle),
                    pointer_button(handle, true),
                ],
            );
            frame(&ctx, &mut editor, vec![egui::Event::PointerMoved(target)]);
            frame(&ctx, &mut editor, vec![pointer_button(target, false)]);
            let output = frame(&ctx, &mut editor, vec![]);
            assert!(editor.sidebar_collapsed.contains(&section));
            assert_ne!(
                editor.sidebar_order, order,
                "collapsed sections can be reordered"
            );
            let toggle = sidebar_icon(&output, section, ICON_CHEVRON_RIGHT);
            frame(
                &ctx,
                &mut editor,
                vec![
                    egui::Event::PointerMoved(toggle),
                    pointer_button(toggle, true),
                ],
            );
            frame(&ctx, &mut editor, vec![pointer_button(toggle, false)]);
            let output = frame(&ctx, &mut editor, vec![]);
            assert!(!editor.sidebar_collapsed.contains(&section));
            assert!(has_body(&output), "expanding restores the section controls");
            assert_eq!(editor.doc.canvas, canvas);
            assert_eq!(editor.brush, brush);
            assert_eq!(editor.doc.sauce.as_ref().unwrap().title, "Example title");
            assert!(!editor.dirty);
            assert!(!editor.history.undo(&mut editor.doc.canvas));
        }
    }

    #[test]
    fn every_sidebar_section_moves_on_release_without_editing_artwork() {
        for section in SidebarSection::ALL {
            let ctx = context();
            let size = Vec2::new(800.0, DEFAULT_WINDOW_SIZE.y);
            let mut editor = Editor::default();
            let original = editor.doc.canvas.clone();
            let order = editor.sidebar_order.clone();
            frame_sized(&ctx, &mut editor, vec![], size);
            let output = frame_sized(&ctx, &mut editor, vec![], size);
            let start = sidebar_handle(&output, section);
            let target = if section == SidebarSection::Brush {
                sidebar_handle(&output, SidebarSection::Attributes) + Vec2::new(-100.0, 38.0)
            } else {
                sidebar_handle(&output, SidebarSection::Brush)
            };
            frame_sized(
                &ctx,
                &mut editor,
                vec![
                    egui::Event::PointerMoved(start),
                    pointer_button(start, true),
                ],
                size,
            );
            frame_sized(
                &ctx,
                &mut editor,
                vec![egui::Event::PointerMoved(target)],
                size,
            );
            assert_eq!(
                editor.sidebar_order, order,
                "hovering previews rather than committing"
            );
            frame_sized(&ctx, &mut editor, vec![pointer_button(target, false)], size);
            let mut expected = order;
            expected.retain(|&item| item != section);
            expected.insert(
                if section == SidebarSection::Brush {
                    3
                } else {
                    0
                },
                section,
            );
            assert_eq!(editor.sidebar_order, expected, "{section:?} can be dragged");
            let output = frame_sized(&ctx, &mut editor, vec![], size);
            let mut last_y = 0.0;
            for item in expected {
                let y = sidebar_handle(&output, item).y;
                assert!(y > last_y, "the layout follows the new order");
                last_y = y;
            }
            assert_eq!(editor.doc.canvas, original);
            assert!(!editor.dirty);
            assert!(!editor.history.undo(&mut editor.doc.canvas));
        }
    }

    #[test]
    fn sidebar_drag_accepts_a_complete_gesture_in_one_frame() {
        let ctx = context();
        let mut editor = Editor::default();
        frame(&ctx, &mut editor, vec![]);
        let output = frame(&ctx, &mut editor, vec![]);
        let start = sidebar_handle(&output, SidebarSection::Sauce);
        let target = sidebar_handle(&output, SidebarSection::Brush);
        frame(
            &ctx,
            &mut editor,
            vec![
                egui::Event::PointerMoved(start),
                pointer_button(start, true),
                egui::Event::PointerMoved(target),
                pointer_button(target, false),
            ],
        );
        assert_eq!(editor.sidebar_order[0], SidebarSection::Sauce);
    }

    #[test]
    fn sidebar_drag_keeps_button_event_positions_when_hover_is_cleared() {
        let ctx = context();
        let mut editor = Editor::default();
        frame(&ctx, &mut editor, vec![]);
        let output = frame(&ctx, &mut editor, vec![]);
        let start = sidebar_handle(&output, SidebarSection::Sauce);
        let target = sidebar_handle(&output, SidebarSection::Brush);
        frame(
            &ctx,
            &mut editor,
            vec![
                egui::Event::PointerMoved(start),
                pointer_button(start, true),
                egui::Event::PointerGone,
            ],
        );
        frame(
            &ctx,
            &mut editor,
            vec![
                egui::Event::PointerMoved(target),
                pointer_button(target, false),
                egui::Event::PointerGone,
            ],
        );
        assert_eq!(editor.sidebar_order[0], SidebarSection::Sauce);
        assert!(!editor.dirty);
    }

    #[test]
    fn sidebar_drag_scrolls_when_held_near_the_viewport_edge() {
        let ctx = context();
        let mut editor = Editor::default();
        frame(&ctx, &mut editor, vec![]);
        let output = frame(&ctx, &mut editor, vec![]);
        let start = sidebar_handle(&output, SidebarSection::Brush);
        let original_y = sidebar_handle(&output, SidebarSection::Sauce).y;
        frame(
            &ctx,
            &mut editor,
            vec![
                egui::Event::PointerMoved(start),
                pointer_button(start, true),
            ],
        );
        let edge = Pos2::new(100.0, 590.0);
        frame(&ctx, &mut editor, vec![egui::Event::PointerMoved(edge)]);
        for _ in 0..20 {
            frame(&ctx, &mut editor, vec![]);
        }
        let output = frame(&ctx, &mut editor, vec![]);
        assert!(sidebar_handle(&output, SidebarSection::Sauce).y < original_y - 4.0);
        assert_eq!(editor.sidebar_order, SidebarSection::ALL);
        frame(
            &ctx,
            &mut editor,
            vec![key(egui::Key::Escape), pointer_button(edge, false)],
        );
        assert!(!editor.dirty);
    }

    #[test]
    fn sidebar_drag_cancels_on_escape_or_release_outside_sidebar() {
        for escape in [false, true] {
            let ctx = context();
            let mut editor = Editor::default();
            let original = editor.doc.canvas.clone();
            frame(&ctx, &mut editor, vec![]);
            let output = frame(&ctx, &mut editor, vec![]);
            let start = sidebar_handle(&output, SidebarSection::Brush);
            frame(
                &ctx,
                &mut editor,
                vec![
                    egui::Event::PointerMoved(start),
                    pointer_button(start, true),
                ],
            );
            let target = Pos2::new(600.0, 300.0);
            frame(&ctx, &mut editor, vec![egui::Event::PointerMoved(target)]);
            assert!(egui::DragAndDrop::has_payload_of_type::<SidebarSection>(
                &ctx
            ));
            if escape {
                frame(&ctx, &mut editor, vec![key(egui::Key::Escape)]);
            }
            frame(&ctx, &mut editor, vec![pointer_button(target, false)]);
            assert_eq!(editor.sidebar_order, SidebarSection::ALL);
            assert_eq!(editor.doc.canvas, original);
            assert!(!editor.dirty);
            assert!(!egui::DragAndDrop::has_payload_of_type::<SidebarSection>(
                &ctx
            ));
        }
    }

    #[test]
    fn initial_window_fits_default_canvas_and_all_expanded_sidebar_sections() {
        let ctx = context();
        let mut editor = Editor::default();
        editor.sidebar_collapsed.clear();
        editor.doc.sauce.as_mut().unwrap().title = "Example title".into();
        let screen = Rect::from_min_size(Pos2::ZERO, DEFAULT_WINDOW_SIZE);
        let mut render = || {
            crate::run_test_frame(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| ctx.root_ui(|ui| editor.ui(ui)),
            )
        };
        render();
        let output = render();
        let mut visible_cells = 0;
        let mut metadata_bottom_visible = false;
        let mut title_visible = false;
        let mut canvas_bounds = Rect::NOTHING;
        for clipped in &output.shapes {
            match &clipped.shape {
                egui::Shape::Rect(shape)
                    if shape.fill == Color32::BLACK && shape.rect.size() == glyph::CELL_SIZE =>
                {
                    assert!(clipped.clip_rect.contains_rect(shape.rect));
                    assert!(screen.contains_rect(shape.rect));
                    visible_cells += 1;
                    canvas_bounds = canvas_bounds.union(shape.rect);
                }
                egui::Shape::Text(shape) if shape.galley.text() == "Dimensions: 80 x 25" => {
                    let rect = shape.galley.rect.translate(shape.pos.to_vec2());
                    metadata_bottom_visible =
                        clipped.clip_rect.contains_rect(rect) && screen.contains_rect(rect);
                    let bottom_padding = screen.bottom() - rect.bottom();
                    assert!(
                        bottom_padding >= 8.0,
                        "startup bottom padding below Dimensions is {bottom_padding}"
                    );
                }
                egui::Shape::Text(shape) if shape.galley.text() == "Example title" => {
                    let rect = shape.galley.rect.translate(shape.pos.to_vec2());
                    title_visible = clipped.clip_rect.contains_rect(rect);
                }
                _ => {}
            }
        }
        assert_eq!(visible_cells, 80 * 25);
        assert!(metadata_bottom_visible);
        assert!(title_visible);
        let right_padding = screen.right() - canvas_bounds.right();
        assert!(
            (198.0..=199.0).contains(&right_padding),
            "startup right padding including scrollbar space is {right_padding}"
        );
        assert!(!output_has_horizontal_scrollbar(&output));
    }

    fn output_has_horizontal_scrollbar(output: &egui::FullOutput) -> bool {
        output.shapes.iter().any(|clipped| {
            matches!(&clipped.shape, egui::Shape::Rect(shape)
                if shape.rect.height() == egui::style::ScrollStyle::solid().bar_width
                    && shape.rect.width() > 400.0
                    && shape.fill.is_opaque())
        })
    }

    #[test]
    fn startup_width_fits_eighty_columns_with_a_vertical_scrollbar() {
        let ctx = context();
        ctx.global_style_mut(|style| style.animation_time = 0.0);
        let mut editor = Editor::default();
        editor.doc.canvas = Canvas::new(80, 51).unwrap();
        let screen = Rect::from_min_size(Pos2::ZERO, DEFAULT_WINDOW_SIZE);
        let mut render = || {
            crate::run_test_frame(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| ctx.root_ui(|ui| editor.ui(ui)),
            )
        };
        render();
        let output = render();
        let mut canvas_right = 0.0_f32;
        let mut scrollbar = None;
        for clipped in &output.shapes {
            if let egui::Shape::Rect(shape) = &clipped.shape {
                if shape.fill == Color32::BLACK && shape.rect.size() == glyph::CELL_SIZE {
                    assert!(shape.rect.left() >= clipped.clip_rect.left());
                    assert!(shape.rect.right() <= clipped.clip_rect.right());
                    canvas_right = canvas_right.max(shape.rect.right());
                }
                if shape.rect.width() == egui::style::ScrollStyle::solid().bar_width
                    && shape.rect.height() > 400.0
                    && shape.fill.is_opaque()
                {
                    scrollbar = Some(shape.rect);
                }
            }
        }
        let scrollbar = scrollbar.expect("taller artwork has a vertical scrollbar");
        assert!((4.0..=5.0).contains(&(scrollbar.left() - canvas_right)));
        assert!((188.0..=189.0).contains(&(screen.right() - scrollbar.right())));
        assert!(!output_has_horizontal_scrollbar(&output));
    }

    #[test]
    fn default_window_fits_eighty_columns_with_minimap() {
        for height in [25, 51] {
            let ctx = context();
            ctx.global_style_mut(|style| style.animation_time = 0.0);
            let mut editor = Editor {
                minimap_visible: true,
                ..Default::default()
            };
            editor.doc.canvas = Canvas::new(80, height).unwrap();
            frame_sized(&ctx, &mut editor, vec![], DEFAULT_WINDOW_SIZE);
            let output = frame_sized(&ctx, &mut editor, vec![], DEFAULT_WINDOW_SIZE);
            let view = editor.minimap.viewport.unwrap();
            assert_eq!(view.size.x, view.canvas_size.x);
            assert!(!output_has_horizontal_scrollbar(&output));
        }
    }

    #[test]
    fn canvas_scroll_does_not_overlay_edge_fades() {
        let ctx = context();
        let mut editor = Editor {
            minimap_visible: true,
            ..Default::default()
        };
        editor.doc.canvas = Canvas::new(200, 200).unwrap();
        frame_sized(&ctx, &mut editor, vec![], DEFAULT_WINDOW_SIZE);
        let output = frame_sized(&ctx, &mut editor, vec![], DEFAULT_WINDOW_SIZE);
        let view = editor.minimap.viewport.unwrap();
        assert!(view.canvas_size.x > view.size.x && view.canvas_size.y > view.size.y);
        assert!(!output.shapes.iter().any(|clipped| {
            matches!(&clipped.shape, egui::Shape::Mesh(mesh)
                if mesh.calc_bounds().left() >= 290.0
                    && mesh.calc_bounds().right() <= view.map.left()
                    && mesh.vertices.iter().any(|vertex| vertex.color.a() == 0)
                    && mesh.vertices.iter().any(|vertex| vertex.color.a() > 0))
        }));
    }

    fn frame(
        ctx: &egui::Context,
        editor: &mut Editor,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        frame_sized(ctx, editor, events, Vec2::new(800.0, 600.0))
    }

    fn frame_sized(
        ctx: &egui::Context,
        editor: &mut Editor,
        mut events: Vec<egui::Event>,
        size: Vec2,
    ) -> egui::FullOutput {
        events.insert(0, egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            focused: true,
            events,
            ..Default::default()
        };
        crate::run_test_frame(ctx, input, |ctx| ctx.root_ui(|ui| editor.ui(ui)))
    }

    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn pointer_button(pos: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn color_click(pos: Pos2, pressed: bool, background: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: if background {
                egui::PointerButton::Secondary
            } else {
                egui::PointerButton::Primary
            },
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn canvas_cell_center(output: &egui::FullOutput, x: usize, y: usize) -> Pos2 {
        let origin = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if shape.fill == Color32::BLACK && shape.rect.size() == glyph::CELL_SIZE =>
                {
                    Some(shape.rect.min)
                }
                _ => None,
            })
            .unwrap();
        origin
            + Vec2::new(
                (x as f32 + 0.5) * glyph::CELL_SIZE.x,
                (y as f32 + 0.5) * glyph::CELL_SIZE.y,
            )
    }

    fn rgb_picker_area(output: &egui::FullOutput) -> Rect {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if shape.rect.size() == Vec2::splat(275.0) && shape.brush.is_some() =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
            .expect("RGB saturation/value control is visible")
    }

    #[test]
    fn cp437_picker_uses_vga_bitmaps_and_preserves_character_selection() {
        let ctx = context();
        ctx.global_style_mut(|style| style.animation_time = 0.0);
        let mut editor = Editor::default();
        editor.brush.ch = '☺';
        editor.picker_open = true;
        let original = editor.doc.canvas.clone();
        frame(&ctx, &mut editor, vec![]);
        let output = frame(&ctx, &mut editor, vec![]);
        assert!(output.shapes.iter().any(|clipped| {
            matches!(&clipped.shape, egui::Shape::Text(shape)
                if shape.galley.text() == "☺"
                    && shape.galley.job.sections[0].format.font_id.family
                        == egui::FontFamily::Name(glyph::FONT_NAME.into()))
        }));
        let character = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape) if shape.rect.size() == Vec2::splat(32.0) => {
                    Some(shape.rect)
                }
                _ => None,
            })
            .nth(b'A' as usize - 1)
            .unwrap();
        assert!(output.shapes.iter().any(|clipped| {
            matches!(&clipped.shape, egui::Shape::Mesh(mesh)
                if !mesh.vertices.is_empty() && character.contains_rect(mesh.calc_bounds()))
        }));
        let pos = character.center();
        frame(
            &ctx,
            &mut editor,
            vec![egui::Event::PointerMoved(pos), pointer_button(pos, true)],
        );
        frame(&ctx, &mut editor, vec![pointer_button(pos, false)]);
        assert_eq!(editor.brush.ch, 'A');
        assert!(!editor.picker_open);
        assert_eq!(editor.doc.canvas, original);
        assert!(!editor.dirty);
    }

    #[test]
    fn unicode_outside_cp437_retains_scaled_text_fallback() {
        let ctx = context();
        let output = crate::run_test_frame(&ctx, egui::RawInput::default(), |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Middle,
                egui::Id::unique("unicode"),
            ));
            paint_cell(
                &painter,
                Cell {
                    ch: '€',
                    ..Cell::default()
                },
                Rect::from_min_size(Pos2::new(20.0, 20.0), glyph::CELL_SIZE * 2.0),
                false,
                2.0,
            );
        });
        assert!(output.shapes.iter().any(|clipped| {
            matches!(&clipped.shape, egui::Shape::Text(shape)
                if shape.galley.text() == "€" && shape.galley.job.sections[0].format.font_id.size == 32.0)
        }));
    }

    #[test]
    fn sidebar_zoom_icons_and_slider_center_preserve_artwork() {
        let ctx = context();
        ctx.global_style_mut(|style| style.animation_time = 0.0);
        let mut editor = Editor::default();
        let original = editor.doc.canvas.clone();
        click_text(&ctx, &mut editor, ICON_ZOOM_IN);
        assert_eq!(editor.zoom_percent, 125.0);
        click_text(&ctx, &mut editor, ICON_SEARCH);
        frame(&ctx, &mut editor, vec![]);
        assert_eq!(
            editor.zoom_percent, 125.0,
            "opening the slider preserves zoom"
        );
        click_text(&ctx, &mut editor, ICON_SEARCH);
        click_text(&ctx, &mut editor, ICON_ZOOM_OUT);
        assert_eq!(editor.zoom_percent, 100.0);
        click_text(&ctx, &mut editor, ICON_SEARCH);
        let output = frame(&ctx, &mut editor, vec![]);
        let popup_clip = output.shapes.iter().find_map(|clipped| {
            matches!(&clipped.shape, egui::Shape::Text(shape) if shape.galley.text() == "Zoom: 100%")
                .then_some(clipped.clip_rect)
        }).expect("zoom popup is visible");
        let rail = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if clipped.clip_rect == popup_clip
                        && shape.rect.width() > 200.0
                        && shape.rect.height() <= 12.0 =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
            .expect("zoom slider rail is visible");
        let thumb = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Circle(shape) if (shape.center.y - rail.center().y).abs() < 1.0 => {
                    Some(shape.center)
                }
                egui::Shape::Rect(shape)
                    if clipped.clip_rect == popup_clip
                        && shape.rect.width() < 30.0
                        && shape.rect.height() < 30.0
                        && (shape.rect.center().y - rail.center().y).abs() < 1.0 =>
                {
                    Some(shape.rect.center())
                }
                _ => None,
            })
            .expect("zoom slider thumb is visible");
        assert!(
            (thumb.x - rail.center().x).abs() < 0.01,
            "100% is at the physical midpoint"
        );
        frame(
            &ctx,
            &mut editor,
            vec![
                egui::Event::PointerMoved(thumb),
                pointer_button(thumb, true),
            ],
        );
        let right = rail.right_center();
        frame(&ctx, &mut editor, vec![egui::Event::PointerMoved(right)]);
        assert_eq!(editor.zoom_percent, 400.0);
        let left = rail.left_center();
        frame(&ctx, &mut editor, vec![egui::Event::PointerMoved(left)]);
        assert_eq!(editor.zoom_percent, 50.0);
        let center = rail.center();
        frame(&ctx, &mut editor, vec![egui::Event::PointerMoved(center)]);
        assert_eq!(editor.zoom_percent, 100.0);
        frame(&ctx, &mut editor, vec![pointer_button(center, false)]);
        click_text(&ctx, &mut editor, ICON_SEARCH);
        click_text(&ctx, &mut editor, ICON_ZOOM_IN);
        click_text(&ctx, &mut editor, ICON_CENTER_FOCUS_STRONG);
        assert_eq!(editor.zoom_percent, 100.0);
        assert_eq!(editor.doc.canvas, original);
        assert!(!editor.dirty);
    }

    #[test]
    fn native_command_shortcuts_undo_redo_and_zoom_without_typing() {
        for modifiers in [
            egui::Modifiers::CTRL | egui::Modifiers::COMMAND,
            egui::Modifiers::MAC_CMD | egui::Modifiers::COMMAND,
        ] {
            let ctx = context();
            let mut editor = Editor::default();
            let cell = Cell {
                ch: 'A',
                ..Cell::default()
            };
            editor.history.paint(&mut editor.doc.canvas, 0, 0, cell);
            for (key, expected_ch, expected_zoom) in [
                (egui::Key::Z, ' ', 100.0),
                (egui::Key::Y, 'A', 100.0),
                (egui::Key::Plus, 'A', 125.0),
                (egui::Key::Minus, 'A', 100.0),
                (egui::Key::Num0, 'A', 100.0),
            ] {
                frame(
                    &ctx,
                    &mut editor,
                    vec![egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers,
                    }],
                );
                assert_eq!(editor.doc.canvas.get(0, 0).unwrap().ch, expected_ch);
                assert_eq!(editor.zoom_percent, expected_zoom);
            }
            editor.undo();
            let mut redo = key(egui::Key::Z);
            if let egui::Event::Key {
                modifiers: event_modifiers,
                ..
            } = &mut redo
            {
                *event_modifiers = modifiers | egui::Modifiers::SHIFT;
            }
            frame(&ctx, &mut editor, vec![redo]);
            assert_eq!(editor.doc.canvas.get(0, 0).unwrap().ch, 'A');
        }
    }

    #[test]
    fn zoom_menus_and_shortcuts_scale_cells_and_glyphs_without_changing_saved_art() {
        let ctx = context();
        let mut editor = Editor::default();
        editor.doc.canvas = Canvas::new(12, 6).unwrap();
        let cell = Cell {
            ch: 'A',
            fg: Color::Rgb(233, 22, 88),
            bg: Color::Rgb(3, 7, 11),
            ..Cell::default()
        };
        editor.doc.canvas.set(0, 0, cell);
        let bytes = ans_io::save(&editor.doc, editor.encoding).unwrap();
        click_text(&ctx, &mut editor, "View");
        click_text(&ctx, &mut editor, "Zoom in");
        assert_eq!(editor.zoom_percent, 125.0);
        for (key, modifiers) in [
            (egui::Key::Equals, egui::Modifiers::COMMAND),
            (
                egui::Key::Plus,
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            ),
        ] {
            frame(
                &ctx,
                &mut editor,
                vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                }],
            );
        }
        assert_eq!(editor.zoom_percent, 200.0);
        let output = frame(&ctx, &mut editor, vec![]);
        assert!(output.shapes.iter().any(|clipped| {
            matches!(&clipped.shape, egui::Shape::Rect(shape)
                if shape.fill == display_color(cell.bg) && shape.rect.size() == glyph::CELL_SIZE * 2.0)
        }));
        assert!(output.shapes.iter().any(|clipped| {
            matches!(&clipped.shape, egui::Shape::Mesh(mesh)
                if mesh.vertices.iter().all(|v| v.color == display_color(cell.fg))
                    && mesh.calc_bounds().size() == Vec2::new(14.0, 20.0))
        }));
        click_text(&ctx, &mut editor, "View");
        click_text(&ctx, &mut editor, "Actual size (100%)");
        assert_eq!(editor.zoom_percent, 100.0);
        click_text(&ctx, &mut editor, "View");
        click_text(&ctx, &mut editor, "Zoom out");
        assert_eq!(editor.zoom_percent, 75.0);
        frame(
            &ctx,
            &mut editor,
            vec![egui::Event::Key {
                key: egui::Key::Num0,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
        );
        assert_eq!(editor.zoom_percent, 100.0);
        assert_eq!(ans_io::save(&editor.doc, editor.encoding).unwrap(), bytes);
        assert!(!editor.dirty);
    }

    #[test]
    fn minimap_view_option_is_saved_without_changing_the_artwork() {
        let ctx = context();
        let mut editor = Editor::default();
        editor.doc.canvas = Canvas::new(8, 4).unwrap();
        let canvas = editor.doc.canvas.clone();
        frame(&ctx, &mut editor, vec![]);
        assert!(editor.minimap.viewport.is_none());

        click_text(&ctx, &mut editor, "View");
        click_text(&ctx, &mut editor, "Show minimap");
        assert!(editor.minimap_visible);
        let view = editor.minimap.viewport.expect("minimap is visible");
        assert!(view.map.left() > 600.0);
        assert_eq!(view.indicator(), view.map);
        let center = view.map.center();
        frame(
            &ctx,
            &mut editor,
            vec![
                egui::Event::PointerMoved(center),
                pointer_button(center, true),
            ],
        );
        frame(&ctx, &mut editor, vec![pointer_button(center, false)]);
        frame(&ctx, &mut editor, vec![]);
        let view = editor.minimap.viewport.unwrap();
        assert_eq!(view.indicator(), view.map);
        assert_eq!(editor.doc.canvas, canvas);
        assert!(!editor.dirty);

        let fonts = tempfile::tempdir().unwrap();
        let mut storage = TestStorage::default();
        eframe::App::save(&mut editor, &mut storage);
        let restored =
            Editor::with_font_directory(Some(&storage), Some(fonts.path().join("fonts")));
        assert!(restored.minimap_visible);

        click_text(&ctx, &mut editor, "View");
        click_text(&ctx, &mut editor, "Show minimap");
        assert!(!editor.minimap_visible);
        assert!(editor.minimap.viewport.is_none());
        eframe::App::save(&mut editor, &mut storage);
        assert!(
            !Editor::with_font_directory(Some(&storage), Some(fonts.path().join("fonts")))
                .minimap_visible
        );
        storage.set_string(MINIMAP_VISIBLE_KEY, "invalid".into());
        let restored =
            Editor::with_font_directory(Some(&storage), Some(fonts.path().join("fonts")));
        assert!(restored.message.unwrap().contains("minimap preference"));
        assert_eq!(editor.doc.canvas, canvas);
        assert!(!editor.dirty);
    }

    #[test]
    fn minimap_viewport_outline_does_not_tint_the_artwork() {
        let ctx = context();
        let mut editor = Editor {
            minimap_visible: true,
            ..Default::default()
        };
        editor.doc.canvas = Canvas::new(200, 200).unwrap();
        let screen = Vec2::new(1000.0, 700.0);
        frame_sized(&ctx, &mut editor, vec![], screen);
        let output = frame_sized(&ctx, &mut editor, vec![], screen);
        let view = editor.minimap.viewport.unwrap();
        let indicator = view.indicator();
        assert!(indicator.width() < view.map.width());
        assert!(indicator.height() < view.map.height());

        let rectangles: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(rect) if rect.rect == indicator => Some(rect),
                _ => None,
            })
            .collect();
        assert!(rectangles.iter().any(|rect| rect.stroke.width > 0.0));
        assert!(
            rectangles
                .iter()
                .all(|rect| rect.fill == Color32::TRANSPARENT),
            "the viewport indicator must not tint the artwork"
        );
    }

    #[test]
    fn minimap_tracks_scrolling_and_clicking_and_dragging_navigate_both_axes() {
        let ctx = context();
        ctx.global_style_mut(|style| style.animation_time = 0.0);
        let mut editor = Editor {
            minimap_visible: true,
            ..Default::default()
        };
        editor.doc.canvas = Canvas::new(200, 200).unwrap();
        let original = editor.doc.canvas.clone();
        let screen = Vec2::new(1000.0, 700.0);
        frame_sized(&ctx, &mut editor, vec![], screen);
        frame_sized(&ctx, &mut editor, vec![], screen);
        let initial = editor.minimap.viewport.unwrap();
        assert_eq!(initial.offset, Vec2::ZERO);
        assert!(initial.indicator().height() < initial.map.height());
        assert!(initial.indicator().width() < initial.map.width());

        frame_sized(
            &ctx,
            &mut editor,
            vec![
                egui::Event::PointerMoved(Pos2::new(350.0, 200.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(0.0, -240.0),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Unknown,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            screen,
        );
        frame_sized(&ctx, &mut editor, vec![], screen);
        let scrolled = editor.minimap.viewport.unwrap();
        assert!(scrolled.offset.y > 0.0);
        assert!(scrolled.indicator().top() > initial.indicator().top());

        let center = scrolled.map.center();
        frame_sized(
            &ctx,
            &mut editor,
            vec![
                egui::Event::PointerMoved(center),
                pointer_button(center, true),
            ],
            screen,
        );
        frame_sized(
            &ctx,
            &mut editor,
            vec![pointer_button(center, false)],
            screen,
        );
        frame_sized(&ctx, &mut editor, vec![], screen);
        let centered = editor.minimap.viewport.unwrap();
        assert!((centered.indicator().center() - centered.map.center()).length() < 2.0);

        let bottom_right = centered.map.right_bottom() - Vec2::splat(1.0);
        frame_sized(
            &ctx,
            &mut editor,
            vec![
                egui::Event::PointerMoved(bottom_right),
                pointer_button(bottom_right, true),
            ],
            screen,
        );
        frame_sized(
            &ctx,
            &mut editor,
            vec![pointer_button(bottom_right, false)],
            screen,
        );
        frame_sized(&ctx, &mut editor, vec![], screen);
        let bottom = editor.minimap.viewport.unwrap();
        let max_offset = bottom.canvas_size - bottom.size;
        assert!(
            (bottom.offset - max_offset).length() < 2.0,
            "bottom viewport: {bottom:?}, maximum offset: {max_offset:?}"
        );
        assert!((bottom.indicator().bottom() - bottom.map.bottom()).abs() < 0.1);
        assert!((bottom.indicator().right() - bottom.map.right()).abs() < 0.1);

        let start = bottom.indicator().center();
        let end = Pos2::new(start.x, bottom.map.top());
        frame_sized(
            &ctx,
            &mut editor,
            vec![
                egui::Event::PointerMoved(start),
                pointer_button(start, true),
            ],
            screen,
        );
        frame_sized(
            &ctx,
            &mut editor,
            vec![egui::Event::PointerMoved(end)],
            screen,
        );
        frame_sized(&ctx, &mut editor, vec![pointer_button(end, false)], screen);
        frame_sized(&ctx, &mut editor, vec![], screen);
        let top = editor.minimap.viewport.unwrap();
        assert!(top.offset.y < 1.0);
        assert!((top.offset.x - bottom.offset.x).abs() < 2.0);
        assert!((top.indicator().top() - top.map.top()).abs() < 0.1);
        assert_eq!(editor.doc.canvas, original);
        assert!(!editor.dirty);
    }

    #[test]
    fn minimap_navigation_cancels_pending_canvas_scroll() {
        let ctx = context();
        let mut editor = Editor {
            minimap_visible: true,
            ..Default::default()
        };
        editor.doc.canvas = Canvas::new(200, 200).unwrap();
        let screen = Vec2::new(1000.0, 700.0);
        frame_sized(&ctx, &mut editor, vec![], screen);
        let output = frame_sized(&ctx, &mut editor, vec![], screen);
        let canvas = canvas_cell_center(&output, 0, 0);
        frame_sized(
            &ctx,
            &mut editor,
            vec![
                egui::Event::PointerMoved(canvas),
                pointer_button(canvas, true),
            ],
            screen,
        );
        frame_sized(
            &ctx,
            &mut editor,
            vec![pointer_button(canvas, false)],
            screen,
        );
        frame_sized(&ctx, &mut editor, vec![key(egui::Key::End)], screen);
        assert_eq!(editor.cursor.0, 199);

        let center = editor.minimap.viewport.unwrap().map.center();
        frame_sized(
            &ctx,
            &mut editor,
            vec![
                egui::Event::PointerMoved(center),
                pointer_button(center, true),
            ],
            screen,
        );
        frame_sized(
            &ctx,
            &mut editor,
            vec![pointer_button(center, false)],
            screen,
        );
        for _ in 0..30 {
            frame_sized(&ctx, &mut editor, vec![], screen);
        }
        let view = editor.minimap.viewport.unwrap();
        assert!(
            (view.indicator().center() - view.map.center()).length() < 2.0,
            "minimap navigation should not resume the earlier cursor scroll: {view:?}"
        );
    }

    #[test]
    fn minimap_navigation_preserves_single_axis_scrollbar_layout() {
        for (width, height) in [(40, 200), (200, 20)] {
            let ctx = context();
            ctx.global_style_mut(|style| style.animation_time = 0.0);
            let mut editor = Editor {
                minimap_visible: true,
                ..Default::default()
            };
            editor.doc.canvas = Canvas::new(width, height).unwrap();
            let screen = Vec2::new(1000.0, 700.0);
            frame_sized(&ctx, &mut editor, vec![], screen);
            frame_sized(&ctx, &mut editor, vec![], screen);
            let before = editor.minimap.viewport.unwrap();
            let max_offset = (before.canvas_size - before.size).max(Vec2::ZERO);
            assert_ne!(max_offset.x > 0.0, max_offset.y > 0.0);

            let end = before.map.right_bottom() - Vec2::splat(1.0);
            frame_sized(
                &ctx,
                &mut editor,
                vec![egui::Event::PointerMoved(end), pointer_button(end, true)],
                screen,
            );
            frame_sized(&ctx, &mut editor, vec![pointer_button(end, false)], screen);
            frame_sized(&ctx, &mut editor, vec![], screen);
            let after = editor.minimap.viewport.unwrap();
            assert_eq!(after.size, before.size);
            assert!(
                (after.offset - max_offset).length() < 2.0,
                "canvas {width}x{height}: {after:?}"
            );
        }
    }

    #[test]
    fn minimap_indicator_tracks_zoom_and_window_size() {
        let ctx = context();
        ctx.global_style_mut(|style| style.animation_time = 0.0);
        let mut editor = Editor {
            minimap_visible: true,
            ..Default::default()
        };
        editor.doc.canvas = Canvas::new(80, 80).unwrap();
        let screen = Vec2::new(1000.0, 700.0);
        frame_sized(&ctx, &mut editor, vec![], screen);
        frame_sized(&ctx, &mut editor, vec![], screen);
        let initial = editor.minimap.viewport.unwrap();

        editor.zoom_percent = 200.0;
        frame_sized(&ctx, &mut editor, vec![], screen);
        frame_sized(&ctx, &mut editor, vec![], screen);
        let zoomed = editor.minimap.viewport.unwrap();
        assert_eq!(zoomed.map, initial.map);
        assert_eq!(zoomed.canvas_size, initial.canvas_size * 2.0);
        assert!(zoomed.indicator().width() < initial.indicator().width());
        assert!(zoomed.indicator().height() < initial.indicator().height());

        let larger_screen = Vec2::new(1200.0, 900.0);
        frame_sized(&ctx, &mut editor, vec![], larger_screen);
        frame_sized(&ctx, &mut editor, vec![], larger_screen);
        let resized = editor.minimap.viewport.unwrap();
        assert!(resized.indicator().width() > zoomed.indicator().width());
        assert!(resized.indicator().height() > zoomed.indicator().height());
        assert_eq!(editor.doc.canvas, Canvas::new(80, 80).unwrap());
        assert!(!editor.dirty);
    }

    fn frame_minimap(
        ctx: &egui::Context,
        editor: &mut Editor,
    ) -> (egui::TextureId, Option<egui::ColorImage>) {
        frame_minimap_with_events(ctx, editor, vec![])
    }

    fn frame_minimap_with_events(
        ctx: &egui::Context,
        editor: &mut Editor,
        events: Vec<egui::Event>,
    ) -> (egui::TextureId, Option<egui::ColorImage>) {
        let mut output = ctx.run_pass(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 700.0))),
                focused: true,
                events,
                ..Default::default()
            },
            |ctx| ctx.root_ui(|ui| editor.ui(ui)),
        );
        let map = editor.minimap.viewport.map(|view| view.map);
        let id = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Mesh(mesh) if Some(mesh.calc_bounds()) == map => Some(mesh.texture_id),
                _ => None,
            });
        let image = id
            .and_then(|id| output.textures_delta.set.get(&id))
            .and_then(|deltas| deltas.last())
            .map(|delta| match &delta.image {
                egui::ImageData::Color(image) => image.as_ref().clone(),
            });
        output.textures_delta.clear();
        (id.expect("minimap texture is drawn"), image)
    }

    #[test]
    fn resizing_minimap_reuses_preview_texture_while_updating_viewport() {
        let ctx = context();
        ctx.global_style_mut(|style| style.animation_time = 0.0);
        let mut editor = Editor {
            minimap_visible: true,
            ..Default::default()
        };
        editor.doc.canvas = Canvas::new(80, 80).unwrap();
        let (id, image) = frame_minimap(&ctx, &mut editor);
        assert!(image.is_some());
        frame_minimap(&ctx, &mut editor);
        let initial = editor.minimap.viewport.unwrap();
        let edge = Pos2::new(1000.0 - MINIMAP_DEFAULT_WIDTH, initial.map.center().y);
        for events in [
            vec![egui::Event::PointerMoved(edge), pointer_button(edge, true)],
            vec![egui::Event::PointerMoved(edge - Vec2::new(70.0, 0.0))],
            vec![],
        ] {
            let (texture, image) = frame_minimap_with_events(&ctx, &mut editor, events);
            assert_eq!(texture, id);
            assert!(
                image.is_none(),
                "resizing must not rebuild or upload the preview"
            );
        }
        let wider = editor.minimap.viewport.unwrap();
        assert!(wider.map.width() > initial.map.width() + 40.0);
        assert!(wider.size.x < initial.size.x);
        assert_ne!(wider.indicator(), initial.indicator());

        let narrower = edge + Vec2::new(30.0, 0.0);
        for events in [
            vec![egui::Event::PointerMoved(narrower)],
            vec![],
            vec![pointer_button(narrower, false)],
            vec![],
        ] {
            let (texture, image) = frame_minimap_with_events(&ctx, &mut editor, events);
            assert_eq!(texture, id);
            assert!(
                image.is_none(),
                "releasing resize must not rebuild the preview"
            );
        }
        let resized = editor.minimap.viewport.unwrap();
        assert!(resized.map.width() < initial.map.width() - 20.0);
        assert!(resized.size.x > initial.size.x);
        assert!(!editor.dirty);
    }

    #[test]
    fn drawing_with_minimap_updates_only_a_small_texture_region() {
        let ctx = context();
        let mut editor = Editor {
            minimap_visible: true,
            ..Default::default()
        };
        let (id, initial) = frame_minimap(&ctx, &mut editor);
        let full_pixels = initial.unwrap().pixels.len();
        frame_minimap(&ctx, &mut editor);
        let output = frame_sized(&ctx, &mut editor, vec![], Vec2::new(1000.0, 700.0));
        let start = canvas_cell_center(&output, 10, 10);
        frame_minimap_with_events(
            &ctx,
            &mut editor,
            vec![
                egui::Event::PointerMoved(start),
                pointer_button(start, true),
            ],
        );
        let started = std::time::Instant::now();
        let mut uploaded_pixels = 0;
        for x in 11..31 {
            let position = start + Vec2::new((x - 10) as f32 * glyph::CELL_SIZE.x, 0.0);
            let (texture, patch) = frame_minimap_with_events(
                &ctx,
                &mut editor,
                vec![egui::Event::PointerMoved(position)],
            );
            assert_eq!(texture, id);
            assert_eq!(editor.doc.canvas.get(x, 10), Some(&editor.brush));
            let patch = patch.expect("drawing updates the minimap immediately");
            assert!(patch.pixels.iter().any(|pixel| *pixel != Color32::BLACK));
            uploaded_pixels += patch.pixels.len();
        }
        eprintln!("20 drawing frames with minimap: {:?}", started.elapsed());
        assert!(
            uploaded_pixels < full_pixels,
            "single-cell edits must not rerender and upload the entire minimap: {uploaded_pixels} pixels"
        );
        let end = start + Vec2::new(20.0 * glyph::CELL_SIZE.x, 0.0);
        assert!(
            frame_minimap_with_events(&ctx, &mut editor, vec![pointer_button(end, false)])
                .1
                .is_none()
        );
        assert!(editor.dirty);
    }

    #[test]
    fn minimap_updates_for_edits_undo_and_selection_preview_but_not_for_idle_frames() {
        let ctx = context();
        let mut editor = Editor {
            minimap_visible: true,
            ..Default::default()
        };
        editor.doc.canvas = Canvas::new(4, 4).unwrap();
        let (id, image) = frame_minimap(&ctx, &mut editor);
        assert_eq!(image.unwrap()[(0, 0)], Color32::BLACK);
        assert!(frame_minimap(&ctx, &mut editor).1.is_none());

        let red = Cell {
            ch: '█',
            fg: Color::Rgb(255, 0, 0),
            ..Default::default()
        };
        editor.history.paint(&mut editor.doc.canvas, 0, 0, red);
        let (updated_id, image) = frame_minimap(&ctx, &mut editor);
        assert_eq!(id, updated_id);
        assert_eq!(image.unwrap()[(0, 0)], Color32::RED);

        editor.selection = Some(Selection {
            start: (0, 0),
            end: (0, 0),
        });
        editor.begin_selection_drag((0, 0)).unwrap();
        editor.update_selection_drag((2, 2));
        let image = frame_minimap(&ctx, &mut editor).1.unwrap();
        assert_eq!(image[(0, 0)], Color32::BLACK);
        assert_eq!(image[(2 * glyph::WIDTH, 2 * glyph::HEIGHT)], Color32::RED);
        assert_eq!(editor.doc.canvas.get(0, 0), Some(&red));

        editor.selection_drag = None;
        editor.undo();
        assert_eq!(
            frame_minimap(&ctx, &mut editor).1.unwrap()[(0, 0)],
            Color32::BLACK
        );
        editor.redo();
        assert_eq!(
            frame_minimap(&ctx, &mut editor).1.unwrap()[(0, 0)],
            Color32::RED
        );
        editor.new_image();
        assert_eq!(
            frame_minimap(&ctx, &mut editor).1.unwrap()[(0, 0)],
            Color32::BLACK
        );
    }

    #[test]
    fn zoomed_typing_selection_moving_and_pasting_use_character_cell_coordinates() {
        for zoom_percent in [50.0, 149.0, 200.0] {
            let ctx = context();
            let mut editor = Editor {
                zoom_percent,
                ..Default::default()
            };
            editor.doc.canvas = Canvas::new(12, 6).unwrap();
            let background = Color::Rgb(3, 7, 11);
            editor.doc.canvas.clear(Cell {
                bg: background,
                ..Cell::default()
            });
            let size = glyph::CELL_SIZE * (zoom_percent / 100.0);
            let output = frame(&ctx, &mut editor, vec![]);
            let origin = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Rect(shape)
                        if shape.fill == display_color(background)
                            && (shape.rect.width() - size.x).abs() <= 1.0
                            && (shape.rect.height() - size.y).abs() <= 1.0 =>
                    {
                        Some(shape.rect.min)
                    }
                    _ => None,
                })
                .unwrap();
            let point = |x: usize, y: usize| {
                origin + Vec2::new((x as f32 + 0.5) * size.x, (y as f32 + 0.5) * size.y)
            };
            let start = point(1, 0);
            frame(
                &ctx,
                &mut editor,
                vec![
                    egui::Event::PointerMoved(start),
                    pointer_button(start, true),
                ],
            );
            frame(&ctx, &mut editor, vec![pointer_button(start, false)]);
            frame(&ctx, &mut editor, vec![egui::Event::Text("AB".into())]);
            assert_eq!(editor.doc.canvas.get(1, 0).unwrap().ch, 'A');
            assert_eq!(editor.doc.canvas.get(2, 0).unwrap().ch, 'B');
            editor.selecting = true;
            let end = point(2, 1);
            frame(
                &ctx,
                &mut editor,
                vec![
                    egui::Event::PointerMoved(start),
                    pointer_button(start, true),
                ],
            );
            frame(&ctx, &mut editor, vec![egui::Event::PointerMoved(end)]);
            frame(&ctx, &mut editor, vec![pointer_button(end, false)]);
            assert_eq!(editor.selection.unwrap().bounds(), (1, 0, 2, 1));
            let destination = point(4, 2);
            frame(&ctx, &mut editor, vec![pointer_button(end, true)]);
            frame(
                &ctx,
                &mut editor,
                vec![egui::Event::PointerMoved(destination)],
            );
            frame(&ctx, &mut editor, vec![pointer_button(destination, false)]);
            assert_eq!(editor.selection.unwrap().bounds(), (3, 1, 4, 2));
            assert_eq!(editor.doc.canvas.get(3, 1).unwrap().ch, 'A');
            assert_eq!(editor.doc.canvas.get(4, 1).unwrap().ch, 'B');
            assert_eq!(editor.doc.canvas.get(1, 0).unwrap().ch, ' ');
            let text = editor.copy_selection(false).unwrap();
            let destination = point(7, 3);
            frame(
                &ctx,
                &mut editor,
                vec![
                    egui::Event::PointerMoved(destination),
                    pointer_button(destination, true),
                ],
            );
            frame(&ctx, &mut editor, vec![pointer_button(destination, false)]);
            frame(&ctx, &mut editor, vec![egui::Event::Paste(text)]);
            assert_eq!(editor.doc.canvas.get(7, 3).unwrap().ch, 'A');
            assert_eq!(editor.doc.canvas.get(8, 3).unwrap().ch, 'B');
        }
    }

    #[test]
    fn images_over_fifty_rows_have_a_visible_scrollbar_at_actual_size() {
        for height in [50, 51] {
            let ctx = context();
            ctx.global_style_mut(|style| style.animation_time = 0.0);
            let mut editor = Editor::default();
            editor.doc.canvas = Canvas::new(80, height).unwrap();
            editor.doc.canvas.set(
                0,
                height - 1,
                Cell {
                    ch: 'Z',
                    fg: Color::Rgb(123, 231, 77),
                    ..Cell::default()
                },
            );
            let mut render = |events| {
                crate::run_test_frame(
                    &ctx,
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            Pos2::ZERO,
                            Vec2::new(1120.0, 1400.0),
                        )),
                        focused: true,
                        events,
                        ..Default::default()
                    },
                    |ctx| ctx.root_ui(|ui| editor.ui(ui)),
                )
            };
            render(vec![]);
            let output = render(vec![]);
            let scrollbar = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Rect(shape)
                        if shape.rect.width() == 6.0
                            && shape.rect.height() >= 50.0 * glyph::CELL_SIZE.y - 10.0
                            && shape.fill.is_opaque() =>
                    {
                        Some(shape.rect)
                    }
                    _ => None,
                });
            assert_eq!(scrollbar.is_some(), height > 50);
            if let Some(scrollbar) = scrollbar {
                assert_eq!(scrollbar.height(), 50.0 * glyph::CELL_SIZE.y);
                render(vec![
                    egui::Event::PointerMoved(Pos2::new(500.0, 500.0)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: Vec2::new(0.0, -120.0),
                        phase: egui::TouchPhase::Move,
                        source: egui::MouseWheelSource::Unknown,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]);
                let output = render(vec![]);
                assert!(output.shapes.iter().any(|clipped| {
                    matches!(&clipped.shape, egui::Shape::Mesh(mesh)
                        if mesh.vertices.iter().all(|v| v.color == display_color(Color::Rgb(123, 231, 77)))
                            && clipped.clip_rect.contains_rect(mesh.calc_bounds()))
                }), "scrolling reveals the final row");
            }
        }
    }

    #[test]
    fn used_colors_list_only_colors_picked_this_session() {
        let ctx = context();
        let mut editor = Editor::default();
        editor.doc.canvas.set(
            0,
            0,
            Cell {
                fg: Color::Rgb(8, 19, 29),
                ..Cell::default()
            },
        );
        frame(&ctx, &mut editor, vec![]);
        assert!(
            editor.picked_colors.is_empty(),
            "artwork colors are not listed"
        );
        let output = frame(&ctx, &mut editor, vec![]);
        assert!(output.shapes.iter().any(|clipped| {
            matches!(&clipped.shape, egui::Shape::Text(shape)
                if shape.galley.text() == "Colors you pick appear here.")
        }));
        let swatch = |output: &egui::FullOutput, color: Color| {
            output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Rect(shape)
                        if shape.rect.size() == Vec2::new(24.0, 20.0)
                            && shape.fill == display_color(color) =>
                    {
                        Some(shape.rect.center())
                    }
                    _ => None,
                })
                .expect("ANSI swatch is visible")
        };
        // Choosing the color that is already the foreground still counts as a pick.
        let current = editor.brush.fg;
        let pos = swatch(&output, current);
        frame(
            &ctx,
            &mut editor,
            vec![
                egui::Event::PointerMoved(pos),
                color_click(pos, true, false),
            ],
        );
        frame(&ctx, &mut editor, vec![color_click(pos, false, false)]);
        assert_eq!(editor.brush.fg, current);
        assert_eq!(editor.picked_colors, [current]);
        editor.picked_colors.clear();
        let output = frame(&ctx, &mut editor, vec![]);
        let pos = swatch(&output, Color::Ansi(12));
        for _ in 0..2 {
            frame(
                &ctx,
                &mut editor,
                vec![egui::Event::PointerMoved(pos), color_click(pos, true, true)],
            );
            frame(&ctx, &mut editor, vec![color_click(pos, false, true)]);
        }
        assert_eq!(editor.picked_colors, [Color::Ansi(12)], "listed once");

        // RGB picker drags are remembered once, when the picker closes.
        editor.rgb_open = true;
        for green in 1..=3 {
            editor.brush.fg = Color::Rgb(0, green, 0);
            editor.remember_picked_colors();
        }
        editor.pending_gradient_end = Some(Color::Indexed(201));
        editor.remember_picked_colors();
        assert_eq!(editor.picked_colors, [Color::Ansi(12)]);
        editor.rgb_open = false;
        editor.remember_picked_colors();
        assert_eq!(
            editor.picked_colors,
            [Color::Ansi(12), Color::Indexed(201), Color::Rgb(0, 3, 0)]
        );
    }

    #[test]
    fn eyedropper_picks_displayed_cell_colors_without_editing_artwork() {
        let ctx = context();
        let mut editor = Editor::default();
        editor.doc.canvas.set(
            1,
            0,
            Cell {
                ch: 'A',
                fg: Color::Ansi(1),
                bg: Color::Rgb(8, 19, 29),
                attrs: ansi_core::canvas::Attributes {
                    bold: true,
                    ..Default::default()
                },
            },
        );
        let original = editor.doc.canvas.clone();
        let output = frame(&ctx, &mut editor, vec![]);
        let tool = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) if shape.galley.text() == ICON_COLORIZE.codepoint => {
                    Some(shape.galley.rect.translate(shape.pos.to_vec2()).center())
                }
                _ => None,
            })
            .expect("eyedropper tool is visible");
        frame(
            &ctx,
            &mut editor,
            vec![egui::Event::PointerMoved(tool), pointer_button(tool, true)],
        );
        frame(&ctx, &mut editor, vec![pointer_button(tool, false)]);
        assert!(editor.eyedropper);
        let brush = editor.brush;
        for background in [false, true] {
            let output = frame(&ctx, &mut editor, vec![]);
            let cell = canvas_cell_center(&output, 1, 0);
            frame(
                &ctx,
                &mut editor,
                vec![
                    egui::Event::PointerMoved(cell),
                    color_click(cell, true, background),
                ],
            );
            frame(
                &ctx,
                &mut editor,
                vec![color_click(cell, false, background)],
            );
        }
        assert_eq!(
            editor.brush.fg,
            Color::Ansi(9),
            "bold brightens the picked color"
        );
        assert_eq!(editor.brush.bg, Color::Rgb(8, 19, 29));
        assert_eq!(
            (editor.brush.ch, editor.brush.attrs),
            (brush.ch, brush.attrs)
        );
        assert_eq!(
            editor.picked_colors,
            [Color::Ansi(9), Color::Rgb(8, 19, 29)]
        );
        assert_eq!(editor.doc.canvas, original);
        assert!(!editor.dirty);
    }

    #[test]
    fn used_swatches_select_foreground_and_background_without_editing_artwork() {
        let ctx = context();
        ctx.global_style_mut(|style| style.animation_time = 0.0);
        let mut editor = Editor::default();
        let cell = Cell {
            fg: Color::Rgb(8, 19, 29),
            bg: Color::Indexed(201),
            ..Cell::default()
        };
        editor.picked_colors = vec![cell.fg, cell.bg];
        let original = editor.doc.canvas.clone();
        frame(&ctx, &mut editor, vec![]);
        for (background, used) in [(false, cell.fg), (true, cell.bg)] {
            frame(
                &ctx,
                &mut editor,
                vec![egui::Event::PointerMoved(Pos2::new(790.0, 590.0))],
            );
            let output = frame(&ctx, &mut editor, vec![]);
            let swatches: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Rect(shape)
                        if shape.rect.size() == Vec2::new(24.0, 20.0)
                            && shape.fill == display_color(used) =>
                    {
                        Some(shape.rect)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(swatches.len(), 1, "each used color has one shared swatch");
            let before = editor.brush;
            let pos = swatches[0].center();
            frame(
                &ctx,
                &mut editor,
                vec![
                    egui::Event::PointerMoved(pos),
                    color_click(pos, true, background),
                ],
            );
            frame(&ctx, &mut editor, vec![color_click(pos, false, background)]);
            if background {
                assert_eq!(editor.brush.bg, used);
                assert_eq!(editor.brush.fg, before.fg);
            } else {
                assert_eq!(editor.brush.fg, used);
                assert_eq!(editor.brush.bg, before.bg);
            }
            assert_eq!(editor.brush.ch, before.ch);
            assert_eq!(editor.brush.attrs, before.attrs);
            assert_eq!(editor.doc.canvas, original);
            assert!(!editor.dirty);
        }
    }

    #[test]
    fn large_used_palettes_are_virtualized_and_scroll_to_the_last_color() {
        let ctx = context();
        ctx.global_style_mut(|style| style.animation_time = 0.0);
        let mut editor = Editor {
            picked_colors: (0..64).map(|n| Color::Rgb(176, n, 20)).collect(),
            ..Default::default()
        };
        frame(&ctx, &mut editor, vec![]);
        let output = frame(&ctx, &mut editor, vec![]);
        let visible: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if shape.rect.size() == Vec2::new(24.0, 20.0)
                        && shape.fill.r() == 176
                        && shape.fill.b() == 20 =>
                {
                    Some((clipped.clip_rect, shape.fill))
                }
                _ => None,
            })
            .collect();
        assert!(!visible.is_empty() && visible.len() < 64);
        let clip = visible[0].0;
        frame(
            &ctx,
            &mut editor,
            vec![
                egui::Event::PointerMoved(clip.center()),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(0.0, -1000.0),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Unknown,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let output = frame(&ctx, &mut editor, vec![]);
        assert!(
            output.shapes.iter().any(|clipped| {
                matches!(&clipped.shape, egui::Shape::Rect(shape)
                if shape.fill == display_color(Color::Rgb(176, 63, 20))
                    && shape.rect.size() == Vec2::new(24.0, 20.0)
                    && clipped.clip_rect.contains_rect(shape.rect))
            }),
            "the last used color can be scrolled fully into view"
        );
    }

    #[test]
    fn current_color_is_a_swatch_for_ansi_indexed_and_rgb() {
        for color in [Color::Ansi(1), Color::Indexed(201), Color::Rgb(8, 19, 29)] {
            let ctx = context();
            let mut editor = Editor::default();
            editor.brush.fg = color;
            editor.brush.bg = Color::Rgb(71, 23, 11);
            let output = frame(&ctx, &mut editor, vec![]);
            for expected in [color, editor.brush.bg] {
                assert!(output.shapes.iter().any(|clipped| {
                    matches!(&clipped.shape, egui::Shape::Rect(shape)
                        if shape.rect.size() == Vec2::new(40.0, 28.0)
                            && shape.fill == display_color(expected))
                }));
            }
            assert!(!output.shapes.iter().any(|clipped| {
                matches!(&clipped.shape, egui::Shape::Text(shape)
                    if shape.galley.text().starts_with("ANSI ")
                        || shape.galley.text() == "201")
            }));
            assert_eq!(editor.brush.fg, color);
        }
    }

    #[test]
    fn shared_ansi_palette_selects_both_brush_colors_without_editing_artwork() {
        for (background, index) in [(false, 1), (true, 4), (false, 15), (true, 2)] {
            let ctx = context();
            let mut editor = Editor::default();
            let original = editor.brush;
            let canvas = editor.doc.canvas.clone();
            let output = frame(&ctx, &mut editor, vec![]);
            let swatches: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Rect(shape)
                        if shape.rect.size() == Vec2::new(24.0, 20.0)
                            && shape.fill == display_color(Color::Ansi(index)) =>
                    {
                        Some(shape.rect)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                swatches.len(),
                1,
                "ANSI colors are not duplicated by brush component"
            );
            let pos = swatches[0].center();
            frame(
                &ctx,
                &mut editor,
                vec![
                    egui::Event::PointerMoved(pos),
                    color_click(pos, true, background),
                ],
            );
            frame(&ctx, &mut editor, vec![color_click(pos, false, background)]);
            let mut expected = original;
            if background {
                expected.bg = Color::Ansi(index);
            } else {
                expected.fg = Color::Ansi(index);
            }
            assert_eq!(editor.brush, expected);
            assert_eq!(editor.doc.canvas, canvas);
            assert!(!editor.dirty);
        }
    }

    #[test]
    fn rgb_controls_edit_the_mouse_selected_component_without_changing_artwork() {
        for background in [false, true] {
            for trigger in 0..3 {
                let ctx = context();
                ctx.global_style_mut(|style| style.animation_time = 0.0);
                let mut editor = Editor::default();
                editor.brush.fg = Color::Ansi(3);
                editor.brush.bg = Color::Indexed(201);
                let original = editor.brush;
                let canvas = editor.doc.canvas.clone();
                let output = frame(&ctx, &mut editor, vec![]);
                let pos = output
                    .shapes
                    .iter()
                    .find_map(|clipped| match &clipped.shape {
                        egui::Shape::Text(shape)
                            if trigger == 0 && shape.galley.text() == ICON_PALETTE.codepoint =>
                        {
                            Some(shape.galley.rect.translate(shape.pos.to_vec2()).center())
                        }
                        egui::Shape::Rect(shape)
                            if trigger != 0
                                && shape.rect.size() == Vec2::new(40.0, 28.0)
                                && shape.fill
                                    == display_color(if trigger == 1 {
                                        original.fg
                                    } else {
                                        original.bg
                                    }) =>
                        {
                            Some(shape.rect.center())
                        }
                        _ => None,
                    })
                    .expect("RGB control is visible");
                frame(
                    &ctx,
                    &mut editor,
                    vec![
                        egui::Event::PointerMoved(pos),
                        color_click(pos, true, background),
                    ],
                );
                frame(&ctx, &mut editor, vec![color_click(pos, false, background)]);
                let output = frame(&ctx, &mut editor, vec![]);
                assert_eq!(
                    editor.brush, original,
                    "opening RGB does not change the color or its encoding"
                );
                let title = if background {
                    "Background RGB color"
                } else {
                    "Foreground RGB color"
                };
                assert!(output.shapes.iter().any(|clipped| {
                    matches!(&clipped.shape, egui::Shape::Text(shape) if shape.galley.text() == title)
                }));
                let pos = rgb_picker_area(&output).center();
                frame(
                    &ctx,
                    &mut editor,
                    vec![egui::Event::PointerMoved(pos), pointer_button(pos, true)],
                );
                frame(&ctx, &mut editor, vec![pointer_button(pos, false)]);
                if background {
                    assert!(matches!(editor.brush.bg, Color::Rgb(..)));
                    assert_ne!(editor.brush.bg, original.bg);
                    assert_eq!(editor.brush.fg, original.fg);
                } else {
                    assert!(matches!(editor.brush.fg, Color::Rgb(..)));
                    assert_ne!(editor.brush.fg, original.fg);
                    assert_eq!(editor.brush.bg, original.bg);
                }
                assert_eq!(editor.brush.ch, original.ch);
                assert_eq!(editor.brush.attrs, original.attrs);
                assert_eq!(editor.doc.canvas, canvas);
                assert!(!editor.dirty);
            }
        }
    }

    #[test]
    fn palette_colors_recolor_the_active_selection_as_one_undo_step() {
        for (background, already_current) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let ctx = context();
            let mut editor = Editor::default();
            if already_current {
                if background {
                    editor.brush.bg = Color::Ansi(12);
                } else {
                    editor.brush.fg = Color::Ansi(12);
                }
            }
            let cell = Cell {
                ch: 'A',
                fg: Color::Ansi(3),
                bg: Color::Indexed(201),
                attrs: ansi_core::canvas::Attributes {
                    bold: true,
                    ..Default::default()
                },
            };
            editor.doc.canvas.clear(cell);
            editor.selection = Some(Selection {
                start: (3, 2),
                end: (1, 1),
            });
            let output = frame(&ctx, &mut editor, vec![]);
            let pos = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Rect(shape)
                        if shape.rect.size() == Vec2::new(24.0, 20.0)
                            && shape.fill == display_color(Color::Ansi(12)) =>
                    {
                        Some(shape.rect.center())
                    }
                    _ => None,
                })
                .expect("ANSI swatch is visible");
            frame(
                &ctx,
                &mut editor,
                vec![
                    egui::Event::PointerMoved(pos),
                    color_click(pos, true, background),
                ],
            );
            frame(&ctx, &mut editor, vec![color_click(pos, false, background)]);
            let mut recolored = cell;
            if background {
                recolored.bg = Color::Ansi(12);
            } else {
                recolored.fg = Color::Ansi(12);
            }
            for y in 0..editor.doc.canvas.height() {
                for x in 0..editor.doc.canvas.width() {
                    let inside = (1..=3).contains(&x) && (1..=2).contains(&y);
                    let expected = if inside { recolored } else { cell };
                    assert_eq!(*editor.doc.canvas.get(x, y).unwrap(), expected);
                }
            }
            assert!(editor.dirty);
            editor.undo();
            assert!(editor.doc.canvas.get(2, 2).unwrap() == &cell);
            assert!(!editor.history.undo(&mut editor.doc.canvas));
        }
    }

    #[test]
    fn rgb_picker_drag_recolors_the_selection_as_one_undo_step() {
        // (opened with the modifier, dragged with the modifier)
        for (open_modified, drag_modified) in [(false, false), (true, false), (false, true)] {
            let gradient = open_modified || drag_modified;
            let ctx = context();
            ctx.global_style_mut(|style| style.animation_time = 0.0);
            let mut editor = Editor {
                selection: Some(Selection {
                    start: (0, 0),
                    end: (1, 0),
                }),
                ..Default::default()
            };
            let brush = editor.brush;
            let original = editor.doc.canvas.clone();
            let output = frame(&ctx, &mut editor, vec![]);
            let pos = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(shape) if shape.galley.text() == ICON_PALETTE.codepoint => {
                        Some(shape.galley.rect.translate(shape.pos.to_vec2()).center())
                    }
                    _ => None,
                })
                .expect("RGB control is visible");
            let output = if open_modified {
                frame_with_command(
                    &ctx,
                    &mut editor,
                    vec![
                        egui::Event::PointerMoved(pos),
                        modified_color_click(pos, true, true),
                    ],
                );
                frame_with_command(
                    &ctx,
                    &mut editor,
                    vec![modified_color_click(pos, false, true)],
                );
                frame(&ctx, &mut editor, vec![])
            } else {
                frame(
                    &ctx,
                    &mut editor,
                    vec![egui::Event::PointerMoved(pos), color_click(pos, true, true)],
                );
                frame(&ctx, &mut editor, vec![color_click(pos, false, true)]);
                frame(&ctx, &mut editor, vec![])
            };
            let title = if open_modified {
                "Background gradient end RGB color"
            } else {
                "Background RGB color"
            };
            assert!(output.shapes.iter().any(|clipped| {
                matches!(&clipped.shape, egui::Shape::Text(shape) if shape.galley.text() == title)
            }));
            let area = rgb_picker_area(&output);
            let start = area.center();
            let drag = |ctx: &egui::Context, editor: &mut Editor, events: Vec<egui::Event>| {
                if drag_modified {
                    frame_with_command(ctx, editor, events)
                } else {
                    frame(ctx, editor, events)
                }
            };
            drag(
                &ctx,
                &mut editor,
                vec![
                    egui::Event::PointerMoved(start),
                    pointer_button(start, true),
                ],
            );
            let first = editor.doc.canvas.get(1, 0).unwrap().bg;
            for step in 1..=3 {
                let point = start + Vec2::new(20.0 * step as f32, -20.0 * step as f32);
                drag(&ctx, &mut editor, vec![egui::Event::PointerMoved(point)]);
            }
            let end = start + Vec2::new(60.0, -60.0);
            drag(&ctx, &mut editor, vec![pointer_button(end, false)]);
            let last = editor.doc.canvas.get(1, 0).unwrap().bg;
            assert!(matches!(last, Color::Rgb(..)));
            assert_ne!(last, first, "the drag changed the color repeatedly");
            if gradient {
                assert_eq!(editor.brush, brush, "gradient picks keep the brush");
                assert_eq!(
                    editor.doc.canvas.get(0, 0).unwrap().bg,
                    Color::Rgb(0, 0, 0),
                    "the gradient starts at the brush color, as RGB"
                );
            } else {
                assert_eq!(last, editor.brush.bg);
            }
            assert_eq!(editor.doc.canvas.get(2, 0), original.get(2, 0));
            editor.undo();
            assert_eq!(editor.doc.canvas, original);
            assert!(!editor.history.undo(&mut editor.doc.canvas));
        }
    }

    fn modified_color_click(pos: Pos2, pressed: bool, background: bool) -> egui::Event {
        let egui::Event::PointerButton { button, .. } = color_click(pos, pressed, background)
        else {
            unreachable!()
        };
        egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::COMMAND,
        }
    }

    fn frame_with_command(
        ctx: &egui::Context,
        editor: &mut Editor,
        mut events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        events.insert(0, egui::Event::ModifiersChanged(egui::Modifiers::COMMAND));
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
            focused: true,
            events,
            ..Default::default()
        };
        crate::run_test_frame(ctx, input, |ctx| ctx.root_ui(|ui| editor.ui(ui)))
    }

    #[test]
    fn command_click_fades_the_selection_from_the_brush_color_along_its_drag() {
        for background in [false, true] {
            let ctx = context();
            let mut editor = Editor::default();
            editor.brush.fg = Color::Ansi(0);
            editor.brush.bg = Color::Ansi(0);
            let brush = editor.brush;
            editor.selection = Some(Selection {
                start: (2, 1),
                end: (0, 0),
            });
            let original = editor.doc.canvas.clone();
            let output = frame(&ctx, &mut editor, vec![]);
            let pos = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Rect(shape)
                        if shape.rect.size() == Vec2::new(24.0, 20.0)
                            && shape.fill == display_color(Color::Ansi(15)) =>
                    {
                        Some(shape.rect.center())
                    }
                    _ => None,
                })
                .expect("ANSI swatch is visible");
            frame_with_command(
                &ctx,
                &mut editor,
                vec![
                    egui::Event::PointerMoved(pos),
                    modified_color_click(pos, true, background),
                ],
            );
            frame_with_command(
                &ctx,
                &mut editor,
                vec![modified_color_click(pos, false, background)],
            );
            assert_eq!(editor.brush, brush, "gradient picks keep the brush");
            let component = |cell: &Cell| if background { cell.bg } else { cell.fg };
            for y in 0..2 {
                for x in 0..3 {
                    let expected = gradient_at(
                        Color::Ansi(0),
                        Color::Ansi(15),
                        (2, 1),
                        (0, 0),
                        (x, y),
                        GradientPalette::Ansi,
                    );
                    let cell = editor.doc.canvas.get(x, y).unwrap();
                    assert_eq!(component(cell), expected);
                    assert!(
                        matches!(component(cell), Color::Ansi(_)),
                        "16-color endpoints keep a 16-color gradient"
                    );
                }
            }
            let canvas = &editor.doc.canvas;
            assert_eq!(component(canvas.get(2, 1).unwrap()), Color::Ansi(0));
            assert_eq!(component(canvas.get(0, 0).unwrap()), Color::Ansi(15));
            assert_ne!(component(canvas.get(1, 0).unwrap()), Color::Ansi(0));
            assert_eq!(canvas.get(3, 0), original.get(3, 0));
            assert_eq!(canvas.get(0, 2), original.get(0, 2));
            assert!(editor.dirty);
            editor.undo();
            assert_eq!(editor.doc.canvas, original);
            assert!(!editor.history.undo(&mut editor.doc.canvas));
        }
    }

    #[test]
    fn command_click_without_a_selection_explains_and_keeps_the_brush() {
        let ctx = context();
        let mut editor = Editor::default();
        let brush = editor.brush;
        let output = frame(&ctx, &mut editor, vec![]);
        let pos = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if shape.rect.size() == Vec2::new(24.0, 20.0)
                        && shape.fill == display_color(Color::Ansi(12)) =>
                {
                    Some(shape.rect.center())
                }
                _ => None,
            })
            .expect("ANSI swatch is visible");
        frame_with_command(
            &ctx,
            &mut editor,
            vec![
                egui::Event::PointerMoved(pos),
                modified_color_click(pos, true, false),
            ],
        );
        frame_with_command(
            &ctx,
            &mut editor,
            vec![modified_color_click(pos, false, false)],
        );
        assert_eq!(editor.brush, brush);
        assert!(editor.message.unwrap().contains("Select an area first"));
        assert!(!editor.dirty);
    }

    #[test]
    fn gradient_direction_controls_redraw_the_last_gradient_in_one_undo_step() {
        let ctx = context();
        let mut editor = Editor::default();
        let original = editor.doc.canvas.clone();
        let (black, red) = (Color::Rgb(0, 0, 0), Color::Rgb(255, 0, 0));
        editor.brush.bg = black;
        editor.selection = Some(Selection {
            start: (0, 0),
            end: (3, 2),
        });
        let pick = GradientPick {
            background: true,
            end: red,
        };
        assert!(editor.apply_gradient(pick, None).unwrap());
        let bg = |editor: &Editor, x, y| editor.doc.canvas.get(x, y).unwrap().bg;
        assert_eq!((bg(&editor, 0, 0), bg(&editor, 3, 2)), (black, red));
        let click = |editor: &mut Editor, icon: MaterialIcon| {
            let output = frame(&ctx, editor, vec![]);
            let pos = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(shape) if shape.galley.text() == icon.codepoint => {
                        Some(shape.galley.rect.translate(shape.pos.to_vec2()).center())
                    }
                    _ => None,
                })
                .expect("gradient direction control is visible");
            frame(
                &ctx,
                editor,
                vec![
                    egui::Event::PointerMoved(pos),
                    color_click(pos, true, false),
                ],
            );
            frame(&ctx, editor, vec![color_click(pos, false, false)]);
        };
        click(&mut editor, ICON_EAST);
        assert_eq!(editor.gradient_direction, GradientDirection::LeftToRight);
        for y in 0..3 {
            assert_eq!((bg(&editor, 0, y), bg(&editor, 3, y)), (black, red));
        }
        click(&mut editor, ICON_SOUTH);
        click(&mut editor, ICON_SYNC_ALT);
        assert!(editor.gradient_reversed);
        for x in 0..4 {
            assert_eq!((bg(&editor, x, 0), bg(&editor, x, 2)), (red, black));
        }
        assert_eq!(editor.brush.bg, black, "direction changes keep the brush");
        editor.undo();
        assert_eq!(editor.doc.canvas, original, "one undo removes the gradient");
        assert!(!editor.history.undo(&mut editor.doc.canvas));

        // Once the selection changes, directions only apply to later gradients.
        editor.selection = Some(Selection {
            start: (5, 5),
            end: (6, 6),
        });
        assert!(!editor
            .set_gradient_direction(GradientDirection::Drag, false)
            .unwrap());
        assert_eq!(editor.doc.canvas, original);
    }

    #[test]
    fn indexed_palette_opens_without_changing_color_and_selects_all_color_ranges() {
        for (background, index) in [(false, 201), (true, 16), (false, 255), (true, 0)] {
            let ctx = context();
            ctx.global_style_mut(|style| style.animation_time = 0.0);
            let mut editor = Editor::default();
            editor.brush.fg = Color::Ansi(3);
            editor.brush.bg = Color::Rgb(8, 19, 29);
            let original = editor.brush;
            let output = frame(&ctx, &mut editor, vec![]);
            let icons: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(shape) if shape.galley.text() == ICON_GRID_VIEW.codepoint => {
                        Some(shape.galley.rect.translate(shape.pos.to_vec2()).center())
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(icons.len(), 1, "there is one shared indexed palette");
            let pos = icons[0];
            frame(
                &ctx,
                &mut editor,
                vec![
                    egui::Event::PointerMoved(pos),
                    color_click(pos, true, background),
                ],
            );
            frame(&ctx, &mut editor, vec![color_click(pos, false, background)]);
            frame(
                &ctx,
                &mut editor,
                vec![egui::Event::PointerMoved(Pos2::new(790.0, 590.0))],
            );
            let output = frame(&ctx, &mut editor, vec![]);
            assert_eq!(editor.brush, original);
            let title = "256-color palette";
            let clip = output.shapes.iter().find_map(|clipped| {
                matches!(&clipped.shape, egui::Shape::Text(shape) if shape.galley.text() == title)
                    .then_some(clipped.clip_rect)
            }).expect("indexed palette is open");
            let swatches: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Rect(shape)
                        if clipped.clip_rect == clip
                            && shape.rect.size() == Vec2::new(24.0, 20.0)
                            && shape.fill.is_opaque() =>
                    {
                        assert!(
                            clip.contains_rect(shape.rect),
                            "every palette color is visible"
                        );
                        Some((shape.rect, shape.fill))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(swatches.len(), 256);
            for n in [0, 15, 16, 231, 232, 255] {
                assert_eq!(swatches[n].1, display_color(Color::Indexed(n as u8)));
            }
            let pos = swatches[usize::from(index)].0.center();
            frame(
                &ctx,
                &mut editor,
                vec![
                    egui::Event::PointerMoved(pos),
                    color_click(pos, true, background),
                ],
            );
            frame(&ctx, &mut editor, vec![color_click(pos, false, background)]);
            let output = frame(&ctx, &mut editor, vec![]);
            if background {
                assert_eq!(editor.brush.bg, Color::Indexed(index));
                assert_eq!(editor.brush.fg, original.fg);
            } else {
                assert_eq!(editor.brush.fg, Color::Indexed(index));
                assert_eq!(editor.brush.bg, original.bg);
            }
            assert!(!output.shapes.iter().any(|clipped| {
                matches!(&clipped.shape, egui::Shape::Text(shape) if shape.galley.text() == title)
            }));
            assert!(!editor.dirty);
        }
    }

    fn tdf_test_font_bytes() -> Vec<u8> {
        let mut bytes = vec![0; 233];
        bytes[..20].copy_from_slice(b"\x13TheDraw FONTS file\x1a");
        bytes[20..24].copy_from_slice(b"\x55\xaa\x00\xff");
        bytes[24] = 4;
        bytes[25..29].copy_from_slice(b"Test");
        bytes[41] = 1;
        bytes[42] = 2;
        bytes[45..233].fill(255);
        bytes[109..111].fill(0);
        let glyph = [2, 2, 219, 32, 13, 219, 255, 0];
        bytes[43..45].copy_from_slice(&(glyph.len() as u16).to_le_bytes());
        bytes.extend(glyph);
        bytes
    }

    fn tdf_test_font() -> font_library::ImportedFont {
        font_library::ImportedFont {
            font: tdf::load(&tdf_test_font_bytes()).unwrap().remove(0),
            source: PathBuf::from("test.tdf"),
        }
    }

    #[test]
    fn tdf_imports_persist_without_sources_and_do_not_overwrite_or_duplicate() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("original");
        let other = root.path().join("other");
        std::fs::create_dir(&original).unwrap();
        std::fs::create_dir(&other).unwrap();
        let source = original.join("example.tdf");
        let collision = other.join("example.tdf");
        let bytes = tdf_test_font_bytes();
        let mut other_bytes = bytes.clone();
        other_bytes[25..29].copy_from_slice(b"Else");
        std::fs::write(&source, &bytes).unwrap();
        std::fs::write(&collision, &other_bytes).unwrap();
        let directory = root.path().join("fonts");
        let mut library = font_library::FontLibrary::new(Some(directory.clone()));

        assert_eq!(library.import(&source).unwrap().added, 1);
        let first_copy = library.fonts[0].source.clone();
        assert_eq!(library.import(&collision).unwrap().added, 1);
        let second_copy = library.fonts[1].source.clone();
        assert_ne!(first_copy, second_copy);
        assert_eq!(first_copy.parent(), Some(directory.as_path()));
        assert_eq!(second_copy.parent(), Some(directory.as_path()));
        let repeated = library.import(&source).unwrap();
        assert_eq!((repeated.first, repeated.added), (0, 0));
        assert_eq!(std::fs::read(&first_copy).unwrap(), bytes);
        assert_eq!(std::fs::read(&second_copy).unwrap(), other_bytes);
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 2);
        std::fs::remove_file(&source).unwrap();
        std::fs::remove_file(&collision).unwrap();

        let storage = TestStorage::default();
        let restored = Editor::with_font_directory(Some(&storage), Some(directory));
        assert!(restored.message.is_none());
        assert_eq!(restored.tdf_library.fonts.len(), 2);
        for entry in &restored.tdf_library.fonts {
            let rendered = entry.font.render("A", restored.brush, 0).unwrap();
            assert_eq!((rendered.width, rendered.height), (2, 2));
            assert!(entry.source == first_copy || entry.source == second_copy);
        }
    }

    #[test]
    fn tdf_import_and_reload_failures_are_reported_without_losing_valid_fonts() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("fonts");
        let source = root.path().join("valid.tdf");
        let invalid = root.path().join("invalid.tdf");
        std::fs::write(&source, tdf_test_font_bytes()).unwrap();
        std::fs::write(&invalid, b"not a font").unwrap();
        let mut library = font_library::FontLibrary::new(Some(directory.clone()));
        assert!(library
            .import(&invalid)
            .unwrap_err()
            .contains("Not a TheDraw"));
        assert!(library.fonts.is_empty());
        assert!(!directory.exists());
        library.import(&source).unwrap();
        std::fs::write(directory.join("broken.TDF"), b"not a font").unwrap();
        let storage = TestStorage::default();
        let restored = Editor::with_font_directory(Some(&storage), Some(directory));
        assert_eq!(restored.tdf_library.fonts.len(), 1);
        assert!(restored.message.unwrap().contains("broken.TDF"));

        let blocked = root.path().join("blocked");
        std::fs::write(&blocked, b"not a directory").unwrap();
        let mut unavailable = font_library::FontLibrary::new(Some(blocked));
        assert!(unavailable
            .import(&source)
            .unwrap_err()
            .contains("Could not create"));
        assert!(unavailable.fonts.is_empty());
        assert!(!unavailable.load().is_empty());
    }

    #[test]
    fn tdf_import_limit_does_not_publish_an_unusable_copy() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("new.tdf");
        std::fs::write(&source, tdf_test_font_bytes()).unwrap();
        let directory = root.path().join("fonts");
        let mut library = font_library::FontLibrary::new(Some(directory.clone()));
        library.fonts = (0..tdf::MAX_FONTS).map(|_| tdf_test_font()).collect();
        assert!(library.import(&source).unwrap_err().contains("exceed 4096"));
        assert_eq!(std::fs::read_dir(directory).unwrap().count(), 0);
    }

    #[test]
    fn tdf_saved_font_list_filters_and_previews_before_undoable_placement() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("collection.tdf");
        let mut bytes = tdf_test_font_bytes();
        let mut second = bytes.clone();
        second[24] = 6;
        second[25..41].fill(0);
        second[25..31].copy_from_slice(b"Second");
        second[42] = 4;
        second[233] = 3;
        bytes.extend_from_slice(&second[20..]);
        std::fs::write(&source, bytes).unwrap();
        let directory = root.path().join("fonts");
        let mut library = font_library::FontLibrary::new(Some(directory.clone()));
        assert_eq!(library.import(&source).unwrap().added, 2);
        std::fs::remove_file(source).unwrap();
        let storage = TestStorage::default();
        let mut editor = Editor::with_font_directory(Some(&storage), Some(directory));
        editor.doc.canvas = Canvas::new(10, 5).unwrap();
        editor.cursor = (2, 1);
        editor.tdf_text = "A".into();
        editor.tdf_open = true;
        let canvas = editor.doc.canvas.clone();
        let brush = editor.brush;
        let ctx = context();
        ctx.global_style_mut(|style| style.animation_time = 0.0);
        let has_text = |output: &egui::FullOutput, text: &str| {
            output.shapes.iter().any(|clipped| {
                matches!(&clipped.shape, egui::Shape::Text(shape) if shape.galley.text() == text)
            })
        };
        let preview_rect = |output: &egui::FullOutput, width: usize| {
            output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Rect(shape)
                        if shape.rect.size()
                            == Vec2::new(
                                width as f32 * glyph::CELL_SIZE.x,
                                2.0 * glyph::CELL_SIZE.y,
                            )
                            && shape.fill == Color32::BLACK =>
                    {
                        Some(shape.rect)
                    }
                    _ => None,
                })
                .expect("The rendered font preview must be visible")
        };
        let output = frame(&ctx, &mut editor, vec![]);
        assert!(has_text(&output, "Test (Block)"));
        assert!(has_text(&output, "Second (Block)"));
        assert!(has_text(&output, "2 x 2 cells at column 3, row 2"));
        preview_rect(&output, 2);
        let output = click_text(&ctx, &mut editor, "Second (Block)");
        assert_eq!(editor.tdf_spacing, 3);
        assert!(has_text(&output, "3 x 2 cells at column 3, row 2"));
        let preview = preview_rect(&output, 3);
        assert!(output.shapes.iter().any(|clipped| {
            matches!(&clipped.shape, egui::Shape::Mesh(mesh)
                if !mesh.vertices.is_empty() && mesh.vertices.iter().all(|vertex|
                    preview.expand(1.0).contains(vertex.pos) && vertex.color == display_color(brush.fg)))
        }));
        assert_eq!(editor.doc.canvas, canvas);
        assert_eq!(editor.brush, brush);
        assert!(!editor.dirty);
        editor.tdf_filter = "  sEcOnD  ".into();
        let output = frame(&ctx, &mut editor, vec![]);
        assert!(has_text(&output, "Second (Block)"));
        assert!(!has_text(&output, "Test (Block)"));
        editor.tdf_filter = "missing".into();
        let output = frame(&ctx, &mut editor, vec![]);
        assert!(has_text(&output, "No matching fonts."));
        assert!(has_text(&output, "3 x 2 cells at column 3, row 2"));
        editor.tdf_filter.clear();
        editor.tdf_text = "AA".into();
        let output = frame(&ctx, &mut editor, vec![]);
        assert!(has_text(&output, "9 x 2 cells at column 3, row 2"));
        assert_eq!(editor.doc.canvas, canvas);
        editor.tdf_text = "A".into();
        click_text(&ctx, &mut editor, "Place text");
        assert_eq!(editor.selection.unwrap().bounds(), (2, 1, 4, 2));
        assert_ne!(editor.doc.canvas, canvas);
        editor.undo();
        assert_eq!(editor.doc.canvas, canvas);
        editor.tdf_text = vec!["A"; 15].join("\n");
        let output = frame(&ctx, &mut editor, vec![]);
        let (button, clip) = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) if shape.galley.text() == "Place text" => Some((
                    shape.galley.rect.translate(shape.pos.to_vec2()),
                    clipped.clip_rect,
                )),
                _ => None,
            })
            .expect("Placement must remain accessible with a tall text preview");
        assert!(clip.contains_rect(button));
        assert!(ctx.content_rect().contains_rect(button));
    }

    #[test]
    fn tdf_preview_placement_preserves_transparency_and_undoes_atomically() {
        let ctx = context();
        let mut editor = Editor::default();
        editor.doc.canvas = Canvas::new(4, 3).unwrap();
        editor.doc.canvas.set(
            3,
            1,
            Cell {
                ch: 'Z',
                ..Cell::default()
            },
        );
        editor.cursor = (2, 1);
        editor.tdf_library.fonts.push(tdf_test_font());
        editor.tdf_text = "A".into();
        editor.tdf_open = true;
        let original = editor.doc.canvas.clone();
        frame(&ctx, &mut editor, vec![]);
        click_text(&ctx, &mut editor, "Place text");
        assert_eq!(editor.doc.canvas.get(2, 1).unwrap().ch, '█');
        assert_eq!(editor.doc.canvas.get(3, 1).unwrap().ch, 'Z');
        assert_eq!(editor.selection.unwrap().bounds(), (2, 1, 3, 2));
        editor.undo();
        assert_eq!(editor.doc.canvas, original);
        editor.redo();
        assert_eq!(editor.doc.canvas.get(2, 2).unwrap().ch, '█');
        let before = editor.doc.canvas.clone();
        editor.cursor = (3, 2);
        let text = editor.tdf_library.fonts[0]
            .font
            .render("A", editor.brush, 0)
            .unwrap();
        assert!(editor.place_tdf_text(&text).is_err());
        assert_eq!(editor.doc.canvas, before);
    }

    #[test]
    fn tdf_destination_click_and_typing_do_not_paint_the_canvas() {
        let ctx = context();
        let mut editor = Editor {
            tdf_open: true,
            ..Default::default()
        };
        let output = frame(&ctx, &mut editor, vec![]);
        let pos = canvas_cell_center(&output, 1, 0);
        frame(
            &ctx,
            &mut editor,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        frame(
            &ctx,
            &mut editor,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(editor.cursor, (1, 0));
        frame(&ctx, &mut editor, vec![egui::Event::Text("A".into())]);
        assert_eq!(editor.doc.canvas.get(1, 0).unwrap().ch, ' ');
        editor.tdf_open = false;
        frame(&ctx, &mut editor, vec![egui::Event::Text("A".into())]);
        assert_eq!(editor.doc.canvas.get(1, 0).unwrap().ch, 'A');
    }

    #[test]
    fn opening_a_requested_file_confirms_unsaved_changes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("my-screen.asc");
        std::fs::write(&path, b"Hi").unwrap();
        let ctx = context();
        let mut editor = Editor {
            dirty: true,
            ..Editor::default()
        };
        editor.request(PendingAction::OpenPath(path.clone()), &ctx);
        assert!(editor.path.is_none());
        click_text(&ctx, &mut editor, "Discard");
        assert_eq!(editor.path.as_deref(), Some(path.as_path()));
        assert_eq!(editor.doc.canvas.get(1, 0).unwrap().ch, 'i');
        assert!(!editor.dirty);
    }

    fn click_text(
        ctx: &egui::Context,
        editor: &mut Editor,
        text: impl Into<String>,
    ) -> egui::FullOutput {
        let text = text.into();
        let output = frame(ctx, editor, vec![]);
        let pos = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) if shape.galley.text() == text.as_str() => {
                    Some(shape.galley.rect.translate(shape.pos.to_vec2()).center())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("UI text is not visible: {text}"));
        frame(
            ctx,
            editor,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        frame(
            ctx,
            editor,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        )
    }

    #[test]
    fn menus_route_selection_clipboard_and_paste_to_canvas() {
        let ctx = context();
        let mut editor = Editor::default();
        editor.doc.canvas = Canvas::new(3, 2).unwrap();
        editor.doc.canvas.set(
            0,
            0,
            Cell {
                ch: 'A',
                ..Cell::default()
            },
        );
        click_text(&ctx, &mut editor, "Selection");
        click_text(&ctx, &mut editor, "Select all");
        assert!(editor.selecting);
        assert_eq!(editor.selection.unwrap().bounds(), (0, 0, 2, 1));
        assert_eq!(editor.cursor, (0, 0));

        click_text(&ctx, &mut editor, "Edit");
        let output = click_text(&ctx, &mut editor, "Cut");
        assert!(output.platform_output.commands.iter().any(
            |command| matches!(command, egui::OutputCommand::CopyText(text) if text == "A  \n   ")
        ));
        assert_eq!(editor.doc.canvas.get(0, 0).unwrap().ch, ' ');
        click_text(&ctx, &mut editor, "Edit");
        click_text(&ctx, &mut editor, "Undo");
        assert_eq!(editor.doc.canvas.get(0, 0).unwrap().ch, 'A');

        click_text(&ctx, &mut editor, "Selection");
        click_text(&ctx, &mut editor, "Select all");
        click_text(&ctx, &mut editor, "Edit");
        let output = click_text(&ctx, &mut editor, "Copy");
        assert!(output.platform_output.commands.iter().any(
            |command| matches!(command, egui::OutputCommand::CopyText(text) if text == "A  \n   ")
        ));
        click_text(&ctx, &mut editor, "Selection");
        click_text(&ctx, &mut editor, "Deselect");
        assert!(editor.selection.is_none());

        click_text(&ctx, &mut editor, "Edit");
        let output = click_text(&ctx, &mut editor, "Paste");
        assert!(output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .iter()
            .any(|command| matches!(command, egui::ViewportCommand::RequestPaste)));
        frame(&ctx, &mut editor, vec![egui::Event::Paste("Z".into())]);
        assert_eq!(editor.doc.canvas.get(0, 0).unwrap().ch, 'Z');
    }

    #[test]
    fn about_menu_shows_logo_version_links_and_blocks_editing_until_closed() {
        let ctx = context();
        let mut editor = Editor::default();
        frame(&ctx, &mut editor, vec![]);
        editor
            .history
            .paint(&mut editor.doc.canvas, 0, 0, editor.brush);
        editor.dirty = true;
        let original = editor.doc.canvas.clone();
        click_text(&ctx, &mut editor, "About");
        frame(&ctx, &mut editor, vec![]);
        let output = frame(&ctx, &mut editor, vec![]);
        assert!(editor.about_open);
        assert!(ctx.memory(|memory| memory.top_modal_layer().is_none()));
        let logo = editor.about_logo.as_ref().unwrap();
        assert_eq!(logo.size(), [256, 256]);
        assert!(ctx
            .tessellate(output.shapes.clone(), output.pixels_per_point)
            .iter()
            .any(|primitive| matches!(&primitive.primitive,
                egui::epaint::Primitive::Mesh(mesh) if mesh.texture_id == logo.id())));
        for label in [
            "About ADraw",
            concat!("Version ", env!("CARGO_PKG_VERSION")),
            "GitHub repository",
            "Check for updates",
        ] {
            assert!(
                output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Text(text) if text.galley.text() == label)),
                "{label}"
            );
        }
        let output = click_text(&ctx, &mut editor, "GitHub repository");
        assert!(output.platform_output.commands.iter().any(
            |command| matches!(command, egui::OutputCommand::OpenUrl(url) if url.url == GITHUB_PAGE)
        ));
        let zoom = editor.zoom_percent;
        let mut undo = key(egui::Key::Z);
        if let egui::Event::Key { modifiers, .. } = &mut undo {
            *modifiers = egui::Modifiers::COMMAND;
        }
        let mut zoom_in = key(egui::Key::Plus);
        if let egui::Event::Key { modifiers, .. } = &mut zoom_in {
            *modifiers = egui::Modifiers::COMMAND;
        }
        frame(
            &ctx,
            &mut editor,
            vec![undo, zoom_in, egui::Event::Text("A".into())],
        );
        assert_eq!(editor.doc.canvas, original);
        assert_eq!(editor.zoom_percent, zoom);
        assert!(editor.dirty);
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.text() == "Close")));
        let output = frame(&ctx, &mut editor, vec![]);
        let title_rect = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "About ADraw" => {
                    Some(text.galley.rect.translate(text.pos.to_vec2()))
                }
                _ => None,
            })
            .unwrap();
        let close_pos = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::LineSegment { points: [a, b], .. }
                    if a.x > title_rect.right()
                        && b.x > a.x
                        && b.y > a.y
                        && title_rect.y_range().contains((a.y + b.y) / 2.0) =>
                {
                    Some(*a + (*b - *a) / 2.0)
                }
                _ => None,
            })
            .expect("standard window title bar must paint its X close control");
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut editor,
                vec![
                    egui::Event::PointerMoved(close_pos),
                    pointer_button(close_pos, pressed),
                ],
            );
        }
        assert!(!editor.about_open);
        editor.undo();
        assert_eq!(*editor.doc.canvas.get(0, 0).unwrap(), Cell::default());
    }

    #[test]
    fn opening_files_replaces_dimensions_and_new_images_restore_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let metadata_path = directory.path().join("metadata.ans");
        let doc = Document {
            canvas: Canvas::new(4, 3).unwrap(),
            sauce: Some(Sauce::default()),
        };
        std::fs::write(&metadata_path, ans_io::save(&doc, Encoding::Cp437).unwrap()).unwrap();
        let plain_path = directory.path().join("plain.ans");
        std::fs::write(&plain_path, b"\x1b[30;1HX").unwrap();
        let mut editor = Editor {
            width: 120,
            height: 70,
            ..Default::default()
        };
        editor
            .history
            .paint(&mut editor.doc.canvas, 0, 0, editor.brush);
        editor.dirty = true;
        editor.open_path(metadata_path.clone());
        assert_eq!((editor.width, editor.height), (4, 3));
        assert_eq!(
            (editor.doc.canvas.width(), editor.doc.canvas.height()),
            (4, 3)
        );
        assert_eq!(editor.path.as_ref(), Some(&metadata_path));
        assert!(!editor.dirty);
        editor.undo();
        assert!(!editor.dirty);

        editor.width = 2;
        editor.height = 2;
        editor.open_path(plain_path.clone());
        assert_eq!((editor.width, editor.height), (80, 30));
        assert_eq!(
            (editor.doc.canvas.width(), editor.doc.canvas.height()),
            (80, 30)
        );
        assert_eq!(editor.doc.canvas.get(0, 29).unwrap().ch, 'X');

        let bad_path = directory.path().join("bad.ans");
        std::fs::write(&bad_path, b"\x1b[").unwrap();
        let original = editor.doc.canvas.clone();
        editor.open_path(bad_path);
        assert!(editor
            .message
            .as_deref()
            .unwrap()
            .contains("incomplete CSI"));
        assert_eq!(editor.doc.canvas, original);
        assert_eq!((editor.width, editor.height), (80, 30));
        assert_eq!(editor.path.as_ref(), Some(&plain_path));
        editor.message = None;

        editor.width = 999;
        editor.height = 999;
        editor.new_image();
        assert_eq!((editor.width, editor.height), (80, 25));
        assert_eq!(
            (editor.doc.canvas.width(), editor.doc.canvas.height()),
            (80, 25)
        );
        assert!(editor.path.is_none());
        assert!(!editor.dirty);
        assert!(editor
            .doc
            .canvas
            .get(0, 0)
            .is_some_and(|cell| *cell == Cell::default()));
    }

    #[test]
    fn menus_preserve_dimensions_resize_history_and_unsaved_protection() {
        let ctx = context();
        let mut editor = Editor::default();
        click_text(&ctx, &mut editor, "File");
        click_text(&ctx, &mut editor, "Encoding");
        click_text(&ctx, &mut editor, "UTF-8");
        assert_eq!(editor.encoding, Encoding::Utf8);
        editor.width = 4;
        editor.height = 3;
        click_text(&ctx, &mut editor, "File");
        click_text(&ctx, &mut editor, "New image");
        assert_eq!(editor.doc.canvas.width(), 80);
        assert_eq!(editor.doc.canvas.height(), 25);
        assert_eq!((editor.width, editor.height), (80, 25));
        editor.width = 6;
        editor.height = 5;
        click_text(&ctx, &mut editor, "Canvas");
        click_text(&ctx, &mut editor, "Resize canvas");
        assert_eq!(editor.doc.canvas.width(), 6);
        assert_eq!(editor.doc.canvas.height(), 5);
        click_text(&ctx, &mut editor, "Edit");
        click_text(&ctx, &mut editor, "Undo");
        assert_eq!(editor.doc.canvas.width(), 80);
        assert_eq!(editor.doc.canvas.height(), 25);
        click_text(&ctx, &mut editor, "Edit");
        click_text(&ctx, &mut editor, "Redo");
        assert_eq!(editor.doc.canvas.width(), 6);
        assert_eq!(editor.doc.canvas.height(), 5);

        click_text(&ctx, &mut editor, "File");
        click_text(&ctx, &mut editor, "New image");
        assert!(matches!(editor.pending, Some(PendingAction::New)));
        click_text(&ctx, &mut editor, "Cancel");
        click_text(&ctx, &mut editor, "File");
        let output = click_text(&ctx, &mut editor, "Exit");
        assert!(matches!(editor.pending, Some(PendingAction::Close)));
        assert!(!output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .iter()
            .any(|command| matches!(command, egui::ViewportCommand::Close)));
    }

    #[test]
    fn image_export_uses_selection_and_native_scale_without_changing_editor_state() {
        let ctx = context();
        let mut editor = Editor::default();
        let cell = Cell {
            ch: '█',
            fg: Color::Rgb(12, 34, 56),
            ..Cell::default()
        };
        editor.history.paint(&mut editor.doc.canvas, 3, 2, cell);
        editor.dirty = true;
        editor.path = Some(PathBuf::from("original.ans"));
        editor.zoom_percent = 400.0;
        editor.selection = Some(Selection {
            start: (4, 3),
            end: (3, 2),
        });
        let original = editor.doc.canvas.clone();
        let mut rendered = None;
        let _ = crate::run_test_frame(&ctx, Default::default(), |ctx| {
            rendered = Some(editor.rendered_image(ctx).unwrap());
        });
        let selected = rendered.unwrap();
        assert_eq!(selected.dimensions(), (18, 32));
        assert_eq!(selected.get_pixel(0, 0).0, [12, 34, 56]);
        assert_eq!(selected.get_pixel(9, 16).0, [0, 0, 0]);
        assert_eq!(editor.doc.canvas, original);
        assert_eq!(
            editor.path.as_deref(),
            Some(std::path::Path::new("original.ans"))
        );
        assert!(editor.dirty);
        assert_eq!(editor.selection.unwrap().bounds(), (3, 2, 4, 3));
        editor.selection = None;
        let _ = crate::run_test_frame(&ctx, Default::default(), |ctx| {
            assert_eq!(editor.rendered_image(ctx).unwrap().dimensions(), (720, 400));
        });
        editor.undo();
        assert_eq!(*editor.doc.canvas.get(3, 2).unwrap(), Cell::default());
        click_text(&ctx, &mut editor, "File");
        let output = frame(&ctx, &mut editor, vec![]);
        for label in [
            "Save as...",
            "Save selection as...",
            "Export PNG...",
            "Export JPEG...",
        ] {
            assert!(
                output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Text(text) if text.galley.text() == label)),
                "{label}"
            );
        }
    }

    #[test]
    fn selection_export_handles_reverse_drag_without_changing_the_document() {
        let mut editor = Editor::default();
        assert!(editor.selection_bytes().is_err());
        editor.save_selection();
        assert_eq!(editor.message.as_deref(), Some("Select an area first"));
        editor.message = None;
        let cell = Cell {
            ch: '█',
            fg: Color::Indexed(129),
            ..Cell::default()
        };
        editor.history.paint(&mut editor.doc.canvas, 3, 2, cell);
        editor.dirty = true;
        editor.path = Some(PathBuf::from("original.ans"));
        editor.selection = Some(Selection {
            start: (4, 3),
            end: (3, 2),
        });
        let original = editor.doc.canvas.clone();
        let bytes = editor.selection_bytes().unwrap();
        let exported = ans_io::load(&bytes, editor.encoding, 80, 25).unwrap();
        assert_eq!((exported.canvas.width(), exported.canvas.height()), (2, 2));
        assert_eq!(*exported.canvas.get(0, 0).unwrap(), cell);
        assert_eq!(*exported.canvas.get(1, 1).unwrap(), Cell::default());
        assert_eq!(editor.doc.canvas, original);
        assert_eq!(editor.path, Some(PathBuf::from("original.ans")));
        assert!(editor.dirty);
        assert_eq!((editor.width, editor.height), (80, 25));
        assert_eq!(editor.selection.unwrap().bounds(), (3, 2, 4, 3));
        editor.undo();
        assert_eq!(*editor.doc.canvas.get(3, 2).unwrap(), Cell::default());
        editor.selection = Some(Selection {
            start: (79, 24),
            end: (80, 25),
        });
        assert_eq!(
            editor.selection_bytes().unwrap_err(),
            "Selection is outside the canvas"
        );
    }

    #[test]
    fn typing_wraps_uses_brush_and_supports_undo() {
        let mut editor = Editor::default();
        editor.doc.canvas = Canvas::new(2, 2).unwrap();
        editor.brush.fg = Color::Rgb(1, 2, 3);
        editor.canvas_input(&[egui::Event::Text("AéBCX".into())]);
        assert_eq!(editor.doc.canvas.get(0, 0).unwrap().ch, 'A');
        assert_eq!(editor.doc.canvas.get(1, 0).unwrap().ch, 'é');
        assert_eq!(editor.doc.canvas.get(0, 1).unwrap().ch, 'B');
        let last = editor.doc.canvas.get(1, 1).unwrap();
        assert_eq!((last.ch, last.fg), ('C', Color::Rgb(1, 2, 3)));
        assert_eq!(editor.cursor, (2, 1));
        editor.canvas_input(&[key(egui::Key::Backspace)]);
        assert_eq!(editor.doc.canvas.get(1, 1).unwrap().ch, ' ');
        editor.undo();
        assert_eq!(editor.doc.canvas.get(1, 1).unwrap().ch, 'C');
    }

    #[test]
    fn cursor_keys_and_resize_remain_in_bounds() {
        let mut editor = Editor::default();
        editor.doc.canvas = Canvas::new(3, 2).unwrap();
        editor.canvas_input(&[
            key(egui::Key::ArrowDown),
            key(egui::Key::End),
            key(egui::Key::Enter),
        ]);
        assert_eq!(editor.cursor, (2, 1));
        editor.doc.canvas.resize(1, 1).unwrap();
        editor.clamp_cursor();
        assert_eq!(editor.cursor, (0, 0));
        editor.canvas_input(&[
            key(egui::Key::Backspace),
            egui::Event::Ime(egui::ImeEvent::Commit("█".into())),
        ]);
        assert_eq!(editor.doc.canvas.get(0, 0).unwrap().ch, '█');
    }

    #[test]
    fn rectangular_cut_copy_paste_preserves_cells_and_undoes_atomically() {
        let mut editor = Editor::default();
        editor.doc.canvas = Canvas::new(4, 3).unwrap();
        let red = Cell {
            ch: 'A',
            fg: Color::Rgb(3, 5, 7),
            ..Cell::default()
        };
        let blue = Cell {
            ch: 'B',
            bg: Color::Indexed(42),
            ..Cell::default()
        };
        editor.doc.canvas.set(0, 0, red);
        editor.doc.canvas.set(1, 1, blue);
        editor.selection = Some(Selection {
            start: (1, 1),
            end: (0, 0),
        });
        let text = editor.copy_selection(true).unwrap();
        assert_eq!(text, "A \n B");
        assert_eq!(*editor.doc.canvas.get(0, 0).unwrap(), Cell::default());
        editor.undo();
        assert_eq!(*editor.doc.canvas.get(0, 0).unwrap(), red);
        assert_eq!(*editor.doc.canvas.get(1, 1).unwrap(), blue);
        editor.cursor = (2, 1);
        editor.paste(&text).unwrap();
        assert_eq!(*editor.doc.canvas.get(2, 1).unwrap(), red);
        assert_eq!(*editor.doc.canvas.get(3, 2).unwrap(), blue);
        editor.undo();
        assert_eq!(*editor.doc.canvas.get(2, 1).unwrap(), Cell::default());
        assert_eq!(*editor.doc.canvas.get(3, 2).unwrap(), Cell::default());
        editor.redo();
        assert_eq!(*editor.doc.canvas.get(3, 2).unwrap(), blue);
    }

    #[test]
    fn paste_checks_bounds_before_edit_and_accepts_external_text() {
        let mut editor = Editor::default();
        editor.doc.canvas = Canvas::new(3, 2).unwrap();
        editor.cursor = (2, 1);
        assert!(editor.paste("AB").is_err());
        assert_eq!(editor.doc.canvas.get(2, 1).unwrap().ch, ' ');
        editor.cursor = (0, 0);
        editor.brush.fg = Color::Ansi(3);
        editor.paste("é\r\nZ").unwrap();
        assert_eq!(editor.doc.canvas.get(0, 0).unwrap().ch, 'é');
        assert_eq!(editor.doc.canvas.get(0, 1).unwrap().fg, Color::Ansi(3));
        assert!(editor.paste("bad\ttext").is_err());
    }

    #[test]
    fn selection_drag_previews_without_editing_and_commits_overlap_atomically() {
        let ctx = context();
        let mut editor = Editor::default();
        editor.doc.canvas = Canvas::new(8, 4).unwrap();
        let styled = Cell {
            ch: 'A',
            fg: Color::Rgb(3, 5, 7),
            bg: Color::Indexed(42),
            attrs: ansi_core::canvas::Attributes {
                blink: true,
                underline: true,
                ..Default::default()
            },
        };
        let blank = Cell {
            bg: Color::Rgb(19, 31, 47),
            ..Cell::default()
        };
        editor.doc.canvas.set(1, 0, styled);
        editor.doc.canvas.set(2, 1, blank);
        editor.doc.canvas.set(3, 2, Cell { ch: 'Z', ..styled });
        editor.selection = Some(Selection {
            start: (2, 1),
            end: (1, 0),
        });
        editor.clipboard = Some(CellClipboard {
            width: 1,
            height: 1,
            cells: vec![styled],
            text: "kept".into(),
        });
        let original = editor.doc.canvas.clone();
        let output = frame(&ctx, &mut editor, vec![]);
        let start = canvas_cell_center(&output, 2, 1);
        let end = canvas_cell_center(&output, 3, 2);
        frame(
            &ctx,
            &mut editor,
            vec![
                egui::Event::PointerMoved(start),
                pointer_button(start, true),
            ],
        );
        let preview = frame(&ctx, &mut editor, vec![egui::Event::PointerMoved(end)]);
        assert_eq!(editor.doc.canvas, original);
        assert!(!editor.dirty);
        assert_eq!(
            editor.selection_drag.as_ref().unwrap().destination.bounds(),
            (2, 1, 3, 2)
        );
        assert!(preview.shapes.iter().any(|clipped| {
            matches!(&clipped.shape, egui::Shape::Rect(shape)
                if shape.fill == Color32::from_rgb(19, 31, 47) && shape.rect.contains(end))
        }));
        frame(
            &ctx,
            &mut editor,
            vec![egui::Event::Text("ignored".into()), key(egui::Key::Delete)],
        );
        assert_eq!(editor.doc.canvas, original);
        frame(&ctx, &mut editor, vec![pointer_button(end, false)]);
        let mut expected = original.clone();
        for y in 0..=1 {
            for x in 1..=2 {
                expected.set(x, y, Cell::default());
            }
        }
        for y in 0..=1 {
            for x in 1..=2 {
                expected.set(x + 1, y + 1, *original.get(x, y).unwrap());
            }
        }
        assert_eq!(editor.doc.canvas, expected);
        assert!(editor.dirty);
        assert!(editor.selection_drag.is_none());
        assert_eq!(editor.selection.unwrap().bounds(), (2, 1, 3, 2));
        assert_eq!(editor.cursor, (2, 1));
        assert_eq!(editor.clipboard.as_ref().unwrap().text, "kept");
        editor.undo();
        assert_eq!(editor.doc.canvas, original);
        assert!(!editor.history.undo(&mut editor.doc.canvas));
        editor.redo();
        assert_eq!(editor.doc.canvas, expected);
    }

    #[test]
    fn selection_drag_clamps_whole_region_and_uses_release_position_outside_canvas() {
        let ctx = context();
        let mut editor = Editor {
            selecting: true,
            ..Default::default()
        };
        editor.doc.canvas = Canvas::new(6, 3).unwrap();
        let cell = Cell {
            ch: 'X',
            ..Cell::default()
        };
        editor.doc.canvas.set(1, 0, cell);
        editor.selection = Some(Selection {
            start: (2, 1),
            end: (1, 0),
        });
        let original = editor.doc.canvas.clone();
        let output = frame(&ctx, &mut editor, vec![]);
        let start = canvas_cell_center(&output, 1, 0);
        frame(
            &ctx,
            &mut editor,
            vec![
                egui::Event::PointerMoved(start),
                pointer_button(start, true),
            ],
        );
        frame(
            &ctx,
            &mut editor,
            vec![egui::Event::PointerMoved(Pos2::new(-500.0, -500.0))],
        );
        assert_eq!(
            editor.selection_drag.as_ref().unwrap().destination.bounds(),
            (0, 0, 1, 1)
        );
        assert_eq!(editor.doc.canvas, original);
        let end = Pos2::new(1000.0, 1000.0);
        frame(
            &ctx,
            &mut editor,
            vec![egui::Event::PointerMoved(end), pointer_button(end, false)],
        );
        assert!(editor.selection_drag.is_none());
        assert_eq!(editor.selection.unwrap().bounds(), (4, 1, 5, 2));
        let moved = editor.selection.unwrap();
        assert_eq!(
            (moved.start, moved.end),
            ((5, 2), (4, 1)),
            "moving keeps the selection's drag direction for gradients"
        );
        assert_eq!(*editor.doc.canvas.get(4, 1).unwrap(), cell);
        assert_eq!(*editor.doc.canvas.get(1, 0).unwrap(), Cell::default());
        editor.undo();
        assert_eq!(editor.doc.canvas, original);
    }

    #[test]
    fn selection_click_and_zero_distance_drag_preserve_artwork_and_history() {
        let ctx = context();
        let mut editor = Editor::default();
        editor.doc.canvas = Canvas::new(4, 2).unwrap();
        editor.doc.canvas.set(0, 0, editor.brush);
        editor.selection = Some(Selection {
            start: (0, 0),
            end: (3, 1),
        });
        let original = editor.doc.canvas.clone();
        let output = frame(&ctx, &mut editor, vec![]);
        let start = canvas_cell_center(&output, 1, 0);
        frame(
            &ctx,
            &mut editor,
            vec![
                egui::Event::PointerMoved(start),
                pointer_button(start, true),
            ],
        );
        frame(&ctx, &mut editor, vec![pointer_button(start, false)]);
        assert_eq!(editor.selection.unwrap().bounds(), (0, 0, 3, 1));
        assert_eq!(editor.doc.canvas, original);
        frame(&ctx, &mut editor, vec![pointer_button(start, true)]);
        let end = canvas_cell_center(&output, 3, 1);
        frame(&ctx, &mut editor, vec![egui::Event::PointerMoved(end)]);
        frame(&ctx, &mut editor, vec![pointer_button(end, false)]);
        assert_eq!(editor.doc.canvas, original);
        assert!(!editor.dirty);
        assert!(!editor.history.undo(&mut editor.doc.canvas));
    }

    #[test]
    fn canvas_click_focuses_typing_and_routes_clipboard_events() {
        let ctx = context();
        let mut editor = Editor::default();
        let output = frame(&ctx, &mut editor, vec![]);
        let pos = canvas_cell_center(&output, 1, 0);
        frame(
            &ctx,
            &mut editor,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        frame(
            &ctx,
            &mut editor,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        frame(&ctx, &mut editor, vec![egui::Event::Text("AB".into())]);
        assert_eq!(editor.doc.canvas.get(1, 0).unwrap().ch, 'A');
        assert_eq!(editor.doc.canvas.get(2, 0).unwrap().ch, 'B');
        editor.selection = Some(Selection {
            start: (1, 0),
            end: (2, 0),
        });
        let output = frame(&ctx, &mut editor, vec![egui::Event::Copy]);
        assert!(output
            .platform_output
            .commands
            .iter()
            .any(|command| matches!(command, egui::OutputCommand::CopyText(text) if text == "AB")));
        editor.cursor = (5, 1);
        frame(&ctx, &mut editor, vec![egui::Event::Paste("AB".into())]);
        assert_eq!(editor.doc.canvas.get(5, 1).unwrap().ch, 'A');
        assert_eq!(editor.doc.canvas.get(6, 1).unwrap().ch, 'B');
    }

    #[test]
    fn arrow_keys_keep_focus_on_canvas() {
        let ctx = context();
        let mut editor = Editor::default();
        let output = frame(&ctx, &mut editor, vec![]);
        let pos = canvas_cell_center(&output, 3, 2);
        frame(
            &ctx,
            &mut editor,
            vec![egui::Event::PointerMoved(pos), pointer_button(pos, true)],
        );
        frame(&ctx, &mut editor, vec![pointer_button(pos, false)]);
        assert_eq!(editor.cursor, (3, 2));
        let key = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        for k in [
            egui::Key::ArrowLeft,
            egui::Key::ArrowLeft,
            egui::Key::ArrowUp,
            egui::Key::ArrowRight,
            egui::Key::ArrowDown,
            egui::Key::ArrowDown,
        ] {
            frame(&ctx, &mut editor, vec![key(k)]);
            frame(&ctx, &mut editor, vec![]);
        }
        assert_eq!(editor.cursor, (2, 3));
        frame(&ctx, &mut editor, vec![egui::Event::Text("Z".into())]);
        assert_eq!(editor.doc.canvas.get(2, 3).unwrap().ch, 'Z');
    }

    #[test]
    fn selection_drag_does_not_paint_and_text_field_takes_focus() {
        let ctx = context();
        let mut editor = Editor {
            selecting: true,
            ..Default::default()
        };
        let output = frame(&ctx, &mut editor, vec![]);
        let field = output
            .shapes
            .iter()
            .find_map(|clipped| {
                if let egui::Shape::Text(shape) = &clipped.shape {
                    if shape.galley.text() == "█" {
                        return Some(
                            shape
                                .galley
                                .rect
                                .translate(shape.pos.to_vec2())
                                .right_center()
                                + Vec2::new(2.0, 0.0),
                        );
                    }
                }
                None
            })
            .expect("brush input is visible");
        let start = canvas_cell_center(&output, 1, 0);
        let end = canvas_cell_center(&output, 3, 1);
        frame(
            &ctx,
            &mut editor,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        frame(&ctx, &mut editor, vec![egui::Event::PointerMoved(end)]);
        frame(
            &ctx,
            &mut editor,
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(editor.selection.unwrap().bounds(), (1, 0, 3, 1));
        assert_eq!(editor.doc.canvas.get(1, 0).unwrap().ch, ' ');
        frame(
            &ctx,
            &mut editor,
            vec![
                egui::Event::PointerMoved(field),
                egui::Event::PointerButton {
                    pos: field,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        frame(
            &ctx,
            &mut editor,
            vec![egui::Event::PointerButton {
                pos: field,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        frame(&ctx, &mut editor, vec![egui::Event::Text("Z".into())]);
        assert_eq!(editor.brush.ch, 'Z');
        assert_eq!(editor.doc.canvas.get(1, 0).unwrap().ch, ' ');
    }
}
