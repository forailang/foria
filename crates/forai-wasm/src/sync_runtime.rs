use forai_core::ast::{Arg, BinOp, Expr, Flow, InterpExpr, Pattern, Statement, UnaryOp};
use forai_core::codec::CodecRegistry;
use forai_core::host;
use forai_core::ir::Ir;
use forai_core::loader::FlowRegistry;
use forai_core::pure_ops;
use forai_core::sync_host::SyncHost;
use forai_core::types::TypeRegistry;
use serde_json::{Value, json};
use std::collections::HashMap;

enum ExecSignal {
    Continue,
    Emit {
        output: String,
        value_var: String,
        value: Value,
    },
    Break,
}

pub struct RunResult {
    pub outputs: Value,
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
        if let Some(i) = serde_json::Number::from_f64(result) {
            if i.is_i64() {
                return Ok(json!(i.as_i64().unwrap()));
            }
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
