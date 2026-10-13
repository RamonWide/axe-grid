//! Hardware check of the messages that change the edit buffer. Every stage
//! puts back what it changed, and none of them stores a preset: the store
//! request is never built here.
//!
//! usage: cargo run -p axegrid-device --example hwtest -- "<midi in>" "<midi out>" <stage> [block] [arg]
//! stages: state | param | bypass | xy | scene | preset | rename | place | disconnect
//!         name | select | modget | modset (these four put nothing back)

use std::time::{Duration, Instant};

use axegrid_device::midi::MidiTransport;
use axegrid_device::Transport;
use axegrid_model::{blocks, params};
use axegrid_sysex::{Frame, ParamAddress, Reply, Request, XyState, MODIFIER_FIELDS};

/// How long to listen after each message.
const LISTEN: Duration = Duration::from_millis(700);
const HEX_PREVIEW: usize = 40;
/// Drive is moved by about one percent of its range and put back.
const PARAM_STEP: u16 = 655;
const TEST_NAME: &str = "AXEGRID TEST";

fn hex(bytes: &[u8]) -> String {
    let shown: Vec<String> = bytes.iter().take(HEX_PREVIEW).map(|b| format!("{b:02X}")).collect();
    let more = if bytes.len() > HEX_PREVIEW { format!(" .. ({} bytes)", bytes.len()) } else { String::new() };
    shown.join(" ") + &more
}

struct Bench {
    pub port: MidiTransport,
    model: u8,
}

impl Bench {
    fn send(&mut self, label: &str, frame: &Frame) -> std::io::Result<Vec<Reply>> {
        let bytes = frame.to_bytes();
        println!("\n>> {label}\n   {}", hex(&bytes));
        self.port.send(&bytes)?;
        let deadline = Instant::now() + LISTEN;
        let mut replies = Vec::new();
        while let Some(message) = self.port.receive(deadline.saturating_duration_since(Instant::now()))? {
            match Reply::parse(&message) {
                // Tempo beats arrive all the time and say nothing here.
                Ok((_, Reply::TempoBeat)) => {}
                Ok((_, reply)) => {
                    println!("<< {}", hex(&message));
                    let text = format!("{reply:?}");
                    println!("   {}", text.chars().take(400).collect::<String>());
                    replies.push(reply);
                }
                Err(error) => println!("<< {}\n   (not parsed: {error})", hex(&message)),
            }
        }
        Ok(replies)
    }

    fn ask(&mut self, label: &str, request: &Request) -> std::io::Result<Vec<Reply>> {
        let frame = request.to_frame(self.model);
        self.send(label, &frame)
    }

    fn edited(&mut self, when: &str) -> std::io::Result<()> {
        self.ask(&format!("preset edited flag, {when}"), &Request::PresetEdited).map(|_| ())
    }

    fn state(&mut self) -> std::io::Result<()> {
        self.ask("preset number", &Request::PresetNumber)?;
        self.ask("preset name", &Request::PresetName)?;
        let replies = self.ask("block states", &Request::BlockStates)?;
        for reply in replies {
            if let Reply::BlockStates(states) = reply {
                for s in states {
                    println!("   {:>3} {:<14} engaged={} {:?}", s.block, blocks::label(s.block), s.engaged, s.xy);
                }
            }
        }
        self.edited("now")
    }

    fn param_value(&mut self, label: &str, request: &Request) -> std::io::Result<Option<u16>> {
        Ok(self.ask(label, request)?.into_iter().find_map(|r| match r {
            Reply::Param(value) => u16::try_from(value.value).ok(),
            _ => None,
        }))
    }
}

