use crate::ffi_manager::{FfiManager, FfiRegistry};
use crate::host::Host;
use crate::runtime::civil_from_days;
use forai_core::pure_ops::{read_f64_arg, read_i64_arg, read_object_arg, read_string_arg};
use base64::Engine;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Handle types — stateful resources (servers, connections, sockets, databases)
// ---------------------------------------------------------------------------

struct ServerHandle {
    listener: std::rc::Rc<tokio::net::TcpListener>,
}

struct ConnectionHandle {
    writer: tokio::net::tcp::OwnedWriteHalf,
}

struct WsHandle {
    socket: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
}

struct DbHandle {
    conn: rusqlite::Connection,
}

pub struct HandleRegistry {
    next_id: u64,
    servers: HashMap<String, ServerHandle>,
    connections: HashMap<String, ConnectionHandle>,
    websockets: HashMap<String, WsHandle>,
    databases: HashMap<String, DbHandle>,
}

impl HandleRegistry {
    pub fn new() -> Self {
        Self {
            next_id: 0,
            servers: HashMap::new(),
            connections: HashMap::new(),
            websockets: HashMap::new(),
            databases: HashMap::new(),
        }
    }

    fn next_key(&mut self, prefix: &str) -> String {
        let key = format!("{prefix}_{}", self.next_id);
        self.next_id += 1;
        key
    }

    fn cleanup(&mut self) {
        self.websockets.clear();
        self.connections.clear();
        self.servers.clear();
        self.databases.clear();
    }
}

// ---------------------------------------------------------------------------
// NativeHost — Host implementation backed by real I/O
// ---------------------------------------------------------------------------

pub struct NativeHost {
    handles: RefCell<HandleRegistry>,
    quiet: bool,
    ffi: RefCell<FfiManager>,
    ffi_registry: FfiRegistry,
}

impl NativeHost {
    pub fn new() -> Self {
        Self {
            handles: RefCell::new(HandleRegistry::new()),
            quiet: false,
            ffi: RefCell::new(FfiManager::new()),
            ffi_registry: FfiRegistry::new(),
        }
    }

    /// Quiet host: silences terminal I/O ops (term.print, term.prompt).
    /// Used during `forai build` to suppress test output.
    pub fn new_quiet() -> Self {
        Self {
            handles: RefCell::new(HandleRegistry::new()),
            quiet: true,
            ffi: RefCell::new(FfiManager::new()),
            ffi_registry: FfiRegistry::new(),
        }
    }

    pub fn with_ffi_registry(mut self, registry: FfiRegistry) -> Self {
        self.ffi_registry = registry;
        self
    }
}

impl Drop for NativeHost {
    fn drop(&mut self) {
        self.handles.borrow_mut().cleanup();
    }
}

