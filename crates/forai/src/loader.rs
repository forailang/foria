pub use forai_core::loader::{FlowProgram, FlowRegistry};

use crate::config;
use crate::deps::ResolvedDeps;
use crate::ffi_manager::{self, FfiRegistry};
use crate::parser;
use crate::runtime;
use crate::sema;
use crate::typecheck;
use forai_core::ast::{DeclKind, FlowDecl, SourceLoopBlock, Statement, TopDecl, UsesDecl};
use forai_core::codec::CodecRegistry;
use forai_core::ir;
use forai_core::types::TypeRegistry;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Register imports from a loaded module into sub_registry, respecting named imports.
/// When `uses.imports` is non-empty, registers only the named items without module prefix.
/// When empty, registers the whole module with prefix as before.
fn register_uses_imports(
    uses: &UsesDecl,
    resolved: &Path,
    sub_registry: &mut FlowRegistry,
    loading_set: &mut HashSet<String>,
    extra_ops: &HashSet<String>,
    resolved_deps: &ResolvedDeps,
    context_path: &Path,
) -> Result<(), String> {
    if resolved.is_file() {
        let sub = load_single_file(&uses.name, resolved, loading_set, extra_ops, resolved_deps)?;
        if uses.imports.is_empty() {
            sub_registry.insert(uses.name.clone(), sub);
        } else {
            // Single file exports one program — register under each imported name
            for import_name in &uses.imports {
                sub_registry.insert(import_name.clone(), sub.clone());
            }
        }
    } else if resolved.is_dir() {
        let loaded = load_module(&uses.name, resolved, loading_set, extra_ops, resolved_deps)?;
        if uses.imports.is_empty() {
            for (name, program) in loaded.flows {
                sub_registry.insert(name, program);
            }
        } else {
            // Pick only the requested names, strip the module prefix
            let prefix = format!("{}.", uses.name);
            for import_name in &uses.imports {
                let qualified = format!("{}{}", prefix, import_name);
                if let Some(program) = loaded.flows.get(&qualified) {
                    sub_registry.insert(import_name.clone(), program.clone());
                } else {
                    return Err(format!(
                        "{}: '{}' not found in module '{}'",
                        context_path.display(),
                        import_name,
                        uses.name
                    ));
                }
            }
            // Also include transitive dependencies (sub-imports that the
            // named funcs depend on at runtime). These are entries without
            // the module prefix — they were propagated from nested use decls.
            for (name, program) in &loaded.flows {
                if !name.starts_with(&prefix) {
                    sub_registry.insert(name.clone(), program.clone());
                }
            }
        }
    } else {
        return Err(format!(
            "{}: import '{}' not found at '{}'",
            context_path.display(),
            uses.name,
            resolved.display()
        ));
    }
    Ok(())
}

fn resolve_use_path(
    uses_path: &str,
    base_dir: &Path,
    resolved_deps: &ResolvedDeps,
) -> Result<PathBuf, String> {
    // Check if the from path matches any declared dependency key
    if let Some(dep) = resolved_deps.get(uses_path) {
        // Load the package's forai.json to find its main entry
        let (dep_config, _) = config::load_config(&dep.path)?;
        let main_path = dep.path.join(&dep_config.main);
        if main_path.is_dir() {
            Ok(main_path)
        } else if main_path.is_file() {
            Ok(main_path)
        } else {
            // Treat main as a directory reference
            let parent = main_path.parent().unwrap_or(&dep.path);
            Ok(parent.to_path_buf())
        }
    } else {
        Ok(base_dir.join(uses_path))
    }
}

fn format_unknown_op(prefix: &str, op: &str) -> String {
    let base = format!("{prefix} uses unknown op `{op}`");
    if let Some(hint) = sema::unknown_op_fix_hint(op) {
        format!("{base} — {hint}")
    } else {
        base
    }
}

