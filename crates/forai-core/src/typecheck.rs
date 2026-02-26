use std::collections::HashMap;

use crate::ast::{Arg, Expr, Statement, TakeDecl};
use crate::op_types::{self, OpType};

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

fn take_type_to_op_type(type_name: &str) -> InferredType {
    match type_name {
        "text" => InferredType::Known(OpType::Text),
        "bool" => InferredType::Known(OpType::Bool),
        "long" => InferredType::Known(OpType::Long),
        "real" => InferredType::Known(OpType::Real),
        "list" => InferredType::Known(OpType::List),
        "dict" => InferredType::Known(OpType::Dict),
        "db_conn" => InferredType::Known(OpType::DbConn),
        "http_server" => InferredType::Known(OpType::HttpServer),
        "http_conn" => InferredType::Known(OpType::HttpConn),
        "ws_conn" => InferredType::Known(OpType::WsConn),
        "ProcessOutput" | "HttpRequest" | "HttpResponse" | "Date" | "Stamp"
        | "TimeRange" | "WebSocketMessage" | "ErrorObject" | "URLParts" => {
            InferredType::Known(OpType::Struct(type_name.to_string()))
        }
        _ => InferredType::Unknown,
    }
}

fn infer_expr_type(expr: &Expr, var_types: &HashMap<String, InferredType>) -> InferredType {
    match expr {
        Expr::Var(name) => var_types
            .get(name)
            .cloned()
            .unwrap_or(InferredType::Unknown),
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
                InferredType::Unknown
            }
        }
        Expr::ListLit(_) => InferredType::Known(OpType::List),
        Expr::DictLit(_) => InferredType::Known(OpType::Dict),
        Expr::Interp(_) => InferredType::Known(OpType::Text),
        _ => InferredType::Unknown,
    }
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

        let actual = infer_expr_type(arg_expr, var_types);
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
    errors: &mut Vec<String>,
) {
    for stmt in stmts {
        match stmt {
            Statement::Node(n) => {
                check_node_args(func_name, &n.op, &n.args, var_types, errors);
                // Infer the bind variable's type from the op's return type
                if let Some(sig) = op_types::op_signature(&n.op) {
                    var_types.insert(n.bind.clone(), InferredType::Known(sig.returns));
                } else {
                    var_types.insert(n.bind.clone(), InferredType::Unknown);
                }
            }
            Statement::ExprAssign(ea) => {
                // Check any op calls inside the expression
                check_expr_calls(func_name, &ea.expr, var_types, errors);
                let inferred = infer_expr_type(&ea.expr, var_types);
                var_types.insert(ea.bind.clone(), inferred);
            }
            Statement::Case(c) => {
                for arm in &c.arms {
                    let mut arm_scope = var_types.clone();
                    check_statements(func_name, &arm.body, &mut arm_scope, errors);
                    // Merge back assignments that exist in outer scope
                    for (k, v) in &arm_scope {
                        if var_types.contains_key(k) {
                            var_types.insert(k.clone(), v.clone());
                        }
                    }
                }
                if !c.else_body.is_empty() {
                    let mut else_scope = var_types.clone();
                    check_statements(func_name, &c.else_body, &mut else_scope, errors);
                    for (k, v) in &else_scope {
                        if var_types.contains_key(k) {
                            var_types.insert(k.clone(), v.clone());
                        }
                    }
                }
            }
            Statement::Loop(l) => {
                let mut loop_scope = var_types.clone();
                loop_scope.insert(l.item.clone(), InferredType::Unknown);
                check_statements(func_name, &l.body, &mut loop_scope, errors);
                // Merge back
                for (k, v) in &loop_scope {
                    if var_types.contains_key(k) {
                        var_types.insert(k.clone(), v.clone());
                    }
                }
            }
            Statement::Sync(s) => {
                check_statements(func_name, &s.body, var_types, errors);
            }
            Statement::BareLoop(b) => {
                let mut loop_scope = var_types.clone();
                check_statements(func_name, &b.body, &mut loop_scope, errors);
                for (k, v) in &loop_scope {
                    if var_types.contains_key(k) {
                        var_types.insert(k.clone(), v.clone());
                    }
                }
            }
            Statement::SourceLoop(sl) => {
                check_node_args(func_name, &sl.source_op, &sl.source_args, var_types, errors);
                if let Some(sig) = op_types::op_signature(&sl.source_op) {
                    var_types.insert(sl.bind.clone(), InferredType::Known(sig.returns));
                } else {
                    var_types.insert(sl.bind.clone(), InferredType::Unknown);
                }
                let mut loop_scope = var_types.clone();
                check_statements(func_name, &sl.body, &mut loop_scope, errors);
            }
            Statement::On(on_block) => {
                check_node_args(func_name, &on_block.source_op, &on_block.source_args, var_types, errors);
                if let Some(sig) = op_types::op_signature(&on_block.source_op) {
                    var_types.insert(on_block.bind.clone(), InferredType::Known(sig.returns));
                } else {
                    var_types.insert(on_block.bind.clone(), InferredType::Unknown);
                }
                let mut on_scope = var_types.clone();
                check_statements(func_name, &on_block.body, &mut on_scope, errors);
            }
            Statement::Emit(emit) => {
                check_expr_calls(func_name, &emit.value_expr, var_types, errors);
            }
            Statement::Break | Statement::SendNowait(_) => {}
        }
    }
}

