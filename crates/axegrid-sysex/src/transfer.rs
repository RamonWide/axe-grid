//! Preset and cabinet dumps: a header message, data chunks and a checksum
//! footer (spec section 4.5). The same messages make up a `.syx` file, a
//! dump the device sends, and an upload to the device.

use std::fmt;

use crate::codec::{put_uint, Reader};
use crate::frame::{split_messages, Frame, FrameError};
use crate::message::Model;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DumpKind {
    Preset,
    /// A cabinet impulse response.
    Cab,
}

impl DumpKind {
    /// Function IDs of the header, a data chunk and the footer.
    pub const fn functions(self) -> (u8, u8, u8) {
        match self {
            Self::Preset => (0x77, 0x78, 0x79),
            Self::Cab => (0x7A, 0x7B, 0x7C),
        }
    }

    /// 7-bit groups that carry one data word, and the footer's checksum.
    const fn groups(self) -> usize {
        match self {
            Self::Preset => 3,
            Self::Cab => 5,
        }
    }

    /// The bits of a data word that carry data: 21 for a preset's words,
    /// 32 for a cabinet's samples.
    const fn word_mask(self) -> u64 {
        match self {
            Self::Preset => 0x1F_FFFF,
            Self::Cab => 0xFFFF_FFFF,
        }
    }

    pub fn of_header(function: u8) -> Option<Self> {
        [Self::Preset, Self::Cab].into_iter().find(|kind| kind.functions().0 == function)
    }

    /// Whether a message with this function belongs to a dump of this kind.
    pub fn owns(self, function: u8) -> bool {
        let (header, data, footer) = self.functions();
        [header, data, footer].contains(&function)
    }

    pub const fn noun(self) -> &'static str {
        match self {
            Self::Preset => "preset",
            Self::Cab => "cabinet IR",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DumpError {
    Frame(FrameError),
    /// No header message was found.
    NotADump,
    /// A message that does not belong to the dump, or one out of order.
    Unexpected {
        function: u8,
    },
    /// The data ended without a footer.
    Incomplete,
    /// A chunk announces more words than it carries.
    ShortChunk,
    /// The footer does not match the data.
    Checksum,
}

impl fmt::Display for DumpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Frame(e) => e.fmt(f),
            Self::NotADump => f.write_str("not a preset or cabinet file"),
            Self::Unexpected { function } => write!(f, "unexpected message {function:#04X} inside the data"),
            Self::Incomplete => f.write_str("the data ends before its checksum message"),
            Self::ShortChunk => f.write_str("a data message is shorter than it says"),
            Self::Checksum => f.write_str("the data does not match its checksum"),
        }
    }
}

impl std::error::Error for DumpError {}

impl From<FrameError> for DumpError {
    fn from(e: FrameError) -> Self {
        Self::Frame(e)
    }
}

/// Characters of a preset name, one per word, after this many words.
const PRESET_NAME_AT: usize = 2;
const NAME_LEN: usize = 32;
/// Header bytes that address a preset dump to the edit buffer.
const EDIT_BUFFER: [u8; 2] = [0x7F, 0x00];

/// One complete, checksum-verified dump.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dump {
    pub kind: DumpKind,
    /// Model byte of the device or file the dump came from.
    pub model: u8,
    header: Vec<u8>,
    chunks: Vec<Vec<u8>>,
    footer: Vec<u8>,
    words: Vec<u64>,
}

impl Dump {
    /// Reads the first dump in a `.syx` file.
    pub fn parse(bytes: &[u8]) -> Result<Self, DumpError> {
        let frames = split_messages(bytes).map(Frame::parse).collect::<Result<Vec<_>, _>>()?;
        Self::from_frames(frames)
    }

