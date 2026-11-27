use crate::{cell_colors, display_color, glyph, Selection, SelectionDrag};
use ansi_core::{
    ans_io::Document,
    canvas::{Canvas, Cell},
};
use eframe::egui::{
    self,
    epaint::{text::Fonts, FontColorTransferFunction, TextOptions},
    Color32, Pos2, Rect, Sense, Vec2,
};
use std::{collections::HashMap, ops::Range};

const MIN_MAP_SIDE: f32 = 8.0;
const MIN_INDICATOR_SIDE: f32 = 6.0;
pub(super) const MAX_PANEL_WIDTH: f32 = 320.0;
const MAX_TEXTURE_SIDE: usize = 1024;
const MAX_TEXTURE_PIXELS: usize = 128 * 1024;
const SAMPLES: usize = 4;

#[derive(Default)]
pub(super) struct Minimap {
    image: Option<(ImageKey, egui::TextureHandle)>,
    preview: Option<Preview>,
    rasterizer: Option<Rasterizer>,
    pub(super) viewport: Option<Viewport>,
    drag_offset: Option<Vec2>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ImageKey {
    dimensions: [usize; 2],
    texture_size: [usize; 2],
    revision: u64,
    ice: bool,
    drag: Option<(Selection, Selection)>,
}

pub(super) struct Widget {
    response: egui::Response,
    painter: egui::Painter,
}

pub(super) struct Navigation {
    pub(super) offset: Vec2,
    pub(super) scroll_id: egui::Id,
    pub(super) scrollable: [bool; 2],
    pub(super) reset: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Viewport {
    pub(super) map: Rect,
    pub(super) canvas_size: Vec2,
    pub(super) size: Vec2,
    pub(super) offset: Vec2,
    scroll_id: egui::Id,
}

impl Viewport {
    fn max_offset(self) -> Vec2 {
        (self.canvas_size - self.size).max(Vec2::ZERO)
    }

    pub(super) fn indicator(self) -> Rect {
        let mut size = Vec2::ZERO;
        let mut position = self.map.min;
        let max_offset = self.max_offset();
        for axis in 0..2 {
            let map_size = self.map.size()[axis];
            let visible = (self.size[axis] / self.canvas_size[axis]).clamp(0.0, 1.0);
            size[axis] = (map_size * visible)
                .max(MIN_INDICATOR_SIDE.min(map_size / 2.0))
                .min(map_size);
            if max_offset[axis] > 0.0 {
                position[axis] += (map_size - size[axis])
                    * (self.offset[axis] / max_offset[axis]).clamp(0.0, 1.0);
            }
        }
        Rect::from_min_size(position, size)
    }

    fn offset_at(self, pointer: Pos2, grab: Vec2) -> Vec2 {
        let indicator = self.indicator();
        let travel = self.map.size() - indicator.size();
        let max_offset = self.max_offset();
        let mut offset = Vec2::ZERO;
        for axis in 0..2 {
            if travel[axis] > 0.0 {
                offset[axis] = ((pointer[axis] - self.map.min[axis] - grab[axis]) / travel[axis])
                    .clamp(0.0, 1.0)
                    * max_offset[axis];
            }
        }
        offset
    }
}

impl Minimap {
    pub(super) fn layout(ui: &mut egui::Ui, canvas: &Canvas) -> Option<Widget> {
        let available = ui.available_size().max(Vec2::ZERO);
        if available.x < 1.0 || available.y < 1.0 {
            return None;
        }
        let canvas_size = Vec2::new(
            canvas.width() as f32 * glyph::CELL_SIZE.x,
            canvas.height() as f32 * glyph::CELL_SIZE.y,
        );
        let scale = (available.x / canvas_size.x)
            .min(available.y / canvas_size.y)
            .min(1.0);
        let size = (canvas_size * scale)
            .max(Vec2::splat(MIN_MAP_SIDE))
            .min(available);
        let (id, row) = ui.allocate_space(Vec2::new(available.x, size.y));
        let rect = Rect::from_min_size(Pos2::new(row.center().x - size.x / 2.0, row.top()), size);
        let response = ui
            .interact(rect, id, Sense::click_and_drag())
            .on_hover_text("Click or drag to navigate the canvas");
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Canvas minimap"));
        Some(Widget {
            response,
            painter: ui.painter().clone(),
        })
    }

