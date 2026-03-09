use std::collections::HashMap;

use crate::ast::{Arg, Expr, FlowGraph, FlowStatement, PortDecl, Statement, TakeDecl};
use crate::loader::FlowRegistry;
use crate::op_types::{self, OpType};
use crate::types::{PrimitiveType, TypeDef, TypeRegistry};

#[derive(Debug, Clone, PartialEq, Eq)]
enum InferredType {
    Known(OpType),
    Unknown,
}

impl std::fmt::Display for InferredType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InferredType::Known(t) => write!(f, "{t}"),
            InferredType::Unknown => write!(f, "unknown"),
        }
    }
}

fn primitive_to_op_type(prim: &PrimitiveType) -> Option<OpType> {
    match prim {
        PrimitiveType::Text => Some(OpType::Text),
        PrimitiveType::Bool => Some(OpType::Bool),
        PrimitiveType::Long => Some(OpType::Long),
        PrimitiveType::Real => Some(OpType::Real),
        PrimitiveType::List => Some(OpType::List),
        PrimitiveType::Dict => Some(OpType::Dict),
        PrimitiveType::DbConn => Some(OpType::DbConn),
        PrimitiveType::HttpServer => Some(OpType::HttpServer),
        PrimitiveType::HttpConn => Some(OpType::HttpConn),
        PrimitiveType::WsConn => Some(OpType::WsConn),
        PrimitiveType::Uuid | PrimitiveType::Time => Some(OpType::Text),
        PrimitiveType::Void | PrimitiveType::Ptr => None,
    }
}

fn take_type_to_op_type(type_name: &str, registry: &TypeRegistry) -> InferredType {
    // Try hardcoded built-in primitives first
    match type_name {
        "text" => return InferredType::Known(OpType::Text),
        "bool" => return InferredType::Known(OpType::Bool),
        "long" => return InferredType::Known(OpType::Long),
        "real" => return InferredType::Known(OpType::Real),
        "list" => return InferredType::Known(OpType::List),
        "dict" => return InferredType::Known(OpType::Dict),
        "db_conn" => return InferredType::Known(OpType::DbConn),
        "http_server" => return InferredType::Known(OpType::HttpServer),
        "http_conn" => return InferredType::Known(OpType::HttpConn),
        "ws_conn" => return InferredType::Known(OpType::WsConn),
        _ => {}
    }

    // Look up in registry for user-defined and built-in struct types
    match registry.get(type_name) {
        Some(TypeDef::Struct { .. }) => InferredType::Known(OpType::Struct(type_name.to_string())),
        Some(TypeDef::Scalar { base, .. }) => match primitive_to_op_type(base) {
            Some(op) => InferredType::Known(op),
            None => InferredType::Unknown,
        },
        Some(TypeDef::Enum { .. }) => InferredType::Known(OpType::Text),
        Some(TypeDef::Primitive(prim)) => match primitive_to_op_type(prim) {
            Some(op) => InferredType::Known(op),
            None => InferredType::Unknown,
        },
        None => InferredType::Unknown,
    }
}

/// Reconcile an optional type annotation with an inferred type.
/// - If annotation is present and inferred is Unknown → use annotation type
/// - If both are Known and incompatible → error, use annotation type
/// - If both are Known and compatible, or no annotation → use inferred
fn resolve_annotation(
    func_name: &str,
    bind: &str,
    annotation: &Option<String>,
    inferred: &InferredType,
    registry: &TypeRegistry,
    errors: &mut Vec<String>,
) -> InferredType {
    let ann_type = match annotation {
        Some(type_name) => take_type_to_op_type(type_name, registry),
        None => return inferred.clone(),
    };

    match (&ann_type, inferred) {
        (InferredType::Known(ann_op), InferredType::Known(inf_op)) => {
            if !op_types::types_compatible(ann_op, inf_op) {
                errors.push(format!(
                    "func '{}': variable '{}' annotated as '{}' but expression has type '{}'",
                    func_name, bind, ann_op, inf_op
                ));
            }
            // Use annotation type (it's what the user declared)
            ann_type
        }
        (InferredType::Known(_), InferredType::Unknown) => {
            // Annotation provides type info that inference couldn't determine
            ann_type
        }
        _ => {
            // Annotation resolved to Unknown — use whatever we inferred
            inferred.clone()
        }
    }
}

fn infer_expr_type(
    expr: &Expr,
    var_types: &HashMap<String, InferredType>,
    registry: &TypeRegistry,
) -> InferredType {
    infer_expr_type_with_flows(expr, var_types, &HashMap::new(), registry, None)
}

fn infer_expr_type_with_flows(
    expr: &Expr,
    var_types: &HashMap<String, InferredType>,
    list_elem_types: &HashMap<String, InferredType>,
    registry: &TypeRegistry,
    flow_registry: Option<&FlowRegistry>,
) -> InferredType {
    match expr {
        Expr::Var(name) => {
            if let Some(t) = var_types.get(name) {
                return t.clone();
            }
            // Handle dotted variable names like "out.stderr" → base "out", field "stderr"
            if let Some(dot_pos) = name.find('.') {
                let base = &name[..dot_pos];
                let field = &name[dot_pos + 1..];
                if let Some(InferredType::Known(OpType::Struct(struct_name))) = var_types.get(base)
                {
                    return infer_struct_field_type(struct_name, field, registry);
                }
            }
            InferredType::Unknown
        }
        Expr::Lit(v) => {
            if v.is_string() {
                InferredType::Known(OpType::Text)
            } else if v.is_boolean() {
                InferredType::Known(OpType::Bool)
            } else if v.is_i64() {
                InferredType::Known(OpType::Long)
            } else if v.is_f64() {
                InferredType::Known(OpType::Real)
            } else {
                InferredType::Unknown
            }
        }
        Expr::Call { func, .. } => {
            if let Some(sig) = op_types::op_signature(func) {
                InferredType::Known(sig.returns)
            } else {
                infer_callee_return_type(func, flow_registry, registry)
            }
        }
        Expr::BinOp { op, lhs, rhs } => infer_binop_type(op, lhs, rhs, var_types, registry),
        Expr::UnaryOp { op, expr: inner } => match op {
            crate::ast::UnaryOp::Not => InferredType::Known(OpType::Bool),
            crate::ast::UnaryOp::Neg => {
                let inner_type = infer_expr_type_with_flows(
                    inner,
                    var_types,
                    list_elem_types,
                    registry,
                    flow_registry,
                );
                match inner_type {
                    InferredType::Known(OpType::Long) => InferredType::Known(OpType::Long),
                    InferredType::Known(OpType::Real) => InferredType::Known(OpType::Real),
                    _ => InferredType::Unknown,
                }
            }
        },
        Expr::Ternary {
            then_expr,
            else_expr,
            ..
        } => {
            let then_type = infer_expr_type_with_flows(
                then_expr,
                var_types,
                list_elem_types,
                registry,
                flow_registry,
            );
            if matches!(then_type, InferredType::Known(_)) {
                then_type
            } else {
                infer_expr_type_with_flows(
                    else_expr,
                    var_types,
                    list_elem_types,
                    registry,
                    flow_registry,
                )
            }
        }
        Expr::Index { expr: base, index } => {
            let base_type = infer_expr_type_with_flows(
                base,
                var_types,
                list_elem_types,
                registry,
                flow_registry,
            );
            // Field access on a struct with a string key — look up the field type
            if let InferredType::Known(OpType::Struct(ref struct_name)) = base_type {
                if let Expr::Lit(v) = index.as_ref() {
                    if let Some(field_name) = v.as_str() {
                        return infer_struct_field_type(struct_name, field_name, registry);
                    }
                }
            }
            // List element access by integer index — look up element type
            if let InferredType::Known(OpType::List) = base_type {
                if let Expr::Var(name) = base.as_ref() {
                    if let Some(elem_type) = list_elem_types.get(name) {
                        return elem_type.clone();
                    }
                }
            }
            InferredType::Unknown
        }
        Expr::ListLit(_) => InferredType::Known(OpType::List),
        Expr::DictLit(_) => InferredType::Known(OpType::Dict),
        Expr::Interp(_) => InferredType::Known(OpType::Text),
        Expr::Coalesce { lhs, rhs } => {
            let lhs_type = infer_expr_type_with_flows(
                lhs,
                var_types,
                list_elem_types,
                registry,
                flow_registry,
            );
            if matches!(lhs_type, InferredType::Known(_)) {
                lhs_type
            } else {
                infer_expr_type_with_flows(rhs, var_types, list_elem_types, registry, flow_registry)
            }
        }
    }
}

fn infer_binop_type(
    op: &crate::ast::BinOp,
    lhs: &Expr,
    rhs: &Expr,
    var_types: &HashMap<String, InferredType>,
    registry: &TypeRegistry,
) -> InferredType {
    use crate::ast::BinOp as B;
    match op {
        // Comparison and logical operators always return Bool
        B::Eq | B::Neq | B::Lt | B::Gt | B::LtEq | B::GtEq | B::And | B::Or => {
            InferredType::Known(OpType::Bool)
        }
        // Add: text + anything = text (string concatenation), else numeric
        B::Add => {
            let lt = infer_expr_type(lhs, var_types, registry);
            let rt = infer_expr_type(rhs, var_types, registry);
            if matches!(lt, InferredType::Known(OpType::Text))
                || matches!(rt, InferredType::Known(OpType::Text))
            {
                InferredType::Known(OpType::Text)
            } else {
                infer_numeric_result(&lt, &rt)
            }
        }
        // Arithmetic operators: numeric result
        B::Sub | B::Mul | B::Div | B::Mod | B::Pow => {
            let lt = infer_expr_type(lhs, var_types, registry);
            let rt = infer_expr_type(rhs, var_types, registry);
            infer_numeric_result(&lt, &rt)
        }
    }
}