impl Host for NativeHost {
    fn execute_io_op<'a>(
        &'a self,
        op: &'a str,
        args: &'a [Value],
    ) -> Pin<Box<dyn Future<Output = Result<Value, String>> + 'a>> {
        Box::pin(async move {
            // FFI dispatch
            if op == "ffi.available" {
                let lib = read_string_arg(args, 0, op)?;
                return Ok(json!(self.ffi.borrow_mut().is_available(&lib)));
            }
            if op.starts_with("ffi.") {
                let meta = self.ffi_registry.get(op)
                    .ok_or_else(|| format!("unknown FFI function `{op}`"))?;
                return self.ffi.borrow_mut().call(
                    &meta.lib_name,
                    &meta.fn_name,
                    args,
                    &meta.param_types,
                    meta.return_type.as_deref(),
                );
            }

            let handles = &self.handles;
            match op {
                // --- SQLite database ops ---
                "db.open" => {
                    let path = read_string_arg(args, 0, op)?;
                    let conn = if path == ":memory:" {
                        rusqlite::Connection::open_in_memory()
                    } else {
                        rusqlite::Connection::open(&path)
                    }
                    .map_err(|e| format!("db.open failed: {e}"))?;
                    let mut h = handles.borrow_mut();
                    let key = h.next_key("db");
                    h.databases.insert(key.clone(), DbHandle { conn });
                    Ok(json!(key))
                }
                "db.exec" => {
                    let conn_key = read_string_arg(args, 0, op)?;
                    let sql = read_string_arg(args, 1, op)?;
                    let params = json_to_sql_params(args, op)?;
                    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                        params.iter().map(|b| b.as_ref()).collect();
                    let h = handles.borrow();
                    let db = h
                        .databases
                        .get(&conn_key)
                        .ok_or_else(|| format!("db.exec: unknown handle `{conn_key}`"))?;
                    let rows_affected = db
                        .conn
                        .execute(&sql, param_refs.as_slice())
                        .map_err(|e| format!("db.exec failed: {e}"))?;
                    Ok(json!({ "rows_affected": rows_affected }))
                }
                "db.query" => {
                    let conn_key = read_string_arg(args, 0, op)?;
                    let sql = read_string_arg(args, 1, op)?;
                    let params = json_to_sql_params(args, op)?;
                    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                        params.iter().map(|b| b.as_ref()).collect();
                    let h = handles.borrow();
                    let db = h
                        .databases
                        .get(&conn_key)
                        .ok_or_else(|| format!("db.query: unknown handle `{conn_key}`"))?;
                    let mut stmt = db
                        .conn
                        .prepare(&sql)
                        .map_err(|e| format!("db.query prepare failed: {e}"))?;
                    let columns: Vec<String> =
                        stmt.column_names().iter().map(|c| c.to_string()).collect();
                    let rows = stmt
                        .query_map(param_refs.as_slice(), |row| {
                            sqlite_row_to_json(row, &columns)
                        })
                        .map_err(|e| format!("db.query failed: {e}"))?;
                    let mut result = Vec::new();
                    for row in rows {
                        let val = row.map_err(|e| format!("db.query row error: {e}"))?;
                        result.push(val);
                    }
                    Ok(json!(result))
                }
                "db.close" => {
                    let conn_key = read_string_arg(args, 0, op)?;
                    let mut h = handles.borrow_mut();
                    if h.databases.remove(&conn_key).is_some() {
                        Ok(json!(true))
                    } else {
                        Err(format!("db.close: unknown handle `{conn_key}`"))
                    }
                }

                // --- Time ops ---
                "time.sleep" | "time.tick" => {
                    let secs = args
                        .first()
                        .and_then(|v| v.as_f64())
                        .ok_or("time.sleep/time.tick expects a number (seconds)")?;
                    tokio::time::sleep(Duration::from_secs_f64(secs)).await;
                    Ok(json!(true))
                }

                // --- Terminal I/O ops ---
                "term.print" => {
                    if !self.quiet {
                        let value = args
                            .first()
                            .ok_or_else(|| format!("Op `{op}` missing arg0"))?;
                        match value {
                            Value::String(s) => println!("{s}"),
                            other => println!("{other}"),
                        }
                    }
                    Ok(json!(true))
                }
                "term.prompt" => {
                    if self.quiet {
                        return Ok(json!(""));
                    }
                    let message = read_string_arg(args, 0, op)?;
                    let op = op.to_string();
                    let line = tokio::task::spawn_blocking(move || {
                        print!("{message}");
                        std::io::Write::flush(&mut std::io::stdout())
                            .map_err(|e| format!("Op `{op}` flush error: {e}"))?;
                        let mut line = String::new();
                        let n = std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut line)
                            .map_err(|e| format!("Op `{op}` read error: {e}"))?;
                        if n == 0 {
                            return Err(
                                "term.prompt: stdin closed (EOF); this app requires interactive input"
                                    .to_string(),
                            );
                        }
                        Ok::<String, String>(
                            line.trim_end_matches('\n')
                                .trim_end_matches('\r')
                                .to_string(),
                        )
                    })
                    .await
                    .map_err(|e| format!("term.prompt join error: {e}"))??;
                    Ok(json!(line))
                }
                "term.clear" => {
                    print!("\x1b[2J\x1b[H");
                    std::io::Write::flush(&mut std::io::stdout())
                        .map_err(|e| format!("Op `{op}` flush error: {e}"))?;
                    Ok(json!(true))
                }
                "term.size" => {
                    let (cols, rows) = crossterm::terminal::size()
                        .map_err(|e| format!("Op `{op}` failed: {e}"))?;
                    Ok(json!({"cols": cols, "rows": rows}))
                }
                "term.cursor" => {
                    let (col, row) = crossterm::cursor::position()
                        .map_err(|e| format!("Op `{op}` failed: {e}"))?;
                    Ok(json!({"col": col, "row": row}))
                }
                "term.move_to" => {
                    let col = read_f64_arg(args, 0, op)? as u16;
                    let row = read_f64_arg(args, 1, op)? as u16;
                    crossterm::execute!(std::io::stdout(), crossterm::cursor::MoveTo(col, row))
                        .map_err(|e| format!("Op `{op}` failed: {e}"))?;
                    Ok(json!(true))
                }
                "term.color" => {
                    let text = read_string_arg(args, 0, op)?;
                    let color_name = read_string_arg(args, 1, op)?;
                    use crossterm::style::{Color, Stylize};
                    let color = match color_name.as_str() {
                        "black" => Color::Black,
                        "red" => Color::Red,
                        "green" => Color::Green,
                        "yellow" => Color::Yellow,
                        "blue" => Color::Blue,
                        "magenta" => Color::Magenta,
                        "cyan" => Color::Cyan,
                        "white" => Color::White,
                        "dark_grey" | "dark_gray" => Color::DarkGrey,
                        "dark_red" => Color::DarkRed,
                        "dark_green" => Color::DarkGreen,
                        "dark_yellow" => Color::DarkYellow,
                        "dark_blue" => Color::DarkBlue,
                        "dark_magenta" => Color::DarkMagenta,
                        "dark_cyan" => Color::DarkCyan,
                        "grey" | "gray" => Color::Grey,
                        other => return Err(format!("Op `{op}` unknown color `{other}`")),
                    };
                    println!("{}", text.with(color));
                    Ok(json!(true))
                }
                "term.read_key" => {
                    use crossterm::event::{self, Event, KeyCode, KeyModifiers};
                    crossterm::terminal::enable_raw_mode()
                        .map_err(|e| format!("Op `{op}` enable raw mode: {e}"))?;
                    let result = loop {
                        match event::read() {
                            Ok(Event::Key(key_event)) => {
                                let key = match key_event.code {
                                    KeyCode::Char(c) => {
                                        if key_event.modifiers.contains(KeyModifiers::CONTROL) {
                                            format!("ctrl+{c}")
                                        } else {
                                            c.to_string()
                                        }
                                    }
                                    KeyCode::Enter => "enter".to_string(),
                                    KeyCode::Esc => "esc".to_string(),
                                    KeyCode::Backspace => "backspace".to_string(),
                                    KeyCode::Tab => "tab".to_string(),
                                    KeyCode::Up => "up".to_string(),
                                    KeyCode::Down => "down".to_string(),
                                    KeyCode::Left => "left".to_string(),
                                    KeyCode::Right => "right".to_string(),
                                    KeyCode::Home => "home".to_string(),
                                    KeyCode::End => "end".to_string(),
                                    KeyCode::PageUp => "page_up".to_string(),
                                    KeyCode::PageDown => "page_down".to_string(),
                                    KeyCode::Delete => "delete".to_string(),
                                    KeyCode::Insert => "insert".to_string(),
                                    KeyCode::F(n) => format!("f{n}"),
                                    _ => "unknown".to_string(),
                                };
                                break Ok(json!(key));
                            }
                            Ok(_) => continue,
                            Err(e) => break Err(format!("Op `{op}` read error: {e}")),
                        }
                    };
                    let _ = crossterm::terminal::disable_raw_mode();
                    result
                }

                // --- File I/O ops ---
                "file.read" => {
                    let path = read_string_arg(args, 0, op)?;
                    let content = tokio::fs::read_to_string(&path)
                        .await
                        .map_err(|e| format!("Op `{op}` failed to read `{path}`: {e}"))?;
                    Ok(json!(content))
                }
                "file.write" => {
                    let path = read_string_arg(args, 0, op)?;
                    let data = read_string_arg(args, 1, op)?;
                    tokio::fs::write(&path, &data)
                        .await
                        .map_err(|e| format!("Op `{op}` failed to write `{path}`: {e}"))?;
                    Ok(json!(true))
                }
                "file.append" => {
                    let path = read_string_arg(args, 0, op)?;
                    let data = read_string_arg(args, 1, op)?;
                    use tokio::io::AsyncWriteExt;
                    let mut file = tokio::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&path)
                        .await
                        .map_err(|e| format!("Op `{op}` failed to open `{path}`: {e}"))?;
                    file.write_all(data.as_bytes())
                        .await
                        .map_err(|e| format!("Op `{op}` failed to append to `{path}`: {e}"))?;
                    Ok(json!(true))
                }
                "file.delete" => {
                    let path = read_string_arg(args, 0, op)?;
                    let p = std::path::Path::new(&path);
                    if p.is_dir() {
                        tokio::fs::remove_dir_all(&path)
                            .await
                            .map_err(|e| format!("Op `{op}` failed to delete `{path}`: {e}"))?;
                    } else {
                        tokio::fs::remove_file(&path)
                            .await
                            .map_err(|e| format!("Op `{op}` failed to delete `{path}`: {e}"))?;
                    }
                    Ok(json!(true))
                }
                "file.exists" => {
                    let path = read_string_arg(args, 0, op)?;
                    Ok(json!(tokio::fs::try_exists(&path).await.unwrap_or(false)))
                }
                "file.list" => {
                    let path = read_string_arg(args, 0, op)?;
                    let mut entries = tokio::fs::read_dir(&path)
                        .await
                        .map_err(|e| format!("Op `{op}` failed to list `{path}`: {e}"))?;
                    let mut names = Vec::new();
                    while let Some(entry) = entries
                        .next_entry()
                        .await
                        .map_err(|e| format!("Op `{op}` read_dir error: {e}"))?
                    {
                        if let Some(name) = entry.file_name().to_str() {
                            names.push(json!(name));
                        }
                    }
                    names.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
                    Ok(Value::Array(names))
                }
                "file.mkdir" => {
                    let path = read_string_arg(args, 0, op)?;
                    tokio::fs::create_dir_all(&path)
                        .await
                        .map_err(|e| format!("Op `{op}` failed to create `{path}`: {e}"))?;
                    Ok(json!(true))
                }
                "file.copy" => {
                    let src = read_string_arg(args, 0, op)?;
                    let dst = read_string_arg(args, 1, op)?;
                    tokio::fs::copy(&src, &dst)
                        .await
                        .map_err(|e| format!("Op `{op}` failed to copy `{src}` -> `{dst}`: {e}"))?;
                    Ok(json!(true))
                }
                "file.move" => {
                    let src = read_string_arg(args, 0, op)?;
                    let dst = read_string_arg(args, 1, op)?;
                    tokio::fs::rename(&src, &dst)
                        .await
                        .map_err(|e| format!("Op `{op}` failed to move `{src}` -> `{dst}`: {e}"))?;
                    Ok(json!(true))
                }
                "file.size" => {
                    let path = read_string_arg(args, 0, op)?;
                    let meta = tokio::fs::metadata(&path)
                        .await
                        .map_err(|e| format!("Op `{op}` failed to stat `{path}`: {e}"))?;
                    Ok(json!(meta.len()))
                }
                "file.is_dir" => {
                    let path = read_string_arg(args, 0, op)?;
                    Ok(json!(
                        tokio::fs::metadata(&path)
                            .await
                            .map(|m| m.is_dir())
                            .unwrap_or(false)
                    ))
                }

                // --- HTTP client ops (reqwest) ---
                "http.get" => {
                    let url = read_string_arg(args, 0, op)?;
                    let options = args.get(1);
                    execute_reqwest("GET", &url, None, options).await
                }
                "http.post" => {
                    let url = read_string_arg(args, 0, op)?;
                    let body = args.get(1).cloned().unwrap_or(Value::Null);
                    let options = args.get(2);
                    execute_reqwest("POST", &url, Some(body), options).await
                }
                "http.put" => {
                    let url = read_string_arg(args, 0, op)?;
                    let body = args.get(1).cloned().unwrap_or(Value::Null);
                    let options = args.get(2);
                    execute_reqwest("PUT", &url, Some(body), options).await
                }
                "http.patch" => {
                    let url = read_string_arg(args, 0, op)?;
                    let body = args.get(1).cloned().unwrap_or(Value::Null);
                    let options = args.get(2);
                    execute_reqwest("PATCH", &url, Some(body), options).await
                }
                "http.delete" => {
                    let url = read_string_arg(args, 0, op)?;
                    let options = args.get(1);
                    execute_reqwest("DELETE", &url, None, options).await
                }
                "http.request" => {
                    let method = read_string_arg(args, 0, op)?;
                    let url = read_string_arg(args, 1, op)?;
                    let options = args.get(2).cloned().unwrap_or_else(|| json!({}));
                    let body = options.get("body").cloned();
                    execute_reqwest(&method, &url, body, Some(&options)).await
                }

                // --- Generic accept op ---
                "accept" => {
                    let handle_id = read_string_arg(args, 0, op)?;
                    if handle_id.starts_with("srv_") {
                        let listener = {
                            let h = handles.borrow();
                            let srv = h.servers.get(&handle_id).ok_or_else(|| {
                                format!("Op `{op}` unknown server handle `{handle_id}`")
                            })?;
                            std::rc::Rc::clone(&srv.listener)
                        };
                        accept_http_connection(op, &listener, handles).await
                    } else if handle_id.starts_with("ws_") {
                        use futures::StreamExt;
                        let mut ws_handle = handles
                            .borrow_mut()
                            .websockets
                            .remove(&handle_id)
                            .ok_or_else(|| format!("Op `{op}` unknown ws handle `{handle_id}`"))?;
                        let msg_opt = ws_handle.socket.next().await;
                        handles.borrow_mut().websockets.insert(handle_id, ws_handle);
                        let msg = msg_opt
                            .ok_or_else(|| format!("Op `{op}` stream closed"))?
                            .map_err(|e| format!("Op `{op}` recv failed: {e}"))?;
                        ws_message_to_json(msg)
                    } else {
                        Err(format!("Op `{op}` unknown handle type for `{handle_id}`"))
                    }
                }

                // --- HTTP server ops ---
                "http.server.listen" => {
                    let port = read_i64_arg(args, 0, op)? as u16;
                    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
                        .await
                        .map_err(|e| format!("Op `{op}` bind failed: {e}"))?;
                    let mut h = handles.borrow_mut();
                    let key = h.next_key("srv");
                    h.servers.insert(
                        key.clone(),
                        ServerHandle {
                            listener: std::rc::Rc::new(listener),
                        },
                    );
                    Ok(json!(key))
                }
                "http.server.accept" => {
                    let handle_id = read_string_arg(args, 0, op)?;
                    let listener = {
                        let h = handles.borrow();
                        let srv = h.servers.get(&handle_id).ok_or_else(|| {
                            format!("Op `{op}` unknown server handle `{handle_id}`")
                        })?;
                        std::rc::Rc::clone(&srv.listener)
                    };
                    accept_http_connection(op, &listener, handles).await
                }
                "http.server.respond" => {
                    let conn_id = read_string_arg(args, 0, op)?;
                    let status = read_i64_arg(args, 1, op)?;
                    let resp_headers = read_object_arg(args, 2, op)?;
                    let body = read_string_arg(args, 3, op)?;

                    let mut writer = {
                        let mut h = handles.borrow_mut();
                        let conn = h
                            .connections
                            .remove(&conn_id)
                            .ok_or_else(|| format!("Op `{op}` unknown connection `{conn_id}`"))?;
                        conn.writer
                    };

                    let mut header_str = String::new();
                    for (k, v) in &resp_headers {
                        let val = v.as_str().unwrap_or("");
                        header_str.push_str(&format!("{k}: {val}\r\n"));
                    }

                    send_http_response(&mut writer, op, status, &header_str, &body).await?;
                    Ok(json!(true))
                }
                "http.respond.html" | "http.respond.json" | "http.respond.text" => {
                    let conn_id = read_string_arg(args, 0, op)?;
                    let status = read_i64_arg(args, 1, op)?;
                    let body = read_string_arg(args, 2, op)?;
                    let extra_headers = if args.len() > 3 {
                        Some(read_object_arg(args, 3, op)?)
                    } else {
                        None
                    };

                    let mut writer = {
                        let mut h = handles.borrow_mut();
                        let conn = h
                            .connections
                            .remove(&conn_id)
                            .ok_or_else(|| format!("Op `{op}` unknown connection `{conn_id}`"))?;
                        conn.writer
                    };

                    let content_type = match op {
                        "http.respond.html" => "text/html; charset=utf-8",
                        "http.respond.json" => "application/json",
                        "http.respond.text" => "text/plain; charset=utf-8",
                        _ => unreachable!(),
                    };
                    let mut header_str = format!("content-type: {content_type}\r\n");
                    if let Some(hdrs) = extra_headers {
                        for (k, v) in &hdrs {
                            if k.to_lowercase() != "content-type" {
                                let val = v.as_str().unwrap_or("");
                                header_str.push_str(&format!("{k}: {val}\r\n"));
                            }
                        }
                    }

                    send_http_response(&mut writer, op, status, &header_str, &body).await?;
                    Ok(json!(true))
                }
                "http.respond.file" => {
                    let conn_id = read_string_arg(args, 0, op)?;
                    let status = read_i64_arg(args, 1, op)?;
                    let file_path = read_string_arg(args, 2, op)?;
                    let content_type = read_string_arg(args, 3, op)?;

                    let bytes = tokio::fs::read(&file_path)
                        .await
                        .map_err(|e| format!("Op `{op}` failed to read `{file_path}`: {e}"))?;

                    let mut writer = {
                        let mut h = handles.borrow_mut();
                        let conn = h
                            .connections
                            .remove(&conn_id)
                            .ok_or_else(|| format!("Op `{op}` unknown connection `{conn_id}`"))?;
                        conn.writer
                    };

                    send_http_response_bytes(&mut writer, op, status, &content_type, &bytes).await?;
                    Ok(json!(true))
                }
                "http.server.close" => {
                    let handle_id = read_string_arg(args, 0, op)?;
                    let mut h = handles.borrow_mut();
                    h.servers
                        .remove(&handle_id)
                        .ok_or_else(|| format!("Op `{op}` unknown server handle `{handle_id}`"))?;
                    Ok(json!(true))
                }

                // --- WebSocket client ops ---
                "ws.connect" => {
                    let url = read_string_arg(args, 0, op)?;
                    let (host, port) = parse_ws_host_port(&url)?;
                    let tcp = tokio::net::TcpStream::connect((&*host, port))
                        .await
                        .map_err(|e| format!("Op `{op}` tcp connect to {host}:{port}: {e}"))?;
                    let (ws, _) = tokio_tungstenite::client_async(&url, tcp)
                        .await
                        .map_err(|e| format!("Op `{op}` ws handshake: {e}"))?;
                    let mut h = handles.borrow_mut();
                    let key = h.next_key("ws");
                    h.websockets.insert(key.clone(), WsHandle { socket: ws });
                    Ok(json!(key))
                }
                "ws.send" => {
                    use futures::SinkExt;
                    let handle_id = read_string_arg(args, 0, op)?;
                    let message = read_string_arg(args, 1, op)?;
                    let mut ws_handle = handles
                        .borrow_mut()
                        .websockets
                        .remove(&handle_id)
                        .ok_or_else(|| format!("Op `{op}` unknown ws handle `{handle_id}`"))?;
                    let result = ws_handle
                        .socket
                        .send(tungstenite::Message::Text(message.into()))
                        .await;
                    handles.borrow_mut().websockets.insert(handle_id, ws_handle);
                    result.map_err(|e| format!("Op `{op}` send failed: {e}"))?;
                    Ok(json!(true))
                }
                "ws.recv" => {
                    use futures::StreamExt;
                    let handle_id = read_string_arg(args, 0, op)?;
                    let mut ws_handle = handles
                        .borrow_mut()
                        .websockets
                        .remove(&handle_id)
                        .ok_or_else(|| format!("Op `{op}` unknown ws handle `{handle_id}`"))?;
                    let msg_opt = ws_handle.socket.next().await;
                    handles.borrow_mut().websockets.insert(handle_id, ws_handle);
                    let msg = msg_opt
                        .ok_or_else(|| format!("Op `{op}` stream closed"))?
                        .map_err(|e| format!("Op `{op}` recv failed: {e}"))?;
                    ws_message_to_json(msg)
                }
                "ws.close" => {
                    let handle_id = read_string_arg(args, 0, op)?;
                    let ws_handle = handles.borrow_mut().websockets.remove(&handle_id);
                    if let Some(mut wh) = ws_handle {
                        let _ = wh.socket.close(None).await;
                    }
                    Ok(json!(true))
                }

                // --- Environment variable ops ---
                "env.get" => {
                    let key = read_string_arg(args, 0, op)?;
                    match std::env::var(&key) {
                        Ok(val) => Ok(json!(val)),
                        Err(_) => {
                            let default = args.get(1).cloned().unwrap_or(json!(""));
                            Ok(default)
                        }
                    }
                }
                "env.set" => {
                    let key = read_string_arg(args, 0, op)?;
                    let val = read_string_arg(args, 1, op)?;
                    // SAFETY: we are single-threaded at the point env ops are called
                    unsafe {
                        std::env::set_var(&key, &val);
                    }
                    Ok(json!(true))
                }
                "env.has" => {
                    let key = read_string_arg(args, 0, op)?;
                    Ok(json!(std::env::var(&key).is_ok()))
                }
                "env.list" => {
                    let mut map = serde_json::Map::new();
                    for (k, v) in std::env::vars() {
                        map.insert(k, json!(v));
                    }
                    Ok(Value::Object(map))
                }
                "env.remove" => {
                    let key = read_string_arg(args, 0, op)?;
                    // SAFETY: we are single-threaded at the point env ops are called
                    unsafe {
                        std::env::remove_var(&key);
                    }
                    Ok(json!(true))
                }

                // --- Process execution ---
                "exec.run" => {
                    let cmd = read_string_arg(args, 0, op)?;
                    let cmd_args = args
                        .get(1)
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();

                    let output = tokio::process::Command::new(&cmd)
                        .args(&cmd_args)
                        .output()
                        .await
                        .map_err(|e| format!("Op `{op}` failed to execute `{cmd}`: {e}"))?;

                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    let code = output.status.code().unwrap_or(-1);

                    Ok(json!({
                        "code": code,
                        "stdout": stdout,
                        "stderr": stderr,
                        "ok": output.status.success()
                    }))
                }

                // --- Logging ops ---
                "log.debug" | "log.info" | "log.warn" | "log.error" | "log.trace" => {
                    let level = op.strip_prefix("log.").unwrap().to_uppercase();
                    let message = read_string_arg(args, 0, op)?;
                    let context = args.get(1);
                    let now = {
                        let d = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default();
                        let secs = d.as_secs();
                        let millis = d.subsec_millis();
                        let (s, m, h, day, mon, year) = {
                            let s = (secs % 60) as u32;
                            let m = ((secs / 60) % 60) as u32;
                            let h = ((secs / 3600) % 24) as u32;
                            let total_days = (secs / 86400) as i64;
                            let (y, mo, d) = civil_from_days(total_days + 719468);
                            (s, m, h, d, mo, y)
                        };
                        format!("{year:04}-{mon:02}-{day:02}T{h:02}:{m:02}:{s:02}.{millis:03}Z")
                    };
                    let line = match context {
                        Some(ctx) if !ctx.is_null() => {
                            let ctx_str = match ctx {
                                Value::String(s) => s.clone(),
                                other => serde_json::to_string(other).unwrap_or_default(),
                            };
                            format!("[{now}] {level}: {message} {ctx_str}")
                        }
                        _ => format!("[{now}] {level}: {message}"),
                    };
                    eprintln!("{line}");
                    Ok(json!(true))
                }

                // --- DOM ops (browser only, no-op on native) ---
                "dom.write" => {
                    let html = args.first()
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if !self.quiet {
                        eprintln!("[dom.write] {html}");
                    }
                    Ok(json!(true))
                }
                "dom.set_title" => {
                    Ok(json!(true))
                }

                _ => Err(format!("Unknown I/O op `{op}`")),
            }
        })
    }

    fn cleanup(&self) {
        self.handles.borrow_mut().cleanup();
    }
}

