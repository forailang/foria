use crate::mcp::protocol::{CallToolResult, ToolInfo};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub fn tool_definitions() -> Vec<ToolInfo> {
    vec![
        ToolInfo {
            name: "forai_check".into(),
            description: "Check a .fa file or directory for syntax, semantic, IR lowering, and typecheck errors. When given a .fa file, also checks all transitively imported modules. Returns structured diagnostics or \"ok\".".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to a .fa file or directory to check" }
                },
                "required": ["path"]
            }),
        },
        ToolInfo {
            name: "forai_stdlib_ref".into(),
            description: "Search the forai standard library reference. Returns matching op signatures and descriptions.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Optional namespace filter (e.g. \"str\", \"http\", \"db\"). Omit to list all namespaces." }
                }
            }),
        },
        ToolInfo {
            name: "forai_doc_search".into(),
            description: "Generate and search module documentation for a .fa project. Returns func/type/enum docs.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to a .fa file or directory" },
                    "query": { "type": "string", "description": "Optional search term to filter results" }
                },
                "required": ["path"]
            }),
        },
        ToolInfo {
            name: "forai_build".into(),
            description: "Build a forai project (compile, test, generate artifacts). Requires forai.json in the project.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "dir": { "type": "string", "description": "Project directory containing forai.json" }
                },
                "required": ["dir"]
            }),
        },
        ToolInfo {
            name: "forai_test".into(),
            description: "Run all test blocks in a forai project. Scans the project root recursively and runs every test block found. Omit path to test the whole project from the current working directory.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to a .fa file or project directory. Defaults to \".\" (whole project from CWD)." }
                }
            }),
        },
        ToolInfo {
            name: "forai_run".into(),
            description: "Compile and execute a .fa flow. Returns execution outputs and trace summary.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "Path to the main .fa source file" },
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional arguments passed as flow inputs"
                    },
                    "timeout": { "type": "number", "description": "Timeout in seconds (default: 120). If the flow blocks longer than this — e.g. waiting for user input via term.prompt — execution is cancelled and an error is returned." }
                },
                "required": ["source"]
            }),
        },
        ToolInfo {
            name: "forai_fmt".into(),
            description: "Format .fa source files. Can check formatting without modifying files.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to a .fa file or directory" },
                    "check": { "type": "boolean", "description": "If true, only check formatting without modifying files" }
                },
                "required": ["path"]
            }),
        },
        ToolInfo {
            name: "forai_flow_graph".into(),
            description: "Compile a .fa file and return the IR flow graph (nodes, edges, inputs, outputs) as JSON.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "Path to a .fa source file" }
                },
                "required": ["source"]
            }),
        },
        ToolInfo {
            name: "forai_debug_snapshot".into(),
            description: "Compile and execute a .fa flow with full tracing. Returns variable values at each step.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "Path to the main .fa source file" },
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional arguments passed as flow inputs"
                    },
                    "timeout": { "type": "number", "description": "Timeout in seconds (default: 120). If the flow blocks longer than this — e.g. waiting for user input via term.prompt — execution is cancelled and an error is returned." }
                },
                "required": ["source"]
            }),
        },
        ToolInfo {
            name: "forai_impact".into(),
            description: "Analyze what depends on a .fa file — importers, callers, and tests. Useful for understanding change impact.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to a .fa file to analyze" }
                },
                "required": ["path"]
            }),
        },
        ToolInfo {
            name: "forai_example_search".into(),
            description: "Search curated forai example projects by keyword, category, or feature. Returns complete runnable examples with all source files. Queries the forailang.com API so examples stay up to date without CLI updates.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search term to match against title, description, keywords, and features (e.g. \"http server\", \"sqlite\", \"term.print\")"
                    },
                    "category": {
                        "type": "string",
                        "enum": ["cli", "web", "browser", "ffi", "database", "library", "language"],
                        "description": "Filter by example category"
                    },
                    "difficulty": {
                        "type": "string",
                        "enum": ["beginner", "intermediate", "advanced"],
                        "description": "Filter by difficulty level"
                    }
                }
            }),
        },
    ]
}

