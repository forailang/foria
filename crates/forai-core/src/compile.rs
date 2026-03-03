use crate::ast::{DeclKind, Expr, FlowDecl, FuncDecl, Statement, TopDecl};
use crate::codec::CodecRegistry;
use crate::ir;
use crate::loader::{FlowProgram, FlowRegistry, ProgramBundle};
use crate::parser;
use crate::sema;
use crate::typecheck;
use crate::types::TypeRegistry;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

/// A structured compile error with location info.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompileError {
    pub file: String,
    pub line: usize,
    pub col: usize,
    pub message: String,
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{} {}", self.file, self.line, self.col, self.message)
    }
}

/// Complete list of known runtime ops (pure + I/O + codec).
/// Computed once and cached for the lifetime of the process.
static KNOWN_OPS: LazyLock<HashSet<String>> = LazyLock::new(|| {
    let mut ops: HashSet<String> = crate::pure_ops::known_ops().iter().map(|s| s.to_string()).collect();
    // I/O ops (from host.rs and runtime)
    for op in &[
        "http.extract_path", "http.extract_params",
        "auth.extract_email", "auth.extract_password", "auth.validate_email",
        "auth.validate_password", "db.query_user_by_email", "db.query_credentials",
        "db.open", "db.exec", "db.query", "db.close",
        "auth.verify_password", "auth.sample_checks", "auth.pass_through",
        "http.error_response", "http.response",
        "http.get", "http.post", "http.put", "http.patch", "http.delete", "http.request",
        "http.server.listen", "http.server.accept", "http.server.respond", "http.server.close",
        "http.respond.html", "http.respond.json", "http.respond.text", "http.respond.file",
        "accept",
        "ws.connect", "ws.send", "ws.recv", "ws.close",
        "headers.new", "headers.set", "headers.get", "headers.delete",
        "math.floor", "math.round",
        "time.sleep", "time.tick", "time.split_hms",
        "fmt.pad_hms", "fmt.wrap_field",
        "date.now", "date.now_tz", "date.from_unix_ms", "date.from_parts",
        "date.from_parts_tz", "date.from_iso", "date.from_epoch",
        "date.to_unix_ms", "date.to_parts", "date.to_iso", "date.to_epoch",
        "date.weekday", "date.with_tz", "date.add", "date.add_days", "date.diff", "date.compare",
        "stamp.now", "stamp.from_ns", "stamp.from_epoch",
        "stamp.to_ns", "stamp.to_ms", "stamp.to_date", "stamp.to_epoch",
        "stamp.add", "stamp.diff", "stamp.compare",
        "trange.new", "trange.start", "trange.end", "trange.duration_ms",
        "trange.contains", "trange.overlaps", "trange.shift",
        "list.range", "list.new", "list.append", "list.len", "list.contains",
        "list.slice", "list.indices",
        "obj.new", "obj.set", "obj.get", "obj.has", "obj.delete", "obj.keys", "obj.merge",
        "term.print", "term.prompt", "term.clear", "term.size", "term.cursor",
        "term.move_to", "term.color", "term.read_key",
        "file.read", "file.write", "file.append", "file.delete", "file.exists",
        "file.list", "file.mkdir", "file.copy", "file.move", "file.size", "file.is_dir",
        "str.len", "str.upper", "str.lower", "str.trim", "str.trim_start", "str.trim_end",
        "str.split", "str.join", "str.replace", "str.contains", "str.starts_with",
        "str.ends_with", "str.slice", "str.index_of", "str.repeat",
        "type.of", "to.text", "to.long", "to.real", "to.bool",
        "env.get", "env.set", "env.has", "env.list", "env.remove",
        "exec.run",
        "regex.match", "regex.find", "regex.find_all", "regex.replace",
        "regex.replace_all", "regex.split",
        "random.int", "random.float", "random.uuid", "random.choice", "random.shuffle",
        "hash.sha256", "hash.sha512", "hash.hmac",
        "base64.encode", "base64.decode", "base64.encode_url", "base64.decode_url",
        "crypto.hash_password", "crypto.verify_password",
        "crypto.sign_token", "crypto.verify_token", "crypto.random_bytes",
        "log.debug", "log.info", "log.warn", "log.error", "log.trace",
        "error.new", "error.wrap", "error.code", "error.message",
        "cookie.parse", "cookie.get", "cookie.set", "cookie.delete",
        "url.parse", "url.query_parse", "url.encode", "url.decode",
        "route.match", "route.params",
        "html.escape", "html.unescape",
        "tmpl.render",
        "ffi.available",
    ] {
        ops.insert(op.to_string());
    }
    // Codec ops
    let codec_registry = CodecRegistry::default_registry();
    for cop in codec_registry.known_ops() {
        ops.insert(cop);
    }
    ops
});

