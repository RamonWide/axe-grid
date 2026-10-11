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
