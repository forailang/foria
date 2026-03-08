use crate::ast::{DeclKind, Statement, TopDecl};
use crate::codec::CodecRegistry;
use crate::host_native::NativeHost;
use crate::ir;
use crate::loader::{self, FlowProgram, FlowRegistry};
use crate::parser;
use crate::runtime;
use crate::sema;
use crate::types::TypeRegistry;
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

fn format_unknown_op(prefix: &str, op: &str) -> String {
    let base = format!("{prefix} uses unknown op `{op}`");
    if let Some(hint) = sema::unknown_op_fix_hint(op) {
        format!("{base} — {hint}")
    } else {
        base
    }
}

fn collect_ops(statements: &[Statement], out: &mut Vec<String>) {
    for stmt in statements {
        match stmt {
            Statement::Node(n) => out.push(n.op.clone()),
            Statement::ExprAssign(_) => {}
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

#[derive(Debug, Clone, Default)]
pub struct TestFailure {
    pub name: String,
    pub error: String,
}

#[derive(Debug, Clone, Default)]
pub struct TestSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub failures: Vec<TestFailure>,
    pub warnings: Vec<String>,
}

struct ItBlock {
    description: String,
    body: String,
    #[allow(dead_code)]
    line_offset: usize,
}

struct ItResult {
    description: String,
    elapsed_ms: u128,
    result: Result<(), String>,
}

fn extract_it_blocks(body: &str) -> Result<(String, Vec<ItBlock>), String> {
    let it_re = Regex::new(r#"^it\s+"([^"]+)"\s*$"#).unwrap();
    let mut setup_lines = Vec::new();
    let mut blocks = Vec::new();
    let mut current_desc: Option<String> = None;
    let mut current_body: Vec<String> = Vec::new();
    let mut current_offset = 0usize;
    let mut found_first_it = false;

    for (idx, raw) in body.lines().enumerate() {
        let line = raw.trim();

        if let Some(caps) = it_re.captures(line) {
            if let Some(desc) = current_desc.take() {
                // Strip trailing blank lines, then remove closing "done"
                while current_body.last().is_some_and(|l| l.trim().is_empty()) {
                    current_body.pop();
                }
                if current_body.last().is_some_and(|l| l.trim() == "done") {
                    current_body.pop();
                }
                blocks.push(ItBlock {
                    description: desc,
                    body: current_body.join("\n"),
                    line_offset: current_offset,
                });
                current_body.clear();
            }
            current_desc = Some(caps.get(1).unwrap().as_str().to_string());
            current_offset = idx;
            found_first_it = true;
            continue;
        }

        if !found_first_it {
            // Before first `it` — this is shared setup
            setup_lines.push(raw.to_string());
        } else if current_desc.is_some() {
            // Inside an `it` block
            current_body.push(raw.to_string());
        } else {
            // Between `it` blocks — only allow comments/blanks
            if !line.is_empty() && !line.starts_with('#') {
                return Err(format!(
                    "line {}: content outside `it` block: `{line}`",
                    idx + 1
                ));
            }
        }
    }

    // Close last it block
    if let Some(desc) = current_desc.take() {
        while current_body.last().is_some_and(|l| l.trim().is_empty()) {
            current_body.pop();
        }
        if current_body.last().is_some_and(|l| l.trim() == "done") {
            current_body.pop();
        }
        blocks.push(ItBlock {
            description: desc,
            body: current_body.join("\n"),
            line_offset: current_offset,
        });
    }

    if blocks.is_empty() {
        return Err("test block has no `it` sub-cases".to_string());
    }

    Ok((setup_lines.join("\n"), blocks))
}

enum FlowCallResult {
    Success(Value),
    Failure(Value),
}

fn split_csv(raw: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut cur = String::new();
    let mut in_string = false;
    let mut quote_char = '\0';
    let mut escape = false;
    let mut brace_depth: i32 = 0;
    let mut bracket_depth: i32 = 0;

    for ch in raw.chars() {
        if in_string {
            cur.push(ch);
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == quote_char {
                in_string = false;
            }
            continue;
        }

        if ch == '"' || ch == '\'' {
            in_string = true;
            quote_char = ch;
            cur.push(ch);
        } else if ch == '{' {
            brace_depth += 1;
            cur.push(ch);
        } else if ch == '}' {
            brace_depth -= 1;
            cur.push(ch);
        } else if ch == '[' {
            bracket_depth += 1;
            cur.push(ch);
        } else if ch == ']' {
            bracket_depth -= 1;
            cur.push(ch);
        } else if ch == ',' && brace_depth == 0 && bracket_depth == 0 {
            let token = cur.trim();
            if !token.is_empty() {
                items.push(token.to_string());
            }
            cur.clear();
        } else {
            cur.push(ch);
        }
    }

    let tail = cur.trim();
    if !tail.is_empty() {
        items.push(tail.to_string());
    }

    items
}

fn unescape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('"') => out.push('"'),
                Some('\'') => out.push('\''),
                Some('\\') => out.push('\\'),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn resolve_path(env: &HashMap<String, Value>, path: &str) -> Option<Value> {
    let mut parts = path.split('.');
    let first = parts.next()?;
    let mut current = env.get(first)?.clone();
    for part in parts {
        let obj = current.as_object()?;
        current = obj.get(part)?.clone();
    }
    Some(current)
}

/// Convert forai-style dict literals (`{key: val}`) to valid JSON (`{"key": val}`).
/// Bare identifier keys after `{` or `,` that are followed by `:` get quoted.
/// Already-quoted keys and strings are left untouched.
fn quote_bare_keys(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut expect_key = false;

    while i < len {
        let c = chars[i];
        // Skip over string literals
        if c == '"' {
            out.push(c);
            i += 1;
            while i < len {
                let sc = chars[i];
                out.push(sc);
                i += 1;
                if sc == '\\' && i < len {
                    out.push(chars[i]);
                    i += 1;
                } else if sc == '"' {
                    break;
                }
            }
            continue;
        }
        if c == '{' || c == ',' {
            out.push(c);
            i += 1;
            expect_key = true;
            continue;
        }
        if expect_key && c.is_ascii_whitespace() {
            out.push(c);
            i += 1;
            continue;
        }
        if expect_key && (c.is_ascii_alphabetic() || c == '_') {
            // Collect the identifier
            let start = i;
            while i < len && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let ident = &s[start..i];
            // Skip whitespace before potential colon
            let mut j = i;
            while j < len && chars[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < len && chars[j] == ':' {
                // It's a bare key — quote it
                out.push('"');
                out.push_str(ident);
                out.push('"');
            } else {
                // Not a key, emit as-is
                out.push_str(ident);
            }
            expect_key = false;
            continue;
        }
        expect_key = false;
        out.push(c);
        i += 1;
    }
    out
}

fn eval_value_expr<'a>(
    expr: &'a str,
    env: &'a HashMap<String, Value>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, String>> + 'a>> {
    Box::pin(async move {
    let token = expr.trim();

    // Array/object literals — support both JSON-quoted and forai bare-key styles
    if (token.starts_with('[') && token.ends_with(']'))
        || (token.starts_with('{') && token.ends_with('}'))
    {
        let json_str = quote_bare_keys(token);
        return serde_json::from_str(&json_str)
            .map_err(|e| format!("Invalid literal: {e}"));
    }

    if token.starts_with('"') && token.ends_with('"') && token.len() >= 2 {
        return Ok(Value::String(unescape_string(&token[1..token.len() - 1])));
    }
    if token.starts_with('\'') && token.ends_with('\'') && token.len() >= 2 {
        return Ok(Value::String(unescape_string(&token[1..token.len() - 1])));
    }
    if token == "true" {
        return Ok(Value::Bool(true));
    }
    if token == "false" {
        return Ok(Value::Bool(false));
    }
    if let Ok(v) = token.parse::<i64>() {
        return Ok(Value::from(v));
    }
    if let Ok(v) = token.parse::<f64>() {
        return Ok(Value::from(v));
    }

    if let Some((fn_name, rest)) = token.split_once('(')
        && token.ends_with(')')
        && !fn_name.trim().is_empty()
    {
        let args_raw = &rest[..rest.len() - 1];
        let arg_tokens = split_csv(args_raw);
        let mut args = Vec::with_capacity(arg_tokens.len());
        for a in &arg_tokens {
            args.push(eval_value_expr(a, env).await?);
        }

        if fn_name.trim() == "dict" {
            return Ok(Value::Object(serde_json::Map::new()));
        }
        if fn_name.trim() == "obj" {
            if args.len() % 2 != 0 {
                return Err("obj() expects alternating key-value pairs".to_string());
            }
            let mut obj = serde_json::Map::new();
            for pair in args.chunks(2) {
                let Some(key) = pair[0].as_str() else {
                    return Err(format!("obj() key must be a string, got {}", pair[0]));
                };
                obj.insert(key.to_string(), pair[1].clone());
            }
            return Ok(Value::Object(obj));
        }
        if fn_name.trim() == "request" {
            if args.len() != 2 {
                return Err("request(email, password) expects 2 args".to_string());
            }
            let Some(email) = args[0].as_str() else {
                return Err("request(email, password): email must be string".to_string());
            };
            let Some(password) = args[1].as_str() else {
                return Err("request(email, password): password must be string".to_string());
            };
            return Ok(serde_json::json!({
                "path": "/login",
                "params": {
                    "email": email,
                    "password": password
                }
            }));
        }
        // Dispatch dotted names to runtime ops (e.g. list.len, obj.get)
        if fn_name.trim().contains('.') {
            let h = NativeHost::new();
            let codecs = CodecRegistry::default_registry();
            return runtime::execute_op(fn_name.trim(), &args, &h, &codecs)
                .await
                .map_err(|e| format!("op `{}` failed: {e}", fn_name.trim()));
        }
        if args.len() == 1 {
            return Ok(args[0].clone());
        }
        return Ok(Value::Array(args));
    }

    resolve_path(env, token).ok_or_else(|| format!("Unknown value `{token}`"))
    }) // end Box::pin(async move)
}

fn value_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(v) => *v,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn compare_values(lhs: &Value, op: &str, rhs: &Value) -> Result<bool, String> {
    match op {
        "==" | "!=" => {
            // Normalize numeric comparisons to f64 to avoid serde_json Number representation mismatches
            if let (Some(l), Some(r)) = (lhs.as_f64(), rhs.as_f64()) {
                return Ok(if op == "==" { l == r } else { l != r });
            }
            Ok(if op == "==" { lhs == rhs } else { lhs != rhs })
        }
        ">" | ">=" | "<" | "<=" => {
            let Some(l) = lhs.as_f64() else {
                return Err(format!("Left side is not numeric: {lhs}"));
            };
            let Some(r) = rhs.as_f64() else {
                return Err(format!("Right side is not numeric: {rhs}"));
            };
            Ok(match op {
                ">" => l > r,
                ">=" => l >= r,
                "<" => l < r,
                "<=" => l <= r,
                _ => unreachable!(),
            })
        }
        _ => Err(format!("Unsupported comparison operator `{op}`")),
    }
}

async fn eval_must(expr: &str, env: &HashMap<String, Value>) -> Result<bool, String> {
    for op in ["==", "!=", ">=", "<=", ">", "<"] {
        if let Some((left, right)) = expr.split_once(op) {
            let lhs = eval_value_expr(left, env).await?;
            let rhs = eval_value_expr(right, env).await?;
            return compare_values(&lhs, op, &rhs);
        }
    }

    let value = eval_value_expr(expr, env).await?;
    Ok(value_truthy(&value))
}

async fn invoke_flow(
    flow_name: &str,
    arg_exprs: &[String],
    env: &HashMap<String, Value>,
    flows: &HashMap<String, FlowProgram>,
    flow_registry: Option<&FlowRegistry>,
    quiet: bool,
) -> Result<FlowCallResult, String> {
    let Some(program) = flows.get(flow_name) else {
        return Err(format!("Unknown flow `{flow_name}`"));
    };

    if arg_exprs.len() != program.flow.inputs.len() {
        return Err(format!(
            "Flow `{}` expects {} args but got {}",
            flow_name,
            program.flow.inputs.len(),
            arg_exprs.len()
        ));
    }

    let mut input_map = HashMap::new();
    for (idx, input) in program.flow.inputs.iter().enumerate() {
        let value = eval_value_expr(&arg_exprs[idx], env).await?;
        input_map.insert(input.name.clone(), value);
    }

    let codecs = CodecRegistry::default_registry();
    let host = if quiet {
        Some(Rc::new(NativeHost::new_quiet()) as Rc<dyn crate::host::Host>)
    } else {
        None
    };
    let report = runtime::execute_flow(
        &program.flow,
        program.ir.clone(),
        input_map,
        &program.registry,
        flow_registry,
        &codecs,
        host,
    )
    .await?;
    let Some(outputs) = report.outputs.as_object() else {
        return Err(format!("Flow `{flow_name}` produced invalid outputs shape"));
    };

    let success = program.emit_name.as_deref().and_then(|n| outputs.get(n)).cloned();
    let failure = program.fail_name.as_deref().and_then(|n| outputs.get(n)).cloned();

    if program.emit_name.is_none() {
        return Ok(FlowCallResult::Success(serde_json::Value::Null));
    }

    match (success, failure) {
        (Some(v), None) => Ok(FlowCallResult::Success(v)),
        (None, Some(v)) => Ok(FlowCallResult::Failure(v)),
        (None, None) => Err(format!("Flow `{flow_name}` produced no outputs")),
        (Some(_), Some(_)) => Err(format!(
            "Flow `{flow_name}` produced both emit and fail outputs"
        )),
    }
}

async fn execute_lines(
    body: &str,
    env: &mut HashMap<String, Value>,
    mocks: &HashMap<String, serde_json::Value>,
    flows: &HashMap<String, FlowProgram>,
    flow_registry: Option<&FlowRegistry>,
    quiet: bool,
) -> Result<(), String> {
    let mock_re = Regex::new(r"^mock\s+([A-Za-z_][A-Za-z0-9_.]*)\s*=>\s*(.+)$").unwrap();
    let trap_re =
        Regex::new(r"^([A-Za-z_][A-Za-z0-9_]*)\s*=\s*trap\s+([A-Za-z_][A-Za-z0-9_.]*)\((.*)\)$")
            .unwrap();
    let call_re =
        Regex::new(r"^([A-Za-z_][A-Za-z0-9_]*)\s*=\s*([A-Za-z_][A-Za-z0-9_.]*)\((.*)\)$").unwrap();
    let assign_re = Regex::new(r"^([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.+)$").unwrap();

    // Build effective registry with mocks overlaid
    let mocked_registry = if !mocks.is_empty() {
        flow_registry.map(|fr| fr.with_value_mocks(mocks.clone()))
    } else {
        None
    };
    let effective_registry: Option<&FlowRegistry> = match &mocked_registry {
        Some(mr) => Some(mr),
        None => flow_registry,
    };

    for (idx, raw) in body.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Skip mock lines (collected separately)
        if mock_re.is_match(line) {
            continue;
        }

        if let Some(expr) = line.strip_prefix("must ") {
            let ok = eval_must(expr, env)
                .await
                .map_err(|e| format!("line {line_no}: must evaluation error: {e}"))?;
            if !ok {
                return Err(format!("line {line_no}: must failed: {expr}"));
            }
            continue;
        }

        if let Some(caps) = trap_re.captures(line) {
            let var = caps.get(1).unwrap().as_str().to_string();
            let flow_name = caps.get(2).unwrap().as_str();
            let args_raw = caps.get(3).unwrap().as_str();
            let arg_exprs = split_csv(args_raw);
            match invoke_flow(flow_name, &arg_exprs, env, flows, effective_registry, quiet)
                .await
                .map_err(|e| format!("line {line_no}: {e}"))?
            {
                FlowCallResult::Failure(err) => {
                    env.insert(var, err);
                }
                FlowCallResult::Success(_) => {
                    return Err(format!(
                        "line {line_no}: trap expected failure but flow `{flow_name}` succeeded"
                    ));
                }
            }
            continue;
        }

        if let Some(caps) = call_re.captures(line) {
            let var = caps.get(1).unwrap().as_str().to_string();
            let callee = caps.get(2).unwrap().as_str();
            let args_raw = caps.get(3).unwrap().as_str();
            let arg_exprs = split_csv(args_raw);

            if flows.contains_key(callee) {
                match invoke_flow(callee, &arg_exprs, env, flows, effective_registry, quiet)
                    .await
                    .map_err(|e| format!("line {line_no}: {e}"))?
                {
                    FlowCallResult::Success(v) => {
                        env.insert(var, v);
                    }
                    FlowCallResult::Failure(err) => {
                        return Err(format!(
                            "line {line_no}: flow `{callee}` failed unexpectedly: {err}"
                        ));
                    }
                }
                continue;
            }

            // Fallback: try as a runtime op (e.g. route.match, obj.get, etc.)
            if callee.contains('.') {
                let mut args = Vec::with_capacity(arg_exprs.len());
                for a in &arg_exprs {
                    args.push(
                        eval_value_expr(a, env)
                            .await
                            .map_err(|e| format!("line {line_no}: op arg error: {e}"))?,
                    );
                }
                let h = NativeHost::new();
                let codecs = CodecRegistry::default_registry();
                let result = runtime::execute_op(callee, &args, &h, &codecs)
                    .await
                    .map_err(|e| format!("line {line_no}: op `{callee}` failed: {e}"))?;
                env.insert(var, result);
                continue;
            }
        }

        if let Some(caps) = assign_re.captures(line) {
            let var = caps.get(1).unwrap().as_str().to_string();
            let expr = caps.get(2).unwrap().as_str();
            let value = eval_value_expr(expr, env)
                .await
                .map_err(|e| format!("line {line_no}: assignment error: {e}"))?;
            env.insert(var, value);
            continue;
        }

        // Bare op call (no assignment): e.g. log.info("message")
        if let Some((op_name, rest)) = line.split_once('(') {
            let op_name = op_name.trim();
            if rest.ends_with(')') {
                let args_raw = &rest[..rest.len() - 1];

                // Bare flow/func call (void): e.g. Print("hello")
                if !op_name.contains('.') && flows.contains_key(op_name) {
                    let arg_exprs = split_csv(args_raw);
                    match invoke_flow(op_name, &arg_exprs, env, flows, effective_registry, quiet)
                        .await
                        .map_err(|e| format!("line {line_no}: {e}"))?
                    {
                        FlowCallResult::Success(_) => {}
                        FlowCallResult::Failure(err) => {
                            return Err(format!(
                                "line {line_no}: flow `{op_name}` failed unexpectedly: {err}"
                            ));
                        }
                    }
                    continue;
                }

                // Bare built-in op call: e.g. log.info("message")
                if op_name.contains('.') {
                    let args: Vec<Value> = if args_raw.trim().is_empty() {
                        vec![]
                    } else {
                        let mut v = Vec::new();
                        for a in split_csv(args_raw) {
                            v.push(
                                eval_value_expr(&a, env)
                                    .await
                                    .map_err(|e| format!("line {line_no}: op arg error: {e}"))?,
                            );
                        }
                        v
                    };
                    let h = NativeHost::new();
                    let codecs = CodecRegistry::default_registry();
                    runtime::execute_op(op_name, &args, &h, &codecs)
                        .await
                        .map_err(|e| format!("line {line_no}: op `{op_name}` failed: {e}"))?;
                    continue;
                }
            }
        }

        return Err(format!("line {line_no}: unsupported test syntax `{line}`"));
    }

    Ok(())
}

async fn collect_mocks_from_lines_async(
    body: &str,
) -> Result<HashMap<String, serde_json::Value>, String> {
    let mock_re = Regex::new(r"^mock\s+([A-Za-z_][A-Za-z0-9_.]*)\s*=>\s*(.+)$").unwrap();
    let empty_env = HashMap::new();
    let mut mocks = HashMap::new();

    for (idx, raw) in body.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(caps) = mock_re.captures(line) {
            let name = caps.get(1).unwrap().as_str().to_string();
            let expr = caps.get(2).unwrap().as_str();
            let value = eval_value_expr(expr, &empty_env)
                .await
                .map_err(|e| format!("line {line_no}: mock value error: {e}"))?;
            mocks.insert(name, value);
        }
    }
    Ok(mocks)
}

async fn run_test_body(
    body: &str,
    flows: &HashMap<String, FlowProgram>,
    flow_registry: Option<&FlowRegistry>,
    quiet: bool,
) -> Result<Vec<ItResult>, String> {
    let (setup, it_blocks) = extract_it_blocks(body)?;

    // Execute shared setup to collect env and mocks
    let shared_mocks = collect_mocks_from_lines_async(&setup).await?;
    let mut shared_env = HashMap::<String, Value>::new();
    execute_lines(&setup, &mut shared_env, &shared_mocks, flows, flow_registry, quiet).await?;

    let mut results = Vec::with_capacity(it_blocks.len());

    for it in &it_blocks {
        let start = Instant::now();

        // Clone shared state for this it block
        let mut env = shared_env.clone();
        let mut mocks = shared_mocks.clone();

        // Collect per-it mocks and merge (override shared)
        let it_mocks = collect_mocks_from_lines_async(&it.body).await?;
        for (k, v) in it_mocks {
            mocks.insert(k, v);
        }

        let result =
            execute_lines(&it.body, &mut env, &mocks, flows, flow_registry, quiet).await;

        results.push(ItResult {
            description: it.description.clone(),
            elapsed_ms: start.elapsed().as_millis(),
            result,
        });
    }

    Ok(results)
}

fn collect_fa_files(path: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if path.is_file() {
        if path.extension().and_then(|s| s.to_str()) == Some("fa") {
            out.push(path.to_path_buf());
        }
        return Ok(());
    }

    collect_fa_files_recursive(path, out)
}

fn collect_fa_files_recursive(path: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(path)
        .map_err(|e| format!("Failed to read test path {}: {e}", path.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("read_dir entry error: {e}"))?;
        let child = entry.path();
        if child.is_dir() {
            collect_fa_files_recursive(&child, out)?;
        } else if child.extension().and_then(|s| s.to_str()) == Some("fa") {
            out.push(child);
        }
    }
    Ok(())
}

async fn run_tests_impl(path: &Path, quiet: bool) -> Result<TestSummary, String> {
    let mut files = Vec::new();
    collect_fa_files(path, &mut files)?;
    files.sort();

    if files.is_empty() {
        return Err(format!("No .fa files found at {}", path.display()));
    }

    // Resolve project dependencies once for all test files
    let resolved_deps = {
        let dir = if path.is_file() {
            path.parent().unwrap_or(Path::new("."))
        } else {
            path
        };
        if let Ok((cfg, root)) = crate::config::find_config(dir) {
            if !cfg.dependencies.is_empty() {
                crate::deps::resolve_dependencies(&cfg, &root).unwrap_or_else(|_| crate::deps::ResolvedDeps::empty())
            } else {
                crate::deps::ResolvedDeps::empty()
            }
        } else {
            crate::deps::ResolvedDeps::empty()
        }
    };

    let mut summary = TestSummary::default();

    for file in files {
        let src = fs::read_to_string(&file)
            .map_err(|e| format!("Failed to read {}: {e}", file.display()))?;

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
        for warning in sema::test_call_warnings(&module) {
            let rendered = format!("{}:{warning}", file.display());
            if !quiet {
                println!("WARN  {rendered}");
            }
            summary.warnings.push(rendered);
        }

        let registry = TypeRegistry::from_module(&module)
            .map_err(|errors| format!("{}: type errors:\n{}", file.display(), errors.join("\n")))?;

        let flow_registry = loader::build_flow_registry(&file, &module, &resolved_deps)?;

        let mut docs_map = HashMap::<String, String>::new();
        let mut flows = HashMap::<String, FlowProgram>::new();
        let mut tests = Vec::new();

        for decl in &module.decls {
            match decl {
                TopDecl::Docs(d) => {
                    docs_map.insert(d.name.clone(), d.markdown.clone());
                }
                TopDecl::Func(f) | TopDecl::Sink(f) | TopDecl::Source(f) => {
                    let kind = if matches!(decl, TopDecl::Sink(_)) {
                        DeclKind::Sink
                    } else if matches!(decl, TopDecl::Source(_)) {
                        DeclKind::Source
                    } else {
                        DeclKind::Func
                    };
                    let flow = parser::parse_runtime_func_decl_v1(f).map_err(|e| {
                        format!("{}: func `{}` parse error: {e}", file.display(), f.name)
                    })?;

                    let known: HashSet<&str> = runtime::known_ops().iter().copied().collect();
                    let codec_ops: HashSet<String> = CodecRegistry::default_registry()
                        .known_ops()
                        .into_iter()
                        .collect();
                    let mut ops = Vec::new();
                    collect_ops(&flow.body, &mut ops);
                    let unknown: Vec<_> = ops
                        .iter()
                        .filter(|op| {
                            !known.contains(op.as_str())
                                && !codec_ops.contains(op.as_str())
                                && !flow_registry.is_flow(op)
                                && !op.starts_with("ffi.")
                        })
                        .collect();
                    if !unknown.is_empty() {
                        return Err(unknown
                            .iter()
                            .map(|op| {
                                format_unknown_op(
                                    &format!("{}: func `{}`", file.display(), f.name),
                                    op,
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n"));
                    }

                    let ir_val = ir::lower_to_ir(&flow).map_err(|e| {
                        format!("{}: func `{}` lower error: {e}", file.display(), f.name)
                    })?;
                    let emit_name = if f.return_type.is_some() {
                        Some("_return".to_string())
                    } else {
                        f.emits.first().map(|e| e.name.clone())
                    };
                    let fail_name = if f.fail_type.is_some() {
                        Some("_fail".to_string())
                    } else {
                        f.fails.first().map(|fa| fa.name.clone())
                    };
                    flows.insert(
                        f.name.clone(),
                        FlowProgram {
                            flow,
                            ir: ir_val,
                            emit_name,
                            fail_name,
                            registry: registry.clone(),
                            kind,
                        },
                    );
                }
                TopDecl::Flow(f) => {
                    let flow_graph = parser::parse_flow_graph_decl_v1(f).map_err(|e| {
                        format!("{}: flow `{}` parse error: {e}", file.display(), f.name)
                    })?;
                    let flow = parser::lower_flow_graph_to_flow(&flow_graph).map_err(|e| {
                        format!("{}: flow `{}` lower error: {e}", file.display(), f.name)
                    })?;

                    let known: HashSet<&str> = runtime::known_ops().iter().copied().collect();
                    let codec_ops: HashSet<String> = CodecRegistry::default_registry()
                        .known_ops()
                        .into_iter()
                        .collect();
                    let mut ops = Vec::new();
                    collect_ops(&flow.body, &mut ops);
                    let unknown: Vec<_> = ops
                        .iter()
                        .filter(|op| {
                            !known.contains(op.as_str())
                                && !codec_ops.contains(op.as_str())
                                && !flow_registry.is_flow(op)
                                && !op.starts_with("ffi.")
                        })
                        .collect();
                    if !unknown.is_empty() {
                        return Err(unknown
                            .iter()
                            .map(|op| {
                                format_unknown_op(
                                    &format!("{}: flow `{}`", file.display(), f.name),
                                    op,
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n"));
                    }

                    let ir_val = ir::lower_to_ir(&flow).map_err(|e| {
                        format!("{}: flow `{}` lower error: {e}", file.display(), f.name)
                    })?;
                    let emit_name = flow.outputs.first().map(|p| p.name.clone());
                    let fail_name = flow.outputs.get(1).map(|p| p.name.clone());
                    flows.insert(
                        f.name.clone(),
                        FlowProgram {
                            flow,
                            ir: ir_val,
                            emit_name,
                            fail_name,
                            registry: registry.clone(),
                            kind: DeclKind::Flow,
                        },
                    );
                }
                TopDecl::Test(t) => tests.push(t.clone()),
                TopDecl::Uses(_) | TopDecl::Type(_) | TopDecl::Enum(_) | TopDecl::Extern(_) => {}
            }
        }

        let file_label = file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        for test in tests {
            match run_test_body(&test.body_text, &flows, Some(&flow_registry), quiet).await {
                Ok(it_results) => {
                    for it in &it_results {
                        summary.total += 1;
                        match &it.result {
                            Ok(()) => {
                                summary.passed += 1;
                                if !quiet {
                                    println!(
                                        "PASS  {}.{} > {}   ({}ms)",
                                        file_label, test.name, it.description, it.elapsed_ms
                                    );
                                }
                            }
                            Err(err) => {
                                summary.failed += 1;
                                if !quiet {
                                    let doc_hint = docs_map
                                        .get(&test.name)
                                        .and_then(|s| s.lines().next())
                                        .unwrap_or("(no docs)");
                                    println!(
                                        "FAIL  {}.{} > {}",
                                        file_label, test.name, it.description
                                    );
                                    println!("      > {}", err);
                                    println!("      > {}", doc_hint);
                                }
                                summary.failures.push(TestFailure {
                                    name: format!(
                                        "{}.{} > {}",
                                        file_label, test.name, it.description
                                    ),
                                    error: err.clone(),
                                });
                            }
                        }
                    }
                }
                Err(err) => {
                    // Structural error (e.g. no `it` blocks found)
                    summary.total += 1;
                    summary.failed += 1;
                    if !quiet {
                        println!("FAIL  {}.{}", file_label, test.name);
                        println!("      > {}", err);
                    }
                    summary.failures.push(TestFailure {
                        name: format!("{}.{}", file_label, test.name),
                        error: err,
                    });
                }
            }
        }
    }

    Ok(summary)
}

pub async fn run_tests_at_path_async(path: &Path) -> Result<TestSummary, String> {
    run_tests_impl(path, false).await
}

pub async fn run_tests_at_path_build(path: &Path) -> Result<TestSummary, String> {
    run_tests_impl(path, true).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn runs_classify_example_tests() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/read-docs/src/app/router/Classify.fa");
        let summary = run_tests_at_path_async(&path)
            .await
            .expect("test run should succeed");
        assert!(summary.total >= 1);
        assert_eq!(summary.failed, 0);
    }

    // Verifies that running tests from a project root finds tests in subdirectories,
    // even when the main entry point is nested (e.g. config.main = "src/main.fa").
    // This tests the same code path as `forai build` and `forai test` (no args).
    #[tokio::test]
    async fn runs_pipeline_example_tests_from_project_root() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/pipeline");
        let summary = run_tests_at_path_async(&path)
            .await
            .expect("test run should succeed");
        assert!(summary.total >= 1, "expected tests to be found under pipeline/");
        assert_eq!(summary.failed, 0, "pipeline tests should all pass: {summary:?}");
    }

    #[tokio::test]
    async fn finds_tests_in_nested_directories() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("forai_test_nested_{stamp}"));
        let nested = root.join("sub");
        fs::create_dir_all(&nested).expect("create nested dir");

        let top = r#"
docs Top
  Top-level test fixture.
done

func Top
  emit result as bool
  fail error as text
body
  ok = true
  emit ok
done

test Top
  it "works"
    result = Top()
    must result == true
  done
done
"#;
        let child = r#"
docs Child
  Nested test fixture.
done

func Child
  emit result as bool
  fail error as text
body
  ok = true
  emit ok
done

test Child
  it "works"
    result = Child()
    must result == true
  done
done
"#;

        fs::write(root.join("Top.fa"), top).expect("write top module");
        fs::write(nested.join("Child.fa"), child).expect("write nested module");

        let summary = run_tests_at_path_async(&root)
            .await
            .expect("recursive test run should succeed");
        assert_eq!(summary.total, 2, "summary={summary:?}");
        assert_eq!(summary.failed, 0, "summary={summary:?}");

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn finds_tests_in_deep_nested_directories_and_ignores_non_fa_files() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("forai_test_deep_nested_{stamp}"));
        let deep = root.join("sub").join("deeper");
        fs::create_dir_all(&deep).expect("create deep nested dir");

        let top = r#"
docs TopLevel
  Top-level module.
done

func TopLevel
  emit result as bool
  fail error as text
body
  ok = true
  emit ok
done

test TopLevel
  it "works"
    result = TopLevel()
    must result == true
  done
done
"#;
        let deep_file = r#"
docs Deep
  Deep module.
done

func Deep
  emit result as bool
  fail error as text
body
  ok = true
  emit ok
done

test Deep
  it "works"
    result = Deep()
    must result == true
  done
done
"#;

        fs::write(root.join("TopLevel.fa"), top).expect("write top-level module");
        fs::write(deep.join("Deep.fa"), deep_file).expect("write deep nested module");
        fs::write(root.join("README.txt"), "not a forai source file").expect("write non-fa file");

        let summary = run_tests_at_path_async(&root)
            .await
            .expect("deep recursive test run should succeed");
        assert_eq!(summary.total, 2, "summary={summary:?}");
        assert_eq!(summary.failed, 0, "summary={summary:?}");

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn eval_json_array_literal() {
        let env = HashMap::new();
        let val = eval_value_expr(r#"[{"role": "user", "content": "Hi"}]"#, &env)
            .await
            .unwrap();
        assert!(val.is_array());
        assert_eq!(val.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn eval_json_object_literal() {
        let env = HashMap::new();
        let val = eval_value_expr(r#"{"key": "value"}"#, &env)
            .await
            .unwrap();
        assert!(val.is_object());
    }

    #[tokio::test]
    async fn eval_bare_key_object_literal() {
        let env = HashMap::new();
        let val = eval_value_expr(r#"{stop_reason: "end", count: 42}"#, &env)
            .await
            .unwrap();
        let obj = val.as_object().unwrap();
        assert_eq!(obj.get("stop_reason").unwrap(), "end");
        assert_eq!(obj.get("count").unwrap(), 42);
    }

    #[tokio::test]
    async fn eval_nested_bare_key_object() {
        let env = HashMap::new();
        let val = eval_value_expr(r#"{outer: {inner: "val"}}"#, &env)
            .await
            .unwrap();
        let outer = val.as_object().unwrap();
        let inner = outer.get("outer").unwrap().as_object().unwrap();
        assert_eq!(inner.get("inner").unwrap(), "val");
    }

    #[test]
    fn split_csv_respects_brace_depth() {
        let args = r#"{key: "val", other: "val2"}, "hello""#;
        let parts = split_csv(args);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], r#"{key: "val", other: "val2"}"#);
        assert_eq!(parts[1], r#""hello""#);
    }

    #[test]
    fn split_csv_respects_bracket_depth() {
        let args = r#"[1, 2, 3], "x""#;
        let parts = split_csv(args);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], "[1, 2, 3]");
        assert_eq!(parts[1], r#""x""#);
    }

    #[test]
    fn unescape_string_handles_quotes() {
        assert_eq!(unescape_string(r#"hello \"world\""#), r#"hello "world""#);
        assert_eq!(unescape_string(r#"a\\b"#), r#"a\b"#);
        assert_eq!(unescape_string(r#"line1\nline2"#), "line1\nline2");
    }

    #[test]
    fn compare_values_numeric_eq() {
        let a = serde_json::json!(2i64);
        let b = Value::from(2i64);
        assert!(compare_values(&a, "==", &b).unwrap());
    }

    #[tokio::test]
    async fn eval_must_inline_op() {
        let mut env = HashMap::new();
        env.insert("r".into(), serde_json::json!([1, 2, 3]));
        assert!(eval_must("list.len(r) == 3", &env).await.unwrap());
    }
}