/// Returns the cached set of all known ops.
pub fn known_ops() -> &'static HashSet<String> {
    &KNOWN_OPS
}

/// Compile a project from a virtual filesystem (HashMap of path -> source).
///
/// `files` maps virtual paths (e.g., "main.fa", "app/Hello.fa") to source text.
/// `entry_point` is the path of the main entry file (e.g., "main.fa").
///
/// Returns a ProgramBundle on success, or a list of CompileErrors on failure.
pub fn compile_project(
    files: &HashMap<String, String>,
    entry_point: &str,
) -> Result<ProgramBundle, Vec<CompileError>> {
    let source = files.get(entry_point).ok_or_else(|| {
        vec![CompileError {
            file: entry_point.to_string(),
            line: 0,
            col: 0,
            message: format!("entry point '{}' not found in files", entry_point),
        }]
    })?;

    // Parse entry module
    let module = parser::parse_module_v1(source).map_err(|e| {
        vec![CompileError {
            file: entry_point.to_string(),
            line: e.span.line,
            col: e.span.col,
            message: e.message,
        }]
    })?;

    // Semantic validation
    let filename = entry_point.rsplit('/').next();
    if let Err(errors) = sema::validate_module(&module, filename) {
        return Err(errors
            .into_iter()
            .map(|e| CompileError {
                file: entry_point.to_string(),
                line: 0,
                col: 0,
                message: e,
            })
            .collect());
    }

    // Build type registry
    let type_registry = TypeRegistry::from_module(&module).map_err(|errors| {
        errors
            .into_iter()
            .map(|e| CompileError {
                file: entry_point.to_string(),
                line: 0,
                col: 0,
                message: e,
            })
            .collect::<Vec<_>>()
    })?;

    // Resolve uses from virtual filesystem
    let known = known_ops();
    let mut flow_registry = FlowRegistry::new();
    let mut loading_set = HashSet::new();

    let entry_dir = entry_point
        .rfind('/')
        .map(|i| &entry_point[..i])
        .unwrap_or("");

    for decl in &module.decls {
        if let TopDecl::Uses(uses) = decl {
            let resolved_path = resolve_virtual_path(&uses.path, entry_dir);
            load_virtual_module(
                &uses.name,
                &resolved_path,
                files,
                known,
                &mut flow_registry,
                &mut loading_set,
            )
            .map_err(|e| {
                vec![CompileError {
                    file: entry_point.to_string(),
                    line: 0,
                    col: 0,
                    message: e,
                }]
            })?;
        }
    }

    // Parse the entry flow body
    let flow = parser::parse_runtime_func_from_module_v1(&module).map_err(|e| {
        vec![CompileError {
            file: entry_point.to_string(),
            line: 0,
            col: 0,
            message: e,
        }]
    })?;

    // Validate ops
    let mut ops = Vec::new();
    collect_ops(&flow.body, &mut ops);
    let unknown: Vec<_> = ops
        .iter()
        .filter(|op| {
            !known.contains(op.as_str())
                && !flow_registry.is_flow(op)
                && !op.starts_with("ffi.")
        })
        .collect();
    if !unknown.is_empty() {
        return Err(unknown
            .into_iter()
            .map(|op| CompileError {
                file: entry_point.to_string(),
                line: 0,
                col: 0,
                message: format!("unknown op `{}`", op),
            })
            .collect());
    }

    // Transform source steps
    let source_names: HashSet<String> = flow_registry
        .iter()
        .filter(|(_, p)| p.kind == DeclKind::Source)
        .map(|(name, _)| name.clone())
        .collect();
    let mut flow = flow;
    if !source_names.is_empty() {
        flow.body = transform_source_steps(&flow.body, &source_names);
    }

    // Lower to IR
    let entry_ir = ir::lower_to_ir(&flow).map_err(|e| {
        vec![CompileError {
            file: entry_point.to_string(),
            line: 0,
            col: 0,
            message: e,
        }]
    })?;

    Ok(ProgramBundle {
        entry_flow: flow,
        entry_ir,
        type_registry,
        flow_registry,
    })
}

/// Resolve a virtual path relative to a base directory.
fn resolve_virtual_path(uses_path: &str, base_dir: &str) -> String {
    if uses_path.starts_with("./") {
        let relative = &uses_path[2..];
        if base_dir.is_empty() {
            relative.to_string()
        } else {
            format!("{}/{}", base_dir, relative)
        }
    } else {
        uses_path.to_string()
    }
}

