#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod font_library;
mod glyph;
mod image_export;
mod updates;

use ansi_core::{
    ans_io::{self, Document},
    canvas::{Canvas, Cell, Color, History, DEFAULT_HEIGHT, DEFAULT_WIDTH},
    encoding::{decode_cp437, Encoding},
    sauce::Sauce,
    tdf,
};
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Sense, Vec2};
use egui_material_icons::icons::{
    ICON_ADD, ICON_APPS, ICON_BRUSH, ICON_CENTER_FOCUS_STRONG, ICON_CHEVRON_RIGHT,
    ICON_CONTENT_COPY, ICON_CONTENT_CUT, ICON_DRAG_HANDLE, ICON_EXPAND_MORE, ICON_FLASH_ON,
    ICON_FORMAT_BOLD, ICON_FORMAT_UNDERLINED, ICON_GRID_VIEW, ICON_PALETTE, ICON_SEARCH,
    ICON_SELECT_ALL, ICON_SWAP_HORIZ, ICON_ZOOM_IN, ICON_ZOOM_OUT,
};
use std::{collections::BTreeMap, path::PathBuf};

const DEFAULT_WINDOW_SIZE: Vec2 = Vec2::new(1006.0, 652.0);
const SIDEBAR_ORDER_KEY: &str = "sidebar_order_v1";
const SIDEBAR_COLLAPSED_KEY: &str = "sidebar_collapsed_v1";
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

const PALETTE: [[u8; 3]; 16] = [
    [0, 0, 0],
    [170, 0, 0],
    [0, 170, 0],
    [170, 85, 0],
    [0, 0, 170],
    [170, 0, 170],
    [0, 170, 170],
    [170, 170, 170],
    [85, 85, 85],
    [255, 85, 85],
    [85, 255, 85],
    [255, 255, 85],
    [85, 85, 255],
    [255, 85, 255],
    [85, 255, 255],
    [255, 255, 255],
];

