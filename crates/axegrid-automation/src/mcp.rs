//! Model Context Protocol server over stdio (JSON-RPC 2.0, one message per
//! line). The tools are thin wrappers over the control methods, so an agent
//! reaches exactly the command registry and nothing else.

use std::io::{self, BufRead, Write};

use serde_json::{json, Value};

use crate::{Backend, RpcError};

const PROTOCOL_VERSION: &str = "2025-06-18";
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const PARSE_ERROR: i64 = -32700;

const STATE_URI: &str = "axegrid://state";
const COMMANDS_URI: &str = "axegrid://commands";

/// `(tool name, control method, description, input schema)`.
fn tools() -> [(&'static str, &'static str, &'static str, Value); 4] {
    let step = json!({
        "type": "object",
        "properties": {
            "id": { "type": "string", "description": "Command id, e.g. \"preset.next\"" },
            "params": { "type": "object", "description": "Command parameters" }
        },
        "required": ["id"]
    });
    [
        (
            "command_list",
            "command.list",
            "List every editor command with its parameters, documentation and whether it can run now. Call this first.",
            json!({ "type": "object", "properties": {} }),
        ),
        (
            "command_run",
            "command.run",
            "Run one editor command by id. Everything the UI can do is a command.",
            step.clone(),
        ),
        (
            "command_batch",
            "command.batch",
            "Run up to 256 commands in order. Stops at the first failure unless stop_on_error is false.",
            json!({
                "type": "object",
                "properties": {
                    "steps": { "type": "array", "items": step },
                    "stop_on_error": { "type": "boolean" }
                },
                "required": ["steps"]
            }),
        ),
        (
            "state_inspect",
            "state.inspect",
            "Read the editor state: connection, preset, grid, block states and the selected block's parameters.",
            json!({ "type": "object", "properties": {} }),
        ),
    ]
}

fn text_content(value: &Value) -> Value {
    json!([{ "type": "text", "text": serde_json::to_string_pretty(value).unwrap_or_default() }])
}

fn resource(backend: &mut dyn Backend, uri: &str) -> Result<Value, (i64, String)> {
    let method = match uri {
        STATE_URI => "state.inspect",
        COMMANDS_URI => "command.list",
        other => return Err((INVALID_PARAMS, format!("unknown resource {other:?}"))),
    };
    let body = backend.call(method, &Value::Null).map_err(|e| (INVALID_PARAMS, e.to_string()))?;
    Ok(json!({ "contents": [{ "uri": uri, "mimeType": "application/json", "text": body.to_string() }] }))
}

fn handle_method(backend: &mut dyn Backend, method: &str, params: &Value) -> Result<Value, (i64, String)> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL_VERSION),
            "capabilities": { "tools": {}, "resources": {} },
            "serverInfo": { "name": "axegrid", "version": env!("CARGO_PKG_VERSION") },
            "instructions": "Editor for Axe-Fx II guitar processors. Call command_list to discover commands, \
                             command_run to act, state_inspect to observe. device.connect_simulator needs no hardware.",
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({
            "tools": tools().iter().map(|(name, _, description, schema)| json!({
                "name": name, "description": description, "inputSchema": schema,
            })).collect::<Vec<_>>()
        })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            let all = tools();
            let (_, control_method, ..) = all
                .iter()
                .find(|tool| tool.0 == name)
                .ok_or_else(|| (INVALID_PARAMS, format!("unknown tool {name:?}")))?;
            let arguments = params.get("arguments").unwrap_or(&Value::Null);
            // A failed command is a tool result the model can read and react
            // to, not a protocol error.
            Ok(match backend.call(control_method, arguments) {
                Ok(result) => json!({ "content": text_content(&result), "isError": false }),
                Err(RpcError { kind, message }) => json!({
                    "content": text_content(&json!({ "kind": kind, "message": message })),
                    "isError": true,
                }),
            })
        }
        "resources/list" => Ok(json!({ "resources": [
            { "uri": STATE_URI, "name": "Editor state", "mimeType": "application/json" },
            { "uri": COMMANDS_URI, "name": "Command registry", "mimeType": "application/json" },
        ]})),
        "resources/read" => resource(backend, params.get("uri").and_then(Value::as_str).unwrap_or_default()),
        other => Err((METHOD_NOT_FOUND, format!("method {other:?} is not supported"))),
    }
}

