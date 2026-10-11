//! The SysEx envelope shared by every Fractal Audio message.
//!
//! ```text
//! F0  00 01 74  <model>  <function>  <payload ...>  <checksum>  F7
//! ```
//!
//! The checksum is the XOR of every byte from `F0` up to and including the
//! last payload byte, masked to seven bits.

use std::fmt;

pub const SYSEX_START: u8 = 0xF0;
pub const SYSEX_END: u8 = 0xF7;
/// Fractal Audio Systems' three-byte MIDI manufacturer ID.
pub const MANUFACTURER_ID: [u8; 3] = [0x00, 0x01, 0x74];

/// Bytes before the payload: start, manufacturer ID, model, function.
const HEADER_LEN: usize = 6;
/// Bytes after the payload: checksum, end.
const TRAILER_LEN: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    TooShort {
        len: usize,
    },
    MissingStart,
    MissingEnd,
    WrongManufacturer {
        found: [u8; 3],
    },
    /// A byte with the high bit set appeared between start and end.
    DataByteOutOfRange {
        offset: usize,
        byte: u8,
    },
    Checksum {
        expected: u8,
        found: u8,
    },
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort { len } => write!(f, "SysEx message of {len} bytes is too short"),
            Self::MissingStart => f.write_str("message does not start with F0"),
            Self::MissingEnd => f.write_str("message does not end with F7"),
            Self::WrongManufacturer { found } => {
                write!(f, "manufacturer ID {found:02X?} is not Fractal Audio")
            }
            Self::DataByteOutOfRange { offset, byte } => {
                write!(f, "byte {byte:#04X} at offset {offset} is not a 7-bit data byte")
            }
            Self::Checksum { expected, found } => {
                write!(f, "checksum mismatch: expected {expected:#04X}, found {found:#04X}")
            }
        }
    }
}

impl std::error::Error for FrameError {}

/// XOR of `bytes`, masked to seven bits.
pub fn checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0, |acc, b| acc ^ b) & 0x7F
}

/// One decoded message: which device family it addresses, what it asks for,
/// and the 7-bit payload between the function byte and the checksum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub model: u8,
    pub function: u8,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn new(model: u8, function: u8, payload: impl Into<Vec<u8>>) -> Self {
        Self { model, function, payload: payload.into() }
    }

    /// Serializes the frame, computing the checksum.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + self.payload.len() + TRAILER_LEN);
        out.push(SYSEX_START);
        out.extend_from_slice(&MANUFACTURER_ID);
        out.push(self.model);
        out.push(self.function);
        out.extend_from_slice(&self.payload);
        out.push(checksum(&out));
        out.push(SYSEX_END);
        out
    }

    /// Parses one complete message, `F0` through `F7`, verifying its checksum.
    pub fn parse(bytes: &[u8]) -> Result<Self, FrameError> {
        let (model, function, body) = split_envelope(bytes)?;
        let (found, payload) = body.split_last().ok_or(FrameError::TooShort { len: bytes.len() })?;
        let expected = checksum(&bytes[..bytes.len() - TRAILER_LEN]);
        if *found != expected {
            return Err(FrameError::Checksum { expected, found: *found });
        }
        Ok(Self { model, function, payload: payload.to_vec() })
    }

    /// Parses a message that carries no checksum byte.
    ///
    /// A few short device replies are sent this way; the caller has to know
    /// from the function ID that it is dealing with one of them.
    pub fn parse_unchecked(bytes: &[u8]) -> Result<Self, FrameError> {
        let (model, function, body) = split_envelope(bytes)?;
        Ok(Self { model, function, payload: body.to_vec() })
    }
}