fn display_color(color: Color) -> Color32 {
    let rgb = match color {
        Color::Ansi(n) => PALETTE.get(n as usize).copied().unwrap_or([255, 0, 255]),
        Color::Indexed(n @ 0..=15) => PALETTE[n as usize],
        Color::Indexed(n @ 16..=231) => {
            let n = n - 16;
            let component = |v| if v == 0 { 0 } else { 55 + 40 * v };
            [component(n / 36), component((n / 6) % 6), component(n % 6)]
        }
        Color::Indexed(n) => {
            let v = 8 + 10 * (n - 232);
            [v, v, v]
        }
        Color::Rgb(r, g, b) => [r, g, b],
    };
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

struct Editor {
    update_check: updates::UpdateCheck,
    about_open: bool,
    about_logo: Option<egui::TextureHandle>,
    sidebar_order: Vec<SidebarSection>,
    sidebar_collapsed: Vec<SidebarSection>,
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

#[derive(Clone, Copy, Debug)]
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

#[derive(Clone, Copy)]
enum PendingAction {
    New,
    Open,
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
        } else {
            editor.message = Some(
                "Sidebar preferences cannot be saved: application settings storage is unavailable. Check access to your user profile's application data folder."
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
                    ui.memory_mut(|memory| memory.toggle_popup(popup_id));
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
                egui::popup::popup_below_widget(
                    ui,
                    popup_id,
                    &picker,
                    egui::popup::PopupCloseBehavior::CloseOnClickOutside,
                    |ui| {
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
                    },
                );
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
            .add_filter("ANSI art", &["ans"])
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
        let definitions = ctx.fonts(|fonts| fonts.lock().fonts.definitions().clone());
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
            drag.destination = Selection {
                start: (x, y),
                end: (x + width - 1, y + height - 1),
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
    let name = "material-icons";
    fonts.font_data.insert(
        name.into(),
        egui::FontData::from_static(egui_material_icons::FONT_DATA).into(),
    );
    fonts
        .families
        .insert(egui::FontFamily::Name(name.into()), vec![name.into()]);
}

fn icon_button(ui: &mut egui::Ui, icon: &str, label: &str, selected: bool) -> egui::Response {
    let button = egui::Button::new(egui::RichText::new(icon).font(FontId::new(
        20.0,
        egui::FontFamily::Name("material-icons".into()),
    )))
    .selected(selected);
    let response = ui.add_sized([28.0, 28.0], button);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, label)
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

fn artwork_colors(doc: &Document) -> Vec<Color> {
    let ice = doc.sauce.as_ref().is_some_and(|s| s.flags() & 1 != 0);
    let mut colors = BTreeMap::new();
    for y in 0..doc.canvas.height() {
        for x in 0..doc.canvas.width() {
            let (fg, bg) = cell_colors(*doc.canvas.get(x, y).unwrap(), ice);
            for color in [fg, bg] {
                let rgb = display_color(color);
                colors.entry([rgb.r(), rgb.g(), rgb.b()]).or_insert(color);
            }
        }
    }
    colors.into_values().collect()
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

fn palette_button(
    ui: &mut egui::Ui,
    label: &str,
    color: Color,
    foreground: &mut Color,
    background: &mut Color,
) -> egui::Response {
    let selected = display_color(*foreground) == display_color(color)
        || display_color(*background) == display_color(color);
    let button = egui::Button::new("")
        .fill(display_color(color))
        .stroke(if selected {
            egui::Stroke::new(2.0_f32, Color32::WHITE)
        } else {
            ui.visuals().widgets.inactive.bg_stroke
        });
    let response = ui.add_sized([24.0, 20.0], button);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, label)
    });
    if response.clicked() {
        *foreground = color;
    }
    if response.secondary_clicked() {
        *background = color;
    }
    response.on_hover_text(format!(
        "{label}. Left-click: foreground; right-click: background."
    ))
}

fn used_palette(
    ui: &mut egui::Ui,
    colors: &[Color],
    foreground: &mut Color,
    background: &mut Color,
) {
    ui.push_id("artwork_palette", |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.small("Used colors")
            .on_hover_text("Colors currently used in the artwork. Left-click for foreground; right-click for background.");
        egui::ScrollArea::vertical()
            .id_salt("used_colors")
            .max_height(68.0)
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
            egui::WidgetType::ColorButton,
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
) {
    ui.small("Left: foreground / Right: background");
    ui.push_id("colors", |ui| {
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
                    );
                    if n % 8 == 7 {
                        ui.end_row();
                    }
                }
            });
        used_palette(ui, used_colors, foreground, background);
        ui.horizontal(|ui| {
            let indexed_id = ui.make_persistent_id("indexed_palette");
            let rgb_id = ui.make_persistent_id("rgb_palette");
            let rgb_target_id = ui.make_persistent_id("rgb_background");
            let indexed = icon_button(
                ui,
                ICON_GRID_VIEW,
                "256-color palette",
                matches!(foreground, Color::Indexed(_)) || matches!(background, Color::Indexed(_)),
            );
            if indexed.clicked() || indexed.secondary_clicked() {
                ui.memory_mut(|memory| memory.toggle_popup(indexed_id));
            }
            egui::popup::popup_below_widget(
                ui,
                indexed_id,
                &indexed,
                egui::popup::PopupCloseBehavior::CloseOnClickOutside,
                |ui| {
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
                                );
                                if response.clicked() || response.secondary_clicked() {
                                    ui.memory_mut(|memory| memory.close_popup());
                                }
                                if n % 16 == 15 {
                                    ui.end_row();
                                }
                            }
                        });
                },
            );
            let rgb = icon_button(
                ui,
                ICON_PALETTE,
                "RGB color: left-click foreground, right-click background",
                matches!(foreground, Color::Rgb(..)) || matches!(background, Color::Rgb(..)),
            );
            ui.small("FG");
            let current_fg = current_color_swatch(ui, "Foreground", *foreground);
            ui.small("BG");
            let current_bg = current_color_swatch(ui, "Background", *background);
            let target = if rgb.secondary_clicked()
                || current_fg.secondary_clicked()
                || current_bg.secondary_clicked()
            {
                Some(true)
            } else if rgb.clicked() || current_fg.clicked() || current_bg.clicked() {
                Some(false)
            } else {
                None
            };
            if let Some(background) = target {
                ui.data_mut(|data| data.insert_temp(rgb_target_id, background));
                ui.memory_mut(|memory| memory.open_popup(rgb_id));
            }
            egui::popup::popup_below_widget(
                ui,
                rgb_id,
                &rgb,
                egui::popup::PopupCloseBehavior::CloseOnClickOutside,
                |ui| {
                    ui.spacing_mut().slider_width = 275.0;
                    let editing_background =
                        ui.data(|data| data.get_temp::<bool>(rgb_target_id).unwrap_or(false));
                    let color = if editing_background {
                        ui.label("Background RGB color");
                        &mut *background
                    } else {
                        ui.label("Foreground RGB color");
                        &mut *foreground
                    };
                    let mut selected = display_color(*color);
                    if egui::color_picker::color_picker_color32(
                        ui,
                        &mut selected,
                        egui::color_picker::Alpha::Opaque,
                    ) {
                        *color = Color::Rgb(selected.r(), selected.g(), selected.b());
                    }
                },
            );
        });
    });
}

