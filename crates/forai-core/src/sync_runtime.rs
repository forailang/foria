use crate::ast::{Arg, BinOp, Expr, Flow, InterpExpr, Pattern, Statement, UnaryOp};
use crate::codec::CodecRegistry;
use crate::host;
use crate::ir::Ir;
use crate::loader::FlowRegistry;
use crate::pure_ops;
use crate::sync_host::SyncHost;
use crate::types::TypeRegistry;
use serde_json::{Value, json};
use std::collections::HashMap;

pub enum ExecSignal {
    Continue,
    Emit {
        output: String,
        value_var: String,
        value: Value,
    },
    Break,
}

#[derive(Debug)]
pub struct RunResult {
    pub outputs: Value,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct StepSnapshot {
    pub step: usize,
    pub op: String,
    pub bind: String,
    pub bindings: HashMap<String, Value>,
}

/// Execute a flow and collect a snapshot after every Statement::Node execution.
/// Returns the final result plus all collected snapshots.
pub fn execute_flow_stepping(
    flow: &Flow,
    _ir: Ir,
    inputs: HashMap<String, Value>,
    _registry: &TypeRegistry,
    flow_registry: Option<&FlowRegistry>,
    codecs: &CodecRegistry,
    host: &dyn SyncHost,
) -> Result<(RunResult, Vec<StepSnapshot>), String> {
    let mut vars = inputs;
    let mut outputs = serde_json::Map::new();
    let snapshots = std::cell::RefCell::new(Vec::new());
    let step_counter = std::cell::Cell::new(0usize);

    let on_step = |op: &str, bind: &str, bindings: &HashMap<String, Value>| {
        let n = step_counter.get();
        step_counter.set(n + 1);
        snapshots.borrow_mut().push(StepSnapshot {
            step: n,
            op: op.to_string(),
            bind: bind.to_string(),
            bindings: bindings.clone(),
        });
    };

    if let ExecSignal::Emit {
        output,
        value_var,
        value,
    } = execute_statements_stepping(&flow.body, &mut vars, flow_registry, host, codecs, &on_step)?
    {
        outputs.insert(output, value.clone());
        if !value_var.contains(' ') && !value_var.contains('"') {
            vars.insert(value_var, value);
        }
    }

    Ok((
        RunResult {
            outputs: Value::Object(outputs),
        },
        snapshots.into_inner(),
    ))
}

fn execute_statements_stepping(
    statements: &[Statement],
    vars: &mut HashMap<String, Value>,
    flow_registry: Option<&FlowRegistry>,
    host: &dyn SyncHost,
    codecs: &CodecRegistry,
    on_step: &dyn Fn(&str, &str, &HashMap<String, Value>),
) -> Result<ExecSignal, String> {
    for stmt in statements {
        match stmt {
            Statement::Node(node) => {
                let mut args = Vec::new();
                for arg in &node.args {
                    match arg {
                        Arg::Lit { lit } => args.push(lit.clone()),
                        Arg::Var { var } => {
                            if let Some(value) = resolve_var_path(vars, var) {
                                args.push(value);
                            } else {
                                return Err(format!("Missing variable `{var}`"));
                            }
                        }
                    }
                }
                let result = dispatch_op(&node.op, &args, flow_registry, host, codecs)?;
                vars.insert(node.bind.clone(), result);
                on_step(&node.op, &node.bind, vars);
            }
            Statement::ExprAssign(ea) => {
                let value = eval_expr(&ea.expr, vars, flow_registry, host, codecs)?;
                vars.insert(ea.bind.clone(), value);
            }
            Statement::Emit(emit) => {
                let value = eval_expr(&emit.value_expr, vars, flow_registry, host, codecs)?;
                let label = match &emit.value_expr {
                    Expr::Var(name) => name.clone(),
                    _ => format!("{:?}", emit.value_expr),
                };
                return Ok(ExecSignal::Emit {
                    output: emit.output.clone(),
                    value_var: label,
                    value,
                });
            }
            Statement::Case(case_block) => {
                let subject = eval_expr(&case_block.expr, vars, flow_registry, host, codecs)?;
                let mut matched = false;
                for arm in &case_block.arms {
                    if pattern_matches(&subject, &arm.pattern) {
                        matched = true;
                        match execute_statements_stepping(&arm.body, vars, flow_registry, host, codecs, on_step)? {
                            ExecSignal::Continue => {}
                            signal @ ExecSignal::Emit { .. } => return Ok(signal),
                            ExecSignal::Break => return Ok(ExecSignal::Break),
                        }
                        break;
                    }
                }
                if !matched {
                    match execute_statements_stepping(&case_block.else_body, vars, flow_registry, host, codecs, on_step)? {
                        ExecSignal::Continue => {}
                        signal @ ExecSignal::Emit { .. } => return Ok(signal),
                        ExecSignal::Break => return Ok(ExecSignal::Break),
                    }
                }
            }
            Statement::Loop(loop_block) => {
                let collection = eval_expr(&loop_block.collection, vars, flow_registry, host, codecs)?;
                let items = collection.as_array().ok_or_else(|| {
                    format!("Loop collection must be an array, got `{}`", collection)
                })?;
                let items = items.clone();
                let previous = vars.get(&loop_block.item).cloned();
                for item in &items {
                    vars.insert(loop_block.item.clone(), item.clone());
                    match execute_statements_stepping(&loop_block.body, vars, flow_registry, host, codecs, on_step)? {
                        ExecSignal::Continue => {}
                        signal @ ExecSignal::Emit { .. } => return Ok(signal),
                        ExecSignal::Break => break,
                    }
                }
                if let Some(prev) = previous {
                    vars.insert(loop_block.item.clone(), prev);
                } else {
                    vars.remove(&loop_block.item);
                }
            }
            Statement::BareLoop(block) => loop {
                match execute_statements_stepping(&block.body, vars, flow_registry, host, codecs, on_step)? {
                    ExecSignal::Continue => {}
                    signal @ ExecSignal::Emit { .. } => return Ok(signal),
                    ExecSignal::Break => break,
                }
            },
            Statement::Sync(sync_block) => {
                for s in &sync_block.body {
                    match execute_statements_stepping(&[s.clone()], vars, flow_registry, host, codecs, on_step)? {
                        ExecSignal::Continue => {}
                        signal @ ExecSignal::Emit { .. } => return Ok(signal),
                        ExecSignal::Break => return Ok(ExecSignal::Break),
                    }
                }
            }
            Statement::SourceLoop(_) => {
                return Err("SourceLoop not supported in WASM runtime".into());
            }
            Statement::On(_) => {
                return Err("On block not supported in WASM runtime".into());
            }
            Statement::SendNowait(sn) => {
                let mut resolved_args = Vec::new();
                for arg_expr in &sn.args {
                    let val = eval_expr(arg_expr, vars, flow_registry, host, codecs)?;
                    resolved_args.push(val);
                }
                let _ = dispatch_op(&sn.target, &resolved_args, flow_registry, host, codecs);
            }
            Statement::Break => {
                return Ok(ExecSignal::Break);
            }
        }
    }
    Ok(ExecSignal::Continue)
}

pub fn execute_flow(
    flow: &Flow,
    _ir: Ir,
    inputs: HashMap<String, Value>,
    registry: &TypeRegistry,
    flow_registry: Option<&FlowRegistry>,
    codecs: &CodecRegistry,
    host: &dyn SyncHost,
) -> Result<RunResult, String> {
    // Validate inputs
    let mut validation_errors = Vec::new();
    for port in &flow.inputs {
        if let Some(value) = inputs.get(&port.name) {
            validation_errors.extend(registry.validate(value, &port.type_name, &port.name));
        }
    }
    if !validation_errors.is_empty() {
        let fail_output = flow
            .outputs
            .get(1)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        let error_details: Vec<Value> = validation_errors
            .iter()
            .map(|e| json!({"path": e.path, "constraint": e.constraint, "message": e.message}))
            .collect();
        let error_value = json!({"kind": "validation_error", "errors": error_details});
        let mut outputs = serde_json::Map::new();
        outputs.insert(fail_output, error_value);
        return Ok(RunResult {
            outputs: Value::Object(outputs),
        });
    }

    let mut vars = inputs;
    let mut outputs = serde_json::Map::new();

    if let ExecSignal::Emit {
        output,
        value_var,
        value,
    } = execute_statements(&flow.body, &mut vars, flow_registry, host, codecs)?
    {
        outputs.insert(output, value.clone());
        if !value_var.contains(' ') && !value_var.contains('"') {
            vars.insert(value_var, value);
        }
    }

    Ok(RunResult {
        outputs: Value::Object(outputs),
    })
}

fn execute_statements(
    statements: &[Statement],
    vars: &mut HashMap<String, Value>,
    flow_registry: Option<&FlowRegistry>,
    host: &dyn SyncHost,
    codecs: &CodecRegistry,
) -> Result<ExecSignal, String> {
    for stmt in statements {
        match stmt {
            Statement::Node(node) => {
                let mut args = Vec::new();
                for arg in &node.args {
                    match arg {
                        Arg::Lit { lit } => args.push(lit.clone()),
                        Arg::Var { var } => {
                            if let Some(value) = resolve_var_path(vars, var) {
                                args.push(value);
                            } else {
                                return Err(format!("Missing variable `{var}`"));
                            }
                        }
                    }
                }

                let result = dispatch_op(&node.op, &args, flow_registry, host, codecs)?;
                vars.insert(node.bind.clone(), result);
            }
            Statement::ExprAssign(ea) => {
                let value = eval_expr(&ea.expr, vars, flow_registry, host, codecs)?;
                vars.insert(ea.bind.clone(), value);
            }
            Statement::Emit(emit) => {
                let value = eval_expr(&emit.value_expr, vars, flow_registry, host, codecs)?;
                let label = match &emit.value_expr {
                    Expr::Var(name) => name.clone(),
                    _ => format!("{:?}", emit.value_expr),
                };
                return Ok(ExecSignal::Emit {
                    output: emit.output.clone(),
                    value_var: label,
                    value,
                });
            }
            Statement::Case(case_block) => {
                let subject = eval_expr(&case_block.expr, vars, flow_registry, host, codecs)?;
                let mut matched = false;
                for arm in &case_block.arms {
                    if pattern_matches(&subject, &arm.pattern) {
                        matched = true;
                        match execute_statements(&arm.body, vars, flow_registry, host, codecs)? {
                            ExecSignal::Continue => {}
                            signal @ ExecSignal::Emit { .. } => return Ok(signal),
                            ExecSignal::Break => return Ok(ExecSignal::Break),
                        }
                        break;
                    }
                }
                if !matched {
                    match execute_statements(
                        &case_block.else_body,
                        vars,
                        flow_registry,
                        host,
                        codecs,
                    )? {
                        ExecSignal::Continue => {}
                        signal @ ExecSignal::Emit { .. } => return Ok(signal),
                        ExecSignal::Break => return Ok(ExecSignal::Break),
                    }
                }
            }
            Statement::Loop(loop_block) => {
                let collection =
                    eval_expr(&loop_block.collection, vars, flow_registry, host, codecs)?;
                let items = collection.as_array().ok_or_else(|| {
                    format!("Loop collection must be an array, got `{}`", collection)
                })?;
                let items = items.clone();
                let previous = vars.get(&loop_block.item).cloned();
                for item in &items {
                    vars.insert(loop_block.item.clone(), item.clone());
                    match execute_statements(&loop_block.body, vars, flow_registry, host, codecs)? {
                        ExecSignal::Continue => {}
                        signal @ ExecSignal::Emit { .. } => return Ok(signal),
                        ExecSignal::Break => break,
                    }
                }
                if let Some(prev) = previous {
                    vars.insert(loop_block.item.clone(), prev);
                } else {
                    vars.remove(&loop_block.item);
                }
            }
            Statement::BareLoop(block) => loop {
                match execute_statements(&block.body, vars, flow_registry, host, codecs)? {
                    ExecSignal::Continue => {}
                    signal @ ExecSignal::Emit { .. } => return Ok(signal),
                    ExecSignal::Break => break,
                }
            },
            Statement::Sync(sync_block) => {
                // In WASM, sync blocks run sequentially (no concurrency)
                let mut merged_vars = vars.clone();
                for stmt in &sync_block.body {
                    let mut local_vars = vars.clone();
                    match execute_statements(
                        std::slice::from_ref(stmt),
                        &mut local_vars,
                        flow_registry,
                        host,
                        codecs,
                    )? {
                        ExecSignal::Continue => {
                            for (k, v) in &local_vars {
                                if !vars.contains_key(k) || local_vars.get(k) != vars.get(k) {
                                    merged_vars.insert(k.clone(), v.clone());
                                }
                            }
                        }
                        signal @ ExecSignal::Emit { .. } => return Ok(signal),
                        ExecSignal::Break => {}
                    }
                }
                for (target, export) in sync_block.targets.iter().zip(sync_block.exports.iter()) {
                    if let Some(v) = resolve_var_path(&merged_vars, export) {
                        vars.insert(target.clone(), v);
                    } else if sync_block.options.safe {
                        vars.insert(target.clone(), Value::Null);
                    } else {
                        return Err(format!("Sync export `{}` not found in local scope", export));
                    }
                }
            }
            Statement::SendNowait(sn) => {
                // In WASM, send nowait runs synchronously
                let mut resolved_args = Vec::new();
                for arg_expr in &sn.args {
                    let val = eval_expr(arg_expr, vars, flow_registry, host, codecs)?;
                    resolved_args.push(val);
                }
                let _ = dispatch_op(&sn.target, &resolved_args, flow_registry, host, codecs);
            }
            Statement::Break => {
                return Ok(ExecSignal::Break);
            }
            Statement::SourceLoop(_) => {
                return Err("SourceLoop not supported in WASM runtime".to_string());
            }
            Statement::On(_) => {
                return Err("On block not supported in WASM runtime".to_string());
            }
        }
    }
    Ok(ExecSignal::Continue)
}

fn dispatch_op(
    op: &str,
    args: &[Value],
    flow_registry: Option<&FlowRegistry>,
    host: &dyn SyncHost,
    codecs: &CodecRegistry,
) -> Result<Value, String> {
    // Check value mocks first
    if let Some(fr) = flow_registry {
        if let Some(value) = fr.get_value_mock(op) {
            return Ok(value.clone());
        }
        // Try sub-flow dispatch
        if let Some(program) = fr.get(op) {
            if args.len() != program.flow.inputs.len() {
                return Err(format!(
                    "flow `{}` expects {} args but got {}",
                    op,
                    program.flow.inputs.len(),
                    args.len()
                ));
            }
            let mut input_map = HashMap::new();
            for (idx, port) in program.flow.inputs.iter().enumerate() {
                input_map.insert(port.name.clone(), args[idx].clone());
            }
            let result = execute_flow(
                &program.flow,
                program.ir.clone(),
                input_map,
                &program.registry,
                flow_registry,
                codecs,
                host,
            )?;
            let outputs = result
                .outputs
                .as_object()
                .ok_or_else(|| format!("flow `{op}` produced invalid outputs shape"))?;
            let success = program.emit_name.as_deref().and_then(|n| outputs.get(n)).cloned();
            let failure = program.fail_name.as_deref().and_then(|n| outputs.get(n)).cloned();
            if program.emit_name.is_none() {
                return Ok(serde_json::Value::Null);
            }
            return match (success, failure) {
                (Some(v), None) => Ok(v),
                (None, Some(f)) => Err(format!("flow `{op}` emitted on fail track: {f}")),
                (None, None) => Err(format!("flow `{op}` produced no outputs")),
                (Some(_), Some(_)) => {
                    Err(format!("flow `{op}` produced both emit and fail outputs"))
                }
            };
        }
    }

    // I/O ops go to host
    if host::is_io_op(op) {
        return host.execute_io_op(op, args);
    }

    // Pure ops
    pure_ops::execute_pure_op(op, args, codecs)
}

fn eval_expr(
    expr: &Expr,
    vars: &HashMap<String, Value>,
    flow_registry: Option<&FlowRegistry>,
    host: &dyn SyncHost,
    codecs: &CodecRegistry,
) -> Result<Value, String> {
    match expr {
        Expr::Lit(v) => Ok(v.clone()),
        Expr::Var(name) => {
            resolve_var_path(vars, name).ok_or_else(|| format!("Unknown variable path `{name}`"))
        }
        Expr::BinOp { op, lhs, rhs } => {
            let l = eval_expr(lhs, vars, flow_registry, host, codecs)?;
            let r = eval_expr(rhs, vars, flow_registry, host, codecs)?;
            eval_binop(*op, &l, &r)
        }
        Expr::UnaryOp { op, expr: inner } => {
            let v = eval_expr(inner, vars, flow_registry, host, codecs)?;
            eval_unary(*op, &v)
        }
        Expr::Call { func, args } => {
            let mut evaluated = Vec::with_capacity(args.len());
            for a in args {
                evaluated.push(eval_expr(a, vars, flow_registry, host, codecs)?);
            }
            dispatch_op(func, &evaluated, flow_registry, host, codecs)
        }
        Expr::Interp(parts) => {
            let mut result = String::new();
            for part in parts {
                match part {
                    InterpExpr::Lit(s) => result.push_str(s),
                    InterpExpr::Expr(e) => {
                        let val = eval_expr(e, vars, flow_registry, host, codecs)?;
                        match &val {
                            Value::String(s) => result.push_str(s),
                            Value::Number(n) => result.push_str(&n.to_string()),
                            Value::Bool(b) => result.push_str(if *b { "true" } else { "false" }),
                            Value::Null => result.push_str("null"),
                            other => result.push_str(&other.to_string()),
                        }
                    }
                }
            }
            Ok(json!(result))
        }
        Expr::Ternary {
            cond,
            then_expr,
            else_expr,
        } => {
            let cond_val = eval_expr(cond, vars, flow_registry, host, codecs)?;
            let is_truthy = match &cond_val {
                Value::Bool(b) => *b,
                Value::Null => false,
                Value::String(s) => !s.is_empty(),
                Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
                _ => true,
            };
            if is_truthy {
                eval_expr(then_expr, vars, flow_registry, host, codecs)
            } else {
                eval_expr(else_expr, vars, flow_registry, host, codecs)
            }
        }
        Expr::ListLit(items) => {
            let mut arr = Vec::with_capacity(items.len());
            for item in items {
                arr.push(eval_expr(item, vars, flow_registry, host, codecs)?);
            }
            Ok(Value::Array(arr))
        }
        Expr::DictLit(pairs) => {
            let mut map = serde_json::Map::new();
            for (key, val_expr) in pairs {
                let val = eval_expr(val_expr, vars, flow_registry, host, codecs)?;
                map.insert(key.clone(), val);
            }
            Ok(Value::Object(map))
        }
        Expr::Index { expr, index } => {
            let collection = eval_expr(expr, vars, flow_registry, host, codecs)?;
            let idx = eval_expr(index, vars, flow_registry, host, codecs)?;
            match &collection {
                Value::Array(arr) => {
                    let i = idx.as_i64().or_else(|| {
                        idx.as_f64().and_then(|f| { let r = f as i64; if r as f64 == f { Some(r) } else { None } })
                    }).ok_or_else(|| format!("Index must be an integer, got {idx}"))?;
                    let len = arr.len() as i64;
                    let resolved = if i < 0 { len + i } else { i };
                    if resolved < 0 || resolved >= len {
                        return Err(format!("Index {i} out of bounds (len={len})"));
                    }
                    Ok(arr[resolved as usize].clone())
                }
                Value::Object(map) => {
                    let key = idx.as_str().ok_or_else(|| format!("Dict key must be a string, got {idx}"))?;
                    map.get(key).cloned().ok_or_else(|| format!("Key \"{key}\" not found"))
                }
                _ => Err(format!("Cannot index into {}", collection)),
            }
        }
    }
}

fn eval_binop(op: BinOp, l: &Value, r: &Value) -> Result<Value, String> {
    match op {
        BinOp::Add => {
            if let (Some(a), Some(b)) = (l.as_f64(), r.as_f64()) {
                if l.is_i64() && r.is_i64() {
                    return Ok(json!(l.as_i64().unwrap() + r.as_i64().unwrap()));
                }
                return Ok(json!(a + b));
            }
            if let (Some(a), Some(b)) = (l.as_str(), r.as_str()) {
                return Ok(json!(format!("{a}{b}")));
            }
            // Coerce to string if one side is string
            if let Some(s) = l.as_str() {
                return Ok(json!(format!("{s}{}", value_to_string(r))));
            }
            if let Some(s) = r.as_str() {
                return Ok(json!(format!("{}{s}", value_to_string(l))));
            }
            Err(format!("Cannot add {l} and {r}"))
        }
        BinOp::Sub => num_binop(l, r, |a, b| a - b, "subtract"),
        BinOp::Mul => num_binop(l, r, |a, b| a * b, "multiply"),
        BinOp::Div => {
            let a = l.as_f64().ok_or("Division requires numbers")?;
            let b = r.as_f64().ok_or("Division requires numbers")?;
            if b == 0.0 {
                return Err("Division by zero".to_string());
            }
            if l.is_i64() && r.is_i64() && l.as_i64().unwrap() % r.as_i64().unwrap() == 0 {
                return Ok(json!(l.as_i64().unwrap() / r.as_i64().unwrap()));
            }
            Ok(json!(a / b))
        }
        BinOp::Mod => {
            if let (Some(a), Some(b)) = (l.as_i64(), r.as_i64()) {
                if b == 0 {
                    return Err("Modulo by zero".to_string());
                }
                return Ok(json!(a % b));
            }
            let a = l.as_f64().ok_or("Mod requires numbers")?;
            let b = r.as_f64().ok_or("Mod requires numbers")?;
            if b == 0.0 {
                return Err("Modulo by zero".to_string());
            }
            Ok(json!(a % b))
        }
        BinOp::Pow => {
            let a = l.as_f64().ok_or("Power requires numbers")?;
            let b = r.as_f64().ok_or("Power requires numbers")?;
            Ok(json!(a.powf(b)))
        }
        BinOp::Eq => Ok(json!(values_equal(l, r))),
        BinOp::Neq => Ok(json!(!values_equal(l, r))),
        BinOp::Lt => compare_values(l, r, |ord| ord == std::cmp::Ordering::Less),
        BinOp::Gt => compare_values(l, r, |ord| ord == std::cmp::Ordering::Greater),
        BinOp::LtEq => compare_values(l, r, |ord| ord != std::cmp::Ordering::Greater),
        BinOp::GtEq => compare_values(l, r, |ord| ord != std::cmp::Ordering::Less),
        BinOp::And => {
            let lb = is_truthy(l);
            if !lb {
                return Ok(json!(false));
            }
            Ok(json!(is_truthy(r)))
        }
        BinOp::Or => {
            if is_truthy(l) {
                return Ok(json!(true));
            }
            Ok(json!(is_truthy(r)))
        }
    }
}

fn eval_unary(op: UnaryOp, v: &Value) -> Result<Value, String> {
    match op {
        UnaryOp::Neg => {
            if let Some(i) = v.as_i64() {
                return Ok(json!(-i));
            }
            if let Some(f) = v.as_f64() {
                return Ok(json!(-f));
            }
            Err(format!("Cannot negate {v}"))
        }
        UnaryOp::Not => Ok(json!(!is_truthy(v))),
    }
}

fn pattern_matches(value: &Value, pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Lit(lit) => values_equal(value, lit),
        Pattern::Ident(name) => {
            if name == "_" {
                return true;
            }
            if let Some(s) = value.as_str() {
                s == name
            } else {
                false
            }
        }
    }
}

