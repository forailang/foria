use std::cell::RefCell;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;

use forai_core::sync_host::SyncHost;
use serde_json::{Value, json};

// --- JS bridge functions via inline_js ---

#[wasm_bindgen(inline_js = "
export function console_log_op(level, message, context) {
    const fn_map = { debug: 'debug', info: 'log', warn: 'warn', error: 'error', trace: 'trace' };
    const method = fn_map[level] || 'log';
    if (context && context.length > 0) {
        console[method](message, context);
    } else {
        console[method](message);
    }
}

export function sync_http_request(method, url, headers_json, body) {
    try {
        const xhr = new XMLHttpRequest();
        xhr.open(method.toUpperCase(), url, false);
        if (headers_json && headers_json.length > 0) {
            try {
                const headers = JSON.parse(headers_json);
                for (const [k, v] of Object.entries(headers)) {
                    xhr.setRequestHeader(k, String(v));
                }
            } catch (_) {}
        }
        xhr.send(body || null);
        const respHeaders = {};
        const rawHeaders = xhr.getAllResponseHeaders();
        if (rawHeaders) {
            for (const line of rawHeaders.trim().split(/\\r?\\n/)) {
                const idx = line.indexOf(':');
                if (idx > 0) {
                    respHeaders[line.slice(0, idx).trim().toLowerCase()] = line.slice(idx + 1).trim();
                }
            }
        }
        return JSON.stringify({ status: xhr.status, headers: respHeaders, body: xhr.responseText });
    } catch (e) {
        return JSON.stringify({ error: e.message || String(e) });
    }
}
")]
extern "C" {
    fn console_log_op(level: &str, message: &str, context: &str);
    fn sync_http_request(method: &str, url: &str, headers_json: &str, body: &str) -> String;
}

/// Compile a .fa project from JSON-encoded virtual files.
///
/// `files_json`: JSON object mapping path → source text, e.g.
///   `{"main.fa": "flow main\nbody\n...\ndone", "lib/Greet.fa": "..."}`
/// `entry_point`: path of the entry file, e.g. "main.fa"
///
/// Returns JSON string:
///   On success: `{"ok": <ProgramBundle as JSON>}`
///   On error:   `{"errors": [{"file","line","col","message"}, ...]}`
#[wasm_bindgen]
pub fn compile(files_json: &str, entry_point: &str) -> String {
    let files: HashMap<String, String> = match serde_json::from_str(files_json) {
        Ok(f) => f,
        Err(e) => {
            return json!({
                "errors": [{"file": "", "line": 0, "col": 0, "message": format!("invalid files JSON: {e}")}]
            }).to_string();
        }
    };

    match forai_core::compile::compile_project(&files, entry_point) {
        Ok(bundle) => {
            let ir_json = serde_json::to_value(&bundle.entry_ir).unwrap_or_default();
            let flow_json = serde_json::to_value(&bundle.entry_flow).unwrap_or_default();
            let types_json = serde_json::to_value(&bundle.type_registry).unwrap_or_default();
            let registry_json = serde_json::to_value(&bundle.flow_registry).unwrap_or_default();
            json!({
                "ok": {
                    "entry_ir": ir_json,
                    "entry_flow": flow_json,
                    "type_registry": types_json,
                    "flow_registry": registry_json,
                }
            })
            .to_string()
        }
        Err(errors) => {
            let errs: Vec<Value> = errors
                .iter()
                .map(|e| {
                    json!({
                        "file": e.file,
                        "line": e.line,
                        "col": e.col,
                        "message": e.message,
                    })
                })
                .collect();
            json!({ "errors": errs }).to_string()
        }
    }
}

