use std::path::PathBuf;

use lsp_types::{GotoDefinitionParams, GotoDefinitionResponse, Location, Position, Range, Uri};

use crate::ast::TopDecl;
use crate::parser;

use super::document::Document;

pub fn goto_definition(
    doc: &Document,
    params: &GotoDefinitionParams,
    uri: &Uri,
) -> Option<GotoDefinitionResponse> {
    let pos = params.text_document_position_params.position;
    let line_num = pos.line as usize;
    let col = pos.character as usize;

    let line_text = doc.text.lines().nth(line_num)?;
    let word = word_at(line_text, col)?;

    let module = parser::parse_module_v1(&doc.text).ok()?;

    // Check for type reference (after "as")
    if let Some(loc) = find_type_definition(&word, &module, uri) {
        return Some(GotoDefinitionResponse::Scalar(loc));
    }

    // Check for module-qualified call (e.g., "router.Classify")
    if word.contains('.') {
        if let Some(loc) = find_module_func(&word, &module, uri) {
            return Some(GotoDefinitionResponse::Scalar(loc));
        }
    }

    // Check for `uses` module — navigate to directory
    if let Some(loc) = find_uses_module(&word, &module, uri) {
        return Some(GotoDefinitionResponse::Scalar(loc));
    }

    // Check for same-file func/flow/sink/source declaration
    if let Some(loc) = find_local_declaration(&word, &module, uri) {
        return Some(GotoDefinitionResponse::Scalar(loc));
    }

    None
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

fn uri_to_file_path(uri: &Uri) -> Option<PathBuf> {
    let s = uri.as_str();
    s.strip_prefix("file://").map(|p| {
        let decoded = percent_decode(p);
        PathBuf::from(decoded)
    })
}

fn file_path_to_uri(path: &std::path::Path) -> Option<Uri> {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        return None;
    };
    let s = format!("file://{}", abs.display());
    s.parse().ok()
}

fn percent_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let h = chars.next().unwrap_or(b'0');
            let l = chars.next().unwrap_or(b'0');
            let val = hex_val(h) * 16 + hex_val(l);
            result.push(val as char);
        } else {
            result.push(b as char);
        }
    }
    result
}

fn hex_val(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}

fn find_type_definition(name: &str, module: &crate::ast::ModuleAst, uri: &Uri) -> Option<Location> {
    for decl in &module.decls {
        match decl {
            TopDecl::Type(t) if t.name == name => {
                return Some(span_to_location(uri, &t.span));
            }
            TopDecl::Enum(e) if e.name == name => {
                return Some(span_to_location(uri, &e.span));
            }
            _ => {}
        }
    }
    None
}

fn find_local_declaration(
    name: &str,
    module: &crate::ast::ModuleAst,
    uri: &Uri,
) -> Option<Location> {
    for decl in &module.decls {
        match decl {
            TopDecl::Func(f) if f.name == name => return Some(span_to_location(uri, &f.span)),
            TopDecl::Flow(f) if f.name == name => return Some(span_to_location(uri, &f.span)),
            TopDecl::Sink(f) if f.name == name => return Some(span_to_location(uri, &f.span)),
            TopDecl::Source(f) if f.name == name => return Some(span_to_location(uri, &f.span)),
            _ => {}
        }
    }
    None
}

fn find_module_func(
    qualified_name: &str,
    module: &crate::ast::ModuleAst,
    uri: &Uri,
) -> Option<Location> {
    let parts: Vec<&str> = qualified_name.splitn(2, '.').collect();
    if parts.len() != 2 {
        return None;
    }
    let module_name = parts[0];
    let func_name = parts[1];

    // Find the uses declaration that binds this module_name
    let uses_path = module.decls.iter().find_map(|d| {
        if let TopDecl::Uses(u) = d {
            if u.name == module_name {
                Some(u.path.clone())
            } else {
                None
            }
        } else {
            None
        }
    });
    let Some(uses_path) = uses_path else {
        return None;
    };

    let file_path = uri_to_file_path(uri)?;
    let dir = file_path.parent()?;
    let resolved = dir.join(&uses_path);

    if resolved.is_dir() {
        let func_file = resolved.join(format!("{func_name}.fa"));
        if func_file.exists() {
            let target_uri = file_path_to_uri(&func_file)?;
            return Some(Location {
                uri: target_uri,
                range: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 0,
                    },
                },
            });
        }
    }

    None
}

fn find_uses_module(name: &str, module: &crate::ast::ModuleAst, uri: &Uri) -> Option<Location> {
    // Find the uses declaration that binds this name
    let uses_path = module.decls.iter().find_map(|d| {
        if let TopDecl::Uses(u) = d {
            if u.name == name {
                Some(u.path.clone())
            } else {
                None
            }
        } else {
            None
        }
    });
    let Some(uses_path) = uses_path else {
        return None;
    };

    let file_path = uri_to_file_path(uri)?;
    let dir = file_path.parent()?;
    let resolved = dir.join(&uses_path);

    if resolved.is_file() {
        let target_uri = file_path_to_uri(&resolved)?;
        return Some(Location {
            uri: target_uri,
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 0,
                },
            },
        });
    }

    if resolved.is_dir() {
        let main_fa = resolved.join("main.fa");
        let target = if main_fa.exists() {
            main_fa
        } else {
            first_fa_file(&resolved)?
        };
        let target_uri = file_path_to_uri(&target)?;
        return Some(Location {
            uri: target_uri,
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 0,
                },
            },
        });
    }

    None
}

fn first_fa_file(dir: &PathBuf) -> Option<PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|s| s.to_str())
                .map_or(false, |ext| ext == "fa")
        })
        .collect();
    entries.sort_by_key(|e| e.file_name());
    entries.first().map(|e| e.path())
}

fn span_to_location(uri: &Uri, span: &crate::ast::Span) -> Location {
    let line = if span.line > 0 { span.line - 1 } else { 0 };
    let col = if span.col > 0 { span.col - 1 } else { 0 };
    Location {
        uri: uri.clone(),
        range: Range {
            start: Position {
                line: line as u32,
                character: col as u32,
            },
            end: Position {
                line: line as u32,
                character: col as u32,
            },
        },
    }
}
