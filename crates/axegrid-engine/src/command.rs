//! What a command is, and the helpers command bodies share.

use std::fmt;

use axegrid_device::SessionError;
use serde_json::Value;

use crate::Engine;

/// One documented parameter of a command.
#[derive(Debug, Clone, Copy)]
pub struct ParamDoc {
    pub name: &'static str,
    /// JSON type: `"integer"`, `"number"`, `"string"` or `"boolean"`.
    pub kind: &'static str,
    pub required: bool,
    pub doc: &'static str,
}

pub const fn req(name: &'static str, kind: &'static str, doc: &'static str) -> ParamDoc {
    ParamDoc { name, kind, required: true, doc }
}

pub const fn opt(name: &'static str, kind: &'static str, doc: &'static str) -> ParamDoc {
    ParamDoc { name, kind, required: false, doc }
}

/// `Err` carries the reason the command cannot run right now.
pub type Enabled = fn(&Engine) -> Result<(), &'static str>;
pub type Run = fn(&mut Engine, &Args<'_>) -> Result<Value, CommandError>;

pub struct CommandSpec {
    /// Stable dotted identifier, e.g. `preset.select`.
    pub id: &'static str,
    pub label: &'static str,
    /// Menu path such as `Preset/Next`; empty for commands without a menu item.
    pub menu: &'static str,
    pub shortcut: &'static str,
    pub doc: &'static str,
    pub params: &'static [ParamDoc],
    pub enabled: Enabled,
    pub run: Run,
}

pub fn always(_: &Engine) -> Result<(), &'static str> {
    Ok(())
}

pub fn connected(engine: &Engine) -> Result<(), &'static str> {
    if engine.is_connected() {
        Ok(())
    } else {
        Err("no device connected")
    }
}

pub fn disconnected(engine: &Engine) -> Result<(), &'static str> {
    if engine.is_connected() {
        Err("a device is already connected")
    } else {
        Ok(())
    }
}

#[derive(Debug)]
pub enum CommandError {
    UnknownCommand(String),
    Disabled { id: &'static str, reason: &'static str },
    BadParam { name: &'static str, problem: String },
    Device(SessionError),
    Io(String),
    NotFound(String),
}

impl CommandError {
    /// Short machine-readable category, stable across releases.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::UnknownCommand(_) => "unknown_command",
            Self::Disabled { .. } => "disabled",
            Self::BadParam { .. } => "bad_param",
            Self::Device(_) => "device",
            Self::Io(_) => "io",
            Self::NotFound(_) => "not_found",
        }
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownCommand(id) => write!(f, "unknown command {id:?}"),
            Self::Disabled { id, reason } => write!(f, "{id} is unavailable: {reason}"),
            Self::BadParam { name, problem } => write!(f, "parameter {name:?}: {problem}"),
            Self::Device(e) => e.fmt(f),
            Self::Io(message) | Self::NotFound(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for CommandError {}

impl From<SessionError> for CommandError {
    fn from(e: SessionError) -> Self {
        Self::Device(e)
    }
}

/// Typed access to a command's JSON parameters. Parameters are untrusted:
/// every getter range-checks and reports the offending name.
pub struct Args<'a>(pub &'a Value);

impl Args<'_> {
    fn bad(name: &'static str, problem: impl Into<String>) -> CommandError {
        CommandError::BadParam { name, problem: problem.into() }
    }

    pub fn has(&self, name: &str) -> bool {
        self.0.get(name).is_some_and(|v| !v.is_null())
    }

    pub fn opt_int(&self, name: &'static str, max: u64) -> Result<Option<u64>, CommandError> {
        match self.0.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => match value.as_u64() {
                Some(n) if n <= max => Ok(Some(n)),
                Some(n) => Err(Self::bad(name, format!("{n} is above the maximum {max}"))),
                None => Err(Self::bad(name, "expected a non-negative integer")),
            },
        }
    }

    pub fn int(&self, name: &'static str, max: u64) -> Result<u64, CommandError> {
        self.opt_int(name, max)?.ok_or_else(|| Self::bad(name, "is required"))
    }

    pub fn opt_str(&self, name: &'static str) -> Result<Option<&str>, CommandError> {
        match self.0.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => value.as_str().map(Some).ok_or_else(|| Self::bad(name, "expected a string")),
        }
    }

    pub fn opt_bool(&self, name: &'static str) -> Result<Option<bool>, CommandError> {
        match self.0.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => value.as_bool().map(Some).ok_or_else(|| Self::bad(name, "expected true or false")),
        }
    }

    pub fn str(&self, name: &'static str) -> Result<&str, CommandError> {
        self.opt_str(name)?.ok_or_else(|| Self::bad(name, "is required"))
    }

    /// A finite number within `min..=max`.
    pub fn number(&self, name: &'static str, min: f64, max: f64) -> Result<f64, CommandError> {
        let value = self.0.get(name).and_then(Value::as_f64);
        match value {
            Some(n) if n.is_finite() && (min..=max).contains(&n) => Ok(n),
            Some(n) => Err(Self::bad(name, format!("{n} is outside {min}..={max}"))),
            None => Err(Self::bad(name, "expected a number")),
        }
    }

    pub fn raw(&self, name: &str) -> Option<&Value> {
        self.0.get(name).filter(|v| !v.is_null())
    }
}
