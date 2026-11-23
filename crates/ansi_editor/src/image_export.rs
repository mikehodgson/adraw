use crate::{cell_colors, display_color, glyph, Selection};
use ansi_core::ans_io::Document;
use eframe::egui::{epaint::text::Fonts, FontDefinitions};
use image::{codecs::jpeg::JpegEncoder, DynamicImage, ImageFormat, Rgb, RgbImage};
use std::{collections::BTreeMap, path::Path};

const MAX_PIXELS: usize = 32_000_000;

#[derive(Clone, Copy)]
pub enum Format {
    Png,
    Jpeg,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
        }
    }

    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Png => &["png"],
            Self::Jpeg => &["jpg", "jpeg"],
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
        }
    }
}

pub fn render(
    doc: &Document,
    selection: Option<Selection>,
    definitions: FontDefinitions,
) -> Result<RgbImage, String> {
    let (left, top, right, bottom) = selection.map_or(
        (0, 0, doc.canvas.width() - 1, doc.canvas.height() - 1),
        Selection::bounds,
    );
    if right >= doc.canvas.width() || bottom >= doc.canvas.height() {
        return Err("Selection is outside the canvas".into());
    }
    let width = (right - left + 1).checked_mul(glyph::WIDTH);
    let height = (bottom - top + 1).checked_mul(glyph::HEIGHT);
    let (width, height) = width
        .zip(height)
        .filter(|(w, h)| w.checked_mul(*h).is_some_and(|pixels| pixels <= MAX_PIXELS))
        .ok_or("Rendered image exceeds 32 megapixels. Select a smaller area to export.")?;
    let mut image = RgbImage::new(width as u32, height as u32);
    let fonts = Fonts::new(1.0, 4096, definitions);
    let mut masks = BTreeMap::new();
    let ice = doc
        .sauce
        .as_ref()
        .is_some_and(|sauce| sauce.flags() & 1 != 0);
    for y in top..=bottom {
        for x in left..=right {
            let cell = *doc.canvas.get(x, y).ok_or("Cell is outside the canvas")?;
            if !masks.contains_key(&cell.ch) {
                if masks.len() == 256 {
                    masks.clear();
                    fonts.begin_pass(1.0, 4096);
                }
                masks.insert(cell.ch, glyph::rasterize(cell.ch, &fonts)?);
            }
            let mask = &masks[&cell.ch];
            let (fg, bg) = cell_colors(cell, ice);
            let fg = display_color(fg).to_array();
            let bg = display_color(bg).to_array();
            for py in 0..glyph::HEIGHT {
                for px in 0..glyph::WIDTH {
                    let alpha = if cell.attrs.underline && py == glyph::HEIGHT - 2 {
                        255
                    } else {
                        u32::from(mask[py * glyph::WIDTH + px])
                    };
                    let pixel = Rgb(std::array::from_fn(|channel| {
                        ((u32::from(fg[channel]) * alpha
                            + u32::from(bg[channel]) * (255 - alpha)
                            + 127)
                            / 255) as u8
                    }));
                    image.put_pixel(
                        ((x - left) * glyph::WIDTH + px) as u32,
                        ((y - top) * glyph::HEIGHT + py) as u32,
                        pixel,
                    );
                }
            }
        }
    }
    Ok(image)
}