fn infer_numeric_result(lhs: &InferredType, rhs: &InferredType) -> InferredType {
    match (lhs, rhs) {
        (InferredType::Known(OpType::Real), _) | (_, InferredType::Known(OpType::Real)) => {
            InferredType::Known(OpType::Real)
        }
        (InferredType::Known(OpType::Long), InferredType::Known(OpType::Long)) => {
            InferredType::Known(OpType::Long)
        }
        _ => InferredType::Unknown,
    }
}

/// Look up a cross-module callee's return type from the FlowRegistry.
/// Returns the emit port type if the callee has exactly one emit port,
/// or Unknown if not found or ambiguous.
fn infer_callee_return_type(
    callee: &str,
    flow_registry: Option<&FlowRegistry>,
    registry: &TypeRegistry,
) -> InferredType {
    if let Some(fr) = flow_registry {
        if let Some(program) = fr.get(callee) {
            // Use emit_name to find the primary output port's type
            if let Some(emit_name) = &program.emit_name {
                if let Some(port) = program.flow.outputs.iter().find(|p| p.name == *emit_name) {
                    return take_type_to_op_type(&port.type_name, registry);
                }
            }
        }
    }
    InferredType::Unknown
}

fn infer_struct_field_type(
    struct_name: &str,
    field_name: &str,
    registry: &TypeRegistry,
) -> InferredType {
    // Check built-in struct field types first
    match (struct_name, field_name) {
        ("ProcessOutput", "stdout" | "stderr") => return InferredType::Known(OpType::Text),
        ("ProcessOutput", "ok") => return InferredType::Known(OpType::Bool),
        ("ProcessOutput", "code") => return InferredType::Known(OpType::Long),
        ("HttpRequest", "method" | "path" | "body") => return InferredType::Known(OpType::Text),
        ("HttpRequest", "headers" | "params") => return InferredType::Known(OpType::Dict),
        ("HttpResponse", "status") => return InferredType::Known(OpType::Long),
        ("HttpResponse", "body") => return InferredType::Known(OpType::Text),
        ("HttpResponse", "headers") => return InferredType::Known(OpType::Dict),
        ("HttpResponse", "ok") => return InferredType::Known(OpType::Bool),
        ("Date", "year" | "month" | "day") => return InferredType::Known(OpType::Long),
        ("Stamp", "ns") => return InferredType::Known(OpType::Long),
        ("TimeRange", "start" | "end") => {
            return InferredType::Known(OpType::Struct("Stamp".to_string()));
        }
        ("WebSocketMessage", "text") => return InferredType::Known(OpType::Text),
        ("WebSocketMessage", "binary") => return InferredType::Known(OpType::List),
        ("ErrorObject", "code" | "message") => return InferredType::Known(OpType::Text),
        ("URLParts", "scheme" | "host" | "path" | "query" | "fragment") => {
            return InferredType::Known(OpType::Text);
        }
        ("URLParts", "port") => return InferredType::Known(OpType::Long),
        _ => {}
    }

    // Check user-defined struct fields in registry
    if let Some(crate::types::TypeDef::Struct { fields, .. }) = registry.get(struct_name) {
        if let Some(field) = fields.iter().find(|f| f.name == field_name) {
            return take_type_to_op_type(&field.type_ref, registry);
        }
    }

    InferredType::Unknown
}

/// Returns true if the expression is a string literal (not a variable).
fn is_string_literal(expr: &Expr) -> bool {
    matches!(expr, Expr::Lit(v) if v.is_string())
}

fn is_handle_type(t: &OpType) -> bool {
    matches!(
        t,
        OpType::DbConn | OpType::HttpServer | OpType::HttpConn | OpType::WsConn
    )
}

fn unwrap_optional(t: &OpType) -> &OpType {
    match t {
        OpType::Optional(inner) => inner,
        other => other,
    }
}

fn check_call_args(
    func_name: &str,
    op: &str,
    args: &[Expr],
    var_types: &HashMap<String, InferredType>,
    registry: &TypeRegistry,
    errors: &mut Vec<String>,
) {
    let Some(sig) = op_types::op_signature(op) else {
        return;
    };

    for (i, expected_type) in sig.args.iter().enumerate() {
        let Some(arg_expr) = args.get(i) else {
            // Missing optional args are fine
            if matches!(expected_type, OpType::Optional(_)) {
                continue;
            }
            break;
        };

        let base_expected = unwrap_optional(expected_type);

        // Reject string literals where handle type is expected
        if is_handle_type(base_expected) && is_string_literal(arg_expr) {
            errors.push(format!(
                "func '{}' op '{}' arg {}: expected '{}', got string literal — \
                 handles cannot be constructed from string literals",
                func_name, op, i, base_expected
            ));
            continue;
        }

        let actual = infer_expr_type(arg_expr, var_types, registry);
        if let InferredType::Known(actual_type) = &actual {
            if !op_types::types_compatible(expected_type, actual_type) {
                let var_hint = if let Expr::Var(name) = arg_expr {
                    format!(" (variable '{name}')")
                } else {
                    String::new()
                };
                errors.push(format!(
                    "func '{}' op '{}' arg {}: expected '{}', got '{}'{}",
                    func_name, op, i, base_expected, actual_type, var_hint
                ));
            }
        }
        // Unknown types pass through — no error
    }
}

fn check_node_args(
    func_name: &str,
    op: &str,
    args: &[Arg],
    var_types: &HashMap<String, InferredType>,
    _registry: &TypeRegistry,
    errors: &mut Vec<String>,
) {
    let Some(sig) = op_types::op_signature(op) else {
        return;
    };

    for (i, expected_type) in sig.args.iter().enumerate() {
        let Some(arg) = args.get(i) else {
            if matches!(expected_type, OpType::Optional(_)) {
                continue;
            }
            break;
        };

        let base_expected = unwrap_optional(expected_type);

        match arg {
            Arg::Lit { lit } => {
                // Reject string literal where handle type is expected
                if is_handle_type(base_expected) {
                    errors.push(format!(
                        "func '{}' op '{}' arg {}: expected '{}', got literal — \
                         handles cannot be constructed from literals",
                        func_name, op, i, base_expected
                    ));
                    continue;
                }
                // Check literal type
                let actual = if lit.is_string() {
                    Some(OpType::Text)
                } else if lit.is_boolean() {
                    Some(OpType::Bool)
                } else if lit.is_i64() {
                    Some(OpType::Long)
                } else if lit.is_f64() {
                    Some(OpType::Real)
                } else {
                    None
                };
                if let Some(actual_type) = actual {
                    if !op_types::types_compatible(expected_type, &actual_type) {
                        errors.push(format!(
                            "func '{}' op '{}' arg {}: expected '{}', got '{}'",
                            func_name, op, i, base_expected, actual_type
                        ));
                    }
                }
            }
            Arg::Var { var } => {
                if let Some(InferredType::Known(actual_type)) = var_types.get(var.as_str()) {
                    if !op_types::types_compatible(expected_type, actual_type) {
                        errors.push(format!(
                            "func '{}' op '{}' arg {}: expected '{}', got '{}' (variable '{}')",
                            func_name, op, i, base_expected, actual_type, var
                        ));
                    }
                }
                // Unknown → pass through
            }
        }
    }
}