    /// Builds a dump from messages in the order they were received. Messages
    /// before the header are skipped; anything after the footer is ignored.
    pub fn from_frames(frames: impl IntoIterator<Item = Frame>) -> Result<Self, DumpError> {
        let mut frames = frames.into_iter().skip_while(|frame| DumpKind::of_header(frame.function).is_none());
        let first = frames.next().ok_or(DumpError::NotADump)?;
        let kind = DumpKind::of_header(first.function).ok_or(DumpError::NotADump)?;
        let (_, data, footer) = kind.functions();
        let mut dump =
            Self { kind, model: first.model, header: first.payload, chunks: vec![], footer: vec![], words: vec![] };
        for frame in frames {
            if frame.function == data {
                let mut reader = Reader::new(&frame.payload);
                let count = reader.uint(2).ok_or(DumpError::ShortChunk)?;
                for _ in 0..count {
                    dump.words.push(wide(&mut reader, kind.groups()).ok_or(DumpError::ShortChunk)?);
                }
                dump.chunks.push(frame.payload);
            } else if frame.function == footer {
                let expected = wide(&mut Reader::new(&frame.payload), kind.groups()).ok_or(DumpError::ShortChunk)?;
                // Only the bits of a word count; a device may fill the spare
                // bits of the last group differently from a file.
                let mask = kind.word_mask();
                if (dump.words.iter().fold(0, |acc, word| acc ^ word) ^ expected) & mask != 0 {
                    return Err(DumpError::Checksum);
                }
                dump.footer = frame.payload;
                return Ok(dump);
            } else {
                return Err(DumpError::Unexpected { function: frame.function });
            }
        }
        Err(DumpError::Incomplete)
    }

    pub fn word_count(&self) -> usize {
        self.words.len()
    }

    /// The data words; a cabinet's can be wider than 32 bits.
    pub fn words(&self) -> &[u64] {
        &self.words
    }

    /// The name stored inside the data.
    pub fn name(&self) -> String {
        let bytes: Vec<u8> = match self.kind {
            DumpKind::Preset => self.words.iter().skip(PRESET_NAME_AT).take(NAME_LEN).map(|&word| word as u8).collect(),
            // Four characters to a word, first character lowest.
            DumpKind::Cab => {
                self.words.iter().take(NAME_LEN / 4).flat_map(|&word| (word as u32).to_le_bytes()).collect()
            }
        };
        let end = bytes.iter().position(|&b| !(b.is_ascii_graphic() || b == b' ')).unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[..end]).trim_end().to_owned()
    }

    /// The messages as they are sent to a device of `model`. A preset goes
    /// to the edit buffer; a cabinet goes to user slot `cab` (zero-based).
    pub fn frames(&self, model: Model, cab: u16) -> Vec<Frame> {
        let (header, data, footer) = self.kind.functions();
        let payload = match self.kind {
            DumpKind::Preset => {
                let mut payload = self.header.clone();
                for (at, byte) in EDIT_BUFFER.into_iter().enumerate() {
                    if let Some(slot) = payload.get_mut(at) {
                        *slot = byte;
                    }
                }
                payload
            }
            DumpKind::Cab => cab_header(model, cab, self.words.len() as u32),
        };
        let byte = model.byte();
        std::iter::once(Frame::new(byte, header, payload))
            .chain(self.chunks.iter().map(|chunk| Frame::new(byte, data, chunk.clone())))
            .chain(std::iter::once(Frame::new(byte, footer, self.footer.clone())))
            .collect()
    }

    /// The dump as the bytes of a `.syx` file, exactly as it was received.
    pub fn to_bytes(&self) -> Vec<u8> {
        let (header, data, footer) = self.kind.functions();
        let mut out = Frame::new(self.model, header, self.header.clone()).to_bytes();
        for chunk in &self.chunks {
            out.extend(Frame::new(self.model, data, chunk.clone()).to_bytes());
        }
        out.extend(Frame::new(self.model, footer, self.footer.clone()).to_bytes());
        out
    }
}

/// An integer of up to five 7-bit groups: wider than 32 bits.
fn wide(reader: &mut Reader<'_>, groups: usize) -> Option<u64> {
    (0..groups).try_fold(0u64, |acc, group| Some(acc | u64::from(reader.u8()?) << (7 * group)))
}

/// Header of a cabinet upload. Each model lays the slot number out its own
/// way (spec 4.5).
fn cab_header(model: Model, cab: u16, words: u32) -> Vec<u8> {
    let mut payload = cab_number(model, cab);
    put_uint(&mut payload, words, 2);
    payload
}