fn collect_expr_ops(expr: &forai_core::ast::Expr, out: &mut Vec<String>) {
    match expr {
        forai_core::ast::Expr::Call { func, args, .. } => {
            out.push(func.clone());
            for a in args {
                collect_expr_ops(a, out);
            }
        }
        forai_core::ast::Expr::BinOp { lhs, rhs, .. } => {
            collect_expr_ops(lhs, out);
            collect_expr_ops(rhs, out);
        }
        forai_core::ast::Expr::UnaryOp { expr: inner, .. } => collect_expr_ops(inner, out),
        forai_core::ast::Expr::Var(_) | forai_core::ast::Expr::Lit(_) => {}
        forai_core::ast::Expr::Interp(parts) => {
            for part in parts {
                if let forai_core::ast::InterpExpr::Expr(e) = part {
                    collect_expr_ops(e, out);
                }
            }
        }
        forai_core::ast::Expr::Ternary {
            cond,
            then_expr,
            else_expr,
        } => {
            collect_expr_ops(cond, out);
            collect_expr_ops(then_expr, out);
            collect_expr_ops(else_expr, out);
        }
        forai_core::ast::Expr::ListLit(items) => {
            for item in items {
                collect_expr_ops(item, out);
            }
        }
        forai_core::ast::Expr::DictLit(pairs) => {
            for (_, v) in pairs {
                collect_expr_ops(v, out);
            }
        }
        forai_core::ast::Expr::Index { expr, index } => {
            collect_expr_ops(expr, out);
            collect_expr_ops(index, out);
        }
        forai_core::ast::Expr::Coalesce { lhs, rhs } => {
            collect_expr_ops(lhs, out);
            collect_expr_ops(rhs, out);
        }
    }
}