/// Handles one JSON-RPC message. Notifications produce no reply.
pub fn handle_message(backend: &mut dyn Backend, message: &Value) -> Option<Value> {
    let id = message.get("id").filter(|id| !id.is_null())?.clone();
    let method = message.get("method").and_then(Value::as_str).unwrap_or_default();
    let params = message.get("params").unwrap_or(&Value::Null);
    Some(match handle_method(backend, method, params) {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, text)) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": text } }),
    })
}

/// Serves MCP until the input ends.
pub fn serve(backend: &mut dyn Backend, input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(message) => handle_message(backend, &message),
            Err(e) => Some(json!({
                "jsonrpc": "2.0", "id": null,
                "error": { "code": PARSE_ERROR, "message": e.to_string() },
            })),
        };
        if let Some(reply) = reply {
            writeln!(output, "{reply}")?;
            output.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::Headless;

    fn call(backend: &mut Headless, method: &str, params: Value) -> Value {
        let message = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
        handle_message(backend, &message).unwrap()
    }

    fn tool(backend: &mut Headless, name: &str, arguments: Value) -> (bool, Value) {
        let reply = call(backend, "tools/call", json!({ "name": name, "arguments": arguments }));
        let result = &reply["result"];
        let text = result["content"][0]["text"].as_str().unwrap();
        (result["isError"].as_bool().unwrap(), serde_json::from_str(text).unwrap())
    }

    #[test]
    fn handshake_and_discovery() {
        let mut backend = Headless::default();
        let init = call(&mut backend, "initialize", json!({ "protocolVersion": "2025-03-26" }));
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(init["result"]["serverInfo"]["name"], "axegrid");
        let listed = call(&mut backend, "tools/list", Value::Null);
        let names: Vec<&str> =
            listed["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["command_list", "command_run", "command_batch", "state_inspect"]);
        // Notifications get no answer.
        let note = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        assert!(handle_message(&mut backend, &note).is_none());
    }

    #[test]
    fn an_agent_can_edit_a_preset() {
        let mut backend = Headless::default();
        assert!(!tool(&mut backend, "command_run", json!({ "id": "device.connect_simulator" })).0);
        let steps = json!({ "steps": [
            { "id": "block.select", "params": { "block": "Amp 1" } },
            { "id": "param.set_normalized", "params": { "param": "DISTORT_DRIVE", "position": 1.0 } },
        ]});
        let (failed, batch) = tool(&mut backend, "command_batch", steps);
        assert!(!failed);
        assert_eq!(batch["results"][1]["result"]["text"], "10.00");
        let (_, state) = tool(&mut backend, "state_inspect", json!({}));
        assert_eq!(state["selected_block"], 106);
        assert_eq!(state["params"][1]["value"], 65534);
    }

    #[test]
    fn command_failures_are_tool_errors_not_protocol_errors() {
        let mut backend = Headless::default();
        let (failed, error) = tool(&mut backend, "command_run", json!({ "id": "preset.next" }));
        assert!(failed);
        assert_eq!(error["kind"], "disabled");
        assert_eq!(call(&mut backend, "tools/call", json!({ "name": "nope" }))["error"]["code"], INVALID_PARAMS);
        assert_eq!(call(&mut backend, "bogus/method", Value::Null)["error"]["code"], METHOD_NOT_FOUND);
    }

    #[test]
    fn resources_expose_live_json() {
        let mut backend = Headless::default();
        let listed = call(&mut backend, "resources/list", Value::Null);
        assert_eq!(listed["result"]["resources"].as_array().unwrap().len(), 2);
        let read = call(&mut backend, "resources/read", json!({ "uri": STATE_URI }));
        let text = read["result"]["contents"][0]["text"].as_str().unwrap();
        assert_eq!(serde_json::from_str::<Value>(text).unwrap()["connected"], false);
        assert!(call(&mut backend, "resources/read", json!({ "uri": "file:///etc/passwd" }))["error"].is_object());
    }

    #[test]
    fn serve_speaks_line_delimited_json() {
        let input = b"{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}\n\nnot json\n";
        let mut output = Vec::new();
        serve(&mut Headless::default(), &input[..], &mut output).unwrap();
        let lines: Vec<Value> =
            String::from_utf8(output).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines[0], json!({ "jsonrpc": "2.0", "id": 7, "result": {} }));
        assert_eq!(lines[1]["error"]["code"], PARSE_ERROR);
    }
}