// ---------------------------------------------------------------------------
// Helper functions (moved from runtime.rs)
// ---------------------------------------------------------------------------

async fn execute_reqwest(
    method: &str,
    url: &str,
    body: Option<Value>,
    options: Option<&Value>,
) -> Result<Value, String> {
    let timeout_ms = options
        .and_then(|o| o.get("timeout_ms"))
        .and_then(|v| v.as_u64())
        .unwrap_or(30000);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .user_agent("forai/0.1")
        .build()
        .map_err(|e| format!("http client build error: {e}"))?;

    let mut req = match method.to_uppercase().as_str() {
        "GET" => client.get(url),
        "POST" => client.post(url),
        "PUT" => client.put(url),
        "PATCH" => client.patch(url),
        "DELETE" => client.delete(url),
        other => client.request(
            other
                .parse::<reqwest::Method>()
                .map_err(|e| format!("invalid method `{other}`: {e}"))?,
            url,
        ),
    };

    if let Some(opts) = options {
        if let Some(hdrs) = opts.get("headers").and_then(|h| h.as_object()) {
            for (k, v) in hdrs {
                if let Some(val) = v.as_str() {
                    req = req.header(k.as_str(), val);
                }
            }
        }
    }

    req = match body {
        Some(Value::String(s)) => req.body(s),
        Some(v) if v.is_object() || v.is_array() => req
            .header("Content-Type", "application/json")
            .body(v.to_string()),
        Some(_) => req.body(""),
        None => req,
    };

    let resp = req
        .send()
        .await
        .map_err(|e| format!("http transport error: {e}"))?;
    let status = resp.status().as_u16() as i64;
    let mut headers_map = serde_json::Map::new();
    for (k, v) in resp.headers() {
        if let Ok(val) = v.to_str() {
            headers_map.insert(k.as_str().to_lowercase(), json!(val));
        }
    }
    let body_text = resp
        .text()
        .await
        .map_err(|e| format!("failed to read response body: {e}"))?;
    Ok(json!({
        "status": status,
        "headers": Value::Object(headers_map),
        "body": body_text
    }))
}