/// Execute a .fa project: compile then run via sync_runtime.
///
/// Returns JSON string:
///   On success: `{"ok": {"prints": [...], "logs": [...], "outputs": {...}}}`
///   On compile error: `{"compile_errors": [...]}`
///   On runtime error: `{"error": "message"}`
#[wasm_bindgen]
pub fn execute(files_json: &str, entry_point: &str) -> String {
    let files: HashMap<String, String> = match serde_json::from_str(files_json) {
        Ok(f) => f,
        Err(e) => {
            return json!({
                "compile_errors": [{"file": "", "line": 0, "col": 0, "message": format!("invalid files JSON: {e}")}]
            }).to_string();
        }
    };

    let bundle = match forai_core::compile::compile_project(&files, entry_point) {
        Ok(b) => b,
        Err(errors) => {
            let errs: Vec<Value> = errors
                .iter()
                .map(|e| {
                    json!({
                        "file": e.file,
                        "line": e.line,
                        "col": e.col,
                        "message": e.message,
                    })
                })
                .collect();
            return json!({ "compile_errors": errs }).to_string();
        }
    };

    let host = PlaygroundHost::new();
    let codecs = forai_core::codec::CodecRegistry::default_registry();

    match forai_core::sync_runtime::execute_flow(
        &bundle.entry_flow,
        bundle.entry_ir,
        HashMap::new(),
        &bundle.type_registry,
        Some(&bundle.flow_registry),
        &codecs,
        &host,
    ) {
        Ok(result) => json!({
            "ok": {
                "prints": host.take_prints(),
                "logs": host.take_logs(),
                "outputs": result.outputs,
            }
        })
        .to_string(),
        Err(e) => json!({
            "error": e,
            "prints": host.take_prints(),
            "logs": host.take_logs(),
        })
        .to_string(),
    }
}

/// Execute with stepping: compile then run, collecting a snapshot after every op.
///
/// Returns JSON string:
///   `{"ok": {"snapshots": [...], "prints": [...], "logs": [...], "outputs": {...}}}`
///   `{"compile_errors": [...]}`
///   `{"error": "message", "snapshots": [...], "prints": [...], "logs": [...]}`
#[wasm_bindgen]
pub fn execute_stepping(files_json: &str, entry_point: &str) -> String {
    let files: HashMap<String, String> = match serde_json::from_str(files_json) {
        Ok(f) => f,
        Err(e) => {
            return json!({
                "compile_errors": [{"file": "", "line": 0, "col": 0, "message": format!("invalid files JSON: {e}")}]
            }).to_string();
        }
    };

    let bundle = match forai_core::compile::compile_project(&files, entry_point) {
        Ok(b) => b,
        Err(errors) => {
            let errs: Vec<Value> = errors
                .iter()
                .map(|e| {
                    json!({
                        "file": e.file,
                        "line": e.line,
                        "col": e.col,
                        "message": e.message,
                    })
                })
                .collect();
            return json!({ "compile_errors": errs }).to_string();
        }
    };

    let host = PlaygroundHost::new();
    let codecs = forai_core::codec::CodecRegistry::default_registry();

    match forai_core::sync_runtime::execute_flow_stepping(
        &bundle.entry_flow,
        bundle.entry_ir,
        HashMap::new(),
        &bundle.type_registry,
        Some(&bundle.flow_registry),
        &codecs,
        &host,
    ) {
        Ok((result, snapshots)) => json!({
            "ok": {
                "snapshots": snapshots,
                "prints": host.take_prints(),
                "logs": host.take_logs(),
                "outputs": result.outputs,
            }
        })
        .to_string(),
        Err(e) => json!({
            "error": e,
            "prints": host.take_prints(),
            "logs": host.take_logs(),
        })
        .to_string(),
    }
}

/// Format .fa source code. Returns the formatted source.
#[wasm_bindgen]
pub fn format_source(source: &str) -> String {
    forai_core::formatter::format_source(source)
}

// --- PlaygroundHost: captures I/O ops for browser display ---

struct PlaygroundHost {
    prints: RefCell<Vec<String>>,
    logs: RefCell<Vec<Value>>,
}

impl PlaygroundHost {
    fn new() -> Self {
        Self {
            prints: RefCell::new(Vec::new()),
            logs: RefCell::new(Vec::new()),
        }
    }

    fn take_prints(&self) -> Vec<String> {
        self.prints.borrow().clone()
    }

    fn take_logs(&self) -> Vec<Value> {
        self.logs.borrow().clone()
    }
}

