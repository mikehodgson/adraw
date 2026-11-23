use crate::canvas::{Attributes, Canvas, Cell, Color, DEFAULT_WIDTH, MAX_CELLS};
use crate::encoding::{decode_cp437, encode_char, Encoding};
use crate::sauce::{self, Sauce};

pub struct Document {
    pub canvas: Canvas,
    pub sauce: Option<Sauce>,
}

fn number(params: &[u16], index: usize, default: usize) -> usize {
    params
        .get(index)
        .copied()
        .filter(|v| *v != 0)
        .map(usize::from)
        .unwrap_or(default)
}

fn sgr(cell: &mut Cell, params: &[u16]) -> Result<(), String> {
    let mut i = 0;
    while i < params.len() {
        let p = params[i];
        match p {
            0 => {
                let ch = cell.ch;
                *cell = Cell::default();
                cell.ch = ch;
            }
            1 => cell.attrs.bold = true,
            4 => cell.attrs.underline = true,
            5 => cell.attrs.blink = true,
            7 => cell.attrs.inverse = true,
            22 => cell.attrs.bold = false,
            24 => cell.attrs.underline = false,
            25 => cell.attrs.blink = false,
            27 => cell.attrs.inverse = false,
            30..=37 => cell.fg = Color::Ansi((p - 30) as u8),
            40..=47 => cell.bg = Color::Ansi((p - 40) as u8),
            90..=97 => cell.fg = Color::Ansi((p - 90 + 8) as u8),
            100..=107 => cell.bg = Color::Ansi((p - 100 + 8) as u8),
            39 => cell.fg = Color::Ansi(7),
            49 => cell.bg = Color::Ansi(0),
            38 | 48 => {
                let mode = *params.get(i + 1).ok_or("incomplete extended color")?;
                let component = |n: usize| -> Result<u8, String> {
                    u8::try_from(*params.get(n).ok_or("incomplete extended color")?)
                        .map_err(|_| "color component exceeds 255".into())
                };
                let color = match mode {
                    5 => {
                        let c = Color::Indexed(component(i + 2)?);
                        i += 2;
                        c
                    }
                    2 => {
                        let c = Color::Rgb(component(i + 2)?, component(i + 3)?, component(i + 4)?);
                        i += 4;
                        c
                    }
                    _ => return Err(format!("unsupported extended color mode {mode}")),
                };
                if p == 38 {
                    cell.fg = color;
                } else {
                    cell.bg = color;
                }
            }
            _ => return Err(format!("unsupported SGR parameter {p}")),
        }
        i += 1;
    }
    Ok(())
}

/// Parse a byte stream into a bounded canvas. SAUCE dimensions override fallback dimensions.
pub fn load(
    bytes: &[u8],
    encoding: Encoding,
    width: usize,
    height: usize,
) -> Result<Document, String> {
    let (art, sauce) = sauce::parse(bytes)?;
    let (width, height) = sauce
        .as_ref()
        .and_then(Sauce::dimensions)
        .unwrap_or((width, height));
    parse_art(art, sauce, encoding, width, height, false)
}

/// Load artwork using SAUCE dimensions, or 80 columns and an inferred height.
pub fn load_auto(bytes: &[u8], encoding: Encoding) -> Result<Document, String> {
    let (art, sauce) = sauce::parse(bytes)?;
    let width = sauce
        .as_ref()
        .and_then(Sauce::width)
        .unwrap_or(DEFAULT_WIDTH);
    let height = sauce.as_ref().and_then(Sauce::height);
    parse_art(
        art,
        sauce,
        encoding,
        width,
        height.unwrap_or(1),
        height.is_none(),
    )
}

