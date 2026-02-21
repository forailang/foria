use lsp_types::{Hover, HoverContents, HoverParams, MarkupContent, MarkupKind};

use crate::ast::TopDecl;
use crate::parser;
use crate::stdlib_docs;

use super::document::Document;

pub fn hover(doc: &Document, params: &HoverParams) -> Option<Hover> {
    let pos = params.text_document_position_params.position;
    let line_num = pos.line as usize;
    let col = pos.character as usize;

    let line_text = doc.text.lines().nth(line_num)?;
    let word = word_at(line_text, col)?;

    // Try stdlib op (namespace.name pattern)
    if let Some(hover) = stdlib_hover(&word) {
        return Some(hover);
    }

    // Try user-defined func/flow/type
    if let Some(hover) = user_symbol_hover(&word, &doc.text) {
        return Some(hover);
    }

    // Try primitive type
    if let Some(hover) = primitive_type_hover(&word) {
        return Some(hover);
    }

    // Try keyword
    keyword_hover(&word)
}

fn word_at(line: &str, col: usize) -> Option<String> {
    let bytes = line.as_bytes();
    if col >= bytes.len() {
        return None;
    }

    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'.';
    if !is_word(bytes[col]) {
        return None;
    }

    let mut start = col;
    while start > 0 && is_word(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = col;
    while end < bytes.len() && is_word(bytes[end]) {
        end += 1;
    }
    Some(line[start..end].to_string())
}

fn stdlib_hover(word: &str) -> Option<Hover> {
    if !word.contains('.') {
        return None;
    }

    for ns_doc in stdlib_docs::all_stdlib_docs() {
        for op in &ns_doc.ops {
            if op.full_name == word {
                let args_str: String = op
                    .args
                    .iter()
                    .map(|a| format!("  {}: {} — {}", a.name, a.type_name, a.description))
                    .collect::<Vec<_>>()
                    .join("\n");

                let mut text = format!("**{}**\n\n{}\n", op.full_name, op.summary);
                if !op.args.is_empty() {
                    text.push_str(&format!("\n**Args:**\n{args_str}\n"));
                }
                text.push_str(&format!(
                    "\n**Returns:** {} — {}",
                    op.returns.type_name, op.returns.description
                ));
                if let Some(errors) = &op.errors {
                    text.push_str(&format!("\n\n**Errors:** {errors}"));
                }

                return Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: text,
                    }),
                    range: None,
                });
            }
        }
    }
    None
}

fn user_symbol_hover(word: &str, source: &str) -> Option<Hover> {
    let module = parser::parse_module_v1(source).ok()?;

    // Find docs block for this name
    let docs_text = module.decls.iter().find_map(|d| {
        if let TopDecl::Docs(dd) = d {
            if dd.name == word {
                return Some(dd.markdown.clone());
            }
        }
        None
    });

    // Find the declaration
    for decl in &module.decls {
        match decl {
            TopDecl::Func(f) if f.name == word => {
                let mut text = format!("**func {}**\n", f.name);
                for t in &f.takes {
                    text.push_str(&format!("  take {} as {}\n", t.name, t.type_name));
                }
                for e in &f.emits {
                    text.push_str(&format!("  emit {} as {}\n", e.name, e.type_name));
                }
                if let Some(rt) = &f.return_type {
                    text.push_str(&format!("  return {rt}\n"));
                }
                if let Some(docs) = &docs_text {
                    text.push_str(&format!("\n{docs}"));
                }
                return Some(make_hover(text));
            }
            TopDecl::Flow(f) if f.name == word => {
                let mut text = format!("**flow {}**\n", f.name);
                for t in &f.takes {
                    text.push_str(&format!("  take {} as {}\n", t.name, t.type_name));
                }
                if let Some(docs) = &docs_text {
                    text.push_str(&format!("\n{docs}"));
                }
                return Some(make_hover(text));
            }
            TopDecl::Sink(f) if f.name == word => {
                let mut text = format!("**sink {}**\n", f.name);
                if let Some(docs) = &docs_text {
                    text.push_str(&format!("\n{docs}"));
                }
                return Some(make_hover(text));
            }
            TopDecl::Source(f) if f.name == word => {
                let mut text = format!("**source {}**\n", f.name);
                if let Some(docs) = &docs_text {
                    text.push_str(&format!("\n{docs}"));
                }
                return Some(make_hover(text));
            }
            TopDecl::Type(t) if t.name == word => {
                let mut text = format!("**type {}**\n", t.name);
                if let Some(docs) = &docs_text {
                    text.push_str(&format!("\n{docs}"));
                }
                return Some(make_hover(text));
            }
            TopDecl::Enum(e) if e.name == word => {
                let mut text = format!("**enum {}**\n", e.name);
                text.push_str(&format!("  variants: {}\n", e.variants.join(", ")));
                if let Some(docs) = &docs_text {
                    text.push_str(&format!("\n{docs}"));
                }
                return Some(make_hover(text));
            }
            _ => {}
        }
    }

    None
}

