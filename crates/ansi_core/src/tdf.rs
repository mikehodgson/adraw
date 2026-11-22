//! TheDraw character-cell fonts, based on Roy/SAC's TDF format specification.

use crate::{
    canvas::{Attributes, Cell, Color, MAX_CELLS},
    encoding::decode_cp437,
    sauce,
};

pub const MAX_FILE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_FONTS: usize = 4096;
const MAX_GLYPH_CELLS: usize = 16 * 1024 * 1024;
const SIGNATURE: &[u8] = b"\x13TheDraw FONTS file\x1a";
const FONT_MARKER: &[u8] = b"\x55\xaa\x00\xff";
const HEADER_SIZE: usize = 213;
const DOS_COLORS: [u8; 16] = [0, 4, 2, 6, 1, 5, 3, 7, 8, 12, 10, 14, 9, 13, 11, 15];
const OUTLINE: &[u8] = &[
    205, 196, 179, 186, 213, 187, 214, 191, 200, 190, 192, 189, 181, 199,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontKind {
    Outline,
    Block,
    Color,
}

#[derive(Clone, Copy, Debug)]
struct FontCell {
    ch: u8,
    attribute: u8,
}

#[derive(Debug)]
struct Glyph {
    width: usize,
    line_height: usize,
    cells: Vec<Option<FontCell>>,
}

#[derive(Debug)]
pub struct Font {
    pub name: String,
    pub kind: FontKind,
    pub spacing: usize,
    line_height: usize,
    glyphs: Vec<Option<Glyph>>,
}

#[derive(Debug)]
pub struct RenderedText {
    pub width: usize,
    pub height: usize,
    pub cells: Vec<(usize, usize, Cell)>,
}

fn glyph(data: &[u8], kind: FontKind) -> Result<Glyph, String> {
    let header = data.get(..2).ok_or("Truncated glyph dimensions")?;
    let width = usize::from(header[0]);
    let line_height = usize::from(header[1]);
    if !(1..=30).contains(&width) || line_height > 12 {
        return Err("Invalid glyph dimensions (maximum 30 x 12)".into());
    }
    let mut cells = vec![None; width];
    let (mut x, mut y, mut pos) = (0, 0, 2);
    loop {
        let ch = *data.get(pos).ok_or("Unterminated glyph")?;
        pos += 1;
        match ch {
            0 => {
                if x == 0 && y > 0 {
                    cells.truncate(y * width);
                }
                return Ok(Glyph {
                    width,
                    line_height,
                    cells,
                });
            }
            13 => {
                x = 0;
                y += 1;
                // Declared height excludes descender rows; don't truncate at it.
                if y >= 24 {
                    return Err("Glyph exceeds 24 rows including descenders".into());
                }
                cells.resize((y + 1) * width, None);
            }
            b'&' if kind != FontKind::Color || matches!(data.get(pos), Some(0 | 13)) => {}
            _ => {
                let attribute = if kind == FontKind::Color {
                    let value = *data.get(pos).ok_or("Missing color attribute")?;
                    pos += 1;
                    value
                } else {
                    0
                };
                if x >= width {
                    return Err("Glyph row exceeds its declared width".into());
                }
                if ch != b' ' {
                    cells[y * width + x] = Some(FontCell { ch, attribute });
                }
                x += 1;
            }
        }
    }
}

/// Load all fonts in a TDF collection, rejecting malformed data without partial imports.
pub fn load(bytes: &[u8]) -> Result<Vec<Font>, String> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err("TDF file exceeds 32 MiB".into());
    }
    let (bytes, _) = sauce::parse(bytes)?;
    if !bytes.starts_with(SIGNATURE) {
        return Err("Not a TheDraw TDF font file".into());
    }
    let mut pos = SIGNATURE.len();
    let mut fonts = Vec::new();
    let mut cell_count = 0usize;
    while pos < bytes.len() {
        if bytes[pos] == 0 {
            if bytes[pos..].iter().any(|&b| b != 0) {
                return Err("Unexpected data after the TDF collection terminator".into());
            }
            break;
        }
        if fonts.len() == MAX_FONTS {
            return Err("TDF collection exceeds 4096 fonts".into());
        }
        let header = bytes
            .get(pos..pos + HEADER_SIZE)
            .ok_or("Truncated font header")?;
        if &header[..4] != FONT_MARKER {
            return Err(format!("Invalid font marker at byte {pos}"));
        }
        let name_len = usize::from(header[4]);
        // Some old tools use the four reserved bytes for longer names.
        if name_len > 16 {
            return Err("TDF font name exceeds 16 bytes".into());
        }
        let name: String = header[5..5 + name_len]
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| decode_cp437(b))
            .collect();
        let kind = match header[21] {
            0 => FontKind::Outline,
            1 => FontKind::Block,
            2 => FontKind::Color,
            value => return Err(format!("Unsupported TDF font type {value}")),
        };
        if header[22] > 41 {
            return Err(format!("Invalid letter spacing in font {name}"));
        }
        let size = usize::from(u16::from_le_bytes([header[23], header[24]]));
        pos += HEADER_SIZE;
        let data = bytes
            .get(pos..pos + size)
            .ok_or("Truncated font data block")?;
        let mut glyphs = Vec::with_capacity(94);
        let mut line_height = 1;
        let mut offsets: Vec<usize> = header[25..]
            .chunks_exact(2)
            .map(|offset| u16::from_le_bytes([offset[0], offset[1]]))
            .filter(|&offset| offset != u16::MAX)
            .map(usize::from)
            .collect();
        offsets.sort_unstable();
        offsets.dedup();
        for (index, offset) in header[25..].chunks_exact(2).enumerate() {
            let offset = u16::from_le_bytes([offset[0], offset[1]]);
            if offset == u16::MAX {
                glyphs.push(None);
                continue;
            }
            let start = usize::from(offset);
            let end = offsets
                .iter()
                .copied()
                .find(|&next| next > start)
                .unwrap_or(data.len());
            let parsed = data
                .get(start..end)
                .ok_or_else(|| "Glyph offset is outside the font data".to_owned())
                .and_then(|data| glyph(data, kind))
                .map_err(|error| {
                    format!(
                        "Font {name}, character {:?}: {error}",
                        char::from(b'!' + index as u8)
                    )
                })?;
            cell_count += parsed.cells.len();
            if cell_count > MAX_GLYPH_CELLS {
                return Err("TDF collection exceeds the decoded glyph cell limit".into());
            }
            line_height = line_height.max(parsed.line_height);
            glyphs.push(Some(parsed));
        }
        fonts.push(Font {
            name,
            kind,
            spacing: usize::from(header[22].saturating_sub(1)),
            line_height,
            glyphs,
        });
        pos += size;
    }
    if fonts.is_empty() {
        return Err("TDF file contains no fonts".into());
    }
    Ok(fonts)
}

