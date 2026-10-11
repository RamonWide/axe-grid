//! MIDI port transport.

use std::io;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use midir::{MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};

use crate::session::Transport;

const CLIENT_NAME: &str = "axegrid";

fn other(error: impl ToString) -> io::Error {
    io::Error::other(error.to_string())
}

/// Names of the available `(input, output)` ports.
pub fn port_names() -> io::Result<(Vec<String>, Vec<String>)> {
    let input = MidiInput::new(CLIENT_NAME).map_err(other)?;
    let output = MidiOutput::new(CLIENT_NAME).map_err(other)?;
    let inputs = input.ports().iter().filter_map(|p| input.port_name(p).ok()).collect();
    let outputs = output.ports().iter().filter_map(|p| output.port_name(p).ok()).collect();
    Ok((inputs, outputs))
}

pub struct MidiTransport {
    output: MidiOutputConnection,
    incoming: Receiver<Vec<u8>>,
    // Held so the input callback keeps running.
    _input: MidiInputConnection<()>,
}

impl MidiTransport {
    pub fn open(input_name: &str, output_name: &str) -> io::Result<Self> {
        let mut input = MidiInput::new(CLIENT_NAME).map_err(other)?;
        // SysEx is filtered out by default.
        input.ignore(midir::Ignore::Time | midir::Ignore::ActiveSense);
        let output = MidiOutput::new(CLIENT_NAME).map_err(other)?;

        let missing = |name: &str| io::Error::new(io::ErrorKind::NotFound, format!("no MIDI port named {name:?}"));
        let in_port = input
            .ports()
            .into_iter()
            .find(|p| input.port_name(p).is_ok_and(|n| n == input_name))
            .ok_or_else(|| missing(input_name))?;
        let out_port = output
            .ports()
            .into_iter()
            .find(|p| output.port_name(p).is_ok_and(|n| n == output_name))
            .ok_or_else(|| missing(output_name))?;

        let (sender, incoming) = mpsc::channel();
        let mut partial = Vec::new();
        let connection = input
            .connect(
                &in_port,
                "axegrid in",
                move |_, bytes, _| {
                    // A long message may arrive in several callbacks.
                    partial.extend_from_slice(bytes);
                    let complete: Vec<Vec<u8>> = axegrid_sysex::split_messages(&partial).map(<[u8]>::to_vec).collect();
                    let consumed = partial.iter().rposition(|&b| b == 0xF7).map_or(0, |i| i + 1);
                    partial.drain(..consumed);
                    for message in complete {
                        let _ = sender.send(message);
                    }
                },
                (),
            )
            .map_err(other)?;
        let output = output.connect(&out_port, "axegrid out").map_err(other)?;
        Ok(Self { output, incoming, _input: connection })
    }
}

impl Transport for MidiTransport {
    fn send(&mut self, message: &[u8]) -> io::Result<()> {
        self.output.send(message).map_err(other)
    }

    fn receive(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        match self.incoming.recv_timeout(timeout) {
            Ok(message) => Ok(Some(message)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(io::Error::new(io::ErrorKind::BrokenPipe, "MIDI input closed")),
        }
    }
}