/// Load a module from the virtual filesystem.
/// Tries as a single file first (path.fa), then as a directory (all path/*.fa files).
fn load_virtual_module(
    name: &str,
    path: &str,
    files: &HashMap<String, String>,
    known: &HashSet<String>,
    registry: &mut FlowRegistry,
    loading_set: &mut HashSet<String>,
) -> Result<(), String> {
    if loading_set.contains(name) {
        return Err(format!(
            "circular import dependency detected: '{}' is already being loaded",
            name
        ));
    }
    loading_set.insert(name.to_string());

    // Try as a single .fa file
    let file_key = if path.ends_with(".fa") {
        path.to_string()
    } else {
        format!("{}.fa", path)
    };

    if let Some(src) = files.get(&file_key) {
        let program = compile_single_virtual_file(
            name, &file_key, src, files, known, registry, loading_set,
        )?;
        registry.insert(name.to_string(), program);
    } else {
        // Try as a directory — find all files with this path prefix
        let dir_prefix = if path.ends_with('/') {
            path.to_string()
        } else {
            format!("{}/", path)
        };
        let mut module_files: Vec<(&String, &String)> = files
            .iter()
            .filter(|(k, _)| {
                k.starts_with(&dir_prefix)
                    && k.ends_with(".fa")
                    && !k[dir_prefix.len()..].contains('/') // only direct children
            })
            .collect();
        module_files.sort_by_key(|(k, _)| k.as_str());

        if module_files.is_empty() {
            loading_set.remove(name);
            return Err(format!(
                "import '{}' not found: no file '{}' or directory '{}'",
                name, file_key, path
            ));
        }

        for (file_path, src) in &module_files {
            let func_name = file_path
                .rsplit('/')
                .next()
                .unwrap_or(file_path)
                .trim_end_matches(".fa");
            let qualified_name = format!("{}.{}", name, func_name);
            let program = compile_single_virtual_file(
                &qualified_name,
                file_path,
                src,
                files,
                known,
                registry,
                loading_set,
            )?;
            registry.insert(qualified_name, program);
        }
    }

    loading_set.remove(name);
    Ok(())
}

/// Compile a single virtual .fa file into a FlowProgram.
fn compile_single_virtual_file(
    _name: &str,
    file_path: &str,
    src: &str,
    files: &HashMap<String, String>,
    known: &HashSet<String>,
    parent_registry: &mut FlowRegistry,
    loading_set: &mut HashSet<String>,
) -> Result<FlowProgram, String> {
    let module = parser::parse_module_v1(src)
        .map_err(|e| format!("{}: parse error at {}:{}: {}", file_path, e.span.line, e.span.col, e.message))?;

    let filename = file_path.rsplit('/').next();
    if let Err(errors) = sema::validate_module(&module, filename) {
        return Err(format!(
            "{}: semantic errors:\n{}",
            file_path,
            errors.join("\n")
        ));
    }

    let type_registry = TypeRegistry::from_module(&module)
        .map_err(|errors| format!("{}: type errors:\n{}", file_path, errors.join("\n")))?;

    // Resolve nested uses — load into parent registry so they're available at runtime
    let file_dir = file_path.rfind('/').map(|i| &file_path[..i]).unwrap_or("");
    for decl in &module.decls {
        if let TopDecl::Uses(uses) = decl {
            let resolved = resolve_virtual_path(&uses.path, file_dir);
            load_virtual_module(
                &uses.name,
                &resolved,
                files,
                known,
                parent_registry,
                loading_set,
            )?;
        }
    }

    // Find the callable
    let result = module.decls.iter().find_map(|decl| match decl {
        TopDecl::Func(f) => Some(compile_virtual_func(
            f, &type_registry, file_path, known, parent_registry,
            DeclKind::Func,
        )),
        TopDecl::Sink(f) => Some(compile_virtual_func(
            f, &type_registry, file_path, known, parent_registry,
            DeclKind::Sink,
        )),
        TopDecl::Source(f) => Some(compile_virtual_func(
            f, &type_registry, file_path, known, parent_registry,
            DeclKind::Source,
        )),
        TopDecl::Flow(f) => Some(compile_virtual_flow(
            f, &type_registry, file_path, known, parent_registry,
        )),
        _ => None,
    });

    result.ok_or_else(|| {
        format!(
            "{}: no callable (func/flow/sink/source) found in file",
            file_path
        )
    })?
}