pub async fn call_tool(name: &str, args: &Value) -> CallToolResult {
    match name {
        "forai_check" => tool_check(args).await,
        "forai_stdlib_ref" => tool_stdlib_ref(args),
        "forai_doc_search" => tool_doc_search(args),
        "forai_build" => tool_build(args).await,
        "forai_test" => tool_test(args).await,
        "forai_run" => tool_run(args).await,
        "forai_fmt" => tool_fmt(args),
        "forai_flow_graph" => tool_flow_graph(args),
        "forai_debug_snapshot" => tool_debug_snapshot(args).await,
        "forai_impact" => tool_impact(args),
        "forai_example_search" => tool_example_search(args).await,
        _ => CallToolResult::error(format!("Unknown tool: {name}")),
    }
}

fn get_string_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str())
}

fn resolve_path(raw: &str) -> PathBuf {
    let p = PathBuf::from(raw);
    if p.is_relative() {
        std::env::current_dir().unwrap_or_default().join(p)
    } else {
        p
    }
}

/// BFS over `use` declarations starting from the already-collected `files`, adding any
/// transitively-imported `.fa` files that have not been visited yet.
pub fn expand_with_imports(files: &mut Vec<PathBuf>) {
    use std::collections::HashSet;
    let mut visited: HashSet<PathBuf> = files.iter().cloned().collect();
    let mut queue: Vec<PathBuf> = files.clone();
    while !queue.is_empty() {
        let file = queue.remove(0);
        let text = match std::fs::read_to_string(&file) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let module = match crate::parser::parse_module_v1(&text) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let base = file.parent().unwrap_or(Path::new("."));
        for decl in &module.decls {
            if let crate::ast::TopDecl::Uses(u) = decl {
                for imp in collect_fa_files(&base.join(&u.path)) {
                    if visited.insert(imp.clone()) {
                        files.push(imp.clone());
                        queue.push(imp);
                    }
                }
            }
        }
    }
}

async fn tool_check(args: &Value) -> CallToolResult {
    let Some(raw_path) = get_string_arg(args, "path") else {
        return CallToolResult::error("Missing required argument: path".into());
    };
    let path = resolve_path(raw_path);

    let mut files = collect_fa_files(&path);
    if path.is_file() {
        expand_with_imports(&mut files);
    }
    if files.is_empty() {
        return CallToolResult::error(format!("No .fa files found at {}", path.display()));
    }

    // Format first, then check
    let fmt_changed = match crate::formatter::fmt_path(&path, false) {
        Ok((changed, _)) => changed,
        Err(_) => vec![],
    };
    if !fmt_changed.is_empty() {
        // Re-collect files after formatting
        files = collect_fa_files(&path);
        if path.is_file() {
            expand_with_imports(&mut files);
        }
    }

    let mut diagnostics = Vec::new();
    let mut warnings = Vec::new();
    let mut ok_count = 0;

    for file in &files {
        let text = match std::fs::read_to_string(file) {
            Ok(t) => t,
            Err(e) => {
                diagnostics.push(format!("{}:0:0 failed to read: {e}", file.display()));
                continue;
            }
        };

        // Parse
        let module = match crate::parser::parse_module_v1(&text) {
            Ok(m) => m,
            Err(e) => {
                diagnostics.push(format!(
                    "{}:{}:{} {}",
                    file.display(),
                    e.span.line,
                    e.span.col,
                    e.message
                ));
                continue;
            }
        };

        // Semantic validation
        let filename = file
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());
        if let Err(errors) = crate::sema::validate_module(&module, filename.as_deref()) {
            for e in errors {
                diagnostics.push(format!("{}:{e}", file.display()));
            }
            continue;
        }
        for w in crate::sema::test_call_warnings(&module) {
            warnings.push(format!("{}:{w}", file.display()));
        }

        // Type registry
        let type_registry = match crate::types::TypeRegistry::from_module(&module) {
            Ok(r) => r,
            Err(errors) => {
                for e in errors {
                    diagnostics.push(format!("{}:{e}", file.display()));
                }
                continue;
            }
        };

        // Per-file compilation: IR lowering + typecheck for each callable
        let mut file_ok = true;
        for decl in &module.decls {
            match decl {
                crate::ast::TopDecl::Func(f)
                | crate::ast::TopDecl::Sink(f)
                | crate::ast::TopDecl::Source(f) => {
                    match crate::parser::parse_runtime_func_decl_v1(f) {
                        Err(e) => {
                            diagnostics.push(format!(
                                "{}: func `{}` parse error: {e}",
                                file.display(),
                                f.name
                            ));
                            file_ok = false;
                        }
                        Ok(flow) => {
                            if let Err(e) = crate::typecheck::typecheck_func(
                                &f.name,
                                &f.takes,
                                &flow.body,
                                &f.emits,
                                &f.fails,
                                &type_registry,
                            ) {
                                diagnostics.push(format!("{}:{e}", file.display()));
                                file_ok = false;
                            }
                            if let Err(e) = forai_core::ir::lower_to_ir(&flow) {
                                diagnostics.push(format!(
                                    "{}: func `{}` IR error: {e}",
                                    file.display(),
                                    f.name
                                ));
                                file_ok = false;
                            }
                        }
                    }
                }
                crate::ast::TopDecl::Flow(f) => match crate::parser::parse_flow_graph_decl_v1(f) {
                    Err(e) => {
                        diagnostics.push(format!(
                            "{}: flow `{}` parse error: {e}",
                            file.display(),
                            f.name
                        ));
                        file_ok = false;
                    }
                    Ok(graph) => match crate::parser::lower_flow_graph_to_flow(&graph) {
                        Err(e) => {
                            diagnostics.push(format!(
                                "{}: flow `{}` lower error: {e}",
                                file.display(),
                                f.name
                            ));
                            file_ok = false;
                        }
                        Ok(flow) => {
                            if let Err(e) = forai_core::ir::lower_to_ir(&flow) {
                                diagnostics.push(format!(
                                    "{}: flow `{}` IR error: {e}",
                                    file.display(),
                                    f.name
                                ));
                                file_ok = false;
                            }
                        }
                    },
                },
                _ => {}
            }
        }

        if file_ok {
            ok_count += 1;
        }
    }

    if diagnostics.is_empty() {
        if warnings.is_empty() {
            CallToolResult::text(format!("ok — {ok_count} file(s) checked, no errors"))
        } else {
            let mut out = format!(
                "ok — {ok_count} file(s) checked, no errors\n{} warning(s):\n",
                warnings.len()
            );
            for w in &warnings {
                out.push_str(w);
                out.push('\n');
            }
            CallToolResult::text(out)
        }
    } else {
        let mut out = format!(
            "{} error(s) in {} file(s):\n",
            diagnostics.len(),
            files.len() - ok_count
        );
        for d in &diagnostics {
            out.push_str(d);
            out.push('\n');
        }
        if !warnings.is_empty() {
            out.push('\n');
            out.push_str(&format!("{} warning(s):\n", warnings.len()));
            for w in &warnings {
                out.push_str(w);
                out.push('\n');
            }
        }
        CallToolResult::text(out)
    }
}