async fn send_http_response(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    op: &str,
    status: i64,
    header_str: &str,
    body: &str,
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    let resp = format!(
        "HTTP/1.1 {status} {}\r\nContent-Length: {}\r\n{header_str}\r\n{body}",
        http_reason_phrase(status),
        body.len()
    );
    writer
        .write_all(resp.as_bytes())
        .await
        .map_err(|e| format!("Op `{op}` write response: {e}"))?;
    writer
        .flush()
        .await
        .map_err(|e| format!("Op `{op}` flush: {e}"))?;
    Ok(())
}

async fn send_http_response_bytes(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    op: &str,
    status: i64,
    content_type: &str,
    body: &[u8],
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    let header = format!(
        "HTTP/1.1 {status} {}\r\nContent-Length: {}\r\ncontent-type: {content_type}\r\n\r\n",
        http_reason_phrase(status),
        body.len()
    );
    writer
        .write_all(header.as_bytes())
        .await
        .map_err(|e| format!("Op `{op}` write response header: {e}"))?;
    writer
        .write_all(body)
        .await
        .map_err(|e| format!("Op `{op}` write response body: {e}"))?;
    writer
        .flush()
        .await
        .map_err(|e| format!("Op `{op}` flush: {e}"))?;
    Ok(())
}

fn http_reason_phrase(status: i64) -> &'static str {
    match status {
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}

