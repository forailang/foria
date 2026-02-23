use crate::ast::Flow;
use crate::codec::CodecRegistry;
use crate::ir::Ir;
use crate::loader::FlowRegistry;
use crate::runtime::{self, RunReport, StateSnapshot, StepCommand, StepHandle};
use crate::types::TypeRegistry;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tungstenite::{Message, WebSocket, accept};

const DEBUG_UI_HTML: &str = include_str!("debug_ui.html");

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ClientMessage {
    #[serde(rename = "step")]
    Step,
    #[serde(rename = "continue")]
    Continue,
    #[serde(rename = "run_to_breakpoint")]
    RunToBreakpoint,
    #[serde(rename = "set_breakpoints")]
    SetBreakpoints { node_ids: Vec<String> },
    #[serde(rename = "restart")]
    Restart,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum ServerMessage {
    #[serde(rename = "init")]
    Init {
        flow_name: String,
        source: String,
        ir: Ir,
        inputs: Value,
        docs: HashMap<String, String>,
    },
    #[serde(rename = "snapshot")]
    Snapshot(StateSnapshot),
    #[serde(rename = "completed")]
    Completed { report: RunReport },
    #[serde(rename = "error")]
    Error { message: String },
}

/// Holds a live execution — the channel pair and the thread join handle.
struct RunningExec {
    handle: StepHandle,
    join: std::thread::JoinHandle<Result<RunReport, String>>,
}

struct Session {
    flow: Flow,
    ir: Ir,
    inputs: HashMap<String, Value>,
    registry: TypeRegistry,
    flow_registry: FlowRegistry,
    source: String,
    docs: HashMap<String, String>,
    /// Singleton execution. At most one execution runs at a time.
    /// A new WS connection takes ownership; if none exists, a fresh one is created.
    active_exec: Mutex<Option<RunningExec>>,
}

fn send_msg(ws: &mut WebSocket<TcpStream>, msg: &ServerMessage) -> Result<(), String> {
    let json = serde_json::to_string(msg).map_err(|e| format!("serialize error: {e}"))?;
    ws.send(Message::Text(json))
        .map_err(|e| format!("ws send error: {e}"))
}

fn new_execution(session: &Session) -> Result<RunningExec, String> {
    let (handle, join) = runtime::execute_flow_stepping(
        &session.flow,
        session.ir.clone(),
        session.inputs.clone(),
        &session.registry,
        Some(&session.flow_registry),
        CodecRegistry::default_registry(),
    )?;
    Ok(RunningExec { handle, join })
}

/// Take the singleton execution from the session, or create a fresh one.
/// Any previously active execution is killed (its channels dropped).
fn take_or_create_exec(session: &Arc<Session>) -> Result<RunningExec, String> {
    let mut guard = session.active_exec.lock().unwrap();
    match guard.take() {
        Some(exec) => Ok(exec),
        None => new_execution(session),
    }
}

/// Put a still-running execution back into the session so the next WS
/// connection can pick it up.
fn return_exec(session: &Arc<Session>, exec: RunningExec) {
    let mut guard = session.active_exec.lock().unwrap();
    *guard = Some(exec);
}

fn make_init_msg(session: &Arc<Session>) -> ServerMessage {
    ServerMessage::Init {
        flow_name: session.flow.name.clone(),
        source: session.source.clone(),
        ir: session.ir.clone(),
        inputs: {
            let mut obj = serde_json::Map::new();
            for (k, v) in &session.inputs {
                obj.insert(k.clone(), v.clone());
            }
            Value::Object(obj)
        },
        docs: session.docs.clone(),
    }
}

fn handle_websocket(stream: TcpStream, session: &Arc<Session>) {
    let mut ws = match accept(stream) {
        Ok(ws) => ws,
        Err(e) => {
            eprintln!("WebSocket handshake failed: {e}");
            return;
        }
    };

    // Take the singleton execution (or create a fresh one if none exists).
    // This ensures only ONE execution is ever active across all WS connections.
    let mut exec = match take_or_create_exec(session) {
        Ok(e) => e,
        Err(e) => {
            let _ = send_msg(&mut ws, &ServerMessage::Error { message: e });
            return;
        }
    };

    // Send init + drain any already-buffered snapshot — both while still blocking.
    if send_msg(&mut ws, &make_init_msg(session)).is_err() {
        return_exec(session, exec);
        return;
    }
    // Flush the initial snapshot (the execution thread sends it immediately on start).
    // Use a short blocking recv so we don't spin; it's almost always already waiting.
    if let Ok(snap) = exec
        .handle
        .snapshot_rx
        .recv_timeout(Duration::from_millis(200))
    {
        if send_msg(&mut ws, &ServerMessage::Snapshot(snap)).is_err() {
            return_exec(session, exec);
            return;
        }
    }

    let raw = ws.get_ref();
    let _ = raw.set_nonblocking(true);

    let mut flow_done = false;

    loop {
        // Drain any snapshots from the execution thread.
        match exec.handle.snapshot_rx.try_recv() {
            Ok(snapshot) => {
                let msg = ServerMessage::Snapshot(snapshot);
                if send_msg(&mut ws, &msg).is_err() {
                    return_exec(session, exec);
                    return;
                }
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) if !flow_done => {
                flow_done = true;
                // Execution finished — collect the result.
                // We need to consume the join handle. Take the exec out, join it,
                // then store a replacement exec for the next WS connection.
                let RunningExec { handle: _, join } = exec;
                match join.join() {
                    Ok(Ok(report)) => {
                        let _ = send_msg(&mut ws, &ServerMessage::Completed { report });
                    }
                    Ok(Err(e)) => {
                        let _ = send_msg(&mut ws, &ServerMessage::Error { message: e });
                    }
                    Err(_) => {
                        let _ = send_msg(
                            &mut ws,
                            &ServerMessage::Error {
                                message: "execution thread panicked".to_string(),
                            },
                        );
                    }
                }
                // Start a fresh execution so the next Restart or reconnect has something.
                exec = match new_execution(session) {
                    Ok(e) => e,
                    Err(e) => {
                        eprintln!("Failed to start replacement execution: {e}");
                        return;
                    }
                };
                // Consume the initial pause snapshot from the fresh execution.
                if let Ok(snap) = exec
                    .handle
                    .snapshot_rx
                    .recv_timeout(Duration::from_millis(200))
                {
                    // Store but don't send — wait for user to reconnect/restart.
                    let _ = snap;
                }
            }
            _ => {}
        }

        // Read client commands.
        match ws.read() {
            Ok(Message::Text(text)) => {
                match serde_json::from_str::<ClientMessage>(&text) {
                    Ok(ClientMessage::Step) => {
                        let _ = exec.handle.command_tx.send(StepCommand::Step);
                    }
                    Ok(ClientMessage::Continue) => {
                        let _ = exec.handle.command_tx.send(StepCommand::Continue);
                    }
                    Ok(ClientMessage::RunToBreakpoint) => {
                        let _ = exec.handle.command_tx.send(StepCommand::RunToBreakpoint);
                    }
                    Ok(ClientMessage::SetBreakpoints { node_ids }) => {
                        let bp: HashSet<String> = node_ids.into_iter().collect();
                        let _ = exec.handle.command_tx.send(StepCommand::SetBreakpoints(bp));
                    }
                    Ok(ClientMessage::Restart) => {
                        // Kill the current execution and start a fresh one.
                        drop(exec);
                        exec = match new_execution(session) {
                            Ok(e) => e,
                            Err(e) => {
                                let _ = send_msg(&mut ws, &ServerMessage::Error { message: e });
                                return;
                            }
                        };
                        flow_done = false;
                        if send_msg(&mut ws, &make_init_msg(session)).is_err() {
                            return_exec(session, exec);
                            return;
                        }
                        // Send the initial pause snapshot.
                        if let Ok(snap) = exec
                            .handle
                            .snapshot_rx
                            .recv_timeout(Duration::from_millis(200))
                        {
                            if send_msg(&mut ws, &ServerMessage::Snapshot(snap)).is_err() {
                                return_exec(session, exec);
                                return;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Bad client message: {e}");
                    }
                }
            }
            Ok(Message::Close(_)) => {
                return_exec(session, exec);
                return;
            }
            Err(tungstenite::Error::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => {
                return_exec(session, exec);
                return;
            }
            _ => {}
        }

        thread::sleep(Duration::from_millis(10));
    }
}

fn handle_http(stream: TcpStream) {
    // Read full request headers from the stream, then write response
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .to_string();

    // Drain remaining headers
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if line.trim().is_empty() {
                    break;
                }
            }
            Err(_) => break,
        }
    }

    // Get the underlying stream back for writing
    let mut stream = reader.into_inner();

    match path.as_str() {
        "/" => {
            let body = DEBUG_UI_HTML.as_bytes();
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(body);
            let _ = stream.flush();
        }
        _ => {
            let body = b"not found";
            let header = format!(
                "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(body);
            let _ = stream.flush();
        }
    }
}

fn is_websocket_upgrade(stream: &TcpStream) -> bool {
    let mut buf = [0u8; 4096];
    match stream.peek(&mut buf) {
        Ok(n) => {
            let text = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
            text.contains("upgrade: websocket")
        }
        Err(_) => false,
    }
}

pub fn serve_dev_server(
    port: u16,
    flow: Flow,
    ir: Ir,
    inputs: HashMap<String, Value>,
    registry: TypeRegistry,
    flow_registry: FlowRegistry,
    source: String,
    docs: HashMap<String, String>,
) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| format!("Failed to bind on 127.0.0.1:{port}: {e}"))?;

    let url = format!("http://127.0.0.1:{port}");
    eprintln!("Debug UI: {url}");
    eprintln!("Press Ctrl+C to stop.");

    // Best-effort open browser
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(&url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
    }

    let session = Arc::new(Session {
        flow,
        ir,
        inputs,
        registry,
        flow_registry,
        source,
        docs,
        active_exec: Mutex::new(None),
    });

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let s = Arc::clone(&session);
        thread::spawn(move || {
            // Set a read timeout so peek doesn't block forever on idle connections
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            if is_websocket_upgrade(&stream) {
                let _ = stream.set_read_timeout(None);
                handle_websocket(stream, &s);
            } else {
                handle_http(stream);
            }
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_message_deserializes() {
        let step: ClientMessage = serde_json::from_str(r#"{"type":"step"}"#).unwrap();
        assert!(matches!(step, ClientMessage::Step));

        let cont: ClientMessage = serde_json::from_str(r#"{"type":"continue"}"#).unwrap();
        assert!(matches!(cont, ClientMessage::Continue));

        let rtb: ClientMessage = serde_json::from_str(r#"{"type":"run_to_breakpoint"}"#).unwrap();
        assert!(matches!(rtb, ClientMessage::RunToBreakpoint));

        let bp: ClientMessage =
            serde_json::from_str(r#"{"type":"set_breakpoints","node_ids":["n1_foo","n2_bar"]}"#)
                .unwrap();
        match bp {
            ClientMessage::SetBreakpoints { node_ids } => {
                assert_eq!(node_ids, vec!["n1_foo", "n2_bar"]);
            }
            _ => panic!("expected SetBreakpoints"),
        }

        let restart: ClientMessage = serde_json::from_str(r#"{"type":"restart"}"#).unwrap();
        assert!(matches!(restart, ClientMessage::Restart));
    }

    #[test]
    fn server_message_serializes() {
        let err = ServerMessage::Error {
            message: "test error".to_string(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "error");
        assert_eq!(parsed["message"], "test error");

        let snap = ServerMessage::Snapshot(StateSnapshot {
            step: 3,
            node_id: Some("n1_foo".to_string()),
            op: Some("http.get".to_string()),
            bindings: HashMap::new(),
            status: "paused".to_string(),
            trace: vec![],
            emits: vec![],
        });
        let json = serde_json::to_string(&snap).unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "snapshot");
        assert_eq!(parsed["step"], 3);
        assert_eq!(parsed["node_id"], "n1_foo");
    }
}