fn tool_stdlib_ref(args: &Value) -> CallToolResult {
    let query = get_string_arg(args, "query").unwrap_or("");
    let all_docs = crate::stdlib_docs::all_stdlib_docs();

    let filtered: Vec<_> = if query.is_empty() {
        all_docs
    } else {
        let q = query.to_lowercase();
        all_docs
            .into_iter()
            .filter(|ns| {
                ns.namespace.to_lowercase().contains(&q)
                    || ns
                        .ops
                        .iter()
                        .any(|op| op.full_name.to_lowercase().contains(&q))
            })
            .collect()
    };

    if filtered.is_empty() {
        return CallToolResult::text(format!("No stdlib entries matching \"{query}\""));
    }

    let result = serde_json::to_string_pretty(&filtered).unwrap_or_else(|e| e.to_string());
    CallToolResult::text(result)
}

fn tool_doc_search(args: &Value) -> CallToolResult {
    let Some(raw_path) = get_string_arg(args, "path") else {
        return CallToolResult::error("Missing required argument: path".into());
    };
    let path = resolve_path(raw_path);
    let query = get_string_arg(args, "query").unwrap_or("");

    let base_dir = if path.is_file() {
        path.parent().unwrap_or(Path::new(".")).to_path_buf()
    } else {
        path.clone()
    };

    let artifact = match crate::doc::generate_docs_at_path(&path, &base_dir) {
        Ok(a) => a,
        Err(e) => return CallToolResult::error(e),
    };

    if query.is_empty() {
        let result = serde_json::to_string_pretty(&artifact).unwrap_or_else(|e| e.to_string());
        return CallToolResult::text(result);
    }

    // Filter by query
    let q = query.to_lowercase();
    let filtered: Value = json!({
        "dataflow_doc": artifact.dataflow_doc,
        "modules": artifact.modules.iter()
            .filter(|m| {
                serde_json::to_string(m)
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(&q)
            })
            .collect::<Vec<_>>()
    });

    let result = serde_json::to_string_pretty(&filtered).unwrap_or_else(|e| e.to_string());
    CallToolResult::text(result)
}