fn json_to_sql_params(
    args: &[Value],
    op: &str,
) -> Result<Vec<Box<dyn rusqlite::types::ToSql>>, String> {
    let params_val = args.get(2);
    match params_val {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(arr)) => {
            let mut out: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::with_capacity(arr.len());
            for v in arr {
                let boxed: Box<dyn rusqlite::types::ToSql> = match v {
                    Value::Null => Box::new(rusqlite::types::Null),
                    Value::Bool(b) => Box::new(*b),
                    Value::Number(n) => {
                        if let Some(i) = n.as_i64() {
                            Box::new(i)
                        } else {
                            Box::new(n.as_f64().unwrap_or(0.0))
                        }
                    }
                    Value::String(s) => Box::new(s.clone()),
                    other => Box::new(serde_json::to_string(other).unwrap_or_default()),
                };
                out.push(boxed);
            }
            Ok(out)
        }
        _ => Err(format!("Op `{op}` expected list for params (arg2)")),
    }
}

fn sqlite_row_to_json(row: &rusqlite::Row, columns: &[String]) -> Result<Value, rusqlite::Error> {
    let mut map = serde_json::Map::new();
    for (i, col) in columns.iter().enumerate() {
        let val = row.get_ref(i)?;
        let json_val = match val {
            rusqlite::types::ValueRef::Null => Value::Null,
            rusqlite::types::ValueRef::Integer(n) => json!(n),
            rusqlite::types::ValueRef::Real(f) => json!(f),
            rusqlite::types::ValueRef::Text(bytes) => {
                let s = std::str::from_utf8(bytes).unwrap_or("");
                json!(s)
            }
            rusqlite::types::ValueRef::Blob(bytes) => {
                json!(base64::engine::general_purpose::STANDARD.encode(bytes))
            }
        };
        map.insert(col.clone(), json_val);
    }
    Ok(Value::Object(map))
}