fn parse_art(
    art: &[u8],
    sauce: Option<Sauce>,
    encoding: Encoding,
    width: usize,
    height: usize,
    automatic_height: bool,
) -> Result<Document, String> {
    let mut canvas = Canvas::new(width, height).map_err(|e| e.to_string())?;
    let height = if automatic_height {
        MAX_CELLS / width
    } else {
        height
    };
    let mut used_height = 1;
    let bound_row = |row: usize| {
        if automatic_height && row >= height {
            Err(format!(
                "artwork exceeds the {MAX_CELLS}-cell limit at row {}",
                row + 1
            ))
        } else {
            Ok(row.min(height - 1))
        }
    };
    let (mut x, mut y, mut saved) = (0usize, 0usize, (0usize, 0usize));
    let mut style = Cell::default();
    let mut i = 0;
    while i < art.len() {
        match art[i] {
            0x1b => {
                if art.get(i + 1) != Some(&b'[') {
                    return Err(format!("unsupported escape at byte {i}"));
                }
                let start = i + 2;
                let mut end = start;
                while end < art.len() && !(0x40..=0x7e).contains(&art[end]) {
                    end += 1;
                }
                if end == art.len() {
                    return Err(format!("incomplete CSI at byte {i}"));
                }
                let raw =
                    std::str::from_utf8(&art[start..end]).map_err(|_| "invalid CSI parameters")?;
                if raw == "?25" && matches!(art[end], b'h' | b'l') {
                    i = end + 1;
                    continue;
                }
                let params: Vec<u16> = if raw.is_empty() {
                    vec![0]
                } else {
                    raw.split(';')
                        .map(|s| {
                            if s.is_empty() {
                                Ok(0)
                            } else {
                                s.parse::<u16>()
                                    .map_err(|_| format!("invalid CSI parameter: {s}"))
                            }
                        })
                        .collect::<Result<_, _>>()?
                };
                let n = number(&params, 0, 1);
                match art[end] {
                    b'm' => sgr(&mut style, &params)?,
                    b'H' | b'f' => {
                        y = bound_row(number(&params, 0, 1).saturating_sub(1))?;
                        x = number(&params, 1, 1).saturating_sub(1).min(width - 1);
                    }
                    b'A' => y = y.saturating_sub(n),
                    b'B' => y = bound_row(y.saturating_add(n))?,
                    b'C' => x = x.saturating_add(n).min(width - 1),
                    b'D' => x = x.saturating_sub(n).min(width - 1),
                    b'E' => {
                        y = bound_row(y.saturating_add(n))?;
                        x = 0;
                    }
                    b'F' => {
                        y = y.saturating_sub(n);
                        x = 0;
                    }
                    b'G' => x = n.saturating_sub(1).min(width - 1),
                    b'd' => y = bound_row(n.saturating_sub(1))?,
                    b's' => saved = (x, y),
                    b'u' => (x, y) = saved,
                    b'J' if params[0] == 2 => {
                        canvas.clear(Cell::default());
                    }
                    b'K' if params[0] <= 2 => {
                        if automatic_height {
                            ensure_row(&mut canvas, y)?;
                            used_height = used_height.max(y + 1);
                        }
                        let start = if params[0] == 0 { x.min(width) } else { 0 };
                        let end = if params[0] == 1 {
                            x.saturating_add(1).min(width)
                        } else {
                            width
                        };
                        for col in start..end {
                            canvas.set(col, y, Cell { ch: ' ', ..style });
                        }
                    }
                    other => return Err(format!("unsupported CSI command '{}'", other as char)),
                }
                i = end + 1;
            }
            b'\r' => {
                x = 0;
                i += 1;
            }
            b'\n' => {
                y = y.saturating_add(1);
                i += 1;
            }
            0x08 => {
                x = x.saturating_sub(1);
                i += 1;
            }
            b'\t' => {
                x = ((x / 8) + 1).saturating_mul(8).min(width);
                i += 1;
            }
            0x00 | 0x1a => {
                i += 1;
            }
            b if b < 32 || b == 127 => {
                return Err(format!("unsupported control byte 0x{b:02X} at byte {i}"))
            }
            _ => {
                let ch = match encoding {
                    Encoding::Cp437 => {
                        let c = decode_cp437(art[i]);
                        i += 1;
                        c
                    }
                    Encoding::Utf8 => {
                        let len = match art[i] {
                            0..=0x7f => 1,
                            0xc2..=0xdf => 2,
                            0xe0..=0xef => 3,
                            0xf0..=0xf4 => 4,
                            _ => return Err(format!("invalid UTF-8 at byte {i}")),
                        };
                        let part = art
                            .get(i..i + len)
                            .ok_or_else(|| format!("truncated UTF-8 at byte {i}"))?;
                        let c = std::str::from_utf8(part)
                            .map_err(|_| format!("invalid UTF-8 at byte {i}"))?
                            .chars()
                            .next()
                            .unwrap();
                        i += len;
                        c
                    }
                };
                if x >= width {
                    x = 0;
                    y = y.saturating_add(1);
                }
                style.ch = ch;
                if automatic_height {
                    ensure_row(&mut canvas, y)?;
                    used_height = used_height.max(y + 1);
                }
                if !canvas.set(x, y, style) {
                    return Err(format!("artwork exceeds the canvas at row {}; increase the fallback dimensions or correct SAUCE dimensions", y + 1));
                }
                x += 1;
            }
        }
    }
    if automatic_height && canvas.height() != used_height {
        canvas
            .resize(width, used_height)
            .map_err(|e| e.to_string())?;
    }
    Ok(Document { canvas, sauce })
}

