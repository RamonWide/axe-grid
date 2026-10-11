//! Automation front ends over the command registry: the JSON control
//! methods, the loopback control channel to a running app, and the MCP
//! server. None of them can do anything a command cannot.

pub mod control;
pub mod mcp;

use axegrid_engine::Engine;
use serde_json::{json, Value};

/// Most steps one `command.batch` may carry.
pub const MAX_BATCH: usize = 256;

#[derive(Debug, Clone, PartialEq)]
pub struct RpcError {
    /// Stable category: a command error kind, or `bad_request` / `transport`.
    pub kind: String,
    pub message: String,
}

impl RpcError {
    pub fn new(kind: &str, message: impl Into<String>) -> Self {
        Self { kind: kind.to_owned(), message: message.into() }
    }

    pub fn to_json(&self) -> Value {
        json!({ "kind": self.kind, "message": self.message })
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl std::error::Error for RpcError {}

/// Something that can execute control methods: an in-process engine, or a
/// connection to a running desktop app.
pub trait Backend {
    fn call(&mut self, method: &str, params: &Value) -> Result<Value, RpcError>;
}

/// Runs the engine in this process, with no window.
#[derive(Default)]
pub struct Headless {
    pub engine: Engine,
}

impl Backend for Headless {
    fn call(&mut self, method: &str, params: &Value) -> Result<Value, RpcError> {
        dispatch(&mut self.engine, method, params)
    }
}

fn run_one(engine: &mut Engine, step: &Value) -> Result<Value, RpcError> {
    let id = step
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::new("bad_request", "a command needs a string \"id\""))?;
    let params = step.get("params").unwrap_or(&Value::Null);
    engine.run(id, params).map_err(|e| RpcError::new(e.kind(), e.to_string()))
}

/// The control methods. This is the single entry point shared by the control
/// channel and MCP.
///
/// | method | params | result |
/// |---|---|---|
/// | `command.list` | — | every command with docs and enabled state |
/// | `command.run` | `{id, params?}` | the command's result |
/// | `command.batch` | `{steps: [{id, params?}], stop_on_error?}` | one entry per step |
/// | `state.inspect` | — | editor state |
pub fn dispatch(engine: &mut Engine, method: &str, params: &Value) -> Result<Value, RpcError> {
    match method {
        "command.list" => Ok(axegrid_engine::command_list(engine)),
        "command.run" => run_one(engine, params),
        "command.batch" => {
            let steps = params
                .get("steps")
                .and_then(Value::as_array)
                .ok_or_else(|| RpcError::new("bad_request", "\"steps\" must be an array"))?;
            if steps.len() > MAX_BATCH {
                return Err(RpcError::new("bad_request", format!("a batch is limited to {MAX_BATCH} steps")));
            }
            let stop = params.get("stop_on_error").and_then(Value::as_bool).unwrap_or(true);
            let mut results = Vec::with_capacity(steps.len());
            for step in steps {
                let outcome = run_one(engine, step);
                let failed = outcome.is_err();
                results.push(match outcome {
                    Ok(result) => json!({ "ok": true, "result": result }),
                    Err(error) => json!({ "ok": false, "error": error.to_json() }),
                });
                if failed && stop {
                    break;
                }
            }
            Ok(json!({ "completed": results.len(), "of": steps.len(), "results": results }))
        }
        "state.inspect" => Ok(engine.inspect()),
        other => Err(RpcError::new("bad_request", format!("unknown method {other:?}"))),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn run_and_inspect() {
        let mut engine = Engine::new();
        let run = json!({ "id": "device.connect_simulator" });
        assert_eq!(dispatch(&mut engine, "command.run", &run).unwrap()["connected"], true);
        assert_eq!(dispatch(&mut engine, "state.inspect", &Value::Null).unwrap()["preset"]["number"], 0);
        let listed = dispatch(&mut engine, "command.list", &Value::Null).unwrap();
        assert!(listed.as_array().unwrap().iter().any(|c| c["id"] == "param.set"));
    }

    #[test]
    fn batch_stops_at_the_first_failure_by_default() {
        let mut engine = Engine::new();
        let steps = json!({ "steps": [
            { "id": "device.connect_simulator" },
            { "id": "preset.select", "params": { "number": 99 } },
            { "id": "preset.next" },
        ]});
        let out = dispatch(&mut engine, "command.batch", &steps).unwrap();
        assert_eq!((out["completed"].as_u64(), out["of"].as_u64()), (Some(2), Some(3)));
        assert_eq!(out["results"][1]["error"]["kind"], "device");

        let mut keep_going = steps.clone();
        keep_going["stop_on_error"] = json!(false);
        keep_going["steps"][0] = json!({ "id": "device.refresh" });
        let out = dispatch(&mut engine, "command.batch", &keep_going).unwrap();
        assert_eq!(out["completed"], 3);
        assert_eq!(out["results"][2]["result"]["number"], 1);
    }

    #[test]
    fn malformed_requests_are_reported() {
        let mut engine = Engine::new();
        for (method, params) in [
            ("nope", json!({})),
            ("command.run", json!({})),
            ("command.run", json!({ "id": 5 })),
            ("command.batch", json!({ "steps": "x" })),
            ("command.batch", json!({ "steps": vec![json!({ "id": "app.inspect" }); MAX_BATCH + 1] })),
        ] {
            assert_eq!(dispatch(&mut engine, method, &params).unwrap_err().kind, "bad_request", "{method}");
        }
        let unknown = json!({ "id": "no.such" });
        assert_eq!(dispatch(&mut engine, "command.run", &unknown).unwrap_err().kind, "unknown_command");
    }
}