/// Reads a modifier field from the command line. A unit asked for a field
/// past the last one stopped answering, so nothing else gets through.
fn modifier_field(arg: Option<&String>) -> Result<u16, Box<dyn std::error::Error>> {
    let field: u16 = arg.ok_or("a modifier field, 0 to 14, is needed")?.parse()?;
    if field >= MODIFIER_FIELDS {
        return Err(format!("{field} is not a modifier field; the last one is {}", MODIFIER_FIELDS - 1).into());
    }
    Ok(field)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output, stage, rest @ ..] = args.as_slice() else {
        return Err("usage: hwtest <midi in> <midi out> <stage> [block] [arg]".into());
    };
    let block: u16 = rest.first().map(|s| s.parse()).transpose()?.unwrap_or(106);
    let number: Option<u16> = rest.get(1).map(|s| s.parse()).transpose()?;

    let mut bench = Bench { port: MidiTransport::open(input, output)?, model: 0 };
    let probe = bench.send("probe", &Request::Probe.to_frame(0))?;
    if probe.is_empty() {
        println!("No answer to the probe. Nothing else was sent.");
        return Ok(());
    }
    // Defaults to the XL; set AXE_MODEL to another model byte in hex (03, 07).
    bench.model = std::env::var("AXE_MODEL").ok().and_then(|m| u8::from_str_radix(&m, 16).ok()).unwrap_or(0x06);

    match stage.as_str() {
        "state" => bench.state()?,
        "get" => {
            // Read-only: every remaining argument is a parameter ID.
            for id in &rest[1.min(rest.len())..] {
                let address = ParamAddress { block, param: id.parse()? };
                bench.ask(&format!("read {id}"), &Request::GetParam(address))?;
                bench.ask(&format!("info {id}"), &Request::ParamInfo(address))?;
            }
        }
        "param" => {
            let address = ParamAddress { block, param: number.unwrap_or(1) };
            bench.edited("before")?;
            let Some(before) = bench.param_value("read", &Request::GetParam(address))? else {
                return Err("no value came back; nothing was changed".into());
            };
            let target = if before > PARAM_STEP { before - PARAM_STEP } else { before + PARAM_STEP };
            let applied = bench.param_value("set", &Request::SetParam(address, target))?;
            let read_back = bench.param_value("read back", &Request::GetParam(address))?;
            bench.edited("after the set")?;
            bench.param_value("restore", &Request::SetParam(address, before))?;
            let restored = bench.param_value("read after restore", &Request::GetParam(address))?;
            println!("\nRESULT before={before} target={target} applied={applied:?} read_back={read_back:?} restored={restored:?}");
        }
        "bypass" => {
            let param = number.unwrap_or(params::SHARED_BYPASS);
            let address = ParamAddress { block, param };
            bench.ask("block states before", &Request::BlockStates)?;
            let Some(before) = bench.param_value("read bypass parameter", &Request::GetParam(address))? else {
                return Err("no value came back; nothing was changed".into());
            };
            let flipped = u16::from(before == 0);
            bench.param_value("set flipped", &Request::SetParam(address, flipped))?;
            bench.ask("block states after the flip", &Request::BlockStates)?;
            bench.param_value("restore", &Request::SetParam(address, before))?;
            bench.ask("block states after restore", &Request::BlockStates)?;
            println!("\nRESULT bypass parameter {param} before={before} flipped={flipped}");
        }
        "xy" => {
            let before = bench.ask("query", &Request::BlockXy(block, None))?.into_iter().find_map(|r| match r {
                Reply::BlockXy { xy, .. } => Some(xy),
                _ => None,
            });
            let Some(before) = before else {
                return Err("no X/Y state came back; nothing was changed".into());
            };
            let other = if before == XyState::X { XyState::Y } else { XyState::X };
            bench.ask("block states before", &Request::BlockStates)?;
            bench.ask("set other", &Request::BlockXy(block, Some(other)))?;
            bench.ask("query after set", &Request::BlockXy(block, None))?;
            bench.ask("block states after set", &Request::BlockStates)?;
            bench.ask("restore", &Request::BlockXy(block, Some(before)))?;
            bench.ask("query after restore", &Request::BlockXy(block, None))?;
            println!("\nRESULT before={before:?} other={other:?}");
        }
        "scene" => {
            let before = bench.ask("query scene", &Request::Scene(None))?.into_iter().find_map(|r| match r {
                Reply::Scene(scene) => Some(scene),
                _ => None,
            });
            let Some(before) = before else {
                return Err("no scene came back; nothing was changed".into());
            };
            let other = (before + 1) % 8;
            bench.ask("select other scene", &Request::Scene(Some(other)))?;
            bench.ask("block states in other scene", &Request::BlockStates)?;
            bench.ask("restore scene", &Request::Scene(Some(before)))?;
            bench.ask("query scene after restore", &Request::Scene(None))?;
            println!("\nRESULT before={before} other={other}");
        }
        "preset" => {
            let before = bench.ask("preset number", &Request::PresetNumber)?.into_iter().find_map(|r| match r {
                Reply::PresetNumber(n) => Some(n),
                _ => None,
            });
            let Some(before) = before else {
                return Err("no preset number came back; nothing was changed".into());
            };
            let other = number.unwrap_or(before + 1);
            bench.ask("select other preset", &Request::SelectPreset(other))?;
            bench.ask("preset number", &Request::PresetNumber)?;
            bench.ask("preset name", &Request::PresetName)?;
            bench.ask("select original preset", &Request::SelectPreset(before))?;
            bench.ask("preset number", &Request::PresetNumber)?;
            bench.ask("preset name", &Request::PresetName)?;
            bench.edited("after reload")?;
            println!("\nRESULT before={before} other={other}");
        }
        "rename" => {
            bench.ask("preset name before", &Request::PresetName)?;
            bench.ask("rename in the edit buffer", &Request::SetPresetName(TEST_NAME.into()))?;
            bench.ask("preset name after", &Request::PresetName)?;
            bench.edited("after rename")?;
            println!("\nRun the preset stage with the current number to reload the stored name.");
        }
        "place" => {
            let cell = number.ok_or("place needs a block and a cell index")? as u8;
            bench.ask("grid before", &Request::Grid)?;
            bench.ask("place", &Request::PlaceBlock { block, cell })?;
            bench.ask("grid after", &Request::Grid)?;
            println!("\nReload the preset to undo.");
        }
        "cable" => {
            // cable <from cell> <to cell> [0 to remove]
            let to = number.ok_or("cable needs two cell indexes")? as u8;
            let connect = rest.get(2).is_none_or(|flag| flag != "0");
            bench.ask("cable", &Request::Cable { from: block as u8, to, connect })?;
            let replies = bench.ask("grid after", &Request::Grid)?;
            for reply in replies {
                if let Reply::Grid(cells) = reply {
                    for (index, cell) in cells.iter().enumerate().filter(|(_, c)| c.block != 0) {
                        println!("   cell {index}: block {} routing {:#06b}", cell.block, cell.routing);
                    }
                }
            }
        }
        "channel" => {
            bench.ask("midi channel", &Request::MidiChannel)?;
        }
        "tuner" => {
            // tuner <zero-based midi channel>: controller 15 on, listen, off.
            let channel = (block as u8) & 0x0F;
            println!(">> tuner on");
            bench.port.send(&[0xB0 | channel, 15, 127])?;
            let deadline = Instant::now() + Duration::from_millis(1500);
            let mut seen = 0;
            while let Some(message) = bench.port.receive(deadline.saturating_duration_since(Instant::now()))? {
                if seen < 6 {
                    println!("<< {}   {:?}", hex(&message), Reply::parse(&message).map(|(_, r)| r));
                }
                seen += 1;
            }
            println!(">> tuner off ({seen} messages seen)");
            bench.port.send(&[0xB0 | channel, 15, 0])?;
            let deadline = Instant::now() + Duration::from_millis(700);
            let mut after = 0;
            while bench.port.receive(deadline.saturating_duration_since(Instant::now()))?.is_some() {
                after += 1;
            }
            println!("   {after} messages in the 0.7 s after switching off");
        }
        "looper" => {
            // looper <zero-based midi channel>: status reporting on, record, stop, play, stop, off.
            let channel = (block as u8) & 0x0F;
            bench.ask("looper status on", &Request::LooperStatus(true))?;
            for (label, controller, value) in
                [("record on", 28, 127), ("record off", 28, 0), ("play on", 29, 127), ("play off", 29, 0)]
            {
                println!(">> controller {controller} = {value} ({label})");
                bench.port.send(&[0xB0 | channel, controller, value])?;
                let deadline = Instant::now() + Duration::from_millis(900);
                while let Some(message) = bench.port.receive(deadline.saturating_duration_since(Instant::now()))? {
                    if !matches!(Reply::parse(&message), Ok((_, Reply::TempoBeat))) {
                        println!("<< {}   {:?}", hex(&message), Reply::parse(&message).map(|(_, r)| r));
                    }
                }
            }
            bench.ask("looper status off", &Request::LooperStatus(false))?;
        }
        "name" => {
            // name <preset>: read-only, the name of a stored preset without loading it.
            bench.ask("name of the stored preset", &Request::PresetNameAt(block))?;
        }
        "select" => {
            // select <preset>: loads a preset and stays there. Whatever was
            // edited in the buffer is dropped, so the flag is shown first.
            bench.edited("before leaving")?;
            bench.ask("select preset", &Request::SelectPreset(block))?;
            bench.state()?;
        }
        "modget" => {
            // modget <block> <param> <field>: read-only, one field per run.
            let address = ParamAddress { block, param: number.ok_or("modget needs a block and a parameter")? };
            let field = modifier_field(rest.get(2))?;
            bench.ask(&format!("modifier field {field}"), &Request::Modifier { address, field, value: None })?;
            bench.edited("after the read")?;
        }
        "modset" => {
            // modset <block> <param> <field> <value>: the one write of the
            // run. Read it back with modget in a run of its own.
            let address = ParamAddress { block, param: number.ok_or("modset needs a block and a parameter")? };
            let field = modifier_field(rest.get(2))?;
            let value: u16 = rest.get(3).ok_or("modset needs a value")?.parse()?;
            let replies = bench.ask(
                &format!("set modifier field {field} to {value}"),
                &Request::Modifier { address, field, value: Some(value) },
            )?;
            println!("\nRESULT field={field} value={value} answered={}", !replies.is_empty());
        }
        "disconnect" => {
            bench.ask("disconnect", &Request::Disconnect)?;
            bench.ask("preset number afterwards", &Request::PresetNumber)?;
        }
        other => return Err(format!("unknown stage {other:?}").into()),
    }
    Ok(())
}