/// A user-cab number as `model` expects it in a dump request and a header.
pub fn cab_number(model: Model, cab: u16) -> Vec<u8> {
    match model {
        Model::AxeFx2 => vec![(cab & 0x7F) as u8],
        // High group first, like preset numbers.
        Model::AxeFx2Xl => vec![((cab >> 7) & 0x7F) as u8, (cab & 0x7F) as u8],
        Model::AxeFx2XlPlus => vec![(cab & 0x7F) as u8, ((cab >> 7) & 0x7F) as u8],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A dump with `words` in chunks of four.
    fn sample(kind: DumpKind, words: &[u64]) -> Vec<u8> {
        let (header, data, footer) = kind.functions();
        let put =
            |out: &mut Vec<u8>, word: u64| out.extend((0..kind.groups()).map(|g| ((word >> (7 * g)) & 0x7F) as u8));
        let mut bytes = Frame::new(0x06, header, [0x7F, 0x00, words.len() as u8, 0]).to_bytes();
        for chunk in words.chunks(4) {
            let mut payload = vec![chunk.len() as u8, 0];
            chunk.iter().for_each(|&word| put(&mut payload, word));
            bytes.extend(Frame::new(0x06, data, payload).to_bytes());
        }
        let mut payload = Vec::new();
        put(&mut payload, words.iter().fold(0, |acc, word| acc ^ word));
        bytes.extend(Frame::new(0x06, footer, payload).to_bytes());
        bytes
    }

    fn preset_words(name: &str) -> Vec<u64> {
        let mut words = vec![0x801, 0];
        words.extend(name.bytes().map(u64::from));
        words.resize(40, 0);
        words.push(0x1D00FD); // wider than sixteen bits
        words
    }

    #[test]
    fn a_preset_round_trips() {
        let bytes = sample(DumpKind::Preset, &preset_words("Clean Chorus"));
        let dump = Dump::parse(&bytes).unwrap();
        assert_eq!((dump.kind, dump.model, dump.word_count()), (DumpKind::Preset, 0x06, 41));
        assert_eq!(dump.name(), "Clean Chorus");
        assert_eq!(dump.to_bytes(), bytes);
    }

    #[test]
    fn a_cab_names_itself_four_characters_to_a_word() {
        let mut words: Vec<u64> =
            b"My Cab 4x12 V30\0".chunks(4).map(|c| u64::from(u32::from_le_bytes([c[0], c[1], c[2], c[3]]))).collect();
        words.resize(8, 0);
        words.push(0x7_FFFF_FFFF); // all 35 bits, as an erased slot reads
        let dump = Dump::parse(&sample(DumpKind::Cab, &words)).unwrap();
        assert_eq!((dump.kind, dump.name().as_str()), (DumpKind::Cab, "My Cab 4x12 V30"));
    }

    #[test]
    fn sending_readdresses_the_dump() {
        let mut bytes = sample(DumpKind::Preset, &preset_words("X"));
        bytes[6] = 0x03; // a file that names a slot
        bytes[10] = crate::checksum(&bytes[..10]);
        let frames = Dump::parse(&bytes).unwrap().frames(Model::AxeFx2XlPlus, 0);
        assert_eq!((frames[0].model, frames[0].function, &frames[0].payload[..2]), (0x07, 0x77, &[0x7F, 0x00][..]));
        assert_eq!(frames.last().map(|f| (f.model, f.function)), Some((0x07, 0x79)));

        let cab = Dump::parse(&sample(DumpKind::Cab, &[1, 2, 3])).unwrap();
        let header = |model| cab.frames(model, 300)[0].payload.clone();
        assert_eq!(header(Model::AxeFx2Xl), [0x02, 0x2C, 3, 0]);
        assert_eq!(header(Model::AxeFx2XlPlus), [0x2C, 0x02, 3, 0]);
        assert_eq!(cab.frames(Model::AxeFx2, 5)[0].payload, [5, 3, 0]);
    }

    #[test]
    fn damage_is_reported() {
        let good = sample(DumpKind::Preset, &preset_words("X"));
        let mut flipped = good.clone();
        flipped[20] ^= 1;
        assert!(matches!(Dump::parse(&flipped), Err(DumpError::Frame(FrameError::Checksum { .. }))));
        // A consistent message with the wrong content fails the footer instead.
        let mut wrong = sample(DumpKind::Preset, &[1, 2, 3, 4]);
        let other = sample(DumpKind::Preset, &[1, 2, 3, 5]);
        let footer_len = 11;
        let at = wrong.len() - footer_len;
        wrong.truncate(at);
        wrong.extend_from_slice(&other[other.len() - footer_len..]);
        assert_eq!(Dump::parse(&wrong), Err(DumpError::Checksum));

        assert_eq!(Dump::parse(&good[..good.len() - footer_len]), Err(DumpError::Incomplete));
        assert_eq!(Dump::parse(b"hello"), Err(DumpError::NotADump));
        // Well-formed messages of another kind.
        let other = Frame::new(0x06, 0x0F, *b"name").to_bytes();
        assert_eq!(Dump::parse(&other), Err(DumpError::NotADump));
    }
}
