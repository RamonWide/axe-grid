//! The loopback control channel: newline-delimited JSON over TCP.
//!
//! ```text
//! -> {"id":"auth","method":"auth","params":{"token":"<64 hex>"}}
//! <- {"id":"auth","result":true}
//! -> {"id":1,"method":"command.run","params":{"id":"preset.next"}}
//! <- {"id":1,"result":{...}}        or   {"id":1,"error":{"kind":..,"message":..}}
//! ```
//!
//! The server only listens on 127.0.0.1 and every connection must
//! authenticate first. Requests are handed to the owner of the engine through
//! a channel, so the desktop app runs them in turn with the user's clicks.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::{Backend, RpcError};

/// Longest accepted request or reply line.
pub const MAX_LINE: u64 = 1024 * 1024;
pub const MAX_CONNECTIONS: usize = 16;
const IO_TIMEOUT: Duration = Duration::from_secs(30);
const TOKEN_BYTES: usize = 32;

/// A fresh 256-bit bearer token as 64 hex digits.
pub fn new_token() -> io::Result<String> {
    let mut bytes = [0u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(|e| io::Error::other(e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Compares without an early exit so timing does not reveal a prefix match.
fn token_matches(expected: &str, offered: &str) -> bool {
    expected.len() == offered.len() && expected.bytes().zip(offered.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

/// One authenticated request waiting for the engine owner.
pub struct Pending {
    pub method: String,
    pub params: Value,
    reply: Sender<Result<Value, RpcError>>,
}

impl Pending {
    pub fn respond(self, outcome: Result<Value, RpcError>) {
        let _ = self.reply.send(outcome);
    }
}

fn read_line(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut line = String::new();
    let read = reader.take(MAX_LINE + 1).read_line(&mut line)?;
    if read == 0 {
        return Ok(None);
    }
    if read as u64 > MAX_LINE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "line exceeds 1 MiB"));
    }
    Ok(Some(line))
}

fn write_json(stream: &mut impl Write, value: &Value) -> io::Result<()> {
    let mut line = value.to_string();
    line.push('\n');
    stream.write_all(line.as_bytes())
}

fn serve_connection(
    stream: TcpStream,
    token: &str,
    requests: &Sender<Pending>,
    wake: &(dyn Fn() + Send + Sync),
) -> io::Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    let mut authenticated = false;
    while let Some(line) = read_line(&mut reader)? {
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(e) => {
                let error = RpcError::new("bad_request", format!("invalid JSON: {e}"));
                write_json(&mut writer, &json!({ "id": null, "error": error.to_json() }))?;
                continue;
            }
        };
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(Value::as_str).unwrap_or_default();
        let params = request.get("params").cloned().unwrap_or(Value::Null);
        if !authenticated {
            let offered = params.get("token").and_then(Value::as_str).unwrap_or_default();
            if method == "auth" && token_matches(token, offered) {
                authenticated = true;
                write_json(&mut writer, &json!({ "id": id, "result": true }))?;
                continue;
            }
            let error = RpcError::new("unauthorized", "the first request must be auth with a valid token");
            write_json(&mut writer, &json!({ "id": id, "error": error.to_json() }))?;
            return Ok(());
        }
        let (reply, answer) = mpsc::channel();
        let pending = Pending { method: method.to_owned(), params, reply };
        let outcome = match requests.send(pending) {
            Ok(()) => {
                wake();
                answer
                    .recv_timeout(IO_TIMEOUT)
                    .unwrap_or_else(|_| Err(RpcError::new("transport", "the application did not answer in time")))
            }
            Err(_) => Err(RpcError::new("transport", "the application is shutting down")),
        };
        write_json(
            &mut writer,
            &match outcome {
                Ok(result) => json!({ "id": id, "result": result }),
                Err(error) => json!({ "id": id, "error": error.to_json() }),
            },
        )?;
    }
    Ok(())
}

/// Starts the control server on `127.0.0.1:port` (0 picks a free port).
/// Returns the bound address and the stream of authenticated requests.
/// `wake` is called after each request is queued, e.g. to repaint the UI.
pub fn start_server(
    port: u16,
    token: String,
    wake: impl Fn() + Send + Sync + 'static,
) -> io::Result<(SocketAddr, Receiver<Pending>)> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
    let address = listener.local_addr()?;
    let (requests, receiver) = mpsc::channel();
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
    let token = Arc::new(token);
    let active = Arc::new(AtomicUsize::new(0));
    std::thread::Builder::new().name("control-accept".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            if active.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
                continue;
            }
            active.fetch_add(1, Ordering::SeqCst);
            let (requests, wake, token, slot) = (requests.clone(), wake.clone(), token.clone(), active.clone());
            let spawned = std::thread::Builder::new().name("control-conn".into()).spawn(move || {
                let _ = serve_connection(stream, &token, &requests, wake.as_ref());
                slot.fetch_sub(1, Ordering::SeqCst);
            });
            if spawned.is_err() {
                // The closure never ran, so undo its slot here.
                active.fetch_sub(1, Ordering::SeqCst);
            }
        }
    })?;
    Ok((address, receiver))
}