fn ensure_row(canvas: &mut Canvas, row: usize) -> Result<(), String> {
    let needed = row
        .checked_add(1)
        .ok_or("artwork row exceeds the dimension limit")?;
    let maximum = MAX_CELLS / canvas.width();
    if needed > maximum {
        return Err(format!(
            "artwork exceeds the {MAX_CELLS}-cell limit at row {needed}"
        ));
    }
    if needed > canvas.height() {
        let height = needed.max(canvas.height().saturating_mul(2)).min(maximum);
        canvas
            .resize(canvas.width(), height)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn color_sgr(color: Color, fg: bool) -> Result<String, String> {
    let base = if fg { 30 } else { 40 };
    Ok(match color {
        Color::Ansi(n @ 0..=7) => (base + n).to_string(),
        Color::Ansi(n @ 8..=15) => (base + 60 + n - 8).to_string(),
        Color::Ansi(_) => return Err("ANSI color must be in 0..=15".into()),
        Color::Indexed(n) => format!("{};5;{n}", if fg { 38 } else { 48 }),
        Color::Rgb(r, g, b) => format!("{};2;{r};{g};{b}", if fg { 38 } else { 48 }),
    })
}

fn emit_style(out: &mut Vec<u8>, cell: &Cell) -> Result<(), String> {
    let Attributes {
        bold,
        blink,
        inverse,
        underline,
    } = cell.attrs;
    let mut parts = vec![
        "0".to_owned(),
        color_sgr(cell.fg, true)?,
        color_sgr(cell.bg, false)?,
    ];
    if bold {
        parts.push("1".into());
    }
    if underline {
        parts.push("4".into());
    }
    if blink {
        parts.push("5".into());
    }
    if inverse {
        parts.push("7".into());
    }
    out.extend_from_slice(format!("\x1b[{}m", parts.join(";")).as_bytes());
    Ok(())
}

pub fn save(document: &Document, encoding: Encoding) -> Result<Vec<u8>, String> {
    let canvas = &document.canvas;
    let mut out = Vec::new();
    let mut style = None;
    for y in 0..canvas.height() {
        out.extend_from_slice(format!("\x1b[{};1H", y + 1).as_bytes());
        for x in 0..canvas.width() {
            let cell = canvas.get(x, y).unwrap();
            let key = (cell.fg, cell.bg, cell.attrs);
            if style != Some(key) {
                emit_style(&mut out, cell)?;
                style = Some(key);
            }
            encode_char(cell.ch, encoding, &mut out)
                .map_err(|e| format!("cell ({}, {}): {e}", x + 1, y + 1))?;
        }
    }
    out.extend_from_slice(b"\x1b[0m");
    if let Some(metadata) = &document.sauce {
        sauce::append(&mut out, metadata, canvas.width(), canvas.height())?;
    }
    Ok(out)
}

/// Export a rectangle at the origin, including SAUCE to record its dimensions.
pub fn save_region(
    document: &Document,
    encoding: Encoding,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
) -> Result<Vec<u8>, String> {
    let source = &document.canvas;
    if width == 0
        || height == 0
        || x.checked_add(width)
            .filter(|&end| end <= source.width())
            .is_none()
        || y.checked_add(height)
            .filter(|&end| end <= source.height())
            .is_none()
    {
        return Err("Export region must be nonempty and inside the canvas".into());
    }
    let mut canvas = Canvas::new(width, height).map_err(|e| e.to_string())?;
    for dy in 0..height {
        for dx in 0..width {
            canvas.set(dx, dy, *source.get(x + dx, y + dy).unwrap());
        }
    }
    save(
        &Document {
            canvas,
            sauce: Some(document.sauce.clone().unwrap_or_default()),
        },
        encoding,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_dimensions_follow_sauce_or_eighty_columns_and_rendered_rows() {
        let doc = Document {
            canvas: Canvas::new(4, 3).unwrap(),
            sauce: Some(Sauce::default()),
        };
        let mut bytes = save(&doc, Encoding::Cp437).unwrap();
        let loaded = load_auto(&bytes, Encoding::Cp437).unwrap();
        assert_eq!((loaded.canvas.width(), loaded.canvas.height()), (4, 3));
        let record = bytes.len() - 128;
        bytes[record + 98..record + 100].fill(0);
        let loaded = load_auto(&bytes, Encoding::Cp437).unwrap();
        assert_eq!((loaded.canvas.width(), loaded.canvas.height()), (4, 3));
        bytes[record + 96..record + 98].fill(0);
        bytes[record + 98..record + 100].copy_from_slice(&7u16.to_le_bytes());
        let loaded = load_auto(&bytes, Encoding::Cp437).unwrap();
        assert_eq!((loaded.canvas.width(), loaded.canvas.height()), (80, 7));
        let mut art = vec![b'X'; 81];
        art.extend_from_slice(b"\r\n\r\n\x1b[30;2H\x1b[38;2;12;34;56m");
        art.extend_from_slice("λ".as_bytes());
        art.extend_from_slice(b"\r\n");
        let loaded = load_auto(&art, Encoding::Utf8).unwrap();
        assert_eq!((loaded.canvas.width(), loaded.canvas.height()), (80, 30));
        assert_eq!(loaded.canvas.get(0, 1).unwrap().ch, 'X');
        assert_eq!(loaded.canvas.get(1, 29).unwrap().ch, 'λ');
        assert_eq!(loaded.canvas.get(1, 29).unwrap().fg, Color::Rgb(12, 34, 56));
        let loaded = load_auto(b"A\r\n\r\nB\r\n", Encoding::Cp437).unwrap();
        assert_eq!(loaded.canvas.height(), 3);
        assert_eq!(loaded.canvas.get(0, 2).unwrap().ch, 'B');
        assert_eq!(load_auto(b"", Encoding::Cp437).unwrap().canvas.height(), 1);
        let loaded = load_auto(b"\x1b[40;1H\x1b[41m\x1b[2K", Encoding::Cp437).unwrap();
        assert_eq!(loaded.canvas.height(), 40);
        assert_eq!(loaded.canvas.get(0, 39).unwrap().bg, Color::Ansi(1));
    }

    #[test]
    fn automatic_height_rejects_resource_overflow_and_malformed_artwork() {
        let maximum_height = MAX_CELLS / DEFAULT_WIDTH;
        let bytes = format!("\x1b[{maximum_height};1HX");
        let doc = load_auto(bytes.as_bytes(), Encoding::Cp437).unwrap();
        assert_eq!(doc.canvas.height(), maximum_height);
        assert_eq!(doc.canvas.get(0, maximum_height - 1).unwrap().ch, 'X');
        let too_tall = format!("\x1b[{};1HX", maximum_height + 1);
        assert!(load_auto(too_tall.as_bytes(), Encoding::Cp437).is_err());
        let mut overflowing = bytes.into_bytes();
        overflowing.extend_from_slice(b"\r\nX");
        assert!(load_auto(&overflowing, Encoding::Cp437).is_err());
        assert!(load_auto(b"\x1b[", Encoding::Cp437).is_err());
        assert!(load_auto(b"\xff", Encoding::Utf8).is_err());
        // Explicit-size callers retain their previous bounds and UTF-8 behavior.
        let doc = load("λ".as_bytes(), Encoding::Utf8, 5, 6).unwrap();
        assert_eq!((doc.canvas.width(), doc.canvas.height()), (5, 6));
        assert_eq!(doc.canvas.get(0, 0).unwrap().ch, 'λ');
        assert!(load(b"A\r\nB", Encoding::Cp437, 5, 1).is_err());
    }

    #[test]
    fn round_trip_styles_and_dimensions() {
        let mut canvas = Canvas::new(4, 2).unwrap();
        let styles = [Color::Ansi(14), Color::Indexed(200), Color::Rgb(4, 55, 240)];
        for (x, fg) in styles.into_iter().enumerate() {
            canvas.set(
                x,
                0,
                Cell {
                    ch: '█',
                    fg,
                    bg: Color::Indexed(25),
                    attrs: Attributes {
                        bold: true,
                        blink: true,
                        ..Default::default()
                    },
                },
            );
        }
        let doc = Document {
            canvas,
            sauce: Some(Sauce::default()),
        };
        let bytes = save(&doc, Encoding::Cp437).unwrap();
        let loaded = load(&bytes, Encoding::Cp437, 80, 25).unwrap();
        assert_eq!(doc.canvas, loaded.canvas);
    }
    #[test]
    fn movement_wrapping_utf8_and_bad_input() {
        let doc = load(b"ABCDX\x1b[2;3H\x1b[38;2;1;2;3mZ", Encoding::Utf8, 4, 2).unwrap();
        assert_eq!(doc.canvas.get(0, 1).unwrap().ch, 'X');
        assert_eq!(doc.canvas.get(2, 1).unwrap().fg, Color::Rgb(1, 2, 3));
        assert!(load(b"\xff", Encoding::Utf8, 4, 2).is_err());
        assert!(load(b"\x1b[38;5m", Encoding::Cp437, 4, 2).is_err());
        assert!(load(b"ABCDE", Encoding::Cp437, 4, 1).is_err());
        let doc = load(b"\x1b[?25lABC\x1b[2G\x1b[K", Encoding::Cp437, 4, 1).unwrap();
        assert_eq!(doc.canvas.get(0, 0).unwrap().ch, 'A');
        assert_eq!(doc.canvas.get(1, 0).unwrap().ch, ' ');
    }

    #[test]
    fn utf8_rgb_background_and_indexed_foreground_round_trip() {
        let mut canvas = Canvas::new(3, 1).unwrap();
        canvas.set(
            0,
            0,
            Cell {
                ch: '😀',
                fg: Color::Indexed(255),
                bg: Color::Rgb(7, 8, 9),
                ..Cell::default()
            },
        );
        let doc = Document {
            canvas,
            sauce: None,
        };
        let bytes = save(&doc, Encoding::Utf8).unwrap();
        assert_eq!(
            load(&bytes, Encoding::Utf8, 3, 1).unwrap().canvas,
            doc.canvas
        );
        assert!(save(&doc, Encoding::Cp437).is_err());
    }

    #[test]
    fn region_export_rebases_cells_and_preserves_metadata_and_styles() {
        let mut canvas = Canvas::new(80, 25).unwrap();
        let mut expected = Canvas::new(3, 2).unwrap();
        let colors = [Color::Ansi(14), Color::Indexed(200), Color::Rgb(4, 55, 240)];
        for y in 0..2 {
            for (x, fg) in colors.into_iter().enumerate() {
                let cell = Cell {
                    ch: if y == 0 { '█' } else { 'é' },
                    fg,
                    bg: Color::Rgb(7, 8, 9),
                    attrs: Attributes {
                        bold: true,
                        blink: true,
                        inverse: true,
                        underline: true,
                    },
                };
                canvas.set(10 + x, 5 + y, cell);
                expected.set(x, y, cell);
            }
        }
        canvas.set(
            9,
            5,
            Cell {
                ch: 'X',
                ..Cell::default()
            },
        );
        let original = canvas.clone();
        let mut metadata = Sauce::default();
        metadata.title = "Selection".into();
        metadata.author = "Artist".into();
        metadata.group = "Group".into();
        metadata.comments = vec!["Export test".into()];
        let doc = Document {
            canvas,
            sauce: Some(metadata),
        };
        for encoding in [Encoding::Cp437, Encoding::Utf8] {
            let bytes = save_region(&doc, encoding, 10, 5, 3, 2).unwrap();
            let loaded = load(&bytes, encoding, 80, 25).unwrap();
            assert_eq!(loaded.canvas, expected);
            let sauce = loaded.sauce.unwrap();
            assert_eq!(sauce.dimensions(), Some((3, 2)));
            assert_eq!(sauce.title, "Selection");
            assert_eq!(sauce.author, "Artist");
            assert_eq!(sauce.group, "Group");
            assert_eq!(sauce.comments, ["Export test"]);
        }
        assert_eq!(doc.canvas, original);
    }

    #[test]
    fn region_export_records_dimensions_without_source_metadata_and_validates_bounds() {
        let mut doc = Document {
            canvas: Canvas::new(4, 3).unwrap(),
            sauce: None,
        };
        doc.canvas.set(
            3,
            2,
            Cell {
                ch: '😀',
                ..Cell::default()
            },
        );
        let bytes = save_region(&doc, Encoding::Utf8, 3, 2, 1, 1).unwrap();
        let loaded = load(&bytes, Encoding::Utf8, 80, 25).unwrap();
        assert_eq!((loaded.canvas.width(), loaded.canvas.height()), (1, 1));
        assert_eq!(loaded.canvas.get(0, 0).unwrap().ch, '😀');
        assert!(doc.sauce.is_none());
        assert!(save_region(&doc, Encoding::Cp437, 3, 2, 1, 1).is_err());
        for (x, y, width, height) in [
            (0, 0, 0, 1),
            (0, 0, 1, 0),
            (3, 2, 2, 1),
            (3, 2, 1, 2),
            (4, 0, 1, 1),
            (0, 3, 1, 1),
            (usize::MAX, 0, 1, 1),
            (0, usize::MAX, 1, 1),
        ] {
            assert!(save_region(&doc, Encoding::Utf8, x, y, width, height).is_err());
        }
    }
}