impl Font {
    pub fn render(&self, text: &str, brush: Cell, spacing: usize) -> Result<RenderedText, String> {
        if spacing > 40 || text.chars().count() > 4096 {
            return Err("Font text permits at most 4096 characters and spacing 0..=40".into());
        }
        let mut result = RenderedText {
            width: 0,
            height: self.line_height,
            cells: Vec::new(),
        };
        let (mut x, mut line_y) = (0usize, 0usize);
        for ch in text.replace("\r\n", "\n").chars() {
            match ch {
                '\n' => {
                    x = 0;
                    line_y += self.line_height + 1;
                    result.height = result.height.max(line_y + self.line_height);
                }
                ' ' => {
                    x += (self.line_height / 2).max(1) + spacing;
                    result.width = result.width.max(x);
                }
                '!'..='~' => {
                    let glyph = self.glyphs[ch as usize - '!' as usize]
                        .as_ref()
                        .ok_or_else(|| {
                            format!("Character {ch:?} is not defined in font {}", self.name)
                        })?;
                    let y = line_y + self.line_height - glyph.line_height;
                    result.width = result.width.max(x + glyph.width);
                    result.height = result.height.max(y + glyph.cells.len() / glyph.width);
                    for (index, pixel) in glyph.cells.iter().enumerate() {
                        if let Some(pixel) = pixel {
                            let cell = self.cell(*pixel, brush);
                            result.cells.push((
                                x + index % glyph.width,
                                y + index / glyph.width,
                                cell,
                            ));
                        }
                    }
                    x += glyph.width + spacing;
                }
                _ => {
                    return Err(format!(
                        "Character {ch:?} is not supported by TDF fonts (ASCII only)"
                    ))
                }
            }
            if result
                .width
                .checked_mul(result.height)
                .is_none_or(|n| n > MAX_CELLS)
            {
                return Err(format!("Rendered font text exceeds {MAX_CELLS} cells"));
            }
        }
        if result.cells.is_empty() {
            return Err("Enter text containing defined, visible font characters".into());
        }
        Ok(result)
    }