/// Client side: drives a running desktop app.
pub struct BridgeClient {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    next_id: u64,
}

impl BridgeClient {
    pub fn connect(address: SocketAddr, token: &str) -> Result<Self, RpcError> {
        let transport = |e: io::Error| RpcError::new("transport", e.to_string());
        if !address.ip().is_loopback() {
            return Err(RpcError::new("transport", "the control channel is loopback-only"));
        }
        let stream = TcpStream::connect_timeout(&address, Duration::from_secs(5)).map_err(transport)?;
        stream.set_read_timeout(Some(IO_TIMEOUT)).map_err(transport)?;
        let writer = stream.try_clone().map_err(transport)?;
        let mut client = Self { reader: BufReader::new(stream), writer, next_id: 0 };
        client.call("auth", &json!({ "token": token }))?;
        Ok(client)
    }
}

impl Backend for BridgeClient {
    fn call(&mut self, method: &str, params: &Value) -> Result<Value, RpcError> {
        let transport = |e: io::Error| RpcError::new("transport", e.to_string());
        self.next_id += 1;
        let request = json!({ "id": self.next_id, "method": method, "params": params });
        write_json(&mut self.writer, &request).map_err(transport)?;
        let line = read_line(&mut self.reader)
            .map_err(transport)?
            .ok_or_else(|| RpcError::new("transport", "the application closed the connection"))?;
        let mut reply: Value = serde_json::from_str(&line).map_err(|e| RpcError::new("transport", e.to_string()))?;
        if let Some(error) = reply.get("error") {
            let field = |name: &str| error.get(name).and_then(Value::as_str).unwrap_or("unknown").to_owned();
            return Err(RpcError { kind: field("kind"), message: field("message") });
        }
        Ok(reply.get_mut("result").map(Value::take).unwrap_or(Value::Null))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use axegrid_engine::Engine;

    /// A server whose requests are executed by an engine on another thread,
    /// the way the desktop app does on its engine thread.
    fn serve(token: &str) -> SocketAddr {
        let (address, requests) = start_server(0, token.to_owned(), || {}).unwrap();
        std::thread::spawn(move || {
            let mut engine = Engine::new();
            for pending in requests {
                let outcome = crate::dispatch(&mut engine, &pending.method, &pending.params);
                pending.respond(outcome);
            }
        });
        address
    }

    #[test]
    fn tokens_are_unique_hex() {
        let (a, b) = (new_token().unwrap(), new_token().unwrap());
        assert_eq!(a.len(), 64);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
        assert!(token_matches(&a, &a) && !token_matches(&a, &b) && !token_matches(&a, &a[..63]));
    }

    #[test]
    fn bridge_drives_the_engine() {
        let token = new_token().unwrap();
        let mut client = BridgeClient::connect(serve(&token), &token).unwrap();
        let connected = client.call("command.run", &json!({ "id": "device.connect_simulator" })).unwrap();
        assert_eq!(connected["model"], "Axe-Fx II XL");
        let next = client.call("command.run", &json!({ "id": "preset.next" })).unwrap();
        assert_eq!(next["name"], "Crunch");
        let err = client.call("command.run", &json!({ "id": "scene.select", "params": { "scene": 99 } }));
        assert_eq!(err.unwrap_err().kind, "bad_param");
    }

    #[test]
    fn a_wrong_token_gets_nothing() {
        let address = serve(&new_token().unwrap());
        let err = BridgeClient::connect(address, &"0".repeat(64)).err().unwrap();
        assert_eq!(err.kind, "unauthorized");

        // Skipping auth is refused too, and the connection is closed.
        let mut stream = TcpStream::connect(address).unwrap();
        write_json(&mut stream, &json!({ "id": 1, "method": "state.inspect" })).unwrap();
        let mut reader = BufReader::new(stream);
        let reply: Value = serde_json::from_str(&read_line(&mut reader).unwrap().unwrap()).unwrap();
        assert_eq!(reply["error"]["kind"], "unauthorized");
        assert!(read_line(&mut reader).unwrap().is_none());
    }

    #[test]
    fn refuses_non_loopback_targets() {
        let address: SocketAddr = "192.0.2.1:9".parse().unwrap();
        assert_eq!(BridgeClient::connect(address, "x").err().unwrap().kind, "transport");
    }
}