    pub(super) fn navigate(&mut self, widget: &Widget) -> Option<Navigation> {
        let view = self.viewport?;
        let response = &widget.response;
        let indicator = view.indicator();
        let button = egui::PointerButton::Primary;
        let started = response.drag_started_by(button);
        let stopped = response.drag_stopped_by(button);
        if response.hovered() {
            let cursor = if response
                .hover_pos()
                .is_some_and(|position| indicator.contains(position))
            {
                egui::CursorIcon::Grab
            } else {
                egui::CursorIcon::Crosshair
            };
            response.ctx.set_cursor_icon(cursor);
        }
        if started {
            let origin = response
                .ctx
                .input(|input| input.pointer.press_origin())
                .unwrap_or(indicator.center());
            self.drag_offset = Some(if indicator.contains(origin) {
                origin - indicator.min
            } else {
                indicator.size() / 2.0
            });
        }
        let clicked = response.clicked_by(button);
        let position = response
            .interact_pointer_pos()
            .or_else(|| response.ctx.input(|input| input.pointer.latest_pos()));
        let navigation = |position, grab, reset| {
            let max_offset = view.max_offset();
            Navigation {
                offset: view.offset_at(position, grab),
                scroll_id: view.scroll_id,
                scrollable: [max_offset.x > 0.0, max_offset.y > 0.0],
                reset,
            }
        };
        let navigation = if response.dragged_by(button) || stopped {
            response.ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
            position
                .zip(self.drag_offset)
                .map(|(position, grab)| navigation(position, grab, started))
        } else if clicked {
            position.map(|position| navigation(position, indicator.size() / 2.0, true))
        } else {
            None
        };
        if stopped {
            self.drag_offset = None;
        }
        navigation
    }