impl SyncHost for PlaygroundHost {
    fn execute_io_op(&self, op: &str, args: &[Value]) -> Result<Value, String> {
        match op {
            // Terminal output — capture
            "term.print" => {
                let text = args
                    .first()
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                self.prints.borrow_mut().push(text);
                Ok(json!(true))
            }
            "term.clear" => Ok(json!(true)),
            "term.size" => Ok(json!({"cols": 80, "rows": 24})),
            "term.cursor" => Ok(json!({"col": 0, "row": 0})),
            "term.move_to" | "term.color" => Ok(json!(true)),
            "term.read_key" => Err("term.read_key is not available in the playground".into()),
            "term.prompt" => {
                // Return empty string — no interactive input in playground v1
                Ok(json!(""))
            }

            // Logging — capture with level + forward to browser console
            "log.debug" | "log.info" | "log.warn" | "log.error" | "log.trace" => {
                let level = op.strip_prefix("log.").unwrap_or("info");
                let message = args
                    .first()
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let context_str = args
                    .get(1)
                    .map(|v| match v {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .unwrap_or_default();
                console_log_op(level, &message, &context_str);
                self.logs.borrow_mut().push(json!({
                    "level": level,
                    "message": message,
                }));
                Ok(json!(true))
            }

            // Time — no-op in playground
            "time.sleep" | "time.tick" => Ok(json!(true)),

            // Unavailable ops — clear error messages
            op if op.starts_with("file.") => {
                Err(format!("{op} is not available in the playground"))
            }
            op if op.starts_with("db.") => Err(format!("{op} is not available in the playground")),
            op if op.starts_with("exec.") => {
                Err(format!("{op} is not available in the playground"))
            }
            op if op.starts_with("env.") => Err(format!("{op} is not available in the playground")),
            op if op.starts_with("http.server.")
                || op.starts_with("http.respond.")
                || op == "accept" =>
            {
                Err(format!("{op} is not available in the playground"))
            }
            // HTTP client ops — synchronous XHR in Web Worker
            "http.get" | "http.post" | "http.put" | "http.patch" | "http.delete"
            | "http.request" => {
                let (method, url, body, options) = match op {
                    "http.get" => {
                        let url = args
                            .first()
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let options = args.get(1).cloned();
                        ("GET".to_string(), url, None, options)
                    }
                    "http.post" | "http.put" | "http.patch" => {
                        let m = op.strip_prefix("http.").unwrap().to_uppercase();
                        let url = args
                            .first()
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let body = args.get(1).cloned();
                        let options = args.get(2).cloned();
                        (m, url, body, options)
                    }
                    "http.delete" => {
                        let url = args
                            .first()
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let options = args.get(1).cloned();
                        ("DELETE".to_string(), url, None, options)
                    }
                    "http.request" => {
                        let m = args
                            .first()
                            .and_then(|v| v.as_str())
                            .unwrap_or("GET")
                            .to_string();
                        let url = args
                            .get(1)
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let options = args.get(2).cloned();
                        let body = options.as_ref().and_then(|o| o.get("body").cloned());
                        (m, url, body, options)
                    }
                    _ => unreachable!(),
                };

                // Extract headers JSON from options
                let headers_json = options
                    .as_ref()
                    .and_then(|o| o.get("headers"))
                    .map(|h| h.to_string())
                    .unwrap_or_default();

                // Serialize body
                let body_str = match body {
                    Some(Value::String(s)) => s,
                    Some(v) if v.is_object() || v.is_array() => v.to_string(),
                    Some(Value::Null) | None => String::new(),
                    Some(v) => v.to_string(),
                };

                let result_json = sync_http_request(&method, &url, &headers_json, &body_str);
                let parsed: Value = serde_json::from_str(&result_json)
                    .map_err(|e| format!("http response parse error: {e}"))?;

                if let Some(err) = parsed.get("error").and_then(|v| v.as_str()) {
                    Err(format!("http transport error: {err}"))
                } else {
                    Ok(parsed)
                }
            }
            op if op.starts_with("ws.") => Err(format!(
                "{op} is not available in the playground (no network access)"
            )),
            op if op.starts_with("ffi.") => Err(format!("{op} is not available in the playground")),

            _ => Err(format!("unknown I/O op: {op}")),
        }
    }

    fn cleanup(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_returns_ok_json() {
        let files = json!({
            "main.fa": "use lib from \"./lib\"\n\ndocs main\n    Test.\ndone\n\nflow main\n    emit result as text\n    fail error as text\nbody\n    step lib.Greet(\"world\" to :name) then\n        next :result to msg\n    done\n    emit msg to :result\ndone\n\ntest main\n    mock lib.Greet => \"Hello world!\"\n    result = main()\n    must result == \"Hello world!\"\ndone\n",
            "lib/Greet.fa": "docs Greet\n    Greets.\ndone\n\nfunc Greet\n    take name as text\n    emit result as text\n    fail error as text\nbody\n    greeting = \"Hello #{name}!\"\n    emit greeting\ndone\n\ntest Greet\n    result = Greet(\"world\")\n    must result == \"Hello world!\"\ndone\n"
        });
        let result = compile(&files.to_string(), "main.fa");
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert!(parsed.get("ok").is_some(), "expected ok, got: {}", result);
    }

    #[test]
    fn compile_returns_errors_json() {
        let files = json!({});
        let result = compile(&files.to_string(), "missing.fa");
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert!(parsed.get("errors").is_some());
    }

    #[test]
    fn format_roundtrip() {
        let src = "func Foo\n    take x as text\n    emit result as text\n    fail error as text\nbody\n    emit x\ndone\n";
        let formatted = format_source(src);
        assert_eq!(src, formatted);
    }

    #[test]
    fn execute_simple_flow() {
        let files = json!({
            "main.fa": "use lib from \"./lib\"\n\ndocs main\n    Test.\ndone\n\nflow main\n    emit result as text\n    fail error as text\nbody\n    step lib.Greet(\"world\" to :name) then\n        next :result to msg\n    done\n    emit msg to :result\ndone\n\ntest main\n    mock lib.Greet => \"Hello world!\"\n    result = main()\n    must result == \"Hello world!\"\ndone\n",
            "lib/Greet.fa": "docs Greet\n    Greets.\ndone\n\nfunc Greet\n    take name as text\n    emit result as text\n    fail error as text\nbody\n    greeting = \"Hello #{name}!\"\n    _ = term.print(greeting)\n    emit greeting\ndone\n\ntest Greet\n    mock term.print => true\n    result = Greet(\"world\")\n    must result == \"Hello world!\"\ndone\n"
        });
        let result = execute(&files.to_string(), "main.fa");
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert!(parsed.get("ok").is_some(), "expected ok, got: {result}");
        let prints = parsed["ok"]["prints"].as_array().unwrap();
        assert_eq!(prints.len(), 1);
        assert_eq!(prints[0].as_str().unwrap(), "Hello world!");
    }

    #[test]
    fn execute_unavailable_op_error() {
        let files = json!({
            "main.fa": "use lib from \"./lib\"\n\ndocs main\n    Test.\ndone\n\nflow main\n    emit result as text\n    fail error as text\nbody\n    step lib.ReadFile() then\n        next :result to msg\n    done\n    emit msg to :result\ndone\n\ntest main\n    mock lib.ReadFile => \"test\"\n    result = main()\n    must result == \"test\"\ndone\n",
            "lib/ReadFile.fa": "docs ReadFile\n    Read.\ndone\n\nfunc ReadFile\n    emit result as text\n    fail error as text\nbody\n    data = file.read(\"test.txt\")\n    emit data\ndone\n\ntest ReadFile\n    mock file.read => \"test\"\n    result = ReadFile()\n    must result == \"test\"\ndone\n"
        });
        let result = execute(&files.to_string(), "main.fa");
        let parsed: Value = serde_json::from_str(&result).unwrap();
        // Should get a runtime error about file.read not available
        assert!(
            parsed.get("error").is_some(),
            "expected error for unavailable op, got: {result}"
        );
        let error = parsed["error"].as_str().unwrap();
        assert!(
            error.contains("not available"),
            "error should mention not available: {error}"
        );
    }
}