/// Validates the envelope and returns `(model, function, bytes before F7)`.
fn split_envelope(bytes: &[u8]) -> Result<(u8, u8, &[u8]), FrameError> {
    if bytes.len() < HEADER_LEN + 1 {
        return Err(FrameError::TooShort { len: bytes.len() });
    }
    if bytes[0] != SYSEX_START {
        return Err(FrameError::MissingStart);
    }
    if bytes[bytes.len() - 1] != SYSEX_END {
        return Err(FrameError::MissingEnd);
    }
    if bytes[1..4] != MANUFACTURER_ID {
        return Err(FrameError::WrongManufacturer { found: [bytes[1], bytes[2], bytes[3]] });
    }
    let inner = &bytes[1..bytes.len() - 1];
    if let Some(pos) = inner.iter().position(|b| b & 0x80 != 0) {
        return Err(FrameError::DataByteOutOfRange { offset: pos + 1, byte: inner[pos] });
    }
    Ok((bytes[4], bytes[5], &bytes[HEADER_LEN..bytes.len() - 1]))
}

/// Splits a byte stream (a `.syx` file, or a MIDI input buffer) into complete
/// `F0 .. F7` messages. Bytes outside any message are skipped; a trailing
/// message without its `F7` is not returned.
pub fn split_messages(stream: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut rest = stream;
    std::iter::from_fn(move || {
        let start = rest.iter().position(|&b| b == SYSEX_START)?;
        let len = rest[start..].iter().position(|&b| b == SYSEX_END)? + 1;
        let message = &rest[start..start + len];
        rest = &rest[start + len..];
        Some(message)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Leading and trailing messages of a real Axe-Fx II XL+ cabinet IR export.
    const IR_HEADER: [u8; 12] = [0xF0, 0x00, 0x01, 0x74, 0x07, 0x7A, 0x7F, 0x7F, 0x00, 0x10, 0x68, 0xF7];
    const IR_FOOTER: [u8; 13] = [0xF0, 0x00, 0x01, 0x74, 0x07, 0x7C, 0x5A, 0x47, 0x48, 0x4D, 0x05, 0x63, 0xF7];

    #[test]
    fn parses_observed_messages() {
        let header = Frame::parse(&IR_HEADER).unwrap();
        assert_eq!((header.model, header.function), (0x07, 0x7A));
        assert_eq!(header.payload, [0x7F, 0x7F, 0x00, 0x10]);

        let footer = Frame::parse(&IR_FOOTER).unwrap();
        assert_eq!(footer.function, 0x7C);
        assert_eq!(footer.payload, [0x5A, 0x47, 0x48, 0x4D, 0x05]);
    }

    #[test]
    fn serializing_reproduces_observed_bytes() {
        assert_eq!(Frame::new(0x07, 0x7A, [0x7F, 0x7F, 0x00, 0x10]).to_bytes(), IR_HEADER);
        assert_eq!(Frame::new(0x07, 0x7C, [0x5A, 0x47, 0x48, 0x4D, 0x05]).to_bytes(), IR_FOOTER);
    }

    #[test]
    fn rejects_corruption() {
        let mut bad = IR_HEADER;
        bad[7] = 0x7E;
        assert_eq!(Frame::parse(&bad), Err(FrameError::Checksum { expected: 0x69, found: 0x68 }));

        let mut wrong_vendor = IR_HEADER;
        wrong_vendor[3] = 0x75;
        assert!(matches!(Frame::parse(&wrong_vendor), Err(FrameError::WrongManufacturer { .. })));

        assert_eq!(Frame::parse(&IR_HEADER[..11]), Err(FrameError::MissingEnd));
        assert_eq!(Frame::parse(&[0xF0, 0xF7]), Err(FrameError::TooShort { len: 2 }));
    }

    #[test]
    fn splits_concatenated_stream() {
        let mut stream = vec![0x00, 0x42];
        stream.extend_from_slice(&IR_HEADER);
        stream.extend_from_slice(&IR_FOOTER);
        stream.extend_from_slice(&[0xF0, 0x00, 0x01]);
        let messages: Vec<&[u8]> = split_messages(&stream).collect();
        assert_eq!(messages, [&IR_HEADER[..], &IR_FOOTER[..]]);
    }
}
