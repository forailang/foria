use wasm_bindgen::prelude::*;
use std::collections::HashMap;

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
            return serde_json::json!({
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
            serde_json::json!({
                "ok": {
                    "entry_ir": ir_json,
                    "entry_flow": flow_json,
                    "type_registry": types_json,
                    "flow_registry": registry_json,
                }
            }).to_string()
        }
        Err(errors) => {
            let errs: Vec<serde_json::Value> = errors
                .iter()
                .map(|e| serde_json::json!({
                    "file": e.file,
                    "line": e.line,
                    "col": e.col,
                    "message": e.message,
                }))
                .collect();
            serde_json::json!({ "errors": errs }).to_string()
        }
    }
}

/// Format .fa source code. Returns the formatted source.
#[wasm_bindgen]
pub fn format_source(source: &str) -> String {
    forai_core::formatter::format_source(source)
}

/// Check if .fa source is already correctly formatted.
#[wasm_bindgen]
pub fn check_formatted(source: &str) -> bool {
    forai_core::formatter::check_formatted(source)
}

/// Lex .fa source into a JSON array of tokens.
/// Each token: `{"kind": "Ident"|"Number"|..., "text": "...", "line": N, "col": N}`
/// Returns `{"ok": [...]}` on success or `{"error": "..."}` on lex error.
#[wasm_bindgen]
pub fn tokenize(source: &str) -> String {
    match forai_core::lexer::lex(source) {
        Ok(tokens) => {
            let arr: Vec<serde_json::Value> = tokens
                .iter()
                .map(|t| serde_json::json!({
                    "kind": format!("{:?}", t.kind),
                    "text": &source[t.start..t.end],
                    "line": t.span.line,
                    "col": t.span.col,
                }))
                .collect();
            serde_json::json!({ "ok": arr }).to_string()
        }
        Err(e) => {
            serde_json::json!({
                "error": format!("{}:{} {}", e.span.line, e.span.col, e.message)
            }).to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_returns_ok_json() {
        let files = serde_json::json!({
            "main.fa": "use lib from \"./lib\"\n\ndocs main\n    Test.\ndone\n\nflow main\n    emit result as text\n    fail error as text\nbody\n    step lib.Greet(\"world\" to :name) then\n        next :result to msg\n    done\n    emit msg to :result\ndone\n\ntest main\n    mock lib.Greet => \"Hello world!\"\n    result = main()\n    must result == \"Hello world!\"\ndone\n",
            "lib/Greet.fa": "docs Greet\n    Greets.\ndone\n\nfunc Greet\n    take name as text\n    emit result as text\n    fail error as text\nbody\n    greeting = \"Hello #{name}!\"\n    emit greeting\ndone\n\ntest Greet\n    result = Greet(\"world\")\n    must result == \"Hello world!\"\ndone\n"
        });
        let result = compile(&files.to_string(), "main.fa");
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed.get("ok").is_some(), "expected ok, got: {}", result);
    }

    #[test]
    fn compile_returns_errors_json() {
        let files = serde_json::json!({});
        let result = compile(&files.to_string(), "missing.fa");
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed.get("errors").is_some());
    }

    #[test]
    fn format_roundtrip() {
        let src = "func Foo\n    take x as text\n    emit result as text\n    fail error as text\nbody\n    emit x\ndone\n";
        let formatted = format_source(src);
        assert!(check_formatted(&formatted));
    }

    #[test]
    fn tokenize_returns_json() {
        let result = tokenize("func main\ndone\n");
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        let tokens = parsed.get("ok").expect("expected ok key");
        assert!(tokens.is_array());
        assert!(!tokens.as_array().unwrap().is_empty());
    }
}
