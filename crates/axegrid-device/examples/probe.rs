//! Read-only hardware probe: sends queries that only ask for state and prints
//! every byte that comes back. It never sends a message that sets, stores,
//! selects or uploads anything.
//!
//! usage: cargo run -p axegrid-device --example probe -- "<midi in>" "<midi out>"

use std::time::{Duration, Instant};

use axegrid_device::midi::MidiTransport;
use axegrid_device::Transport;
use axegrid_sysex::{Frame, ParamAddress, Reply, Request};

/// How long to listen after each query.
const LISTEN: Duration = Duration::from_millis(900);
const HEX_PREVIEW: usize = 48;

fn hex(bytes: &[u8]) -> String {
    let shown: Vec<String> = bytes.iter().take(HEX_PREVIEW).map(|b| format!("{b:02X}")).collect();
    let more = if bytes.len() > HEX_PREVIEW { format!(" .. ({} bytes)", bytes.len()) } else { String::new() };
    shown.join(" ") + &more
}

/// Sends one query and returns the model byte of the first valid reply.
fn ask(port: &mut MidiTransport, label: &str, frame: &Frame) -> std::io::Result<Option<u8>> {
    let bytes = frame.to_bytes();
    println!("\n>> {label}\n   {}", hex(&bytes));
    port.send(&bytes)?;
    let deadline = Instant::now() + LISTEN;
    let mut model = None;
    while let Some(message) = port.receive(deadline.saturating_duration_since(Instant::now()))? {
        println!("<< {}", hex(&message));
        match Reply::parse(&message) {
            Ok((from, reply)) => {
                model.get_or_insert(from);
                let text = format!("{reply:?}");
                println!("   {}", text.chars().take(300).collect::<String>());
            }
            Err(error) => println!("   (not parsed: {error})"),
        }
    }
    Ok(model)
}

fn args_want_dump() -> bool {
    std::env::var_os("PROBE_DUMP").is_some()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output] = args.as_slice() else {
        return Err("usage: probe <midi in> <midi out>".into());
    };
    let mut port = MidiTransport::open(input, output)?;

    let Some(model) = ask(&mut port, "who is there (function 00, model 7F)", &Request::Probe.to_frame(0))? else {
        println!("\nNo answer to the probe. Nothing else was sent.");
        return Ok(());
    };
    println!("\nDevice model byte: {model:#04X}");

    // Queries only: every request below has an empty payload or the
    // "query" form of its payload.
    let queries = [
        ("firmware version (08)", Request::FirmwareVersion),
        ("preset number (14)", Request::PresetNumber),
        ("preset name (0F)", Request::PresetName),
        ("grid layout (20)", Request::Grid),
        ("block states (0E)", Request::BlockStates),
        ("preset edited flag (2A)", Request::PresetEdited),
        ("cpu load (13)", Request::CpuLoad),
        ("amp 1 type, query (02 set=0)", Request::GetParam(ParamAddress { block: 106, param: 0 })),
        ("amp 1 drive, query (02 set=0)", Request::GetParam(ParamAddress { block: 106, param: 1 })),
        ("amp 1 X/Y, query (11 set=0)", Request::BlockXy(106, None)),
    ];
    for (label, request) in queries {
        ask(&mut port, label, &request.to_frame(model))?;
    }
    if args_want_dump() {
        // Function 01 carries only a block ID. Check that it is a read by
        // comparing the preset-edited flag before and after.
        ask(&mut port, "amp 1 parameter dump (01)", &Request::ParamDump { block: 106 }.to_frame(model))?;
        ask(&mut port, "preset edited flag after the dump (2A)", &Request::PresetEdited.to_frame(model))?;
    }
    Ok(())
}