fn check_statements(
    func_name: &str,
    stmts: &[Statement],
    var_types: &mut HashMap<String, InferredType>,
    list_elem_types: &mut HashMap<String, InferredType>,
    emit_port_types: &HashMap<String, InferredType>,
    registry: &TypeRegistry,
    flow_registry: Option<&FlowRegistry>,
    errors: &mut Vec<String>,
) {
    for stmt in stmts {
        match stmt {
            Statement::Node(n) => {
                check_node_args(func_name, &n.op, &n.args, var_types, registry, errors);
                // Infer the bind variable's type from the op's return type
                let inferred = if let Some(sig) = op_types::op_signature(&n.op) {
                    InferredType::Known(sig.returns)
                } else {
                    infer_callee_return_type(&n.op, flow_registry, registry)
                };
                let final_type = resolve_annotation(
                    func_name,
                    &n.bind,
                    &n.type_annotation,
                    &inferred,
                    registry,
                    errors,
                );
                // Track list element types for ops that return lists
                infer_list_elem_type_from_node(
                    &n.op,
                    &n.args,
                    &n.bind,
                    var_types,
                    list_elem_types,
                    registry,
                );
                var_types.insert(n.bind.clone(), final_type);
            }
            Statement::ExprAssign(ea) => {
                // Check any op calls inside the expression
                check_expr_calls(func_name, &ea.expr, var_types, registry, errors);
                let inferred = infer_expr_type_with_flows(
                    &ea.expr,
                    var_types,
                    list_elem_types,
                    registry,
                    flow_registry,
                );
                let final_type = resolve_annotation(
                    func_name,
                    &ea.bind,
                    &ea.type_annotation,
                    &inferred,
                    registry,
                    errors,
                );
                // Track list element types from expressions
                infer_list_elem_type_from_expr(
                    &ea.expr,
                    &ea.bind,
                    var_types,
                    list_elem_types,
                    registry,
                );
                var_types.insert(ea.bind.clone(), final_type);
            }
            Statement::Case(c) => {
                for arm in &c.arms {
                    let mut arm_scope = var_types.clone();
                    let mut arm_list_elems = list_elem_types.clone();
                    check_statements(
                        func_name,
                        &arm.body,
                        &mut arm_scope,
                        &mut arm_list_elems,
                        emit_port_types,
                        registry,
                        flow_registry,
                        errors,
                    );
                    // Merge back assignments that exist in outer scope
                    for (k, v) in &arm_scope {
                        if var_types.contains_key(k) {
                            var_types.insert(k.clone(), v.clone());
                        }
                    }
                    for (k, v) in &arm_list_elems {
                        if list_elem_types.contains_key(k) {
                            list_elem_types.insert(k.clone(), v.clone());
                        }
                    }
                }
                if !c.else_body.is_empty() {
                    let mut else_scope = var_types.clone();
                    let mut else_list_elems = list_elem_types.clone();
                    check_statements(
                        func_name,
                        &c.else_body,
                        &mut else_scope,
                        &mut else_list_elems,
                        emit_port_types,
                        registry,
                        flow_registry,
                        errors,
                    );
                    for (k, v) in &else_scope {
                        if var_types.contains_key(k) {
                            var_types.insert(k.clone(), v.clone());
                        }
                    }
                    for (k, v) in &else_list_elems {
                        if list_elem_types.contains_key(k) {
                            list_elem_types.insert(k.clone(), v.clone());
                        }
                    }
                }
            }
            Statement::Loop(l) => {
                let mut loop_scope = var_types.clone();
                // Infer loop item type from collection's element type
                let item_type = match &l.collection {
                    Expr::Var(name) => list_elem_types
                        .get(name)
                        .cloned()
                        .unwrap_or(InferredType::Unknown),
                    _ => InferredType::Unknown,
                };
                loop_scope.insert(l.item.clone(), item_type);
                if let Some(idx) = &l.index {
                    loop_scope.insert(idx.clone(), InferredType::Known(OpType::Long));
                }
                let mut loop_list_elems = list_elem_types.clone();
                check_statements(
                    func_name,
                    &l.body,
                    &mut loop_scope,
                    &mut loop_list_elems,
                    emit_port_types,
                    registry,
                    flow_registry,
                    errors,
                );
                // Merge back
                for (k, v) in &loop_scope {
                    if var_types.contains_key(k) {
                        var_types.insert(k.clone(), v.clone());
                    }
                }
            }
            Statement::Sync(s) => {
                check_statements(
                    func_name,
                    &s.body,
                    var_types,
                    list_elem_types,
                    emit_port_types,
                    registry,
                    flow_registry,
                    errors,
                );
            }
            Statement::BareLoop(b) => {
                let mut loop_scope = var_types.clone();
                let mut loop_list_elems = list_elem_types.clone();
                check_statements(
                    func_name,
                    &b.body,
                    &mut loop_scope,
                    &mut loop_list_elems,
                    emit_port_types,
                    registry,
                    flow_registry,
                    errors,
                );
                for (k, v) in &loop_scope {
                    if var_types.contains_key(k) {
                        var_types.insert(k.clone(), v.clone());
                    }
                }
            }
            Statement::SourceLoop(sl) => {
                check_node_args(
                    func_name,
                    &sl.source_op,
                    &sl.source_args,
                    var_types,
                    registry,
                    errors,
                );
                if let Some(sig) = op_types::op_signature(&sl.source_op) {
                    var_types.insert(sl.bind.clone(), InferredType::Known(sig.returns));
                } else {
                    var_types.insert(sl.bind.clone(), InferredType::Unknown);
                }
                let mut loop_scope = var_types.clone();
                let mut loop_list_elems = list_elem_types.clone();
                check_statements(
                    func_name,
                    &sl.body,
                    &mut loop_scope,
                    &mut loop_list_elems,
                    emit_port_types,
                    registry,
                    flow_registry,
                    errors,
                );
            }
            Statement::On(on_block) => {
                check_node_args(
                    func_name,
                    &on_block.source_op,
                    &on_block.source_args,
                    var_types,
                    registry,
                    errors,
                );
                if let Some(sig) = op_types::op_signature(&on_block.source_op) {
                    var_types.insert(on_block.bind.clone(), InferredType::Known(sig.returns));
                } else {
                    var_types.insert(on_block.bind.clone(), InferredType::Unknown);
                }
                let mut on_scope = var_types.clone();
                let mut on_list_elems = list_elem_types.clone();
                check_statements(
                    func_name,
                    &on_block.body,
                    &mut on_scope,
                    &mut on_list_elems,
                    emit_port_types,
                    registry,
                    flow_registry,
                    errors,
                );
            }
            Statement::Emit(emit) => {
                check_expr_calls(func_name, &emit.value_expr, var_types, registry, errors);
                // Check emit value type against declared port type
                if let Some(port_type) = emit_port_types.get(&emit.output) {
                    let value_type = infer_expr_type_with_flows(
                        &emit.value_expr,
                        var_types,
                        list_elem_types,
                        registry,
                        flow_registry,
                    );
                    match (port_type, &value_type) {
                        (InferredType::Known(port_op), InferredType::Known(val_op)) => {
                            if !op_types::types_compatible(port_op, val_op) {
                                errors.push(format!(
                                    "func '{}' emit '{}': expected type '{}', got '{}'",
                                    func_name, emit.output, port_op, val_op
                                ));
                            }
                        }
                        (InferredType::Known(_), InferredType::Unknown) => {
                            // Unknown type (e.g. cross-module call result) — allow through;
                            // we can't resolve the return type at compile time.
                        }
                        _ => {}
                    }
                }
            }
            Statement::Break | Statement::Continue | Statement::SendNowait(_) => {}
        }
    }
}

/// Infer list element type from a Node assignment (bind = op(args)).
fn infer_list_elem_type_from_node(
    op: &str,
    args: &[Arg],
    bind: &str,
    var_types: &HashMap<String, InferredType>,
    list_elem_types: &mut HashMap<String, InferredType>,
    registry: &TypeRegistry,
) {
    match op {
        // Ops that return List with known element types
        "str.split" | "regex.split" | "regex.find_all" | "file.list" | "obj.keys" => {
            list_elem_types.insert(bind.to_string(), InferredType::Known(OpType::Text));
        }
        "list.range" | "list.indices" => {
            list_elem_types.insert(bind.to_string(), InferredType::Known(OpType::Long));
        }
        "list.append" => {
            // list.append(list, val) → element type from val
            if let Some(Arg::Var { var }) = args.get(1) {
                if let Some(t) = var_types.get(var) {
                    list_elem_types.insert(bind.to_string(), t.clone());
                }
            } else if let Some(Arg::Lit { lit }) = args.get(1) {
                let elem_type = if lit.is_string() {
                    InferredType::Known(OpType::Text)
                } else if lit.is_boolean() {
                    InferredType::Known(OpType::Bool)
                } else if lit.is_i64() {
                    InferredType::Known(OpType::Long)
                } else if lit.is_f64() {
                    InferredType::Known(OpType::Real)
                } else {
                    InferredType::Unknown
                };
                list_elem_types.insert(bind.to_string(), elem_type);
            }
        }
        "list.slice" | "random.shuffle" => {
            // Inherit element type from source list arg
            if let Some(Arg::Var { var }) = args.first() {
                if let Some(t) = list_elem_types.get(var) {
                    list_elem_types.insert(bind.to_string(), t.clone());
                }
            }
        }
        "db.query" => {
            list_elem_types.insert(bind.to_string(), InferredType::Known(OpType::Dict));
        }
        _ => {
            // For any other op returning List, check if we can propagate from first arg
            if let Some(sig) = op_types::op_signature(op) {
                if matches!(sig.returns, OpType::List) {
                    if let Some(Arg::Var { var }) = args.first() {
                        if let Some(t) = list_elem_types.get(var) {
                            list_elem_types.insert(bind.to_string(), t.clone());
                        }
                    }
                }
            }
        }
    }
    let _ = registry; // suppress unused warning
}

/// Infer list element type from an ExprAssign expression.
fn infer_list_elem_type_from_expr(
    expr: &Expr,
    bind: &str,
    var_types: &HashMap<String, InferredType>,
    list_elem_types: &mut HashMap<String, InferredType>,
    registry: &TypeRegistry,
) {
    match expr {
        Expr::ListLit(items) => {
            // Infer element type from first item
            if let Some(first) = items.first() {
                let elem_type = infer_expr_type(first, var_types, registry);
                if matches!(elem_type, InferredType::Known(_)) {
                    list_elem_types.insert(bind.to_string(), elem_type);
                }
            }
        }
        Expr::Call { func, args, .. } => {
            // Delegate to the same logic as Node assignments
            let node_args: Vec<Arg> = args
                .iter()
                .map(|e| match e {
                    Expr::Var(v) => Arg::Var { var: v.clone() },
                    Expr::Lit(l) => Arg::Lit { lit: l.clone() },
                    _ => Arg::Lit {
                        lit: serde_json::Value::Null,
                    },
                })
                .collect();
            infer_list_elem_type_from_node(
                func,
                &node_args,
                bind,
                var_types,
                list_elem_types,
                registry,
            );
        }
        _ => {}
    }
}

