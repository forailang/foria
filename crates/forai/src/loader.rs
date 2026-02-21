pub use forai_core::loader::{FlowProgram, FlowRegistry};

use forai_core::ast::{DeclKind, FlowDecl, SourceLoopBlock, Statement, TopDecl};
use forai_core::codec::CodecRegistry;
use forai_core::ir::{self, Ir};
use forai_core::types::TypeRegistry;
use crate::parser;
use crate::runtime;
use crate::sema;
use crate::typecheck;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

fn collect_expr_ops(expr: &forai_core::ast::Expr, out: &mut Vec<String>) {
    match expr {
        forai_core::ast::Expr::Call { func, args } => {
            out.push(func.clone());
            for a in args { collect_expr_ops(a, out); }
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
        forai_core::ast::Expr::Ternary { cond, then_expr, else_expr } => {
            collect_expr_ops(cond, out);
            collect_expr_ops(then_expr, out);
            collect_expr_ops(else_expr, out);
        }
        forai_core::ast::Expr::ListLit(items) => {
            for item in items { collect_expr_ops(item, out); }
        }
        forai_core::ast::Expr::DictLit(pairs) => {
            for (_, v) in pairs { collect_expr_ops(v, out); }
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
            Statement::Break => {}
            Statement::BareLoop(b) => collect_ops(&b.body, out),
            Statement::SourceLoop(sl) => {
                out.push(sl.source_op.clone());
                collect_ops(&sl.body, out);
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
        format!("{}: func `{}` parse error: {e}", file_path.display(), func_decl.name)
    })?;

    typecheck::typecheck_func(&func_decl.name, &func_decl.takes, &flow.body).map_err(|e| {
        format!("{}: {e}", file_path.display())
    })?;

    let known: HashSet<&str> = runtime::known_ops().iter().copied().collect();
    let mut ops = Vec::new();
    collect_ops(&flow.body, &mut ops);
    let unknown: Vec<_> = ops
        .iter()
        .filter(|op| !known.contains(op.as_str()) && !extra_ops.contains(op.as_str()) && !flow_registry.is_flow(op))
        .collect();
    if !unknown.is_empty() {
        return Err(unknown
            .iter()
            .map(|op| format!("{}: func `{}` uses unknown op `{op}`", file_path.display(), func_decl.name))
            .collect::<Vec<_>>()
            .join("\n"));
    }

    let ir = ir::lower_to_ir(&flow).map_err(|e| {
        format!("{}: func `{}` lower error: {e}", file_path.display(), func_decl.name)
    })?;

    let emit_name = flow
        .outputs
        .first()
        .map(|p| p.name.clone())
        .ok_or_else(|| format!("{}: func `{}` has no emit output", file_path.display(), func_decl.name))?;
    let fail_name = flow
        .outputs
        .get(1)
        .map(|p| p.name.clone())
        .ok_or_else(|| format!("{}: func `{}` has no fail output", file_path.display(), func_decl.name))?;

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
        format!("{}: flow `{}` parse error: {e}", file_path.display(), flow_decl.name)
    })?;
    let flow = parser::lower_flow_graph_to_flow(&flow_graph).map_err(|e| {
        format!("{}: flow `{}` lower error: {e}", file_path.display(), flow_decl.name)
    })?;

    let known: HashSet<&str> = runtime::known_ops().iter().copied().collect();
    let mut ops = Vec::new();
    collect_ops(&flow.body, &mut ops);
    let unknown: Vec<_> = ops
        .iter()
        .filter(|op| !known.contains(op.as_str()) && !extra_ops.contains(op.as_str()) && !flow_registry.is_flow(op))
        .collect();
    if !unknown.is_empty() {
        return Err(unknown
            .iter()
            .map(|op| format!("{}: flow `{}` uses unknown op `{op}`", file_path.display(), flow_decl.name))
            .collect::<Vec<_>>()
            .join("\n"));
    }

    let ir_val = ir::lower_to_ir(&flow).map_err(|e| {
        format!("{}: flow `{}` IR lower error: {e}", file_path.display(), flow_decl.name)
    })?;

    let emit_name = flow
        .outputs
        .first()
        .map(|p| p.name.clone())
        .ok_or_else(|| format!("{}: flow `{}` has no emit output", file_path.display(), flow_decl.name))?;
    let fail_name = flow
        .outputs
        .get(1)
        .map(|p| p.name.clone())
        .ok_or_else(|| format!("{}: flow `{}` has no fail output", file_path.display(), flow_decl.name))?;

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
pub fn transform_source_steps(stmts: &[Statement], source_names: &HashSet<String>) -> Vec<Statement> {
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

pub fn load_module(
    module_name: &str,
    module_dir: &Path,
    loading_set: &mut HashSet<String>,
    extra_ops: &HashSet<String>,
) -> Result<FlowRegistry, String> {
    if loading_set.contains(module_name) {
        return Err(format!(
            "circular module dependency detected: module '{}' is already being loaded",
            module_name
        ));
    }
    loading_set.insert(module_name.to_string());

    let entries = fs::read_dir(module_dir)
        .map_err(|e| format!("failed to read module directory {}: {e}", module_dir.display()))?;

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

        let type_registry = TypeRegistry::from_module(&module).map_err(|errors| {
            format!(
                "{}: type errors:\n{}",
                file.display(),
                errors.join("\n")
            )
        })?;

        // Process uses within the module file (nested modules)
        let mut sub_registry = FlowRegistry::new();
        for decl in &module.decls {
            if let TopDecl::Uses(uses) = decl {
                let sub_dir = module_dir.join(&uses.module);
                if !sub_dir.is_dir() {
                    return Err(format!(
                        "{}: module '{}' not found; expected directory '{}'",
                        file.display(),
                        uses.module,
                        sub_dir.display()
                    ));
                }
                let sub = load_module(&uses.module, &sub_dir, loading_set, extra_ops)?;
                for (name, program) in sub.flows {
                    sub_registry.insert(name, program);
                }
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
                    let program = compile_func_decl(f, &type_registry, file, &sub_registry, extra_ops, DeclKind::Func)?;
                    let qualified = format!("{}.{}", module_name, f.name);
                    registry.insert(qualified, program);
                }
                TopDecl::Sink(f) => {
                    let program = compile_func_decl(f, &type_registry, file, &sub_registry, extra_ops, DeclKind::Sink)?;
                    let qualified = format!("{}.{}", module_name, f.name);
                    registry.insert(qualified, program);
                }
                TopDecl::Source(f) => {
                    let program = compile_func_decl(f, &type_registry, file, &sub_registry, extra_ops, DeclKind::Source)?;
                    let qualified = format!("{}.{}", module_name, f.name);
                    registry.insert(qualified, program);
                }
                TopDecl::Flow(f) => {
                    let program = compile_flow_decl(f, &type_registry, file, &sub_registry, extra_ops)?;
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
            let module_dir = base_dir.join(&uses.module);
            if !module_dir.is_dir() {
                return Err(format!(
                    "module '{}' not found; expected directory '{}'",
                    uses.module,
                    module_dir.display()
                ));
            }
            let loaded = load_module(&uses.module, &module_dir, &mut loading_set, &extra_ops)?;
            for (name, program) in loaded.flows {
                registry.insert(name, program);
            }
        }
    }

    Ok(registry)
}
