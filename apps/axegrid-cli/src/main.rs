//! Headless front end: run commands, serve the JSON control methods on
//! stdio, or serve MCP.

use std::io::{self, BufRead, Write};
use std::net::SocketAddr;
use std::process::ExitCode;

use axegrid_automation::control::BridgeClient;
use axegrid_automation::{mcp, Backend, Headless, RpcError};
use serde_json::{json, Value};

const USAGE: &str = "\
axegrid-cli: drive the Axe-Grid editor without a window

USAGE
  axegrid-cli commands                      list every command as JSON
  axegrid-cli run <id> [json-params]        run one command and print its result
  axegrid-cli batch <file.json>             run {\"steps\":[{\"id\":..,\"params\":..}]}
  axegrid-cli serve                         JSON lines on stdio: {\"id\",\"method\",\"params\"}
  axegrid-cli mcp                           Model Context Protocol server on stdio

OPTIONS (any subcommand)
  --simulator                  connect to the built-in simulated device first
  --bridge <127.0.0.1:port>    act on a running desktop app instead of in-process
  --control-token-file <path>  token file written by `axegrid --control`

Exit status is 0 on success, 1 when a command fails, 2 on a usage error.";

struct Options {
    words: Vec<String>,
    simulator: bool,
    bridge: Option<String>,
    token_file: Option<String>,
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut options = Options { words: Vec::new(), simulator: false, bridge: None, token_file: None };
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--simulator" => options.simulator = true,
            "--bridge" => options.bridge = Some(value("--bridge")?),
            "--control-token-file" => options.token_file = Some(value("--control-token-file")?),
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            _ => options.words.push(arg),
        }
    }
    Ok(options)
}

fn backend(options: &Options) -> Result<Box<dyn Backend>, String> {
    let mut backend: Box<dyn Backend> = match &options.bridge {
        Some(address) => {
            let address: SocketAddr = address.parse().map_err(|_| format!("{address:?} is not host:port"))?;
            let path = options.token_file.as_ref().ok_or("--bridge needs --control-token-file")?;
            let token = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            Box::new(BridgeClient::connect(address, token.trim()).map_err(|e| e.to_string())?)
        }
        None => Box::new(Headless::default()),
    };
    if options.simulator {
        let connect = json!({ "id": "device.connect_simulator" });
        backend.call("command.run", &connect).map_err(|e| e.to_string())?;
    }
    Ok(backend)
}

fn print(value: &Value) {
    println!("{}", serde_json::to_string_pretty(value).unwrap_or_default());
}

fn report(outcome: Result<Value, RpcError>) -> ExitCode {
    match outcome {
        Ok(value) => {
            print(&value);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}

/// One request per line in, one reply per line out.
fn serve(backend: &mut dyn Backend) -> io::Result<()> {
    let mut out = io::stdout().lock();
    for line in io::stdin().lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(request) => {
                let id = request.get("id").cloned().unwrap_or(Value::Null);
                let method = request.get("method").and_then(Value::as_str).unwrap_or_default();
                match backend.call(method, request.get("params").unwrap_or(&Value::Null)) {
                    Ok(result) => json!({ "id": id, "result": result }),
                    Err(error) => json!({ "id": id, "error": error.to_json() }),
                }
            }
            Err(e) => json!({ "id": null, "error": RpcError::new("bad_request", e.to_string()).to_json() }),
        };
        writeln!(out, "{reply}")?;
        out.flush()?;
    }
    Ok(())
}

fn run(options: &Options) -> Result<ExitCode, String> {
    let words: Vec<&str> = options.words.iter().map(String::as_str).collect();
    let io_error = |e: io::Error| e.to_string();
    match words.as_slice() {
        ["commands"] => Ok(report(backend(options)?.call("command.list", &Value::Null))),
        ["run", id, rest @ ..] if rest.len() <= 1 => {
            let params = match rest.first() {
                Some(text) => serde_json::from_str(text).map_err(|e| format!("parameters are not JSON: {e}"))?,
                None => Value::Null,
            };
            Ok(report(backend(options)?.call("command.run", &json!({ "id": id, "params": params }))))
        }
        ["batch", path] => {
            let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            let steps: Value = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
            let outcome = backend(options)?.call("command.batch", &steps);
            let incomplete = outcome
                .as_ref()
                .is_ok_and(|o| o["results"].as_array().is_some_and(|r| r.iter().any(|s| s["ok"] == false)));
            let code = report(outcome);
            Ok(if incomplete { ExitCode::from(1) } else { code })
        }
        ["serve"] => serve(backend(options)?.as_mut()).map(|()| ExitCode::SUCCESS).map_err(io_error),
        ["mcp"] => mcp::serve(backend(options)?.as_mut(), io::stdin().lock(), io::stdout().lock())
            .map(|()| ExitCode::SUCCESS)
            .map_err(io_error),
        _ => Err("expected a subcommand".into()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    match parse_args(args.into_iter()).and_then(|options| run(&options)) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("error: {message}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}