fn check_expr_calls(
    func_name: &str,
    expr: &Expr,
    var_types: &HashMap<String, InferredType>,
    registry: &TypeRegistry,
    errors: &mut Vec<String>,
) {
    match expr {
        Expr::Call { func, args, .. } => {
            check_call_args(func_name, func, args, var_types, registry, errors);
            for arg in args {
                check_expr_calls(func_name, arg, var_types, registry, errors);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            check_expr_calls(func_name, lhs, var_types, registry, errors);
            check_expr_calls(func_name, rhs, var_types, registry, errors);
        }
        Expr::UnaryOp { expr: inner, .. } => {
            check_expr_calls(func_name, inner, var_types, registry, errors);
        }
        Expr::Ternary {
            cond,
            then_expr,
            else_expr,
        } => {
            check_expr_calls(func_name, cond, var_types, registry, errors);
            check_expr_calls(func_name, then_expr, var_types, registry, errors);
            check_expr_calls(func_name, else_expr, var_types, registry, errors);
        }
        Expr::ListLit(items) => {
            for item in items {
                check_expr_calls(func_name, item, var_types, registry, errors);
            }
        }
        Expr::DictLit(pairs) => {
            for (_, v) in pairs {
                check_expr_calls(func_name, v, var_types, registry, errors);
            }
        }
        Expr::Interp(parts) => {
            for part in parts {
                if let crate::ast::InterpExpr::Expr(e) = part {
                    check_expr_calls(func_name, e, var_types, registry, errors);
                }
            }
        }
        Expr::Index { expr, index } => {
            check_expr_calls(func_name, expr, var_types, registry, errors);
            check_expr_calls(func_name, index, var_types, registry, errors);
        }
        Expr::Coalesce { lhs, rhs } => {
            check_expr_calls(func_name, lhs, var_types, registry, errors);
            check_expr_calls(func_name, rhs, var_types, registry, errors);
        }
        Expr::Var(_) | Expr::Lit(_) => {}
    }
}

pub fn typecheck_func(
    func_name: &str,
    takes: &[TakeDecl],
    body: &[Statement],
    emits: &[PortDecl],
    fails: &[PortDecl],
    registry: &TypeRegistry,
) -> Result<(), String> {
    typecheck_func_with_flows(func_name, takes, body, emits, fails, registry, None)
}

pub fn typecheck_func_with_flows(
    func_name: &str,
    takes: &[TakeDecl],
    body: &[Statement],
    emits: &[PortDecl],
    fails: &[PortDecl],
    registry: &TypeRegistry,
    flow_registry: Option<&FlowRegistry>,
) -> Result<(), String> {
    let mut var_types: HashMap<String, InferredType> = HashMap::new();

    // Seed from take declarations
    for take in takes {
        var_types.insert(
            take.name.clone(),
            take_type_to_op_type(&take.type_name, registry),
        );
    }

    // Build emit/fail port type map for checking emit statements
    let mut emit_port_types: HashMap<String, InferredType> = HashMap::new();
    for port in emits {
        emit_port_types.insert(
            port.name.clone(),
            take_type_to_op_type(&port.type_name, registry),
        );
    }
    for port in fails {
        emit_port_types.insert(
            port.name.clone(),
            take_type_to_op_type(&port.type_name, registry),
        );
    }

    let mut errors = Vec::new();

    // Validate all port types resolve to Known
    for take in takes {
        if matches!(
            take_type_to_op_type(&take.type_name, registry),
            InferredType::Unknown
        ) {
            errors.push(format!(
                "func '{}': take '{}' has unknown type '{}'",
                func_name, take.name, take.type_name
            ));
        }
    }
    for port in emits {
        if matches!(
            take_type_to_op_type(&port.type_name, registry),
            InferredType::Unknown
        ) {
            errors.push(format!(
                "func '{}': emit '{}' has unknown type '{}'",
                func_name, port.name, port.type_name
            ));
        }
    }
    for port in fails {
        if matches!(
            take_type_to_op_type(&port.type_name, registry),
            InferredType::Unknown
        ) {
            errors.push(format!(
                "func '{}': fail '{}' has unknown type '{}'",
                func_name, port.name, port.type_name
            ));
        }
    }

    let mut list_elem_types: HashMap<String, InferredType> = HashMap::new();
    check_statements(
        func_name,
        body,
        &mut var_types,
        &mut list_elem_types,
        &emit_port_types,
        registry,
        flow_registry,
        &mut errors,
    );

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

/// Type-check a flow graph's step wiring against the flow registry.
/// For each step, checks that the input wire types match the callee's declared input ports.
/// Unknown callees (not in the registry) are silently skipped.
pub fn typecheck_flow(
    flow_name: &str,
    flow_graph: &FlowGraph,
    registry: &TypeRegistry,
    flow_registry: &FlowRegistry,
) -> Result<(), String> {
    let mut scope: HashMap<String, InferredType> = HashMap::new();

    // Seed scope from flow's own input ports
    for port in &flow_graph.inputs {
        scope.insert(
            port.name.clone(),
            take_type_to_op_type(&port.type_name, registry),
        );
    }

    let mut errors = Vec::new();

    // Validate all flow port types resolve to Known
    for port in &flow_graph.inputs {
        if matches!(
            take_type_to_op_type(&port.type_name, registry),
            InferredType::Unknown
        ) {
            errors.push(format!(
                "flow '{}': input '{}' has unknown type '{}'",
                flow_name, port.name, port.type_name
            ));
        }
    }
    for port in &flow_graph.emit_ports {
        if matches!(
            take_type_to_op_type(&port.type_name, registry),
            InferredType::Unknown
        ) {
            errors.push(format!(
                "flow '{}': emit '{}' has unknown type '{}'",
                flow_name, port.name, port.type_name
            ));
        }
    }
    for port in &flow_graph.fail_ports {
        if matches!(
            take_type_to_op_type(&port.type_name, registry),
            InferredType::Unknown
        ) {
            errors.push(format!(
                "flow '{}': fail '{}' has unknown type '{}'",
                flow_name, port.name, port.type_name
            ));
        }
    }

    check_flow_statements(
        flow_name,
        &flow_graph.body,
        &mut scope,
        registry,
        flow_registry,
        &mut errors,
    );

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

fn check_flow_statements(
    flow_name: &str,
    stmts: &[FlowStatement],
    scope: &mut HashMap<String, InferredType>,
    registry: &TypeRegistry,
    flow_registry: &FlowRegistry,
    errors: &mut Vec<String>,
) {
    for stmt in stmts {
        match stmt {
            FlowStatement::Step(step) => {
                // Look up callee in flow registry
                if let Some(program) = flow_registry.get(&step.callee) {
                    let callee_inputs = &program.flow.inputs;

                    // Check each input mapping against callee's declared input ports
                    for mapping in &step.inputs {
                        // Find the callee port matching this mapping
                        if let Some(callee_port) =
                            callee_inputs.iter().find(|p| p.name == mapping.port)
                        {
                            let expected = take_type_to_op_type(&callee_port.type_name, registry);
                            let actual = match &mapping.value {
                                Arg::Var { var } => {
                                    scope.get(var).cloned().unwrap_or(InferredType::Unknown)
                                }
                                Arg::Lit { lit } => {
                                    if lit.is_string() {
                                        InferredType::Known(OpType::Text)
                                    } else if lit.is_boolean() {
                                        InferredType::Known(OpType::Bool)
                                    } else if lit.is_i64() {
                                        InferredType::Known(OpType::Long)
                                    } else if lit.is_f64() {
                                        InferredType::Known(OpType::Real)
                                    } else {
                                        InferredType::Unknown
                                    }
                                }
                            };
                            match (&expected, &actual) {
                                (InferredType::Known(exp_op), InferredType::Known(act_op)) => {
                                    if !op_types::types_compatible(exp_op, act_op) {
                                        errors.push(format!(
                                            "flow '{}' step '{}': port '{}' expected type '{}', got '{}'",
                                            flow_name, step.callee, mapping.port, exp_op, act_op
                                        ));
                                    }
                                }
                                (InferredType::Known(exp_op), InferredType::Unknown) => {
                                    errors.push(format!(
                                        "flow '{}' step '{}': port '{}' requires '{}' but wire has unknown type",
                                        flow_name, step.callee, mapping.port, exp_op
                                    ));
                                }
                                _ => {}
                            }
                        }
                    }

                    // Record output types from callee into scope
                    // The step's then_body wires (Next items) connect output ports to scope variables
                    for then_item in &step.then_body {
                        if let crate::ast::StepThenItem::Next(next) = then_item {
                            // next.port is the output port name, next.wire is the scope variable
                            if let Some(out_port) =
                                program.flow.outputs.iter().find(|p| p.name == next.port)
                            {
                                scope.insert(
                                    next.wire.clone(),
                                    take_type_to_op_type(&out_port.type_name, registry),
                                );
                            }
                        }
                    }
                }
                // Unknown callee: skip (don't error)
            }
            FlowStatement::Branch(branch) => {
                let mut branch_scope = scope.clone();
                check_flow_statements(
                    flow_name,
                    &branch.body,
                    &mut branch_scope,
                    registry,
                    flow_registry,
                    errors,
                );
                // Merge back
                for (k, v) in &branch_scope {
                    if scope.contains_key(k) {
                        scope.insert(k.clone(), v.clone());
                    }
                }
            }
            FlowStatement::Choose(choose) => {
                for branch in &choose.branches {
                    let mut branch_scope = scope.clone();
                    check_flow_statements(
                        flow_name,
                        &branch.body,
                        &mut branch_scope,
                        registry,
                        flow_registry,
                        errors,
                    );
                    // Merge back
                    for (k, v) in &branch_scope {
                        if scope.contains_key(k) {
                            scope.insert(k.clone(), v.clone());
                        }
                    }
                }
            }
            FlowStatement::State(state) => {
                if state.value.is_some() {
                    scope.insert(state.bind.clone(), InferredType::Unknown);
                } else if let Some(sig) = op_types::op_signature(&state.callee) {
                    scope.insert(state.bind.clone(), InferredType::Known(sig.returns));
                } else {
                    scope.insert(state.bind.clone(), InferredType::Unknown);
                }
            }
            FlowStatement::Local(local) => {
                if local.value.is_some() {
                    scope.insert(local.bind.clone(), InferredType::Unknown);
                } else if let Some(sig) = op_types::op_signature(&local.callee) {
                    scope.insert(local.bind.clone(), InferredType::Known(sig.returns));
                } else {
                    scope.insert(local.bind.clone(), InferredType::Unknown);
                }
            }
            _ => {} // Emit, Fail, SendNowait, Log — no wiring to check
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Arg, Emit, ExprAssign, NodeAssign, Span, Statement, TakeDecl};
    use serde_json::json;

    fn span() -> Span {
        Span { line: 1, col: 1 }
    }

    fn take(name: &str, type_name: &str) -> TakeDecl {
        TakeDecl {
            name: name.to_string(),
            type_name: type_name.to_string(),
            span: span(),
        }
    }

    fn node(bind: &str, op: &str, args: Vec<Arg>) -> Statement {
        Statement::Node(NodeAssign {
            bind: bind.to_string(),
            node_id: format!("n_{bind}"),
            op: op.to_string(),
            args,
            type_annotation: None,
        })
    }

    fn var_arg(name: &str) -> Arg {
        Arg::Var {
            var: name.to_string(),
        }
    }

    fn lit_arg(val: serde_json::Value) -> Arg {
        Arg::Lit { lit: val }
    }

    fn expr_assign(bind: &str, expr: Expr) -> Statement {
        Statement::ExprAssign(ExprAssign {
            bind: bind.to_string(),
            type_annotation: None,
            expr,
        })
    }

    fn emit(output: &str, var: &str) -> Statement {
        Statement::Emit(Emit {
            output: output.to_string(),
            value_expr: Expr::Var(var.to_string()),
        })
    }

    #[test]
    fn happy_path_db_conn() {
        let takes = vec![take("conn", "db_conn")];
        let body = vec![
            node(
                "result",
                "db.exec",
                vec![var_arg("conn"), lit_arg(json!("CREATE TABLE t(id)"))],
            ),
            emit("response", "result"),
        ];
        assert!(
            typecheck_func("TestFunc", &takes, &body, &[], &[], &TypeRegistry::empty()).is_ok()
        );
    }

    #[test]
    fn type_mismatch_text_for_db_conn() {
        let takes = vec![take("name", "text")];
        let body = vec![node(
            "result",
            "db.exec",
            vec![var_arg("name"), lit_arg(json!("SELECT 1"))],
        )];
        let err = typecheck_func("TestFunc", &takes, &body, &[], &[], &TypeRegistry::empty())
            .unwrap_err();
        assert!(err.contains("expected 'db_conn'"), "got: {err}");
        assert!(err.contains("got 'text'"), "got: {err}");
    }

    #[test]
    fn literal_rejected_for_handle() {
        let body = vec![node(
            "result",
            "db.exec",
            vec![lit_arg(json!("my_db")), lit_arg(json!("SELECT 1"))],
        )];
        let err =
            typecheck_func("TestFunc", &[], &body, &[], &[], &TypeRegistry::empty()).unwrap_err();
        assert!(
            err.contains("handles cannot be constructed from literals"),
            "got: {err}"
        );
    }

    #[test]
    fn unknown_passes_through() {
        // Variable from obj.get → Unknown, should not error
        let body = vec![
            node(
                "req",
                "obj.get",
                vec![var_arg("input"), lit_arg(json!("conn"))],
            ),
            node(
                "result",
                "db.exec",
                vec![var_arg("req"), lit_arg(json!("SELECT 1"))],
            ),
        ];
        assert!(typecheck_func("TestFunc", &[], &body, &[], &[], &TypeRegistry::empty()).is_ok());
    }

    #[test]
    fn cross_handle_mismatch() {
        let takes = vec![take("srv", "http_server")];
        let body = vec![node(
            "result",
            "db.exec",
            vec![var_arg("srv"), lit_arg(json!("SELECT 1"))],
        )];
        let err = typecheck_func("TestFunc", &takes, &body, &[], &[], &TypeRegistry::empty())
            .unwrap_err();
        assert!(err.contains("expected 'db_conn'"), "got: {err}");
        assert!(err.contains("got 'http_server'"), "got: {err}");
    }

    #[test]
    fn inference_from_producer() {
        let body = vec![
            node("conn", "db.open", vec![lit_arg(json!(":memory:"))]),
            node(
                "result",
                "db.exec",
                vec![var_arg("conn"), lit_arg(json!("SELECT 1"))],
            ),
        ];
        assert!(typecheck_func("TestFunc", &[], &body, &[], &[], &TypeRegistry::empty()).is_ok());
    }

    #[test]
    fn expr_assign_call_checked() {
        let takes = vec![take("name", "text")];
        let body = vec![expr_assign(
            "result",
            Expr::Call {
                func: "db.exec".to_string(),
                args: vec![Expr::Var("name".to_string()), Expr::Lit(json!("SELECT 1"))],
                children: None,
            },
        )];
        let err = typecheck_func("TestFunc", &takes, &body, &[], &[], &TypeRegistry::empty())
            .unwrap_err();
        assert!(err.contains("expected 'db_conn'"), "got: {err}");
    }

    // --- Struct type tests ---

    #[test]
    fn exec_run_returns_process_output() {
        let body = vec![
            node("out", "exec.run", vec![lit_arg(json!("ls"))]),
            emit("result", "out"),
        ];
        assert!(typecheck_func("TestFunc", &[], &body, &[], &[], &TypeRegistry::empty()).is_ok());
    }

    #[test]
    fn struct_type_mismatch_with_handle() {
        // exec.run returns ProcessOutput; passing it to db.exec (expects db_conn) should fail
        let body = vec![
            node("out", "exec.run", vec![lit_arg(json!("ls"))]),
            node(
                "result",
                "db.exec",
                vec![var_arg("out"), lit_arg(json!("SELECT 1"))],
            ),
        ];
        let err =
            typecheck_func("TestFunc", &[], &body, &[], &[], &TypeRegistry::empty()).unwrap_err();
        assert!(err.contains("expected 'db_conn'"), "got: {err}");
        assert!(err.contains("got 'ProcessOutput'"), "got: {err}");
    }

    #[test]
    fn date_ops_chain() {
        // date.now → date.add should pass (Date → Date)
        let body = vec![
            node("d", "date.now", vec![]),
            node(
                "d2",
                "date.add",
                vec![var_arg("d"), lit_arg(json!(86400000))],
            ),
            emit("result", "d2"),
        ];
        assert!(typecheck_func("TestFunc", &[], &body, &[], &[], &TypeRegistry::empty()).is_ok());
    }

    #[test]
    fn text_rejected_for_date() {
        // text var passed to date.to_unix_ms (expects Date) should fail
        let takes = vec![take("s", "text")];
        let body = vec![node("ms", "date.to_unix_ms", vec![var_arg("s")])];
        let err = typecheck_func("TestFunc", &takes, &body, &[], &[], &TypeRegistry::empty())
            .unwrap_err();
        assert!(err.contains("expected 'Date'"), "got: {err}");
        assert!(err.contains("got 'text'"), "got: {err}");
    }

    #[test]
    fn take_struct_type_resolves() {
        // ProcessOutput take passed to date.to_unix_ms (expects Date) should fail
        let takes = vec![take("po", "ProcessOutput")];
        let body = vec![node("ms", "date.to_unix_ms", vec![var_arg("po")])];
        let err = typecheck_func("TestFunc", &takes, &body, &[], &[], &TypeRegistry::empty())
            .unwrap_err();
        assert!(err.contains("expected 'Date'"), "got: {err}");
        assert!(err.contains("got 'ProcessOutput'"), "got: {err}");
    }

    #[test]
    fn cross_struct_mismatch() {
        // Date passed to trange.duration_ms (expects TimeRange) should fail
        let body = vec![
            node("d", "date.now", vec![]),
            node("ms", "trange.duration_ms", vec![var_arg("d")]),
        ];
        let err =
            typecheck_func("TestFunc", &[], &body, &[], &[], &TypeRegistry::empty()).unwrap_err();
        assert!(err.contains("expected 'TimeRange'"), "got: {err}");
        assert!(err.contains("got 'Date'"), "got: {err}");
    }

    #[test]
    fn expr_assign_struct_inference() {
        // exec.run via expr assignment, then misuse in db.exec
        let body = vec![
            expr_assign(
                "out",
                Expr::Call {
                    func: "exec.run".to_string(),
                    args: vec![Expr::Lit(json!("ls"))],
                    children: None,
                },
            ),
            node(
                "result",
                "db.exec",
                vec![var_arg("out"), lit_arg(json!("SELECT 1"))],
            ),
        ];
        let err =
            typecheck_func("TestFunc", &[], &body, &[], &[], &TypeRegistry::empty()).unwrap_err();
        assert!(err.contains("expected 'db_conn'"), "got: {err}");
        assert!(err.contains("got 'ProcessOutput'"), "got: {err}");
    }

    // --- User-defined type tests ---

    fn build_registry_with_types() -> TypeRegistry {
        use crate::ast::{
            ConstraintValue, EnumDecl, FieldDecl, ModuleAst, Span, TopDecl, TypeConstraint,
            TypeDecl, TypeKind,
        };
        let module = ModuleAst {
            decls: vec![
                TopDecl::Type(TypeDecl {
                    open: false,
                    name: "LoginRequest".to_string(),
                    kind: TypeKind::Struct {
                        fields: vec![
                            FieldDecl {
                                name: "email".to_string(),
                                type_ref: "text".to_string(),
                                constraints: vec![],
                                span: Span { line: 1, col: 1 },
                            },
                            FieldDecl {
                                name: "password".to_string(),
                                type_ref: "text".to_string(),
                                constraints: vec![],
                                span: Span { line: 2, col: 1 },
                            },
                        ],
                    },
                    span: Span { line: 1, col: 1 },
                }),
                TopDecl::Type(TypeDecl {
                    open: false,
                    name: "Email".to_string(),
                    kind: TypeKind::Scalar {
                        base_type: "text".to_string(),
                        constraints: vec![TypeConstraint {
                            key: "matches".to_string(),
                            value: ConstraintValue::Regex("@".to_string()),
                            span: Span { line: 5, col: 1 },
                        }],
                    },
                    span: Span { line: 5, col: 1 },
                }),
                TopDecl::Enum(EnumDecl {
                    open: false,
                    name: "Status".to_string(),
                    variants: vec!["active".to_string(), "inactive".to_string()],
                    span: Span { line: 8, col: 1 },
                }),
            ],
        };
        TypeRegistry::from_module(&module).unwrap()
    }

    #[test]
    fn user_struct_type_resolves() {
        let registry = build_registry_with_types();
        let takes = vec![take("req", "LoginRequest")];
        let body = vec![node("ms", "date.to_unix_ms", vec![var_arg("req")])];
        let err = typecheck_func("TestFunc", &takes, &body, &[], &[], &registry).unwrap_err();
        assert!(err.contains("expected 'Date'"), "got: {err}");
        assert!(err.contains("got 'LoginRequest'"), "got: {err}");
    }

    #[test]
    fn user_scalar_resolves_to_base() {
        // Email is a scalar based on text — should be compatible with text-expecting ops
        let registry = build_registry_with_types();
        let takes = vec![take("e", "Email")];
        // db.exec expects db_conn; passing text (Email resolves to text) should fail with text mismatch
        let body = vec![node(
            "result",
            "db.exec",
            vec![var_arg("e"), lit_arg(json!("SELECT 1"))],
        )];
        let err = typecheck_func("TestFunc", &takes, &body, &[], &[], &registry).unwrap_err();
        assert!(err.contains("expected 'db_conn'"), "got: {err}");
        assert!(err.contains("got 'text'"), "got: {err}");
    }

    #[test]
    fn user_enum_resolves_to_text() {
        // Status enum should resolve to text
        let registry = build_registry_with_types();
        let takes = vec![take("s", "Status")];
        // db.exec expects db_conn; passing text (Status resolves to text) should fail
        let body = vec![node(
            "result",
            "db.exec",
            vec![var_arg("s"), lit_arg(json!("SELECT 1"))],
        )];
        let err = typecheck_func("TestFunc", &takes, &body, &[], &[], &registry).unwrap_err();
        assert!(err.contains("expected 'db_conn'"), "got: {err}");
        assert!(err.contains("got 'text'"), "got: {err}");
    }

    #[test]
    fn user_struct_not_compatible_with_builtin_struct() {
        // LoginRequest should NOT be compatible with ProcessOutput
        let registry = build_registry_with_types();
        let takes = vec![take("req", "LoginRequest")];
        // exec.run returns ProcessOutput; the take is LoginRequest (a struct)
        // Pass LoginRequest to date.to_unix_ms (expects Date) — should fail
        let body = vec![node("ms", "date.to_unix_ms", vec![var_arg("req")])];
        let err = typecheck_func("TestFunc", &takes, &body, &[], &[], &registry).unwrap_err();
        assert!(err.contains("expected 'Date'"), "got: {err}");
        assert!(err.contains("got 'LoginRequest'"), "got: {err}");
    }

    // --- Open modifier enforcement tests ---

    #[test]
    fn closed_struct_rejects_extra_fields() {
        let registry = build_registry_with_types();
        let value = serde_json::json!({"email": "a@b.com", "password": "x", "extra": "field"});
        let errors = registry.validate(&value, "LoginRequest", "req");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].constraint, "closed");
        assert!(
            errors[0].message.contains("unexpected field 'extra'"),
            "got: {}",
            errors[0].message
        );
    }

    #[test]
    fn closed_struct_accepts_valid_fields() {
        let registry = build_registry_with_types();
        let value = serde_json::json!({"email": "a@b.com", "password": "secret"});
        let errors = registry.validate(&value, "LoginRequest", "req");
        assert!(errors.is_empty(), "got: {:?}", errors);
    }

    #[test]
    fn builtin_struct_is_open() {
        let registry = TypeRegistry::empty();
        let value =
            serde_json::json!({"method": "GET", "path": "/", "conn_id": "c1", "extra": "ok"});
        let errors = registry.validate(&value, "HttpRequest", "req");
        assert!(
            errors.is_empty(),
            "built-in structs should be open: {:?}",
            errors
        );
    }

    #[test]
    fn closed_enum_rejects_unknown_variant() {
        let registry = build_registry_with_types();
        let errors = registry.validate(&serde_json::json!("pending"), "Status", "status");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].constraint, "enum");
    }

    #[test]
    fn open_enum_accepts_unknown_variant() {
        use crate::ast::{EnumDecl, ModuleAst, Span, TopDecl};
        let module = ModuleAst {
            decls: vec![TopDecl::Enum(EnumDecl {
                open: true,
                name: "OpenStatus".to_string(),
                variants: vec!["active".to_string()],
                span: Span { line: 1, col: 1 },
            })],
        };
        let registry = TypeRegistry::from_module(&module).unwrap();
        let errors = registry.validate(
            &serde_json::json!("unknown_variant"),
            "OpenStatus",
            "status",
        );
        assert!(
            errors.is_empty(),
            "open enum should accept unknown variants: {:?}",
            errors
        );
    }

    // --- Type annotation tests ---

    fn node_annotated(bind: &str, op: &str, args: Vec<Arg>, ann: &str) -> Statement {
        Statement::Node(NodeAssign {
            bind: bind.to_string(),
            node_id: format!("n_{bind}"),
            op: op.to_string(),
            args,
            type_annotation: Some(ann.to_string()),
        })
    }

    fn expr_assign_annotated(bind: &str, expr: Expr, ann: &str) -> Statement {
        Statement::ExprAssign(ExprAssign {
            bind: bind.to_string(),
            type_annotation: Some(ann.to_string()),
            expr,
        })
    }

    #[test]
    fn annotation_overrides_unknown() {
        // obj.get returns Any (Unknown in inference); annotation provides concrete type
        let registry = build_registry_with_types();
        let takes = vec![take("input", "dict")];
        let body = vec![
            node_annotated(
                "data",
                "obj.get",
                vec![var_arg("input"), lit_arg(json!("data"))],
                "LoginRequest",
            ),
            // Now use `data` where LoginRequest would be wrong — pass to date.to_unix_ms (expects Date)
            node("ms", "date.to_unix_ms", vec![var_arg("data")]),
        ];
        let err = typecheck_func("TestFunc", &takes, &body, &[], &[], &registry).unwrap_err();
        assert!(err.contains("expected 'Date'"), "got: {err}");
        assert!(err.contains("got 'LoginRequest'"), "got: {err}");
    }

    #[test]
    fn annotation_matches_inferred_no_error() {
        // str.upper returns text; annotating as text should produce no error
        let takes = vec![take("x", "text")];
        let body = vec![node_annotated("y", "str.upper", vec![var_arg("x")], "text")];
        assert!(
            typecheck_func("TestFunc", &takes, &body, &[], &[], &TypeRegistry::empty()).is_ok()
        );
    }

    #[test]
    fn annotation_mismatch_errors() {
        // str.upper returns text; annotating as long should error
        let takes = vec![take("x", "text")];
        let body = vec![node_annotated("y", "str.upper", vec![var_arg("x")], "long")];
        let err = typecheck_func("TestFunc", &takes, &body, &[], &[], &TypeRegistry::empty())
            .unwrap_err();
        assert!(err.contains("annotated as 'long'"), "got: {err}");
        assert!(err.contains("type 'text'"), "got: {err}");
    }

    #[test]
    fn expr_annotation_overrides_unknown() {
        // A variable assignment from an unknown source, annotated with a type
        let registry = build_registry_with_types();
        let body = vec![
            expr_assign_annotated(
                "val",
                Expr::Call {
                    func: "obj.get".to_string(),
                    args: vec![Expr::Var("input".to_string()), Expr::Lit(json!("key"))],
                    children: None,
                },
                "Email",
            ),
            // Email resolves to text; pass to db.exec which expects db_conn → error
            node(
                "result",
                "db.exec",
                vec![var_arg("val"), lit_arg(json!("SELECT 1"))],
            ),
        ];
        let takes = vec![take("input", "dict")];
        let err = typecheck_func("TestFunc", &takes, &body, &[], &[], &registry).unwrap_err();
        assert!(err.contains("expected 'db_conn'"), "got: {err}");
        assert!(err.contains("got 'text'"), "got: {err}");
    }

    #[test]
    fn no_annotation_inferred_passes() {
        // Without annotation, unknown types pass through silently (gradual typing)
        let takes = vec![take("input", "dict")];
        let body = vec![
            node(
                "data",
                "obj.get",
                vec![var_arg("input"), lit_arg(json!("key"))],
            ),
            // data is Any from obj.get — passes through all checks
            node("ms", "date.to_unix_ms", vec![var_arg("data")]),
        ];
        assert!(
            typecheck_func("TestFunc", &takes, &body, &[], &[], &TypeRegistry::empty()).is_ok()
        );
    }

    // --- Emit type checking tests ---

    fn port(name: &str, type_name: &str) -> PortDecl {
        PortDecl {
            name: name.to_string(),
            type_name: type_name.to_string(),
            span: span(),
        }
    }

    #[test]
    fn emit_type_mismatch_errors() {
        // db.open returns db_conn; emit port expects long → error
        let takes = vec![take("path", "text")];
        let emits = vec![port("result", "long")];
        let body = vec![
            node("conn", "db.open", vec![var_arg("path")]),
            emit("result", "conn"),
        ];
        let err = typecheck_func(
            "TestFunc",
            &takes,
            &body,
            &emits,
            &[],
            &TypeRegistry::empty(),
        )
        .unwrap_err();
        assert!(err.contains("emit 'result'"), "got: {err}");
        assert!(err.contains("expected type 'long'"), "got: {err}");
        assert!(err.contains("got 'db_conn'"), "got: {err}");
    }

    #[test]
    fn emit_correct_type_passes() {
        // str.len returns long; emit port expects long → ok
        let takes = vec![take("x", "text")];
        let emits = vec![port("result", "long")];
        let body = vec![
            node("len", "str.len", vec![var_arg("x")]),
            emit("result", "len"),
        ];
        assert!(
            typecheck_func(
                "TestFunc",
                &takes,
                &body,
                &emits,
                &[],
                &TypeRegistry::empty()
            )
            .is_ok()
        );
    }

    #[test]
    fn emit_unknown_variable_allowed() {
        // Unknown variable type (e.g. cross-module call) emitted to a typed port is allowed
        let takes = vec![take("x", "text")];
        let emits = vec![port("result", "long")];
        let body = vec![
            node("data", "some.unknown.op", vec![var_arg("x")]),
            emit("result", "data"),
        ];
        assert!(
            typecheck_func(
                "TestFunc",
                &takes,
                &body,
                &emits,
                &[],
                &TypeRegistry::empty()
            )
            .is_ok()
        );
    }

    #[test]
    fn fail_port_type_checked() {
        // str.upper returns text; fail port expects long → error
        let takes = vec![take("x", "text")];
        let fails = vec![port("error", "long")];
        let body = vec![
            node("msg", "str.upper", vec![var_arg("x")]),
            emit("error", "msg"),
        ];
        let err = typecheck_func(
            "TestFunc",
            &takes,
            &body,
            &[],
            &fails,
            &TypeRegistry::empty(),
        )
        .unwrap_err();
        assert!(err.contains("emit 'error'"), "got: {err}");
        assert!(err.contains("expected type 'long'"), "got: {err}");
        assert!(err.contains("got 'text'"), "got: {err}");
    }

    #[test]
    fn emit_no_ports_declared_passes() {
        // Without emit port declarations, emit type checking is skipped
        let takes = vec![take("x", "text")];
        let body = vec![
            node("conn", "db.open", vec![var_arg("x")]),
            emit("result", "conn"),
        ];
        assert!(
            typecheck_func("TestFunc", &takes, &body, &[], &[], &TypeRegistry::empty()).is_ok()
        );
    }

    // --- Flow type checking tests ---

    use crate::ast::{
        FlowGraph, FlowStatement, NextWire, Port, PortMapping, StepBlock, StepThenItem,
    };
    use crate::ir::Ir;
    use crate::loader::{FlowProgram, FlowRegistry};

    fn make_flow_program(inputs: Vec<(&str, &str)>, outputs: Vec<(&str, &str)>) -> FlowProgram {
        let flow_inputs: Vec<Port> = inputs
            .iter()
            .map(|(n, t)| Port {
                name: n.to_string(),
                type_name: t.to_string(),
            })
            .collect();
        let flow_outputs: Vec<Port> = outputs
            .iter()
            .map(|(n, t)| Port {
                name: n.to_string(),
                type_name: t.to_string(),
            })
            .collect();
        FlowProgram {
            flow: crate::ast::Flow {
                name: "mock".to_string(),
                inputs: flow_inputs,
                outputs: flow_outputs,
                body: vec![],
                state_names: vec![],
                local_names: vec![],
            },
            ir: Ir {
                forai_ir: "0.1".to_string(),
                flow: "mock".to_string(),
                inputs: vec![],
                outputs: vec![],
                nodes: vec![],
                edges: vec![],
                emits: vec![],
            },
            emit_name: None,
            fail_name: None,
            registry: TypeRegistry::empty(),
            kind: crate::ast::DeclKind::Func,
        }
    }

    #[test]
    fn flow_type_mismatch() {
        // Flow wires text into a step that expects long
        let mut flow_reg = FlowRegistry::new();
        flow_reg.insert(
            "StepA".to_string(),
            make_flow_program(vec![("input", "long")], vec![("result", "text")]),
        );

        let flow_graph = FlowGraph {
            name: "TestFlow".to_string(),
            inputs: vec![Port {
                name: "req".to_string(),
                type_name: "text".to_string(),
            }],
            emit_ports: vec![],
            fail_ports: vec![],
            body: vec![FlowStatement::Step(StepBlock {
                callee: "StepA".to_string(),
                inputs: vec![PortMapping {
                    port: "input".to_string(),
                    value: Arg::Var {
                        var: "req".to_string(),
                    },
                    span: span(),
                }],
                then_body: vec![],
                span: span(),
            })],
        };

        let err =
            typecheck_flow("TestFlow", &flow_graph, &TypeRegistry::empty(), &flow_reg).unwrap_err();
        assert!(err.contains("expected type 'long'"), "got: {err}");
        assert!(err.contains("got 'text'"), "got: {err}");
    }

    #[test]
    fn flow_compatible_types() {
        let mut flow_reg = FlowRegistry::new();
        flow_reg.insert(
            "StepA".to_string(),
            make_flow_program(vec![("input", "text")], vec![("result", "long")]),
        );

        let flow_graph = FlowGraph {
            name: "TestFlow".to_string(),
            inputs: vec![Port {
                name: "req".to_string(),
                type_name: "text".to_string(),
            }],
            emit_ports: vec![],
            fail_ports: vec![],
            body: vec![FlowStatement::Step(StepBlock {
                callee: "StepA".to_string(),
                inputs: vec![PortMapping {
                    port: "input".to_string(),
                    value: Arg::Var {
                        var: "req".to_string(),
                    },
                    span: span(),
                }],
                then_body: vec![],
                span: span(),
            })],
        };

        assert!(typecheck_flow("TestFlow", &flow_graph, &TypeRegistry::empty(), &flow_reg).is_ok());
    }

    #[test]
    fn flow_unknown_callee_passes() {
        // Unknown callee (not in registry) should not produce an error
        let flow_reg = FlowRegistry::new();

        let flow_graph = FlowGraph {
            name: "TestFlow".to_string(),
            inputs: vec![Port {
                name: "req".to_string(),
                type_name: "text".to_string(),
            }],
            emit_ports: vec![],
            fail_ports: vec![],
            body: vec![FlowStatement::Step(StepBlock {
                callee: "UnknownStep".to_string(),
                inputs: vec![PortMapping {
                    port: "input".to_string(),
                    value: Arg::Var {
                        var: "req".to_string(),
                    },
                    span: span(),
                }],
                then_body: vec![],
                span: span(),
            })],
        };

        assert!(typecheck_flow("TestFlow", &flow_graph, &TypeRegistry::empty(), &flow_reg).is_ok());
    }

    #[test]
    fn flow_output_wiring_propagates_type() {
        // StepA outputs text, StepB expects long — should error
        let mut flow_reg = FlowRegistry::new();
        flow_reg.insert(
            "StepA".to_string(),
            make_flow_program(vec![("input", "text")], vec![("result", "text")]),
        );
        flow_reg.insert(
            "StepB".to_string(),
            make_flow_program(vec![("data", "long")], vec![("output", "bool")]),
        );

        let flow_graph = FlowGraph {
            name: "TestFlow".to_string(),
            inputs: vec![Port {
                name: "req".to_string(),
                type_name: "text".to_string(),
            }],
            emit_ports: vec![],
            fail_ports: vec![],
            body: vec![
                FlowStatement::Step(StepBlock {
                    callee: "StepA".to_string(),
                    inputs: vec![PortMapping {
                        port: "input".to_string(),
                        value: Arg::Var {
                            var: "req".to_string(),
                        },
                        span: span(),
                    }],
                    then_body: vec![StepThenItem::Next(NextWire {
                        port: "result".to_string(),
                        wire: "step_a_out".to_string(),
                        via_callee: None,
                        via_inputs: vec![],
                        via_outputs: vec![],
                        span: span(),
                    })],
                    span: span(),
                }),
                FlowStatement::Step(StepBlock {
                    callee: "StepB".to_string(),
                    inputs: vec![PortMapping {
                        port: "data".to_string(),
                        value: Arg::Var {
                            var: "step_a_out".to_string(),
                        },
                        span: span(),
                    }],
                    then_body: vec![],
                    span: span(),
                }),
            ],
        };

        let err =
            typecheck_flow("TestFlow", &flow_graph, &TypeRegistry::empty(), &flow_reg).unwrap_err();
        assert!(err.contains("expected type 'long'"), "got: {err}");
        assert!(err.contains("got 'text'"), "got: {err}");
    }

    #[test]
    fn port_unknown_type_errors() {
        // take/emit/fail with undefined type → compile error
        let takes = vec![take("x", "Nonexistent")];
        let emits = vec![port("out", "AlsoMissing")];
        let fails = vec![port("err", "NoSuchType")];
        let body = vec![];
        let err = typecheck_func(
            "TestFunc",
            &takes,
            &body,
            &emits,
            &fails,
            &TypeRegistry::empty(),
        )
        .unwrap_err();
        assert!(
            err.contains("take 'x' has unknown type 'Nonexistent'"),
            "got: {err}"
        );
        assert!(
            err.contains("emit 'out' has unknown type 'AlsoMissing'"),
            "got: {err}"
        );
        assert!(
            err.contains("fail 'err' has unknown type 'NoSuchType'"),
            "got: {err}"
        );
    }

    #[test]
    fn flow_port_unknown_type_errors() {
        // Flow with unknown port types → compile error
        let flow_reg = FlowRegistry::new();
        let flow_graph = FlowGraph {
            name: "TestFlow".to_string(),
            inputs: vec![Port {
                name: "req".to_string(),
                type_name: "Nonexistent".to_string(),
            }],
            emit_ports: vec![Port {
                name: "out".to_string(),
                type_name: "AlsoMissing".to_string(),
            }],
            fail_ports: vec![Port {
                name: "err".to_string(),
                type_name: "NoSuchType".to_string(),
            }],
            body: vec![],
        };
        let err =
            typecheck_flow("TestFlow", &flow_graph, &TypeRegistry::empty(), &flow_reg).unwrap_err();
        assert!(
            err.contains("input 'req' has unknown type 'Nonexistent'"),
            "got: {err}"
        );
        assert!(
            err.contains("emit 'out' has unknown type 'AlsoMissing'"),
            "got: {err}"
        );
        assert!(
            err.contains("fail 'err' has unknown type 'NoSuchType'"),
            "got: {err}"
        );
    }

    #[test]
    fn flow_wire_unknown_type_errors() {
        // Unknown wire into typed step port is now a hard error
        let mut flow_reg = FlowRegistry::new();
        flow_reg.insert(
            "StepB".to_string(),
            make_flow_program(vec![("data", "long")], vec![("output", "bool")]),
        );

        let flow_graph = FlowGraph {
            name: "TestFlow".to_string(),
            inputs: vec![Port {
                name: "req".to_string(),
                type_name: "text".to_string(),
            }],
            emit_ports: vec![],
            fail_ports: vec![],
            body: vec![FlowStatement::Step(StepBlock {
                callee: "StepB".to_string(),
                inputs: vec![PortMapping {
                    port: "data".to_string(),
                    value: Arg::Var {
                        var: "undefined_var".to_string(),
                    },
                    span: span(),
                }],
                then_body: vec![],
                span: span(),
            })],
        };

        let err =
            typecheck_flow("TestFlow", &flow_graph, &TypeRegistry::empty(), &flow_reg).unwrap_err();
        assert!(
            err.contains("requires 'long' but wire has unknown type"),
            "got: {err}"
        );
    }

    #[test]
    fn emit_loop_var_typed_passes() {
        // List with known element type → loop var inherits type → emit succeeds
        let takes = vec![take("x", "text")];
        let emits = vec![port("result", "text")];
        let body = vec![
            // items = str.split(x, ",") → list of text
            node(
                "items",
                "str.split",
                vec![var_arg("x"), lit_arg(json!(","))],
            ),
            Statement::Loop(crate::ast::LoopBlock {
                collection: Expr::Var("items".to_string()),
                item: "item".to_string(),
                index: None,
                body: vec![emit("result", "item")],
            }),
        ];
        assert!(
            typecheck_func(
                "TestFunc",
                &takes,
                &body,
                &emits,
                &[],
                &TypeRegistry::empty()
            )
            .is_ok()
        );
    }

    #[test]
    fn emit_loop_var_unknown_errors() {
        // List from unknown op → loop var is Unknown → allowed (can't resolve at compile time)
        let takes = vec![take("x", "text")];
        let emits = vec![port("result", "long")];
        let body = vec![
            node("items", "some.unknown.list.op", vec![var_arg("x")]),
            Statement::Loop(crate::ast::LoopBlock {
                collection: Expr::Var("items".to_string()),
                item: "item".to_string(),
                index: None,
                body: vec![emit("result", "item")],
            }),
        ];
        assert!(
            typecheck_func(
                "TestFunc",
                &takes,
                &body,
                &emits,
                &[],
                &TypeRegistry::empty()
            )
            .is_ok()
        );
    }

    #[test]
    fn dict_struct_compatible() {
        // Dict emitted to struct-typed port passes (structs are dicts at runtime)
        let registry = build_registry_with_types();
        let emits = vec![port("result", "LoginRequest")];
        let body = vec![
            expr_assign(
                "data",
                Expr::DictLit(vec![
                    ("email".to_string(), Expr::Lit(json!("a@b.com"))),
                    ("password".to_string(), Expr::Lit(json!("secret"))),
                ]),
            ),
            emit("result", "data"),
        ];
        assert!(typecheck_func("TestFunc", &[], &body, &emits, &[], &registry).is_ok());
    }

    #[test]
    fn list_range_loop_var_is_long() {
        // list.range produces list of longs → loop var should be long
        let emits = vec![port("result", "long")];
        let body = vec![
            node(
                "nums",
                "list.range",
                vec![lit_arg(json!(0)), lit_arg(json!(10))],
            ),
            Statement::Loop(crate::ast::LoopBlock {
                collection: Expr::Var("nums".to_string()),
                item: "n".to_string(),
                index: None,
                body: vec![emit("result", "n")],
            }),
        ];
        assert!(
            typecheck_func("TestFunc", &[], &body, &emits, &[], &TypeRegistry::empty()).is_ok()
        );
    }

    #[test]
    fn flow_wire_literal_type_checked() {
        // String literal wired to long port → type mismatch
        let mut flow_reg = FlowRegistry::new();
        flow_reg.insert(
            "StepA".to_string(),
            make_flow_program(vec![("count", "long")], vec![("result", "text")]),
        );

        let flow_graph = FlowGraph {
            name: "TestFlow".to_string(),
            inputs: vec![],
            emit_ports: vec![],
            fail_ports: vec![],
            body: vec![FlowStatement::Step(StepBlock {
                callee: "StepA".to_string(),
                inputs: vec![PortMapping {
                    port: "count".to_string(),
                    value: Arg::Lit {
                        lit: json!("hello"),
                    },
                    span: span(),
                }],
                then_body: vec![],
                span: span(),
            })],
        };

        let err =
            typecheck_flow("TestFlow", &flow_graph, &TypeRegistry::empty(), &flow_reg).unwrap_err();
        assert!(err.contains("expected type 'long'"), "got: {err}");
        assert!(err.contains("got 'text'"), "got: {err}");
    }
}
