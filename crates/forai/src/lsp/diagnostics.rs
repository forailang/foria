use lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range, Uri};
use regex::Regex;

use crate::parser;
use crate::sema;
use crate::types::TypeRegistry;

use super::document::Document;

pub fn compute_diagnostics(doc: &Document, uri: &Uri) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    let module = match parser::parse_module_v1(&doc.text) {
        Ok(m) => m,
        Err(e) => {
            let line = if e.span.line > 0 { e.span.line - 1 } else { 0 };
            let col = if e.span.col > 0 { e.span.col - 1 } else { 0 };
            diagnostics.push(Diagnostic {
                range: Range {
                    start: Position {
                        line: line as u32,
                        character: col as u32,
                    },
                    end: Position {
                        line: line as u32,
                        character: (col + 1) as u32,
                    },
                },
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("forai".to_string()),
                message: e.message,
                ..Default::default()
            });
            return diagnostics;
        }
    };

    let filename = extract_filename(uri);

    if let Err(errors) = sema::validate_module(&module, filename.as_deref()) {
        parse_error_strings(&errors, &mut diagnostics);
    }

    if let Err(errors) = TypeRegistry::from_module(&module) {
        parse_error_strings(&errors, &mut diagnostics);
    }

    diagnostics
}

fn extract_filename(uri: &Uri) -> Option<String> {
    let s = uri.as_str();
    // Extract filename from file:///path/to/Foo.fa
    s.rsplit('/').next().map(|s| s.to_string())
}

fn parse_error_strings(errors: &[String], diagnostics: &mut Vec<Diagnostic>) {
    let re = Regex::new(r"^(\d+):(\d+)\s+(.*)$").unwrap();

    for err in errors {
        if let Some(caps) = re.captures(err) {
            let line: u32 = caps[1].parse().unwrap_or(1);
            let col: u32 = caps[2].parse().unwrap_or(1);
            let message = caps[3].to_string();
            let line = if line > 0 { line - 1 } else { 0 };
            let col = if col > 0 { col - 1 } else { 0 };
            diagnostics.push(Diagnostic {
                range: Range {
                    start: Position {
                        line,
                        character: col,
                    },
                    end: Position {
                        line,
                        character: col + 1,
                    },
                },
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("forai".to_string()),
                message,
                ..Default::default()
            });
        } else {
            diagnostics.push(Diagnostic {
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
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("forai".to_string()),
                message: err.clone(),
                ..Default::default()
            });
        }
    }
}