async fn tool_build(args: &Value) -> CallToolResult {
    let Some(raw_dir) = get_string_arg(args, "dir") else {
        return CallToolResult::error("Missing required argument: dir".into());
    };
    let dir = resolve_path(raw_dir);

    // Use the build command via subprocess to capture all output
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => return CallToolResult::error(format!("Cannot locate forai binary: {e}")),
    };

    let output = match tokio::process::Command::new(&exe)
        .arg("build")
        .arg(&dir)
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => return CallToolResult::error(format!("Failed to run build: {e}")),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut result = String::new();
    if !stderr.is_empty() {
        result.push_str(&stderr);
    }
    if !stdout.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(&stdout);
    }

    if output.status.success() {
        CallToolResult::text(result)
    } else {
        CallToolResult::error(result)
    }
}

async fn tool_test(args: &Value) -> CallToolResult {
    let raw_path = get_string_arg(args, "path").unwrap_or(".");
    let path = resolve_path(raw_path);

    // When given a directory, walk up to find the forai.json project root and scan
    // from there. This ensures all .fa files are found regardless of nesting depth —
    // the same behavior as `forai test` CLI. Passing "lib" or "sinks" would otherwise
    // silently miss tests in other parts of the project.
    let test_path = if path.is_dir() {
        match crate::config::find_config(&path) {
            Ok((_, root)) => root,
            Err(_) => path,
        }
    } else {
        path
    };

    match crate::tester::run_tests_at_path_async(&test_path).await {
        Ok(summary) => {
            let failures: Vec<_> = summary
                .failures
                .iter()
                .map(|f| {
                    json!({
                        "test": f.name,
                        "error": f.error,
                    })
                })
                .collect();
            let result = json!({
                "total": summary.total,
                "passed": summary.passed,
                "failed": summary.failed,
                "status": if summary.failed == 0 { "pass" } else { "fail" },
                "failures": failures,
                "warnings": summary.warnings,
            });
            CallToolResult::text(serde_json::to_string_pretty(&result).unwrap())
        }
        Err(e) => CallToolResult::error(e),
    }
}

async fn tool_run(args: &Value) -> CallToolResult {
    let Some(raw_source) = get_string_arg(args, "source") else {
        return CallToolResult::error("Missing required argument: source".into());
    };
    let source = resolve_path(raw_source);
    let cli_args: Vec<String> = args
        .get("args")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let timeout_secs: Option<f64> = args.get("timeout").and_then(|v| v.as_f64());

    let (flow, ir, registry, flow_registry, _ffi_registry) =
        match crate::compile_source(&source, &crate::deps::ResolvedDeps::empty()) {
            Ok(v) => v,
            Err(e) => return CallToolResult::error(e),
        };

    let inputs = if !cli_args.is_empty() {
        match crate::runtime::load_inputs_from_args(&flow, &cli_args) {
            Ok(i) => i,
            Err(e) => return CallToolResult::error(e),
        }
    } else {
        match crate::runtime::load_inputs(&flow, None) {
            Ok(i) => i,
            Err(e) => return CallToolResult::error(e),
        }
    };

    let codecs = crate::codec::CodecRegistry::default_registry();
    let future = crate::runtime::execute_flow(
        &flow,
        ir,
        inputs,
        &registry,
        Some(&flow_registry),
        &codecs,
        None,
    );

    let secs = timeout_secs.unwrap_or(120.0);
    let run_result = match tokio::time::timeout(std::time::Duration::from_secs_f64(secs), future)
        .await
    {
        Ok(r) => r,
        Err(_) => {
            return CallToolResult::error(format!(
                "Execution timed out after {secs}s — the flow may be waiting for user input (term.prompt) or an external event"
            ));
        }
    };

    match run_result {
        Ok(report) => {
            let result = json!({
                "outputs": report.outputs,
                "trace_len": report.trace.len(),
            });
            CallToolResult::text(serde_json::to_string_pretty(&result).unwrap())
        }
        Err(e) => CallToolResult::error(e),
    }
}

