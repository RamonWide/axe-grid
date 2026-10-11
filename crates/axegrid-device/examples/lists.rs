//! Read-only hardware probe for choice lists: for one block of the current
//! preset it reads every parameter (function 01) and asks for the strings
//! (function 28) and metadata (function 16) of each one that reports a
//! highest choice index. It never sends a message that sets, stores, selects
//! or uploads anything.
//!
//! usage: cargo run -p axegrid-device --example lists -- "<midi in>" "<midi out>" [block] [param ...]
//!
//! Without a block it lists the blocks of the current preset. With parameter
//! IDs it asks for exactly those, whatever they report.

use std::time::{Duration, Instant};

use axegrid_device::midi::MidiTransport;
use axegrid_device::Transport;
use axegrid_model::{blocks, Grid};
use axegrid_sysex::{function, ParamAddress, ParamValue, Reply, Request};

/// How long to wait for the first message of an answer, and for the next one.
const FIRST: Duration = Duration::from_secs(12);
const NEXT: Duration = Duration::from_millis(400);
const SHOWN: usize = 6;

/// Sends one request and collects what comes back until the line goes quiet.
fn ask(port: &mut MidiTransport, model: u8, request: &Request) -> std::io::Result<(Vec<Reply>, usize, Duration)> {
    port.send(&request.to_frame(model).to_bytes())?;
    let started = Instant::now();
    let (mut replies, mut bytes, mut wait) = (Vec::new(), 0, FIRST);
    while let Some(message) = port.receive(wait)? {
        bytes += message.len();
        match Reply::parse(&message) {
            // Beats and other chatter are not part of the answer.
            Ok((_, Reply::TempoBeat | Reply::Tuner { .. })) => continue,
            Ok((_, reply)) => replies.push(reply),
            Err(error) => println!("   (not parsed, {} bytes: {error})", message.len()),
        }
        wait = NEXT;
        let done = matches!(replies.last(), Some(Reply::ListEnd | Reply::Status { .. }));
        if done || request.function() != function::PARAM_DUMP {
            break;
        }
    }
    Ok((replies, bytes, started.elapsed()))
}

fn strings(
    port: &mut MidiTransport,
    model: u8,
    address: ParamAddress,
    value: Option<&ParamValue>,
) -> std::io::Result<()> {
    let reported =
        value.map_or_else(String::new, |v| format!(" value {} max {} text {:?}", v.value, v.extra[1], v.text));
    println!("\nparam {}{reported}", address.param);
    let (replies, bytes, took) = ask(port, model, &Request::ParamStrings(address))?;
    match replies.first() {
        Some(Reply::ParamStrings(list)) => {
            let first: Vec<&str> = list.iter().take(SHOWN).map(String::as_str).collect();
            let last = list.last().map(String::as_str).unwrap_or_default();
            println!("   28: {} strings, {bytes} bytes, {took:?}; first {first:?}, last {last:?}", list.len());
        }
        other => println!("   28: {other:?} ({bytes} bytes, {took:?})"),
    }
    let (replies, _, took) = ask(port, model, &Request::ParamInfo(address))?;
    println!("   16: {:?} ({took:?})", replies.first());
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output, rest @ ..] = args.as_slice() else {
        return Err("usage: lists <midi in> <midi out> [block] [param ...]".into());
    };
    let mut port = MidiTransport::open(input, output)?;
    let (replies, _, _) = ask(&mut port, 0, &Request::Probe)?;
    if replies.is_empty() {
        println!("No answer to the probe. Nothing else was sent.");
        return Ok(());
    }
    // Defaults to the XL; set AXE_MODEL to another model byte in hex (03, 07).
    let model = std::env::var("AXE_MODEL").ok().and_then(|m| u8::from_str_radix(&m, 16).ok()).unwrap_or(0x06);

    let Some(block) = rest.first() else {
        let (replies, _, _) = ask(&mut port, model, &Request::Grid)?;
        if let Some(Reply::Grid(cells)) = replies.first() {
            for id in Grid::from_cells(cells).blocks() {
                println!("{id:>4} {}", blocks::label(id));
            }
        }
        println!(" 139 Input Gate\n 140 Output\n 141 Controllers");
        return Ok(());
    };
    let block: u16 = block.parse()?;
    if rest.len() > 1 {
        for param in &rest[1..] {
            strings(&mut port, model, ParamAddress { block, param: param.parse()? }, None)?;
        }
        return Ok(());
    }
    let (replies, bytes, took) = ask(&mut port, model, &Request::ParamDump { block })?;
    let values: Vec<ParamValue> = replies
        .into_iter()
        .filter_map(|reply| match reply {
            Reply::ParamDumpItem(value) => Some(value),
            _ => None,
        })
        .collect();
    println!("{}: {} parameters, {bytes} bytes, {took:?}", blocks::label(block), values.len());
    for value in values.iter().filter(|v| v.extra[1] > 0) {
        strings(&mut port, model, value.address, Some(value))?;
    }
    Ok(())
}