    pub(super) fn paint(
        &mut self,
        widget: Widget,
        doc: &Document,
        revision: u64,
        drag: Option<&SelectionDrag>,
        canvas_size: Vec2,
        scroll: &egui::scroll_area::ScrollAreaOutput<()>,
    ) {
        let ctx = &widget.response.ctx;
        let size = texture_size(&doc.canvas, ctx.pixels_per_point());
        let ice = doc
            .sauce
            .as_ref()
            .is_some_and(|sauce| sauce.flags() & 1 != 0);
        let key = ImageKey {
            dimensions: [doc.canvas.width(), doc.canvas.height()],
            texture_size: size,
            revision,
            ice,
            drag: drag.map(|drag| (drag.source, drag.destination)),
        };
        if self.image.as_ref().is_none_or(|(old, _)| *old != key) {
            let rasterizer = self.rasterizer.get_or_insert_with(|| {
                Rasterizer::new(ctx.fonts(|fonts| fonts.definitions().clone()))
            });
            let rebuild = self.image.as_ref().is_none_or(|(old, _)| {
                old.dimensions != key.dimensions || old.texture_size != size || old.ice != ice
            });
            if rebuild {
                let preview = Preview::new(&doc.canvas, drag, ice, size, rasterizer);
                let image = preview.render(ice, [0..size[0], 0..size[1]], rasterizer);
                self.preview = Some(preview);
                if let Some((old, texture)) = &mut self.image {
                    texture.set(image, egui::TextureOptions::LINEAR);
                    *old = key;
                } else {
                    self.image = Some((
                        key,
                        ctx.load_texture("canvas_minimap", image, egui::TextureOptions::LINEAR),
                    ));
                }
            } else {
                let preview = self.preview.as_mut().expect("minimap preview exists");
                let (old, texture) = self.image.as_mut().expect("minimap image exists");
                if let Some(region) = preview.update(&doc.canvas, drag, ice, rasterizer) {
                    let position = [region[0].start, region[1].start];
                    texture.set_partial(
                        position,
                        preview.render(ice, region, rasterizer),
                        egui::TextureOptions::LINEAR,
                    );
                }
                *old = key;
            }
        }
        let texture = &self.image.as_ref().expect("minimap image exists").1;
        widget.painter.image(
            texture.id(),
            widget.response.rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
        let viewport_size = scroll.inner_rect.size().min(canvas_size).max(Vec2::ZERO);
        let max_offset = (canvas_size - viewport_size).max(Vec2::ZERO);
        let view = Viewport {
            map: widget.response.rect,
            canvas_size,
            size: viewport_size,
            offset: scroll.state.clamped_offset().clamp(Vec2::ZERO, max_offset),
            scroll_id: scroll.id,
        };
        let indicator = view.indicator();
        widget.painter.rect_stroke(
            indicator,
            0.0,
            (2.0, Color32::BLACK),
            egui::StrokeKind::Inside,
        );
        widget.painter.rect_stroke(
            indicator,
            0.0,
            (1.0, Color32::WHITE),
            egui::StrokeKind::Inside,
        );
        self.viewport = Some(view);
    }

    pub(super) fn hide(&mut self) {
        self.viewport = None;
        self.drag_offset = None;
    }

    pub(super) fn invalidate(&mut self) {
        self.image = None;
        self.preview = None;
        self.hide();
    }
}

fn texture_size(canvas: &Canvas, pixels_per_point: f32) -> [usize; 2] {
    let native_size = Vec2::new(
        canvas.width() as f32 * glyph::CELL_SIZE.x,
        canvas.height() as f32 * glyph::CELL_SIZE.y,
    );
    // Render for the widest panel once; resizing only scales the cached GPU texture.
    let panel_scale = (MAX_PANEL_WIDTH / native_size.x).min(1.0);
    let desired = (native_size * panel_scale * pixels_per_point).max(Vec2::splat(1.0));
    let scale = 1.0_f32
        .min(MAX_TEXTURE_SIDE as f32 / desired.x)
        .min(MAX_TEXTURE_SIDE as f32 / desired.y)
        .min((MAX_TEXTURE_PIXELS as f32 / (desired.x * desired.y)).sqrt());
    let width = ((desired.x * scale).round() as usize).clamp(1, MAX_TEXTURE_SIDE);
    let height = ((desired.y * scale).round() as usize)
        .clamp(1, MAX_TEXTURE_SIDE)
        .min(MAX_TEXTURE_PIXELS / width);
    [width, height]
}

struct Glyph {
    pixels: [u8; glyph::WIDTH * glyph::HEIGHT],
    coverage: u32,
    underlined_coverage: u32,
}

struct Rasterizer {
    fonts: Fonts,
    options: TextOptions,
    glyphs: HashMap<char, Glyph>,
}

impl Rasterizer {
    fn new(definitions: egui::FontDefinitions) -> Self {
        let options = TextOptions {
            max_texture_side: 4096,
            color_transfer_function: FontColorTransferFunction::Gamma(0.55),
            ..Default::default()
        };
        Self {
            fonts: Fonts::new(options, definitions),
            options,
            glyphs: HashMap::new(),
        }
    }

    fn glyph(&mut self, ch: char) -> &Glyph {
        if !self.glyphs.contains_key(&ch) {
            if self.glyphs.len() == 512 {
                self.glyphs.clear();
                self.fonts.begin_pass(self.options, Default::default());
            }
            let pixels = glyph::rasterize_for_display(ch, &mut self.fonts);
            let coverage = pixels.iter().map(|&alpha| u32::from(alpha)).sum();
            let underline = &pixels[(glyph::HEIGHT - 2) * glyph::WIDTH..][..glyph::WIDTH];
            let underlined_coverage = coverage + 255 * glyph::WIDTH as u32
                - underline.iter().map(|&a| u32::from(a)).sum::<u32>();
            self.glyphs.insert(
                ch,
                Glyph {
                    pixels,
                    coverage,
                    underlined_coverage,
                },
            );
        }
        self.glyphs.get(&ch).expect("glyph was cached")
    }