    fn cell(&self, pixel: FontCell, brush: Cell) -> Cell {
        let ch = if pixel.ch == 255 {
            ' '
        } else if self.kind == FontKind::Outline {
            match pixel.ch {
                b'A'..=b'N' => decode_cp437(OUTLINE[usize::from(pixel.ch - b'A')]),
                b'O'..=b'R' | b'@' => ' ',
                _ => decode_cp437(pixel.ch),
            }
        } else {
            decode_cp437(pixel.ch)
        };
        if self.kind == FontKind::Color {
            Cell {
                ch,
                fg: Color::Ansi(DOS_COLORS[usize::from(pixel.attribute & 15)]),
                bg: Color::Ansi(DOS_COLORS[usize::from((pixel.attribute >> 4) & 7)]),
                attrs: Attributes {
                    blink: pixel.attribute & 128 != 0,
                    ..Attributes::default()
                },
            }
        } else {
            Cell { ch, ..brush }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font_block(kind: u8, name: &[u8], glyphs: &[(u8, &[u8])]) -> Vec<u8> {
        let mut header = vec![0; HEADER_SIZE];
        header[..4].copy_from_slice(FONT_MARKER);
        header[4] = name.len() as u8;
        header[5..5 + name.len()].copy_from_slice(name);
        header[21] = kind;
        header[22] = 2;
        header[25..].fill(255);
        let mut data = Vec::new();
        for &(ch, glyph) in glyphs {
            let index = 25 + usize::from(ch - b'!') * 2;
            header[index..index + 2].copy_from_slice(&(data.len() as u16).to_le_bytes());
            data.extend_from_slice(glyph);
        }
        header[23..25].copy_from_slice(&(data.len() as u16).to_le_bytes());
        header.extend(data);
        header
    }

    #[test]
    fn collection_types_colors_transparency_and_cp437_round_trip() {
        let mut data = SIGNATURE.to_vec();
        data.extend(font_block(
            1,
            b"Bl\x82ck",
            &[(b'A', &[3, 1, 219, 32, 255, 0])],
        ));
        data.extend(font_block(
            2,
            b"Color",
            &[(b'A', &[3, 1, 219, 0x91, 32, 0x40, 255, 0x24, 0])],
        ));
        data.extend(font_block(
            0,
            b"Outline",
            &[(b'A', &[3, 1, b'E', b'A', b'F', 0])],
        ));
        let fonts = load(&data).unwrap();
        assert_eq!(fonts.len(), 3);
        assert_eq!(fonts[0].name, "Bléck");
        let brush = Cell {
            fg: Color::Rgb(2, 3, 4),
            ..Cell::default()
        };
        let block = fonts[0].render("A", brush, 0).unwrap();
        assert_eq!(block.width, 3);
        assert_eq!(block.cells.len(), 2);
        assert_eq!(block.cells[0].2, Cell { ch: '█', ..brush });
        assert_eq!(block.cells[1], (2, 0, Cell { ch: ' ', ..brush }));
        let color = fonts[1].render("A", brush, 0).unwrap();
        assert_eq!(color.cells[0].2.fg, Color::Ansi(4));
        assert_eq!(color.cells[0].2.bg, Color::Ansi(4));
        assert!(color.cells[0].2.attrs.blink);
        assert_eq!(color.cells[1].2.bg, Color::Ansi(2));
        let outline = fonts[2].render("A", brush, 0).unwrap();
        assert_eq!(
            outline.cells.iter().map(|p| p.2.ch).collect::<String>(),
            "╒═╗"
        );
    }

    #[test]
    fn baseline_descenders_spacing_and_missing_characters() {
        let mut data = SIGNATURE.to_vec();
        data.extend(font_block(
            1,
            b"Test",
            &[
                (b'A', &[1, 2, b'X', 13, b'X', 0]),
                (b'g', &[1, 2, b'X', 13, b'X', 13, b'X', b'&', 0]),
                (b'.', &[1, 1, b'.', 0]),
            ],
        ));
        let font = load(&data).unwrap().remove(0);
        let rendered = font
            .render("A.g\nA", Cell::default(), font.spacing)
            .unwrap();
        assert_eq!((rendered.width, rendered.height), (5, 5));
        assert!(rendered
            .cells
            .iter()
            .any(|p| p.0 == 2 && p.1 == 1 && p.2.ch == '.'));
        assert!(rendered.cells.iter().any(|p| p.0 == 4 && p.1 == 2));
        assert!(rendered.cells.iter().any(|p| p.0 == 0 && p.1 == 3));
        assert!(font
            .render("B", Cell::default(), 0)
            .unwrap_err()
            .contains("not defined"));
        assert!(font.render("é", Cell::default(), 0).is_err());
        assert!(font.render(&"A".repeat(4097), Cell::default(), 0).is_err());
    }

    #[test]
    fn rejects_truncation_invalid_offsets_and_unterminated_glyphs() {
        let mut valid = SIGNATURE.to_vec();
        valid.extend(font_block(2, b"Test", &[(b'A', &[1, 1, 219, 0, 0])]));
        for length in 0..valid.len() {
            assert!(
                load(&valid[..length]).is_err(),
                "accepted truncation at {length}"
            );
        }
        let index = SIGNATURE.len() + 25 + usize::from(b'A' - b'!') * 2;
        let mut invalid = valid.clone();
        invalid[index..index + 2].copy_from_slice(&1000u16.to_le_bytes());
        assert!(load(&invalid).is_err());
        *valid.last_mut().unwrap() = 13;
        assert!(load(&valid).is_err());
        let mut missing_terminator = SIGNATURE.to_vec();
        missing_terminator.extend(font_block(
            1,
            b"Test",
            &[(b'A', &[30, 1, b'X']), (b'B', &[1, 1, b'Y', 0])],
        ));
        assert!(load(&missing_terminator).is_err());
    }

    #[test]
    fn accepts_alias_offsets_sauce_trailers_and_literal_colored_ampersands() {
        let mut data = SIGNATURE.to_vec();
        data.extend(font_block(2, b"Test", &[(b'A', &[1, 1, b'&', 8, 0])]));
        let a = SIGNATURE.len() + 25 + usize::from(b'A' - b'!') * 2;
        let offset = [data[a], data[a + 1]];
        data[a + 2..a + 4].copy_from_slice(&offset);
        sauce::append(&mut data, &sauce::Sauce::default(), 80, 25).unwrap();
        let font = load(&data).unwrap().remove(0);
        let rendered = font.render("AB", Cell::default(), 0).unwrap();
        assert_eq!(
            rendered.cells.iter().map(|p| p.2.ch).collect::<String>(),
            "&&"
        );
    }

    #[test]
    #[ignore = "requires an extracted TDF collection in ADRAW_TDF_TEST_DIR"]
    fn external_collection_compatibility() {
        let root = std::env::var_os("ADRAW_TDF_TEST_DIR").expect("Set ADRAW_TDF_TEST_DIR");
        let mut directories = vec![std::path::PathBuf::from(root)];
        let (mut files, mut accepted, mut font_count) = (0, 0, 0);
        while let Some(directory) = directories.pop() {
            for entry in std::fs::read_dir(directory).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    directories.push(path);
                } else if path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("tdf"))
                {
                    files += 1;
                    match load(&std::fs::read(&path).unwrap()) {
                        Ok(fonts) => {
                            accepted += 1;
                            font_count += fonts.len();
                            for font in fonts {
                                let text: String = font
                                    .glyphs
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, glyph)| {
                                        glyph
                                            .as_ref()
                                            .is_some_and(|g| g.cells.iter().any(Option::is_some))
                                    })
                                    .map(|(index, _)| char::from(b'!' + index as u8))
                                    .collect();
                                if !text.is_empty() {
                                    font.render(&text, Cell::default(), font.spacing).unwrap();
                                }
                            }
                        }
                        Err(error) => eprintln!("{}: {error}", path.display()),
                    }
                }
            }
        }
        println!("TDF compatibility: {accepted}/{files} files, {font_count} fonts rendered");
        assert!(files > 0);
        assert!(
            accepted * 100 >= files * 95,
            "More than 5% of the collection was rejected"
        );
    }
}
