use std::fmt;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Encoding {
    #[default]
    Cp437,
    Utf8,
}

const LOW: [char; 32] = [
    '\0', '☺', '☻', '♥', '♦', '♣', '♠', '•', '◘', '○', '◙', '♂', '♀', '♪', '♫', '☼', '►', '◄', '↕',
    '‼', '¶', '§', '▬', '↨', '↑', '↓', '→', '←', '∟', '↔', '▲', '▼',
];
const HIGH: [char; 128] = [
    'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å', 'É', 'æ', 'Æ',
    'ô', 'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ', 'á', 'í', 'ó', 'ú', 'ñ', 'Ñ',
    'ª', 'º', '¿', '⌐', '¬', '½', '¼', '¡', '«', '»', '░', '▒', '▓', '│', '┤', '╡', '╢', '╖', '╕',
    '╣', '║', '╗', '╝', '╜', '╛', '┐', '└', '┴', '┬', '├', '─', '┼', '╞', '╟', '╚', '╔', '╩', '╦',
    '╠', '═', '╬', '╧', '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘', '┌', '█', '▄', '▌', '▐',
    '▀', 'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ', '∞', 'φ', 'ε', '∩', '≡', '±',
    '≥', '≤', '⌠', '⌡', '÷', '≈', '°', '∙', '·', '√', 'ⁿ', '²', '■', '\u{a0}',
];

pub fn decode_cp437(byte: u8) -> char {
    match byte {
        0..=31 => LOW[byte as usize],
        32..=126 => byte as char,
        127 => '⌂',
        _ => HIGH[(byte - 128) as usize],
    }
}

pub fn encode_cp437(ch: char) -> Option<u8> {
    (0u8..=255).find(|&byte| decode_cp437(byte) == ch)
}

#[derive(Debug)]
pub struct EncodingError(pub String);
impl fmt::Display for EncodingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for EncodingError {}

pub fn encode_char(ch: char, encoding: Encoding, out: &mut Vec<u8>) -> Result<(), EncodingError> {
    if ch.is_control() {
        return Err(EncodingError(format!(
            "control character U+{:04X} cannot be written as artwork",
            ch as u32
        )));
    }
    match encoding {
        Encoding::Cp437 => {
            let byte = encode_cp437(ch)
                .ok_or_else(|| EncodingError(format!("'{ch}' is not representable in CP437")))?;
            if byte < 32 || byte == 127 {
                return Err(EncodingError(format!(
                    "'{ch}' is a CP437 control byte; save as UTF-8 instead"
                )));
            }
            out.push(byte);
        }
        Encoding::Utf8 => {
            let mut buf = [0; 4];
            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cp437_round_trips_every_byte() {
        for byte in 0..=255 {
            assert_eq!(encode_cp437(decode_cp437(byte)), Some(byte));
        }
        assert_eq!(decode_cp437(0xdb), '█');
        assert!(encode_char('☺', Encoding::Cp437, &mut vec![]).is_err());
        assert!(encode_char('😀', Encoding::Cp437, &mut vec![]).is_err());
    }
}
