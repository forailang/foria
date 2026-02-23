use lsp_types::{DocumentSymbol, DocumentSymbolParams, Position, Range, SymbolKind};

use crate::ast::TopDecl;
use crate::parser;

use super::document::Document;

#[allow(deprecated)] // DocumentSymbol.deprecated field
pub fn document_symbols(doc: &Document, _params: &DocumentSymbolParams) -> Vec<DocumentSymbol> {
    let module = match parser::parse_module_v1(&doc.text) {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };

    let mut symbols = Vec::new();
    let last_line = doc.line_index.line_count().saturating_sub(1) as u32;

    for decl in &module.decls {
        match decl {
            TopDecl::Func(f) => {
                let mut children = Vec::new();
                for t in &f.takes {
                    children.push(port_symbol("take", &t.name, &t.type_name, &f.span));
                }
                for e in &f.emits {
                    children.push(port_symbol("emit", &e.name, &e.type_name, &f.span));
                }
                for fl in &f.fails {
                    children.push(port_symbol("fail", &fl.name, &fl.type_name, &f.span));
                }
                symbols.push(make_symbol(
                    &f.name,
                    SymbolKind::FUNCTION,
                    &f.span,
                    last_line,
                    Some("func".to_string()),
                    children,
                ));
            }
            TopDecl::Sink(f) => {
                symbols.push(make_symbol(
                    &f.name,
                    SymbolKind::FUNCTION,
                    &f.span,
                    last_line,
                    Some("sink".to_string()),
                    Vec::new(),
                ));
            }
            TopDecl::Source(f) => {
                symbols.push(make_symbol(
                    &f.name,
                    SymbolKind::EVENT,
                    &f.span,
                    last_line,
                    Some("source".to_string()),
                    Vec::new(),
                ));
            }
            TopDecl::Flow(f) => {
                let mut children = Vec::new();
                for t in &f.takes {
                    children.push(port_symbol("take", &t.name, &t.type_name, &f.span));
                }
                for e in &f.emits {
                    children.push(port_symbol("emit", &e.name, &e.type_name, &f.span));
                }
                symbols.push(make_symbol(
                    &f.name,
                    SymbolKind::MODULE,
                    &f.span,
                    last_line,
                    Some("flow".to_string()),
                    children,
                ));
            }
            TopDecl::Type(t) => {
                let children = match &t.kind {
                    crate::ast::TypeKind::Struct { fields } => fields
                        .iter()
                        .map(|f| field_symbol(&f.name, &f.type_ref, &t.span))
                        .collect(),
                    _ => Vec::new(),
                };
                symbols.push(make_symbol(
                    &t.name,
                    SymbolKind::STRUCT,
                    &t.span,
                    last_line,
                    Some("type".to_string()),
                    children,
                ));
            }
            TopDecl::Enum(e) => {
                let children = e
                    .variants
                    .iter()
                    .map(|v| variant_symbol(v, &e.span))
                    .collect();
                symbols.push(make_symbol(
                    &e.name,
                    SymbolKind::ENUM,
                    &e.span,
                    last_line,
                    Some("enum".to_string()),
                    children,
                ));
            }
            TopDecl::Test(t) => {
                symbols.push(make_symbol(
                    &t.name,
                    SymbolKind::METHOD,
                    &t.span,
                    last_line,
                    Some("test".to_string()),
                    Vec::new(),
                ));
            }
            TopDecl::Docs(_) | TopDecl::Uses(_) => {}
        }
    }

    symbols
}

#[allow(deprecated)]
fn make_symbol(
    name: &str,
    kind: SymbolKind,
    span: &crate::ast::Span,
    last_line: u32,
    detail: Option<String>,
    children: Vec<DocumentSymbol>,
) -> DocumentSymbol {
    let line = if span.line > 0 {
        (span.line - 1) as u32
    } else {
        0
    };
    let col = if span.col > 0 {
        (span.col - 1) as u32
    } else {
        0
    };
    let start = Position {
        line,
        character: col,
    };
    // Approximate range: from declaration line to next decl or end of file
    let selection_range = Range {
        start,
        end: Position {
            line,
            character: col + name.len() as u32,
        },
    };
    let range = Range {
        start,
        end: Position {
            line: last_line,
            character: 0,
        },
    };

    DocumentSymbol {
        name: name.to_string(),
        detail,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: if children.is_empty() {
            None
        } else {
            Some(children)
        },
    }
}

#[allow(deprecated)]
fn port_symbol(
    kind_str: &str,
    name: &str,
    type_name: &str,
    parent_span: &crate::ast::Span,
) -> DocumentSymbol {
    let line = if parent_span.line > 0 {
        (parent_span.line - 1) as u32
    } else {
        0
    };
    let pos = Position { line, character: 0 };
    DocumentSymbol {
        name: format!("{kind_str} {name}"),
        detail: Some(type_name.to_string()),
        kind: SymbolKind::PROPERTY,
        tags: None,
        deprecated: None,
        range: Range {
            start: pos,
            end: pos,
        },
        selection_range: Range {
            start: pos,
            end: pos,
        },
        children: None,
    }
}

#[allow(deprecated)]
fn field_symbol(name: &str, type_name: &str, parent_span: &crate::ast::Span) -> DocumentSymbol {
    let line = if parent_span.line > 0 {
        (parent_span.line - 1) as u32
    } else {
        0
    };
    let pos = Position { line, character: 0 };
    DocumentSymbol {
        name: name.to_string(),
        detail: Some(type_name.to_string()),
        kind: SymbolKind::FIELD,
        tags: None,
        deprecated: None,
        range: Range {
            start: pos,
            end: pos,
        },
        selection_range: Range {
            start: pos,
            end: pos,
        },
        children: None,
    }
}

#[allow(deprecated)]
fn variant_symbol(name: &str, parent_span: &crate::ast::Span) -> DocumentSymbol {
    let line = if parent_span.line > 0 {
        (parent_span.line - 1) as u32
    } else {
        0
    };
    let pos = Position { line, character: 0 };
    DocumentSymbol {
        name: name.to_string(),
        detail: None,
        kind: SymbolKind::ENUM_MEMBER,
        tags: None,
        deprecated: None,
        range: Range {
            start: pos,
            end: pos,
        },
        selection_range: Range {
            start: pos,
            end: pos,
        },
        children: None,
    }
}