fn tool_fmt(args: &Value) -> CallToolResult {
    let Some(raw_path) = get_string_arg(args, "path") else {
        return CallToolResult::error("Missing required argument: path".into());
    };
    let path = resolve_path(raw_path);
    let check = args.get("check").and_then(|v| v.as_bool()).unwrap_or(false);

    match crate::formatter::fmt_path(&path, check) {
        Ok((changed, total)) => {
            if check {
                let result = json!({
                    "mode": "check",
                    "total_files": total,
                    "unformatted": changed.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                    "status": if changed.is_empty() { "ok" } else { "needs_formatting" }
                });
                CallToolResult::text(serde_json::to_string_pretty(&result).unwrap())
            } else {
                let result = json!({
                    "mode": "format",
                    "total_files": total,
                    "changed": changed.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                    "changed_count": changed.len()
                });
                CallToolResult::text(serde_json::to_string_pretty(&result).unwrap())
            }
        }
        Err(e) => CallToolResult::error(e),
    }
}

fn tool_flow_graph(args: &Value) -> CallToolResult {
    let Some(raw_source) = get_string_arg(args, "source") else {
        return CallToolResult::error("Missing required argument: source".into());
    };
    let source = resolve_path(raw_source);

    match crate::compile_source(&source, &crate::deps::ResolvedDeps::empty()) {
        Ok((_flow, ir, _registry, _flow_registry, _ffi_registry)) => {
            let result = serde_json::to_string_pretty(&ir).unwrap_or_else(|e| e.to_string());
            CallToolResult::text(result)
        }
        Err(e) => CallToolResult::error(e),
    }
}

async fn tool_debug_snapshot(args: &Value) -> CallToolResult {
    let Some(raw_source) = get_string_arg(args, "source") else {
        return CallToolResult::error("Missing required argument: source".into());
    };
    let source = resolve_path(raw_source);
    let cli_args: Vec<String> = args
        .get("args")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let timeout_secs: Option<f64> = args.get("timeout").and_then(|v| v.as_f64());

    let (flow, ir, registry, flow_registry, _ffi_registry) =
        match crate::compile_source(&source, &crate::deps::ResolvedDeps::empty()) {
            Ok(v) => v,
            Err(e) => return CallToolResult::error(e),
        };

    let inputs = if !cli_args.is_empty() {
        match crate::runtime::load_inputs_from_args(&flow, &cli_args) {
            Ok(i) => i,
            Err(e) => return CallToolResult::error(e),
        }
    } else {
        match crate::runtime::load_inputs(&flow, None) {
            Ok(i) => i,
            Err(e) => return CallToolResult::error(e),
        }
    };

    let codecs = crate::codec::CodecRegistry::default_registry();
    let future = crate::runtime::execute_flow(
        &flow,
        ir,
        inputs,
        &registry,
        Some(&flow_registry),
        &codecs,
        None,
    );

    let secs = timeout_secs.unwrap_or(120.0);
    let run_result = match tokio::time::timeout(std::time::Duration::from_secs_f64(secs), future)
        .await
    {
        Ok(r) => r,
        Err(_) => {
            return CallToolResult::error(format!(
                "Execution timed out after {secs}s — the flow may be waiting for user input (term.prompt) or an external event"
            ));
        }
    };

    match run_result {
        Ok(report) => {
            let result = json!({
                "outputs": report.outputs,
                "trace": report.trace,
            });
            CallToolResult::text(serde_json::to_string_pretty(&result).unwrap())
        }
        Err(e) => CallToolResult::error(e),
    }
}

