use ansi_core::encoding::encode_cp437;
use eframe::egui::{
    Color32, FontData, FontDefinitions, FontFamily, Mesh, Painter, Pos2, Rect, Shape, Vec2,
};

pub const WIDTH: usize = 9;
pub const HEIGHT: usize = 16;
const BITMAP: &[u8; 256 * HEIGHT * 2] = include_bytes!("../assets/ibm-vga-9x16.bin");
pub const FONT_NAME: &str = "ibm-vga-9x16";
pub const CELL_SIZE: Vec2 = Vec2::new(WIDTH as f32, HEIGHT as f32);

pub fn fonts(fonts: &mut FontDefinitions) {
    fonts.font_data.insert(
        FONT_NAME.into(),
        FontData::from_static(include_bytes!("../assets/Px437_IBM_VGA_9x16.ttf")).into(),
    );
    let mut family = vec![FONT_NAME.into()];
    family.extend(fonts.families[&FontFamily::Monospace].iter().cloned());
    fonts
        .families
        .insert(FontFamily::Name(FONT_NAME.into()), family);
}

fn snap(value: f32, pixels_per_point: f32) -> f32 {
    (value * pixels_per_point).round() / pixels_per_point
}

pub fn cell_rect(origin: Pos2, x: usize, y: usize, size: Vec2, pixels_per_point: f32) -> Rect {
    Rect::from_min_max(
        Pos2::new(
            snap(origin.x + x as f32 * size.x, pixels_per_point),
            snap(origin.y + y as f32 * size.y, pixels_per_point),
        ),
        Pos2::new(
            snap(origin.x + (x + 1) as f32 * size.x, pixels_per_point),
            snap(origin.y + (y + 1) as f32 * size.y, pixels_per_point),
        ),
    )
}

fn row(byte: u8, y: usize) -> u16 {
    let offset = (byte as usize * HEIGHT + y) * 2;
    u16::from_le_bytes([BITMAP[offset], BITMAP[offset + 1]])
}

pub fn rasterize(
    ch: char,
    fonts: &mut eframe::egui::epaint::text::Fonts,
) -> Result<[u8; WIDTH * HEIGHT], String> {
    let font = eframe::egui::FontId::monospace(16.0);
    if encode_cp437(ch).is_none() && !fonts.has_glyph(&font, ch) {
        return Err(format!(
            "Cannot render '{ch}' (U+{:04X}): no installed display font contains this character.",
            ch as u32
        ));
    }
    Ok(rasterize_for_display(ch, fonts))
}

pub fn rasterize_for_display(
    ch: char,
    fonts: &mut eframe::egui::epaint::text::Fonts,
) -> [u8; WIDTH * HEIGHT] {
    let mut mask = [0; WIDTH * HEIGHT];
    if let Some(byte) = encode_cp437(ch) {
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                if row(byte, y) & (1 << (WIDTH - 1 - x)) != 0 {
                    mask[y * WIDTH + x] = 255;
                }
            }
        }
        return mask;
    }
    let font = eframe::egui::FontId::monospace(16.0);
    let galley =
        fonts
            .with_pixels_per_point(1.0)
            .layout_no_wrap(ch.to_string(), font, Color32::WHITE);
    let origin = ((CELL_SIZE - galley.size()) / 2.0).round();
    let image = fonts.texture_atlas().image();
    for row in &galley.rows {
        for glyph in &row.glyphs {
            let uv = glyph.uv_rect;
            if uv.is_nothing() {
                continue;
            }
            let top_left = (glyph.pos + row.pos.to_vec2() + uv.offset).round() + origin;
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    let local = Pos2::new(x as f32 + 0.5, y as f32 + 0.5) - top_left;
                    if local.x >= 0.0
                        && local.y >= 0.0
                        && local.x < uv.size.x
                        && local.y < uv.size.y
                    {
                        let tx = usize::from(uv.min[0]) + local.x.floor() as usize;
                        let ty = usize::from(uv.min[1]) + local.y.floor() as usize;
                        mask[y * WIDTH + x] = mask[y * WIDTH + x].max(image[(tx, ty)].a());
                    }
                }
            }
        }
    }
    mask
}

fn rectangles(ch: char, rect: Rect, pixels_per_point: f32) -> Option<Vec<Rect>> {
    let byte = encode_cp437(ch)?;
    let mut result = Vec::new();
    for y in 0..HEIGHT {
        let bits = row(byte, y);
        let mut x = 0;
        while x < WIDTH {
            if bits & (1 << (WIDTH - 1 - x)) == 0 {
                x += 1;
                continue;
            }
            let start = x;
            while x < WIDTH && bits & (1 << (WIDTH - 1 - x)) != 0 {
                x += 1;
            }
            let area = Rect::from_min_max(
                Pos2::new(
                    snap(
                        rect.left() + start as f32 * rect.width() / WIDTH as f32,
                        pixels_per_point,
                    ),
                    snap(
                        rect.top() + y as f32 * rect.height() / HEIGHT as f32,
                        pixels_per_point,
                    ),
                ),
                Pos2::new(
                    snap(
                        rect.left() + x as f32 * rect.width() / WIDTH as f32,
                        pixels_per_point,
                    ),
                    snap(
                        rect.top() + (y + 1) as f32 * rect.height() / HEIGHT as f32,
                        pixels_per_point,
                    ),
                ),
            );
            if area.is_positive() {
                result.push(area);
            }
        }
    }
    Some(result)
}