fn parse_ws_host_port(url: &str) -> Result<(String, u16), String> {
    let stripped = url
        .strip_prefix("ws://")
        .or_else(|| url.strip_prefix("wss://"))
        .ok_or_else(|| format!("WebSocket URL must start with ws:// or wss://: {url}"))?;
    let authority = stripped.split('/').next().unwrap_or(stripped);
    let (host, port) = if let Some((h, p)) = authority.rsplit_once(':') {
        let port: u16 = p
            .parse()
            .map_err(|_| format!("invalid port in URL: {url}"))?;
        (h.to_string(), port)
    } else {
        (authority.to_string(), 80)
    };
    Ok((host, port))
}

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

fn ws_message_to_json(msg: tungstenite::Message) -> Result<Value, String> {
    match msg {
        tungstenite::Message::Text(t) => Ok(json!({"type": "text", "data": t.to_string()})),
        tungstenite::Message::Binary(b) => {
            let encoded = base64_encode(&b);
            Ok(json!({"type": "binary", "data": encoded}))
        }
        tungstenite::Message::Close(_) => Ok(json!({"type": "close", "data": ""})),
        tungstenite::Message::Ping(d) => {
            Ok(json!({"type": "ping", "data": String::from_utf8_lossy(&d).to_string()}))
        }
        tungstenite::Message::Pong(d) => {
            Ok(json!({"type": "pong", "data": String::from_utf8_lossy(&d).to_string()}))
        }
        _ => Ok(json!({"type": "other", "data": ""})),
    }
}

