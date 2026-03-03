use serde_json::Value;
use std::future::Future;
use std::pin::Pin;

#[allow(dead_code)]
pub trait Host {
    fn execute_io_op<'a>(
        &'a self,
        op: &'a str,
        args: &'a [Value],
    ) -> Pin<Box<dyn Future<Output = Result<Value, String>> + 'a>>;

    fn cleanup(&self);
}

pub fn is_io_op(op: &str) -> bool {
    if op.starts_with("ffi.") {
        return true;
    }
    matches!(
        op,
        // HTTP server
        "http.server.listen"
        | "http.server.accept"
        | "http.server.respond"
        | "http.server.close"
        // HTTP respond convenience
        | "http.respond.html"
        | "http.respond.json"
        | "http.respond.text"
        | "http.respond.file"
        // Generic accept
        | "accept"
        // HTTP client (reqwest)
        | "http.get"
        | "http.post"
        | "http.put"
        | "http.patch"
        | "http.delete"
        | "http.request"
        // WebSocket
        | "ws.connect"
        | "ws.send"
        | "ws.recv"
        | "ws.close"
        // SQLite database
        | "db.open"
        | "db.exec"
        | "db.query"
        | "db.close"
        // File I/O
        | "file.read"
        | "file.write"
        | "file.append"
        | "file.delete"
        | "file.exists"
        | "file.list"
        | "file.mkdir"
        | "file.copy"
        | "file.move"
        | "file.size"
        | "file.is_dir"
        // Terminal I/O
        | "term.print"
        | "term.prompt"
        | "term.clear"
        | "term.size"
        | "term.cursor"
        | "term.move_to"
        | "term.color"
        | "term.read_key"
        // Process execution
        | "exec.run"
        // Environment variables
        | "env.get"
        | "env.set"
        | "env.has"
        | "env.list"
        | "env.remove"
        // Logging
        | "log.debug"
        | "log.info"
        | "log.warn"
        | "log.error"
        | "log.trace"
        // Time (blocking)
        | "time.sleep"
        | "time.tick"
    )
}