fn compile_virtual_func(
    func_decl: &FuncDecl,
    registry: &TypeRegistry,
    file_path: &str,
    known: &HashSet<String>,
    flow_registry: &FlowRegistry,
    kind: DeclKind,
) -> Result<FlowProgram, String> {
    let flow = parser::parse_runtime_func_decl_v1(func_decl)
        .map_err(|e| format!("{}: func `{}` parse error: {e}", file_path, func_decl.name))?;

    typecheck::typecheck_func(&func_decl.name, &func_decl.takes, &flow.body)
        .map_err(|e| format!("{}: {e}", file_path))?;

    let mut ops = Vec::new();
    collect_ops(&flow.body, &mut ops);
    let unknown: Vec<_> = ops
        .iter()
        .filter(|op| {
            !known.contains(op.as_str())
                && !flow_registry.is_flow(op)
                && !op.starts_with("ffi.")
        })
        .collect();
    if !unknown.is_empty() {
        return Err(unknown
            .iter()
            .map(|op| format!("{}: func `{}` uses unknown op `{}`", file_path, func_decl.name, op))
            .collect::<Vec<_>>()
            .join("\n"));
    }

    let ir_val = ir::lower_to_ir(&flow)
        .map_err(|e| format!("{}: func `{}` lower error: {e}", file_path, func_decl.name))?;

    let emit_name = if func_decl.return_type.is_some() {
        Some("_return".to_string())
    } else {
        func_decl.emits.first().map(|e| e.name.clone())
    };
    let fail_name = if func_decl.fail_type.is_some() {
        Some("_fail".to_string())
    } else {
        func_decl.fails.first().map(|f| f.name.clone())
    };

    Ok(FlowProgram {
        flow,
        ir: ir_val,
        emit_name,
        fail_name,
        registry: registry.clone(),
        kind,
    })
}

fn compile_virtual_flow(
    flow_decl: &FlowDecl,
    registry: &TypeRegistry,
    file_path: &str,
    known: &HashSet<String>,
    flow_registry: &FlowRegistry,
) -> Result<FlowProgram, String> {
    let flow_graph = parser::parse_flow_graph_decl_v1(flow_decl)
        .map_err(|e| format!("{}: flow `{}` parse error: {e}", file_path, flow_decl.name))?;
    let flow = parser::lower_flow_graph_to_flow(&flow_graph)
        .map_err(|e| format!("{}: flow `{}` lower error: {e}", file_path, flow_decl.name))?;

    let mut ops = Vec::new();
    collect_ops(&flow.body, &mut ops);
    let unknown: Vec<_> = ops
        .iter()
        .filter(|op| {
            !known.contains(op.as_str())
                && !flow_registry.is_flow(op)
                && !op.starts_with("ffi.")
        })
        .collect();
    if !unknown.is_empty() {
        return Err(unknown
            .iter()
            .map(|op| format!("{}: flow `{}` uses unknown op `{}`", file_path, flow_decl.name, op))
            .collect::<Vec<_>>()
            .join("\n"));
    }

    let ir_val = ir::lower_to_ir(&flow)
        .map_err(|e| format!("{}: flow `{}` IR lower error: {e}", file_path, flow_decl.name))?;

    let emit_name = flow.outputs.first().map(|p| p.name.clone());
    let fail_name = flow.outputs.get(1).map(|p| p.name.clone());

    Ok(FlowProgram {
        flow,
        ir: ir_val,
        emit_name,
        fail_name,
        registry: registry.clone(),
        kind: DeclKind::Flow,
    })
}

/// Collect all op names from a list of statements.
fn collect_ops(stmts: &[Statement], out: &mut Vec<String>) {
    for stmt in stmts {
        match stmt {
            Statement::Node(n) => out.push(n.op.clone()),
            Statement::ExprAssign(ea) => collect_expr_ops(&ea.expr, out),
            Statement::Emit(_) => {}
            Statement::Case(c) => {
                for arm in &c.arms {
                    collect_ops(&arm.body, out);
                }
                collect_ops(&c.else_body, out);
            }
            Statement::Loop(l) => collect_ops(&l.body, out),
            Statement::BareLoop(l) => collect_ops(&l.body, out),
            Statement::Sync(s) => collect_ops(&s.body, out),
            Statement::SendNowait(s) => out.push(s.target.clone()),
            Statement::Break => {}
            Statement::SourceLoop(sl) => {
                out.push(sl.source_op.clone());
                collect_ops(&sl.body, out);
            }
            Statement::On(on_block) => {
                out.push(on_block.source_op.clone());
                collect_ops(&on_block.body, out);
            }
        }
    }
}