fn resolve_var_path(vars: &HashMap<String, Value>, path: &str) -> Option<Value> {
    let mut parts = path.split('.');
    let first = parts.next()?;
    let mut current = vars.get(first)?.clone();
    for part in parts {
        let obj = current.as_object()?;
        current = obj.get(part)?.clone();
    }
    Some(current)
}

fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        _ => a == b,
    }
}

fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Null => false,
        Value::String(s) => !s.is_empty(),
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        _ => true,
    }
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else {
                n.to_string()
            }
        }
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn num_binop(l: &Value, r: &Value, f: fn(f64, f64) -> f64, name: &str) -> Result<Value, String> {
    let a = l
        .as_f64()
        .ok_or_else(|| format!("{name} requires numbers"))?;
    let b = r
        .as_f64()
        .ok_or_else(|| format!("{name} requires numbers"))?;
    let result = f(a, b);
    if l.is_i64() && r.is_i64() {
        let ri = result as i64;
        if ri as f64 == result {
            return Ok(json!(ri));
        }
    }
    Ok(json!(result))
}

fn compare_values(
    l: &Value,
    r: &Value,
    pred: fn(std::cmp::Ordering) -> bool,
) -> Result<Value, String> {
    if let (Some(a), Some(b)) = (l.as_f64(), r.as_f64()) {
        return Ok(json!(pred(
            a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
        )));
    }
    if let (Some(a), Some(b)) = (l.as_str(), r.as_str()) {
        return Ok(json!(pred(a.cmp(b))));
    }
    Err(format!("Cannot compare {l} and {r}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;
    use crate::codec::CodecRegistry;
    use crate::ir::Ir;
    use crate::loader::{FlowProgram, FlowRegistry};
    use crate::types::TypeRegistry;
    use serde_json::json;

    // ---- NullHost: rejects all I/O ----

    struct NullHost;

    impl SyncHost for NullHost {
        fn execute_io_op(&self, op: &str, _args: &[Value]) -> Result<Value, String> {
            Err(format!("no I/O in tests: {op}"))
        }
        fn cleanup(&self) {}
    }

    // ---- Helpers to build AST nodes concisely ----

    fn dummy_ir() -> Ir {
        Ir {
            forai_ir: "0.1".to_string(),
            flow: "test".to_string(),
            inputs: vec![],
            outputs: vec![],
            nodes: vec![],
            edges: vec![],
            emits: vec![],
        }
    }

    fn make_flow(
        name: &str,
        inputs: Vec<(&str, &str)>,
        outputs: Vec<(&str, &str)>,
        body: Vec<Statement>,
    ) -> Flow {
        Flow {
            name: name.to_string(),
            inputs: inputs
                .iter()
                .map(|(n, t)| Port {
                    name: n.to_string(),
                    type_name: t.to_string(),
                })
                .collect(),
            outputs: outputs
                .iter()
                .map(|(n, t)| Port {
                    name: n.to_string(),
                    type_name: t.to_string(),
                })
                .collect(),
            body,
        }
    }

    fn lit(v: Value) -> Expr {
        Expr::Lit(v)
    }
    fn var(name: &str) -> Expr {
        Expr::Var(name.to_string())
    }
    fn binop(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
        Expr::BinOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        }
    }
    fn unary(op: UnaryOp, expr: Expr) -> Expr {
        Expr::UnaryOp {
            op,
            expr: Box::new(expr),
        }
    }
    fn call(func: &str, args: Vec<Expr>) -> Expr {
        Expr::Call {
            func: func.to_string(),
            args,
        }
    }
    fn ternary(cond: Expr, then_e: Expr, else_e: Expr) -> Expr {
        Expr::Ternary {
            cond: Box::new(cond),
            then_expr: Box::new(then_e),
            else_expr: Box::new(else_e),
        }
    }
    fn index(expr: Expr, idx: Expr) -> Expr {
        Expr::Index {
            expr: Box::new(expr),
            index: Box::new(idx),
        }
    }
    fn interp(parts: Vec<InterpExpr>) -> Expr {
        Expr::Interp(parts)
    }
    fn list_lit(items: Vec<Expr>) -> Expr {
        Expr::ListLit(items)
    }
    fn dict_lit(pairs: Vec<(&str, Expr)>) -> Expr {
        Expr::DictLit(
            pairs
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        )
    }

    fn assign(name: &str, expr: Expr) -> Statement {
        Statement::ExprAssign(ExprAssign {
            bind: name.to_string(),
            expr,
        })
    }
    fn emit(output: &str, expr: Expr) -> Statement {
        Statement::Emit(Emit {
            output: output.to_string(),
            value_expr: expr,
        })
    }
    fn node(bind: &str, op: &str, args: Vec<Arg>) -> Statement {
        Statement::Node(NodeAssign {
            bind: bind.to_string(),
            node_id: bind.to_string(),
            op: op.to_string(),
            args,
        })
    }
    fn arg_lit(v: Value) -> Arg {
        Arg::Lit { lit: v }
    }
    fn arg_var(name: &str) -> Arg {
        Arg::Var {
            var: name.to_string(),
        }
    }

    /// Run a flow with given body statements, returning the first emit output value.
    fn run_body(body: Vec<Statement>) -> Value {
        let flow = make_flow("Test", vec![], vec![("result", "Any")], body);
        let result = execute_flow(
            &flow,
            dummy_ir(),
            HashMap::new(),
            &TypeRegistry::empty(),
            None,
            &CodecRegistry::default_registry(),
            &NullHost,
        )
        .unwrap();
        let outputs = result.outputs.as_object().unwrap();
        outputs.get("result").cloned().unwrap_or(Value::Null)
    }

    /// Run a flow with inputs, returning the first emit output value.
    /// Note: input ports are left empty to skip type validation in tests.
    /// The inputs HashMap is used directly as the variable scope.
    fn run_body_with_inputs(
        body: Vec<Statement>,
        inputs: Vec<(&str, Value)>,
    ) -> Value {
        let flow = make_flow("Test", vec![], vec![("result", "Any")], body);
        let input_map: HashMap<String, Value> = inputs
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        let result = execute_flow(
            &flow,
            dummy_ir(),
            input_map,
            &TypeRegistry::empty(),
            None,
            &CodecRegistry::default_registry(),
            &NullHost,
        )
        .unwrap();
        let outputs = result.outputs.as_object().unwrap();
        outputs.get("result").cloned().unwrap_or(Value::Null)
    }

    // =========================================================
    // Expressions: Arithmetic
    // =========================================================

    #[test]
    fn arithmetic_add_integers() {
        let v = run_body(vec![
            assign("x", binop(BinOp::Add, lit(json!(3)), lit(json!(4)))),
            emit("result", var("x")),
        ]);
        assert_eq!(v, json!(7));
        assert!(v.is_i64(), "3 + 4 should stay integer, got {v}");
    }

    #[test]
    fn arithmetic_add_floats() {
        let v = run_body(vec![
            assign("x", binop(BinOp::Add, lit(json!(1.5)), lit(json!(2.5)))),
            emit("result", var("x")),
        ]);
        assert_eq!(v, json!(4.0));
    }

    #[test]
    fn arithmetic_sub() {
        let v = run_body(vec![
            assign("x", binop(BinOp::Sub, lit(json!(10)), lit(json!(3)))),
            emit("result", var("x")),
        ]);
        assert_eq!(v, json!(7));
    }

    #[test]
    fn arithmetic_mul() {
        let v = run_body(vec![
            assign("x", binop(BinOp::Mul, lit(json!(6)), lit(json!(7)))),
            emit("result", var("x")),
        ]);
        assert_eq!(v, json!(42));
    }

    #[test]
    fn arithmetic_div_exact() {
        let v = run_body(vec![
            assign("x", binop(BinOp::Div, lit(json!(10)), lit(json!(2)))),
            emit("result", var("x")),
        ]);
        assert_eq!(v, json!(5));
        assert!(v.is_i64(), "10 / 2 should stay integer");
    }

    #[test]
    fn arithmetic_div_fractional() {
        let v = run_body(vec![
            assign("x", binop(BinOp::Div, lit(json!(7)), lit(json!(2)))),
            emit("result", var("x")),
        ]);
        assert_eq!(v, json!(3.5));
    }

    #[test]
    fn arithmetic_mod() {
        let v = run_body(vec![
            assign("x", binop(BinOp::Mod, lit(json!(10)), lit(json!(3)))),
            emit("result", var("x")),
        ]);
        assert_eq!(v, json!(1));
    }

    #[test]
    fn arithmetic_pow() {
        let v = run_body(vec![
            assign("x", binop(BinOp::Pow, lit(json!(2)), lit(json!(10)))),
            emit("result", var("x")),
        ]);
        assert_eq!(v, json!(1024.0));
    }

    #[test]
    fn integer_preservation_add() {
        // 3 + 4 should be 7 (integer), not 7.0
        let v = run_body(vec![emit(
            "result",
            binop(BinOp::Add, lit(json!(3)), lit(json!(4))),
        )]);
        assert_eq!(v, json!(7));
        assert!(v.is_i64());
    }

    #[test]
    fn integer_preservation_mul() {
        let v = run_body(vec![emit(
            "result",
            binop(BinOp::Mul, lit(json!(3)), lit(json!(4))),
        )]);
        assert_eq!(v, json!(12));
        assert!(v.is_i64());
    }

    // =========================================================
    // Expressions: String concatenation
    // =========================================================

    #[test]
    fn string_concat() {
        let v = run_body(vec![emit(
            "result",
            binop(BinOp::Add, lit(json!("hello ")), lit(json!("world"))),
        )]);
        assert_eq!(v, json!("hello world"));
    }

    #[test]
    fn string_concat_with_number() {
        let v = run_body(vec![emit(
            "result",
            binop(BinOp::Add, lit(json!("count: ")), lit(json!(42))),
        )]);
        assert_eq!(v, json!("count: 42"));
    }

    // =========================================================
    // Expressions: Comparisons
    // =========================================================

    #[test]
    fn comparison_eq() {
        let v = run_body(vec![emit(
            "result",
            binop(BinOp::Eq, lit(json!(5)), lit(json!(5))),
        )]);
        assert_eq!(v, json!(true));
    }

    #[test]
    fn comparison_neq() {
        let v = run_body(vec![emit(
            "result",
            binop(BinOp::Neq, lit(json!(5)), lit(json!(3))),
        )]);
        assert_eq!(v, json!(true));
    }

    #[test]
    fn comparison_lt() {
        let v = run_body(vec![emit(
            "result",
            binop(BinOp::Lt, lit(json!(3)), lit(json!(5))),
        )]);
        assert_eq!(v, json!(true));
    }

    #[test]
    fn comparison_gt() {
        let v = run_body(vec![emit(
            "result",
            binop(BinOp::Gt, lit(json!(5)), lit(json!(3))),
        )]);
        assert_eq!(v, json!(true));
    }

    #[test]
    fn comparison_lteq() {
        assert_eq!(
            run_body(vec![emit(
                "result",
                binop(BinOp::LtEq, lit(json!(5)), lit(json!(5))),
            )]),
            json!(true)
        );
        assert_eq!(
            run_body(vec![emit(
                "result",
                binop(BinOp::LtEq, lit(json!(6)), lit(json!(5))),
            )]),
            json!(false)
        );
    }

    #[test]
    fn comparison_gteq() {
        assert_eq!(
            run_body(vec![emit(
                "result",
                binop(BinOp::GtEq, lit(json!(5)), lit(json!(5))),
            )]),
            json!(true)
        );
    }

    #[test]
    fn comparison_strings() {
        let v = run_body(vec![emit(
            "result",
            binop(BinOp::Lt, lit(json!("abc")), lit(json!("def"))),
        )]);
        assert_eq!(v, json!(true));
    }

    // =========================================================
    // Expressions: Logical operators
    // =========================================================

    #[test]
    fn logical_and() {
        assert_eq!(
            run_body(vec![emit(
                "result",
                binop(BinOp::And, lit(json!(true)), lit(json!(true))),
            )]),
            json!(true)
        );
        assert_eq!(
            run_body(vec![emit(
                "result",
                binop(BinOp::And, lit(json!(true)), lit(json!(false))),
            )]),
            json!(false)
        );
        assert_eq!(
            run_body(vec![emit(
                "result",
                binop(BinOp::And, lit(json!(false)), lit(json!(true))),
            )]),
            json!(false)
        );
    }

    #[test]
    fn logical_or() {
        assert_eq!(
            run_body(vec![emit(
                "result",
                binop(BinOp::Or, lit(json!(false)), lit(json!(true))),
            )]),
            json!(true)
        );
        assert_eq!(
            run_body(vec![emit(
                "result",
                binop(BinOp::Or, lit(json!(false)), lit(json!(false))),
            )]),
            json!(false)
        );
    }

    #[test]
    fn logical_not() {
        assert_eq!(
            run_body(vec![emit("result", unary(UnaryOp::Not, lit(json!(true))))]),
            json!(false)
        );
        assert_eq!(
            run_body(vec![emit(
                "result",
                unary(UnaryOp::Not, lit(json!(false))),
            )]),
            json!(true)
        );
    }

    // =========================================================
    // Expressions: Ternary
    // =========================================================

    #[test]
    fn ternary_true_branch() {
        let v = run_body(vec![emit(
            "result",
            ternary(lit(json!(true)), lit(json!("yes")), lit(json!("no"))),
        )]);
        assert_eq!(v, json!("yes"));
    }

    #[test]
    fn ternary_false_branch() {
        let v = run_body(vec![emit(
            "result",
            ternary(lit(json!(false)), lit(json!("yes")), lit(json!("no"))),
        )]);
        assert_eq!(v, json!("no"));
    }

    #[test]
    fn ternary_with_comparison() {
        let v = run_body_with_inputs(
            vec![emit(
                "result",
                ternary(
                    binop(BinOp::Gt, var("x"), lit(json!(0))),
                    lit(json!("pos")),
                    lit(json!("neg")),
                ),
            )],
            vec![("x", json!(5))],
        );
        assert_eq!(v, json!("pos"));
    }

    // =========================================================
    // Expressions: Unary negation
    // =========================================================

    #[test]
    fn unary_negation_int() {
        let v = run_body(vec![emit("result", unary(UnaryOp::Neg, lit(json!(42))))]);
        assert_eq!(v, json!(-42));
    }

    #[test]
    fn unary_negation_float() {
        let v = run_body(vec![emit(
            "result",
            unary(UnaryOp::Neg, lit(json!(3.14))),
        )]);
        assert_eq!(v, json!(-3.14));
    }

    // =========================================================
    // Expressions: String interpolation
    // =========================================================

    #[test]
    fn string_interpolation() {
        let v = run_body_with_inputs(
            vec![emit(
                "result",
                interp(vec![
                    InterpExpr::Lit("hello ".to_string()),
                    InterpExpr::Expr(Box::new(var("name"))),
                    InterpExpr::Lit("!".to_string()),
                ]),
            )],
            vec![("name", json!("world"))],
        );
        assert_eq!(v, json!("hello world!"));
    }

    #[test]
    fn string_interpolation_with_number() {
        let v = run_body_with_inputs(
            vec![emit(
                "result",
                interp(vec![
                    InterpExpr::Lit("count=".to_string()),
                    InterpExpr::Expr(Box::new(var("n"))),
                ]),
            )],
            vec![("n", json!(42))],
        );
        assert_eq!(v, json!("count=42"));
    }

    // =========================================================
    // Indexing
    // =========================================================

    #[test]
    fn array_index_zero() {
        let v = run_body(vec![
            assign("items", list_lit(vec![lit(json!(10)), lit(json!(20)), lit(json!(30))])),
            emit("result", index(var("items"), lit(json!(0)))),
        ]);
        assert_eq!(v, json!(10));
    }

    #[test]
    fn array_index_negative() {
        let v = run_body(vec![
            assign("items", list_lit(vec![lit(json!(10)), lit(json!(20)), lit(json!(30))])),
            emit("result", index(var("items"), lit(json!(-1)))),
        ]);
        assert_eq!(v, json!(30));
    }

    #[test]
    fn array_index_float_as_int() {
        // items[2.0] should work when 2.0 is a whole number (from arithmetic)
        let v = run_body(vec![
            assign("items", list_lit(vec![lit(json!("a")), lit(json!("b")), lit(json!("c"))])),
            emit("result", index(var("items"), lit(json!(2.0)))),
        ]);
        assert_eq!(v, json!("c"));
    }

    #[test]
    fn dict_index() {
        let v = run_body(vec![
            assign("row", dict_lit(vec![("name", lit(json!("Alice"))), ("age", lit(json!(30)))])),
            emit("result", index(var("row"), lit(json!("name")))),
        ]);
        assert_eq!(v, json!("Alice"));
    }

    #[test]
    fn array_index_out_of_bounds() {
        let flow = make_flow(
            "Test",
            vec![],
            vec![("result", "Any")],
            vec![
                assign("items", list_lit(vec![lit(json!(1))])),
                emit("result", index(var("items"), lit(json!(5)))),
            ],
        );
        let result = execute_flow(
            &flow,
            dummy_ir(),
            HashMap::new(),
            &TypeRegistry::empty(),
            None,
            &CodecRegistry::default_registry(),
            &NullHost,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("out of bounds"));
    }

    // =========================================================
    // Control flow: case/when
    // =========================================================

    #[test]
    fn case_when_literal_match() {
        let v = run_body_with_inputs(
            vec![
                Statement::Case(CaseBlock {
                    expr: var("color"),
                    arms: vec![
                        CaseArm {
                            pattern: Pattern::Lit(json!("red")),
                            body: vec![emit("result", lit(json!(1)))],
                        },
                        CaseArm {
                            pattern: Pattern::Lit(json!("blue")),
                            body: vec![emit("result", lit(json!(2)))],
                        },
                    ],
                    else_body: vec![emit("result", lit(json!(0)))],
                }),
            ],
            vec![("color", json!("blue"))],
        );
        assert_eq!(v, json!(2));
    }

    #[test]
    fn case_when_else() {
        let v = run_body_with_inputs(
            vec![Statement::Case(CaseBlock {
                expr: var("color"),
                arms: vec![CaseArm {
                    pattern: Pattern::Lit(json!("red")),
                    body: vec![emit("result", lit(json!(1)))],
                }],
                else_body: vec![emit("result", lit(json!(99)))],
            })],
            vec![("color", json!("green"))],
        );
        assert_eq!(v, json!(99));
    }

    #[test]
    fn case_when_ident_pattern() {
        // Ident pattern matches the string value
        let v = run_body_with_inputs(
            vec![Statement::Case(CaseBlock {
                expr: var("cmd"),
                arms: vec![
                    CaseArm {
                        pattern: Pattern::Ident("quit".to_string()),
                        body: vec![emit("result", lit(json!("bye")))],
                    },
                    CaseArm {
                        pattern: Pattern::Ident("_".to_string()),
                        body: vec![emit("result", lit(json!("unknown")))],
                    },
                ],
                else_body: vec![],
            })],
            vec![("cmd", json!("quit"))],
        );
        assert_eq!(v, json!("bye"));
    }

    #[test]
    fn case_wildcard_matches_anything() {
        let v = run_body_with_inputs(
            vec![Statement::Case(CaseBlock {
                expr: var("x"),
                arms: vec![CaseArm {
                    pattern: Pattern::Ident("_".to_string()),
                    body: vec![emit("result", lit(json!("matched")))],
                }],
                else_body: vec![],
            })],
            vec![("x", json!(12345))],
        );
        assert_eq!(v, json!("matched"));
    }

    // =========================================================
    // Control flow: if/else (desugars to case)
    // =========================================================

    #[test]
    fn if_else_true() {
        // if x > 0 → true branch; desugared as case(expr) when true → body
        let v = run_body_with_inputs(
            vec![Statement::Case(CaseBlock {
                expr: binop(BinOp::Gt, var("x"), lit(json!(0))),
                arms: vec![CaseArm {
                    pattern: Pattern::Lit(json!(true)),
                    body: vec![emit("result", lit(json!("positive")))],
                }],
                else_body: vec![emit("result", lit(json!("non-positive")))],
            })],
            vec![("x", json!(5))],
        );
        assert_eq!(v, json!("positive"));
    }

    #[test]
    fn if_else_false() {
        let v = run_body_with_inputs(
            vec![Statement::Case(CaseBlock {
                expr: binop(BinOp::Gt, var("x"), lit(json!(0))),
                arms: vec![CaseArm {
                    pattern: Pattern::Lit(json!(true)),
                    body: vec![emit("result", lit(json!("positive")))],
                }],
                else_body: vec![emit("result", lit(json!("non-positive")))],
            })],
            vec![("x", json!(-3))],
        );
        assert_eq!(v, json!("non-positive"));
    }

    // =========================================================
    // Control flow: loop
    // =========================================================

    #[test]
    fn loop_sum() {
        // sum = 0; loop [1,2,3] as n → sum = sum + n; emit sum
        let v = run_body(vec![
            assign("sum", lit(json!(0))),
            Statement::Loop(LoopBlock {
                collection: list_lit(vec![lit(json!(1)), lit(json!(2)), lit(json!(3))]),
                item: "n".to_string(),
                body: vec![assign("sum", binop(BinOp::Add, var("sum"), var("n")))],
            }),
            emit("result", var("sum")),
        ]);
        assert_eq!(v, json!(6));
    }

    #[test]
    fn loop_with_break() {
        // bare loop: increment counter, break when counter == 3
        let v = run_body(vec![
            assign("counter", lit(json!(0))),
            Statement::BareLoop(BareLoopBlock {
                body: vec![
                    assign(
                        "counter",
                        binop(BinOp::Add, var("counter"), lit(json!(1))),
                    ),
                    Statement::Case(CaseBlock {
                        expr: binop(BinOp::Eq, var("counter"), lit(json!(3))),
                        arms: vec![CaseArm {
                            pattern: Pattern::Lit(json!(true)),
                            body: vec![Statement::Break],
                        }],
                        else_body: vec![],
                    }),
                ],
            }),
            emit("result", var("counter")),
        ]);
        assert_eq!(v, json!(3));
    }

    #[test]
    fn nested_case_in_loop() {
        // Loop over items, classify each with case, accumulate result
        let v = run_body(vec![
            assign("out", lit(json!(""))),
            Statement::Loop(LoopBlock {
                collection: list_lit(vec![lit(json!(1)), lit(json!(2)), lit(json!(3))]),
                item: "n".to_string(),
                body: vec![Statement::Case(CaseBlock {
                    expr: binop(BinOp::Eq, var("n"), lit(json!(2))),
                    arms: vec![CaseArm {
                        pattern: Pattern::Lit(json!(true)),
                        body: vec![assign(
                            "out",
                            binop(BinOp::Add, var("out"), lit(json!("two "))),
                        )],
                    }],
                    else_body: vec![assign(
                        "out",
                        binop(BinOp::Add, var("out"), lit(json!("other "))),
                    )],
                })],
            }),
            emit("result", var("out")),
        ]);
        assert_eq!(v, json!("other two other "));
    }

    // =========================================================
    // Data structures: list and dict literals
    // =========================================================

    #[test]
    fn list_literal() {
        let v = run_body(vec![emit(
            "result",
            list_lit(vec![lit(json!(1)), lit(json!(2)), lit(json!(3))]),
        )]);
        assert_eq!(v, json!([1, 2, 3]));
    }

    #[test]
    fn dict_literal() {
        let v = run_body(vec![emit(
            "result",
            dict_lit(vec![("a", lit(json!(1))), ("b", lit(json!(2)))]),
        )]);
        assert_eq!(v, json!({"a": 1, "b": 2}));
    }

    // =========================================================
    // Pure ops: obj.*
    // =========================================================

    #[test]
    fn obj_new_and_set_and_get() {
        let v = run_body(vec![
            node("o", "obj.new", vec![]),
            node("o2", "obj.set", vec![arg_var("o"), arg_lit(json!("x")), arg_lit(json!(42))]),
            node("val", "obj.get", vec![arg_var("o2"), arg_lit(json!("x"))]),
            emit("result", var("val")),
        ]);
        assert_eq!(v, json!(42));
    }

    #[test]
    fn obj_has() {
        let v = run_body(vec![
            node("o", "obj.new", vec![]),
            node("o2", "obj.set", vec![arg_var("o"), arg_lit(json!("k")), arg_lit(json!(1))]),
            node("yes", "obj.has", vec![arg_var("o2"), arg_lit(json!("k"))]),
            node("no", "obj.has", vec![arg_var("o2"), arg_lit(json!("z"))]),
            emit(
                "result",
                list_lit(vec![var("yes"), var("no")]),
            ),
        ]);
        assert_eq!(v, json!([true, false]));
    }

    #[test]
    fn obj_keys() {
        let v = run_body(vec![
            assign("o", dict_lit(vec![("a", lit(json!(1))), ("b", lit(json!(2)))])),
            node("ks", "obj.keys", vec![arg_var("o")]),
            emit("result", var("ks")),
        ]);
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert!(arr.contains(&json!("a")));
        assert!(arr.contains(&json!("b")));
    }

    #[test]
    fn obj_delete() {
        let v = run_body(vec![
            assign("o", dict_lit(vec![("a", lit(json!(1))), ("b", lit(json!(2)))])),
            node("o2", "obj.delete", vec![arg_var("o"), arg_lit(json!("a"))]),
            node("has_a", "obj.has", vec![arg_var("o2"), arg_lit(json!("a"))]),
            emit("result", var("has_a")),
        ]);
        assert_eq!(v, json!(false));
    }

    #[test]
    fn obj_merge() {
        let v = run_body(vec![
            assign("a", dict_lit(vec![("x", lit(json!(1)))])),
            assign("b", dict_lit(vec![("y", lit(json!(2)))])),
            node("merged", "obj.merge", vec![arg_var("a"), arg_var("b")]),
            emit("result", var("merged")),
        ]);
        assert_eq!(v, json!({"x": 1, "y": 2}));
    }

    // =========================================================
    // Pure ops: list.*
    // =========================================================

    #[test]
    fn list_new_and_append() {
        let v = run_body(vec![
            node("l", "list.new", vec![]),
            node("l2", "list.append", vec![arg_var("l"), arg_lit(json!(42))]),
            emit("result", var("l2")),
        ]);
        assert_eq!(v, json!([42]));
    }

    #[test]
    fn list_len() {
        let v = run_body(vec![
            assign("l", list_lit(vec![lit(json!(1)), lit(json!(2)), lit(json!(3))])),
            node("n", "list.len", vec![arg_var("l")]),
            emit("result", var("n")),
        ]);
        assert_eq!(v, json!(3));
    }

    #[test]
    fn list_contains() {
        let v = run_body(vec![
            assign("l", list_lit(vec![lit(json!("a")), lit(json!("b"))])),
            node("yes", "list.contains", vec![arg_var("l"), arg_lit(json!("a"))]),
            node("no", "list.contains", vec![arg_var("l"), arg_lit(json!("z"))]),
            emit("result", list_lit(vec![var("yes"), var("no")])),
        ]);
        assert_eq!(v, json!([true, false]));
    }

    #[test]
    fn list_slice() {
        let v = run_body(vec![
            assign("l", list_lit(vec![lit(json!(10)), lit(json!(20)), lit(json!(30)), lit(json!(40))])),
            node("s", "list.slice", vec![arg_var("l"), arg_lit(json!(1)), arg_lit(json!(3))]),
            emit("result", var("s")),
        ]);
        assert_eq!(v, json!([20, 30]));
    }

    #[test]
    fn list_range() {
        // list.range is inclusive: range(0, 3) → [0, 1, 2, 3]
        let v = run_body(vec![
            node("r", "list.range", vec![arg_lit(json!(0)), arg_lit(json!(3))]),
            emit("result", var("r")),
        ]);
        assert_eq!(v, json!([0, 1, 2, 3]));
    }

    // =========================================================
    // Pure ops: str.*
    // =========================================================

    #[test]
    fn str_len() {
        let v = run_body(vec![
            node("n", "str.len", vec![arg_lit(json!("hello"))]),
            emit("result", var("n")),
        ]);
        assert_eq!(v, json!(5));
    }

    #[test]
    fn str_upper_lower() {
        let v = run_body(vec![
            node("u", "str.upper", vec![arg_lit(json!("hello"))]),
            node("l", "str.lower", vec![arg_lit(json!("WORLD"))]),
            emit("result", list_lit(vec![var("u"), var("l")])),
        ]);
        assert_eq!(v, json!(["HELLO", "world"]));
    }

    #[test]
    fn str_trim() {
        let v = run_body(vec![
            node("t", "str.trim", vec![arg_lit(json!("  hi  "))]),
            emit("result", var("t")),
        ]);
        assert_eq!(v, json!("hi"));
    }

    #[test]
    fn str_split_join() {
        let v = run_body(vec![
            node("parts", "str.split", vec![arg_lit(json!("a,b,c")), arg_lit(json!(","))]),
            node("joined", "str.join", vec![arg_var("parts"), arg_lit(json!("-"))]),
            emit("result", var("joined")),
        ]);
        assert_eq!(v, json!("a-b-c"));
    }

    #[test]
    fn str_replace() {
        let v = run_body(vec![
            node(
                "out",
                "str.replace",
                vec![arg_lit(json!("hello world")), arg_lit(json!("world")), arg_lit(json!("rust"))],
            ),
            emit("result", var("out")),
        ]);
        assert_eq!(v, json!("hello rust"));
    }

    #[test]
    fn str_contains() {
        let v = run_body(vec![
            node("yes", "str.contains", vec![arg_lit(json!("foobar")), arg_lit(json!("bar"))]),
            node("no", "str.contains", vec![arg_lit(json!("foobar")), arg_lit(json!("baz"))]),
            emit("result", list_lit(vec![var("yes"), var("no")])),
        ]);
        assert_eq!(v, json!([true, false]));
    }

    #[test]
    fn str_starts_ends_with() {
        let v = run_body(vec![
            node("sw", "str.starts_with", vec![arg_lit(json!("hello world")), arg_lit(json!("hello"))]),
            node("ew", "str.ends_with", vec![arg_lit(json!("hello world")), arg_lit(json!("world"))]),
            emit("result", list_lit(vec![var("sw"), var("ew")])),
        ]);
        assert_eq!(v, json!([true, true]));
    }

    #[test]
    fn str_slice() {
        let v = run_body(vec![
            node("s", "str.slice", vec![arg_lit(json!("hello")), arg_lit(json!(1)), arg_lit(json!(4))]),
            emit("result", var("s")),
        ]);
        assert_eq!(v, json!("ell"));
    }

    // =========================================================
    // Pure ops: type conversion and introspection
    // =========================================================

    #[test]
    fn type_of_values() {
        let v = run_body(vec![
            node("t1", "type.of", vec![arg_lit(json!("hi"))]),
            node("t2", "type.of", vec![arg_lit(json!(42))]),
            node("t3", "type.of", vec![arg_lit(json!(true))]),
            node("t4", "type.of", vec![arg_lit(json!(null))]),
            emit("result", list_lit(vec![var("t1"), var("t2"), var("t3"), var("t4")])),
        ]);
        assert_eq!(v, json!(["text", "long", "bool", "void"]));
    }

    #[test]
    fn to_text_conversion() {
        let v = run_body(vec![
            node("s", "to.text", vec![arg_lit(json!(42))]),
            emit("result", var("s")),
        ]);
        assert_eq!(v, json!("42"));
    }

    #[test]
    fn to_long_conversion() {
        let v = run_body(vec![
            node("n", "to.long", vec![arg_lit(json!("123"))]),
            emit("result", var("n")),
        ]);
        assert_eq!(v, json!(123));
    }

    // =========================================================
    // Pure ops: json codec
    // =========================================================

    #[test]
    fn json_decode_encode() {
        let v = run_body(vec![
            node("parsed", "json.decode", vec![arg_lit(json!("{\"a\":1}"))]),
            node("encoded", "json.encode", vec![arg_var("parsed")]),
            emit("result", list_lit(vec![var("parsed"), var("encoded")])),
        ]);
        let arr = v.as_array().unwrap();
        assert_eq!(arr[0], json!({"a": 1}));
        assert_eq!(arr[1], json!("{\"a\":1}"));
    }

    // =========================================================
    // Sub-flow dispatch
    // =========================================================

    #[test]
    fn sub_flow_dispatch() {
        // Register a sub-flow "Double" that doubles its input
        let sub_flow = Flow {
            name: "Double".to_string(),
            inputs: vec![Port {
                name: "n".to_string(),
                type_name: "long".to_string(),
            }],
            outputs: vec![Port {
                name: "result".to_string(),
                type_name: "long".to_string(),
            }],
            body: vec![emit(
                "result",
                binop(BinOp::Mul, var("n"), lit(json!(2))),
            )],
        };
        let sub_ir = dummy_ir();
        let mut flow_registry = FlowRegistry::new();
        flow_registry.insert(
            "Double".to_string(),
            FlowProgram {
                flow: sub_flow,
                ir: sub_ir,
                emit_name: Some("result".to_string()),
                fail_name: None,
                registry: TypeRegistry::empty(),
                kind: DeclKind::Func,
            },
        );

        let main_flow = make_flow(
            "Main",
            vec![],
            vec![("result", "long")],
            vec![
                assign("doubled", call("Double", vec![lit(json!(21))])),
                emit("result", var("doubled")),
            ],
        );

        let result = execute_flow(
            &main_flow,
            dummy_ir(),
            HashMap::new(),
            &TypeRegistry::empty(),
            Some(&flow_registry),
            &CodecRegistry::default_registry(),
            &NullHost,
        )
        .unwrap();
        let outputs = result.outputs.as_object().unwrap();
        assert_eq!(outputs.get("result"), Some(&json!(42)));
    }

    // =========================================================
    // Value mocks
    // =========================================================

    #[test]
    fn value_mock() {
        let mut mocks = HashMap::new();
        mocks.insert("external.GetData".to_string(), json!({"key": "mocked"}));
        let flow_registry = FlowRegistry::new().with_value_mocks(mocks);

        let v = run_body_with_registry(
            vec![
                assign("data", call("external.GetData", vec![])),
                emit("result", var("data")),
            ],
            HashMap::new(),
            Some(&flow_registry),
        );
        assert_eq!(v, json!({"key": "mocked"}));
    }

    fn run_body_with_registry(
        body: Vec<Statement>,
        inputs: HashMap<String, Value>,
        flow_registry: Option<&FlowRegistry>,
    ) -> Value {
        let flow = make_flow("Test", vec![], vec![("result", "Any")], body);
        let result = execute_flow(
            &flow,
            dummy_ir(),
            inputs,
            &TypeRegistry::empty(),
            flow_registry,
            &CodecRegistry::default_registry(),
            &NullHost,
        )
        .unwrap();
        let outputs = result.outputs.as_object().unwrap();
        outputs.get("result").cloned().unwrap_or(Value::Null)
    }

    // =========================================================
    // Sync block
    // =========================================================

    #[test]
    fn sync_block_merges_results() {
        let v = run_body(vec![
            Statement::Sync(SyncBlock {
                targets: vec!["a".to_string(), "b".to_string()],
                options: SyncOptions::default(),
                body: vec![
                    assign("x", lit(json!(10))),
                    assign("y", lit(json!(20))),
                ],
                exports: vec!["x".to_string(), "y".to_string()],
            }),
            emit(
                "result",
                binop(BinOp::Add, var("a"), var("b")),
            ),
        ]);
        assert_eq!(v, json!(30));
    }

    // =========================================================
    // Variable resolution: dot paths
    // =========================================================

    #[test]
    fn dot_path_resolution() {
        let v = run_body_with_inputs(
            vec![emit("result", var("user.name"))],
            vec![("user", json!({"name": "Alice", "age": 30}))],
        );
        assert_eq!(v, json!("Alice"));
    }

    #[test]
    fn nested_dot_path() {
        let v = run_body_with_inputs(
            vec![emit("result", var("a.b.c"))],
            vec![("a", json!({"b": {"c": "deep"}}))],
        );
        assert_eq!(v, json!("deep"));
    }

    // =========================================================
    // Error cases
    // =========================================================

    #[test]
    fn division_by_zero() {
        let flow = make_flow(
            "Test",
            vec![],
            vec![("result", "Any")],
            vec![emit(
                "result",
                binop(BinOp::Div, lit(json!(1)), lit(json!(0))),
            )],
        );
        let result = execute_flow(
            &flow,
            dummy_ir(),
            HashMap::new(),
            &TypeRegistry::empty(),
            None,
            &CodecRegistry::default_registry(),
            &NullHost,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Division by zero"));
    }

    #[test]
    fn unknown_variable() {
        let flow = make_flow(
            "Test",
            vec![],
            vec![("result", "Any")],
            vec![emit("result", var("nonexistent"))],
        );
        let result = execute_flow(
            &flow,
            dummy_ir(),
            HashMap::new(),
            &TypeRegistry::empty(),
            None,
            &CodecRegistry::default_registry(),
            &NullHost,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("nonexistent"));
    }

    #[test]
    fn io_op_rejected_by_null_host() {
        let flow = make_flow(
            "Test",
            vec![],
            vec![("result", "Any")],
            vec![
                node("x", "term.print", vec![arg_lit(json!("hi"))]),
                emit("result", var("x")),
            ],
        );
        let result = execute_flow(
            &flow,
            dummy_ir(),
            HashMap::new(),
            &TypeRegistry::empty(),
            None,
            &CodecRegistry::default_registry(),
            &NullHost,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no I/O in tests"));
    }

    // =========================================================
    // Loop restores item variable
    // =========================================================

    #[test]
    fn loop_restores_item_var() {
        // If "n" existed before the loop, it should be restored after
        let v = run_body_with_inputs(
            vec![
                Statement::Loop(LoopBlock {
                    collection: list_lit(vec![lit(json!(10)), lit(json!(20))]),
                    item: "n".to_string(),
                    body: vec![],
                }),
                emit("result", var("n")),
            ],
            vec![("n", json!(999))],
        );
        assert_eq!(v, json!(999));
    }

    // =========================================================
    // Emit from inside case arm
    // =========================================================

    #[test]
    fn emit_from_case_arm() {
        let v = run_body(vec![
            assign("x", lit(json!(42))),
            Statement::Case(CaseBlock {
                expr: binop(BinOp::Gt, var("x"), lit(json!(0))),
                arms: vec![CaseArm {
                    pattern: Pattern::Lit(json!(true)),
                    body: vec![emit("result", var("x"))],
                }],
                else_body: vec![emit("result", lit(json!(0)))],
            }),
        ]);
        assert_eq!(v, json!(42));
    }

    // =========================================================
    // Emit from inside loop (early return)
    // =========================================================

    #[test]
    fn emit_from_loop_early_return() {
        let v = run_body(vec![Statement::Loop(LoopBlock {
            collection: list_lit(vec![lit(json!(1)), lit(json!(2)), lit(json!(3))]),
            item: "n".to_string(),
            body: vec![
                Statement::Case(CaseBlock {
                    expr: binop(BinOp::Eq, var("n"), lit(json!(2))),
                    arms: vec![CaseArm {
                        pattern: Pattern::Lit(json!(true)),
                        body: vec![emit("result", var("n"))],
                    }],
                    else_body: vec![],
                }),
            ],
        })]);
        assert_eq!(v, json!(2));
    }

    // =========================================================
    // Mixed int/float arithmetic preserves types correctly
    // =========================================================

    #[test]
    fn mixed_int_float_add() {
        let v = run_body(vec![emit(
            "result",
            binop(BinOp::Add, lit(json!(1)), lit(json!(0.5))),
        )]);
        assert_eq!(v, json!(1.5));
    }

    // =========================================================
    // Expr::Call through eval_expr (not Node dispatch)
    // =========================================================

    #[test]
    fn call_expr_pure_op() {
        let v = run_body(vec![
            assign("result_val", call("str.upper", vec![lit(json!("test"))])),
            emit("result", var("result_val")),
        ]);
        assert_eq!(v, json!("TEST"));
    }

    // =========================================================
    // Truthiness semantics
    // =========================================================

    #[test]
    fn truthiness() {
        // Non-empty string is truthy
        assert_eq!(
            run_body(vec![emit(
                "result",
                ternary(lit(json!("hi")), lit(json!("yes")), lit(json!("no"))),
            )]),
            json!("yes")
        );
        // Empty string is falsy
        assert_eq!(
            run_body(vec![emit(
                "result",
                ternary(lit(json!("")), lit(json!("yes")), lit(json!("no"))),
            )]),
            json!("no")
        );
        // null is falsy
        assert_eq!(
            run_body(vec![emit(
                "result",
                ternary(lit(json!(null)), lit(json!("yes")), lit(json!("no"))),
            )]),
            json!("no")
        );
        // 0 is falsy
        assert_eq!(
            run_body(vec![emit(
                "result",
                ternary(lit(json!(0)), lit(json!("yes")), lit(json!("no"))),
            )]),
            json!("no")
        );
        // Non-zero is truthy
        assert_eq!(
            run_body(vec![emit(
                "result",
                ternary(lit(json!(1)), lit(json!("yes")), lit(json!("no"))),
            )]),
            json!("yes")
        );
    }
}
