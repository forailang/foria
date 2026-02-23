use crate::mcp::protocol::{JsonRpcError, JsonRpcMessage, JsonRpcResponse};
use crate::mcp::tools;
use serde_json::{Value, json};
use std::io::{BufRead, Write};

pub async fn main_loop() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // Spawn a blocking thread for stdin reads
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let reader = stdin.lock();
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    if tx.send(l).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    while let Some(line) = rx.recv().await {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let msg: JsonRpcMessage = match serde_json::from_str(trimmed) {
            Ok(m) => m,
            Err(e) => {
                let err = JsonRpcError::new(Value::Null, -32700, format!("Parse error: {e}"));
                write_response(&serde_json::to_value(&err).unwrap());
                continue;
            }
        };

        // Notifications (no id) — just acknowledge silently
        if msg.id.is_none() {
            continue;
        }

        let id = msg.id.unwrap();
        let method = msg.method.as_deref().unwrap_or("");
        let params = msg.params.unwrap_or(Value::Null);

        let response = match method {
            "initialize" => handle_initialize(id, &params),
            "tools/list" => handle_tools_list(id),
            "tools/call" => handle_tools_call(id, &params).await,
            "ping" => {
                let resp = JsonRpcResponse::new(id, json!({}));
                serde_json::to_value(&resp).unwrap()
            }
            _ => {
                let err = JsonRpcError::method_not_found(id, method);
                serde_json::to_value(&err).unwrap()
            }
        };

        write_response(&response);
    }
}

fn write_response(response: &Value) {
    let mut stdout = std::io::stdout().lock();
    let _ = serde_json::to_writer(&mut stdout, response);
    let _ = stdout.write_all(b"\n");
    let _ = stdout.flush();
}

fn handle_initialize(id: Value, _params: &Value) -> Value {
    let resp = JsonRpcResponse::new(
        id,
        json!({
            "protocolVersion": "2024-11-05",
            "serverInfo": {
                "name": "forai-mcp",
                "version": env!("CARGO_PKG_VERSION")
            },
            "capabilities": {
                "tools": {}
            }
        }),
    );
    serde_json::to_value(&resp).unwrap()
}

fn handle_tools_list(id: Value) -> Value {
    let tool_defs = tools::tool_definitions();
    let resp = JsonRpcResponse::new(id, json!({ "tools": tool_defs }));
    serde_json::to_value(&resp).unwrap()
}

async fn handle_tools_call(id: Value, params: &Value) -> Value {
    let name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => {
            let err = JsonRpcError::invalid_params(id, "Missing tool name".into());
            return serde_json::to_value(&err).unwrap();
        }
    };

    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
    let result = tools::call_tool(name, &arguments).await;

    let resp = JsonRpcResponse::new(id, serde_json::to_value(&result).unwrap());
    serde_json::to_value(&resp).unwrap()
}
