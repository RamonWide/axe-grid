//! Fractal Audio MIDI System Exclusive protocol, as spoken by the Axe-Fx II
//! family of processors.

pub mod codec;
pub mod frame;
pub mod message;
pub mod transfer;

pub use frame::{checksum, split_messages, Frame, FrameError};
pub use message::{
    function, BlockState, GridCell, Model, ParamAddress, ParamInfo, ParamValue, Reply, ReplyError, Request, XyState,
};
pub use transfer::{Dump, DumpError, DumpKind};
