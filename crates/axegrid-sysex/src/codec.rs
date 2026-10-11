//! Field encodings used inside message payloads.
//!
//! Integers travel as 7-bit groups, least significant group first.

/// Appends `value` as `groups` 7-bit bytes.
pub fn put_uint(out: &mut Vec<u8>, value: u32, groups: usize) {
    out.extend((0..groups).map(|i| (value.checked_shr(7 * i as u32).unwrap_or(0) & 0x7F) as u8));
}

/// Appends an IEEE-754 single as five 7-bit bytes.
pub fn put_f32(out: &mut Vec<u8>, value: f32) {
    put_uint(out, value.to_bits(), 5);
}

/// Appends `text` as exactly `width` characters, space padded and truncated.
/// Characters outside printable ASCII become `?`.
pub fn put_fixed_text(out: &mut Vec<u8>, text: &str, width: usize) {
    let chars = text.chars().map(|c| if c.is_ascii_graphic() || c == ' ' { c as u8 } else { b'?' });
    out.extend(chars.chain(std::iter::repeat(b' ')).take(width));
}

/// Sequential reader over a payload. Every getter returns `None` once the
/// payload is exhausted, so a short message fails as a whole.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    rest: &'a [u8],
}

impl<'a> Reader<'a> {
    pub fn new(payload: &'a [u8]) -> Self {
        Self { rest: payload }
    }

    pub fn remaining(&self) -> usize {
        self.rest.len()
    }

    pub fn uint(&mut self, groups: usize) -> Option<u32> {
        if self.rest.len() < groups {
            return None;
        }
        let (head, tail) = self.rest.split_at(groups);
        self.rest = tail;
        Some(
            head.iter()
                .enumerate()
                .fold(0u32, |acc, (i, &b)| acc | u32::from(b & 0x7F).checked_shl(7 * i as u32).unwrap_or(0)),
        )
    }

    pub fn u8(&mut self) -> Option<u8> {
        self.uint(1).map(|v| v as u8)
    }

    pub fn u16(&mut self, groups: usize) -> Option<u16> {
        self.uint(groups).map(|v| v as u16)
    }

    pub fn f32(&mut self) -> Option<f32> {
        self.uint(5).map(f32::from_bits)
    }

    /// Reads up to the next NUL (consumed) or the end of the payload.
    pub fn text(&mut self) -> String {
        let end = self.rest.iter().position(|&b| b == 0).unwrap_or(self.rest.len());
        let text = String::from_utf8_lossy(&self.rest[..end]).into_owned();
        self.rest = &self.rest[(end + 1).min(self.rest.len())..];
        text
    }

    /// Reads everything left as text with trailing padding removed.
    pub fn padded_text(&mut self) -> String {
        let text = String::from_utf8_lossy(self.rest);
        self.rest = &[];
        text.trim_end_matches(['\0', ' ']).to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uint_groups_are_least_significant_first() {
        let mut out = Vec::new();
        put_uint(&mut out, 106, 2);
        put_uint(&mut out, 0xFFFE, 3);
        assert_eq!(out, [0x6A, 0x00, 0x7E, 0x7F, 0x03]);

        let mut reader = Reader::new(&out);
        assert_eq!(reader.uint(2), Some(106));
        assert_eq!(reader.uint(3), Some(0xFFFE));
        assert_eq!(reader.uint(1), None);
    }

    #[test]
    fn float_round_trips_through_five_groups() {
        for value in [-80.0f32, 0.1, 1000.0, f32::MAX] {
            let mut out = Vec::new();
            put_f32(&mut out, value);
            assert_eq!(out.len(), 5);
            assert!(out.iter().all(|b| b & 0x80 == 0));
            assert_eq!(Reader::new(&out).f32(), Some(value));
        }
    }

    #[test]
    fn text_fields() {
        let mut out = Vec::new();
        put_fixed_text(&mut out, "Plexi é", 10);
        assert_eq!(out, b"Plexi ?   ");

        let mut reader = Reader::new(b"OFF\0ON\0tail");
        assert_eq!(reader.text(), "OFF");
        assert_eq!(reader.text(), "ON");
        assert_eq!(reader.text(), "tail");
        assert_eq!(reader.remaining(), 0);
    }
}