fn collect_ops(statements: &[Statement], out: &mut Vec<String>) {
    for stmt in statements {
        match stmt {
            Statement::Node(n) => out.push(n.op.clone()),
            Statement::ExprAssign(ea) => collect_expr_ops(&ea.expr, out),
            Statement::Case(c) => {
                for arm in &c.arms {
                    collect_ops(&arm.body, out);
                }
                collect_ops(&c.else_body, out);
            }
            Statement::Loop(l) => collect_ops(&l.body, out),
            Statement::Sync(s) => collect_ops(&s.body, out),
            Statement::Emit(_) => {}
            Statement::SendNowait(sn) => out.push(sn.target.clone()),
            Statement::Break | Statement::Continue => {}
            Statement::BareLoop(b) => collect_ops(&b.body, out),
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

fn compile_func_decl(
    func_decl: &forai_core::ast::FuncDecl,
    registry: &TypeRegistry,
    file_path: &Path,
    flow_registry: &FlowRegistry,
    extra_ops: &HashSet<String>,
    kind: DeclKind,
) -> Result<FlowProgram, String> {
    let flow = parser::parse_runtime_func_decl_v1(func_decl).map_err(|e| {
        format!(
            "{}: func `{}` parse error: {e}",
            file_path.display(),
            func_decl.name
        )
    })?;

    typecheck::typecheck_func_with_flows(&func_decl.name, &func_decl.takes, &flow.body, &func_decl.emits, &func_decl.fails, registry, Some(flow_registry))
        .map_err(|e| format!("{}: {e}", file_path.display()))?;

    let known: HashSet<&str> = runtime::known_ops().iter().copied().collect();
    let mut ops = Vec::new();
    collect_ops(&flow.body, &mut ops);
    let unknown: Vec<_> = ops
        .iter()
        .filter(|op| {
            !known.contains(op.as_str())
                && !extra_ops.contains(op.as_str())
                && !flow_registry.is_flow(op)
                && !op.starts_with("ffi.")
        })
        .collect();
    if !unknown.is_empty() {
        return Err(unknown
            .iter()
            .map(|op| {
                format_unknown_op(
                    &format!("{}: func `{}`", file_path.display(), func_decl.name),
                    op,
                )
            })
            .collect::<Vec<_>>()
            .join("\n"));
    }

    let ir = ir::lower_to_ir(&flow).map_err(|e| {
        format!(
            "{}: func `{}` lower error: {e}",
            file_path.display(),
            func_decl.name
        )
    })?;

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
        ir,
        emit_name,
        fail_name,
        registry: registry.clone(),
        kind,
    })
}

fn compile_flow_decl(
    flow_decl: &FlowDecl,
    registry: &TypeRegistry,
    file_path: &Path,
    flow_registry: &FlowRegistry,
    extra_ops: &HashSet<String>,
) -> Result<FlowProgram, String> {
    let flow_graph = parser::parse_flow_graph_decl_v1(flow_decl).map_err(|e| {
        format!(
            "{}: flow `{}` parse error: {e}",
            file_path.display(),
            flow_decl.name
        )
    })?;
    // Type-check flow step wiring against the flow registry
    typecheck::typecheck_flow(&flow_decl.name, &flow_graph, registry, flow_registry)
        .map_err(|e| format!("{}: {e}", file_path.display()))?;

    let flow = parser::lower_flow_graph_to_flow(&flow_graph).map_err(|e| {
        format!(
            "{}: flow `{}` lower error: {e}",
            file_path.display(),
            flow_decl.name
        )
    })?;

    let known: HashSet<&str> = runtime::known_ops().iter().copied().collect();
    let mut ops = Vec::new();
    collect_ops(&flow.body, &mut ops);
    let unknown: Vec<_> = ops
        .iter()
        .filter(|op| {
            !known.contains(op.as_str())
                && !extra_ops.contains(op.as_str())
                && !flow_registry.is_flow(op)
                && !op.starts_with("ffi.")
        })
        .collect();
    if !unknown.is_empty() {
        return Err(unknown
            .iter()
            .map(|op| {
                format_unknown_op(
                    &format!("{}: flow `{}`", file_path.display(), flow_decl.name),
                    op,
                )
            })
            .collect::<Vec<_>>()
            .join("\n"));
    }

    let ir_val = ir::lower_to_ir(&flow).map_err(|e| {
        format!(
            "{}: flow `{}` IR lower error: {e}",
            file_path.display(),
            flow_decl.name
        )
    })?;

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

pub fn validate_call_hierarchy(registry: &FlowRegistry) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    for (name, program) in registry.iter() {
        let mut ops = Vec::new();
        collect_ops(&program.flow.body, &mut ops);
        match program.kind {
            DeclKind::Func => {
                // Funcs can only call other funcs and built-in ops (not flows, sinks, or sources)
                for op in &ops {
                    if let Some(target) = registry.get(op) {
                        if target.kind != DeclKind::Func {
                            errors.push(format!(
                                "func `{}` cannot call {} `{}`",
                                name, target.kind, op
                            ));
                        }
                    }
                }
            }
            DeclKind::Source => {
                // Sources can call funcs and built-in ops (not sinks, flows, or other sources)
                for op in &ops {
                    if let Some(target) = registry.get(op) {
                        if target.kind != DeclKind::Func {
                            errors.push(format!(
                                "source `{}` cannot call {} `{}`",
                                name, target.kind, op
                            ));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Scan a statement list for Node calls to sources. When found, wrap the source
/// call + all subsequent statements into a `SourceLoop` block. The source's
/// bind variable becomes the loop variable that each downstream statement receives.
pub fn transform_source_steps(
    stmts: &[Statement],
    source_names: &HashSet<String>,
) -> Vec<Statement> {
    // Find the first Node that calls a source
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
    // Recursively transform in case there are nested sources
    let body = transform_source_steps(remaining, source_names);

    result.push(Statement::SourceLoop(SourceLoopBlock {
        source_op: source_node.op.clone(),
        source_args: source_node.args.clone(),
        bind: source_node.bind.clone(),
        body,
    }));

    result
}

/// Post-processing pass: for every flow in the registry, transform its body
/// so that Node calls to sources become SourceLoop blocks.
pub fn wrap_source_loops(registry: &mut FlowRegistry) {
    let source_names: HashSet<String> = registry
        .iter()
        .filter(|(_, p)| p.kind == DeclKind::Source)
        .map(|(name, _)| name.clone())
        .collect();

    if source_names.is_empty() {
        return;
    }

    let flow_keys: Vec<String> = registry
        .iter()
        .filter(|(_, p)| p.kind == DeclKind::Flow)
        .map(|(name, _)| name.clone())
        .collect();

    for key in flow_keys {
        if let Some(program) = registry.get_mut(&key) {
            let new_body = transform_source_steps(&program.flow.body, &source_names);
            program.flow.body = new_body;
        }
    }
}

/// Load a single `.fa` file and register its callable under `name`.
/// Used for file imports: `use Round from "./round.fa"` → callable as `Round(...)`.
pub fn load_single_file(
    name: &str,
    file_path: &Path,
    loading_set: &mut HashSet<String>,
    extra_ops: &HashSet<String>,
    resolved_deps: &ResolvedDeps,
) -> Result<FlowProgram, String> {
    if loading_set.contains(name) {
        return Err(format!(
            "circular import dependency detected: '{}' is already being loaded",
            name
        ));
    }
    loading_set.insert(name.to_string());

    let src = fs::read_to_string(file_path)
        .map_err(|e| format!("failed to read {}: {e}", file_path.display()))?;

    let module = parser::parse_module_v1(&src)
        .map_err(|e| format!("{}: parse error: {e}", file_path.display()))?;

    let filename = file_path
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string());
    if let Err(errors) = sema::validate_module(&module, filename.as_deref()) {
        return Err(format!(
            "{}: semantic errors:\n{}",
            file_path.display(),
            errors.join("\n")
        ));
    }

    let type_registry = TypeRegistry::from_module(&module)
        .map_err(|errors| format!("{}: type errors:\n{}", file_path.display(), errors.join("\n")))?;

    // Process nested use declarations within this file
    let mut sub_registry = FlowRegistry::new();
    let file_dir = file_path
        .parent()
        .ok_or_else(|| format!("cannot determine parent dir of {}", file_path.display()))?;
    for decl in &module.decls {
        if let TopDecl::Uses(uses) = decl {
            let resolved = resolve_use_path(&uses.path, file_dir, resolved_deps)?;
            register_uses_imports(uses, &resolved, &mut sub_registry, loading_set, extra_ops, resolved_deps, file_path)?;
        }
    }

    // Find the single callable in the file
    let result = module.decls.iter().find_map(|decl| match decl {
        TopDecl::Func(f) => Some(compile_func_decl(
            f,
            &type_registry,
            file_path,
            &sub_registry,
            extra_ops,
            DeclKind::Func,
        )),
        TopDecl::Sink(f) => Some(compile_func_decl(
            f,
            &type_registry,
            file_path,
            &sub_registry,
            extra_ops,
            DeclKind::Sink,
        )),
        TopDecl::Source(f) => Some(compile_func_decl(
            f,
            &type_registry,
            file_path,
            &sub_registry,
            extra_ops,
            DeclKind::Source,
        )),
        TopDecl::Flow(f) => Some(compile_flow_decl(
            f,
            &type_registry,
            file_path,
            &sub_registry,
            extra_ops,
        )),
        _ => None,
    });

    loading_set.remove(name);

    result.ok_or_else(|| {
        format!(
            "{}: no callable (func/flow/sink/source) found in file",
            file_path.display()
        )
    })?
}

pub fn load_module(
    module_name: &str,
    module_dir: &Path,
    loading_set: &mut HashSet<String>,
    extra_ops: &HashSet<String>,
    resolved_deps: &ResolvedDeps,
) -> Result<FlowRegistry, String> {
    if loading_set.contains(module_name) {
        return Err(format!(
            "circular module dependency detected: module '{}' is already being loaded",
            module_name
        ));
    }
    loading_set.insert(module_name.to_string());

    let entries = fs::read_dir(module_dir).map_err(|e| {
        format!(
            "failed to read module directory {}: {e}",
            module_dir.display()
        )
    })?;

    let mut fa_files: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("read_dir entry error: {e}"))?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("fa") && path.is_file() {
            fa_files.push(path);
        }
    }
    fa_files.sort();

    let mut registry = FlowRegistry::new();

    for file in &fa_files {
        let src = fs::read_to_string(file)
            .map_err(|e| format!("failed to read {}: {e}", file.display()))?;

        let module = parser::parse_module_v1(&src)
            .map_err(|e| format!("{}: parse error: {e}", file.display()))?;

        let filename = file
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());
        if let Err(errors) = sema::validate_module(&module, filename.as_deref()) {
            return Err(format!(
                "{}: semantic errors:\n{}",
                file.display(),
                errors.join("\n")
            ));
        }

        let type_registry = TypeRegistry::from_module(&module)
            .map_err(|errors| format!("{}: type errors:\n{}", file.display(), errors.join("\n")))?;

        // Process use declarations within the module file (nested modules)
        let mut sub_registry = FlowRegistry::new();
        for decl in &module.decls {
            if let TopDecl::Uses(uses) = decl {
                let resolved = resolve_use_path(&uses.path, module_dir, resolved_deps)?;
                register_uses_imports(uses, &resolved, &mut sub_registry, loading_set, extra_ops, resolved_deps, file)?;
            }
        }

        // Include nested module funcs in the registry so they are
        // available at runtime (not just during compile-time validation)
        for (name, program) in &sub_registry.flows {
            registry.insert(name.clone(), program.clone());
        }

        for decl in &module.decls {
            match decl {
                TopDecl::Func(f) => {
                    let program = compile_func_decl(
                        f,
                        &type_registry,
                        file,
                        &sub_registry,
                        extra_ops,
                        DeclKind::Func,
                    )?;
                    let qualified = format!("{}.{}", module_name, f.name);
                    registry.insert(qualified, program);
                }
                TopDecl::Sink(f) => {
                    let program = compile_func_decl(
                        f,
                        &type_registry,
                        file,
                        &sub_registry,
                        extra_ops,
                        DeclKind::Sink,
                    )?;
                    let qualified = format!("{}.{}", module_name, f.name);
                    registry.insert(qualified, program);
                }
                TopDecl::Source(f) => {
                    let program = compile_func_decl(
                        f,
                        &type_registry,
                        file,
                        &sub_registry,
                        extra_ops,
                        DeclKind::Source,
                    )?;
                    let qualified = format!("{}.{}", module_name, f.name);
                    registry.insert(qualified, program);
                }
                TopDecl::Flow(f) => {
                    let program =
                        compile_flow_decl(f, &type_registry, file, &sub_registry, extra_ops)?;
                    let qualified = format!("{}.{}", module_name, f.name);
                    registry.insert(qualified, program);
                }
                _ => {}
            }
        }
    }

    loading_set.remove(module_name);

    // Validate call hierarchy: funcs can only call funcs + built-in ops
    if let Err(errs) = validate_call_hierarchy(&registry) {
        return Err(errs.join("\n"));
    }

    // Transform flow bodies: wrap source calls into SourceLoop blocks
    wrap_source_loops(&mut registry);

    Ok(registry)
}

pub fn build_flow_registry(
    entry_path: &Path,
    module: &forai_core::ast::ModuleAst,
    resolved_deps: &ResolvedDeps,
) -> Result<FlowRegistry, String> {
    let base_dir = entry_path
        .parent()
        .ok_or_else(|| "cannot determine parent directory of entry file".to_string())?;

    let codec_registry = CodecRegistry::default_registry();
    let extra_ops: HashSet<String> = codec_registry.known_ops().into_iter().collect();

    let mut registry = FlowRegistry::new();
    let mut loading_set = HashSet::new();

    for decl in &module.decls {
        if let TopDecl::Uses(uses) = decl {
            let resolved = resolve_use_path(&uses.path, base_dir, resolved_deps)?;
            register_uses_imports(uses, &resolved, &mut registry, &mut loading_set, &extra_ops, resolved_deps, entry_path)?;
        }
    }

    // Transform flow bodies: wrap source calls into SourceLoop blocks
    wrap_source_loops(&mut registry);

    Ok(registry)
}

/// Collect extern blocks from all imported modules (not the main module itself).
pub fn collect_imported_ffi(
    entry_path: &Path,
    module: &forai_core::ast::ModuleAst,
    resolved_deps: &ResolvedDeps,
) -> FfiRegistry {
    let base_dir = match entry_path.parent() {
        Some(d) => d,
        None => return FfiRegistry::new(),
    };
    let mut ffi = FfiRegistry::new();
    collect_ffi_recursive(module, base_dir, resolved_deps, &mut HashSet::new(), &mut ffi);
    ffi
}

fn collect_ffi_recursive(
    module: &forai_core::ast::ModuleAst,
    base_dir: &Path,
    resolved_deps: &ResolvedDeps,
    visited: &mut HashSet<PathBuf>,
    ffi: &mut FfiRegistry,
) {
    for decl in &module.decls {
        if let TopDecl::Uses(uses) = decl {
            let resolved = match resolve_use_path(&uses.path, base_dir, resolved_deps) {
                Ok(p) => p,
                Err(_) => continue,
            };
            if resolved.is_file() {
                if !visited.insert(resolved.clone()) {
                    continue;
                }
                let src = match fs::read_to_string(&resolved) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let sub_module = match parser::parse_module_v1(&src) {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                ffi.merge(ffi_manager::build_ffi_registry(&sub_module));
                let sub_dir = resolved.parent().unwrap_or(base_dir);
                collect_ffi_recursive(&sub_module, sub_dir, resolved_deps, visited, ffi);
            } else if resolved.is_dir() {
                collect_ffi_from_dir(&resolved, resolved_deps, visited, ffi);
            }
        }
    }
}

fn collect_ffi_from_dir(
    dir: &Path,
    resolved_deps: &ResolvedDeps,
    visited: &mut HashSet<PathBuf>,
    ffi: &mut FfiRegistry,
) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("fa") {
            if !visited.insert(path.clone()) {
                continue;
            }
            let src = match fs::read_to_string(&path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let sub_module = match parser::parse_module_v1(&src) {
                Ok(m) => m,
                Err(_) => continue,
            };
            ffi.merge(ffi_manager::build_ffi_registry(&sub_module));
            collect_ffi_recursive(&sub_module, dir, resolved_deps, visited, ffi);
        } else if path.is_dir() {
            collect_ffi_from_dir(&path, resolved_deps, visited, ffi);
        }
    }
}
