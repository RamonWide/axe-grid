//! Things that change by themselves while the editor is connected: the
//! tuner, the tempo, and whatever the device reports without being asked.

use std::time::{Duration, Instant};

use axegrid_model::{blocks, params};
use axegrid_sysex::{ParamAddress, Reply};
use serde_json::{json, Value};

use crate::command::{connected, opt, req, Args, CommandError, CommandSpec};
use crate::{Engine, TunerReading};

/// MIDI controller the device's tuner listens on unless its owner changed it.
const TUNER_CONTROLLER: u8 = 15;
const CONTROL_CHANGE: u8 = 0xB0;
/// The looper's transport buttons: name, MIDI controller, bit in the status
/// byte the device reports, and whether the button fires once instead of
/// latching.
const LOOPER_BUTTONS: &[(&str, u8, u8, bool)] = &[
    ("record", 28, 0, false),
    ("play", 29, 1, false),
    ("once", 30, 2, false),
    ("stack", 31, 3, false),
    ("reverse", 32, 4, false),
    ("half", 120, 5, false),
    ("undo", 121, 6, true),
];
/// Slowest and fastest tempo the device accepts, in beats per minute.
pub const TEMPO_RANGE: (u64, u64) = (30, 250);
/// Two taps further apart than this start a new measurement.
const TAP_WINDOW: Duration = Duration::from_secs(2);

pub(crate) fn tempo_address() -> Result<ParamAddress, CommandError> {
    let controllers = blocks::by_id(blocks::CONTROLLERS).map(|b| b.effect_type);
    controllers
        .and_then(|effect| params::id(effect, "CONTROLLERS_TEMPO"))
        .map(|param| ParamAddress { block: blocks::CONTROLLERS, param })
        .ok_or_else(|| CommandError::NotFound("the tempo parameter is not in the parameter tables".into()))
}

fn set_tuner(engine: &mut Engine, on: bool) -> Result<Value, CommandError> {
    let status = CONTROL_CHANGE | (engine.state.midi_channel & 0x0F);
    engine.session()?.send_raw(&[status, TUNER_CONTROLLER, if on { 127 } else { 0 }])?;
    engine.state.tuner_on = on;
    engine.state.tuner = None;
    Ok(json!({ "tuner": on }))
}

fn tuner_on(engine: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    set_tuner(engine, true)
}

fn tuner_off(engine: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    set_tuner(engine, false)
}

fn tuner_toggle(engine: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    set_tuner(engine, !engine.state.tuner_on)
}

impl Engine {
    /// Reads the tempo of the current preset into the state.
    pub(crate) fn refresh_tempo(&mut self) -> Result<(), CommandError> {
        let value = self.session()?.get_param(tempo_address()?)?;
        self.state.tempo = u16::try_from(value.value).ok();
        Ok(())
    }

    /// Whether the looper button on `controller` is lit, when that is known.
    pub(crate) fn looper_lit(&self, controller: u8) -> Option<bool> {
        let bit = LOOPER_BUTTONS.iter().find(|button| button.1 == controller)?.2;
        self.state.looper.map(|bits| bits & (1 << bit) != 0)
    }

    /// Name `looper.press` knows the button on `controller` by.
    pub(crate) fn looper_button(controller: u8) -> Option<&'static str> {
        LOOPER_BUTTONS.iter().find(|button| button.1 == controller).map(|button| button.0)
    }

    pub(crate) fn looper_json(&self) -> Value {
        let lit = |bits: u8| -> Vec<&str> {
            LOOPER_BUTTONS.iter().filter(|button| bits & (1 << button.2) != 0).map(|button| button.0).collect()
        };
        json!({ "watched": self.state.looper.is_some(), "active": self.state.looper.map(lit) })
    }

    /// Asks the device to start or stop reporting what the looper does.
    pub(crate) fn watch_looper(&mut self, on: bool) -> Result<(), CommandError> {
        self.session()?.request(&axegrid_sysex::Request::LooperStatus(on))?;
        self.state.looper = on.then_some(self.state.looper.unwrap_or(0));
        Ok(())
    }

    /// Files away what the device sent without being asked. Returns whether
    /// the preset on the device changed under the editor.
    fn absorb_unsolicited(&mut self) -> Result<bool, CommandError> {
        let session = self.session()?;
        session.poll()?;
        let replies: Vec<Reply> = session.drain_unsolicited().collect();
        let mut preset_changed = false;
        for reply in replies {
            match reply {
                Reply::Tuner { note, string, cents } if self.state.tuner_on => {
                    self.state.tuner = Some(TunerReading { note, string, cents });
                }
                Reply::TempoBeat => self.state.beats += 1,
                Reply::LooperStatus(bits) => self.state.looper = Some(bits),
                Reply::FrontPanelChange => preset_changed = true,
                Reply::PresetNumber(number) if number != self.state.preset.number => preset_changed = true,
                _ => {}
            }
        }
        Ok(preset_changed)
    }
}

fn poll(engine: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    let refreshed = engine.absorb_unsolicited()?;
    if refreshed {
        engine.refresh_preset()?;
    }
    Ok(json!({ "refreshed": refreshed, "beats": engine.state.beats, "tuner": engine.tuner_json() }))
}

fn looper_watch(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    engine.watch_looper(args.opt_bool("on")?.unwrap_or(true))?;
    Ok(engine.looper_json())
}