fn primitive_type_hover(word: &str) -> Option<Hover> {
    let desc = match word {
        "text" => "UTF-8 string type",
        "bool" => "Boolean type (true/false)",
        "long" => "64-bit signed integer",
        "real" => "64-bit floating point number",
        "uuid" => "UUID v4 string",
        "time" => "ISO 8601 timestamp",
        "list" => "Ordered collection (JSON array)",
        "dict" => "Key-value map (JSON object)",
        "void" => "No value (unit type)",
        "db_conn" => "SQLite database connection handle",
        "http_server" => "HTTP server handle",
        "http_conn" => "HTTP connection handle",
        "ws_conn" => "WebSocket connection handle",
        _ => return None,
    };

    Some(make_hover(format!("**{word}** — {desc}")))
}

fn keyword_hover(word: &str) -> Option<Hover> {
    let desc = match word {
        "func" => "Declares an imperative computation block with take/emit/fail ports",
        "flow" => "Declares a declarative pipeline that wires sources, funcs, and sinks",
        "sink" => "Declares a side-effect-only endpoint",
        "source" => "Declares an event producer (listener, poller, stream)",
        "docs" => "Documentation block — required for every func, flow, type, enum, and test",
        "test" => "Test block with `must` assertions and optional `trap`/`mock`",
        "type" => "Declares a struct or scalar type with optional constraints",
        "data" => "Declares a data type",
        "enum" => "Declares an enumeration type with string variants",
        "uses" => "Imports a module directory — resolves relative to this file's directory",
        "case" => "Pattern matching: `case value` with `when` arms and `else`",
        "loop" => "Iteration: `loop items as item` or bare `loop` with `break`",
        "sync" => "Concurrent execution: runs body statements in parallel via join_all",
        "if" => "Conditional: `if condition` ... `done`",
        "emit" => "Sends a value to the output port",
        "fail" => "Sends a value to the failure port",
        "take" => "Declares an input port with a name and type",
        "done" => "Ends a block (func, flow, case, loop, sync, if, type, docs, test)",
        "body" => "Begins the executable body of a func or flow",
        "break" => "Exits the current loop",
        "must" => "Test assertion — fails the test if the expression is false",
        "trap" => "Captures a failure in a test block for inspection",
        "mock" => "Substitutes a sub-func call with a fixed value in tests",
        "branch" => "Conditional sub-pipeline in a flow: `branch when <expr>` runs body only if true; `branch` always runs",
        "step" => "Declares a pipeline stage inside a flow body",
        "nowait" => "Fire-and-forget: starts a background task without waiting",
        _ => return None,
    };

    Some(make_hover(format!("**{word}** — {desc}")))
}

fn make_hover(text: String) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: text,
        }),
        range: None,
    }
}