fn collect_expr_ops(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Call { func, args } => {
            out.push(func.clone());
            for a in args {
                collect_expr_ops(a, out);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_expr_ops(lhs, out);
            collect_expr_ops(rhs, out);
        }
        Expr::UnaryOp { expr, .. } => {
            collect_expr_ops(expr, out);
        }
        Expr::Index { expr: e, index } => {
            collect_expr_ops(e, out);
            collect_expr_ops(index, out);
        }
        Expr::ListLit(items) => {
            for item in items {
                collect_expr_ops(item, out);
            }
        }
        Expr::DictLit(entries) => {
            for (_, v) in entries {
                collect_expr_ops(v, out);
            }
        }
        Expr::Ternary { cond, then_expr, else_expr } => {
            collect_expr_ops(cond, out);
            collect_expr_ops(then_expr, out);
            collect_expr_ops(else_expr, out);
        }
        Expr::Interp(parts) => {
            for part in parts {
                if let crate::ast::InterpExpr::Expr(e) = part {
                    collect_expr_ops(e, out);
                }
            }
        }
        Expr::Var(_) | Expr::Lit(_) => {}
    }
}

/// Transform source-call steps into SourceLoop blocks.
/// All statements after the source call become the loop body,
/// and nested sources are recursively transformed.
fn transform_source_steps(
    stmts: &[Statement],
    source_names: &HashSet<String>,
) -> Vec<Statement> {
    let source_idx = stmts.iter().position(|s| {
        if let Statement::Node(n) = s {
            source_names.contains(&n.op)
        } else {
            false
        }
    });

    let Some(idx) = source_idx else {
        return stmts.to_vec();
    };

    let source_node = match &stmts[idx] {
        Statement::Node(n) => n,
        _ => unreachable!(),
    };

    // Everything before the source node stays as-is
    let mut result: Vec<Statement> = stmts[..idx].to_vec();

    // Everything after the source node becomes the SourceLoop body
    let remaining = &stmts[idx + 1..];
    let body = transform_source_steps(remaining, source_names);

    result.push(Statement::SourceLoop(crate::ast::SourceLoopBlock {
        source_op: source_node.op.clone(),
        source_args: source_node.args.clone(),
        bind: source_node.bind.clone(),
        body,
    }));

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_simple_flow() {
        let mut files = HashMap::new();
        files.insert(
            "main.fa".to_string(),
            r#"
use lib from "./lib"

docs main
    A simple test flow.
done

flow main
    emit result as text
    fail error as text
body
    step lib.Greet("world" to :name) then
        next :result to msg
    done
    emit msg to :result
done

test main
    mock lib.Greet => "Hello world!"
    result = main()
    must result == "Hello world!"
done
"#
            .to_string(),
        );
        files.insert(
            "lib/Greet.fa".to_string(),
            r#"
docs Greet
    Builds a greeting.
done

func Greet
    take name as text
    emit result as text
    fail error as text
body
    greeting = "Hello #{name}!"
    emit greeting
done

test Greet
    result = Greet("world")
    must result == "Hello world!"
done
"#
            .to_string(),
        );

        let result = compile_project(&files, "main.fa");
        assert!(result.is_ok(), "compile failed: {:?}", result.err());
        let bundle = result.unwrap();
        assert_eq!(bundle.entry_flow.name, "main");
    }

    #[test]
    fn compile_missing_entry_point() {
        let files = HashMap::new();
        let result = compile_project(&files, "main.fa");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("not found"));
    }

    #[test]
    fn compile_parse_error() {
        let mut files = HashMap::new();
        files.insert(
            "main.fa".to_string(),
            "func main\n  this is invalid\ndone\n".to_string(),
        );
        let result = compile_project(&files, "main.fa");
        assert!(result.is_err());
    }

    #[test]
    fn compile_with_uses() {
        let mut files = HashMap::new();
        files.insert(
            "main.fa".to_string(),
            r#"
use greeter from "./greeter"

docs main
    Entry point.
done

flow main
    emit result as text
    fail error as text
body
    step greeter.Hello("world" to :name) then
        next :result to msg
    done
    emit msg to :result
done

test main
    mock greeter.Hello => "Hello world!"
    result = main()
    must result == "Hello world!"
done
"#
            .to_string(),
        );
        files.insert(
            "greeter/Hello.fa".to_string(),
            r#"
docs Hello
    Says hello.
done

func Hello
    take name as text
    emit result as text
    fail error as text
body
    greeting = "Hello #{name}!"
    emit greeting
done

test Hello
    result = Hello("world")
    must result == "Hello world!"
done
"#
            .to_string(),
        );

        let result = compile_project(&files, "main.fa");
        assert!(result.is_ok(), "compile failed: {:?}", result.err());
    }
}
