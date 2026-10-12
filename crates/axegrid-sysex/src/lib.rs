//! Fractal Audio MIDI System Exclusive protocol, as spoken by the Axe-Fx II
//! family of processors.

pub mod codec;
pub mod frame;
pub mod message;
pub mod transfer;

pub use frame::{checksum, split_messages, Frame, FrameError};
pub use message::{
    function, BlockState, GridCell, LooperState, Model, ModifierValue, ParamAddress, ParamInfo, ParamValue, Reply,
    ReplyError, Request, XyState, MODIFIER_FIELDS,
};
pub use transfer::{Dump, DumpError, DumpKind};