fn looper_press(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let wanted = args.str("button")?;
    let &(name, controller, _, once) =
        LOOPER_BUTTONS.iter().find(|button| button.0 == wanted).ok_or_else(|| CommandError::BadParam {
            name: "button",
            problem: format!("{wanted:?} is not one of record, play, once, stack, reverse, half, undo"),
        })?;
    // Without an explicit state a latching button flips what the device last reported.
    let on = match args.opt_bool("on")? {
        Some(on) => on,
        None => once || !engine.looper_lit(controller).unwrap_or(false),
    };
    let status = CONTROL_CHANGE | (engine.state.midi_channel & 0x0F);
    engine.session()?.send_raw(&[status, controller, if on { 127 } else { 0 }])?;
    Ok(json!({ "button": name, "on": on }))
}

fn tempo_get(engine: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    engine.refresh_tempo()?;
    Ok(json!({ "bpm": engine.state.tempo }))
}

fn apply_tempo(engine: &mut Engine, bpm: u64) -> Result<Value, CommandError> {
    let bpm = bpm.clamp(TEMPO_RANGE.0, TEMPO_RANGE.1) as u16;
    let address = tempo_address()?;
    let before = match engine.state.tempo {
        Some(tempo) => u32::from(tempo),
        None => engine.value_before(address)?,
    };
    let applied = engine.session()?.set_param(address, bpm)?;
    engine.remember(&applied);
    engine.record_param(before, &applied);
    engine.state.tempo = u16::try_from(applied.value).ok();
    Ok(json!({ "bpm": engine.state.tempo }))
}

fn tempo_set(engine: &mut Engine, args: &Args<'_>) -> Result<Value, CommandError> {
    let bpm = args.int("bpm", TEMPO_RANGE.1)?;
    if bpm < TEMPO_RANGE.0 {
        return Err(CommandError::BadParam {
            name: "bpm",
            problem: format!("must be {} to {}", TEMPO_RANGE.0, TEMPO_RANGE.1),
        });
    }
    apply_tempo(engine, bpm)
}

/// Tempo for two taps `gap` apart, or `None` when they do not form a pair.
pub fn tapped_bpm(gap: Duration) -> Option<u64> {
    if gap.is_zero() || gap >= TAP_WINDOW {
        return None;
    }
    Some((60.0 / gap.as_secs_f64()).round() as u64)
}

fn tempo_tap(engine: &mut Engine, _: &Args<'_>) -> Result<Value, CommandError> {
    let now = Instant::now();
    let pair = engine.last_tap.replace(now).and_then(|before| tapped_bpm(now - before));
    match pair {
        Some(bpm) => apply_tempo(engine, bpm),
        None => Ok(json!({ "bpm": engine.state.tempo, "waiting_for_second_tap": true })),
    }
}

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "device.poll",
        label: "Poll Device",
        menu: "",
        shortcut: "",
        doc: "Take in what the device sent by itself: tuner readings, tempo beats, and changes \
              made on its front panel (which re-read the preset). Sends nothing unless a re-read is needed.",
        params: &[],
        enabled: connected,
        run: poll,
    },
    CommandSpec {
        id: "tuner.on",
        label: "Tuner On",
        menu: "",
        shortcut: "",
        doc: "Switch the device's tuner on with MIDI controller 15 on the device's channel. \
              Readings then arrive through device.poll.",
        params: &[],
        enabled: connected,
        run: tuner_on,
    },
    CommandSpec {
        id: "tuner.off",
        label: "Tuner Off",
        menu: "",
        shortcut: "",
        doc: "Switch the device's tuner off.",
        params: &[],
        enabled: connected,
        run: tuner_off,
    },
    CommandSpec {
        id: "tuner.toggle",
        label: "Tuner",
        menu: "Device/Tuner",
        shortcut: "T",
        doc: "Switch the device's tuner on or off.",
        params: &[],
        enabled: connected,
        run: tuner_toggle,
    },
    CommandSpec {
        id: "looper.watch",
        label: "Watch Looper",
        menu: "",
        shortcut: "",
        doc: "Ask the device to report what the looper is doing, so its buttons can show their state. \
              Selecting the Looper block does this by itself.",
        params: &[opt("on", "boolean", "false stops the reports; default true")],
        enabled: connected,
        run: looper_watch,
    },
    CommandSpec {
        id: "looper.press",
        label: "Looper Button",
        menu: "",
        shortcut: "",
        doc: "Press one of the looper's transport buttons. They are sent as the MIDI controllers \
              the device assigns to the looper by default (record 28, play 29, once 30, stack 31, \
              reverse 32, half 120, undo 121).",
        params: &[
            req("button", "string", "record, play, once, stack, reverse, half or undo"),
            opt("on", "boolean", "State to switch to; by default the button toggles"),
        ],
        enabled: connected,
        run: looper_press,
    },
    CommandSpec {
        id: "tempo.get",
        label: "Read Tempo",
        menu: "",
        shortcut: "",
        doc: "Read the tempo of the current preset in beats per minute.",
        params: &[],
        enabled: connected,
        run: tempo_get,
    },
    CommandSpec {
        id: "tempo.set",
        label: "Set Tempo",
        menu: "",
        shortcut: "",
        doc: "Set the tempo in the edit buffer.",
        params: &[req("bpm", "integer", "Beats per minute, 30 to 250")],
        enabled: connected,
        run: tempo_set,
    },
    CommandSpec {
        id: "tempo.tap",
        label: "Tap Tempo",
        menu: "Device/Tap Tempo",
        shortcut: "Space",
        doc: "Tap twice within two seconds: the time between the taps becomes the tempo.",
        params: &[],
        enabled: connected,
        run: tempo_tap,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taps_become_beats_per_minute() {
        assert_eq!(tapped_bpm(Duration::from_millis(500)), Some(120));
        assert_eq!(tapped_bpm(Duration::from_millis(1000)), Some(60));
        assert_eq!(tapped_bpm(Duration::from_secs(2)), None);
        assert_eq!(tapped_bpm(Duration::ZERO), None);
    }
}
