//! Connecting to a device.

use axegrid_device::{Session, SimDevice, Transport};
use axegrid_sysex::{Model, Reply, Request};
use serde_json::{json, Value};

use crate::command::{always, connected, disconnected, opt, req, Args, CommandError, CommandSpec};
use crate::Engine;

fn list_ports(_: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    let (inputs, outputs) = axegrid_device::midi::port_names().map_err(|e| CommandError::Io(e.to_string()))?;
    Ok(json!({ "inputs": inputs, "outputs": outputs }))
}

fn connect(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let (input, output) = (args.str("input")?, args.str("output")?);
    let transport =
        axegrid_device::midi::MidiTransport::open(input, output).map_err(|e| CommandError::Io(e.to_string()))?;
    let boxed: Box<dyn Transport> = Box::new(transport);
    engine.attach(Session::connect(boxed)?, false)?;
    Ok(engine.inspect())
}

/// What the unit's own USB MIDI port is called, in upper case.
const USB_PORT_MARK: &str = "AXE-FX";

/// Port pairs worth probing, the unit's USB port first: every name that is
/// both an input and an output.
fn candidate_ports(inputs: &[String], outputs: &[String]) -> Vec<String> {
    let mut both: Vec<String> = inputs.iter().filter(|name| outputs.contains(name)).cloned().collect();
    both.dedup();
    // A stable sort keeps the system's order within each group.
    both.sort_by_key(|name| !name.to_ascii_uppercase().contains(USB_PORT_MARK));
    both
}

fn connect_auto(engine: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    let (inputs, outputs) = axegrid_device::midi::port_names().map_err(|e| CommandError::Io(e.to_string()))?;
    let candidates = candidate_ports(&inputs, &outputs);
    let mut tried = Vec::new();
    for port in &candidates {
        let opened = axegrid_device::midi::MidiTransport::open(port, port);
        let session = opened.map_err(|e| e.to_string()).and_then(|transport| {
            let boxed: Box<dyn Transport> = Box::new(transport);
            Session::connect(boxed).map_err(|e| e.to_string())
        });
        match session {
            Ok(session) => {
                engine.attach(session, false)?;
                let mut state = engine.inspect();
                state["port"] = json!(port);
                return Ok(state);
            }
            Err(reason) => tried.push(format!("{port}: {reason}")),
        }
    }
    let detail = if tried.is_empty() { "this computer has no MIDI port pairs".to_owned() } else { tried.join("; ") };
    Err(CommandError::NotFound(format!("no Axe-Fx II answered ({detail})")))
}

fn connect_simulator(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let model = match args.opt_str("model")? {
        None | Some("XL") => Model::AxeFx2Xl,
        Some("II") => Model::AxeFx2,
        Some("XL+") => Model::AxeFx2XlPlus,
        Some(other) => {
            return Err(CommandError::BadParam {
                name: "model",
                problem: format!("{other:?} is not one of \"II\", \"XL\", \"XL+\""),
            })
        }
    };
    let boxed: Box<dyn Transport> = Box::new(SimDevice::new(model));
    engine.attach(Session::connect(boxed)?, true)?;
    Ok(engine.inspect())
}

fn disconnect(engine: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    if engine.state.tuner_on {
        // Do not leave the unit muted in its tuner screen.
        let _ = engine.run("tuner.off", &Value::Null);
    }
    if let Some(session) = engine.session.take() {
        session.disconnect();
    }
    engine.state = crate::State::default();
    engine.forget_device();
    Ok(Value::Null)
}

fn refresh(engine: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    engine.refresh_preset()?;
    Ok(engine.inspect())
}

fn cpu_load(engine: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    if let Reply::CpuLoad(load) = engine.session()?.request(&Request::CpuLoad)? {
        engine.state.cpu_load = Some(load);
    }
    Ok(json!({ "cpu_load": engine.state.cpu_load }))
}

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "device.list_ports",
        label: "List MIDI Ports",
        menu: "",
        shortcut: "",
        doc: "Names of the MIDI input and output ports on this computer.",
        params: &[],
        enabled: always,
        run: list_ports,
    },
    CommandSpec {
        id: "device.connect",
        label: "Connect",
        menu: "",
        shortcut: "",
        doc: "Open a pair of MIDI ports, find the Axe-Fx II on them and read its current preset.",
        params: &[
            req("input", "string", "MIDI input port name, as listed by device.list_ports"),
            req("output", "string", "MIDI output port name"),
        ],
        enabled: disconnected,
        run: connect,
    },
    CommandSpec {
        id: "device.connect_auto",
        label: "Find My Axe-Fx",
        menu: "Device/Find My Axe-Fx",
        shortcut: "",
        doc: "Look for an Axe-Fx II and connect to the first one that answers: the unit's own USB port first,               then every other MIDI port that has both an input and an output.",
        params: &[],
        enabled: disconnected,
        run: connect_auto,
    },
    CommandSpec {
        id: "device.connect_simulator",
        label: "Connect to Simulator",
        menu: "Device/Connect to Simulator",
        shortcut: "",
        doc: "Connect to a built-in stand-in device with three presets. No hardware is touched.",
        params: &[opt("model", "string", "\"II\", \"XL\" (default) or \"XL+\"")],
        enabled: disconnected,
        run: connect_simulator,
    },
    CommandSpec {
        id: "device.disconnect",
        label: "Disconnect",
        menu: "Device/Disconnect",
        shortcut: "",
        doc: "Tell the device the editor is leaving and close the ports.",
        params: &[],
        enabled: connected,
        run: disconnect,
    },
    CommandSpec {
        id: "device.refresh",
        label: "Refresh Preset",
        menu: "Device/Refresh Preset",
        shortcut: "F5",
        doc: "Re-read preset number, name, scene, grid and block states from the device.",
        params: &[],
        enabled: connected,
        run: refresh,
    },
    CommandSpec {
        id: "device.cpu_load",
        label: "Read CPU Load",
        menu: "Device/Read CPU Load",
        shortcut: "",
        doc: "Ask the device for its current DSP load.",
        params: &[],
        enabled: connected,
        run: cpu_load,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_usb_port_is_tried_first() {
        let names = |list: &[&str]| list.iter().map(|name| (*name).to_owned()).collect::<Vec<_>>();
        let inputs = names(&["UCX II Midi Port 1", "AXE-FX II Audio Intfc", "Keyboard"]);
        let outputs = names(&["Microsoft GS Wavetable Synth", "UCX II Midi Port 1", "AXE-FX II Audio Intfc"]);
        // Ports without both directions cannot hold a conversation.
        assert_eq!(candidate_ports(&inputs, &outputs), ["AXE-FX II Audio Intfc", "UCX II Midi Port 1"]);
        assert!(candidate_ports(&[], &outputs).is_empty());
    }
}