impl eframe::App for Editor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ui(ctx);
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
                    if icon_button(ui, ICON_BRUSH, "Draw", !self.selecting).clicked() {
                        self.selecting = false;
                    }
                    if icon_button(ui, ICON_SELECT_ALL, "Select a rectangle", self.selecting)
                        .clicked()
                    {
                        self.selecting = true;
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
                color_controls(ui, &mut self.brush.fg, &mut self.brush.bg, used_colors)
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
                    if ui.text_edit_multiline(&mut comments).changed() {
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

    fn ui(&mut self, ctx: &egui::Context) {
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
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui
                        .button("New image")
                        .on_hover_text(format!(
                            "Create a {DEFAULT_WIDTH} x {DEFAULT_HEIGHT} image; use Resize canvas for custom dimensions"
                        ))
                        .clicked()
                    {
                        ui.close_menu();
                        self.request(PendingAction::New, ctx);
                    }
                    if ui
                        .button("Open...")
                        .on_hover_text("Use SAUCE dimensions, or 80 columns and the artwork's inferred height")
                        .clicked()
                    {
                        ui.close_menu();
                        self.request(PendingAction::Open, ctx);
                    }
                    ui.separator();
                    if ui.button("Save").clicked() {
                        ui.close_menu();
                        self.save(false);
                    }
                    if ui.button("Save as...").clicked() {
                        ui.close_menu();
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
                        ui.close_menu();
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
                            ui.close_menu();
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
                                ui.close_menu();
                            }
                        }
                    });
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        ui.close_menu();
                        self.request(PendingAction::Close, ctx);
                    }
                });
                ui.menu_button("Edit", |ui| {
                    if ui
                        .add(egui::Button::new("Undo").shortcut_text(shortcut_text(ctx, egui::Key::Z)))
                        .clicked()
                    {
                        ui.close_menu();
                        self.undo();
                    }
                    if ui
                        .add(egui::Button::new("Redo").shortcut_text(ctx.format_shortcut(&REDO_SHORTCUT)))
                        .clicked()
                    {
                        ui.close_menu();
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
                            ui.close_menu();
                            clipboard_action = Some(cut);
                        }
                    }
                    if ui
                        .add(egui::Button::new("Paste").shortcut_text(shortcut_text(ctx, egui::Key::V)))
                        .clicked()
                    {
                        ui.close_menu();
                        focus_canvas = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                    }
                    ui.separator();
                    if ui
                        .selectable_value(&mut self.selecting, false, "Draw mode")
                        .clicked()
                    {
                        ui.close_menu();
                        focus_canvas = true;
                    }
                    if ui.button("CP437 character picker...").clicked() {
                        ui.close_menu();
                        self.picker_open = true;
                    }
                    if ui.button("TheDraw fonts...").clicked() {
                        ui.close_menu();
                        self.tdf_open = true;
                    }
                });
                ui.menu_button("Selection", |ui| {
                    if ui
                        .selectable_value(&mut self.selecting, true, "Rectangular selection")
                        .clicked()
                    {
                        ui.close_menu();
                        focus_canvas = true;
                    }
                    ui.separator();
                    if ui.button("Select all").clicked() {
                        ui.close_menu();
                        self.selecting = true;
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
                        ui.close_menu();
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
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            self.selection_drag.is_none() && self.zoom_percent > 50.0,
                            egui::Button::new("Zoom out").shortcut_text(shortcut_text(ctx, egui::Key::Minus)),
                        )
                        .clicked()
                    {
                        self.change_zoom(-1);
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            self.selection_drag.is_none(),
                            egui::Button::new("Actual size (100%)").shortcut_text(shortcut_text(ctx, egui::Key::Num0)),
                        )
                        .clicked()
                    {
                        self.zoom_percent = DEFAULT_ZOOM_PERCENT;
                        ui.close_menu();
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
                        ui.close_menu();
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
        let used_colors = artwork_colors(&self.doc);
        egui::SidePanel::left("tools")
            .resizable(true)
            .default_width(260.0)
            .show(ctx, |ui| {
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
                                                    egui::WidgetType::Button,
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
                                                    ui.memory_mut(|memory| memory.close_popup());
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
                                                            egui::WidgetType::Button,
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
        let mut canvas_id = None;
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.style_mut().spacing.scroll = egui::style::ScrollStyle::solid();
            let zoom = self.zoom_percent / DEFAULT_ZOOM_PERCENT;
            let max_height = if self.zoom_percent == DEFAULT_ZOOM_PERCENT {
                50.0 * glyph::CELL_SIZE.y
            } else {
                f32::INFINITY
            };
            egui::ScrollArea::both()
                .max_height(max_height)
                .show_viewport(ui, |ui, viewport| {
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
                    let bounded_point = |p: Pos2| {
                        let (x, y) = point(p);
                        (
                            x.min(self.doc.canvas.width() - 1),
                            y.min(self.doc.canvas.height() - 1),
                        )
                    };
                    if !self.tdf_open && response.drag_started_by(egui::PointerButton::Primary) {
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
                        if !self.tdf_open && response.hovered() && !moving_selection {
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
                });
        });
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
                                                egui::WidgetType::Button,
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
        if let Some(action) = self.pending {
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
                        ui.add(egui::TextEdit::multiline(&mut self.tdf_text).desired_rows(2));
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

fn main() -> eframe::Result {
    let icon = eframe::icon_data::from_png_bytes(APP_ICON_PNG)
        .map_err(|error| eframe::Error::AppCreation(Box::new(error)))?;
    eframe::run_native(
        "ADraw",
        eframe::NativeOptions {
            persist_window: false,
            viewport: egui::ViewportBuilder::default()
                .with_inner_size(DEFAULT_WINDOW_SIZE)
                .with_icon(icon),
            ..Default::default()
        },
        Box::new(|cc| {
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
            cc.egui_ctx.set_fonts(fonts);
            let mut editor = Editor::new(cc.storage);
            editor.update_check = updates::UpdateCheck::start(cc.egui_ctx.clone());
            Ok(Box::new(editor))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::Storage;

    #[test]
    fn window_icon_matches_the_supplied_executable_icon_and_decodes() {
        let ico = include_bytes!("../assets/adraw.ico");
        let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
        let entry = ico[6..]
            .chunks_exact(16)
            .take(count)
            .find(|entry| entry[0] == 0 && entry[1] == 0)
            .expect("ICO must include a 256x256 image");
        let length = u32::from_le_bytes(entry[8..12].try_into().unwrap()) as usize;
        let offset = u32::from_le_bytes(entry[12..16].try_into().unwrap()) as usize;
        assert_eq!(APP_ICON_PNG, &ico[offset..offset + length]);
        let icon = eframe::icon_data::from_png_bytes(APP_ICON_PNG).unwrap();
        assert_eq!((icon.width, icon.height), (256, 256));
        assert_eq!(icon.rgba.len(), 256 * 256 * 4);
        assert!(icon.rgba.chunks_exact(4).any(|pixel| pixel[3] != 0));
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

    fn sidebar_icon(output: &egui::FullOutput, section: SidebarSection, icon: &str) -> Pos2 {
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
                egui::Shape::Text(shape) if shape.galley.text() == icon => {
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
                SidebarSection::Brush => ICON_BRUSH,
                SidebarSection::Zoom => ICON_ZOOM_IN,
                SidebarSection::Colors => ICON_PALETTE,
                SidebarSection::Attributes => ICON_FORMAT_BOLD,
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
                        if shape.fill == ctx.style().visuals.widgets.inactive.bg_fill
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
                        && shape.fill != ctx.style().visuals.widgets.inactive.bg_fill)
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
            let mut editor = Editor::default();
            let original = editor.doc.canvas.clone();
            let order = editor.sidebar_order.clone();
            frame(&ctx, &mut editor, vec![]);
            let output = frame(&ctx, &mut editor, vec![]);
            let start = sidebar_handle(&output, section);
            let target = if section == SidebarSection::Brush {
                sidebar_handle(&output, SidebarSection::Attributes) + Vec2::new(-100.0, 38.0)
            } else {
                sidebar_handle(&output, SidebarSection::Brush)
            };
            frame(
                &ctx,
                &mut editor,
                vec![
                    egui::Event::PointerMoved(start),
                    pointer_button(start, true),
                ],
            );
            frame(&ctx, &mut editor, vec![egui::Event::PointerMoved(target)]);
            assert_eq!(
                editor.sidebar_order, order,
                "hovering previews rather than committing"
            );
            frame(&ctx, &mut editor, vec![pointer_button(target, false)]);
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
            let output = frame(&ctx, &mut editor, vec![]);
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
    fn initial_window_fits_default_canvas_and_full_metadata_sidebar() {
        let ctx = context();
        let mut editor = Editor::default();
        editor.doc.sauce.as_mut().unwrap().title = "Example title".into();
        let screen = Rect::from_min_size(Pos2::ZERO, DEFAULT_WINDOW_SIZE);
        let mut render = || {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| editor.ui(ctx),
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
                        (8.0..=24.0).contains(&bottom_padding),
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
            (18.0..=19.0).contains(&right_padding),
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
        ctx.style_mut(|style| style.animation_time = 0.0);
        let mut editor = Editor::default();
        editor.doc.canvas = Canvas::new(80, 51).unwrap();
        let screen = Rect::from_min_size(Pos2::ZERO, DEFAULT_WINDOW_SIZE);
        let mut render = || {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| editor.ui(ctx),
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
        assert!((8.0..=9.0).contains(&(screen.right() - scrollbar.right())));
        assert!(!output_has_horizontal_scrollbar(&output));
    }

    fn frame(
        ctx: &egui::Context,
        editor: &mut Editor,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
            focused: true,
            events,
            ..Default::default()
        };
        ctx.run(input, |ctx| editor.ui(ctx))
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

    #[test]
    fn cp437_picker_uses_vga_bitmaps_and_preserves_character_selection() {
        let ctx = context();
        ctx.style_mut(|style| style.animation_time = 0.0);
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
        assert_eq!(
            output
                .shapes
                .iter()
                .filter(|clipped| matches!(clipped.shape, egui::Shape::Mesh(_)))
                .count(),
            253
        );
        let pos = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape) if shape.rect.size() == Vec2::splat(32.0) => {
                    Some(shape.rect.center())
                }
                _ => None,
            })
            .nth(b'A' as usize - 1)
            .unwrap();
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
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Middle,
                egui::Id::new("unicode"),
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
        ctx.style_mut(|style| style.animation_time = 0.0);
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
    fn zoomed_typing_selection_moving_and_pasting_use_character_cell_coordinates() {
        for zoom_percent in [50.0, 149.0, 200.0] {
            let ctx = context();
            let mut editor = Editor::default();
            editor.zoom_percent = zoom_percent;
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
            ctx.style_mut(|style| style.animation_time = 0.0);
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
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            Pos2::ZERO,
                            Vec2::new(1120.0, 1400.0),
                        )),
                        focused: true,
                        events,
                        ..Default::default()
                    },
                    |ctx| editor.ui(ctx),
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
    fn used_colors_merge_formats_and_follow_displayed_brightness() {
        let mut editor = Editor::default();
        editor.doc.canvas = Canvas::new(3, 1).unwrap();
        editor.doc.canvas.set(
            0,
            0,
            Cell {
                fg: Color::Ansi(1),
                ..Cell::default()
            },
        );
        editor.doc.canvas.set(
            1,
            0,
            Cell {
                fg: Color::Indexed(1),
                bg: Color::Rgb(170, 0, 0),
                ..Cell::default()
            },
        );
        editor.doc.canvas.set(
            2,
            0,
            Cell {
                fg: Color::Rgb(8, 19, 29),
                bg: Color::Indexed(201),
                ..Cell::default()
            },
        );
        assert_eq!(
            artwork_colors(&editor.doc),
            vec![
                Color::Ansi(0),
                Color::Rgb(8, 19, 29),
                Color::Ansi(1),
                Color::Indexed(201)
            ]
        );
        editor.doc.canvas = Canvas::new(1, 1).unwrap();
        editor.doc.canvas.set(
            0,
            0,
            Cell {
                fg: Color::Ansi(1),
                bg: Color::Ansi(2),
                attrs: ansi_core::canvas::Attributes {
                    bold: true,
                    blink: true,
                    inverse: true,
                    ..Default::default()
                },
                ..Cell::default()
            },
        );
        assert_eq!(
            artwork_colors(&editor.doc),
            vec![Color::Ansi(2), Color::Ansi(9)]
        );
        let mut record = [0; 128];
        record[..7].copy_from_slice(b"SAUCE00");
        record[105] = 1;
        editor.doc.sauce = ansi_core::sauce::parse(&record).unwrap().1;
        assert_eq!(
            artwork_colors(&editor.doc),
            vec![Color::Ansi(10), Color::Ansi(9)]
        );
    }

    #[test]
    fn used_swatches_select_foreground_and_background_without_editing_artwork() {
        let ctx = context();
        ctx.style_mut(|style| style.animation_time = 0.0);
        let mut editor = Editor::default();
        editor.doc.canvas = Canvas::new(1, 1).unwrap();
        let cell = Cell {
            fg: Color::Rgb(8, 19, 29),
            bg: Color::Indexed(201),
            ..Cell::default()
        };
        editor.doc.canvas.set(0, 0, cell);
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
    fn used_swatches_update_after_typing_undo_redo_cut_paste_and_resize() {
        let ctx = context();
        let mut editor = Editor::default();
        editor.doc.canvas = Canvas::new(2, 1).unwrap();
        let color = Color::Rgb(8, 19, 29);
        let count = |output: &egui::FullOutput| {
            output
                .shapes
                .iter()
                .filter(|clipped| {
                    matches!(&clipped.shape, egui::Shape::Rect(shape)
                if shape.rect.size() == Vec2::new(24.0, 20.0) && shape.fill == display_color(color))
                })
                .count()
        };
        frame(&ctx, &mut editor, vec![]);
        assert_eq!(count(&frame(&ctx, &mut editor, vec![])), 0);
        editor.brush.fg = color;
        editor.canvas_input(&[egui::Event::Text("A".into())]);
        assert_eq!(count(&frame(&ctx, &mut editor, vec![])), 1);
        editor.undo();
        assert_eq!(count(&frame(&ctx, &mut editor, vec![])), 0);
        editor.redo();
        assert_eq!(count(&frame(&ctx, &mut editor, vec![])), 1);
        editor.selection = Some(Selection {
            start: (0, 0),
            end: (0, 0),
        });
        let text = editor.copy_selection(true).unwrap();
        assert_eq!(count(&frame(&ctx, &mut editor, vec![])), 0);
        editor.cursor = (1, 0);
        editor.paste(&text).unwrap();
        assert_eq!(count(&frame(&ctx, &mut editor, vec![])), 1);
        editor.history.resize(&mut editor.doc.canvas, 1, 1).unwrap();
        editor.clamp_cursor();
        assert_eq!(count(&frame(&ctx, &mut editor, vec![])), 0);
        editor.undo();
        assert_eq!(count(&frame(&ctx, &mut editor, vec![])), 1);
        editor.doc.canvas.clear(Cell::default());
        assert_eq!(count(&frame(&ctx, &mut editor, vec![])), 0);
    }

    #[test]
    fn large_used_palettes_are_virtualized_and_scroll_to_the_last_color() {
        let ctx = context();
        ctx.style_mut(|style| style.animation_time = 0.0);
        let mut editor = Editor::default();
        editor.doc.canvas = Canvas::new(64, 1).unwrap();
        for n in 0..64 {
            editor.doc.canvas.set(
                n,
                0,
                Cell {
                    fg: Color::Rgb(176, n as u8, 20),
                    ..Cell::default()
                },
            );
        }
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
                ctx.style_mut(|style| style.animation_time = 0.0);
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
                            if trigger == 0 && shape.galley.text() == ICON_PALETTE =>
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
                let pos = output
                    .shapes
                    .iter()
                    .find_map(|clipped| match &clipped.shape {
                        egui::Shape::Mesh(mesh)
                            if mesh.calc_bounds().size() == Vec2::splat(275.0) =>
                        {
                            Some(mesh.calc_bounds().center())
                        }
                        _ => None,
                    })
                    .expect("RGB saturation/value control is visible");
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
    fn indexed_palette_opens_without_changing_color_and_selects_all_color_ranges() {
        for (background, index) in [(false, 201), (true, 16), (false, 255), (true, 0)] {
            let ctx = context();
            ctx.style_mut(|style| style.animation_time = 0.0);
            let mut editor = Editor::default();
            editor.brush.fg = Color::Ansi(3);
            editor.brush.bg = Color::Rgb(8, 19, 29);
            let original = editor.brush;
            let output = frame(&ctx, &mut editor, vec![]);
            let icons: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(shape) if shape.galley.text() == ICON_GRID_VIEW => {
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
                            && shape.rect.size() == Vec2::new(24.0, 20.0) =>
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
        ctx.style_mut(|style| style.animation_time = 0.0);
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
        assert!(ctx.screen_rect().contains_rect(button));
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
        let mut editor = Editor::default();
        editor.tdf_open = true;
        frame(&ctx, &mut editor, vec![]);
        let pos = Pos2::new(280.0, 40.0);
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

    fn click_text(ctx: &egui::Context, editor: &mut Editor, text: &str) -> egui::FullOutput {
        let output = frame(ctx, editor, vec![]);
        let pos = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) if shape.galley.text() == text => {
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
        let mut editor = Editor::default();
        editor.width = 120;
        editor.height = 70;
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
        let _ = ctx.run(Default::default(), |ctx| {
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
        let _ = ctx.run(Default::default(), |ctx| {
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
        let mut editor = Editor::default();
        editor.selecting = true;
        editor.doc.canvas = Canvas::new(6, 3).unwrap();
        let cell = Cell {
            ch: 'X',
            ..Cell::default()
        };
        editor.doc.canvas.set(1, 0, cell);
        editor.selection = Some(Selection {
            start: (1, 0),
            end: (2, 1),
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
        frame(&ctx, &mut editor, vec![]);
        let start = Pos2::new(285.0, 45.0);
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
        let end = Pos2::new(305.0, 65.0);
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
        frame(&ctx, &mut editor, vec![]);
        let pos = Pos2::new(280.0, 40.0);
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
    fn selection_drag_does_not_paint_and_text_field_takes_focus() {
        let ctx = context();
        let mut editor = Editor::default();
        editor.selecting = true;
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
        let start = Pos2::new(280.0, 40.0);
        let end = Pos2::new(300.0, 60.0);
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
