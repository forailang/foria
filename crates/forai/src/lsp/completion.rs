use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionParams, Documentation, MarkupContent,
    MarkupKind,
};

use crate::ast::TopDecl;
use crate::parser;
use crate::stdlib_docs;

use super::document::Document;

pub fn completions(doc: &Document, params: &CompletionParams) -> Vec<CompletionItem> {
    let pos = params.text_document_position.position;
    let line_num = pos.line as usize;
    let col = pos.character as usize;

    let line_text = doc.text.lines().nth(line_num).unwrap_or("");
    let prefix = &line_text[..col.min(line_text.len())];

    // Check for namespace.prefix pattern (e.g. "str." or "obj.g")
    if let Some(dot_pos) = prefix.rfind('.') {
        let ns = prefix[..dot_pos].trim_start();
        let partial = &prefix[dot_pos + 1..];
        return namespace_completions(ns, partial, &doc.text);
    }

    let trimmed = prefix.trim_start();

    // Top-level keywords at indent 0 or line start
    if trimmed.is_empty() || is_keyword_prefix(trimmed) {
        let indent = prefix.len() - trimmed.len();
        if indent == 0 {
            return top_level_keyword_completions(trimmed);
        } else {
            return body_keyword_completions(trimmed);
        }
    }

    // After "as " — type completions
    if let Some(after_as) = extract_after_keyword(prefix, "as") {
        return type_completions(after_as, &doc.text);
    }

    // After "= " — op completions
    if prefix.contains('=') {
        let after_eq = prefix.rsplit('=').next().unwrap_or("").trim_start();
        return op_completions(after_eq);
    }

    Vec::new()
}

fn is_keyword_prefix(s: &str) -> bool {
    let keywords = [
        "func", "flow", "sink", "source", "docs", "test", "type", "data", "enum", "uses",
        "branch", "case", "when", "else", "loop", "sync", "if", "emit", "fail", "break",
        "step", "body", "done", "take", "return", "must", "trap", "mock", "send", "nowait",
    ];
    keywords.iter().any(|k| k.starts_with(s) && *k != s)
}

fn extract_after_keyword<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let pat = format!(" {keyword} ");
    if let Some(idx) = line.rfind(&pat) {
        return Some(&line[idx + pat.len()..]);
    }
    if line.trim_start().starts_with(&format!("{keyword} ")) {
        let trimmed = line.trim_start();
        return Some(&trimmed[keyword.len() + 1..]);
    }
    None
}

fn top_level_keyword_completions(prefix: &str) -> Vec<CompletionItem> {
    let keywords = [
        ("func", "func declaration", "Imperative computation block"),
        ("flow", "flow declaration", "Declarative pipeline wiring"),
        ("sink", "sink declaration", "Side-effect endpoint"),
        ("source", "source declaration", "Event producer"),
        ("docs", "docs block", "Documentation block"),
        ("test", "test block", "Test block with assertions"),
        ("type", "type declaration", "Struct or scalar type"),
        ("data", "data declaration", "Data type"),
        ("enum", "enum declaration", "Enum type"),
        ("uses", "module import", "Import a module directory"),
    ];

    keywords
        .iter()
        .filter(|(kw, _, _)| kw.starts_with(prefix))
        .map(|(kw, detail, doc)| CompletionItem {
            label: kw.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some(detail.to_string()),
            documentation: Some(Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::PlainText,
                value: doc.to_string(),
            })),
            ..Default::default()
        })
        .collect()
}

fn body_keyword_completions(prefix: &str) -> Vec<CompletionItem> {
    let keywords = [
        ("branch", "conditional routing"),
        ("case", "pattern matching"),
        ("when", "case arm"),
        ("else", "else branch"),
        ("loop", "iteration"),
        ("sync", "concurrent block"),
        ("if", "conditional"),
        ("emit", "emit output"),
        ("fail", "emit failure"),
        ("break", "break loop"),
        ("done", "end block"),
    ];

    keywords
        .iter()
        .filter(|(kw, _)| kw.starts_with(prefix))
        .map(|(kw, detail)| CompletionItem {
            label: kw.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some(detail.to_string()),
            ..Default::default()
        })
        .collect()
}