fn tool_impact(args: &Value) -> CallToolResult {
    let Some(raw_path) = get_string_arg(args, "path") else {
        return CallToolResult::error("Missing required argument: path".into());
    };
    let path = resolve_path(raw_path);

    if !path.exists() {
        return CallToolResult::error(format!("File not found: {}", path.display()));
    }
    if !path.is_file() {
        return CallToolResult::error("path must be a .fa file, not a directory".into());
    }

    // Determine the module name from directory structure
    let file_dir = path.parent().unwrap_or(Path::new("."));
    let module_name = file_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let func_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    // Walk up to find project root (directory containing forai.json or the topmost .fa directory)
    let project_root = find_project_root(&path);

    // Collect all .fa files in the project
    let all_files = collect_fa_files(&project_root);

    let mut importers = Vec::new();
    let mut callers = Vec::new();
    let mut test_files = Vec::new();

    let qualified_name = if module_name.is_empty() {
        func_name.clone()
    } else {
        format!("{module_name}.{func_name}")
    };

    for file in &all_files {
        if file == &path {
            continue;
        }
        let text = match std::fs::read_to_string(file) {
            Ok(t) => t,
            Err(_) => continue,
        };

        // Check for `use <module_name> from "..."` import
        if !module_name.is_empty() {
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("use ")
                    && trimmed.contains(&format!(" {} from ", module_name))
                {
                    importers.push(file.display().to_string());
                    break;
                }
            }
        }

        // Check for calls like `module.FuncName(`
        if text.contains(&qualified_name) {
            callers.push(file.display().to_string());
        }

        // Check if this is a test file that references the func
        if text.contains(&format!("test {func_name}"))
            || text.contains(&format!("test {qualified_name}"))
        {
            test_files.push(file.display().to_string());
        }
    }

    // Check the target file itself for tests
    if let Ok(text) = std::fs::read_to_string(&path) {
        if text.contains("test ") {
            test_files.push(path.display().to_string());
        }
    }

    let result = json!({
        "file": path.display().to_string(),
        "module": module_name,
        "func": func_name,
        "importers": importers,
        "callers": callers,
        "tests": test_files,
    });

    CallToolResult::text(serde_json::to_string_pretty(&result).unwrap())
}

fn find_project_root(start: &Path) -> PathBuf {
    let mut dir = if start.is_file() {
        start.parent().unwrap_or(Path::new(".")).to_path_buf()
    } else {
        start.to_path_buf()
    };

    loop {
        if dir.join("forai.json").exists() {
            return dir;
        }
        if dir.join("main.fa").exists() {
            return dir;
        }
        match dir.parent() {
            Some(parent) if parent != dir => dir = parent.to_path_buf(),
            _ => break,
        }
    }

    // Fallback: use the file's parent directory
    if start.is_file() {
        start.parent().unwrap_or(Path::new(".")).to_path_buf()
    } else {
        start.to_path_buf()
    }
}

pub fn collect_fa_files(path: &Path) -> Vec<PathBuf> {
    if path.is_file() {
        if path.extension().and_then(|s| s.to_str()) == Some("fa") {
            return vec![path.to_path_buf()];
        }
        return vec![];
    }

    let mut files = Vec::new();
    collect_fa_files_recursive(path, &mut files);
    files
}

fn collect_fa_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.is_dir() {
            collect_fa_files_recursive(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("fa") {
            out.push(path);
        }
    }
}

async fn tool_example_search(args: &Value) -> CallToolResult {
    let base_url =
        std::env::var("FORAI_API_URL").unwrap_or_else(|_| "https://forailang.com".to_string());

    let mut params = Vec::new();
    if let Some(q) = get_string_arg(args, "query") {
        if !q.is_empty() {
            params.push(format!("q={}", urlencoding(q)));
        }
    }
    if let Some(cat) = get_string_arg(args, "category") {
        if !cat.is_empty() {
            params.push(format!("category={}", urlencoding(cat)));
        }
    }
    if let Some(diff) = get_string_arg(args, "difficulty") {
        if !diff.is_empty() {
            params.push(format!("difficulty={}", urlencoding(diff)));
        }
    }

    let url = if params.is_empty() {
        format!("{base_url}/api/examples")
    } else {
        format!("{base_url}/api/examples?{}", params.join("&"))
    };

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(e) => return CallToolResult::error(format!("Failed to create HTTP client: {e}")),
    };

    match client.get(&url).send().await {
        Ok(resp) => match resp.text().await {
            Ok(body) => CallToolResult::text(body),
            Err(e) => CallToolResult::error(format!("Failed to read response body: {e}")),
        },
        Err(e) => CallToolResult::error(format!(
            "Could not reach forailang.com examples API: {e}. Check network or set $FORAI_API_URL."
        )),
    }
}

/// Percent-encode a string for use in URL query parameters.
fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(char::from(HEX[(b >> 4) as usize]));
                out.push(char::from(HEX[(b & 0x0f) as usize]));
            }
        }
    }
    out
}

const HEX: [u8; 16] = *b"0123456789ABCDEF";