fn check_expr_calls(
    func_name: &str,
    expr: &Expr,
    var_types: &HashMap<String, InferredType>,
    errors: &mut Vec<String>,
) {
    match expr {
        Expr::Call { func, args } => {
            check_call_args(func_name, func, args, var_types, errors);
            for arg in args {
                check_expr_calls(func_name, arg, var_types, errors);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            check_expr_calls(func_name, lhs, var_types, errors);
            check_expr_calls(func_name, rhs, var_types, errors);
        }
        Expr::UnaryOp { expr: inner, .. } => {
            check_expr_calls(func_name, inner, var_types, errors);
        }
        Expr::Ternary {
            cond,
            then_expr,
            else_expr,
        } => {
            check_expr_calls(func_name, cond, var_types, errors);
            check_expr_calls(func_name, then_expr, var_types, errors);
            check_expr_calls(func_name, else_expr, var_types, errors);
        }
        Expr::ListLit(items) => {
            for item in items {
                check_expr_calls(func_name, item, var_types, errors);
            }
        }
        Expr::DictLit(pairs) => {
            for (_, v) in pairs {
                check_expr_calls(func_name, v, var_types, errors);
            }
        }
        Expr::Interp(parts) => {
            for part in parts {
                if let crate::ast::InterpExpr::Expr(e) = part {
                    check_expr_calls(func_name, e, var_types, errors);
                }
            }
        }
        Expr::Index { expr, index } => {
            check_expr_calls(func_name, expr, var_types, errors);
            check_expr_calls(func_name, index, var_types, errors);
        }
        Expr::Var(_) | Expr::Lit(_) => {}
    }
}

pub fn typecheck_func(
    func_name: &str,
    takes: &[TakeDecl],
    body: &[Statement],
) -> Result<(), String> {
    let mut var_types: HashMap<String, InferredType> = HashMap::new();

    // Seed from take declarations
    for take in takes {
        var_types.insert(take.name.clone(), take_type_to_op_type(&take.type_name));
    }

    let mut errors = Vec::new();
    check_statements(func_name, body, &mut var_types, &mut errors);

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
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
        assert!(typecheck_func("TestFunc", &takes, &body).is_ok());
    }

    #[test]
    fn type_mismatch_text_for_db_conn() {
        let takes = vec![take("name", "text")];
        let body = vec![node(
            "result",
            "db.exec",
            vec![var_arg("name"), lit_arg(json!("SELECT 1"))],
        )];
        let err = typecheck_func("TestFunc", &takes, &body).unwrap_err();
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
        let err = typecheck_func("TestFunc", &[], &body).unwrap_err();
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
        assert!(typecheck_func("TestFunc", &[], &body).is_ok());
    }

    #[test]
    fn cross_handle_mismatch() {
        let takes = vec![take("srv", "http_server")];
        let body = vec![node(
            "result",
            "db.exec",
            vec![var_arg("srv"), lit_arg(json!("SELECT 1"))],
        )];
        let err = typecheck_func("TestFunc", &takes, &body).unwrap_err();
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
        assert!(typecheck_func("TestFunc", &[], &body).is_ok());
    }

    #[test]
    fn expr_assign_call_checked() {
        let takes = vec![take("name", "text")];
        let body = vec![expr_assign(
            "result",
            Expr::Call {
                func: "db.exec".to_string(),
                args: vec![Expr::Var("name".to_string()), Expr::Lit(json!("SELECT 1"))],
            },
        )];
        let err = typecheck_func("TestFunc", &takes, &body).unwrap_err();
        assert!(err.contains("expected 'db_conn'"), "got: {err}");
    }

    // --- Struct type tests ---

    #[test]
    fn exec_run_returns_process_output() {
        let body = vec![
            node("out", "exec.run", vec![lit_arg(json!("ls"))]),
            emit("result", "out"),
        ];
        assert!(typecheck_func("TestFunc", &[], &body).is_ok());
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
        let err = typecheck_func("TestFunc", &[], &body).unwrap_err();
        assert!(err.contains("expected 'db_conn'"), "got: {err}");
        assert!(err.contains("got 'ProcessOutput'"), "got: {err}");
    }

    #[test]
    fn date_ops_chain() {
        // date.now → date.add should pass (Date → Date)
        let body = vec![
            node("d", "date.now", vec![]),
            node("d2", "date.add", vec![var_arg("d"), lit_arg(json!(86400000))]),
            emit("result", "d2"),
        ];
        assert!(typecheck_func("TestFunc", &[], &body).is_ok());
    }

    #[test]
    fn text_rejected_for_date() {
        // text var passed to date.to_unix_ms (expects Date) should fail
        let takes = vec![take("s", "text")];
        let body = vec![node("ms", "date.to_unix_ms", vec![var_arg("s")])];
        let err = typecheck_func("TestFunc", &takes, &body).unwrap_err();
        assert!(err.contains("expected 'Date'"), "got: {err}");
        assert!(err.contains("got 'text'"), "got: {err}");
    }

    #[test]
    fn take_struct_type_resolves() {
        // ProcessOutput take passed to date.to_unix_ms (expects Date) should fail
        let takes = vec![take("po", "ProcessOutput")];
        let body = vec![node("ms", "date.to_unix_ms", vec![var_arg("po")])];
        let err = typecheck_func("TestFunc", &takes, &body).unwrap_err();
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
        let err = typecheck_func("TestFunc", &[], &body).unwrap_err();
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
                },
            ),
            node(
                "result",
                "db.exec",
                vec![var_arg("out"), lit_arg(json!("SELECT 1"))],
            ),
        ];
        let err = typecheck_func("TestFunc", &[], &body).unwrap_err();
        assert!(err.contains("expected 'db_conn'"), "got: {err}");
        assert!(err.contains("got 'ProcessOutput'"), "got: {err}");
    }
}