    fn sample(&mut self, cell: Cell, ice: bool, x: usize, y: usize) -> [u8; 3] {
        let (fg, bg) = cell_colors(cell, ice);
        let fg = display_color(fg).to_array();
        let bg = display_color(bg).to_array();
        let alpha = if cell.attrs.underline && y == glyph::HEIGHT - 2 {
            255
        } else {
            u32::from(self.glyph(cell.ch).pixels[y * glyph::WIDTH + x])
        };
        std::array::from_fn(|channel| {
            ((u32::from(fg[channel]) * alpha + u32::from(bg[channel]) * (255 - alpha) + 127) / 255)
                as u8
        })
    }

    fn average(&mut self, cell: Cell, ice: bool) -> [u8; 3] {
        let (fg, bg) = cell_colors(cell, ice);
        let fg = display_color(fg).to_array();
        let bg = display_color(bg).to_array();
        if fg == bg {
            return [fg[0], fg[1], fg[2]];
        }
        let glyph = self.glyph(cell.ch);
        let alpha = if cell.attrs.underline {
            glyph.underlined_coverage
        } else {
            glyph.coverage
        };
        let total = (glyph::WIDTH * glyph::HEIGHT * 255) as u32;
        std::array::from_fn(|channel| {
            ((u32::from(fg[channel]) * alpha
                + u32::from(bg[channel]) * (total - alpha)
                + total / 2)
                / total) as u8
        })
    }
}

fn displayed_cell(canvas: &Canvas, drag: Option<&SelectionDrag>, x: usize, y: usize) -> Cell {
    drag.and_then(|drag| drag.preview_cell(x, y))
        .unwrap_or(*canvas.get(x, y).expect("minimap samples within the canvas"))
}

#[cfg(test)]
fn render_image(
    canvas: &Canvas,
    drag: Option<&SelectionDrag>,
    ice: bool,
    size: [usize; 2],
    rasterizer: &mut Rasterizer,
) -> egui::ColorImage {
    Preview::new(canvas, drag, ice, size, rasterizer).render(
        ice,
        [0..size[0], 0..size[1]],
        rasterizer,
    )
}

struct Preview {
    dimensions: [usize; 2],
    size: [usize; 2],
    cells: Vec<Cell>,
    averages: Option<Vec<[u8; 3]>>,
}

impl Preview {
    fn new(
        canvas: &Canvas,
        drag: Option<&SelectionDrag>,
        ice: bool,
        size: [usize; 2],
        rasterizer: &mut Rasterizer,
    ) -> Self {
        let width = canvas.width();
        let height = canvas.height();
        let mut cells = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                cells.push(displayed_cell(canvas, drag, x, y));
            }
        }
        let averages = (size[0] < width || size[1] < height).then(|| {
            cells
                .iter()
                .map(|&cell| rasterizer.average(cell, ice))
                .collect()
        });
        Self {
            dimensions: [width, height],
            size,
            cells,
            averages,
        }
    }

    fn update(
        &mut self,
        canvas: &Canvas,
        drag: Option<&SelectionDrag>,
        ice: bool,
        rasterizer: &mut Rasterizer,
    ) -> Option<[Range<usize>; 2]> {
        let [width, height] = self.dimensions;
        let mut left = width;
        let mut top = height;
        let mut right = 0;
        let mut bottom = 0;
        for y in 0..height {
            for x in 0..width {
                let index = y * width + x;
                let cell = displayed_cell(canvas, drag, x, y);
                if cell != self.cells[index] {
                    self.cells[index] = cell;
                    if let Some(averages) = &mut self.averages {
                        averages[index] = rasterizer.average(cell, ice);
                    }
                    left = left.min(x);
                    top = top.min(y);
                    right = right.max(x + 1);
                    bottom = bottom.max(y + 1);
                }
            }
        }
        // Include every preview pixel overlapping a changed cell, even across fractional boundaries.
        (right > left).then(|| {
            [
                left * self.size[0] / width..(right * self.size[0]).div_ceil(width),
                top * self.size[1] / height..(bottom * self.size[1]).div_ceil(height),
            ]
        })
    }

