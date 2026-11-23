use crate::encoding::{decode_cp437, encode_cp437};

#[derive(Clone, Debug)]
pub struct Sauce {
    raw: [u8; 128],
    pub title: String,
    pub author: String,
    pub group: String,
    pub comments: Vec<String>,
}

impl Default for Sauce {
    fn default() -> Self {
        let mut raw = [0; 128];
        raw[..7].copy_from_slice(b"SAUCE00");
        raw[7..90].fill(b' ');
        raw[94] = 1;
        raw[95] = 1;
        Self {
            raw,
            title: String::new(),
            author: String::new(),
            group: String::new(),
            comments: vec![],
        }
    }
}

impl Sauce {
    pub fn dimensions(&self) -> Option<(usize, usize)> {
        self.width().zip(self.height())
    }
    pub fn width(&self) -> Option<usize> {
        let width = u16::from_le_bytes([self.raw[96], self.raw[97]]) as usize;
        (width > 0).then_some(width)
    }
    pub fn height(&self) -> Option<usize> {
        let height = u16::from_le_bytes([self.raw[98], self.raw[99]]) as usize;
        (height > 0).then_some(height)
    }
    pub fn flags(&self) -> u8 {
        self.raw[105]
    }
}

fn text(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| decode_cp437(b))
        .collect::<String>()
        .trim_end_matches([' ', '\0'])
        .to_owned()
}

fn write_text(target: &mut [u8], value: &str, name: &str) -> Result<(), String> {
    let bytes: Vec<u8> = value
        .chars()
        .map(|c| {
            encode_cp437(c)
                .filter(|b| *b >= 32 && *b != 127)
                .ok_or_else(|| format!("{name} contains a character not supported by SAUCE CP437"))
        })
        .collect::<Result<_, _>>()?;
    if bytes.len() > target.len() {
        return Err(format!("{name} exceeds {} characters", target.len()));
    }
    target.fill(b' ');
    target[..bytes.len()].copy_from_slice(&bytes);
    Ok(())
}

/// Returns the artwork slice and metadata, excluding a conventional EOF marker.
pub fn parse(data: &[u8]) -> Result<(&[u8], Option<Sauce>), String> {
    if data.len() < 128 || &data[data.len() - 128..data.len() - 121] != b"SAUCE00" {
        return Ok((data.strip_suffix(&[0x1a]).unwrap_or(data), None));
    }
    let record_start = data.len() - 128;
    let mut raw = [0; 128];
    raw.copy_from_slice(&data[record_start..]);
    let count = raw[104] as usize;
    let mut art_end = record_start;
    let mut comments = Vec::new();
    if count > 0 {
        let length = 5 + count * 64;
        let start = record_start
            .checked_sub(length)
            .ok_or("truncated SAUCE comment block")?;
        if &data[start..start + 5] != b"COMNT" {
            return Err("missing SAUCE COMNT block".into());
        }
        for line in data[start + 5..record_start].chunks_exact(64) {
            comments.push(text(line));
        }
        art_end = start;
    }
    let art = data[..art_end]
        .strip_suffix(&[0x1a])
        .unwrap_or(&data[..art_end]);
    Ok((
        art,
        Some(Sauce {
            title: text(&raw[7..42]),
            author: text(&raw[42..62]),
            group: text(&raw[62..82]),
            comments,
            raw,
        }),
    ))
}

pub fn append(
    data: &mut Vec<u8>,
    sauce: &Sauce,
    width: usize,
    height: usize,
) -> Result<(), String> {
    let w = u16::try_from(width).map_err(|_| "SAUCE width exceeds 65535")?;
    let h = u16::try_from(height).map_err(|_| "SAUCE height exceeds 65535")?;
    let length = u32::try_from(data.len()).map_err(|_| "artwork exceeds SAUCE file size limit")?;
    let count =
        u8::try_from(sauce.comments.len()).map_err(|_| "SAUCE permits at most 255 comments")?;
    let mut raw = sauce.raw;
    write_text(&mut raw[7..42], &sauce.title, "title")?;
    write_text(&mut raw[42..62], &sauce.author, "author")?;
    write_text(&mut raw[62..82], &sauce.group, "group")?;
    raw[90..94].copy_from_slice(&length.to_le_bytes());
    raw[96..98].copy_from_slice(&w.to_le_bytes());
    raw[98..100].copy_from_slice(&h.to_le_bytes());
    raw[104] = count;
    let mut block = Vec::new();
    if count > 0 {
        block.extend_from_slice(b"COMNT");
        for line in &sauce.comments {
            let mut field = [b' '; 64];
            write_text(&mut field, line, "comment")?;
            block.extend_from_slice(&field);
        }
    }
    data.push(0x1a);
    data.extend_from_slice(&block);
    data.extend_from_slice(&raw);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_comments_and_eof_round_trip() {
        let mut bytes = b"art".to_vec();
        let sauce = Sauce {
            title: "Test".into(),
            comments: vec!["hello".into()],
            ..Sauce::default()
        };
        append(&mut bytes, &sauce, 100, 40).unwrap();
        let (art, meta) = parse(&bytes).unwrap();
        let meta = meta.unwrap();
        assert_eq!(art, b"art");
        assert_eq!(meta.title, "Test");
        assert_eq!(meta.comments, ["hello"]);
        assert_eq!(meta.dimensions(), Some((100, 40)));
        bytes[4] = b'X';
        assert!(parse(&bytes).is_err());
    }

    #[test]
    fn preserves_noneditable_metadata_and_rejects_invalid_fields() {
        let mut bytes = b"art".to_vec();
        let mut original = Sauce::default();
        original.raw[82..90].copy_from_slice(b"20261121");
        original.raw[105] = 1;
        original.raw[120] = 42;
        append(&mut bytes, &original, 3, 1).unwrap();
        let (_, parsed) = parse(&bytes).unwrap();
        let mut parsed = parsed.unwrap();
        parsed.title = "Updated".into();
        let mut saved = b"art".to_vec();
        append(&mut saved, &parsed, 5, 2).unwrap();
        let (_, parsed) = parse(&saved).unwrap();
        let parsed = parsed.unwrap();
        assert_eq!(&parsed.raw[82..90], b"20261121");
        assert_eq!(parsed.flags(), 1);
        assert_eq!(parsed.raw[120], 42);
        assert_eq!(parsed.dimensions(), Some((5, 2)));
        let mut invalid = parsed;
        invalid.title = "😀".into();
        let mut data = b"art".to_vec();
        assert!(append(&mut data, &invalid, 5, 2).is_err());
        assert_eq!(data, b"art");
    }
}