async fn accept_http_connection(
    op: &str,
    listener: &tokio::net::TcpListener,
    handles: &RefCell<HandleRegistry>,
) -> Result<Value, String> {
    let (stream, _peer) = listener
        .accept()
        .await
        .map_err(|e| format!("Op `{op}` accept failed: {e}"))?;

    use tokio::io::{AsyncBufReadExt, AsyncReadExt};
    let (read_half, write_half) = stream.into_split();
    let mut reader = tokio::io::BufReader::new(read_half);

    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .await
        .map_err(|e| format!("Op `{op}` read request line: {e}"))?;
    let parts: Vec<&str> = request_line.trim().split_whitespace().collect();
    let method = parts.first().copied().unwrap_or("GET").to_string();
    let raw_path = parts.get(1).copied().unwrap_or("/").to_string();

    let (path, query) = if let Some(idx) = raw_path.find('?') {
        (raw_path[..idx].to_string(), raw_path[idx + 1..].to_string())
    } else {
        (raw_path, String::new())
    };

    let mut headers_map = serde_json::Map::new();
    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .map_err(|e| format!("Op `{op}` read header: {e}"))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            let key = k.trim().to_lowercase();
            let val = v.trim().to_string();
            if key == "content-length" {
                content_length = val.parse().unwrap_or(0);
            }
            headers_map.insert(key, json!(val));
        }
    }

    let body = if content_length > 0 {
        let mut buf = vec![0u8; content_length];
        reader
            .read_exact(&mut buf)
            .await
            .map_err(|e| format!("Op `{op}` read body: {e}"))?;
        String::from_utf8_lossy(&buf).to_string()
    } else {
        String::new()
    };

    let mut h = handles.borrow_mut();
    let conn_key = h.next_key("conn");
    drop(reader);
    h.connections
        .insert(conn_key.clone(), ConnectionHandle { writer: write_half });

    Ok(json!({
        "method": method,
        "path": path,
        "query": query,
        "headers": Value::Object(headers_map),
        "body": body,
        "conn_id": conn_key
    }))
}