    fn render(
        &self,
        ice: bool,
        region: [Range<usize>; 2],
        rasterizer: &mut Rasterizer,
    ) -> egui::ColorImage {
        let [width, height] = self.dimensions;
        let size = self.size;
        let [xs, ys] = region;
        let mut image = egui::ColorImage::filled([xs.len(), ys.len()], Color32::BLACK);
        for y in ys.clone() {
            for x in xs.clone() {
                let rgb: [u8; 3] = if let Some(averages) = &self.averages {
                    let left = x * width;
                    let right = (x + 1) * width;
                    let top = y * height;
                    let bottom = (y + 1) * height;
                    let mut sum = [0_u64; 3];
                    for cy in top / size[1]..=(bottom - 1) / size[1] {
                        let overlap_y = bottom.min((cy + 1) * size[1]) - top.max(cy * size[1]);
                        for cx in left / size[0]..=(right - 1) / size[0] {
                            let overlap_x = right.min((cx + 1) * size[0]) - left.max(cx * size[0]);
                            let weight = (overlap_x * overlap_y) as u64;
                            let color = averages[cy * width + cx];
                            for channel in 0..3 {
                                sum[channel] += u64::from(color[channel]) * weight;
                            }
                        }
                    }
                    let total = (width * height) as u64;
                    std::array::from_fn(|channel| ((sum[channel] + total / 2) / total) as u8)
                } else {
                    let mut sum = [0_u32; 3];
                    for sy in 0..SAMPLES {
                        let source_y = (2 * (y * SAMPLES + sy) + 1) * height * glyph::HEIGHT
                            / (2 * SAMPLES * size[1]);
                        for sx in 0..SAMPLES {
                            let source_x = (2 * (x * SAMPLES + sx) + 1) * width * glyph::WIDTH
                                / (2 * SAMPLES * size[0]);
                            let cell = self.cells
                                [(source_y / glyph::HEIGHT) * width + source_x / glyph::WIDTH];
                            let color = rasterizer.sample(
                                cell,
                                ice,
                                source_x % glyph::WIDTH,
                                source_y % glyph::HEIGHT,
                            );
                            for channel in 0..3 {
                                sum[channel] += u32::from(color[channel]);
                            }
                        }
                    }
                    std::array::from_fn(|channel| {
                        ((sum[channel] + (SAMPLES * SAMPLES / 2) as u32)
                            / (SAMPLES * SAMPLES) as u32) as u8
                    })
                };
                image[(x - xs.start, y - ys.start)] = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
            }
        }
        image
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_export;
    use ansi_core::canvas::{Attributes, Color};

    fn definitions() -> egui::FontDefinitions {
        let mut definitions = egui::FontDefinitions::default();
        glyph::fonts(&mut definitions);
        definitions
    }

    #[test]
    fn partial_refresh_matches_full_render_at_cell_edges_and_during_selection_moves() {
        for size in [[63, 80], [29, 23], [3, 2], [1, 80], [80, 1]] {
            for ice in [false, true] {
                let mut canvas = Canvas::new(7, 5).unwrap();
                let cell = Cell {
                    ch: 'A',
                    fg: Color::Ansi(2),
                    bg: Color::Ansi(1),
                    attrs: Attributes {
                        bold: true,
                        blink: true,
                        inverse: true,
                        underline: true,
                    },
                };
                let drag = SelectionDrag {
                    source: Selection {
                        start: (0, 0),
                        end: (1, 0),
                    },
                    destination: Selection {
                        start: (5, 4),
                        end: (6, 4),
                    },
                    anchor: (0, 0),
                    cells: vec![cell; 2],
                };
                let mut rasterizer = Rasterizer::new(definitions());
                let mut preview = Preview::new(&canvas, None, ice, size, &mut rasterizer);
                let mut image = preview.render(ice, [0..size[0], 0..size[1]], &mut rasterizer);
                for (position, drag) in [
                    ((0, 0), None),
                    ((6, 4), None),
                    ((3, 2), None),
                    ((3, 2), Some(&drag)),
                    ((3, 2), None),
                ] {
                    canvas.set(position.0, position.1, cell);
                    let region = preview.update(&canvas, drag, ice, &mut rasterizer).unwrap();
                    let origin = [region[0].start, region[1].start];
                    let patch = preview.render(ice, region, &mut rasterizer);
                    for y in 0..patch.size[1] {
                        for x in 0..patch.size[0] {
                            image[(origin[0] + x, origin[1] + y)] = patch[(x, y)];
                        }
                    }
                    let full = render_image(&canvas, drag, ice, size, &mut rasterizer);
                    assert_eq!(
                        image, full,
                        "partial refresh differs at {position:?}, size {size:?}, iCE {ice}"
                    );
                }
            }
        }
    }

    #[test]
    fn native_size_preview_matches_rendered_artwork() {
        let mut canvas = Canvas::new(2, 2).unwrap();
        canvas.set(
            0,
            0,
            Cell {
                ch: 'A',
                fg: Color::Ansi(1),
                bg: Color::Rgb(3, 7, 11),
                attrs: Attributes {
                    bold: true,
                    underline: true,
                    ..Default::default()
                },
            },
        );
        canvas.set(
            1,
            1,
            Cell {
                ch: 'λ',
                fg: Color::Rgb(10, 90, 200),
                bg: Color::Ansi(2),
                attrs: Attributes {
                    inverse: true,
                    ..Default::default()
                },
            },
        );
        let doc = Document {
            canvas,
            sauce: None,
        };
        let fonts = definitions();
        let expected = image_export::render(&doc, None, fonts.clone()).unwrap();
        let mut rasterizer = Rasterizer::new(fonts);
        let preview = render_image(
            &doc.canvas,
            None,
            false,
            [2 * glyph::WIDTH, 2 * glyph::HEIGHT],
            &mut rasterizer,
        );
        for y in 0..expected.height() as usize {
            for x in 0..expected.width() as usize {
                assert_eq!(
                    &preview[(x, y)].to_array()[..3],
                    &expected.get_pixel(x as u32, y as u32).0
                );
            }
        }
    }

    #[test]
    fn downsampled_preview_includes_every_canvas_region() {
        let mut canvas = Canvas::new(2, 2).unwrap();
        for (x, y, color) in [
            (0, 0, Color::Rgb(255, 0, 0)),
            (1, 0, Color::Rgb(0, 255, 0)),
            (0, 1, Color::Rgb(0, 0, 255)),
            (1, 1, Color::Rgb(255, 255, 255)),
        ] {
            canvas.set(
                x,
                y,
                Cell {
                    ch: '█',
                    fg: color,
                    ..Default::default()
                },
            );
        }
        let image = render_image(
            &canvas,
            None,
            false,
            [1, 1],
            &mut Rasterizer::new(definitions()),
        );
        assert_eq!(image[(0, 0)], Color32::from_rgb(128, 128, 128));
    }

    #[test]
    fn narrow_previews_include_both_ends_of_the_canvas() {
        for (width, height, size) in [(16, 1, [4, 8]), (1, 16, [8, 4])] {
            let mut canvas = Canvas::new(width, height).unwrap();
            let position = |index| {
                if width == 1 {
                    (0, index)
                } else {
                    (index, 0)
                }
            };
            for index in [0, 1, 14, 15] {
                let (x, y) = position(index);
                canvas.set(
                    x,
                    y,
                    Cell {
                        ch: '█',
                        fg: if index < 2 {
                            Color::Rgb(255, 0, 0)
                        } else {
                            Color::Rgb(0, 255, 0)
                        },
                        ..Default::default()
                    },
                );
            }
            let image = render_image(
                &canvas,
                None,
                false,
                size,
                &mut Rasterizer::new(definitions()),
            );
            assert_eq!(image[(0, 0)], Color32::from_rgb(128, 0, 0));
            assert_eq!(
                image[(size[0] - 1, size[1] - 1)],
                Color32::from_rgb(0, 128, 0)
            );
        }
    }
}