pub fn paint(painter: &Painter, ch: char, rect: Rect, color: Color32) -> bool {
    let Some(rectangles) = rectangles(ch, rect, painter.ctx().pixels_per_point()) else {
        return false;
    };
    if !rectangles.is_empty() {
        // Unfeathered meshes retain the font's bitmap edges and one-pixel details.
        let mut mesh = Mesh::default();
        for area in rectangles {
            mesh.add_colored_rect(area, color);
        }
        painter.add(Shape::mesh(mesh));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use ansi_core::encoding::decode_cp437;

    #[test]
    fn exported_glyphs_match_bitmap_geometry_and_support_unicode_fallback() {
        let mut definitions = FontDefinitions::default();
        fonts(&mut definitions);
        let mut fonts = eframe::egui::epaint::text::Fonts::new(Default::default(), definitions);
        let rect = Rect::from_min_size(Pos2::ZERO, CELL_SIZE);
        for byte in 0..=255 {
            let ch = decode_cp437(byte);
            let mask = rasterize(ch, &mut fonts).unwrap();
            let parts = rectangles(ch, rect, 1.0).unwrap();
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    assert_eq!(
                        mask[y * WIDTH + x] == 255,
                        parts
                            .iter()
                            .any(|part| part.contains(Pos2::new(x as f32 + 0.5, y as f32 + 0.5))),
                        "CP437 {byte:02X}, pixel {x},{y}",
                    );
                }
            }
        }
        for ch in ['€', 'λ'] {
            let mask = rasterize(ch, &mut fonts).unwrap();
            assert!(mask.iter().any(|alpha| *alpha > 0), "{ch}");
            assert!(mask.contains(&0), "{ch}");
        }
        assert!(rasterize('\u{10ffff}', &mut fonts)
            .unwrap_err()
            .contains("no installed display font"));
    }

    #[test]
    fn all_cp437_glyphs_render_the_embedded_bitmap_without_smoothing() {
        let rect = Rect::from_min_size(Pos2::ZERO, CELL_SIZE);
        for byte in 0..=255 {
            let parts = rectangles(decode_cp437(byte), rect, 1.0).unwrap();
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    let center = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
                    assert_eq!(
                        parts.iter().any(|part| part.contains(center)),
                        row(byte, y) & (1 << (WIDTH - 1 - x)) != 0,
                        "CP437 {byte:02X}, pixel {x},{y}"
                    );
                }
            }
        }
        assert!(rectangles('€', rect, 1.0).is_none());
        assert!(rectangles(' ', rect, 1.0).unwrap().is_empty());
        assert!(rectangles('\u{a0}', rect, 1.0).unwrap().is_empty());
        assert_eq!(
            (0..HEIGHT).map(|y| row(b'A', y)).collect::<Vec<_>>(),
            vec![
                0, 0, 0x20, 0x70, 0xd8, 0x18c, 0x18c, 0x1fc, 0x18c, 0x18c, 0x18c, 0x18c, 0, 0, 0, 0
            ]
        );
        assert_eq!(row(0xb0, 0), 0x22);
        assert_eq!(row(0xb1, 0), 0xaa);
        assert_eq!(row(0xb2, 0), 0x1ba);
    }

    #[test]
    fn adjoining_cells_and_vga_border_lanes_have_no_gap_at_dpi_scales() {
        for (scale, zoom) in [1.0, 1.25, 1.5, 2.0].into_iter().flat_map(|scale| {
            [0.5, 0.75, 1.0, 1.25, 1.49, 1.503, 2.0, 4.0].map(|zoom| (scale, zoom))
        }) {
            let origin = Pos2::new(0.3, 0.7);
            let size = CELL_SIZE * zoom;
            let a = cell_rect(origin, 0, 0, size, scale);
            let b = cell_rect(origin, 1, 0, size, scale);
            let c = cell_rect(origin, 0, 1, size, scale);
            assert_eq!(a.right(), b.left());
            assert_eq!(a.bottom(), c.top());
            for ch in ['─', '═'] {
                let ra = rectangles(ch, a, scale).unwrap();
                let rb = rectangles(ch, b, scale).unwrap();
                for left in ra.iter().filter(|r| r.right() == a.right()) {
                    assert!(rb.iter().any(|right| {
                        left.right() == right.left() && left.y_range() == right.y_range()
                    }));
                }
            }
            for ch in ['│', '║'] {
                let ra = rectangles(ch, a, scale).unwrap();
                let rc = rectangles(ch, c, scale).unwrap();
                for top in ra.iter().filter(|r| r.bottom() == a.bottom()) {
                    assert!(rc.iter().any(|bottom| {
                        top.bottom() == bottom.top() && top.x_range() == bottom.x_range()
                    }));
                }
            }
            for rect in [a, b, c] {
                let blocks = rectangles('█', rect, scale).unwrap();
                let bounds = blocks
                    .iter()
                    .fold(Rect::NOTHING, |bounds, part| bounds.union(*part));
                assert_eq!(bounds, rect);
                let pixels = |area: &Rect| {
                    ((area.right() * scale).round() - (area.left() * scale).round()) as i32
                        * ((area.bottom() * scale).round() - (area.top() * scale).round()) as i32
                };
                assert_eq!(blocks.iter().map(pixels).sum::<i32>(), pixels(&rect));
            }
        }
    }
}