pub fn save(path: &Path, image: RgbImage, format: Format) -> Result<(), String> {
    let extension = path.extension().and_then(|extension| extension.to_str());
    if !extension.is_some_and(|extension| {
        format
            .extensions()
            .iter()
            .any(|allowed| extension.eq_ignore_ascii_case(allowed))
    }) {
        return Err(format!(
            "Choose a .{} filename for {} export.",
            format.extension(),
            format.label()
        ));
    }
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let parent = path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        match format {
            Format::Png => {
                DynamicImage::ImageRgb8(image).write_to(file.as_file_mut(), ImageFormat::Png)?
            }
            Format::Jpeg => {
                JpegEncoder::new_with_quality(file.as_file_mut(), 90).encode_image(&image)?
            }
        }
        file.as_file().sync_all()?;
        file.persist(path)?;
        Ok(())
    })();
    result.map_err(|error| format!("Could not export {} image: {error}", format.label()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ansi_core::{
        canvas::{Canvas, Cell, Color},
        sauce::Sauce,
    };

    #[test]
    fn native_rendering_preserves_colors_attributes_and_reverse_selections() {
        let mut doc = Document {
            canvas: Canvas::new(3, 2).unwrap(),
            sauce: Some(Sauce::default()),
        };
        let mut record = [0; 128];
        record[..7].copy_from_slice(b"SAUCE00");
        record[105] = 1;
        doc.sauce = ansi_core::sauce::parse(&record).unwrap().1;
        let block = Cell {
            ch: '▀',
            fg: Color::Rgb(12, 34, 56),
            bg: Color::Indexed(196),
            ..Cell::default()
        };
        assert!(doc.canvas.set(1, 1, block));
        let mut bright = Cell {
            ch: ' ',
            fg: Color::Ansi(1),
            bg: Color::Ansi(2),
            ..Cell::default()
        };
        bright.attrs.bold = true;
        bright.attrs.blink = true;
        bright.attrs.inverse = true;
        bright.attrs.underline = true;
        assert!(doc.canvas.set(2, 1, bright));
        let full = render(&doc, None, FontDefinitions::default()).unwrap();
        assert_eq!(full.dimensions(), (27, 32));
        assert_eq!(full.get_pixel(9, 16).0, [12, 34, 56]);
        assert_eq!(full.get_pixel(9, 24).0, [255, 0, 0]);
        assert_eq!(full.get_pixel(18, 16).0, [255, 85, 85]);
        assert_eq!(full.get_pixel(18, 30).0, [85, 255, 85]);
        let selected = render(
            &doc,
            Some(Selection {
                start: (2, 1),
                end: (1, 1),
            }),
            FontDefinitions::default(),
        )
        .unwrap();
        assert_eq!(selected.dimensions(), (18, 16));
        assert_eq!(
            selected,
            image::imageops::crop_imm(&full, 9, 16, 18, 16).to_image()
        );
        assert!(render(
            &doc,
            Some(Selection {
                start: (3, 0),
                end: (1, 1)
            }),
            FontDefinitions::default()
        )
        .is_err());
        let large = Document {
            canvas: Canvas::new(1000, 1000).unwrap(),
            sauce: None,
        };
        assert!(render(&large, None, FontDefinitions::default())
            .unwrap_err()
            .contains("32 megapixels"));
    }

    #[test]
    fn png_and_jpeg_are_valid_images_and_failed_exports_preserve_files() {
        let directory = tempfile::tempdir().unwrap();
        let pixels = RgbImage::from_pixel(18, 32, Rgb([24, 96, 192]));
        for format in [Format::Png, Format::Jpeg] {
            let path = directory.path().join(format!("art.{}", format.extension()));
            std::fs::write(&path, b"previous file").unwrap();
            save(&path, pixels.clone(), format).unwrap();
            let decoded = image::open(&path).unwrap().to_rgb8();
            assert_eq!(decoded.dimensions(), pixels.dimensions());
            match format {
                Format::Png => assert_eq!(decoded, pixels),
                Format::Jpeg => {
                    for (actual, expected) in
                        decoded.get_pixel(9, 16).0.into_iter().zip([24, 96, 192])
                    {
                        assert!(actual.abs_diff(expected) <= 3);
                    }
                }
            }
        }
        let original = directory.path().join("original.ans");
        std::fs::write(&original, b"artwork").unwrap();
        assert!(save(&original, pixels.clone(), Format::Png).is_err());
        assert_eq!(std::fs::read(&original).unwrap(), b"artwork");
        assert!(save(
            &directory.path().join("missing").join("art.png"),
            pixels,
            Format::Png
        )
        .is_err());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 3);
    }
}
