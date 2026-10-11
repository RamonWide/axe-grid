//! Talking to an Axe-Fx II: a request/reply session over any transport, a
//! MIDI port transport, and a simulated device.

#[cfg(feature = "midi")]
pub mod midi;
pub mod session;
pub mod sim;

pub use session::{PresetView, Session, SessionError, Transport};
pub use sim::SimDevice;