fn type_completions(prefix: &str, source: &str) -> Vec<CompletionItem> {
    let primitives = [
        "text", "bool", "long", "real", "uuid", "time", "list", "dict", "void",
        "db_conn", "http_server", "http_conn", "ws_conn",
    ];

    let mut items: Vec<CompletionItem> = primitives
        .iter()
        .filter(|t| t.starts_with(prefix))
        .map(|t| CompletionItem {
            label: t.to_string(),
            kind: Some(CompletionItemKind::TYPE_PARAMETER),
            detail: Some("primitive type".to_string()),
            ..Default::default()
        })
        .collect();

    // Add user-defined types from current module
    if let Ok(module) = parser::parse_module_v1(source) {
        for decl in &module.decls {
            let name = match decl {
                TopDecl::Type(t) => &t.name,
                TopDecl::Enum(e) => &e.name,
                _ => continue,
            };
            if name.starts_with(prefix) {
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::STRUCT),
                    detail: Some("user type".to_string()),
                    ..Default::default()
                });
            }
        }
    }

    items
}

fn namespace_completions(namespace: &str, prefix: &str, source: &str) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    // Check stdlib namespaces
    for ns_doc in stdlib_docs::all_stdlib_docs() {
        if ns_doc.namespace == namespace {
            for op in &ns_doc.ops {
                let short_name = op
                    .name
                    .strip_prefix(&format!("{namespace}."))
                    .unwrap_or(&op.name);
                if short_name.starts_with(prefix) {
                    let args_str: String = op
                        .args
                        .iter()
                        .map(|a| format!("{}: {}", a.name, a.type_name))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let sig = format!("({args_str}) → {}", op.returns.type_name);

                    items.push(CompletionItem {
                        label: short_name.to_string(),
                        kind: Some(CompletionItemKind::FUNCTION),
                        detail: Some(sig),
                        documentation: Some(Documentation::MarkupContent(MarkupContent {
                            kind: MarkupKind::PlainText,
                            value: op.summary.clone(),
                        })),
                        ..Default::default()
                    });
                }
            }
            return items;
        }
    }

    // Check if it's a module-qualified call (user module)
    if let Ok(module) = parser::parse_module_v1(source) {
        for decl in &module.decls {
            if let TopDecl::Uses(u) = decl {
                if u.module == namespace {
                    items.extend(module_func_completions(namespace, prefix, source));
                    break;
                }
            }
        }
    }

    items
}

fn module_func_completions(
    _module_name: &str,
    prefix: &str,
    _source: &str,
) -> Vec<CompletionItem> {
    // Try to resolve the module directory and scan for .fa files
    // For now, return empty — would need file path context to resolve
    let _ = prefix;
    Vec::new()
}

fn op_completions(prefix: &str) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    for ns_doc in stdlib_docs::all_stdlib_docs() {
        for op in &ns_doc.ops {
            if op.full_name.starts_with(prefix) {
                let args_str: String = op
                    .args
                    .iter()
                    .map(|a| format!("{}: {}", a.name, a.type_name))
                    .collect::<Vec<_>>()
                    .join(", ");
                let sig = format!("({args_str}) → {}", op.returns.type_name);

                items.push(CompletionItem {
                    label: op.full_name.clone(),
                    kind: Some(CompletionItemKind::FUNCTION),
                    detail: Some(sig),
                    documentation: Some(Documentation::MarkupContent(MarkupContent {
                        kind: MarkupKind::PlainText,
                        value: op.summary.clone(),
                    })),
                    ..Default::default()
                });
            }
        }
    }

    items
}
