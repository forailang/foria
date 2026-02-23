use crate::ast::{ConstraintValue, DocsDecl, ModuleAst, TopDecl, TypeKind};
use crate::parser;
use crate::stdlib_docs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

// --- Output types ---

#[derive(Debug, Clone, Serialize)]
pub struct DocsArtifact {
    pub dataflow_doc: String,
    pub modules: Vec<ModuleDoc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModuleDoc {
    pub file: String,
    pub uses: Vec<String>,
    pub symbols: SymbolsDoc,
}

#[derive(Debug, Clone, Serialize)]
pub struct SymbolsDoc {
    pub flows: Vec<FlowDoc>,
    pub types: Vec<TypeDoc>,
    pub enums: Vec<EnumDoc>,
    pub tests: Vec<TestDoc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FlowDoc {
    pub name: String,
    pub kind: String,
    pub takes: Vec<PortDoc>,
    pub emits: Vec<PortDoc>,
    pub fails: Vec<PortDoc>,
    pub docs: Option<String>,
    pub span: SpanDoc,
}

#[derive(Debug, Clone, Serialize)]
pub struct PortDoc {
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind")]
pub enum TypeDoc {
    #[serde(rename = "struct")]
    Struct {
        name: String,
        open: bool,
        fields: Vec<FieldDoc>,
        docs: Option<String>,
        span: SpanDoc,
    },
    #[serde(rename = "scalar")]
    Scalar {
        name: String,
        open: bool,
        base_type: String,
        constraints: Vec<ConstraintDoc>,
        docs: Option<String>,
        span: SpanDoc,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct FieldDoc {
    pub name: String,
    #[serde(rename = "type")]
    pub type_ref: String,
    pub constraints: Vec<ConstraintDoc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConstraintDoc {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnumDoc {
    pub name: String,
    pub open: bool,
    pub variants: Vec<String>,
    pub docs: Option<String>,
    pub span: SpanDoc,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestDoc {
    pub name: String,
    pub docs: Option<String>,
    pub tests_flow: Option<String>,
    pub span: SpanDoc,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpanDoc {
    pub line: usize,
    pub col: usize,
}

// --- Stdlib doc types ---

#[derive(Debug, Clone, Serialize)]
pub struct OpArgDoc {
    pub position: usize,
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpReturnDoc {
    #[serde(rename = "type")]
    pub type_name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StdlibOpDoc {
    pub name: String,
    pub full_name: String,
    pub summary: String,
    pub args: Vec<OpArgDoc>,
    pub returns: OpReturnDoc,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StdlibNamespaceDoc {
    pub namespace: String,
    pub summary: String,
    pub ops: Vec<StdlibOpDoc>,
}

// --- Index entry ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub name: String,
    pub kind: String,
    pub module: String,
    pub source: String,
    pub summary: String,
    pub detail_file: String,
}

// --- Extraction logic ---

fn constraint_to_doc(c: &crate::ast::TypeConstraint) -> ConstraintDoc {
    let value = match &c.value {
        ConstraintValue::Bool(b) => b.to_string(),
        ConstraintValue::Number(n) => n.to_string(),
        ConstraintValue::Regex(r) => format!("/{r}/"),
        ConstraintValue::Symbol(s) => s.clone(),
    };
    ConstraintDoc {
        key: c.key.clone(),
        value,
    }
}

fn infer_tested_flow(body: &str, flow_names: &[String]) -> Option<String> {
    for name in flow_names {
        let pattern = format!("{name}(");
        if body.contains(&pattern) {
            return Some(name.clone());
        }
    }
    None
}

pub fn extract_module_doc(module: &ModuleAst, file_path: &str) -> ModuleDoc {
    let mut docs_map = HashMap::<String, String>::new();
    let mut docs_decls = HashMap::<String, &DocsDecl>::new();
    let mut uses = Vec::new();
    let mut flows = Vec::new();
    let mut types = Vec::new();
    let mut enums = Vec::new();
    let mut tests = Vec::new();
    let mut flow_names = Vec::new();

    // First pass: collect docs and flow/func names
    for decl in &module.decls {
        match decl {
            TopDecl::Docs(d) => {
                docs_map.insert(d.name.clone(), d.markdown.trim().to_string());
                docs_decls.insert(d.name.clone(), d);
            }
            TopDecl::Flow(f) => {
                flow_names.push(f.name.clone());
            }
            TopDecl::Func(f) | TopDecl::Sink(f) | TopDecl::Source(f) => {
                flow_names.push(f.name.clone());
            }
            _ => {}
        }
    }

    // Second pass: extract all symbols
    for decl in &module.decls {
        match decl {
            TopDecl::Uses(u) => {
                uses.push(u.name.clone());
            }
            TopDecl::Flow(f) => {
                flows.push(FlowDoc {
                    name: f.name.clone(),
                    kind: "flow".to_string(),
                    takes: f
                        .takes
                        .iter()
                        .map(|t| PortDoc {
                            name: t.name.clone(),
                            type_name: t.type_name.clone(),
                        })
                        .collect(),
                    emits: f
                        .emits
                        .iter()
                        .map(|p| PortDoc {
                            name: p.name.clone(),
                            type_name: p.type_name.clone(),
                        })
                        .collect(),
                    fails: f
                        .fails
                        .iter()
                        .map(|p| PortDoc {
                            name: p.name.clone(),
                            type_name: p.type_name.clone(),
                        })
                        .collect(),
                    docs: docs_map.get(&f.name).cloned(),
                    span: SpanDoc {
                        line: f.span.line,
                        col: f.span.col,
                    },
                });
            }
            TopDecl::Func(f) => {
                let (emits, fails) = if let Some(ref ret_type) = f.return_type {
                    let fail_t = f.fail_type.as_deref().unwrap_or("unknown");
                    (
                        vec![PortDoc {
                            name: "_return".to_string(),
                            type_name: ret_type.clone(),
                        }],
                        vec![PortDoc {
                            name: "_fail".to_string(),
                            type_name: fail_t.to_string(),
                        }],
                    )
                } else {
                    (
                        f.emits
                            .iter()
                            .map(|p| PortDoc {
                                name: p.name.clone(),
                                type_name: p.type_name.clone(),
                            })
                            .collect(),
                        f.fails
                            .iter()
                            .map(|p| PortDoc {
                                name: p.name.clone(),
                                type_name: p.type_name.clone(),
                            })
                            .collect(),
                    )
                };
                flows.push(FlowDoc {
                    name: f.name.clone(),
                    kind: "func".to_string(),
                    takes: f
                        .takes
                        .iter()
                        .map(|t| PortDoc {
                            name: t.name.clone(),
                            type_name: t.type_name.clone(),
                        })
                        .collect(),
                    emits,
                    fails,
                    docs: docs_map.get(&f.name).cloned(),
                    span: SpanDoc {
                        line: f.span.line,
                        col: f.span.col,
                    },
                });
            }
            TopDecl::Sink(f) => {
                flows.push(FlowDoc {
                    name: f.name.clone(),
                    kind: "sink".to_string(),
                    takes: f
                        .takes
                        .iter()
                        .map(|t| PortDoc {
                            name: t.name.clone(),
                            type_name: t.type_name.clone(),
                        })
                        .collect(),
                    emits: f
                        .emits
                        .iter()
                        .map(|p| PortDoc {
                            name: p.name.clone(),
                            type_name: p.type_name.clone(),
                        })
                        .collect(),
                    fails: f
                        .fails
                        .iter()
                        .map(|p| PortDoc {
                            name: p.name.clone(),
                            type_name: p.type_name.clone(),
                        })
                        .collect(),
                    docs: docs_map.get(&f.name).cloned(),
                    span: SpanDoc {
                        line: f.span.line,
                        col: f.span.col,
                    },
                });
            }
            TopDecl::Source(f) => {
                let (emits, fails) = if let Some(ref ret_type) = f.return_type {
                    let fail_t = f.fail_type.as_deref().unwrap_or("unknown");
                    (
                        vec![PortDoc {
                            name: "_return".to_string(),
                            type_name: ret_type.clone(),
                        }],
                        vec![PortDoc {
                            name: "_fail".to_string(),
                            type_name: fail_t.to_string(),
                        }],
                    )
                } else {
                    (
                        f.emits
                            .iter()
                            .map(|p| PortDoc {
                                name: p.name.clone(),
                                type_name: p.type_name.clone(),
                            })
                            .collect(),
                        f.fails
                            .iter()
                            .map(|p| PortDoc {
                                name: p.name.clone(),
                                type_name: p.type_name.clone(),
                            })
                            .collect(),
                    )
                };
                flows.push(FlowDoc {
                    name: f.name.clone(),
                    kind: "source".to_string(),
                    takes: f
                        .takes
                        .iter()
                        .map(|t| PortDoc {
                            name: t.name.clone(),
                            type_name: t.type_name.clone(),
                        })
                        .collect(),
                    emits,
                    fails,
                    docs: docs_map.get(&f.name).cloned(),
                    span: SpanDoc {
                        line: f.span.line,
                        col: f.span.col,
                    },
                });
            }
            TopDecl::Type(t) => match &t.kind {
                TypeKind::Struct { fields } => {
                    let field_docs_map: HashMap<&str, &str> = docs_decls
                        .get(&t.name)
                        .map(|dd| {
                            dd.field_docs
                                .iter()
                                .map(|fd| (fd.name.as_str(), fd.markdown.as_str()))
                                .collect()
                        })
                        .unwrap_or_default();
                    types.push(TypeDoc::Struct {
                        name: t.name.clone(),
                        open: t.open,
                        fields: fields
                            .iter()
                            .map(|f| FieldDoc {
                                name: f.name.clone(),
                                type_ref: f.type_ref.clone(),
                                constraints: f.constraints.iter().map(constraint_to_doc).collect(),
                                docs: field_docs_map.get(f.name.as_str()).map(|s| s.to_string()),
                            })
                            .collect(),
                        docs: docs_map.get(&t.name).cloned(),
                        span: SpanDoc {
                            line: t.span.line,
                            col: t.span.col,
                        },
                    });
                }
                TypeKind::Scalar {
                    base_type,
                    constraints,
                } => {
                    types.push(TypeDoc::Scalar {
                        name: t.name.clone(),
                        open: t.open,
                        base_type: base_type.clone(),
                        constraints: constraints.iter().map(constraint_to_doc).collect(),
                        docs: docs_map.get(&t.name).cloned(),
                        span: SpanDoc {
                            line: t.span.line,
                            col: t.span.col,
                        },
                    });
                }
            },
            TopDecl::Enum(e) => {
                enums.push(EnumDoc {
                    name: e.name.clone(),
                    open: e.open,
                    variants: e.variants.clone(),
                    docs: docs_map.get(&e.name).cloned(),
                    span: SpanDoc {
                        line: e.span.line,
                        col: e.span.col,
                    },
                });
            }
            TopDecl::Test(t) => {
                tests.push(TestDoc {
                    name: t.name.clone(),
                    docs: docs_map.get(&t.name).cloned(),
                    tests_flow: infer_tested_flow(&t.body_text, &flow_names),
                    span: SpanDoc {
                        line: t.span.line,
                        col: t.span.col,
                    },
                });
            }
            TopDecl::Docs(_) => {}
        }
    }

    ModuleDoc {
        file: file_path.to_string(),
        uses,
        symbols: SymbolsDoc {
            flows,
            types,
            enums,
            tests,
        },
    }
}

// --- File collection ---

fn collect_fa_files_recursive(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        if path.extension().and_then(|s| s.to_str()) == Some("fa") {
            out.push(path.to_path_buf());
        }
        return;
    }

    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    let mut children: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    children.sort();
    for child in children {
        if child.is_dir() {
            collect_fa_files_recursive(&child, out);
        } else if child.extension().and_then(|s| s.to_str()) == Some("fa") {
            out.push(child);
        }
    }
}

// --- Top-level entry point ---

pub fn generate_docs_at_path(path: &Path, base_dir: &Path) -> Result<DocsArtifact, String> {
    let mut files = Vec::new();
    collect_fa_files_recursive(path, &mut files);

    if files.is_empty() {
        return Err(format!("No .fa files found at {}", path.display()));
    }

    let mut modules = Vec::new();
    for file in &files {
        let src = fs::read_to_string(file)
            .map_err(|e| format!("Failed to read {}: {e}", file.display()))?;

        let module = parser::parse_module_v1(&src).map_err(|e| {
            format!(
                "{}:{}:{} {}",
                file.display(),
                e.span.line,
                e.span.col,
                e.message
            )
        })?;

        let rel = file
            .strip_prefix(base_dir)
            .unwrap_or(file)
            .to_string_lossy()
            .to_string();

        modules.push(extract_module_doc(&module, &rel));
    }

    Ok(DocsArtifact {
        dataflow_doc: "0.1".to_string(),
        modules,
    })
}

// --- Docs folder generation ---

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    fs::write(path, format!("{json}\n"))
        .map_err(|e| format!("Failed to write {}: {e}", path.display()))
}

fn ensure_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|e| format!("Failed to create {}: {e}", path.display()))
}

pub fn generate_docs_folder(
    project_root: &Path,
    entry_path: &Path,
    module: &ModuleAst,
) -> Result<(), String> {
    let docs_dir = project_root.join("docs");
    let project_dir = docs_dir.join("project");
    let stdlib_dir = docs_dir.join("stdlib");
    let libs_dir = docs_dir.join("libs");

    ensure_dir(&project_dir)?;
    ensure_dir(&stdlib_dir)?;
    ensure_dir(&libs_dir)?;

    let mut index: Vec<IndexEntry> = Vec::new();

    // --- Project docs ---
    let mut fa_files = Vec::new();
    collect_fa_files_recursive(project_root, &mut fa_files);
    // Exclude lib subdirs (use targets) so we only get project-level files
    let lib_dirs: Vec<PathBuf> = module
        .decls
        .iter()
        .filter_map(|d| {
            if let TopDecl::Uses(u) = d {
                let resolved = project_root.join(&u.path);
                if resolved.is_dir() { Some(resolved) } else { None }
            } else {
                None
            }
        })
        .collect();
    fa_files.retain(|f| !lib_dirs.iter().any(|ld| f.starts_with(ld)));

    for file in &fa_files {
        let src = match fs::read_to_string(file) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let parsed = match parser::parse_module_v1(&src) {
            Ok(m) => m,
            Err(_) => continue,
        };

        let rel = file
            .strip_prefix(project_root)
            .unwrap_or(file)
            .to_string_lossy()
            .to_string();
        let mod_doc = extract_module_doc(&parsed, &rel);

        let stem = file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");
        let detail_file = format!("project/{stem}.json");
        write_json(&project_dir.join(format!("{stem}.json")), &mod_doc)?;

        for flow in &mod_doc.symbols.flows {
            let summary = flow.docs.clone().unwrap_or_default();
            let first_line = summary.lines().next().unwrap_or("").to_string();
            index.push(IndexEntry {
                name: flow.name.clone(),
                kind: flow.kind.clone(),
                module: rel.clone(),
                source: "project".to_string(),
                summary: first_line,
                detail_file: detail_file.clone(),
            });
        }
        for ty in &mod_doc.symbols.types {
            let (name, docs) = match ty {
                TypeDoc::Struct { name, docs, .. } => (name.clone(), docs.clone()),
                TypeDoc::Scalar { name, docs, .. } => (name.clone(), docs.clone()),
            };
            let first_line = docs
                .as_deref()
                .and_then(|d| d.lines().next())
                .unwrap_or("")
                .to_string();
            index.push(IndexEntry {
                name,
                kind: "type".to_string(),
                module: rel.clone(),
                source: "project".to_string(),
                summary: first_line,
                detail_file: detail_file.clone(),
            });
        }
        for en in &mod_doc.symbols.enums {
            let first_line = en
                .docs
                .as_deref()
                .and_then(|d| d.lines().next())
                .unwrap_or("")
                .to_string();
            index.push(IndexEntry {
                name: en.name.clone(),
                kind: "enum".to_string(),
                module: rel.clone(),
                source: "project".to_string(),
                summary: first_line,
                detail_file: detail_file.clone(),
            });
        }
    }

    // --- Stdlib docs ---
    let namespaces = stdlib_docs::all_stdlib_docs();
    for ns_doc in &namespaces {
        let ns_file = format!("{}.json", ns_doc.namespace);
        write_json(&stdlib_dir.join(&ns_file), ns_doc)?;

        let detail_file = format!("stdlib/{ns_file}");
        for op_doc in &ns_doc.ops {
            index.push(IndexEntry {
                name: op_doc.full_name.clone(),
                kind: "op".to_string(),
                module: ns_doc.namespace.clone(),
                source: "stdlib".to_string(),
                summary: op_doc.summary.clone(),
                detail_file: detail_file.clone(),
            });
        }
    }

    // --- Lib docs ---
    let base_dir = entry_path.parent().unwrap_or(Path::new("."));
    for decl in &module.decls {
        if let TopDecl::Uses(u) = decl {
            let module_dir = base_dir.join(&u.path);
            if !module_dir.is_dir() {
                continue;
            }
            let lib_out = libs_dir.join(&u.name);
            ensure_dir(&lib_out)?;

            let mut lib_files = Vec::new();
            collect_fa_files_recursive(&module_dir, &mut lib_files);

            for file in &lib_files {
                let src = match fs::read_to_string(file) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let parsed = match parser::parse_module_v1(&src) {
                    Ok(m) => m,
                    Err(_) => continue,
                };

                let rel = file
                    .strip_prefix(&module_dir)
                    .unwrap_or(file)
                    .to_string_lossy()
                    .to_string();
                let mod_doc = extract_module_doc(&parsed, &rel);

                let stem = file
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");
                let detail_file = format!("libs/{}/{stem}.json", u.name);
                write_json(&lib_out.join(format!("{stem}.json")), &mod_doc)?;

                for flow in &mod_doc.symbols.flows {
                    let summary = flow.docs.clone().unwrap_or_default();
                    let first_line = summary.lines().next().unwrap_or("").to_string();
                    index.push(IndexEntry {
                        name: format!("{}.{}", u.name, flow.name),
                        kind: flow.kind.clone(),
                        module: format!("{}/{rel}", u.name),
                        source: "lib".to_string(),
                        summary: first_line,
                        detail_file: detail_file.clone(),
                    });
                }
                for ty in &mod_doc.symbols.types {
                    let (name, docs) = match ty {
                        TypeDoc::Struct { name, docs, .. } => (name.clone(), docs.clone()),
                        TypeDoc::Scalar { name, docs, .. } => (name.clone(), docs.clone()),
                    };
                    let first_line = docs
                        .as_deref()
                        .and_then(|d| d.lines().next())
                        .unwrap_or("")
                        .to_string();
                    index.push(IndexEntry {
                        name,
                        kind: "type".to_string(),
                        module: format!("{}/{rel}", u.name),
                        source: "lib".to_string(),
                        summary: first_line,
                        detail_file: detail_file.clone(),
                    });
                }
            }
        }
    }

    // --- Write index ---
    write_json(&docs_dir.join("index.json"), &index)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_single_flow_module() {
        let src = r#"
type HttpRequest
  path text
done

type HttpResponse
  status long
done

type AuthError
  status long
done

docs LoginFlow
  Authenticates a user.
done

func LoginFlow
  take request as HttpRequest
  emit response as HttpResponse
  fail error as AuthError
body
  params = http.extract_params(request)
  emit params
done

docs LoginFlowTest
  Tests the login flow.
done

test LoginFlowTest
  req = request("a@b.com", "pass")
  res = LoginFlow(req)
  must res.status == 200
done
"#;
        let module = parser::parse_module_v1(src).expect("parse should succeed");
        let doc = extract_module_doc(&module, "LoginFlow.fa");

        assert_eq!(doc.file, "LoginFlow.fa");
        assert_eq!(doc.symbols.flows.len(), 1);
        let flow = &doc.symbols.flows[0];
        assert_eq!(flow.name, "LoginFlow");
        assert_eq!(flow.takes.len(), 1);
        assert_eq!(flow.takes[0].name, "request");
        assert_eq!(flow.takes[0].type_name, "HttpRequest");
        assert_eq!(flow.emits[0].name, "response");
        assert_eq!(flow.fails[0].name, "error");
        assert_eq!(flow.docs.as_deref(), Some("Authenticates a user."));

        assert_eq!(doc.symbols.types.len(), 3);
        assert_eq!(doc.symbols.tests.len(), 1);
        let test = &doc.symbols.tests[0];
        assert_eq!(test.name, "LoginFlowTest");
        assert_eq!(test.docs.as_deref(), Some("Tests the login flow."));
        assert_eq!(test.tests_flow.as_deref(), Some("LoginFlow"));
    }

    #[test]
    fn extract_scalar_type_with_constraints() {
        let src = r#"
type Email as text :matches => /@/
type Password as text :min => 8
"#;
        let module = parser::parse_module_v1(src).expect("parse should succeed");
        let doc = extract_module_doc(&module, "scalars.fa");

        assert_eq!(doc.symbols.types.len(), 2);
        match &doc.symbols.types[0] {
            TypeDoc::Scalar {
                name,
                base_type,
                constraints,
                ..
            } => {
                assert_eq!(name, "Email");
                assert_eq!(base_type, "text");
                assert_eq!(constraints.len(), 1);
                assert_eq!(constraints[0].key, "matches");
                assert_eq!(constraints[0].value, "/@/");
            }
            other => panic!("Expected Scalar, got {other:?}"),
        }
        match &doc.symbols.types[1] {
            TypeDoc::Scalar {
                name,
                base_type,
                constraints,
                ..
            } => {
                assert_eq!(name, "Password");
                assert_eq!(base_type, "text");
                assert_eq!(constraints.len(), 1);
                assert_eq!(constraints[0].key, "min");
                assert_eq!(constraints[0].value, "8");
            }
            other => panic!("Expected Scalar, got {other:?}"),
        }
    }

    #[test]
    fn extract_enum_with_variants() {
        let src = r#"
enum Status
  Admin
  User
  Guest
done
"#;
        let module = parser::parse_module_v1(src).expect("parse should succeed");
        let doc = extract_module_doc(&module, "enums.fa");

        assert_eq!(doc.symbols.enums.len(), 1);
        let e = &doc.symbols.enums[0];
        assert_eq!(e.name, "Status");
        assert!(!e.open);
        assert_eq!(e.variants, vec!["Admin", "User", "Guest"]);
        assert!(e.docs.is_none());
    }

    #[test]
    fn docs_null_when_absent() {
        let src = r#"
type PrivateStuff
  value text
done
"#;
        let module = parser::parse_module_v1(src).expect("parse should succeed");
        let doc = extract_module_doc(&module, "private.fa");

        assert_eq!(doc.symbols.types.len(), 1);
        match &doc.symbols.types[0] {
            TypeDoc::Struct { docs, .. } => assert!(docs.is_none()),
            other => panic!("Expected Struct, got {other:?}"),
        }
    }

    #[test]
    fn infer_tested_flow_from_body() {
        let flows = vec!["LoginFlow".to_string(), "SignupFlow".to_string()];

        assert_eq!(
            infer_tested_flow("res = LoginFlow(req)", &flows),
            Some("LoginFlow".to_string())
        );
        assert_eq!(
            infer_tested_flow("err = trap LoginFlow(req)", &flows),
            Some("LoginFlow".to_string())
        );
        assert_eq!(infer_tested_flow("must x == 1", &flows), None);
    }

    #[test]
    fn uses_declarations_captured() {
        let src = r#"
use travel from "./travel"

type Req
  x text
done

type Res
  y text
done

type Err
  z text
done

docs MyFlow
  A flow.
done

func MyFlow
  take r as Req
  emit o as Res
  fail e as Err
body
  emit r
done
"#;
        let module = parser::parse_module_v1(src).expect("parse should succeed");
        let doc = extract_module_doc(&module, "test.fa");
        assert_eq!(doc.uses, vec!["travel"]);
    }

    #[test]
    fn generates_docs_for_examples_directory() {
        let examples_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        let artifact = generate_docs_at_path(&examples_dir, &examples_dir)
            .expect("doc generation should succeed");

        assert_eq!(artifact.dataflow_doc, "0.1");

        let file_names: Vec<&str> = artifact.modules.iter().map(|m| m.file.as_str()).collect();
        assert!(
            file_names.iter().any(|f| f.contains("Classify")),
            "should have Classify, got {file_names:?}"
        );
        assert!(
            file_names.iter().any(|f| f.contains("read-docs")),
            "should have read-docs subdir files, got {file_names:?}"
        );

        // All flows should have docs (all example flows have docs blocks)
        for m in &artifact.modules {
            for flow in &m.symbols.flows {
                assert!(
                    flow.docs.is_some(),
                    "flow {} in {} should have docs",
                    flow.name,
                    m.file
                );
            }
        }

        // read-docs/app/Start.fa should have use declarations
        let start = artifact
            .modules
            .iter()
            .find(|m| m.file.contains("read-docs") && m.file.contains("Start"))
            .expect("should have read-docs Start module");
        assert!(
            !start.uses.is_empty(),
            "Start.fa should have use declarations"
        );
    }

    #[test]
    fn kind_field_correct_for_func_flow_sink() {
        let src = r#"
type Req
  x text
done

type Res
  y text
done

type Err
  z text
done

docs MyFunc
  A func.
done

func MyFunc
  take r as Req
  emit o as Res
  fail e as Err
body
  emit r
done

docs MySink
  A sink.
done

sink MySink
  take r as Req
  emit o as Res
  fail e as Err
body
  emit r
done

docs MyFlow
  A flow.
done

flow MyFlow
  take r as Req
  emit o as Res
  fail e as Err
body
  step MyFunc
    r => r
  then
    emit o => result
  done
done
"#;
        let module = parser::parse_module_v1(src).expect("parse should succeed");
        let doc = extract_module_doc(&module, "test.fa");

        assert_eq!(doc.symbols.flows.len(), 3);

        let func = doc
            .symbols
            .flows
            .iter()
            .find(|f| f.name == "MyFunc")
            .unwrap();
        assert_eq!(func.kind, "func");

        let sink = doc
            .symbols
            .flows
            .iter()
            .find(|f| f.name == "MySink")
            .unwrap();
        assert_eq!(sink.kind, "sink");

        let flow = doc
            .symbols
            .flows
            .iter()
            .find(|f| f.name == "MyFlow")
            .unwrap();
        assert_eq!(flow.kind, "flow");
    }

    #[test]
    fn index_entry_serializes_correctly() {
        let entry = IndexEntry {
            name: "str.len".to_string(),
            kind: "op".to_string(),
            module: "str".to_string(),
            source: "stdlib".to_string(),
            summary: "Returns the length of a string".to_string(),
            detail_file: "stdlib/str.json".to_string(),
        };
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["name"], "str.len");
        assert_eq!(json["kind"], "op");
        assert_eq!(json["module"], "str");
        assert_eq!(json["source"], "stdlib");
        assert_eq!(json["summary"], "Returns the length of a string");
        assert_eq!(json["detail_file"], "stdlib/str.json");
    }

    #[test]
    fn generate_docs_folder_end_to_end() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let read_docs_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/read-docs");

        // Use a temp dir to avoid polluting the example
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let tmp = std::env::temp_dir().join(format!("forai_docs_test_{stamp}"));

        // Recursively copy read-docs to temp
        fn copy_dir_recursive(src: &Path, dst: &Path) {
            fs::create_dir_all(dst).unwrap();
            for entry in fs::read_dir(src).unwrap() {
                let entry = entry.unwrap();
                let s = entry.path();
                let d = dst.join(entry.file_name());
                if s.is_dir() {
                    // Skip existing docs/ directories
                    if entry.file_name() == "docs" {
                        continue;
                    }
                    copy_dir_recursive(&s, &d);
                } else {
                    fs::copy(&s, &d).unwrap();
                }
            }
        }
        copy_dir_recursive(&read_docs_dir, &tmp);

        let tmp_entry = tmp.join("main.fa");
        let text = fs::read_to_string(&tmp_entry).unwrap();
        let module = parser::parse_module_v1(&text).unwrap();

        generate_docs_folder(&tmp, &tmp_entry, &module)
            .expect("docs folder generation should succeed");

        // Verify structure
        assert!(
            tmp.join("docs/index.json").exists(),
            "index.json should exist"
        );
        assert!(
            tmp.join("docs/project/main.json").exists(),
            "project/main.json should exist"
        );
        assert!(
            tmp.join("docs/stdlib/str.json").exists(),
            "stdlib/str.json should exist"
        );

        // Verify index has entries from project and stdlib sources
        let index_text = fs::read_to_string(tmp.join("docs/index.json")).unwrap();
        let index: Vec<IndexEntry> = serde_json::from_str(&index_text).unwrap();

        let has_project = index.iter().any(|e| e.source == "project");
        let has_stdlib = index.iter().any(|e| e.source == "stdlib");

        assert!(has_project, "index should have project entries");
        assert!(has_stdlib, "index should have stdlib entries");

        // Cleanup
        let _ = fs::remove_dir_all(&tmp);
    }
}
