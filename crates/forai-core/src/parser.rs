use crate::ast::{
    Arg, BareLoopBlock, BinOp, CaseArm, CaseBlock, ConstraintValue, ContinuationWire, DocsDecl,
    Emit, EnumDecl, Expr, ExprAssign, ExternBlock, ExternFnDecl, FieldDecl, FieldDocsEntry, Flow,
    FlowBranchBlock, FlowChooseBlock, FlowDecl, FlowEmitStmt, FlowGraph, FlowLocalDecl,
    FlowLogStmt, FlowOnBlock, FlowSendNowait, FlowStateDecl, FlowStatement, FuncDecl, InterpExpr,
    LoopBlock, ModuleAst, NextWire, NodeAssign, OnBlock, Pattern, Port, PortDecl, PortMapping,
    SendNowait, Span, Statement, StepBlock, StepThenItem, SyncBlock, SyncOptions, TakeDecl,
    TestDecl, TopDecl, TypeConstraint, TypeDecl, TypeKind, UnaryOp, UsesDecl,
};
use crate::lexer::{InterpPart, Token, TokenKind, lex};
use serde_json::json;

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{} {}", self.span.line, self.span.col, self.message)
    }
}

impl std::error::Error for ParseError {}

fn parse_interp_parts(parts: &[InterpPart]) -> Result<Expr, ParseError> {
    let mut segments = Vec::new();
    for part in parts {
        match part {
            InterpPart::Lit(s) => segments.push(InterpExpr::Lit(s.clone())),
            InterpPart::Expr(raw) => {
                let mut p = RuntimeBodyParser::new(raw, "_", "_").map_err(|e| ParseError {
                    message: e.message,
                    span: e.span,
                })?;
                let expr = p.parse_pratt_expr(0)?;
                segments.push(InterpExpr::Expr(Box::new(expr)));
            }
        }
    }
    Ok(Expr::Interp(segments))
}

pub fn parse_module_v1(source: &str) -> Result<ModuleAst, ParseError> {
    let tokens = lex(source).map_err(|e| ParseError {
        message: e.message,
        span: e.span,
    })?;
    let mut parser = TokenParser::new(source, tokens);
    parser.parse_module()
}

/// Scan the raw markdown captured from a docs block for nested
/// `docs field_name ... done` sub-blocks. Returns the cleaned
/// top-level markdown (sub-blocks stripped) plus a vec of field doc entries.
fn extract_field_docs(raw: &str) -> (String, Vec<FieldDocsEntry>) {
    let mut cleaned = String::new();
    let mut field_docs = Vec::new();
    let mut lines = raw.lines().peekable();

    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        // Detect "docs <field_name>" sub-block (must be indented)
        if trimmed.starts_with("docs ") && line.starts_with(|c: char| c == ' ' || c == '\t') {
            let field_name = trimmed["docs ".len()..].trim().to_string();
            let mut md = String::new();
            // Collect lines until indented "done"
            for inner in lines.by_ref() {
                let inner_trimmed = inner.trim();
                if inner_trimmed == "done" && inner.starts_with(|c: char| c == ' ' || c == '\t') {
                    break;
                }
                if !md.is_empty() {
                    md.push('\n');
                }
                md.push_str(inner_trimmed);
            }
            field_docs.push(FieldDocsEntry {
                name: field_name,
                markdown: md,
            });
        } else {
            if !cleaned.is_empty() {
                cleaned.push('\n');
            }
            cleaned.push_str(line);
        }
    }

    // Trim trailing whitespace from the cleaned markdown
    let cleaned = cleaned.trim_end().to_string();

    (cleaned, field_docs)
}

// --- Second-pass: func body → runtime Flow ---

#[allow(dead_code)]
pub fn parse_runtime_flow_v1(source: &str) -> Result<Flow, String> {
    let module = parse_module_v1(source).map_err(|e| e.to_string())?;
    parse_runtime_func_from_module_v1(&module)
}

pub fn parse_runtime_func_from_module_v1(module: &ModuleAst) -> Result<Flow, String> {
    let funcs: Vec<&FuncDecl> = module
        .decls
        .iter()
        .filter_map(|d| match d {
            TopDecl::Func(f) | TopDecl::Sink(f) | TopDecl::Source(f) => Some(f),
            _ => None,
        })
        .collect();

    if funcs.len() == 1 {
        return parse_runtime_func_decl_v1(funcs[0]);
    }

    // Fallback: look for flow decls (legacy compat during transition)
    let flows: Vec<&FlowDecl> = module
        .decls
        .iter()
        .filter_map(|d| match d {
            TopDecl::Flow(f) => Some(f),
            _ => None,
        })
        .collect();

    if funcs.is_empty() && flows.is_empty() {
        return Err("1:1 No `func` or `flow` declaration found".to_string());
    }
    if funcs.len() + flows.len() > 1 {
        return Err("1:1 Multiple func/flow declarations; keep one per file".to_string());
    }

    let flow_graph = parse_flow_graph_decl_v1(flows[0])?;
    lower_flow_graph_to_flow(&flow_graph)
}

pub fn parse_runtime_func_decl_v1(func_decl: &FuncDecl) -> Result<Flow, String> {
    let (emit_name, fail_name, outputs) = if let Some(ref ret_type) = func_decl.return_type {
        // v2 func: `return <Type>` / `fail <Type>` (unnamed)
        let fail_t = func_decl.fail_type.as_ref().ok_or_else(|| {
            format!(
                "{}:{} func `{}` has `return` but is missing `fail <Type>`",
                func_decl.span.line, func_decl.span.col, func_decl.name
            )
        })?;
        (
            "_return".to_string(),
            "_fail".to_string(),
            vec![
                Port {
                    name: "_return".to_string(),
                    type_name: ret_type.clone(),
                },
                Port {
                    name: "_fail".to_string(),
                    type_name: fail_t.clone(),
                },
            ],
        )
    } else {
        // v1 func: named `emit`/`fail` ports (both optional for void funcs)
        let emit_name = func_decl
            .emits
            .first()
            .map(|e| e.name.clone())
            .unwrap_or_else(|| "_void".to_string());
        let fail_name = func_decl
            .fails
            .first()
            .map(|f| f.name.clone())
            .unwrap_or_else(|| "_void_fail".to_string());

        let mut outs = Vec::new();
        for emit in &func_decl.emits {
            outs.push(Port {
                name: emit.name.clone(),
                type_name: emit.type_name.clone(),
            });
        }
        for fail in &func_decl.fails {
            outs.push(Port {
                name: fail.name.clone(),
                type_name: fail.type_name.clone(),
            });
        }
        (emit_name, fail_name, outs)
    };

    let body = parse_runtime_body_v1(&func_decl.body_text, &emit_name, &fail_name)
        .map_err(|e| e.to_string())?;

    Ok(Flow {
        name: func_decl.name.clone(),
        inputs: func_decl
            .takes
            .iter()
            .map(|take| Port {
                name: take.name.clone(),
                type_name: take.type_name.clone(),
            })
            .collect(),
        outputs,
        body,
        state_names: vec![],
        local_names: vec![],
    })
}

// --- Second-pass: flow body → FlowGraph ---

#[cfg(test)]
pub fn parse_flow_graph_from_module_v1(module: &ModuleAst) -> Result<FlowGraph, String> {
    let flows: Vec<&FlowDecl> = module
        .decls
        .iter()
        .filter_map(|d| match d {
            TopDecl::Flow(f) => Some(f),
            _ => None,
        })
        .collect();

    if flows.is_empty() {
        return Err("1:1 No `flow` declaration found".to_string());
    }
    if flows.len() > 1 {
        return Err("1:1 Multiple flows are not supported; keep one flow per file".to_string());
    }

    parse_flow_graph_decl_v1(flows[0])
}

pub fn parse_flow_graph_decl_v1(flow_decl: &FlowDecl) -> Result<FlowGraph, String> {
    let body = parse_flow_body_v1(&flow_decl.body_text).map_err(|e| e.to_string())?;

    Ok(FlowGraph {
        name: flow_decl.name.clone(),
        inputs: flow_decl
            .takes
            .iter()
            .map(|take| Port {
                name: take.name.clone(),
                type_name: take.type_name.clone(),
            })
            .collect(),
        emit_ports: flow_decl
            .emits
            .iter()
            .map(|p| Port {
                name: p.name.clone(),
                type_name: p.type_name.clone(),
            })
            .collect(),
        fail_ports: flow_decl
            .fails
            .iter()
            .map(|p| Port {
                name: p.name.clone(),
                type_name: p.type_name.clone(),
            })
            .collect(),
        body,
    })
}

// --- FlowGraph → Flow lowering ---

pub fn lower_flow_graph_to_flow(graph: &FlowGraph) -> Result<Flow, String> {
    let body = lower_flow_statements(&graph.body, &mut 0)?;

    let mut outputs = Vec::new();
    for port in &graph.emit_ports {
        outputs.push(Port {
            name: port.name.clone(),
            type_name: port.type_name.clone(),
        });
    }
    for port in &graph.fail_ports {
        outputs.push(Port {
            name: port.name.clone(),
            type_name: port.type_name.clone(),
        });
    }

    // Collect state and local variable names for cycle execution
    let mut state_names = Vec::new();
    let mut local_names = Vec::new();
    for stmt in &graph.body {
        match stmt {
            FlowStatement::State(s) => state_names.push(s.bind.clone()),
            FlowStatement::Local(l) => local_names.push(l.bind.clone()),
            _ => {}
        }
    }

    Ok(Flow {
        name: graph.name.clone(),
        inputs: graph.inputs.clone(),
        outputs,
        body,
        state_names,
        local_names,
    })
}

fn lower_step_then_items(
    items: &[StepThenItem],
    step_bind: &str,
    counter: &mut usize,
) -> Result<Vec<Statement>, String> {
    use crate::ast::FlowOnBlock;

    let mut out = Vec::new();

    // Collect on blocks for case routing
    let on_blocks: Vec<&FlowOnBlock> = items
        .iter()
        .filter_map(|item| {
            if let StepThenItem::On(on) = item {
                Some(on)
            } else {
                None
            }
        })
        .collect();

    if on_blocks.len() >= 2 {
        // Event-routed lowering: generate safe Case routing on action field
        let type_var = format!("_type_{}", *counter);
        let has_var = format!("_has_{}", *counter);
        let route_var = format!("_route_{}", *counter);
        *counter += 1;

        // _type_N = type.of(step_bind)
        out.push(Statement::Node(NodeAssign {
            bind: type_var.clone(),
            node_id: type_var.clone(),
            op: "type.of".to_string(),
            args: vec![Arg::Var {
                var: step_bind.to_string(),
            }],
            type_annotation: None,
        }));

        // Build routing case arms from on blocks
        let mut route_arms = Vec::new();
        for on in &on_blocks {
            let mut arm_body = Vec::new();

            // Bind event value: wire = obj.get(step_bind, "value")
            arm_body.push(Statement::Node(NodeAssign {
                bind: on.wire.clone(),
                node_id: on.wire.clone(),
                op: "obj.get".to_string(),
                args: vec![
                    Arg::Var {
                        var: step_bind.to_string(),
                    },
                    Arg::Lit {
                        lit: serde_json::Value::String("value".to_string()),
                    },
                ],
                type_annotation: None,
            }));

            // Recursively lower nested items in the on block body
            let nested = lower_step_then_items(&on.body, &on.wire, counter)?;
            arm_body.extend(nested);

            route_arms.push(CaseArm {
                pattern: Pattern::Lit(serde_json::Value::String(on.port.clone())),
                guard: None,
                body: arm_body,
            });
        }

        // Build nested safety checks: type == "dict" → has("action") == true → route
        let route_case = Statement::Case(CaseBlock {
            expr: Expr::Var(route_var.clone()),
            arms: route_arms,
            else_body: vec![],
        });

        let has_body = vec![
            Statement::Node(NodeAssign {
                bind: route_var.clone(),
                node_id: route_var,
                op: "obj.get".to_string(),
                args: vec![
                    Arg::Var {
                        var: step_bind.to_string(),
                    },
                    Arg::Lit {
                        lit: serde_json::Value::String("action".to_string()),
                    },
                ],
                type_annotation: None,
            }),
            route_case,
        ];

        let has_case = Statement::Case(CaseBlock {
            expr: Expr::Var(has_var.clone()),
            arms: vec![CaseArm {
                pattern: Pattern::Lit(serde_json::Value::Bool(true)),
                guard: None,
                body: has_body,
            }],
            else_body: vec![],
        });

        let dict_body = vec![
            Statement::Node(NodeAssign {
                bind: has_var.clone(),
                node_id: has_var,
                op: "obj.has".to_string(),
                args: vec![
                    Arg::Var {
                        var: step_bind.to_string(),
                    },
                    Arg::Lit {
                        lit: serde_json::Value::String("action".to_string()),
                    },
                ],
                type_annotation: None,
            }),
            has_case,
        ];

        out.push(Statement::Case(CaseBlock {
            expr: Expr::Var(type_var),
            arms: vec![CaseArm {
                pattern: Pattern::Lit(serde_json::Value::String("dict".to_string())),
                guard: None,
                body: dict_body,
            }],
            else_body: vec![],
        }));
    }

    // Process non-on items sequentially (single on blocks are handled inline here)
    for item in items {
        match item {
            StepThenItem::On(on) if on_blocks.len() < 2 => {
                // Single on block — lower its body sequentially (no case routing)
                // Bind event value: wire = obj.get(step_bind, "value")
                out.push(Statement::Node(NodeAssign {
                    bind: on.wire.clone(),
                    node_id: on.wire.clone(),
                    op: "obj.get".to_string(),
                    args: vec![
                        Arg::Var {
                            var: step_bind.to_string(),
                        },
                        Arg::Lit {
                            lit: serde_json::Value::String("value".to_string()),
                        },
                    ],
                    type_annotation: None,
                }));
                let nested = lower_step_then_items(&on.body, &on.wire, counter)?;
                out.extend(nested);
            }
            StepThenItem::On(_) => { /* multi-on handled above via case routing */ }
            StepThenItem::Next(n) => {
                if n.via_callee.is_some() {
                    // Legacy via handling (single via in sequential context)
                    if let Some(ref callee) = n.via_callee {
                        let handler_args: Vec<Arg> =
                            n.via_inputs.iter().map(|pm| pm.value.clone()).collect();
                        let via_bind = if let Some(first_out) = n.via_outputs.first() {
                            first_out.wire.clone()
                        } else {
                            n.wire.clone()
                        };
                        out.push(Statement::Node(NodeAssign {
                            bind: via_bind.clone(),
                            node_id: via_bind,
                            op: callee.clone(),
                            args: handler_args,
                            type_annotation: None,
                        }));
                    }
                } else {
                    // Simple next — bind step output to wire
                    if n.wire != step_bind {
                        out.push(Statement::ExprAssign(ExprAssign {
                            bind: n.wire.clone(),
                            type_annotation: None,
                            expr: Expr::Var(step_bind.to_string()),
                        }));
                    }
                }
            }
            StepThenItem::Step(nested_step) => {
                // Nested step: generate node call + process its then_body
                let nested_bind = if let Some(first_next) =
                    nested_step.then_body.iter().find_map(|item| {
                        if let StepThenItem::Next(n) = item {
                            if n.via_callee.is_none() {
                                return Some(&n.wire);
                            }
                        }
                        None
                    }) {
                    first_next.clone()
                } else {
                    let name = format!("_step_{}", *counter);
                    *counter += 1;
                    name
                };

                let args: Vec<Arg> = nested_step
                    .inputs
                    .iter()
                    .map(|pm| pm.value.clone())
                    .collect();
                out.push(Statement::Node(NodeAssign {
                    bind: nested_bind.clone(),
                    node_id: nested_bind.clone(),
                    op: nested_step.callee.clone(),
                    args,
                    type_annotation: None,
                }));

                let nested_stmts =
                    lower_step_then_items(&nested_step.then_body, &nested_bind, counter)?;
                out.extend(nested_stmts);
            }
            StepThenItem::Emit(e) => {
                out.push(Statement::Emit(Emit {
                    output: e.port.clone(),
                    value_expr: Expr::Var(e.wire.clone()),
                }));
            }
            StepThenItem::Fail(f) => {
                out.push(Statement::Emit(Emit {
                    output: f.port.clone(),
                    value_expr: Expr::Var(f.wire.clone()),
                }));
            }
            StepThenItem::Continuation(_) => { /* handled elsewhere */ }
        }
    }

    Ok(out)
}

fn lower_flow_statements(
    stmts: &[FlowStatement],
    counter: &mut usize,
) -> Result<Vec<Statement>, String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < stmts.len() {
        match &stmts[i] {
            FlowStatement::Step(step) => {
                // Determine bind name for this step
                let has_on_blocks = step
                    .then_body
                    .iter()
                    .any(|item| matches!(item, StepThenItem::On(_)));
                let has_via = step
                    .then_body
                    .iter()
                    .any(|item| matches!(item, StepThenItem::Next(n) if n.via_callee.is_some()));

                let bind = if has_on_blocks || has_via {
                    let name = format!("_step_{}", *counter);
                    *counter += 1;
                    name
                } else if let Some(first_next) = step.then_body.iter().find_map(|item| {
                    if let StepThenItem::Next(n) = item {
                        if n.via_callee.is_none() {
                            return Some(&n.wire);
                        }
                    }
                    None
                }) {
                    first_next.clone()
                } else {
                    let name = format!("_step_{}", *counter);
                    *counter += 1;
                    name
                };

                let args: Vec<Arg> = step.inputs.iter().map(|pm| pm.value.clone()).collect();

                out.push(Statement::Node(NodeAssign {
                    bind: bind.clone(),
                    node_id: bind.clone(),
                    op: step.callee.clone(),
                    args,
                    type_annotation: None,
                }));

                // Recursively lower then_body items
                let lowered = lower_step_then_items(&step.then_body, &bind, counter)?;
                out.extend(lowered);

                i += 1;
            }
            FlowStatement::Emit(e) => {
                out.push(Statement::Emit(Emit {
                    output: e.port.clone(),
                    value_expr: Expr::Var(e.wire.clone()),
                }));
                i += 1;
            }
            FlowStatement::Fail(f) => {
                out.push(Statement::Emit(Emit {
                    output: f.port.clone(),
                    value_expr: Expr::Var(f.wire.clone()),
                }));
                i += 1;
            }
            FlowStatement::State(state) => {
                if let Some(expr) = &state.value {
                    out.push(Statement::ExprAssign(ExprAssign {
                        bind: state.bind.clone(),
                        type_annotation: None,
                        expr: expr.clone(),
                    }));
                } else {
                    let args: Vec<Arg> = state.args.clone();
                    let bind = state.bind.clone();
                    out.push(Statement::Node(NodeAssign {
                        bind: bind.clone(),
                        node_id: bind,
                        op: state.callee.clone(),
                        args,
                        type_annotation: None,
                    }));
                }
                i += 1;
            }
            FlowStatement::Local(local) => {
                if let Some(expr) = &local.value {
                    out.push(Statement::ExprAssign(ExprAssign {
                        bind: local.bind.clone(),
                        type_annotation: None,
                        expr: expr.clone(),
                    }));
                } else {
                    let args: Vec<Arg> = local.args.clone();
                    let bind = local.bind.clone();
                    out.push(Statement::Node(NodeAssign {
                        bind: bind.clone(),
                        node_id: bind,
                        op: local.callee.clone(),
                        args,
                        type_annotation: None,
                    }));
                }
                i += 1;
            }
            FlowStatement::SendNowait(sn) => {
                let args: Vec<Expr> = sn.args.iter().map(|a| Expr::Var(a.clone())).collect();
                out.push(Statement::SendNowait(SendNowait {
                    target: sn.target.clone(),
                    args,
                }));
                i += 1;
            }
            FlowStatement::Choose(choose) => {
                let lowered = lower_branch_blocks(&choose.branches, counter)?;
                out.extend(lowered);
                i += 1;
            }
            FlowStatement::Branch(_) => {
                // Collect consecutive branch blocks into a single if/else-if/else chain
                let lowered = lower_branch_chain(&stmts[i..], &mut i, counter)?;
                out.extend(lowered);
            }
            FlowStatement::Log(log) => {
                let bind = format!("_log_{}", *counter);
                *counter += 1;
                out.push(Statement::Node(NodeAssign {
                    bind: bind.clone(),
                    node_id: bind,
                    op: "log.info".to_string(),
                    args: log.args.clone(),
                    type_annotation: None,
                }));
                i += 1;
            }
        }
    }
    Ok(out)
}

/// Lower consecutive `branch` blocks into a nested if/else-if/else `Case` statement.
/// Advances `pos` past all consumed branches.
fn lower_branch_chain(
    stmts: &[FlowStatement],
    pos: &mut usize,
    counter: &mut usize,
) -> Result<Vec<Statement>, String> {
    let mut branches: Vec<FlowBranchBlock> = Vec::new();
    let mut j = 0;

    while j < stmts.len() {
        match &stmts[j] {
            FlowStatement::Branch(branch) => {
                branches.push(branch.clone());
                j += 1;
                if branch.condition.is_none() {
                    // Unguarded branch is the else — consumes and terminates the chain.
                    break;
                }
            }
            _ => break, // Non-branch statement ends the chain
        }
    }

    *pos += j;
    lower_branch_blocks(&branches, counter)
}

fn lower_branch_blocks(
    branches: &[FlowBranchBlock],
    counter: &mut usize,
) -> Result<Vec<Statement>, String> {
    let mut guarded: Vec<(Expr, Vec<Statement>)> = Vec::new();
    let mut else_body: Vec<Statement> = Vec::new();
    for branch in branches {
        let lowered = lower_flow_statements(&branch.body, counter)?;
        match &branch.condition {
            Some(cond) => guarded.push((cond.clone(), lowered)),
            None => {
                else_body = lowered;
                break;
            }
        }
    }

    // No guarded branches: inline unconditional branch body.
    if guarded.is_empty() {
        return Ok(else_body);
    }

    // Build nested if/else-if/else: last guarded branch gets the else_body,
    // then wrap each preceding branch around it.
    let (last_cond, last_body) = guarded.pop().unwrap();
    let mut result = Statement::Case(CaseBlock {
        expr: last_cond,
        arms: vec![CaseArm {
            pattern: Pattern::Lit(serde_json::json!(true)),
            guard: None,
            body: last_body,
        }],
        else_body,
    });

    for (cond, body) in guarded.into_iter().rev() {
        result = Statement::Case(CaseBlock {
            expr: cond,
            arms: vec![CaseArm {
                pattern: Pattern::Lit(serde_json::json!(true)),
                guard: None,
                body,
            }],
            else_body: vec![result],
        });
    }

    Ok(vec![result])
}

// --- Func body parser (existing RuntimeBodyParser, unchanged) ---

fn parse_runtime_body_v1(
    body_text: &str,
    emit_output_name: &str,
    fail_output_name: &str,
) -> Result<Vec<Statement>, ParseError> {
    RuntimeBodyParser::new(body_text, emit_output_name, fail_output_name)?.parse()
}

#[derive(Clone, Copy)]
enum BodyStop {
    Done,
    Else,
    ElseIf,
    When,
    DoneWithExports,
}

struct RuntimeBodyParser {
    tokens: Vec<Token>,
    pos: usize,
    emit_output_name: String,
    fail_output_name: String,
    discard_counter: usize,
}

impl RuntimeBodyParser {
    fn new(
        source: &str,
        emit_output_name: &str,
        fail_output_name: &str,
    ) -> Result<Self, ParseError> {
        let tokens = lex(source).map_err(|e| ParseError {
            message: e.message,
            span: e.span,
        })?;
        Ok(Self {
            tokens,
            pos: 0,
            emit_output_name: emit_output_name.to_string(),
            fail_output_name: fail_output_name.to_string(),
            discard_counter: 0,
        })
    }

    fn parse(&mut self) -> Result<Vec<Statement>, ParseError> {
        let statements = self.parse_block(&[])?;
        self.skip_newlines();
        if self.at_eof() {
            Ok(statements)
        } else {
            self.err_here("unexpected trailing syntax in func body")
        }
    }

    fn parse_block(&mut self, stop: &[BodyStop]) -> Result<Vec<Statement>, ParseError> {
        let mut out = Vec::new();
        loop {
            self.skip_newlines();
            if self.at_eof() || self.at_stop(stop) {
                return Ok(out);
            }
            out.push(self.parse_statement()?);
        }
    }

    fn parse_statement(&mut self) -> Result<Statement, ParseError> {
        if self.peek_keyword("break") {
            self.bump();
            self.expect_line_end("expected newline after `break`")?;
            return Ok(Statement::Break);
        }
        if self.peek_keyword("continue") {
            self.bump();
            self.expect_line_end("expected newline after `continue`")?;
            return Ok(Statement::Continue);
        }
        if self.peek_keyword("if") {
            return self.parse_if();
        }
        if self.peek_keyword("case") {
            return self.parse_case();
        }
        if self.peek_keyword("loop") {
            return self.parse_loop();
        }
        if self.peek_keyword("on") {
            return self.parse_on();
        }
        if self.peek_symbol('[') {
            return self.parse_sync();
        }
        if self.peek_keyword("emit") {
            return self.parse_emit_stmt();
        }
        if self.peek_keyword("return") {
            return self.parse_return_stmt();
        }
        if self.peek_keyword("fail") {
            return self.parse_fail_stmt();
        }
        if self.peek_keyword("send") {
            return self.parse_send_nowait();
        }
        if self.peek_keyword("nowait") {
            return self.parse_nowait();
        }
        // Bare op call: `ns.op(args)` — sugar for `_ = ns.op(args)`
        if matches!(self.current().kind, TokenKind::Ident(_)) && self.peek_symbol_at(1, '.') {
            return self.parse_bare_call();
        }
        self.parse_assign_stmt()
    }

    fn parse_case(&mut self) -> Result<Statement, ParseError> {
        self.expect_keyword("case")?;
        let expr = self.parse_expr_atom()?;
        self.expect_line_end("expected newline after `case` expression")?;

        let mut arms = Vec::new();
        let mut else_body = Vec::new();

        loop {
            self.skip_newlines();
            if self.peek_keyword("when") {
                self.bump();
                let pattern = self.parse_pattern()?;
                let guard = if self.peek_keyword("if") {
                    self.bump(); // consume 'if'
                    Some(self.parse_pratt_expr(0)?)
                } else {
                    None
                };
                self.expect_line_end("expected newline after `when` pattern")?;
                let body = self.parse_block(&[BodyStop::When, BodyStop::Else, BodyStop::Done])?;
                arms.push(CaseArm {
                    pattern,
                    guard,
                    body,
                });
                continue;
            }
            if self.peek_keyword("else") {
                self.bump();
                self.expect_line_end("expected newline after `else`")?;
                else_body = self.parse_block(&[BodyStop::Done])?;
                self.expect_keyword("done")?;
                self.expect_line_end("expected newline after `done`")?;
                break;
            }
            if self.peek_keyword("done") {
                self.bump();
                self.expect_line_end("expected newline after `done`")?;
                break;
            }
            return self.err_here("expected `when`, `else`, or `done` in case block");
        }

        Ok(Statement::Case(CaseBlock {
            expr,
            arms,
            else_body,
        }))
    }

    /// Parse case-as-expression: `bind = case expr \n when pat then expr \n ... done`
    fn parse_case_expr(
        &mut self,
        bind: &str,
        type_annotation: Option<String>,
    ) -> Result<Statement, ParseError> {
        self.expect_keyword("case")?;
        let expr = self.parse_expr_atom()?;
        self.expect_line_end("expected newline after `case` expression")?;

        let mut arms = Vec::new();
        let mut else_body = Vec::new();

        loop {
            self.skip_newlines();
            if self.peek_keyword("when") {
                self.bump();
                let pattern = self.parse_pattern()?;
                let guard = if self.peek_keyword("if") {
                    self.bump();
                    Some(self.parse_pratt_expr(0)?)
                } else {
                    None
                };
                self.expect_keyword("then")?;
                let body = vec![self.parse_case_expr_arm_body(bind, &type_annotation)?];
                arms.push(CaseArm {
                    pattern,
                    guard,
                    body,
                });
                continue;
            }
            if self.peek_keyword("else") {
                self.bump();
                self.expect_keyword("then")?;
                else_body = vec![self.parse_case_expr_arm_body(bind, &type_annotation)?];
                self.skip_newlines();
                self.expect_keyword("done")?;
                self.expect_line_end("expected newline after `done`")?;
                break;
            }
            if self.peek_keyword("done") {
                self.bump();
                self.expect_line_end("expected newline after `done`")?;
                break;
            }
            return self.err_here("expected `when`, `else`, or `done` in case expression");
        }

        Ok(Statement::Case(CaseBlock {
            expr,
            arms,
            else_body,
        }))
    }

    /// Parse the RHS of a `then` in case-as-expression: fail/return or an expression assigned to bind.
    fn parse_case_expr_arm_body(
        &mut self,
        bind: &str,
        type_annotation: &Option<String>,
    ) -> Result<Statement, ParseError> {
        if self.peek_keyword("fail") {
            return self.parse_fail_stmt();
        }
        if self.peek_keyword("return") {
            return self.parse_return_stmt();
        }
        let arm_expr = self.parse_pratt_expr(0)?;
        self.expect_line_end("expected newline after case expression arm")?;
        Ok(Statement::ExprAssign(ExprAssign {
            bind: bind.to_string(),
            type_annotation: type_annotation.clone(),
            expr: arm_expr,
        }))
    }

    fn parse_if(&mut self) -> Result<Statement, ParseError> {
        self.expect_keyword("if")?;
        let cond = self.parse_pratt_expr(0)?;
        self.expect_line_end("expected newline after `if` condition")?;

        let body = self.parse_block(&[BodyStop::ElseIf, BodyStop::Else, BodyStop::Done])?;

        let else_body = if self.peek_keyword("else") {
            if self.peek_keyword_at(1, "if") {
                // "else if" → consume "else", recurse into parse_if()
                self.bump(); // consume "else"
                vec![self.parse_if()?]
            } else {
                // plain "else"
                self.bump(); // consume "else"
                self.expect_line_end("expected newline after `else`")?;
                let eb = self.parse_block(&[BodyStop::Done])?;
                self.expect_keyword("done")?;
                self.expect_line_end("expected newline after `done`")?;
                eb
            }
        } else {
            // bare "done" — no else
            self.expect_keyword("done")?;
            self.expect_line_end("expected newline after `done`")?;
            vec![]
        };

        Ok(Statement::Case(CaseBlock {
            expr: cond,
            arms: vec![CaseArm {
                pattern: Pattern::Lit(json!(true)),
                guard: None,
                body,
            }],
            else_body,
        }))
    }

    fn parse_loop(&mut self) -> Result<Statement, ParseError> {
        self.expect_keyword("loop")?;

        // Bare loop: `loop\n ... break ... done`
        if !self.at_eof() && matches!(self.current().kind, TokenKind::Newline) {
            self.expect_line_end("expected newline after bare `loop`")?;
            let body = self.parse_block(&[BodyStop::Done])?;
            self.expect_keyword("done")?;
            self.expect_line_end("expected newline after loop `done`")?;
            return Ok(Statement::BareLoop(BareLoopBlock { body }));
        }

        let collection = self.parse_expr_atom()?;
        self.expect_keyword("as")?;
        let item = self.expect_ident("expected loop item variable after `as`")?;

        let index = if self.peek_keyword("with") {
            self.bump();
            self.expect_keyword("index")?;
            Some(self.expect_ident("expected index variable name")?)
        } else {
            None
        };

        self.expect_line_end("expected newline after loop header")?;

        let body = self.parse_block(&[BodyStop::Done])?;
        self.expect_keyword("done")?;
        self.expect_line_end("expected newline after loop `done`")?;

        Ok(Statement::Loop(LoopBlock {
            collection,
            item,
            index,
            body,
        }))
    }

    fn parse_on(&mut self) -> Result<Statement, ParseError> {
        self.expect_keyword("on")?;

        // Expect :eventTag
        self.expect_symbol(':')?;
        let event_tag = self.expect_ident("expected event tag after `:`")?;

        // Expect `from`
        self.expect_keyword("from")?;

        // Parse the source call as an expression (e.g. http.server.accept(srv))
        let call_expr = self.parse_prefix_expr()?;

        // Extract op and args from the call expression
        let (source_op, source_args) = match call_expr {
            Expr::Call { func, args, .. } => {
                let old_args: Vec<Arg> = args
                    .iter()
                    .map(|a| match a {
                        Expr::Lit(v) => Arg::Lit { lit: v.clone() },
                        Expr::Var(v) => Arg::Var { var: v.clone() },
                        _ => Arg::Lit {
                            lit: serde_json::Value::Null,
                        },
                    })
                    .collect();
                (func, old_args)
            }
            _ => {
                return self.err_here("expected op call after `from` in on block");
            }
        };

        // Expect `to`
        self.expect_keyword("to")?;

        // Parse bind variable
        let bind = self.expect_ident("expected variable name after `to`")?;

        self.expect_line_end("expected newline after on header")?;

        let body = self.parse_block(&[BodyStop::Done])?;
        self.expect_keyword("done")?;
        self.expect_line_end("expected newline after on `done`")?;

        Ok(Statement::On(OnBlock {
            event_tag,
            source_op,
            source_args,
            bind,
            body,
        }))
    }

    fn parse_sync(&mut self) -> Result<Statement, ParseError> {
        self.expect_symbol('[')?;
        let targets = self.parse_ident_list(']')?;
        self.expect_symbol(']')?;
        if targets.is_empty() {
            return self.err_here("sync target list cannot be empty");
        }
        self.expect_symbol('=')?;
        self.expect_keyword("sync")?;
        let options = self.parse_sync_options()?;
        self.expect_line_end("expected newline after sync header")?;

        let body = self.parse_block(&[BodyStop::DoneWithExports])?;
        self.expect_keyword("done")?;
        self.expect_symbol('[')?;
        let exports = self.parse_ident_list(']')?;
        self.expect_symbol(']')?;
        self.expect_line_end("expected newline after sync `done [..]`")?;

        if exports.len() != targets.len() {
            return self.err_here(&format!(
                "sync export count ({}) must match target count ({})",
                exports.len(),
                targets.len()
            ));
        }

        Ok(Statement::Sync(SyncBlock {
            targets,
            options,
            body,
            exports,
        }))
    }

    fn parse_emit_stmt(&mut self) -> Result<Statement, ParseError> {
        self.expect_keyword("emit")?;
        let value_expr = self.parse_pratt_expr(0)?;
        // Optional `to :port` — allows named event ports in do...done blocks
        let output = if self.peek_keyword("to") {
            self.bump(); // consume `to`
            self.expect_symbol(':')?;
            self.expect_ident("expected port name after `:`")?
        } else {
            self.emit_output_name.clone()
        };
        self.expect_line_end("expected newline after emit statement")?;
        Ok(Statement::Emit(Emit { output, value_expr }))
    }

    fn parse_fail_stmt(&mut self) -> Result<Statement, ParseError> {
        self.expect_keyword("fail")?;
        let value_expr = self.parse_pratt_expr(0)?;
        self.expect_line_end("expected newline after fail statement")?;
        Ok(Statement::Emit(Emit {
            output: self.fail_output_name.clone(),
            value_expr,
        }))
    }

    fn parse_return_stmt(&mut self) -> Result<Statement, ParseError> {
        self.expect_keyword("return")?;
        let value_expr = self.parse_pratt_expr(0)?;
        self.expect_line_end("expected newline after return statement")?;
        Ok(Statement::Emit(Emit {
            output: self.emit_output_name.clone(),
            value_expr,
        }))
    }

    fn parse_send_nowait(&mut self) -> Result<Statement, ParseError> {
        self.expect_keyword("send")?;
        self.expect_keyword("nowait")?;
        let target = self.parse_var_path("expected function name after `send nowait`")?;
        self.expect_symbol('(')?;
        let args = self.parse_expr_args()?;
        self.expect_symbol(')')?;
        self.expect_line_end("expected newline after send nowait")?;
        Ok(Statement::SendNowait(SendNowait { target, args }))
    }

    fn parse_nowait(&mut self) -> Result<Statement, ParseError> {
        self.expect_keyword("nowait")?;
        let target = self.parse_var_path("expected function name after `nowait`")?;
        self.expect_symbol('(')?;
        let args = self.parse_expr_args()?;
        self.expect_symbol(')')?;
        self.expect_line_end("expected newline after nowait")?;
        Ok(Statement::SendNowait(SendNowait { target, args }))
    }

    fn parse_bare_call(&mut self) -> Result<Statement, ParseError> {
        let op = self.parse_var_path("expected dotted op name")?;
        self.expect_symbol('(')?;
        let args = self.parse_expr_args()?;
        self.expect_symbol(')')?;

        // Check for do...done block before requiring line end
        let children = self.try_parse_do_block()?;
        if children.is_none() {
            self.expect_line_end("expected newline after bare op call")?;
        } else {
            // After done, consume optional newline
            self.skip_newlines();
        }

        let bind = format!("_discard_{}", self.discard_counter);
        self.discard_counter += 1;

        // If has children, always use ExprAssign path (not Node)
        if children.is_some() {
            return Ok(Statement::ExprAssign(ExprAssign {
                bind,
                type_annotation: None,
                expr: Expr::Call {
                    func: op,
                    args,
                    children,
                },
            }));
        }

        // If all args are simple (Var/Lit), produce Statement::Node directly
        let all_simple = args
            .iter()
            .all(|a| matches!(a, Expr::Var(_) | Expr::Lit(_)));
        if all_simple {
            let old_args: Vec<Arg> = args
                .iter()
                .map(|a| match a {
                    Expr::Lit(v) => Arg::Lit { lit: v.clone() },
                    Expr::Var(v) => Arg::Var { var: v.clone() },
                    _ => unreachable!(),
                })
                .collect();
            return Ok(Statement::Node(NodeAssign {
                bind: bind.clone(),
                node_id: bind,
                op,
                args: old_args,
                type_annotation: None,
            }));
        }

        Ok(Statement::ExprAssign(ExprAssign {
            bind,
            type_annotation: None,
            expr: Expr::Call {
                func: op,
                args,
                children: None,
            },
        }))
    }

    fn parse_assign_stmt(&mut self) -> Result<Statement, ParseError> {
        let bind = self.expect_ident("expected assignment target identifier")?;

        // Optional type annotation: `bind: TypeName = expr`
        let type_annotation = if self.peek_symbol(':') {
            self.bump();
            Some(self.expect_ident("expected type name after ':'")?)
        } else {
            None
        };

        // Check for compound assignment operators (+=, -=, *=, /=, %=)
        let compound_op = match &self.current().kind {
            TokenKind::PlusEq => Some(BinOp::Add),
            TokenKind::MinusEq => Some(BinOp::Sub),
            TokenKind::StarEq => Some(BinOp::Mul),
            TokenKind::SlashEq => Some(BinOp::Div),
            TokenKind::PercentEq => Some(BinOp::Mod),
            _ => None,
        };

        if let Some(op) = compound_op {
            if type_annotation.is_some() {
                return self.err_here("type annotation is not allowed with compound assignment");
            }
            self.bump(); // consume the compound op token
            let rhs = self.parse_pratt_expr(0)?;
            self.expect_line_end("expected newline after compound assignment")?;
            let expr = Expr::BinOp {
                op,
                lhs: Box::new(Expr::Var(bind.clone())),
                rhs: Box::new(rhs),
            };
            return Ok(Statement::ExprAssign(ExprAssign {
                bind,
                type_annotation: None,
                expr,
            }));
        }

        self.expect_symbol('=')?;

        // Case-as-expression: `bind = case expr ... done`
        if self.peek_keyword("case") {
            return self.parse_case_expr(&bind, type_annotation);
        }

        let expr = self.parse_pratt_expr(0)?;
        self.expect_line_end("expected newline after assignment")?;

        // Backward compat: if the expression is a simple Call with only Var/Lit args
        // and no children block, produce a Statement::Node to keep the old IR/runtime path.
        if let Expr::Call {
            func,
            args,
            children,
            ..
        } = &expr
        {
            if children.is_some() {
                return Ok(Statement::ExprAssign(ExprAssign {
                    bind,
                    type_annotation,
                    expr,
                }));
            }
            let all_simple = args
                .iter()
                .all(|a| matches!(a, Expr::Var(_) | Expr::Lit(_)));
            if all_simple {
                let old_args: Vec<Arg> = args
                    .iter()
                    .map(|a| match a {
                        Expr::Lit(v) => Arg::Lit { lit: v.clone() },
                        Expr::Var(v) => Arg::Var { var: v.clone() },
                        _ => unreachable!(),
                    })
                    .collect();
                return Ok(Statement::Node(NodeAssign {
                    bind: bind.clone(),
                    node_id: bind,
                    op: func.clone(),
                    args: old_args,
                    type_annotation,
                }));
            }
        }

        Ok(Statement::ExprAssign(ExprAssign {
            bind,
            type_annotation,
            expr,
        }))
    }

    // --- Pratt expression parser ---

    fn infix_bp(tok: &TokenKind) -> Option<(u8, u8, BinOp)> {
        // Returns (left_bp, right_bp, op). Left < right = left-assoc; left > right = right-assoc.
        match tok {
            TokenKind::PipePipe => Some((1, 2, BinOp::Or)),
            TokenKind::AmpAmp => Some((3, 4, BinOp::And)),
            TokenKind::EqEq => Some((5, 6, BinOp::Eq)),
            TokenKind::BangEq => Some((5, 6, BinOp::Neq)),
            TokenKind::Symbol('<') => Some((7, 8, BinOp::Lt)),
            TokenKind::Symbol('>') => Some((7, 8, BinOp::Gt)),
            TokenKind::LtEq => Some((7, 8, BinOp::LtEq)),
            TokenKind::GtEq => Some((7, 8, BinOp::GtEq)),
            TokenKind::Symbol('+') => Some((9, 10, BinOp::Add)),
            TokenKind::Symbol('-') => Some((9, 10, BinOp::Sub)),
            TokenKind::Symbol('*') => Some((11, 12, BinOp::Mul)),
            TokenKind::Symbol('/') => Some((11, 12, BinOp::Div)),
            TokenKind::Symbol('%') => Some((11, 12, BinOp::Mod)),
            TokenKind::StarStar => Some((14, 13, BinOp::Pow)), // right-assoc
            _ => None,
        }
    }

    fn parse_pratt_expr(&mut self, min_bp: u8) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_prefix_expr()?;

        loop {
            // Check if we hit a line end or stop token
            if self.at_eof()
                || matches!(self.current().kind, TokenKind::Newline)
                || self.peek_symbol(')')
                || self.peek_symbol(',')
                || self.peek_symbol(']')
                || self.peek_symbol('}')
                || self.peek_symbol(':')
            {
                break;
            }

            // Postfix bracket indexing: expr[index]
            if self.peek_symbol('[') {
                self.bump(); // consume '['
                let index = self.parse_pratt_expr(0)?;
                self.expect_symbol(']')?;
                lhs = Expr::Index {
                    expr: Box::new(lhs),
                    index: Box::new(index),
                };
                continue;
            }

            // Null-coalescing: lhs ?? rhs (same precedence as ||, left-assoc)
            if matches!(self.current().kind, TokenKind::QuestionQuestion) {
                let (l_bp, r_bp) = (1u8, 2u8); // same as ||
                if l_bp < min_bp {
                    break;
                }
                self.bump(); // consume ??
                let rhs = self.parse_pratt_expr(r_bp)?;
                lhs = Expr::Coalesce {
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                };
                continue;
            }

            let Some((l_bp, r_bp, op)) = Self::infix_bp(&self.current().kind) else {
                break;
            };

            if l_bp < min_bp {
                break;
            }

            self.bump(); // consume the operator
            let rhs = self.parse_pratt_expr(r_bp)?;
            lhs = Expr::BinOp {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }

        // Ternary: cond ? then_expr : else_expr (lowest precedence, only at top-level expr)
        if min_bp == 0 && !self.at_eof() && self.peek_symbol('?') {
            self.bump(); // consume '?'
            let then_expr = self.parse_pratt_expr(0)?;
            self.expect_symbol(':')?;
            let else_expr = self.parse_pratt_expr(0)?;
            lhs = Expr::Ternary {
                cond: Box::new(lhs),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            };
        }

        Ok(lhs)
    }

    fn parse_prefix_expr(&mut self) -> Result<Expr, ParseError> {
        // List literal: [expr, expr, ...]
        if self.peek_symbol('[') {
            self.bump(); // consume '['
            let mut items = Vec::new();
            if !self.peek_symbol(']') {
                items.push(self.parse_pratt_expr(0)?);
                while self.peek_symbol(',') {
                    self.bump();
                    if self.peek_symbol(']') {
                        break;
                    }
                    items.push(self.parse_pratt_expr(0)?);
                }
            }
            self.expect_symbol(']')?;
            return Ok(Expr::ListLit(items));
        }
        // Dict literal: {key: expr, key: expr, ...}
        if self.peek_symbol('{') {
            self.bump(); // consume '{'
            let mut pairs = Vec::new();
            if !self.peek_symbol('}') {
                let key = self.expect_ident("expected key identifier in dict literal")?;
                self.expect_symbol(':')?;
                let value = self.parse_pratt_expr(0)?;
                pairs.push((key, value));
                while self.peek_symbol(',') {
                    self.bump();
                    if self.peek_symbol('}') {
                        break;
                    }
                    let key = self.expect_ident("expected key identifier in dict literal")?;
                    self.expect_symbol(':')?;
                    let value = self.parse_pratt_expr(0)?;
                    pairs.push((key, value));
                }
            }
            self.expect_symbol('}')?;
            return Ok(Expr::DictLit(pairs));
        }
        // Unary minus
        if self.peek_symbol('-') {
            self.bump();
            let inner = self.parse_prefix_expr()?;
            return Ok(Expr::UnaryOp {
                op: UnaryOp::Neg,
                expr: Box::new(inner),
            });
        }
        // Unary not
        if self.peek_symbol('!') {
            self.bump();
            let inner = self.parse_prefix_expr()?;
            return Ok(Expr::UnaryOp {
                op: UnaryOp::Not,
                expr: Box::new(inner),
            });
        }
        // Parenthesized expression
        if self.peek_symbol('(') {
            self.bump();
            let inner = self.parse_pratt_expr(0)?;
            self.expect_symbol(')')?;
            return Ok(inner);
        }
        // Atoms: number, string, bool, ident (possibly dotted path), call
        match self.current().kind.clone() {
            TokenKind::StringLit(s) => {
                self.bump();
                Ok(Expr::Lit(json!(s)))
            }
            TokenKind::StringInterp(parts) => {
                self.bump();
                parse_interp_parts(&parts)
            }
            TokenKind::Number(n) => {
                self.bump();
                if let Ok(v) = n.parse::<i64>() {
                    Ok(Expr::Lit(json!(v)))
                } else if let Ok(v) = n.parse::<f64>() {
                    Ok(Expr::Lit(json!(v)))
                } else {
                    self.err_here("invalid numeric literal")
                }
            }
            TokenKind::Ident(v) => {
                if v == "true" || v == "false" {
                    self.bump();
                    return Ok(Expr::Lit(json!(v == "true")));
                }
                if v == "null" {
                    self.bump();
                    return Ok(Expr::Lit(serde_json::Value::Null));
                }
                // Parse dotted path (e.g. input.apr)
                let path = self.parse_var_path("expected expression")?;
                // Check if it's a function call: path(...)
                if self.peek_symbol('(') {
                    self.bump(); // consume '('
                    let args = self.parse_expr_args()?;
                    self.expect_symbol(')')?;
                    let children = self.try_parse_do_block()?;
                    return Ok(Expr::Call {
                        func: path,
                        args,
                        children,
                    });
                }
                Ok(Expr::Var(path))
            }
            _ => self.err_here("expected expression"),
        }
    }

    /// If the next token is `do`, parse a `do...done` block and return `Some(stmts)`.
    /// Otherwise return `None`.
    fn try_parse_do_block(&mut self) -> Result<Option<Vec<Statement>>, ParseError> {
        if !self.peek_keyword("do") {
            return Ok(None);
        }
        self.bump(); // consume `do`
        // Optional block context binding: `do stack`
        // We only treat it as a binding when it is a single identifier followed by newline.
        let block_bind = if matches!(self.current().kind, TokenKind::Ident(_))
            && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|t| matches!(t.kind, TokenKind::Newline))
        {
            Some(self.expect_ident("expected block context identifier after `do`")?)
        } else {
            None
        };
        self.skip_newlines();
        let mut body = self.parse_block(&[BodyStop::Done])?;
        self.expect_keyword("done")?;
        if let Some(bind) = block_bind {
            body.insert(
                0,
                Statement::ExprAssign(ExprAssign {
                    bind,
                    type_annotation: None,
                    expr: Expr::Lit(json!("__forai_ui_block_ctx__")),
                }),
            );
        }
        Ok(Some(body))
    }

    fn parse_expr_args(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut args = Vec::new();
        if self.peek_symbol(')') {
            return Ok(args);
        }
        loop {
            args.push(self.parse_pratt_expr(0)?);
            if self.peek_symbol(')') {
                return Ok(args);
            }
            self.expect_symbol(',')?;
        }
    }

    fn parse_expr_atom(&mut self) -> Result<Expr, ParseError> {
        match self.current().kind.clone() {
            TokenKind::StringLit(s) => {
                self.bump();
                Ok(Expr::Lit(json!(s)))
            }
            TokenKind::StringInterp(parts) => {
                self.bump();
                parse_interp_parts(&parts)
            }
            TokenKind::Number(n) => {
                self.bump();
                if let Ok(v) = n.parse::<i64>() {
                    Ok(Expr::Lit(json!(v)))
                } else if let Ok(v) = n.parse::<f64>() {
                    Ok(Expr::Lit(json!(v)))
                } else {
                    self.err_here("invalid numeric literal")
                }
            }
            TokenKind::Ident(v) => {
                if v == "true" || v == "false" {
                    self.bump();
                    return Ok(Expr::Lit(json!(v == "true")));
                }
                let var = self.parse_var_path("expected expression")?;
                Ok(Expr::Var(var))
            }
            _ => self.err_here("expected expression"),
        }
    }

    fn parse_pattern(&mut self) -> Result<Pattern, ParseError> {
        let first = self.parse_pattern_atom()?;
        if !self.peek_symbol('|') {
            return Ok(first);
        }
        let mut alts = vec![first];
        while self.peek_symbol('|') {
            self.bump(); // consume '|'
            alts.push(self.parse_pattern_atom()?);
        }
        Ok(Pattern::Or(alts))
    }

    fn parse_pattern_atom(&mut self) -> Result<Pattern, ParseError> {
        // Type pattern: :text, :long, :real, :bool, :list, :dict, :void
        if self.peek_symbol(':') {
            self.bump(); // consume ':'
            let name = self.expect_ident("expected type name after `:`")?;
            let valid = ["text", "bool", "long", "real", "list", "dict", "void"];
            if !valid.contains(&name.as_str()) {
                return self.err_here(&format!(
                    "unknown type pattern `:{name}`, expected one of: {}",
                    valid.join(", ")
                ));
            }
            return Ok(Pattern::Type(name));
        }

        // Negative number: -N or -N..M
        if self.peek_symbol('-') {
            if let Some(next) = self.tokens.get(self.pos + 1) {
                if let TokenKind::Number(_) = &next.kind {
                    self.bump(); // consume '-'
                    if let TokenKind::Number(n) = self.current().kind.clone() {
                        self.bump();
                        let lo = n.parse::<i64>().map_err(|_| ParseError {
                            message: format!("invalid integer in range pattern `-{n}`"),
                            span: self.current().span,
                        })?;
                        let lo = -lo;
                        if matches!(self.current().kind, TokenKind::DotDot) {
                            self.bump(); // consume '..'
                            let neg_hi = self.peek_symbol('-');
                            if neg_hi {
                                self.bump();
                            }
                            if let TokenKind::Number(hi_str) = self.current().kind.clone() {
                                self.bump();
                                let mut hi = hi_str.parse::<i64>().map_err(|_| ParseError {
                                    message: format!(
                                        "invalid integer in range hi bound `{hi_str}`"
                                    ),
                                    span: self.current().span,
                                })?;
                                if neg_hi {
                                    hi = -hi;
                                }
                                return Ok(Pattern::Range { lo, hi });
                            }
                            return self.err_here("expected number after `..`");
                        }
                        return Ok(Pattern::Lit(json!(lo)));
                    }
                }
            }
        }

        match self.current().kind.clone() {
            TokenKind::StringLit(s) => {
                self.bump();
                Ok(Pattern::Lit(json!(s)))
            }
            TokenKind::Number(n) => {
                self.bump();
                // Check for range: N..M
                if matches!(self.current().kind, TokenKind::DotDot) {
                    let lo = n.parse::<i64>().map_err(|_| ParseError {
                        message: format!("range patterns require integer bounds, got `{n}`"),
                        span: self.current().span,
                    })?;
                    self.bump(); // consume '..'
                    let neg_hi = self.peek_symbol('-');
                    if neg_hi {
                        self.bump();
                    }
                    if let TokenKind::Number(hi_str) = self.current().kind.clone() {
                        self.bump();
                        let mut hi = hi_str.parse::<i64>().map_err(|_| ParseError {
                            message: format!(
                                "range patterns require integer bounds, got `{hi_str}`"
                            ),
                            span: self.current().span,
                        })?;
                        if neg_hi {
                            hi = -hi;
                        }
                        return Ok(Pattern::Range { lo, hi });
                    }
                    return self.err_here("expected number after `..`");
                }
                if let Ok(v) = n.parse::<i64>() {
                    Ok(Pattern::Lit(json!(v)))
                } else if let Ok(v) = n.parse::<f64>() {
                    Ok(Pattern::Lit(json!(v)))
                } else {
                    self.err_here("invalid numeric literal")
                }
            }
            TokenKind::Ident(v) => {
                if v == "true" || v == "false" {
                    self.bump();
                    return Ok(Pattern::Lit(json!(v == "true")));
                }
                let ident = self.parse_var_path("expected pattern")?;
                Ok(Pattern::Ident(ident))
            }
            _ => self.err_here("expected pattern"),
        }
    }

    fn parse_sync_options(&mut self) -> Result<SyncOptions, ParseError> {
        let mut options = SyncOptions::default();
        if self.at_eof() || matches!(self.current().kind, TokenKind::Newline) {
            return Ok(options);
        }

        loop {
            self.expect_symbol(':')?;
            let key = self.expect_ident("expected sync option key after `:`")?;
            self.expect_fat_arrow()?;
            let value = self.parse_sync_option_value()?;

            match key.as_str() {
                "timeout" => options.timeout = Some(value),
                "retry" => {
                    options.retry = Some(value.parse::<i64>().map_err(|_| ParseError {
                        message: format!("invalid sync option value for :retry `{value}`"),
                        span: self.current().span,
                    })?);
                }
                "safe" => {
                    options.safe = match value.as_str() {
                        "true" => true,
                        "false" => false,
                        _ => {
                            return self.err_here(&format!(
                                "invalid sync option value for :safe `{value}`"
                            ));
                        }
                    };
                }
                _ => return self.err_here(&format!("unknown sync option `:{key}`")),
            }

            if self.peek_symbol(',') {
                self.bump();
                continue;
            }
            break;
        }

        Ok(options)
    }

    fn parse_sync_option_value(&mut self) -> Result<String, ParseError> {
        let mut out = String::new();
        loop {
            if self.at_eof()
                || matches!(self.current().kind, TokenKind::Newline)
                || self.peek_symbol(',')
            {
                break;
            }

            let fragment = match &self.current().kind {
                TokenKind::Ident(v) => v.clone(),
                TokenKind::Number(v) => v.clone(),
                TokenKind::StringLit(v) => v.clone(),
                TokenKind::StringInterp(_) => break,
                TokenKind::RegexLit(v) => format!("/{v}/"),
                TokenKind::Symbol(ch) => ch.to_string(),
                TokenKind::FatArrow => "=>".to_string(),
                TokenKind::EqEq => "==".to_string(),
                TokenKind::BangEq => "!=".to_string(),
                TokenKind::GtEq => ">=".to_string(),
                TokenKind::LtEq => "<=".to_string(),
                TokenKind::AmpAmp => "&&".to_string(),
                TokenKind::PipePipe => "||".to_string(),
                TokenKind::StarStar => "**".to_string(),
                TokenKind::DotDot => "..".to_string(),
                TokenKind::PlusEq => "+=".to_string(),
                TokenKind::MinusEq => "-=".to_string(),
                TokenKind::StarEq => "*=".to_string(),
                TokenKind::SlashEq => "/=".to_string(),
                TokenKind::PercentEq => "%=".to_string(),
                TokenKind::QuestionQuestion => "??".to_string(),
                TokenKind::Newline | TokenKind::Eof => break,
            };
            out.push_str(&fragment);
            self.bump();
        }

        if out.is_empty() {
            self.err_here("expected sync option value")
        } else {
            Ok(out)
        }
    }

    fn parse_ident_list(&mut self, end_symbol: char) -> Result<Vec<String>, ParseError> {
        let mut out = Vec::new();
        if self.peek_symbol(end_symbol) {
            return Ok(out);
        }

        loop {
            out.push(self.expect_ident("expected identifier in list")?);
            if self.peek_symbol(end_symbol) {
                return Ok(out);
            }
            self.expect_symbol(',')?;
        }
    }

    fn parse_var_path(&mut self, missing_ident_msg: &str) -> Result<String, ParseError> {
        let mut out = self.expect_ident(missing_ident_msg)?;
        while self.peek_symbol('.') {
            self.bump();
            let part = self.expect_ident("expected identifier after `.`")?;
            out.push('.');
            out.push_str(&part);
        }
        Ok(out)
    }

    fn at_stop(&self, stop: &[BodyStop]) -> bool {
        stop.iter().any(|marker| match marker {
            BodyStop::Done => self.peek_keyword("done"),
            BodyStop::Else => self.peek_keyword("else"),
            BodyStop::ElseIf => self.peek_keyword("else") && self.peek_keyword_at(1, "if"),
            BodyStop::When => self.peek_keyword("when"),
            BodyStop::DoneWithExports => self.peek_keyword("done") && self.peek_symbol_at(1, '['),
        })
    }

    fn expect_line_end(&mut self, message: &str) -> Result<(), ParseError> {
        if self.consume_newline() || self.at_eof() {
            Ok(())
        } else {
            self.err_here(message)
        }
    }

    fn expect_keyword(&mut self, keyword: &str) -> Result<(), ParseError> {
        if self.peek_keyword(keyword) {
            self.bump();
            Ok(())
        } else {
            self.err_here(&format!("expected keyword `{keyword}`"))
        }
    }

    fn expect_fat_arrow(&mut self) -> Result<(), ParseError> {
        if matches!(self.current().kind, TokenKind::FatArrow) {
            self.bump();
            Ok(())
        } else {
            self.err_here("expected `=>`")
        }
    }

    fn expect_symbol(&mut self, symbol: char) -> Result<(), ParseError> {
        if self.peek_symbol(symbol) {
            self.bump();
            Ok(())
        } else {
            self.err_here(&format!("expected `{symbol}`"))
        }
    }

    fn expect_ident(&mut self, message: &str) -> Result<String, ParseError> {
        match &self.current().kind {
            TokenKind::Ident(v) => {
                let out = v.clone();
                self.bump();
                Ok(out)
            }
            _ => self.err_here(message),
        }
    }

    fn consume_newline(&mut self) -> bool {
        if matches!(self.current().kind, TokenKind::Newline) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn skip_newlines(&mut self) {
        while self.consume_newline() {}
    }

    fn peek_keyword(&self, keyword: &str) -> bool {
        matches!(&self.current().kind, TokenKind::Ident(s) if s == keyword)
    }

    fn peek_symbol(&self, symbol: char) -> bool {
        matches!(self.current().kind, TokenKind::Symbol(ch) if ch == symbol)
    }

    fn peek_symbol_at(&self, offset: usize, symbol: char) -> bool {
        self.tokens
            .get(self.pos + offset)
            .is_some_and(|t| matches!(t.kind, TokenKind::Symbol(ch) if ch == symbol))
    }

    fn peek_keyword_at(&self, offset: usize, keyword: &str) -> bool {
        self.tokens
            .get(self.pos + offset)
            .is_some_and(|t| matches!(&t.kind, TokenKind::Ident(s) if s == keyword))
    }

    fn at_eof(&self) -> bool {
        matches!(self.current().kind, TokenKind::Eof)
    }

    fn current(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn bump(&mut self) {
        if !self.at_eof() {
            self.pos += 1;
        }
    }

    fn err_here<T>(&self, message: &str) -> Result<T, ParseError> {
        Err(ParseError {
            message: message.to_string(),
            span: self.current().span,
        })
    }
}

// --- New flow body parser (step/case/emit/fail only) ---

fn parse_flow_body_v1(body_text: &str) -> Result<Vec<FlowStatement>, ParseError> {
    FlowBodyParser::new(body_text)?.parse()
}

struct FlowBodyParser {
    tokens: Vec<Token>,
    pos: usize,
}

impl FlowBodyParser {
    fn new(source: &str) -> Result<Self, ParseError> {
        let tokens = lex(source).map_err(|e| ParseError {
            message: e.message,
            span: e.span,
        })?;
        Ok(Self { tokens, pos: 0 })
    }

    fn parse(&mut self) -> Result<Vec<FlowStatement>, ParseError> {
        let mut stmts = Vec::new();
        loop {
            self.skip_newlines();
            if self.at_eof() {
                return Ok(stmts);
            }
            stmts.push(self.parse_flow_statement()?);
        }
    }

    fn parse_flow_statement(&mut self) -> Result<FlowStatement, ParseError> {
        if self.peek_keyword("step") {
            return self.parse_step_block();
        }
        if self.peek_keyword("state") {
            return self.parse_state_decl();
        }
        if self.peek_keyword("local") {
            return self.parse_local_decl();
        }
        if self.peek_keyword("send") {
            return self.parse_flow_send_nowait();
        }
        if self.peek_keyword("log") {
            return self.parse_flow_log();
        }
        if self.peek_keyword("emit") {
            let emit = self.parse_flow_emit_inner()?;
            return Ok(FlowStatement::Emit(emit));
        }
        if self.peek_keyword("fail") {
            let fail = self.parse_flow_fail_inner()?;
            return Ok(FlowStatement::Fail(fail));
        }
        if self.peek_keyword("choose") {
            return self.parse_flow_choose();
        }
        if self.peek_keyword("branch") {
            return self.parse_flow_branch();
        }
        self.err_here("expected `step`, `state`, `local`, `send`, `log`, `emit`, `fail`, `choose`, or `branch` in flow body")
    }

    // Parse: state <bind> = <callee>(<arg1>, <arg2>, ...)
    //    or: state <bind> = <literal-expr>
    fn parse_state_decl(&mut self) -> Result<FlowStatement, ParseError> {
        let span = self.current().span;
        self.expect_keyword("state")?;
        let bind = self.expect_ident("expected variable name after `state`")?;
        self.expect_symbol('=')?;
        // Peek to decide: literal expression or op call
        let is_literal = matches!(
            &self.current().kind,
            TokenKind::StringLit(_)
                | TokenKind::StringInterp(_)
                | TokenKind::Number(_)
                | TokenKind::Symbol('[')
                | TokenKind::Symbol('{')
        ) || matches!(&self.current().kind, TokenKind::Ident(s) if s == "true" || s == "false" || s == "null");
        if is_literal {
            let expr = self.parse_pratt_expr(0)?;
            self.expect_line_end("expected newline after state declaration")?;
            return Ok(FlowStatement::State(FlowStateDecl {
                bind,
                callee: String::new(),
                args: Vec::new(),
                value: Some(expr),
                span,
            }));
        }
        let callee = self.parse_var_path("expected callee after `=` in state declaration")?;
        self.expect_symbol('(')?;
        let args = self.parse_arg_list(')')?;
        self.expect_symbol(')')?;
        self.expect_line_end("expected newline after state declaration")?;
        Ok(FlowStatement::State(FlowStateDecl {
            bind,
            callee,
            args,
            value: None,
            span,
        }))
    }

    // Parse: local <bind> = <callee>(<arg1>, <arg2>, ...)
    //    or: local <bind> = <literal-expr>
    fn parse_local_decl(&mut self) -> Result<FlowStatement, ParseError> {
        let span = self.current().span;
        self.expect_keyword("local")?;
        let bind = self.expect_ident("expected variable name after `local`")?;
        self.expect_symbol('=')?;
        let is_literal = matches!(
            &self.current().kind,
            TokenKind::StringLit(_)
                | TokenKind::StringInterp(_)
                | TokenKind::Number(_)
                | TokenKind::Symbol('[')
                | TokenKind::Symbol('{')
        ) || matches!(&self.current().kind, TokenKind::Ident(s) if s == "true" || s == "false" || s == "null");
        if is_literal {
            let expr = self.parse_pratt_expr(0)?;
            self.expect_line_end("expected newline after local declaration")?;
            return Ok(FlowStatement::Local(FlowLocalDecl {
                bind,
                callee: String::new(),
                args: Vec::new(),
                value: Some(expr),
                span,
            }));
        }
        let callee = self.parse_var_path("expected callee after `=` in local declaration")?;
        self.expect_symbol('(')?;
        let args = self.parse_arg_list(')')?;
        self.expect_symbol(')')?;
        self.expect_line_end("expected newline after local declaration")?;
        Ok(FlowStatement::Local(FlowLocalDecl {
            bind,
            callee,
            args,
            value: None,
            span,
        }))
    }

    // Parse: log <arg1>, <arg2>
    fn parse_flow_log(&mut self) -> Result<FlowStatement, ParseError> {
        let span = self.current().span;
        self.expect_keyword("log")?;
        let mut args = Vec::new();
        // Parse comma-separated args until newline/eof
        loop {
            match self.current().kind.clone() {
                TokenKind::StringLit(s) => {
                    self.bump();
                    args.push(Arg::Lit { lit: json!(s) });
                }
                TokenKind::Number(n) => {
                    self.bump();
                    if let Ok(v) = n.parse::<i64>() {
                        args.push(Arg::Lit { lit: json!(v) });
                    } else if let Ok(v) = n.parse::<f64>() {
                        args.push(Arg::Lit { lit: json!(v) });
                    } else {
                        return self.err_here("invalid numeric literal in log");
                    }
                }
                TokenKind::Ident(v) if v == "true" || v == "false" => {
                    let b = v == "true";
                    self.bump();
                    args.push(Arg::Lit { lit: json!(b) });
                }
                TokenKind::Ident(_) => {
                    let ident = self.expect_ident("expected identifier")?;
                    args.push(Arg::Var { var: ident });
                }
                _ => return self.err_here("expected argument after `log`"),
            }
            if self.at_eof() || matches!(self.current().kind, TokenKind::Newline) {
                break;
            }
            self.expect_symbol(',')?;
        }
        self.expect_line_end("expected newline after log statement")?;
        Ok(FlowStatement::Log(FlowLogStmt { args, span }))
    }

    // Parse: send nowait <target>(<arg1>, <arg2>, ...)
    fn parse_flow_send_nowait(&mut self) -> Result<FlowStatement, ParseError> {
        let span = self.current().span;
        self.expect_keyword("send")?;
        self.expect_keyword("nowait")?;
        let target = self.parse_var_path("expected target after `send nowait`")?;
        self.expect_symbol('(')?;
        let args = self.parse_ident_list(')')?;
        self.expect_symbol(')')?;
        self.expect_line_end("expected newline after send nowait")?;
        Ok(FlowStatement::SendNowait(FlowSendNowait {
            target,
            args,
            span,
        }))
    }

    fn parse_step_block(&mut self) -> Result<FlowStatement, ParseError> {
        let span = self.current().span;
        self.expect_keyword("step")?;

        // v2 detection: if next token is NOT newline, it's inline syntax
        if !self.at_eof() && !matches!(self.current().kind, TokenKind::Newline) {
            return self.parse_step_inline(span);
        }

        self.expect_line_end("expected newline after `step`")?;

        // Parse callee: dotted.Ident(...)
        self.skip_newlines();
        let callee = self.parse_var_path("expected callee name in step block")?;
        self.expect_symbol('(')?;
        let inputs = self.parse_port_mappings()?;
        self.expect_symbol(')')?;
        self.expect_line_end("expected newline after step call")?;

        // Parse then_body items
        let then_body = self.parse_then_items()?;

        self.expect_keyword("done")?;
        self.expect_line_end("expected newline after step `done`")?;

        Ok(FlowStatement::Step(StepBlock {
            callee,
            inputs,
            then_body,
            span,
        }))
    }

    fn parse_step_inline(&mut self, span: Span) -> Result<FlowStatement, ParseError> {
        // Parse callee and inputs: callee(port_mappings)
        let callee = self.parse_var_path("expected callee name after `step`")?;
        self.expect_symbol('(')?;
        let inputs = self.parse_port_mappings()?;
        self.expect_symbol(')')?;

        // Fire-and-forget: `step callee(...) done`
        if self.peek_keyword("done") {
            self.bump();
            self.expect_line_end("expected newline after step `done`")?;
            return Ok(FlowStatement::Step(StepBlock {
                callee,
                inputs,
                then_body: Vec::new(),
                span,
            }));
        }

        // v2 with then: `step callee(...) then`
        self.expect_keyword("then")?;
        self.expect_line_end("expected newline after `then`")?;

        let then_body = self.parse_then_items()?;

        self.expect_keyword("done")?;
        self.expect_line_end("expected newline after step `done`")?;

        Ok(FlowStatement::Step(StepBlock {
            callee,
            inputs,
            then_body,
            span,
        }))
    }

    fn parse_then_items(&mut self) -> Result<Vec<StepThenItem>, ParseError> {
        let mut items = Vec::new();
        loop {
            self.skip_newlines();
            if self.at_eof() || self.peek_keyword("done") {
                break;
            }
            if self.peek_keyword("next") {
                let next_span = self.current().span;
                self.bump();
                self.expect_symbol(':')?;
                let port = self.expect_ident("expected port name after `next :`")?;
                self.expect_keyword("to")?;
                let wire = self.expect_ident("expected wire label after `to`")?;
                self.expect_line_end("expected newline after next declaration")?;
                items.push(StepThenItem::Next(NextWire {
                    port,
                    wire,
                    via_callee: None,
                    via_inputs: vec![],
                    via_outputs: vec![],
                    span: next_span,
                }));
                continue;
            }
            if self.peek_keyword("on") {
                let on_span = self.current().span;
                self.bump(); // consume `on`
                self.expect_symbol(':')?;
                let port = self.expect_ident("expected port name after `on :`")?;
                self.expect_keyword("as")?;
                let wire = self.expect_ident("expected wire name after `as`")?;
                self.expect_keyword("then")?;
                self.expect_line_end("expected newline after `then`")?;
                let body = self.parse_then_items()?; // RECURSIVE
                self.expect_keyword("done")?;
                self.skip_newlines();
                items.push(StepThenItem::On(FlowOnBlock {
                    port,
                    wire,
                    body,
                    span: on_span,
                }));
                continue;
            }
            if self.peek_keyword("step") {
                let step_span = self.current().span;
                self.bump(); // consume `step`
                let step_stmt = self.parse_step_inline(step_span)?;
                if let FlowStatement::Step(step_block) = step_stmt {
                    items.push(StepThenItem::Step(step_block));
                }
                continue;
            }
            if self.peek_keyword("emit") {
                let emit = self.parse_flow_emit_inner()?;
                items.push(StepThenItem::Emit(emit));
                continue;
            }
            if self.peek_keyword("fail") {
                let fail = self.parse_flow_fail_inner()?;
                items.push(StepThenItem::Fail(fail));
                continue;
            }
            if self.peek_symbol(':') {
                let cont_span = self.current().span;
                self.bump();
                let port = self.expect_ident("expected event name after `:`")?;
                self.expect_keyword("to")?;
                let cont_callee = self.parse_var_path("expected callee after `to`")?;
                self.expect_symbol('(')?;
                let args = self.parse_ident_list(')')?;
                self.expect_symbol(')')?;
                self.expect_line_end("expected newline after continuation")?;
                items.push(StepThenItem::Continuation(ContinuationWire {
                    port,
                    callee: cont_callee,
                    args,
                    span: cont_span,
                }));
                continue;
            }
            return self.err_here("expected `next`, `on`, `step`, `emit`, `fail`, `:event to callee(...)`, or `done` in step then block");
        }
        Ok(items)
    }

    fn parse_port_mappings(&mut self) -> Result<Vec<PortMapping>, ParseError> {
        let mut mappings = Vec::new();
        if self.peek_symbol(')') {
            return Ok(mappings);
        }
        loop {
            let span = self.current().span;
            let value = match self.current().kind.clone() {
                TokenKind::StringLit(s) => {
                    self.bump();
                    Arg::Lit {
                        lit: serde_json::json!(s),
                    }
                }
                TokenKind::Number(n) => {
                    self.bump();
                    if let Ok(v) = n.parse::<i64>() {
                        Arg::Lit {
                            lit: serde_json::json!(v),
                        }
                    } else if let Ok(v) = n.parse::<f64>() {
                        Arg::Lit {
                            lit: serde_json::json!(v),
                        }
                    } else {
                        return self.err_here("invalid numeric literal in port mapping");
                    }
                }
                TokenKind::Ident(ref v) if v == "true" || v == "false" => {
                    let b = v == "true";
                    self.bump();
                    Arg::Lit {
                        lit: serde_json::json!(b),
                    }
                }
                TokenKind::Ident(_) => {
                    let var_path = self.parse_var_path("expected wire label in port mapping")?;
                    Arg::Var { var: var_path }
                }
                _ => return self.err_here("expected identifier or literal in port mapping"),
            };
            self.expect_keyword("to")?;
            self.expect_symbol(':')?;
            let port = self.expect_ident("expected port name after `:`")?;
            mappings.push(PortMapping { port, value, span });
            if self.peek_symbol(')') {
                return Ok(mappings);
            }
            self.expect_symbol(',')?;
        }
    }

    fn parse_flow_emit_inner(&mut self) -> Result<FlowEmitStmt, ParseError> {
        let span = self.current().span;
        self.expect_keyword("emit")?;
        let wire = self.expect_ident("expected wire label after `emit`")?;
        self.expect_keyword("to")?;
        self.expect_symbol(':')?;
        let port = self.expect_ident("expected port name after `:`")?;
        self.expect_line_end("expected newline after emit statement")?;
        Ok(FlowEmitStmt { port, wire, span })
    }

    fn parse_flow_fail_inner(&mut self) -> Result<FlowEmitStmt, ParseError> {
        let span = self.current().span;
        self.expect_keyword("fail")?;
        let wire = self.expect_ident("expected wire label after `fail`")?;
        self.expect_keyword("to")?;
        self.expect_symbol(':')?;
        let port = self.expect_ident("expected port name after `:`")?;
        self.expect_line_end("expected newline after fail statement")?;
        Ok(FlowEmitStmt { port, wire, span })
    }

    fn parse_var_path(&mut self, msg: &str) -> Result<String, ParseError> {
        let mut out = self.expect_ident(msg)?;
        while self.peek_symbol('.') {
            self.bump();
            let part = self.expect_ident("expected identifier after `.`")?;
            out.push('.');
            out.push_str(&part);
        }
        Ok(out)
    }

    fn parse_ident_list(&mut self, end_symbol: char) -> Result<Vec<String>, ParseError> {
        let mut out = Vec::new();
        if self.peek_symbol(end_symbol) {
            return Ok(out);
        }
        loop {
            out.push(self.expect_ident("expected identifier in list")?);
            if self.peek_symbol(end_symbol) {
                return Ok(out);
            }
            self.expect_symbol(',')?;
        }
    }

    fn parse_arg_list(&mut self, end_symbol: char) -> Result<Vec<Arg>, ParseError> {
        let mut out = Vec::new();
        if self.peek_symbol(end_symbol) {
            return Ok(out);
        }
        loop {
            match self.current().kind.clone() {
                TokenKind::StringLit(s) => {
                    self.bump();
                    out.push(Arg::Lit { lit: json!(s) });
                }
                TokenKind::Number(n) => {
                    self.bump();
                    if let Ok(v) = n.parse::<i64>() {
                        out.push(Arg::Lit { lit: json!(v) });
                    } else if let Ok(v) = n.parse::<f64>() {
                        out.push(Arg::Lit { lit: json!(v) });
                    } else {
                        return self.err_here("invalid numeric literal");
                    }
                }
                TokenKind::Ident(v) if v == "true" || v == "false" => {
                    let b = v == "true";
                    self.bump();
                    out.push(Arg::Lit { lit: json!(b) });
                }
                TokenKind::Ident(_) => {
                    let ident = self.expect_ident("expected identifier")?;
                    out.push(Arg::Var { var: ident });
                }
                _ => return self.err_here("expected identifier or literal in argument list"),
            }
            if self.peek_symbol(end_symbol) {
                return Ok(out);
            }
            self.expect_symbol(',')?;
        }
    }

    fn expect_line_end(&mut self, message: &str) -> Result<(), ParseError> {
        if self.consume_newline() || self.at_eof() {
            Ok(())
        } else {
            self.err_here(message)
        }
    }

    fn expect_keyword(&mut self, keyword: &str) -> Result<(), ParseError> {
        if self.peek_keyword(keyword) {
            self.bump();
            Ok(())
        } else {
            self.err_here(&format!("expected keyword `{keyword}`"))
        }
    }

    fn expect_symbol(&mut self, symbol: char) -> Result<(), ParseError> {
        if self.peek_symbol(symbol) {
            self.bump();
            Ok(())
        } else {
            self.err_here(&format!("expected `{symbol}`"))
        }
    }

    fn expect_ident(&mut self, message: &str) -> Result<String, ParseError> {
        match &self.current().kind {
            TokenKind::Ident(v) => {
                let out = v.clone();
                self.bump();
                Ok(out)
            }
            _ => self.err_here(message),
        }
    }

    fn consume_newline(&mut self) -> bool {
        if matches!(self.current().kind, TokenKind::Newline) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn skip_newlines(&mut self) {
        while self.consume_newline() {}
    }

    fn peek_keyword(&self, keyword: &str) -> bool {
        matches!(&self.current().kind, TokenKind::Ident(s) if s == keyword)
    }

    fn peek_newline(&self) -> bool {
        matches!(self.current().kind, TokenKind::Newline)
    }

    fn peek_symbol(&self, symbol: char) -> bool {
        matches!(self.current().kind, TokenKind::Symbol(ch) if ch == symbol)
    }

    fn at_eof(&self) -> bool {
        matches!(self.current().kind, TokenKind::Eof)
    }

    fn current(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn bump(&mut self) {
        if !self.at_eof() {
            self.pos += 1;
        }
    }

    fn err_here<T>(&self, message: &str) -> Result<T, ParseError> {
        Err(ParseError {
            message: message.to_string(),
            span: self.current().span,
        })
    }

    // --- Pratt expression parser (ported from RuntimeBodyParser for branch conditions) ---

    fn infix_bp(tok: &TokenKind) -> Option<(u8, u8, BinOp)> {
        match tok {
            TokenKind::PipePipe => Some((1, 2, BinOp::Or)),
            TokenKind::AmpAmp => Some((3, 4, BinOp::And)),
            TokenKind::EqEq => Some((5, 6, BinOp::Eq)),
            TokenKind::BangEq => Some((5, 6, BinOp::Neq)),
            TokenKind::Symbol('<') => Some((7, 8, BinOp::Lt)),
            TokenKind::Symbol('>') => Some((7, 8, BinOp::Gt)),
            TokenKind::LtEq => Some((7, 8, BinOp::LtEq)),
            TokenKind::GtEq => Some((7, 8, BinOp::GtEq)),
            TokenKind::Symbol('+') => Some((9, 10, BinOp::Add)),
            TokenKind::Symbol('-') => Some((9, 10, BinOp::Sub)),
            TokenKind::Symbol('*') => Some((11, 12, BinOp::Mul)),
            TokenKind::Symbol('/') => Some((11, 12, BinOp::Div)),
            TokenKind::Symbol('%') => Some((11, 12, BinOp::Mod)),
            TokenKind::StarStar => Some((14, 13, BinOp::Pow)),
            _ => None,
        }
    }

    fn parse_pratt_expr(&mut self, min_bp: u8) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_prefix_expr()?;

        loop {
            if self.at_eof()
                || matches!(self.current().kind, TokenKind::Newline)
                || self.peek_symbol(')')
                || self.peek_symbol(',')
                || self.peek_symbol(']')
                || self.peek_symbol('}')
                || self.peek_symbol(':')
            {
                break;
            }

            // Postfix bracket indexing: expr[index]
            if self.peek_symbol('[') {
                self.bump(); // consume '['
                let index = self.parse_pratt_expr(0)?;
                self.expect_symbol(']')?;
                lhs = Expr::Index {
                    expr: Box::new(lhs),
                    index: Box::new(index),
                };
                continue;
            }

            // Null-coalescing: lhs ?? rhs (same precedence as ||, left-assoc)
            if matches!(self.current().kind, TokenKind::QuestionQuestion) {
                let (l_bp, r_bp) = (1u8, 2u8);
                if l_bp < min_bp {
                    break;
                }
                self.bump();
                let rhs = self.parse_pratt_expr(r_bp)?;
                lhs = Expr::Coalesce {
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                };
                continue;
            }

            let Some((l_bp, r_bp, op)) = Self::infix_bp(&self.current().kind) else {
                break;
            };

            if l_bp < min_bp {
                break;
            }

            self.bump();
            let rhs = self.parse_pratt_expr(r_bp)?;
            lhs = Expr::BinOp {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }

        // Ternary: cond ? then_expr : else_expr
        if min_bp == 0 && !self.at_eof() && self.peek_symbol('?') {
            self.bump();
            let then_expr = self.parse_pratt_expr(0)?;
            self.expect_symbol(':')?;
            let else_expr = self.parse_pratt_expr(0)?;
            lhs = Expr::Ternary {
                cond: Box::new(lhs),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            };
        }

        Ok(lhs)
    }

    fn parse_prefix_expr(&mut self) -> Result<Expr, ParseError> {
        if self.peek_symbol('[') {
            self.bump();
            let mut items = Vec::new();
            if !self.peek_symbol(']') {
                items.push(self.parse_pratt_expr(0)?);
                while self.peek_symbol(',') {
                    self.bump();
                    if self.peek_symbol(']') {
                        break;
                    }
                    items.push(self.parse_pratt_expr(0)?);
                }
            }
            self.expect_symbol(']')?;
            return Ok(Expr::ListLit(items));
        }
        if self.peek_symbol('{') {
            self.bump();
            let mut pairs = Vec::new();
            if !self.peek_symbol('}') {
                let key = self.expect_ident("expected key identifier in dict literal")?;
                self.expect_symbol(':')?;
                let value = self.parse_pratt_expr(0)?;
                pairs.push((key, value));
                while self.peek_symbol(',') {
                    self.bump();
                    if self.peek_symbol('}') {
                        break;
                    }
                    let key = self.expect_ident("expected key identifier in dict literal")?;
                    self.expect_symbol(':')?;
                    let value = self.parse_pratt_expr(0)?;
                    pairs.push((key, value));
                }
            }
            self.expect_symbol('}')?;
            return Ok(Expr::DictLit(pairs));
        }
        if self.peek_symbol('-') {
            self.bump();
            let inner = self.parse_prefix_expr()?;
            return Ok(Expr::UnaryOp {
                op: UnaryOp::Neg,
                expr: Box::new(inner),
            });
        }
        if self.peek_symbol('!') {
            self.bump();
            let inner = self.parse_prefix_expr()?;
            return Ok(Expr::UnaryOp {
                op: UnaryOp::Not,
                expr: Box::new(inner),
            });
        }
        if self.peek_symbol('(') {
            self.bump();
            let inner = self.parse_pratt_expr(0)?;
            self.expect_symbol(')')?;
            return Ok(inner);
        }
        match self.current().kind.clone() {
            TokenKind::StringLit(s) => {
                self.bump();
                Ok(Expr::Lit(serde_json::json!(s)))
            }
            TokenKind::StringInterp(parts) => {
                self.bump();
                parse_interp_parts(&parts)
            }
            TokenKind::Number(n) => {
                self.bump();
                if let Ok(v) = n.parse::<i64>() {
                    Ok(Expr::Lit(serde_json::json!(v)))
                } else if let Ok(v) = n.parse::<f64>() {
                    Ok(Expr::Lit(serde_json::json!(v)))
                } else {
                    self.err_here("invalid numeric literal")
                }
            }
            TokenKind::Ident(v) => {
                if v == "true" || v == "false" {
                    self.bump();
                    return Ok(Expr::Lit(serde_json::json!(v == "true")));
                }
                if v == "null" {
                    self.bump();
                    return Ok(Expr::Lit(serde_json::Value::Null));
                }
                let path = self.parse_var_path("expected expression")?;
                if self.peek_symbol('(') {
                    self.bump();
                    let args = self.parse_expr_args()?;
                    self.expect_symbol(')')?;
                    return Ok(Expr::Call {
                        func: path,
                        args,
                        children: None,
                    });
                }
                Ok(Expr::Var(path))
            }
            _ => self.err_here("expected expression"),
        }
    }

    fn parse_expr_args(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut args = Vec::new();
        if self.peek_symbol(')') {
            return Ok(args);
        }
        loop {
            args.push(self.parse_pratt_expr(0)?);
            if self.peek_symbol(')') {
                return Ok(args);
            }
            self.expect_symbol(',')?;
        }
    }

    fn parse_flow_choose(&mut self) -> Result<FlowStatement, ParseError> {
        let span = self.current().span;
        self.expect_keyword("choose")?;
        self.expect_line_end("expected newline after `choose`")?;

        let mut branches = Vec::new();
        let mut saw_default = false;

        loop {
            self.skip_newlines();
            if self.at_eof() || self.peek_keyword("done") {
                break;
            }
            if !self.peek_keyword("branch") {
                return self.err_here("expected `branch` or `done` in choose block");
            }
            if saw_default {
                return self.err_here("bare `branch` must be last in choose block");
            }
            let stmt = self.parse_flow_branch()?;
            let FlowStatement::Branch(branch) = stmt else {
                unreachable!("parse_flow_branch always returns FlowStatement::Branch");
            };
            if branch.condition.is_none() {
                saw_default = true;
            }
            branches.push(branch);
        }

        if branches.is_empty() {
            return Err(ParseError {
                message: "choose block requires at least one branch".to_string(),
                span,
            });
        }

        self.expect_keyword("done")?;
        self.expect_line_end("expected newline after choose `done`")?;
        Ok(FlowStatement::Choose(FlowChooseBlock { branches, span }))
    }

    // --- Branch parsing ---

    fn parse_flow_branch(&mut self) -> Result<FlowStatement, ParseError> {
        let span = self.current().span;
        self.expect_keyword("branch")?;

        let condition = if self.peek_keyword("when") {
            self.bump(); // consume `when`
            Some(self.parse_pratt_expr(0)?)
        } else if !self.peek_newline() {
            Some(self.parse_pratt_expr(0)?)
        } else {
            None
        };

        self.expect_line_end("expected newline after `branch` header")?;

        let body = self.parse_branch_body()?;

        self.expect_keyword("done")?;
        self.expect_line_end("expected newline after branch `done`")?;

        Ok(FlowStatement::Branch(FlowBranchBlock {
            condition,
            body,
            span,
        }))
    }

    fn parse_branch_body(&mut self) -> Result<Vec<FlowStatement>, ParseError> {
        let mut out = Vec::new();
        loop {
            self.skip_newlines();
            if self.at_eof() || self.peek_keyword("done") {
                return Ok(out);
            }
            out.push(self.parse_flow_statement()?);
        }
    }
}

// --- Module-level parser (first pass) ---

struct TokenParser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    pos: usize,
}

impl<'a> TokenParser<'a> {
    fn new(source: &'a str, tokens: Vec<Token>) -> Self {
        Self {
            source,
            tokens,
            pos: 0,
        }
    }

    fn parse_module(&mut self) -> Result<ModuleAst, ParseError> {
        let mut decls = Vec::new();
        while !self.at_eof() {
            self.skip_newlines();
            if self.at_eof() {
                break;
            }

            let open = if self.peek_keyword("open") {
                self.bump();
                true
            } else {
                false
            };

            if self.peek_keyword("use") {
                if open {
                    return self.err_here("`open use` is not valid");
                }
                decls.push(TopDecl::Uses(self.parse_uses()?));
            } else if self.peek_keyword("docs") {
                if open {
                    return self.err_here("`open docs` is not valid");
                }
                decls.push(TopDecl::Docs(self.parse_docs()?));
            } else if self.peek_keyword("func") {
                if open {
                    return self.err_here("`open func` is not valid; all funcs are public");
                }
                decls.push(TopDecl::Func(self.parse_func()?));
            } else if self.peek_keyword("sink") {
                if open {
                    return self.err_here("`open sink` is not valid; all sinks are public");
                }
                decls.push(TopDecl::Sink(self.parse_sink()?));
            } else if self.peek_keyword("source") {
                if open {
                    return self.err_here("`open source` is not valid; all sources are public");
                }
                decls.push(TopDecl::Source(self.parse_source()?));
            } else if self.peek_keyword("flow") {
                if open {
                    return self.err_here("`open flow` is not valid; all flows are public");
                }
                decls.push(TopDecl::Flow(self.parse_flow()?));
            } else if self.peek_keyword("type") {
                decls.push(TopDecl::Type(self.parse_type(open)?));
            } else if self.peek_keyword("data") {
                decls.push(TopDecl::Type(self.parse_data(open)?));
            } else if self.peek_keyword("enum") {
                decls.push(TopDecl::Enum(self.parse_enum(open)?));
            } else if self.peek_keyword("test") {
                if open {
                    return self.err_here("`open test` is not valid");
                }
                decls.push(TopDecl::Test(self.parse_test()?));
            } else if self.peek_keyword("extern") {
                if open {
                    return self.err_here("`open extern` is not valid");
                }
                decls.push(TopDecl::Extern(self.parse_extern_block()?));
            } else {
                return self.err_here("expected top-level declaration");
            }
        }
        Ok(ModuleAst { decls })
    }

    fn parse_uses(&mut self) -> Result<UsesDecl, ParseError> {
        let span = self.expect_keyword("use")?.span;

        // Check for destructured import: use { Name1, Name2 } from "path"
        if self.peek_symbol('{') {
            self.bump(); // consume '{'
            let mut imports = Vec::new();
            loop {
                let ident = self.expect_ident_value("expected identifier inside `{ ... }`")?;
                imports.push(ident);
                if self.peek_symbol(',') {
                    self.bump(); // consume ','
                } else {
                    break;
                }
            }
            self.expect_symbol('}')?;
            self.expect_keyword("from")?;
            let path = self.expect_string_lit_value("expected path string after `from`")?;
            self.expect_line_end("expected newline after use declaration")?;
            // Derive module name from path (last segment without extension)
            let name = path
                .rsplit('/')
                .next()
                .unwrap_or(&path)
                .trim_end_matches(".fa")
                .to_string();
            Ok(UsesDecl {
                name,
                path,
                imports,
                span,
            })
        } else {
            let name = self.expect_ident_value("expected name after `use`")?;
            self.expect_keyword("from")?;
            let path = self.expect_string_lit_value("expected path string after `from`")?;
            self.expect_line_end("expected newline after use declaration")?;
            Ok(UsesDecl {
                name,
                path,
                imports: Vec::new(),
                span,
            })
        }
    }

    fn expect_string_lit_value(&mut self, message: &str) -> Result<String, ParseError> {
        match self.current().kind.clone() {
            TokenKind::StringLit(s) => {
                self.bump();
                Ok(s)
            }
            _ => self.err_here(message),
        }
    }

    fn parse_docs(&mut self) -> Result<DocsDecl, ParseError> {
        let span = self.expect_keyword("docs")?.span;
        let name = self.expect_ident_value("expected identifier after `docs`")?;
        self.expect_line_end("expected newline after docs header")?;

        let start = self.current().start;
        while !self.at_eof() {
            if self.peek_keyword("done") && self.current().span.col == 1 {
                let end = self.current().start;
                let raw = self.source[start..end].trim_end().to_string();
                let (markdown, field_docs) = extract_field_docs(&raw);
                self.bump();
                self.consume_newline();
                return Ok(DocsDecl {
                    name,
                    markdown,
                    field_docs,
                    span,
                });
            }
            self.bump();
        }

        self.err_at(span, "unclosed docs block")
    }

    fn parse_func(&mut self) -> Result<FuncDecl, ParseError> {
        let kw_span = self.expect_keyword("func")?.span;
        let name = self.expect_ident_value("expected func name")?;
        self.expect_line_end("expected newline after func declaration")?;

        let (takes, emits, fails, return_type, fail_type) = self.parse_header_ports()?;

        self.expect_keyword("body")?;
        self.expect_line_end("expected newline after `body`")?;

        let body_text =
            self.collect_body_text(kw_span, &["case", "loop", "sync", "if", "on", "do"])?;

        Ok(FuncDecl {
            name,
            takes,
            emits,
            fails,
            return_type,
            fail_type,
            body_text,
            span: kw_span,
        })
    }

    fn parse_sink(&mut self) -> Result<FuncDecl, ParseError> {
        let kw_span = self.expect_keyword("sink")?.span;
        let name = self.expect_ident_value("expected sink name")?;
        self.expect_line_end("expected newline after sink declaration")?;

        let (takes, emits, fails, _return_type, _fail_type) = self.parse_header_ports()?;

        self.expect_keyword("body")?;
        self.expect_line_end("expected newline after `body`")?;

        let body_text =
            self.collect_body_text(kw_span, &["case", "loop", "sync", "if", "on", "do"])?;

        Ok(FuncDecl {
            name,
            takes,
            emits,
            fails,
            return_type: None,
            fail_type: None,
            body_text,
            span: kw_span,
        })
    }

    fn parse_source(&mut self) -> Result<FuncDecl, ParseError> {
        let kw_span = self.expect_keyword("source")?.span;
        let name = self.expect_ident_value("expected source name")?;
        self.expect_line_end("expected newline after source declaration")?;

        let (takes, emits, fails, return_type, fail_type) = self.parse_header_ports()?;

        self.skip_newlines();

        if !self.peek_keyword("body") {
            return self.err_here("expected `body` in source");
        }
        self.expect_keyword("body")?;
        self.expect_line_end("expected newline after `body`")?;
        let body_text =
            self.collect_body_text(kw_span, &["case", "loop", "sync", "if", "on", "do"])?;

        Ok(FuncDecl {
            name,
            takes,
            emits,
            fails,
            return_type,
            fail_type,
            body_text,
            span: kw_span,
        })
    }

    fn parse_flow(&mut self) -> Result<FlowDecl, ParseError> {
        let kw_span = self.expect_keyword("flow")?.span;
        let name = self.expect_ident_value("expected flow name")?;
        self.expect_line_end("expected newline after flow declaration")?;

        let (takes, emits, fails, _return_type, _fail_type) = self.parse_header_ports()?;

        self.expect_keyword("body")?;
        self.expect_line_end("expected newline after `body`")?;

        let body_text =
            self.collect_body_text(kw_span, &["step", "case", "if", "branch", "choose", "on"])?;

        Ok(FlowDecl {
            name,
            takes,
            emits,
            fails,
            body_text,
            span: kw_span,
        })
    }

    fn parse_header_ports(
        &mut self,
    ) -> Result<
        (
            Vec<TakeDecl>,
            Vec<PortDecl>,
            Vec<PortDecl>,
            Option<String>,
            Option<String>,
        ),
        ParseError,
    > {
        let mut takes = Vec::new();
        let mut emits = Vec::new();
        let mut fails = Vec::new();
        let mut return_type: Option<String> = None;
        let mut fail_type: Option<String> = None;

        loop {
            self.skip_newlines();
            if self.peek_keyword("take") {
                takes.push(self.parse_take()?);
                continue;
            }
            if self.peek_keyword("emit") {
                emits.push(self.parse_port_decl("emit")?);
                continue;
            }
            if self.peek_keyword("return") {
                // v2 syntax: `return <Type>`
                self.bump(); // consume "return"
                let type_name = self.expect_ident_value("expected type name after `return`")?;
                self.expect_line_end("expected newline after return declaration")?;
                return_type = Some(type_name);
                continue;
            }
            if self.peek_keyword("fail") {
                // Disambiguate v1 vs v2:
                // v1: `fail <name> as <Type>` — second token after fail is `as`
                // v2: `fail <Type>` — second token after fail is newline/eof/body
                if self.is_v1_fail_port() {
                    fails.push(self.parse_port_decl("fail")?);
                } else {
                    // v2 syntax: `fail <Type>`
                    self.bump(); // consume "fail"
                    let type_name = self.expect_ident_value("expected type name after `fail`")?;
                    self.expect_line_end("expected newline after fail declaration")?;
                    fail_type = Some(type_name);
                }
                continue;
            }
            break;
        }

        Ok((takes, emits, fails, return_type, fail_type))
    }

    /// Check if the current `fail` keyword starts a v1 named port (`fail <name> as <Type>`)
    /// by peeking two tokens ahead for the `as` keyword.
    fn is_v1_fail_port(&self) -> bool {
        // Current token is `fail` (pos). Next is ident (pos+1). If pos+2 is `as`, it's v1.
        let pos2 = self.pos + 2;
        if pos2 < self.tokens.len() {
            matches!(&self.tokens[pos2].kind, TokenKind::Ident(s) if s == "as")
        } else {
            false
        }
    }

    fn collect_body_text(
        &mut self,
        open_span: Span,
        nesting_keywords: &[&str],
    ) -> Result<String, ParseError> {
        let body_start = self.current().start;
        let mut depth = 1i32;
        let mut prev_was_else = false;
        let mut at_stmt_start = true; // track if we're at the beginning of a statement
        let mut paren_depth = 0i32; // track ( ) to skip keywords inside function call args
        let mut prev_was_close_paren = false; // track if previous token was `)`
        let mut line_has_nesting_kw = false; // track if current line started with a nesting keyword

        while !self.at_eof() {
            // Track parenthesis depth to avoid treating `:branch`, `:step` etc. as keywords
            match &self.current().kind {
                TokenKind::Symbol('(') => {
                    paren_depth += 1;
                    self.bump();
                    prev_was_else = false;
                    prev_was_close_paren = false;
                    at_stmt_start = false;
                    continue;
                }
                TokenKind::Symbol(')') => {
                    paren_depth -= 1;
                    self.bump();
                    prev_was_else = false;
                    prev_was_close_paren = true;
                    at_stmt_start = false;
                    continue;
                }
                TokenKind::Newline => {
                    at_stmt_start = true;
                    line_has_nesting_kw = false;
                    self.bump();
                    prev_was_else = false;
                    // Don't reset prev_was_close_paren across newlines —
                    // `via Handler()\nthen` should still count
                    continue;
                }
                _ => {}
            }

            if let Some(word) = self.current_ident() {
                if paren_depth > 0 {
                    // Inside a function call — identifiers here are argument labels, not keywords
                    prev_was_else = false;
                } else if word == "done" {
                    depth -= 1;
                    if depth == 0 {
                        let body_end = self.current().start;
                        let body_text = self.source[body_start..body_end].trim_end().to_string();
                        self.bump();
                        self.consume_newline();
                        return Ok(body_text);
                    }
                    prev_was_else = false;
                } else if word == "then"
                    && paren_depth == 0
                    && prev_was_close_paren
                    && !line_has_nesting_kw
                {
                    // `then` after `)` on a line without a nesting keyword (e.g., `via Handler(args) then`)
                    // introduces a nested block with its own `done`.
                    // `step View() then` has `step` (nesting keyword) so `then` doesn't double-count.
                    let next_pos = self.pos + 1;
                    let next_is_eol = next_pos >= self.tokens.len()
                        || matches!(self.tokens[next_pos].kind, TokenKind::Newline);
                    if next_is_eol {
                        depth += 1;
                    }
                    prev_was_else = false;
                } else if word == "if" && nesting_keywords.contains(&"if") && prev_was_else {
                    // "else if" — continuation, not a new nesting level
                    prev_was_else = false;
                } else if word == "if" && nesting_keywords.contains(&"if") && !at_stmt_start {
                    // "if" mid-line (e.g. guard in `when _ if expr`) — not a nesting keyword
                    prev_was_else = false;
                } else if nesting_keywords.contains(&word) {
                    depth += 1;
                    if at_stmt_start {
                        line_has_nesting_kw = true;
                    }
                    prev_was_else = false;
                } else {
                    prev_was_else = word == "else";
                }
            } else {
                prev_was_else = false;
            }
            prev_was_close_paren = false;
            at_stmt_start = false;
            self.bump();
        }

        self.err_at(open_span, "unclosed block")
    }

    fn parse_take(&mut self) -> Result<TakeDecl, ParseError> {
        let span = self.expect_keyword("take")?.span;
        let name = self.expect_ident_value("expected input name")?;
        self.expect_keyword("as")?;
        let type_name = self.expect_ident_value("expected type name")?;
        self.expect_line_end("expected newline after take declaration")?;
        Ok(TakeDecl {
            name,
            type_name,
            span,
        })
    }

    fn parse_port_decl(&mut self, kw: &str) -> Result<PortDecl, ParseError> {
        let span = self.expect_keyword(kw)?.span;
        let name = self.expect_ident_value("expected port name")?;
        self.expect_keyword("as")?;
        let type_name = self.expect_ident_value("expected type name")?;
        self.expect_line_end("expected newline after declaration")?;
        Ok(PortDecl {
            name,
            type_name,
            span,
        })
    }

    fn parse_type(&mut self, open: bool) -> Result<TypeDecl, ParseError> {
        let span = self.expect_keyword("type")?.span;
        let name = self.expect_ident_value("expected type name")?;

        if self.peek_keyword("as") {
            self.bump();
            let base_type = self.expect_ident_value("expected base type after `as`")?;
            let constraints = self.parse_constraint_list()?;
            self.expect_line_end("expected newline after type declaration")?;
            Ok(TypeDecl {
                open,
                name,
                kind: TypeKind::Scalar {
                    base_type,
                    constraints,
                },
                span,
            })
        } else {
            self.expect_line_end("expected newline after type declaration")?;
            let fields = self.parse_field_decls()?;
            self.expect_keyword("done")?;
            self.consume_newline();
            Ok(TypeDecl {
                open,
                name,
                kind: TypeKind::Struct { fields },
                span,
            })
        }
    }

    fn parse_data(&mut self, open: bool) -> Result<TypeDecl, ParseError> {
        let span = self.expect_keyword("data")?.span;
        let name = self.expect_ident_value("expected data name")?;
        self.expect_line_end("expected newline after data declaration")?;
        let fields = self.parse_field_decls()?;
        self.expect_keyword("done")?;
        self.consume_newline();
        Ok(TypeDecl {
            open,
            name,
            kind: TypeKind::Struct { fields },
            span,
        })
    }

    fn parse_field_decls(&mut self) -> Result<Vec<FieldDecl>, ParseError> {
        let mut fields = Vec::new();
        loop {
            self.skip_newlines();
            if self.at_eof() || self.peek_keyword("done") {
                return Ok(fields);
            }
            fields.push(self.parse_field_decl()?);
        }
    }

    fn parse_field_decl(&mut self) -> Result<FieldDecl, ParseError> {
        let span = self.current().span;
        let name = self.expect_ident_value("expected field name")?;
        let type_ref = self.expect_ident_value("expected field type")?;
        let constraints = self.parse_constraint_list()?;
        self.expect_line_end("expected newline after field declaration")?;
        Ok(FieldDecl {
            name,
            type_ref,
            constraints,
            span,
        })
    }

    fn parse_constraint_list(&mut self) -> Result<Vec<TypeConstraint>, ParseError> {
        let mut constraints = Vec::new();
        while self.peek_symbol(':') {
            constraints.push(self.parse_constraint()?);
            if self.peek_symbol(',') {
                self.bump();
            }
        }
        Ok(constraints)
    }

    fn parse_constraint(&mut self) -> Result<TypeConstraint, ParseError> {
        let span = self.current().span;
        self.expect_symbol(':')?;
        let key = self.expect_ident_value("expected constraint key after `:`")?;
        if self.peek_fat_arrow() {
            self.bump();
            let value = self.parse_constraint_value()?;
            Ok(TypeConstraint { key, value, span })
        } else {
            Ok(TypeConstraint {
                key: key.clone(),
                value: ConstraintValue::Bool(true),
                span,
            })
        }
    }

    fn parse_constraint_value(&mut self) -> Result<ConstraintValue, ParseError> {
        match &self.current().kind {
            TokenKind::Ident(s) if s == "true" => {
                self.bump();
                Ok(ConstraintValue::Bool(true))
            }
            TokenKind::Ident(s) if s == "false" => {
                self.bump();
                Ok(ConstraintValue::Bool(false))
            }
            TokenKind::Number(n) => {
                let val = n.parse::<f64>().map_err(|_| ParseError {
                    message: format!("invalid numeric constraint value `{n}`"),
                    span: self.current().span,
                })?;
                self.bump();
                Ok(ConstraintValue::Number(val))
            }
            TokenKind::RegexLit(p) => {
                let pat = p.clone();
                self.bump();
                Ok(ConstraintValue::Regex(pat))
            }
            TokenKind::StringLit(s) => {
                let val = s.clone();
                self.bump();
                Ok(ConstraintValue::Regex(val))
            }
            TokenKind::Symbol(':') => {
                self.bump();
                let sym = self.expect_ident_value("expected symbol name after `:`")?;
                Ok(ConstraintValue::Symbol(sym))
            }
            _ => self.err_here(
                "expected constraint value (true/false, number, regex, string, or :symbol)",
            ),
        }
    }

    fn peek_fat_arrow(&self) -> bool {
        matches!(self.current().kind, TokenKind::FatArrow)
    }

    fn expect_symbol(&mut self, symbol: char) -> Result<(), ParseError> {
        if self.peek_symbol(symbol) {
            self.bump();
            Ok(())
        } else {
            self.err_here(&format!("expected `{symbol}`"))
        }
    }

    fn peek_symbol(&self, symbol: char) -> bool {
        matches!(self.current().kind, TokenKind::Symbol(ch) if ch == symbol)
    }

    fn parse_enum(&mut self, open: bool) -> Result<EnumDecl, ParseError> {
        let span = self.expect_keyword("enum")?.span;
        let name = self.expect_ident_value("expected enum name")?;
        self.expect_line_end("expected newline after enum declaration")?;

        let mut variants = Vec::new();
        while !self.at_eof() {
            self.skip_newlines();
            if self.peek_keyword("done") {
                self.bump();
                self.consume_newline();
                return Ok(EnumDecl {
                    open,
                    name,
                    variants,
                    span,
                });
            }
            variants.push(self.expect_ident_value("expected enum variant name")?);
            self.expect_line_end("expected newline after enum variant")?;
        }

        self.err_at(span, "unclosed enum block")
    }

    fn parse_test(&mut self) -> Result<TestDecl, ParseError> {
        let span = self.expect_keyword("test")?.span;
        let name = self.expect_ident_value("expected test name")?;
        self.expect_line_end("expected newline after test declaration")?;

        let start = self.current().start;
        while !self.at_eof() {
            if self.peek_keyword("done") && self.current().span.col == 1 {
                let end = self.current().start;
                let body_text = self.source[start..end].trim_end().to_string();
                self.bump();
                self.consume_newline();
                return Ok(TestDecl {
                    name,
                    body_text,
                    span,
                });
            }
            self.bump();
        }

        self.err_at(span, "unclosed test block")
    }

    fn parse_extern_block(&mut self) -> Result<ExternBlock, ParseError> {
        let span = self.expect_keyword("extern")?.span;
        let lib_name =
            self.expect_string_lit_value("expected library name string after `extern`")?;
        self.expect_line_end("expected newline after extern declaration")?;

        let mut fns = Vec::new();
        loop {
            self.skip_newlines();
            if self.at_eof() {
                return self.err_at(span, "unclosed extern block");
            }
            if self.peek_keyword("done") {
                self.bump();
                self.consume_newline();
                break;
            }
            if self.peek_keyword("fn") {
                fns.push(self.parse_extern_fn()?);
            } else {
                return self.err_here("expected `fn` or `done` in extern block");
            }
        }

        Ok(ExternBlock {
            lib_name,
            fns,
            span,
        })
    }

    fn parse_extern_fn(&mut self) -> Result<ExternFnDecl, ParseError> {
        let span = self.expect_keyword("fn")?.span;
        let name = self.expect_ident_value("expected function name after `fn`")?;
        self.expect_line_end("expected newline after fn declaration")?;

        let mut takes = Vec::new();
        let mut return_type = None;

        loop {
            self.skip_newlines();
            if self.at_eof() {
                return self.err_at(span, "unclosed extern fn block");
            }
            if self.peek_keyword("done") {
                self.bump();
                self.consume_newline();
                break;
            }
            if self.peek_keyword("take") {
                takes.push(self.parse_take()?);
            } else if self.peek_keyword("return") {
                self.bump();
                let type_name = self.expect_ident_value("expected type name after `return`")?;
                self.expect_line_end("expected newline after return declaration")?;
                return_type = Some(type_name);
            } else {
                return self.err_here("expected `take`, `return`, or `done` in extern fn");
            }
        }

        Ok(ExternFnDecl {
            name,
            takes,
            return_type,
            span,
        })
    }

    fn expect_line_end(&mut self, message: &str) -> Result<(), ParseError> {
        if self.consume_newline() {
            return Ok(());
        }
        self.err_here(message)
    }

    fn consume_newline(&mut self) -> bool {
        if matches!(self.current().kind, TokenKind::Newline) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn skip_newlines(&mut self) {
        while self.consume_newline() {}
    }

    fn expect_keyword(&mut self, keyword: &str) -> Result<Token, ParseError> {
        if self.peek_keyword(keyword) {
            let t = self.current().clone();
            self.bump();
            Ok(t)
        } else {
            self.err_here(&format!("expected keyword `{keyword}`"))
        }
    }

    fn expect_ident_value(&mut self, message: &str) -> Result<String, ParseError> {
        match &self.current().kind {
            TokenKind::Ident(v) => {
                let out = v.clone();
                self.bump();
                Ok(out)
            }
            _ => self.err_here(message),
        }
    }

    fn current_ident(&self) -> Option<&str> {
        if let TokenKind::Ident(s) = &self.current().kind {
            Some(s.as_str())
        } else {
            None
        }
    }

    fn peek_keyword(&self, keyword: &str) -> bool {
        matches!(&self.current().kind, TokenKind::Ident(s) if s == keyword)
    }

    fn at_eof(&self) -> bool {
        matches!(self.current().kind, TokenKind::Eof)
    }

    fn current(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn bump(&mut self) {
        if !self.at_eof() {
            self.pos += 1;
        }
    }

    fn err_here<T>(&self, message: &str) -> Result<T, ParseError> {
        Err(ParseError {
            message: message.to_string(),
            span: self.current().span,
        })
    }

    fn err_at<T>(&self, span: Span, message: &str) -> Result<T, ParseError> {
        Err(ParseError {
            message: message.to_string(),
            span,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::TopDecl;

    #[test]
    fn parses_docs_and_func() {
        let src = r#"
docs LoginFunc
  Example docs for login func.
done

func LoginFunc
  take req as Request
  emit res as Response
  fail err as AuthError
body
  user = auth.find_user(req)
  emit user
done
"#;

        let module = parse_module_v1(src).expect("module should parse");
        assert_eq!(module.decls.len(), 2);
        match &module.decls[0] {
            TopDecl::Docs(d) => assert_eq!(d.name, "LoginFunc"),
            _ => panic!("expected docs decl"),
        }
        match &module.decls[1] {
            TopDecl::Func(f) => {
                assert_eq!(f.name, "LoginFunc");
                assert_eq!(f.takes.len(), 1);
                assert_eq!(f.emits.len(), 1);
                assert_eq!(f.fails.len(), 1);
            }
            _ => panic!("expected func decl"),
        }
    }

    #[test]
    fn rejects_open_func() {
        let src = r#"
open func LoginFunc
  take req as Request
  emit res as Response
  fail err as AuthError
body
  emit req
done
"#;
        let err = parse_module_v1(src).expect_err("open func should be rejected");
        assert!(
            err.message.contains("all funcs are public"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn builds_runtime_flow_from_func() {
        let src = r#"
func LoginFunc
  take req as Request
  emit res as Response
  fail err as AuthError
body
  user = auth.find_user(req)
  emit user
done
"#;

        let flow = parse_runtime_flow_v1(src).expect("runtime flow should parse");
        assert_eq!(flow.name, "LoginFunc");
        assert_eq!(flow.inputs.len(), 1);
        assert_eq!(flow.outputs.len(), 2);
        assert_eq!(flow.body.len(), 2);
    }

    #[test]
    fn parses_case_loop_sync_in_func_body() {
        let src = r#"
func DemoFunc
  take request as HttpRequest
  emit response as HttpResponse
  fail error as AuthError
body
  params = http.extract_params(request)
  [user, credentials] = sync :safe => false
    user_local = db.query_user_by_email(params)
    credentials_local = db.query_credentials(user_local)
  done [user_local, credentials_local]
  checks = auth.sample_checks()
  loop checks as check
    pass = auth.pass_through(check)
  done
  case user
    when "x"
      emit pass
    else
      fail pass
  done
done
"#;

        let flow = parse_runtime_flow_v1(src).expect("runtime flow with control statements");
        assert_eq!(flow.body.len(), 5);
    }

    #[test]
    fn parses_flow_with_steps() {
        let src = r#"
flow NumberCrunch
  take input as NumberInput
  emit result as NumberResult
  fail error as NumberError
body
  step
    calc.AddTwo(input to :input)
    next :result to added
  done
  step
    calc.MultiplyFive(added to :input)
    next :result to multiplied
    emit multiplied to :result
  done
done
"#;

        let module = parse_module_v1(src).expect("module should parse");
        match &module.decls[0] {
            TopDecl::Flow(f) => {
                assert_eq!(f.name, "NumberCrunch");
                assert_eq!(f.emits.len(), 1);
                assert_eq!(f.fails.len(), 1);
            }
            _ => panic!("expected flow decl"),
        }

        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        assert_eq!(graph.name, "NumberCrunch");
        assert_eq!(graph.body.len(), 2); // 2 steps
        assert!(matches!(graph.body[0], FlowStatement::Step(_)));
        assert!(matches!(graph.body[1], FlowStatement::Step(_)));
    }

    #[test]
    fn flow_body_rejects_bind_syntax() {
        let src = r#"
flow BadFlow
  take input as Foo
  emit result as Bar
  fail error as Baz
body
  x = some.op(input)
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let err = parse_flow_graph_from_module_v1(&module).expect_err("should reject bind syntax");
        assert!(err.contains("expected `step`"), "got: {err}");
    }

    #[test]
    fn rejects_case_in_step_then_block() {
        let src = r#"
flow CaseFlow
  take input as Foo
  emit ok as Bar
  fail error as Baz
body
  step check.Validate(input to :input) then
    next :valid to is_valid
    case is_valid
      when true
        emit input to :ok
    done
  done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let err = parse_flow_graph_from_module_v1(&module)
            .expect_err("should reject case in step then block");
        assert!(
            err.contains("expected `next`"),
            "error should mention valid then-block items, got: {err}"
        );
    }

    #[test]
    fn parses_dotted_var_in_step_arg() {
        let src = r#"
flow Route
  take req as dict
body
  step handler.Run(req.conn_id to :id, req.method to :method) done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        match &graph.body[0] {
            FlowStatement::Step(step) => {
                assert_eq!(step.inputs.len(), 2);
                assert_eq!(
                    step.inputs[0].value,
                    Arg::Var {
                        var: "req.conn_id".to_string()
                    }
                );
                assert_eq!(step.inputs[0].port, "id");
                assert_eq!(
                    step.inputs[1].value,
                    Arg::Var {
                        var: "req.method".to_string()
                    }
                );
                assert_eq!(step.inputs[1].port, "method");
            }
            _ => panic!("expected Step"),
        }
    }

    #[test]
    fn parses_multiple_emit_fail_ports() {
        let src = r#"
func MultiPort
  take input as Foo
  emit ok as Bar
  emit partial as Baz
  fail error as Err
  fail timeout as TimeoutErr
body
  emit input
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        match &module.decls[0] {
            TopDecl::Func(f) => {
                assert_eq!(f.emits.len(), 2);
                assert_eq!(f.fails.len(), 2);
                assert_eq!(f.emits[0].name, "ok");
                assert_eq!(f.emits[1].name, "partial");
                assert_eq!(f.fails[0].name, "error");
                assert_eq!(f.fails[1].name, "timeout");
            }
            _ => panic!("expected func decl"),
        }
    }

    #[test]
    fn parses_open_data_decl() {
        let src = r#"
open data UserRecord
  id uuid
done
"#;

        let module = parse_module_v1(src).expect("module should parse");
        assert_eq!(module.decls.len(), 1);
        match &module.decls[0] {
            TopDecl::Type(d) => {
                assert!(d.open);
                assert_eq!(d.name, "UserRecord");
                match &d.kind {
                    crate::ast::TypeKind::Struct { fields } => {
                        assert_eq!(fields.len(), 1);
                        assert_eq!(fields[0].name, "id");
                        assert_eq!(fields[0].type_ref, "uuid");
                    }
                    _ => panic!("expected struct kind"),
                }
            }
            _ => panic!("expected type decl"),
        }
    }

    #[test]
    fn parses_scalar_type_with_constraints() {
        let src = "type Email as text :matches => /@/, :min => 3\n";
        let module = parse_module_v1(src).expect("module should parse");
        assert_eq!(module.decls.len(), 1);
        match &module.decls[0] {
            TopDecl::Type(d) => {
                assert_eq!(d.name, "Email");
                match &d.kind {
                    crate::ast::TypeKind::Scalar {
                        base_type,
                        constraints,
                    } => {
                        assert_eq!(base_type, "text");
                        assert_eq!(constraints.len(), 2);
                        assert_eq!(constraints[0].key, "matches");
                        assert_eq!(constraints[1].key, "min");
                    }
                    _ => panic!("expected scalar kind"),
                }
            }
            _ => panic!("expected type decl"),
        }
    }

    #[test]
    fn parses_struct_type_with_fields() {
        let src = r#"
type LoginRequest
  email text :required => true
  password text :min => 8
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        assert_eq!(module.decls.len(), 1);
        match &module.decls[0] {
            TopDecl::Type(d) => {
                assert_eq!(d.name, "LoginRequest");
                match &d.kind {
                    crate::ast::TypeKind::Struct { fields } => {
                        assert_eq!(fields.len(), 2);
                        assert_eq!(fields[0].name, "email");
                        assert_eq!(fields[0].type_ref, "text");
                        assert_eq!(fields[0].constraints.len(), 1);
                        assert_eq!(fields[0].constraints[0].key, "required");
                        assert_eq!(fields[1].name, "password");
                        assert_eq!(fields[1].constraints[0].key, "min");
                    }
                    _ => panic!("expected struct kind"),
                }
            }
            _ => panic!("expected type decl"),
        }
    }

    #[test]
    fn parses_data_keyword_as_struct() {
        let src = r#"
data UserRecord
  id uuid
  name text :required => true
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        assert_eq!(module.decls.len(), 1);
        match &module.decls[0] {
            TopDecl::Type(d) => {
                assert_eq!(d.name, "UserRecord");
                match &d.kind {
                    crate::ast::TypeKind::Struct { fields } => {
                        assert_eq!(fields.len(), 2);
                        assert_eq!(fields[0].name, "id");
                        assert_eq!(fields[0].type_ref, "uuid");
                        assert_eq!(fields[1].name, "name");
                    }
                    _ => panic!("expected struct kind"),
                }
            }
            _ => panic!("expected type decl"),
        }
    }

    #[test]
    fn parses_scalar_type_no_constraints() {
        let src = "type ID as uuid\n";
        let module = parse_module_v1(src).expect("module should parse");
        assert_eq!(module.decls.len(), 1);
        match &module.decls[0] {
            TopDecl::Type(d) => {
                assert_eq!(d.name, "ID");
                match &d.kind {
                    crate::ast::TypeKind::Scalar {
                        base_type,
                        constraints,
                    } => {
                        assert_eq!(base_type, "uuid");
                        assert!(constraints.is_empty());
                    }
                    _ => panic!("expected scalar kind"),
                }
            }
            _ => panic!("expected type decl"),
        }
    }

    #[test]
    fn parses_sink_declaration() {
        let src = r#"
docs Greet
  A greeting sink.
done

sink Greet
  take name as Text
  emit greeting as Text
  fail error as Error
body
  greeting = fmt.wrap_field("hello", name)
  emit greeting
done
"#;

        let module = parse_module_v1(src).expect("module should parse");
        assert_eq!(module.decls.len(), 2);
        match &module.decls[1] {
            TopDecl::Sink(f) => {
                assert_eq!(f.name, "Greet");
                assert_eq!(f.takes.len(), 1);
                assert_eq!(f.emits.len(), 1);
                assert_eq!(f.fails.len(), 1);
            }
            _ => panic!("expected sink decl"),
        }
    }

    #[test]
    fn rejects_open_sink() {
        let src = r#"
open sink Greet
  take name as Text
  emit greeting as Text
  fail error as Error
body
  emit name
done
"#;
        let err = parse_module_v1(src).expect_err("open sink should be rejected");
        assert!(
            err.message.contains("all sinks are public"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn sink_compiles_to_runtime_flow() {
        let src = r#"
docs Greet
  A greeting sink.
done

sink Greet
  take name as Text
  emit greeting as Text
  fail error as Error
body
  greeting = fmt.wrap_field("hello", name)
  emit greeting
done
"#;
        let flow = parse_runtime_flow_v1(src).expect("sink should parse as runtime flow");
        assert_eq!(flow.name, "Greet");
        assert_eq!(flow.inputs.len(), 1);
        assert_eq!(flow.outputs.len(), 2);
    }

    #[test]
    fn parses_v2_step_with_then() {
        let src = r#"
flow TestFlow
  take input as Foo
  emit result as Bar
  fail error as Baz
body
  step calc.AddTwo(input to :input) then
    next :result to added
    emit added to :result
  done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        assert_eq!(graph.body.len(), 1); // step
        match &graph.body[0] {
            FlowStatement::Step(s) => {
                assert_eq!(s.callee, "calc.AddTwo");
                assert_eq!(s.inputs.len(), 1);
                assert_eq!(s.then_body.len(), 2); // next + emit
                match &s.then_body[0] {
                    StepThenItem::Next(n) => {
                        assert_eq!(n.port, "result");
                        assert_eq!(n.wire, "added");
                    }
                    _ => panic!("expected Next"),
                }
                assert!(matches!(s.then_body[1], StepThenItem::Emit(_)));
            }
            _ => panic!("expected step"),
        }
    }

    #[test]
    fn parses_v2_fire_and_forget_step() {
        let src = r#"
flow TestFlow
  take input as Foo
  emit result as Bar
  fail error as Baz
body
  step logger.Log(input to :input) done
  step passthrough.Id(input to :input) then
    next :result to result
    emit result to :result
  done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        assert_eq!(graph.body.len(), 2); // fire-and-forget step + step with emit
        match &graph.body[0] {
            FlowStatement::Step(s) => {
                assert_eq!(s.callee, "logger.Log");
                assert!(s.then_body.is_empty());
            }
            _ => panic!("expected step"),
        }
    }

    #[test]
    fn parses_v2_step_with_continuation() {
        let src = r#"
flow TestFlow
  take request as Foo
  emit response as Bar
  fail error as Baz
body
  step auth.ExtractParams(request to :request) then
    next :params to params
    :error to ErrorHandler(params)
    emit params to :response
  done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        match &graph.body[0] {
            FlowStatement::Step(s) => {
                assert_eq!(s.then_body.len(), 3); // next + continuation + emit
                match &s.then_body[0] {
                    StepThenItem::Next(n) => assert_eq!(n.wire, "params"),
                    _ => panic!("expected Next"),
                }
                match &s.then_body[1] {
                    StepThenItem::Continuation(c) => {
                        assert_eq!(c.port, "error");
                        assert_eq!(c.callee, "ErrorHandler");
                        assert_eq!(c.args, vec!["params"]);
                    }
                    _ => panic!("expected Continuation"),
                }
                assert!(matches!(s.then_body[2], StepThenItem::Emit(_)));
            }
            _ => panic!("expected step"),
        }
    }

    #[test]
    fn parses_mixed_v1_and_v2_steps() {
        let src = r#"
flow MixedFlow
  take input as Foo
  emit result as Bar
  fail error as Baz
body
  step
    calc.First(input to :input)
    next :result to first
  done
  step calc.Second(first to :input) then
    next :result to second
  done
  step logger.Log(second to :input) then
    emit second to :result
  done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        assert_eq!(graph.body.len(), 3); // v1 step + v2 then step + step with emit
        assert!(matches!(graph.body[0], FlowStatement::Step(_)));
        assert!(matches!(graph.body[1], FlowStatement::Step(_)));
        assert!(matches!(graph.body[2], FlowStatement::Step(_)));
    }

    #[test]
    fn lower_flow_graph_single_step() {
        let src = r#"
flow TestFlow
  take input as Foo
  emit result as Bar
  fail error as Baz
body
  step calc.AddTwo(input to :input) then
    next :result to added
    emit added to :result
  done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        let flow = lower_flow_graph_to_flow(&graph).expect("lowering should succeed");

        assert_eq!(flow.name, "TestFlow");
        assert_eq!(flow.inputs.len(), 1);
        assert_eq!(flow.outputs.len(), 2); // emit + fail
        assert_eq!(flow.body.len(), 2); // node + emit

        match &flow.body[0] {
            Statement::Node(n) => {
                assert_eq!(n.bind, "added");
                assert_eq!(n.op, "calc.AddTwo");
                assert_eq!(n.args.len(), 1);
                match &n.args[0] {
                    Arg::Var { var } => assert_eq!(var, "input"),
                    _ => panic!("expected Arg::Var"),
                }
            }
            _ => panic!("expected Statement::Node"),
        }
    }

    #[test]
    fn lower_flow_graph_emit() {
        let src = r#"
flow EmitFlow
  take input as Foo
  emit result as Bar
  fail error as Baz
body
  step passthrough.Id(input to :input) then
    next :result to output
    emit output to :result
  done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        let flow = lower_flow_graph_to_flow(&graph).expect("lowering should succeed");

        assert_eq!(flow.body.len(), 2); // node + emit
        match &flow.body[1] {
            Statement::Emit(e) => {
                assert_eq!(e.output, "result");
                assert!(matches!(&e.value_expr, Expr::Var(v) if v == "output"));
            }
            _ => panic!("expected Statement::Emit"),
        }
    }

    #[test]
    fn lower_fire_and_forget_gets_synthetic_bind() {
        let src = r#"
flow LogFlow
  take input as Foo
  emit result as Bar
  fail error as Baz
body
  step logger.Log(input to :input) done
  step passthrough.Id(input to :input) then
    next :result to result
    emit result to :result
  done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        let flow = lower_flow_graph_to_flow(&graph).expect("lowering should succeed");

        match &flow.body[0] {
            Statement::Node(n) => {
                assert_eq!(n.bind, "_step_0");
                assert_eq!(n.op, "logger.Log");
            }
            _ => panic!("expected Statement::Node"),
        }
    }

    #[test]
    fn flow_compiles_via_parse_runtime_flow_v1() {
        let src = r#"
use calc from "./calc"

docs main
  Pipes through two steps.
done

flow main
  take input as NumberInput
  emit result as NumberResult
  fail error as NumberError
body
  step calc.AddTwo(input to :input) then
    next :result to added
  done
  step calc.MultiplyFive(added to :input) then
    next :result to result
    emit result to :result
  done
done
"#;
        let flow = parse_runtime_flow_v1(src).expect("flow should parse via runtime path");
        assert_eq!(flow.name, "main");
        assert_eq!(flow.body.len(), 3); // 2 nodes + 1 emit
    }

    // --- Expression parser tests ---

    #[test]
    fn old_call_syntax_still_produces_node() {
        let src = r#"
func Calc
  take input as Foo
  emit result as Bar
  fail error as Baz
body
  x = math.floor(input)
  emit x
done
"#;
        let flow = parse_runtime_flow_v1(src).expect("should parse");
        assert!(matches!(flow.body[0], Statement::Node(_)));
    }

    #[test]
    fn infix_expr_produces_expr_assign() {
        let src = r#"
func Calc
  take input as Foo
  emit result as Bar
  fail error as Baz
body
  x = input + 1
  emit x
done
"#;
        let flow = parse_runtime_flow_v1(src).expect("should parse");
        assert!(matches!(flow.body[0], Statement::ExprAssign(_)));
    }

    #[test]
    fn expr_precedence_mul_before_add() {
        let src = r#"
func Calc
  take input as Foo
  emit result as Bar
  fail error as Baz
body
  x = 2 + 3 * 4
  emit x
done
"#;
        let flow = parse_runtime_flow_v1(src).expect("should parse");
        // Should be (2 + (3 * 4)) = BinOp(Add, Lit(2), BinOp(Mul, Lit(3), Lit(4)))
        match &flow.body[0] {
            Statement::ExprAssign(ea) => match &ea.expr {
                Expr::BinOp { op, lhs, rhs } => {
                    assert_eq!(*op, BinOp::Add);
                    assert!(matches!(**lhs, Expr::Lit(_)));
                    match &**rhs {
                        Expr::BinOp { op: inner_op, .. } => {
                            assert_eq!(*inner_op, BinOp::Mul);
                        }
                        _ => panic!("expected BinOp(Mul) on rhs"),
                    }
                }
                _ => panic!("expected BinOp"),
            },
            _ => panic!("expected ExprAssign"),
        }
    }

    #[test]
    fn expr_parens_override_precedence() {
        let src = r#"
func Calc
  take input as Foo
  emit result as Bar
  fail error as Baz
body
  x = (2 + 3) * 4
  emit x
done
"#;
        let flow = parse_runtime_flow_v1(src).expect("should parse");
        match &flow.body[0] {
            Statement::ExprAssign(ea) => {
                match &ea.expr {
                    Expr::BinOp { op, lhs, .. } => {
                        assert_eq!(*op, BinOp::Mul);
                        // lhs should be BinOp(Add, ...)
                        match &**lhs {
                            Expr::BinOp { op: inner_op, .. } => {
                                assert_eq!(*inner_op, BinOp::Add);
                            }
                            _ => panic!("expected BinOp(Add) in parens"),
                        }
                    }
                    _ => panic!("expected BinOp"),
                }
            }
            _ => panic!("expected ExprAssign"),
        }
    }

    #[test]
    fn expr_unary_minus() {
        let src = r#"
func Calc
  take input as Foo
  emit result as Bar
  fail error as Baz
body
  x = -5 + 3
  emit x
done
"#;
        let flow = parse_runtime_flow_v1(src).expect("should parse");
        match &flow.body[0] {
            Statement::ExprAssign(ea) => match &ea.expr {
                Expr::BinOp { op, lhs, .. } => {
                    assert_eq!(*op, BinOp::Add);
                    assert!(matches!(
                        **lhs,
                        Expr::UnaryOp {
                            op: UnaryOp::Neg,
                            ..
                        }
                    ));
                }
                _ => panic!("expected BinOp"),
            },
            _ => panic!("expected ExprAssign"),
        }
    }

    #[test]
    fn expr_power_right_associative() {
        let src = r#"
func Calc
  take input as Foo
  emit result as Bar
  fail error as Baz
body
  x = 2 ** 3 ** 2
  emit x
done
"#;
        let flow = parse_runtime_flow_v1(src).expect("should parse");
        // Should be 2 ** (3 ** 2) due to right-assoc
        match &flow.body[0] {
            Statement::ExprAssign(ea) => match &ea.expr {
                Expr::BinOp { op, rhs, .. } => {
                    assert_eq!(*op, BinOp::Pow);
                    match &**rhs {
                        Expr::BinOp { op: inner_op, .. } => {
                            assert_eq!(*inner_op, BinOp::Pow);
                        }
                        _ => panic!("expected BinOp(Pow) on rhs"),
                    }
                }
                _ => panic!("expected BinOp"),
            },
            _ => panic!("expected ExprAssign"),
        }
    }

    #[test]
    fn expr_call_in_expression() {
        let src = r#"
func Calc
  take input as Foo
  emit result as Bar
  fail error as Baz
body
  x = math.round(a, b) + 1
  emit x
done
"#;
        let flow = parse_runtime_flow_v1(src).expect("should parse");
        // math.round(a, b) + 1 → BinOp(Add, Call, Lit)
        match &flow.body[0] {
            Statement::ExprAssign(ea) => match &ea.expr {
                Expr::BinOp { op, lhs, .. } => {
                    assert_eq!(*op, BinOp::Add);
                    assert!(matches!(**lhs, Expr::Call { .. }));
                }
                _ => panic!("expected BinOp"),
            },
            _ => panic!("expected ExprAssign (call + literal is not a simple call)"),
        }
    }

    #[test]
    fn expr_comparison_operators() {
        let src = r#"
func Calc
  take input as Foo
  emit result as Bar
  fail error as Baz
body
  x = 1 < 2
  y = 3 >= 4
  z = 5 == 5
  w = 6 != 7
  emit x
done
"#;
        let flow = parse_runtime_flow_v1(src).expect("should parse");
        match &flow.body[0] {
            Statement::ExprAssign(ea) => {
                assert!(matches!(&ea.expr, Expr::BinOp { op: BinOp::Lt, .. }));
            }
            _ => panic!("expected ExprAssign"),
        }
        match &flow.body[1] {
            Statement::ExprAssign(ea) => {
                assert!(matches!(
                    &ea.expr,
                    Expr::BinOp {
                        op: BinOp::GtEq,
                        ..
                    }
                ));
            }
            _ => panic!("expected ExprAssign"),
        }
        match &flow.body[2] {
            Statement::ExprAssign(ea) => {
                assert!(matches!(&ea.expr, Expr::BinOp { op: BinOp::Eq, .. }));
            }
            _ => panic!("expected ExprAssign"),
        }
        match &flow.body[3] {
            Statement::ExprAssign(ea) => {
                assert!(matches!(&ea.expr, Expr::BinOp { op: BinOp::Neq, .. }));
            }
            _ => panic!("expected ExprAssign"),
        }
    }

    #[test]
    fn expr_logical_operators() {
        let src = r#"
func Calc
  take input as Foo
  emit result as Bar
  fail error as Baz
body
  x = true && false || true
  emit x
done
"#;
        let flow = parse_runtime_flow_v1(src).expect("should parse");
        // Should be (true && false) || true because && binds tighter than ||
        match &flow.body[0] {
            Statement::ExprAssign(ea) => match &ea.expr {
                Expr::BinOp { op, lhs, .. } => {
                    assert_eq!(*op, BinOp::Or);
                    assert!(matches!(**lhs, Expr::BinOp { op: BinOp::And, .. }));
                }
                _ => panic!("expected BinOp(Or)"),
            },
            _ => panic!("expected ExprAssign"),
        }
    }

    #[test]
    fn extract_field_docs_from_nested_blocks() {
        let raw = r#"  Contains the result of an email check.

  docs email
    The cleaned email address.
  done

  docs valid
    Whether it passed validation.
  done
"#;
        let (md, fields) = extract_field_docs(raw);
        assert_eq!(md.trim(), "Contains the result of an email check.");
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "email");
        assert_eq!(fields[0].markdown, "The cleaned email address.");
        assert_eq!(fields[1].name, "valid");
        assert_eq!(fields[1].markdown, "Whether it passed validation.");
    }

    #[test]
    fn extract_field_docs_no_sub_blocks() {
        let raw = "  A scalar type description.\n";
        let (md, fields) = extract_field_docs(raw);
        assert_eq!(md.trim(), "A scalar type description.");
        assert!(fields.is_empty());
    }

    #[test]
    fn parse_docs_with_field_sub_blocks() {
        let src = r#"
docs MyType
  Top-level description.

  docs name
    The user name.
  done

  docs age
    The user age.
  done
done

type MyType
  name text
  age long
done
"#;
        let module = parse_module_v1(src).expect("parse");
        match &module.decls[0] {
            TopDecl::Docs(d) => {
                assert_eq!(d.name, "MyType");
                assert!(d.markdown.contains("Top-level description."));
                assert!(!d.markdown.contains("docs name"));
                assert_eq!(d.field_docs.len(), 2);
                assert_eq!(d.field_docs[0].name, "name");
                assert_eq!(d.field_docs[0].markdown, "The user name.");
                assert_eq!(d.field_docs[1].name, "age");
                assert_eq!(d.field_docs[1].markdown, "The user age.");
            }
            _ => panic!("expected docs decl"),
        }
    }

    #[test]
    fn parse_ternary_expression() {
        let source = r#"
func Decide
  take flag as Bool
  emit result as Text
  fail error as Text
body
  x = flag ? "yes" : "no"
  emit x
done
"#;
        let flow = parse_runtime_flow_v1(source).unwrap();
        assert_eq!(flow.body.len(), 2);
        match &flow.body[0] {
            Statement::ExprAssign(ea) => {
                assert_eq!(ea.bind, "x");
                assert!(matches!(&ea.expr, Expr::Ternary { .. }));
            }
            _ => panic!("expected ExprAssign with Ternary"),
        }
    }

    #[test]
    fn parse_list_literal() {
        let source = r#"
func Build
  take a as Long
  emit result as List
  fail error as Text
body
  items = [1, 2, 3]
  emit items
done
"#;
        let flow = parse_runtime_flow_v1(source).unwrap();
        match &flow.body[0] {
            Statement::ExprAssign(ea) => {
                assert_eq!(ea.bind, "items");
                match &ea.expr {
                    Expr::ListLit(items) => assert_eq!(items.len(), 3),
                    _ => panic!("expected ListLit"),
                }
            }
            _ => panic!("expected ExprAssign"),
        }
    }

    #[test]
    fn parse_empty_list_literal() {
        let source = r#"
func Build
  take a as Long
  emit result as List
  fail error as Text
body
  items = []
  emit items
done
"#;
        let flow = parse_runtime_flow_v1(source).unwrap();
        match &flow.body[0] {
            Statement::ExprAssign(ea) => match &ea.expr {
                Expr::ListLit(items) => assert_eq!(items.len(), 0),
                _ => panic!("expected ListLit"),
            },
            _ => panic!("expected ExprAssign"),
        }
    }

    #[test]
    fn parse_dict_literal() {
        let source = r#"
func Build
  take a as Long
  emit result as Dict
  fail error as Text
body
  obj = {name: "test", count: 42}
  emit obj
done
"#;
        let flow = parse_runtime_flow_v1(source).unwrap();
        match &flow.body[0] {
            Statement::ExprAssign(ea) => {
                assert_eq!(ea.bind, "obj");
                match &ea.expr {
                    Expr::DictLit(pairs) => {
                        assert_eq!(pairs.len(), 2);
                        assert_eq!(pairs[0].0, "name");
                        assert_eq!(pairs[1].0, "count");
                    }
                    _ => panic!("expected DictLit"),
                }
            }
            _ => panic!("expected ExprAssign"),
        }
    }

    #[test]
    fn parse_empty_dict_literal() {
        let source = r#"
func Build
  take a as Long
  emit result as Dict
  fail error as Text
body
  obj = {}
  emit obj
done
"#;
        let flow = parse_runtime_flow_v1(source).unwrap();
        match &flow.body[0] {
            Statement::ExprAssign(ea) => match &ea.expr {
                Expr::DictLit(pairs) => assert_eq!(pairs.len(), 0),
                _ => panic!("expected DictLit"),
            },
            _ => panic!("expected ExprAssign"),
        }
    }

    #[test]
    fn parse_bare_loop_with_break() {
        let source = r#"
func Counter
  take limit as Long
  emit result as Long
  fail error as Text
body
  x = 0
  loop
    x = x + 1
    done_flag = x >= limit
    case done_flag
      when true
        break
    done
  done
  emit x
done
"#;
        let flow = parse_runtime_flow_v1(source).unwrap();
        assert_eq!(flow.body.len(), 3); // x=0, bare loop, emit
        assert!(matches!(&flow.body[1], Statement::BareLoop(_)));
    }

    #[test]
    fn parse_list_literal_with_variables() {
        let source = r#"
func Build
  take a as Long
  emit result as List
  fail error as Text
body
  x = 1
  y = 2
  items = [x, y, 3]
  emit items
done
"#;
        let flow = parse_runtime_flow_v1(source).unwrap();
        match &flow.body[2] {
            Statement::ExprAssign(ea) => {
                assert_eq!(ea.bind, "items");
                match &ea.expr {
                    Expr::ListLit(items) => {
                        assert_eq!(items.len(), 3);
                        assert!(matches!(&items[0], Expr::Var(v) if v == "x"));
                        assert!(matches!(&items[1], Expr::Var(v) if v == "y"));
                    }
                    _ => panic!("expected ListLit"),
                }
            }
            _ => panic!("expected ExprAssign"),
        }
    }

    // --- state/on/send-nowait tests ---

    #[test]
    fn parses_state_decl_in_flow() {
        let src = r#"
flow Server
  take port as Long
  emit result as ServerResult
  fail error as ServerError
body
  state conn = db.open(port)
  step handler.Run(conn to :conn) then
    next :result to res
    emit res to :result
  done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        assert_eq!(graph.body.len(), 2);
        match &graph.body[0] {
            FlowStatement::State(s) => {
                assert_eq!(s.bind, "conn");
                assert_eq!(s.callee, "db.open");
                assert_eq!(
                    s.args,
                    vec![Arg::Var {
                        var: "port".to_string()
                    }]
                );
            }
            _ => panic!("expected State"),
        }
    }

    #[test]
    fn parses_state_literal_string() {
        let src = r#"
flow Hello
body
  state name = "bob"
  step Display(name to :name) done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        match &graph.body[0] {
            FlowStatement::State(s) => {
                assert_eq!(s.bind, "name");
                assert!(s.value.is_some());
                assert!(matches!(&s.value, Some(Expr::Lit(v)) if v == "bob"));
            }
            _ => panic!("expected State"),
        }
    }

    #[test]
    fn parses_state_literal_number() {
        let src = r#"
flow Counter
body
  state count = 42
  step Display(count to :count) done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        match &graph.body[0] {
            FlowStatement::State(s) => {
                assert_eq!(s.bind, "count");
                assert!(s.value.is_some());
                assert!(matches!(&s.value, Some(Expr::Lit(v)) if v == 42));
            }
            _ => panic!("expected State"),
        }
    }

    #[test]
    fn parses_state_literal_list() {
        let src = r#"
flow Users
body
  state users = ["alice", "bob", "charlie"]
  step Display(users to :users) done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        match &graph.body[0] {
            FlowStatement::State(s) => {
                assert_eq!(s.bind, "users");
                match &s.value {
                    Some(Expr::ListLit(items)) => assert_eq!(items.len(), 3),
                    _ => panic!("expected ListLit"),
                }
            }
            _ => panic!("expected State"),
        }
    }

    #[test]
    fn parses_state_literal_dict() {
        let src = r#"
flow Config
body
  state cfg = {host: "localhost", port: 8080}
  step Display(cfg to :cfg) done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        match &graph.body[0] {
            FlowStatement::State(s) => {
                assert_eq!(s.bind, "cfg");
                match &s.value {
                    Some(Expr::DictLit(pairs)) => {
                        assert_eq!(pairs.len(), 2);
                        assert_eq!(pairs[0].0, "host");
                        assert_eq!(pairs[1].0, "port");
                    }
                    _ => panic!("expected DictLit"),
                }
            }
            _ => panic!("expected State"),
        }
    }

    #[test]
    fn parses_state_literal_bool() {
        let src = r#"
flow Toggle
body
  state active = true
  step Display(active to :active) done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        match &graph.body[0] {
            FlowStatement::State(s) => {
                assert_eq!(s.bind, "active");
                assert!(matches!(&s.value, Some(Expr::Lit(v)) if v == true));
            }
            _ => panic!("expected State"),
        }
    }

    #[test]
    fn state_op_call_still_works() {
        // Ensure the existing op-call syntax is unaffected
        let src = r#"
flow Server
  take port as Long
  emit result as ServerResult
  fail error as ServerError
body
  state conn = db.open(port)
  step handler.Run(conn to :conn) then
    next :result to res
    emit res to :result
  done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        match &graph.body[0] {
            FlowStatement::State(s) => {
                assert_eq!(s.bind, "conn");
                assert_eq!(s.callee, "db.open");
                assert!(s.value.is_none());
            }
            _ => panic!("expected State"),
        }
    }

    #[test]
    fn parses_send_nowait_in_flow() {
        let src = r#"
flow Server
  take port as Long
  emit result as ServerResult
  fail error as ServerError
body
  state conn = db.open(port)
  send nowait workflow.RunJobLoop(conn)
  step handler.Run(conn to :conn) then
    next :result to res
    emit res to :result
  done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        assert_eq!(graph.body.len(), 3);
        match &graph.body[1] {
            FlowStatement::SendNowait(sn) => {
                assert_eq!(sn.target, "workflow.RunJobLoop");
                assert_eq!(sn.args, vec!["conn"]);
            }
            _ => panic!("expected SendNowait"),
        }
    }

    #[test]
    fn parses_log_in_flow() {
        let src = r#"
flow Server
  take port as Long
  emit result as ServerResult
  fail error as ServerError
body
  state conn = db.open(port)
  log "server started", conn
  step handler.Run(conn to :conn) then
    next :result to res
    emit res to :result
  done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        assert_eq!(graph.body.len(), 3);
        match &graph.body[1] {
            FlowStatement::Log(log) => {
                assert_eq!(log.args.len(), 2);
                assert_eq!(
                    log.args[0],
                    Arg::Lit {
                        lit: json!("server started")
                    }
                );
                assert_eq!(
                    log.args[1],
                    Arg::Var {
                        var: "conn".to_string()
                    }
                );
            }
            _ => panic!("expected Log"),
        }
    }

    #[test]
    fn lower_log_to_node() {
        let src = r#"
flow Server
  take port as Long
  emit result as ServerResult
  fail error as ServerError
body
  log "started"
  step handler.Run(port to :port) then
    next :result to res
    emit res to :result
  done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        let flow = lower_flow_graph_to_flow(&graph).expect("lowering should succeed");

        // log node + step node + emit
        match &flow.body[0] {
            Statement::Node(n) => {
                assert_eq!(n.op, "log.info");
                assert!(n.bind.starts_with("_log_"));
                assert_eq!(n.args.len(), 1);
                assert_eq!(
                    n.args[0],
                    Arg::Lit {
                        lit: json!("started")
                    }
                );
            }
            _ => panic!("expected Node for log"),
        }
    }

    #[test]
    fn lower_state_to_node() {
        let src = r#"
flow Server
  take port as Long
  emit result as ServerResult
  fail error as ServerError
body
  state conn = db.open(port)
  step handler.Run(conn to :conn) then
    next :result to res
    emit res to :result
  done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        let flow = lower_flow_graph_to_flow(&graph).expect("lowering should succeed");

        assert_eq!(flow.body.len(), 3); // state node + step node + emit
        match &flow.body[0] {
            Statement::Node(n) => {
                assert_eq!(n.bind, "conn");
                assert_eq!(n.op, "db.open");
                assert_eq!(n.args.len(), 1);
                match &n.args[0] {
                    Arg::Var { var } => assert_eq!(var, "port"),
                    _ => panic!("expected Arg::Var"),
                }
            }
            _ => panic!("expected Statement::Node for state"),
        }
    }

    #[test]
    fn parse_state_with_literals() {
        let src = r#"
flow Server
  emit result as ServerResult
  fail error as ServerError
body
  state conn = db.open("factory.db")
  state srv = http.server.listen(8080)
  step handler.Run(conn to :conn) then
    next :result to res
    emit res to :result
  done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        let flow = lower_flow_graph_to_flow(&graph).expect("lowering should succeed");

        // state conn node
        match &flow.body[0] {
            Statement::Node(n) => {
                assert_eq!(n.bind, "conn");
                assert_eq!(n.op, "db.open");
                assert_eq!(n.args.len(), 1);
                match &n.args[0] {
                    Arg::Lit { lit } => assert_eq!(lit, "factory.db"),
                    _ => panic!("expected Arg::Lit for string literal"),
                }
            }
            _ => panic!("expected Statement::Node for state conn"),
        }
        // state srv node
        match &flow.body[1] {
            Statement::Node(n) => {
                assert_eq!(n.bind, "srv");
                assert_eq!(n.op, "http.server.listen");
                assert_eq!(n.args.len(), 1);
                match &n.args[0] {
                    Arg::Lit { lit } => assert_eq!(lit, &serde_json::json!(8080)),
                    _ => panic!("expected Arg::Lit for number literal"),
                }
            }
            _ => panic!("expected Statement::Node for state srv"),
        }
    }

    #[test]
    fn lower_send_nowait_to_statement() {
        let src = r#"
flow Server
  take port as Long
  emit result as ServerResult
  fail error as ServerError
body
  send nowait workflow.RunJobLoop(port)
  step handler.Run(port to :port) then
    next :result to res
    emit res to :result
  done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        let flow = lower_flow_graph_to_flow(&graph).expect("lowering should succeed");

        match &flow.body[0] {
            Statement::SendNowait(sn) => {
                assert_eq!(sn.target, "workflow.RunJobLoop");
                assert_eq!(sn.args.len(), 1);
                assert!(matches!(&sn.args[0], Expr::Var(v) if v == "port"));
            }
            _ => panic!("expected SendNowait"),
        }
    }

    #[test]
    fn parse_step_with_literal_port_mappings() {
        let src = r#"
flow TestFlow
  take req as dict
  emit result as bool
  fail error as text
body
  step obj.get(req to :dict, "handler" to :key) then
    next :result to handler
  done
  step http.server.respond(conn_id to :id, 200 to :status, hdrs to :hdrs, html to :body) done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        assert_eq!(graph.body.len(), 2);

        // First step: obj.get with string literal "handler"
        match &graph.body[0] {
            FlowStatement::Step(s) => {
                assert_eq!(s.callee, "obj.get");
                assert_eq!(s.inputs.len(), 2);
                assert_eq!(s.inputs[0].port, "dict");
                assert!(matches!(&s.inputs[0].value, Arg::Var { var } if var == "req"));
                assert_eq!(s.inputs[1].port, "key");
                assert!(matches!(&s.inputs[1].value, Arg::Lit { lit } if lit == "handler"));
            }
            _ => panic!("expected step"),
        }

        // Second step: http.server.respond with number literal 200
        match &graph.body[1] {
            FlowStatement::Step(s) => {
                assert_eq!(s.callee, "http.server.respond");
                assert_eq!(s.inputs.len(), 4);
                assert!(matches!(&s.inputs[0].value, Arg::Var { var } if var == "conn_id"));
                assert_eq!(s.inputs[1].port, "status");
                assert!(
                    matches!(&s.inputs[1].value, Arg::Lit { lit } if lit == &serde_json::json!(200))
                );
                assert!(matches!(&s.inputs[2].value, Arg::Var { var } if var == "hdrs"));
                assert!(matches!(&s.inputs[3].value, Arg::Var { var } if var == "html"));
            }
            _ => panic!("expected step"),
        }

        // Lower to Flow and verify literal args pass through
        let flow = lower_flow_graph_to_flow(&graph).expect("lowering should succeed");
        match &flow.body[0] {
            Statement::Node(n) => {
                assert_eq!(n.op, "obj.get");
                assert_eq!(n.args.len(), 2);
                assert!(matches!(&n.args[0], Arg::Var { var } if var == "req"));
                assert!(matches!(&n.args[1], Arg::Lit { lit } if lit == "handler"));
            }
            _ => panic!("expected Node"),
        }
        match &flow.body[1] {
            Statement::Node(n) => {
                assert_eq!(n.op, "http.server.respond");
                assert!(matches!(&n.args[1], Arg::Lit { lit } if lit == &serde_json::json!(200)));
            }
            _ => panic!("expected Node"),
        }
    }

    #[test]
    fn parses_v2_func_with_return_and_fail() {
        let src = r#"
docs Compute
  A v2 func.
done

func Compute
  take x as long
  return dict
  fail text
body
  result = obj.new()
  return result
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        match &module.decls[1] {
            TopDecl::Func(f) => {
                assert_eq!(f.name, "Compute");
                assert_eq!(f.takes.len(), 1);
                assert!(f.emits.is_empty(), "v2 func should have no named emits");
                assert!(f.fails.is_empty(), "v2 func should have no named fails");
                assert_eq!(f.return_type.as_deref(), Some("dict"));
                assert_eq!(f.fail_type.as_deref(), Some("text"));
            }
            _ => panic!("expected func decl"),
        }
    }

    #[test]
    fn v2_func_compiles_to_runtime_flow() {
        let src = r#"
docs Compute
  A v2 func.
done

func Compute
  take x as long
  return dict
  fail text
body
  result = obj.new()
  return result
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let funcs: Vec<&FuncDecl> = module
            .decls
            .iter()
            .filter_map(|d| match d {
                TopDecl::Func(f) => Some(f),
                _ => None,
            })
            .collect();
        let flow = parse_runtime_func_decl_v1(funcs[0]).expect("should compile v2 func");
        assert_eq!(flow.name, "Compute");
        assert_eq!(flow.inputs.len(), 1);
        assert_eq!(flow.outputs.len(), 2);
        assert_eq!(flow.outputs[0].name, "_return");
        assert_eq!(flow.outputs[0].type_name, "dict");
        assert_eq!(flow.outputs[1].name, "_fail");
        assert_eq!(flow.outputs[1].type_name, "text");
        // Body should have 2 statements: assignment + emit (from return)
        assert_eq!(flow.body.len(), 2);
        match &flow.body[1] {
            Statement::Emit(e) => {
                assert_eq!(e.output, "_return");
                assert!(matches!(&e.value_expr, Expr::Var(v) if v == "result"));
            }
            other => panic!("expected Emit from return, got: {other:?}"),
        }
    }

    #[test]
    fn v1_func_still_works_with_named_ports() {
        let src = r#"
docs Legacy
  A v1 func.
done

func Legacy
  take x as text
  emit result as dict
  fail error as text
body
  r = obj.new()
  emit r
done
"#;
        let module = parse_module_v1(src).expect("parse");
        match &module.decls[1] {
            TopDecl::Func(f) => {
                assert_eq!(f.emits.len(), 1);
                assert_eq!(f.fails.len(), 1);
                assert!(f.return_type.is_none());
                assert!(f.fail_type.is_none());
            }
            _ => panic!("expected func"),
        }
    }

    #[test]
    fn v2_fail_disambiguates_from_v1() {
        // v1 fail with `as` keyword
        let src_v1 = r#"
func V1Func
  take x as text
  emit result as dict
  fail error as text
body
  emit x
done
"#;
        let mod_v1 = parse_module_v1(src_v1).expect("v1 parse");
        match &mod_v1.decls[0] {
            TopDecl::Func(f) => {
                assert_eq!(f.fails.len(), 1);
                assert_eq!(f.fails[0].name, "error");
                assert_eq!(f.fails[0].type_name, "text");
                assert!(f.fail_type.is_none());
            }
            _ => panic!("expected func"),
        }

        // v2 fail without `as` keyword
        let src_v2 = r#"
func V2Func
  take x as text
  return dict
  fail text
body
  r = obj.new()
  return r
done
"#;
        let mod_v2 = parse_module_v1(src_v2).expect("v2 parse");
        match &mod_v2.decls[0] {
            TopDecl::Func(f) => {
                assert!(f.fails.is_empty());
                assert_eq!(f.fail_type.as_deref(), Some("text"));
            }
            _ => panic!("expected func"),
        }
    }

    #[test]
    fn parses_source_declaration() {
        let src = r#"
docs HTTPRequests
  Accepts HTTP connections and emits request dicts.
done

source HTTPRequests
  take port as long
  emit req as dict
  fail error as text
body
  srv = http.server.listen(port)
  on :request from http.server.accept(srv) to req
    emit req
  done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        assert_eq!(module.decls.len(), 2);
        match &module.decls[0] {
            TopDecl::Docs(d) => assert_eq!(d.name, "HTTPRequests"),
            _ => panic!("expected docs decl"),
        }
        match &module.decls[1] {
            TopDecl::Source(f) => {
                assert_eq!(f.name, "HTTPRequests");
                assert_eq!(f.takes.len(), 1);
                assert_eq!(f.takes[0].name, "port");
                assert_eq!(f.takes[0].type_name, "long");
                assert_eq!(f.emits.len(), 1);
                assert_eq!(f.emits[0].name, "req");
                assert_eq!(f.fails.len(), 1);
                assert!(!f.body_text.is_empty());
            }
            _ => panic!("expected source decl"),
        }
    }

    #[test]
    fn parses_source_with_on_block() {
        let src = r#"
source HTTPRequests
  take port as long
  emit req as dict
  fail error as text
body
  srv = http.server.listen(port)
  on :request from http.server.accept(srv) to req
    emit req
  done
done
"#;
        let flow = parse_runtime_flow_v1(src).expect("should parse");
        assert_eq!(flow.body.len(), 2); // srv = ..., on block
        match &flow.body[1] {
            Statement::On(on_block) => {
                assert_eq!(on_block.event_tag, "request");
                assert_eq!(on_block.source_op, "http.server.accept");
                assert_eq!(on_block.source_args.len(), 1);
                assert_eq!(on_block.bind, "req");
                assert_eq!(on_block.body.len(), 1);
            }
            other => panic!("expected On, got {:?}", other),
        }
    }

    #[test]
    fn parses_source_with_on_transform() {
        let src = r#"
source Commands
  emit cmd as text
  fail error as text
body
  on :input from term.prompt("docs> ") to raw
    trimmed = str.trim(raw)
    emit trimmed
  done
done
"#;
        let flow = parse_runtime_flow_v1(src).expect("should parse");
        assert_eq!(flow.body.len(), 1); // just the on block
        match &flow.body[0] {
            Statement::On(on_block) => {
                assert_eq!(on_block.event_tag, "input");
                assert_eq!(on_block.source_op, "term.prompt");
                assert_eq!(on_block.bind, "raw");
                assert_eq!(on_block.body.len(), 2); // trimmed = ..., emit trimmed
            }
            other => panic!("expected On, got {:?}", other),
        }
    }

    #[test]
    fn parses_source_with_body_loop() {
        let src = r#"
source Numbers
  take count as long
  emit num as long
body
  items = list.range(1, count)
  loop items as n
    emit n
  done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        assert_eq!(module.decls.len(), 1);
        match &module.decls[0] {
            TopDecl::Source(f) => {
                assert_eq!(f.name, "Numbers");
                assert!(
                    f.body_text.contains("list.range(1, count)"),
                    "body: {}",
                    f.body_text
                );
                assert!(
                    f.body_text.contains("loop items as n"),
                    "body: {}",
                    f.body_text
                );
            }
            _ => panic!("expected source decl"),
        }
    }

    #[test]
    fn parses_simple_if_else() {
        let src = r#"
func Greet
  take x as long
  emit r as text
  fail e as text
body
  if x > 0
    r = "positive"
    emit r
  else
    r = "non-positive"
    emit r
  done
done
"#;
        let flow = parse_runtime_flow_v1(src).expect("should parse");
        // Desugared to a CaseBlock
        assert_eq!(flow.body.len(), 1);
        match &flow.body[0] {
            Statement::Case(cb) => {
                assert_eq!(cb.arms.len(), 1);
                assert_eq!(cb.arms[0].body.len(), 2); // r = "positive", emit r
                assert_eq!(cb.else_body.len(), 2); // r = "non-positive", emit r
            }
            other => panic!("expected Case, got {:?}", other),
        }
    }

    #[test]
    fn parses_if_else_if_else() {
        let src = r#"
func Check
  take x as long
  emit r as text
  fail e as text
body
  if x > 10
    r = "big"
    emit r
  else if x > 0
    r = "small"
    emit r
  else
    r = "zero"
    emit r
  done
done
"#;
        let flow = parse_runtime_flow_v1(src).expect("should parse");
        assert_eq!(flow.body.len(), 1);
        match &flow.body[0] {
            Statement::Case(cb) => {
                assert_eq!(cb.arms.len(), 1);
                // else_body is a nested CaseBlock (the "else if")
                assert_eq!(cb.else_body.len(), 1);
                match &cb.else_body[0] {
                    Statement::Case(inner) => {
                        assert_eq!(inner.arms.len(), 1);
                        assert_eq!(inner.else_body.len(), 2); // r = "zero", emit r
                    }
                    other => panic!("expected nested Case, got {:?}", other),
                }
            }
            other => panic!("expected Case, got {:?}", other),
        }
    }

    #[test]
    fn parses_if_without_else() {
        let src = r#"
func Maybe
  take x as long
  emit r as text
  fail e as text
body
  r = "default"
  if x > 0
    r = "yes"
  done
  emit r
done
"#;
        let flow = parse_runtime_flow_v1(src).expect("should parse");
        assert_eq!(flow.body.len(), 3); // r = "default", if block, emit r
        match &flow.body[1] {
            Statement::Case(cb) => {
                assert_eq!(cb.arms.len(), 1);
                assert!(cb.else_body.is_empty());
            }
            other => panic!("expected Case, got {:?}", other),
        }
    }

    // --- Branch tests ---

    #[test]
    fn parses_guarded_branch() {
        let src = r#"
flow TestFlow
    take input as Foo
    emit result as Bar
    fail error as Baz
body
    step sources.RandomNum() then
        next :num to raw
    done
    branch when raw > 50.0
        step lib.Square(raw to :num) then
            next :result to processed
        done
        step sinks.Print(processed to :line) done
    done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        assert_eq!(graph.body.len(), 2); // step + branch
        match &graph.body[1] {
            FlowStatement::Branch(b) => {
                assert!(b.condition.is_some());
                assert_eq!(b.body.len(), 2); // two steps inside branch
            }
            other => panic!("expected Branch, got {:?}", other),
        }
    }

    #[test]
    fn parses_guarded_branch_shorthand() {
        let src = r#"
flow TestFlow
    take input as Foo
    emit result as Bar
    fail error as Baz
body
    step sources.RandomNum() then
        next :num to raw
    done
    branch raw > 50.0
        step lib.Square(raw to :num) then
            next :result to processed
        done
        step sinks.Print(processed to :line) done
    done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        assert_eq!(graph.body.len(), 2); // step + branch
        match &graph.body[1] {
            FlowStatement::Branch(b) => {
                assert!(b.condition.is_some());
                assert_eq!(b.body.len(), 2); // two steps inside branch
            }
            other => panic!("expected Branch, got {:?}", other),
        }
    }

    #[test]
    fn parses_unguarded_branch() {
        let src = r#"
flow TestFlow
    take input as Foo
    emit result as Bar
    fail error as Baz
body
    step sources.RandomNum() then
        next :num to raw
    done
    branch
        step lib.Double(raw to :num) then
            next :result to doubled
        done
        step sinks.Print(doubled to :line) done
    done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        assert_eq!(graph.body.len(), 2); // step + branch
        match &graph.body[1] {
            FlowStatement::Branch(b) => {
                assert!(b.condition.is_none());
                assert_eq!(b.body.len(), 2);
            }
            other => panic!("expected Branch, got {:?}", other),
        }
    }

    #[test]
    fn parses_choose_with_fallback() {
        let src = r#"
flow TestFlow
    take req as dict
    emit result as text
    fail error as text
body
    choose
        branch route.get("/", req)
            step routes.Home() done
        done
        branch route.get("/about", req)
            step routes.About() done
        done
        branch
            step routes.NotFound() done
        done
    done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        assert_eq!(graph.body.len(), 1);
        match &graph.body[0] {
            FlowStatement::Choose(choose) => {
                assert_eq!(choose.branches.len(), 3);
                assert!(choose.branches[0].condition.is_some());
                assert!(choose.branches[1].condition.is_some());
                assert!(choose.branches[2].condition.is_none());
            }
            other => panic!("expected Choose, got {:?}", other),
        }
    }

    #[test]
    fn parses_nested_choose_inside_branch() {
        let src = r#"
flow TestFlow
    take req as dict
    emit result as text
    fail error as text
body
    branch route.get("/", req)
        choose
            branch true
                step routes.Home() done
            done
        done
    done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        assert_eq!(graph.body.len(), 1);
        match &graph.body[0] {
            FlowStatement::Branch(branch) => {
                assert_eq!(branch.body.len(), 1);
                assert!(matches!(branch.body[0], FlowStatement::Choose(_)));
            }
            other => panic!("expected Branch, got {:?}", other),
        }
    }

    #[test]
    fn choose_requires_at_least_one_branch() {
        let src = r#"
flow TestFlow
    emit result as text
    fail error as text
body
    choose
    done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let err = parse_flow_graph_from_module_v1(&module)
            .expect_err("choose without branches must fail");
        assert!(err.contains("choose block requires at least one branch"));
    }

    #[test]
    fn choose_requires_default_branch_last() {
        let src = r#"
flow TestFlow
    take req as dict
    emit result as text
    fail error as text
body
    choose
        branch
            step routes.NotFound() done
        done
        branch route.get("/", req)
            step routes.Home() done
        done
    done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let err = parse_flow_graph_from_module_v1(&module)
            .expect_err("default branch not last must fail");
        assert!(err.contains("bare `branch` must be last in choose block"));
    }

    #[test]
    fn lower_choose_matches_consecutive_branch_chain() {
        let src_choose = r#"
flow TestFlow
    take req as dict
    emit result as text
    fail error as text
body
    choose
        branch route.get("/", req)
            step routes.Home() then
                next :result to out
                emit out to :result
            done
        done
        branch route.get("/about", req)
            step routes.About() then
                next :result to out
                emit out to :result
            done
        done
        branch
            step routes.NotFound() then
                next :result to out
                emit out to :result
            done
        done
    done
done
"#;

        let src_branch_chain = r#"
flow TestFlow
    take req as dict
    emit result as text
    fail error as text
body
    branch when route.get("/", req)
        step routes.Home() then
            next :result to out
            emit out to :result
        done
    done
    branch when route.get("/about", req)
        step routes.About() then
            next :result to out
            emit out to :result
        done
    done
    branch
        step routes.NotFound() then
            next :result to out
            emit out to :result
        done
    done
done
"#;

        let choose_module = parse_module_v1(src_choose).expect("choose module should parse");
        let choose_graph =
            parse_flow_graph_from_module_v1(&choose_module).expect("choose graph should parse");
        let choose_flow =
            lower_flow_graph_to_flow(&choose_graph).expect("choose lower should succeed");

        let chain_module = parse_module_v1(src_branch_chain).expect("chain module should parse");
        let chain_graph =
            parse_flow_graph_from_module_v1(&chain_module).expect("chain graph should parse");
        let chain_flow =
            lower_flow_graph_to_flow(&chain_graph).expect("chain lower should succeed");

        let choose_body = serde_json::to_value(&choose_flow.body).expect("serialize choose body");
        let chain_body = serde_json::to_value(&chain_flow.body).expect("serialize chain body");
        assert_eq!(choose_body, chain_body);
    }

    #[test]
    fn lower_guarded_branch_to_case() {
        let src = r#"
flow TestFlow
    take input as Foo
    emit result as Bar
    fail error as Baz
body
    step sources.RandomNum() then
        next :num to raw
    done
    branch when raw > 50.0
        step lib.Square(raw to :num) then
            next :result to processed
            emit processed to :result
        done
    done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        let flow = lower_flow_graph_to_flow(&graph).expect("lowering should succeed");

        // body: node(RandomNum) + case(raw > 50.0)
        assert_eq!(flow.body.len(), 2);
        match &flow.body[1] {
            Statement::Case(c) => {
                assert_eq!(c.arms.len(), 1);
                assert!(matches!(&c.arms[0].pattern, Pattern::Lit(v) if v == true));
                assert!(c.else_body.is_empty());
                // Inside arm: node(Square) + emit
                assert_eq!(c.arms[0].body.len(), 2);
            }
            other => panic!("expected Statement::Case, got {:?}", other),
        }
    }

    #[test]
    fn lower_unguarded_branch_inlines_body() {
        let src = r#"
flow TestFlow
    take input as Foo
    emit result as Bar
    fail error as Baz
body
    step sources.RandomNum() then
        next :num to raw
    done
    branch
        step lib.Double(raw to :num) then
            next :result to doubled
            emit doubled to :result
        done
    done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        let flow = lower_flow_graph_to_flow(&graph).expect("lowering should succeed");

        // body: node(RandomNum) + node(Double) + emit — inlined, no Case wrapper
        assert_eq!(flow.body.len(), 3);
        assert!(matches!(flow.body[0], Statement::Node(_)));
        assert!(matches!(flow.body[1], Statement::Node(_)));
        assert!(matches!(flow.body[2], Statement::Emit(_)));
    }

    #[test]
    fn parses_nested_branches() {
        let src = r#"
flow TestFlow
    take input as Foo
    emit result as Bar
    fail error as Baz
body
    step sources.RandomNum() then
        next :num to raw
    done
    branch when raw > 50.0
        branch when raw > 90.0
            step sinks.Print(raw to :line) done
        done
    done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        assert_eq!(graph.body.len(), 2); // step + outer branch
        match &graph.body[1] {
            FlowStatement::Branch(outer) => {
                assert!(outer.condition.is_some());
                assert_eq!(outer.body.len(), 1); // inner branch
                match &outer.body[0] {
                    FlowStatement::Branch(inner) => {
                        assert!(inner.condition.is_some());
                        assert_eq!(inner.body.len(), 1); // step
                    }
                    other => panic!("expected inner Branch, got {:?}", other),
                }
            }
            other => panic!("expected outer Branch, got {:?}", other),
        }
    }

    #[test]
    fn branch_when_expression_parses_correctly() {
        let src = r#"
flow TestFlow
    take input as Foo
    emit result as Bar
    fail error as Baz
body
    step sources.RandomNum() then
        next :num to raw
    done
    branch when raw > 50.0
        step sinks.Print(raw to :line) done
    done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        match &graph.body[1] {
            FlowStatement::Branch(b) => {
                let cond = b.condition.as_ref().unwrap();
                // Should be BinOp { Gt, Var("raw"), Lit(50.0) }
                match cond {
                    Expr::BinOp { op, lhs, rhs } => {
                        assert_eq!(*op, BinOp::Gt);
                        assert!(matches!(**lhs, Expr::Var(ref v) if v == "raw"));
                        assert!(matches!(**rhs, Expr::Lit(ref v) if v.as_f64() == Some(50.0)));
                    }
                    other => panic!("expected BinOp(Gt), got {:?}", other),
                }
            }
            other => panic!("expected Branch, got {:?}", other),
        }
    }

    #[test]
    fn emit_accepts_string_literal() {
        let src = r#"
docs Hello
  A test func.
done

func Hello
  emit result as text
  fail error as text
body
  emit "hello"
done

test Hello
  ok = true
  must ok == true
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let flow = parse_runtime_func_decl_v1(match &module.decls[1] {
            TopDecl::Func(f) => f,
            _ => panic!("expected func"),
        })
        .expect("compile");
        match &flow.body[0] {
            Statement::Emit(e) => {
                assert_eq!(e.output, "result");
                assert!(matches!(&e.value_expr, Expr::Lit(v) if v == "hello"));
            }
            other => panic!("expected Emit, got: {other:?}"),
        }
    }

    #[test]
    fn emit_accepts_interpolation() {
        let src = r#"
docs Greet
  A test func.
done

func Greet
  take name as text
  emit result as text
  fail error as text
body
  emit "hello #{name}!"
done

test Greet
  ok = true
  must ok == true
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let flow = parse_runtime_func_decl_v1(match &module.decls[1] {
            TopDecl::Func(f) => f,
            _ => panic!("expected func"),
        })
        .expect("compile");
        match &flow.body[0] {
            Statement::Emit(e) => {
                assert_eq!(e.output, "result");
                assert!(matches!(&e.value_expr, Expr::Interp(_)));
            }
            other => panic!("expected Emit, got: {other:?}"),
        }
    }

    #[test]
    fn fail_accepts_dotted_path() {
        let src = r#"
docs RunCmd
  A test func.
done

func RunCmd
  emit result as text
  fail error as text
body
  out = exec.run("echo", ["hi"])
  fail out.stderr
done

test RunCmd
  ok = true
  must ok == true
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let flow = parse_runtime_func_decl_v1(match &module.decls[1] {
            TopDecl::Func(f) => f,
            _ => panic!("expected func"),
        })
        .expect("compile");
        match &flow.body[1] {
            Statement::Emit(e) => {
                assert_eq!(e.output, "error");
                assert!(matches!(&e.value_expr, Expr::Var(v) if v == "out.stderr"));
            }
            other => panic!("expected Emit (fail), got: {other:?}"),
        }
    }

    #[test]
    fn fail_accepts_interpolation() {
        let src = r#"
docs ErrMsg
  A test func.
done

func ErrMsg
  take msg as text
  emit result as text
  fail error as text
body
  fail "error: #{msg}"
done

test ErrMsg
  ok = true
  must ok == true
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let flow = parse_runtime_func_decl_v1(match &module.decls[1] {
            TopDecl::Func(f) => f,
            _ => panic!("expected func"),
        })
        .expect("compile");
        match &flow.body[0] {
            Statement::Emit(e) => {
                assert_eq!(e.output, "error");
                assert!(matches!(&e.value_expr, Expr::Interp(_)));
            }
            other => panic!("expected Emit (fail), got: {other:?}"),
        }
    }

    #[test]
    fn type_annotation_parsed_on_expr_assign() {
        let src = r#"
docs Annotated
  Test type annotations.
done

func Annotated
  take x as text
  emit result as text
body
  y: long = str.len(x)
  z = str.upper(x)
  emit result
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let flow = parse_runtime_func_decl_v1(match &module.decls[1] {
            TopDecl::Func(f) => f,
            _ => panic!("expected func"),
        })
        .expect("compile");
        // y: long = str.len(x) should be a Node with type_annotation = Some("long")
        match &flow.body[0] {
            Statement::Node(n) => {
                assert_eq!(n.bind, "y");
                assert_eq!(n.type_annotation, Some("long".to_string()));
            }
            other => panic!("expected Node for 'y', got: {other:?}"),
        }
        // z = str.upper(x) should be a Node with type_annotation = None
        match &flow.body[1] {
            Statement::Node(n) => {
                assert_eq!(n.bind, "z");
                assert_eq!(n.type_annotation, None);
            }
            other => panic!("expected Node for 'z', got: {other:?}"),
        }
    }

    #[test]
    fn type_annotation_on_plain_expr() {
        let src = r#"
docs Ann2
  Test annotation on plain expression.
done

func Ann2
  take x as long
  emit result as long
body
  y: long = x + 1
  emit result
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let flow = parse_runtime_func_decl_v1(match &module.decls[1] {
            TopDecl::Func(f) => f,
            _ => panic!("expected func"),
        })
        .expect("compile");
        // y: long = x + 1 — this is an ExprAssign (expression, not op call)
        match &flow.body[0] {
            Statement::ExprAssign(ea) => {
                assert_eq!(ea.bind, "y");
                assert_eq!(ea.type_annotation, Some("long".to_string()));
            }
            other => panic!("expected ExprAssign for 'y', got: {other:?}"),
        }
    }

    // --- S4: Expanded case/when pattern tests ---

    fn parse_func_body(src: &str) -> Vec<Statement> {
        let module = parse_module_v1(src).expect("module should parse");
        let funcs: Vec<_> = module
            .decls
            .iter()
            .filter_map(|d| match d {
                TopDecl::Func(f) => Some(f),
                _ => None,
            })
            .collect();
        let flow = parse_runtime_func_decl_v1(funcs[0]).expect("should compile func");
        flow.body
    }

    #[test]
    fn parse_case_or_pattern() {
        let src = r#"
docs Test
  test
done
func Test
  take x as text
  emit result as text
body
  case x
    when "yes" | "y" | "true"
      emit "confirmed"
  done
done
"#;
        let body = parse_func_body(src);
        match &body[0] {
            Statement::Case(c) => {
                assert_eq!(c.arms.len(), 1);
                match &c.arms[0].pattern {
                    Pattern::Or(alts) => assert_eq!(alts.len(), 3),
                    other => panic!("expected Or pattern, got: {other:?}"),
                }
            }
            other => panic!("expected Case, got: {other:?}"),
        }
    }

    #[test]
    fn parse_case_range_pattern() {
        let src = r#"
docs Test
  test
done
func Test
  take score as long
  emit result as text
body
  case score
    when 90..101
      emit "A"
    when 80..90
      emit "B"
  done
done
"#;
        let body = parse_func_body(src);
        match &body[0] {
            Statement::Case(c) => {
                assert_eq!(c.arms.len(), 2);
                assert!(matches!(
                    &c.arms[0].pattern,
                    Pattern::Range { lo: 90, hi: 101 }
                ));
                assert!(matches!(
                    &c.arms[1].pattern,
                    Pattern::Range { lo: 80, hi: 90 }
                ));
            }
            other => panic!("expected Case, got: {other:?}"),
        }
    }

    #[test]
    fn parse_case_type_pattern() {
        let src = r#"
docs Test
  test
done
func Test
  take val as text
  emit result as text
body
  case val
    when :text
      emit "string"
    when :long | :real
      emit "number"
  done
done
"#;
        let body = parse_func_body(src);
        match &body[0] {
            Statement::Case(c) => {
                assert_eq!(c.arms.len(), 2);
                assert!(matches!(&c.arms[0].pattern, Pattern::Type(t) if t == "text"));
                match &c.arms[1].pattern {
                    Pattern::Or(alts) => {
                        assert_eq!(alts.len(), 2);
                        assert!(matches!(&alts[0], Pattern::Type(t) if t == "long"));
                        assert!(matches!(&alts[1], Pattern::Type(t) if t == "real"));
                    }
                    other => panic!("expected Or pattern, got: {other:?}"),
                }
            }
            other => panic!("expected Case, got: {other:?}"),
        }
    }

    #[test]
    fn parse_case_guard() {
        let src = r#"
docs Test
  test
done
func Test
  take score as long
  emit result as text
body
  case score
    when _ if score >= 70
      emit "C"
  done
done
"#;
        let body = parse_func_body(src);
        match &body[0] {
            Statement::Case(c) => {
                assert_eq!(c.arms.len(), 1);
                assert!(matches!(&c.arms[0].pattern, Pattern::Ident(s) if s == "_"));
                assert!(c.arms[0].guard.is_some());
            }
            other => panic!("expected Case, got: {other:?}"),
        }
    }

    #[test]
    fn parse_case_negative_range() {
        let src = r#"
docs Test
  test
done
func Test
  take val as long
  emit result as text
body
  case val
    when -10..0
      emit "negative"
  done
done
"#;
        let body = parse_func_body(src);
        match &body[0] {
            Statement::Case(c) => {
                assert!(matches!(
                    &c.arms[0].pattern,
                    Pattern::Range { lo: -10, hi: 0 }
                ));
            }
            other => panic!("expected Case, got: {other:?}"),
        }
    }

    #[test]
    fn parse_compound_plus_eq() {
        let src = r#"
docs Test
  test
done
func Test
  take x as Long
  emit result as Long
body
  x += 1
  emit x
done
"#;
        let body = parse_func_body(src);
        match &body[0] {
            Statement::ExprAssign(ea) => {
                assert_eq!(ea.bind, "x");
                assert!(ea.type_annotation.is_none());
                match &ea.expr {
                    Expr::BinOp { op, lhs, rhs } => {
                        assert_eq!(*op, BinOp::Add);
                        assert!(matches!(lhs.as_ref(), Expr::Var(v) if v == "x"));
                        assert!(matches!(rhs.as_ref(), Expr::Lit(v) if v == &serde_json::json!(1)));
                    }
                    other => panic!("expected BinOp, got: {other:?}"),
                }
            }
            other => panic!("expected ExprAssign, got: {other:?}"),
        }
    }

    #[test]
    fn parse_compound_all_operators() {
        for (op_str, expected_op) in [
            ("+=", BinOp::Add),
            ("-=", BinOp::Sub),
            ("*=", BinOp::Mul),
            ("/=", BinOp::Div),
            ("%=", BinOp::Mod),
        ] {
            let src = format!(
                r#"
docs Test
  test
done
func Test
  take x as Long
  emit result as Long
body
  x {} 2
  emit x
done
"#,
                op_str
            );
            let body = parse_func_body(&src);
            match &body[0] {
                Statement::ExprAssign(ea) => {
                    assert_eq!(ea.bind, "x");
                    match &ea.expr {
                        Expr::BinOp { op, .. } => {
                            assert_eq!(*op, expected_op, "failed for {op_str}")
                        }
                        other => panic!("expected BinOp for {op_str}, got: {other:?}"),
                    }
                }
                other => panic!("expected ExprAssign for {op_str}, got: {other:?}"),
            }
        }
    }

    #[test]
    fn parse_compound_rhs_is_full_expr() {
        let src = r#"
docs Test
  test
done
func Test
  take x as Long
  emit result as Long
body
  x += 2 + 3
  emit x
done
"#;
        let body = parse_func_body(src);
        match &body[0] {
            Statement::ExprAssign(ea) => {
                // Should be: x = x + (2 + 3)
                match &ea.expr {
                    Expr::BinOp { op, lhs, rhs } => {
                        assert_eq!(*op, BinOp::Add);
                        assert!(matches!(lhs.as_ref(), Expr::Var(v) if v == "x"));
                        // rhs should be BinOp(Add, 2, 3)
                        assert!(matches!(rhs.as_ref(), Expr::BinOp { op: BinOp::Add, .. }));
                    }
                    other => panic!("expected BinOp, got: {other:?}"),
                }
            }
            other => panic!("expected ExprAssign, got: {other:?}"),
        }
    }

    #[test]
    fn bare_op_call_simple_args() {
        let src = r#"
docs Greet
  A test.
done

func Greet
  take name as text
  emit result as text
  fail error as text
body
  term.print("hello")
  emit name
done

test Greet
  ok = true
  must ok == true
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let flow = parse_runtime_func_decl_v1(match &module.decls[1] {
            TopDecl::Func(f) => f,
            _ => panic!("expected func"),
        })
        .expect("compile");
        match &flow.body[0] {
            Statement::Node(n) => {
                assert_eq!(n.op, "term.print");
                assert!(n.bind.starts_with("_discard_"));
                assert_eq!(n.args.len(), 1);
            }
            other => panic!("expected Node, got: {other:?}"),
        }
    }

    #[test]
    fn bare_op_call_with_var_arg() {
        let src = r#"
docs Log
  A test.
done

func Log
  take msg as text
  emit result as text
  fail error as text
body
  log.info(msg)
  emit msg
done

test Log
  ok = true
  must ok == true
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let flow = parse_runtime_func_decl_v1(match &module.decls[1] {
            TopDecl::Func(f) => f,
            _ => panic!("expected func"),
        })
        .expect("compile");
        match &flow.body[0] {
            Statement::Node(n) => {
                assert_eq!(n.op, "log.info");
                assert!(n.bind.starts_with("_discard_"));
                assert_eq!(n.args.len(), 1);
                assert!(matches!(&n.args[0], Arg::Var { var } if var == "msg"));
            }
            other => panic!("expected Node, got: {other:?}"),
        }
    }

    #[test]
    fn bare_op_call_complex_expr_arg() {
        let src = r#"
docs Test
  A test.
done

func Test
  take x as long
  emit result as text
  fail error as text
body
  term.print(x + 1)
  emit "ok"
done

test Test
  ok = true
  must ok == true
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let flow = parse_runtime_func_decl_v1(match &module.decls[1] {
            TopDecl::Func(f) => f,
            _ => panic!("expected func"),
        })
        .expect("compile");
        match &flow.body[0] {
            Statement::ExprAssign(ea) => {
                assert!(ea.bind.starts_with("_discard_"));
                assert!(matches!(&ea.expr, Expr::Call { func, .. } if func == "term.print"));
            }
            other => panic!("expected ExprAssign for complex arg, got: {other:?}"),
        }
    }

    #[test]
    fn case_expr_basic() {
        let src = r#"
docs Convert
  A test func.
done

func Convert
  take from as text
  take value as real
  emit result as real
  fail error as text
body
  base = case from
    when "m" then value
    when "km" then value * 1000.0
  done
  emit base
done

test Convert
  it "ok"
    mock Convert => 1.0
    must true
  done
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let flow = parse_runtime_func_decl_v1(match &module.decls[1] {
            TopDecl::Func(f) => f,
            _ => panic!("expected func"),
        })
        .expect("compile");
        match &flow.body[0] {
            Statement::Case(cb) => {
                assert_eq!(cb.arms.len(), 2);
                // Each arm should have an ExprAssign with bind "base"
                match &cb.arms[0].body[0] {
                    Statement::ExprAssign(ea) => assert_eq!(ea.bind, "base"),
                    other => panic!("expected ExprAssign, got: {other:?}"),
                }
                match &cb.arms[1].body[0] {
                    Statement::ExprAssign(ea) => {
                        assert_eq!(ea.bind, "base");
                        assert!(matches!(&ea.expr, Expr::BinOp { op: BinOp::Mul, .. }));
                    }
                    other => panic!("expected ExprAssign, got: {other:?}"),
                }
            }
            other => panic!("expected Case, got: {other:?}"),
        }
    }

    #[test]
    fn case_expr_fail_in_else() {
        let src = r#"
docs Convert
  A test func.
done

func Convert
  take from as text
  take value as real
  emit result as real
  fail error as text
body
  base = case from
    when "m" then value
    else then fail "Unknown"
  done
  emit base
done

test Convert
  it "ok"
    mock Convert => 1.0
    must true
  done
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let flow = parse_runtime_func_decl_v1(match &module.decls[1] {
            TopDecl::Func(f) => f,
            _ => panic!("expected func"),
        })
        .expect("compile");
        match &flow.body[0] {
            Statement::Case(cb) => {
                assert_eq!(cb.arms.len(), 1);
                // else body should be a fail (Emit with fail output)
                match &cb.else_body[0] {
                    Statement::Emit(e) => assert_eq!(e.output, "error"),
                    other => panic!("expected Emit (fail), got: {other:?}"),
                }
            }
            other => panic!("expected Case, got: {other:?}"),
        }
    }

    #[test]
    fn case_expr_with_guard() {
        let src = r#"
docs Convert
  A test func.
done

func Convert
  take from as text
  take value as real
  take flag as bool
  emit result as real
  fail error as text
body
  base = case from
    when "m" if flag then value
    when "km" then value * 1000.0
  done
  emit base
done

test Convert
  it "ok"
    mock Convert => 1.0
    must true
  done
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let flow = parse_runtime_func_decl_v1(match &module.decls[1] {
            TopDecl::Func(f) => f,
            _ => panic!("expected func"),
        })
        .expect("compile");
        match &flow.body[0] {
            Statement::Case(cb) => {
                assert_eq!(cb.arms.len(), 2);
                assert!(cb.arms[0].guard.is_some());
                assert!(cb.arms[1].guard.is_none());
            }
            other => panic!("expected Case, got: {other:?}"),
        }
    }

    #[test]
    fn case_expr_with_return_in_else() {
        let src = r#"
docs Convert
  A test func.
done

func Convert
  take from as text
  take value as real
  emit result as real
  fail error as text
body
  base = case from
    when "m" then value
    else then return 0.0
  done
  emit base
done

test Convert
  it "ok"
    mock Convert => 1.0
    must true
  done
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let flow = parse_runtime_func_decl_v1(match &module.decls[1] {
            TopDecl::Func(f) => f,
            _ => panic!("expected func"),
        })
        .expect("compile");
        match &flow.body[0] {
            Statement::Case(cb) => {
                // else body should be a return (Emit with emit output)
                match &cb.else_body[0] {
                    Statement::Emit(e) => assert_eq!(e.output, "result"),
                    other => panic!("expected Emit (return), got: {other:?}"),
                }
            }
            other => panic!("expected Case, got: {other:?}"),
        }
    }

    // --- UI do...done block parsing tests ---

    #[test]
    fn parse_ui_block_bare_call() {
        let src = r#"
docs UiTest
  Test UI block parsing.
done

func UiTest
  take x as text
  emit result as text
body
  ui.vstack(10) do
    ui.text("hello")
  done
  emit result
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let flow = parse_runtime_func_decl_v1(match &module.decls[1] {
            TopDecl::Func(f) => f,
            _ => panic!("expected func"),
        })
        .expect("compile");

        // First statement should be an ExprAssign with Expr::Call that has children
        match &flow.body[0] {
            Statement::ExprAssign(ea) => {
                assert!(ea.bind.starts_with("_discard_"));
                match &ea.expr {
                    Expr::Call { func, children, .. } => {
                        assert_eq!(func, "ui.vstack");
                        assert!(children.is_some(), "expected children in do...done block");
                        let kids = children.as_ref().unwrap();
                        assert_eq!(kids.len(), 1, "expected 1 child statement");
                    }
                    other => panic!("expected Call, got: {other:?}"),
                }
            }
            other => panic!("expected ExprAssign, got: {other:?}"),
        }
    }

    #[test]
    fn parse_ui_block_assignment() {
        let src = r#"
docs UiAssign
  Test UI block in assignment.
done

func UiAssign
  take count as long
  emit view as text
body
  tree = ui.screen() do
    ui.text("hello")
  done
  emit view
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let flow = parse_runtime_func_decl_v1(match &module.decls[1] {
            TopDecl::Func(f) => f,
            _ => panic!("expected func"),
        })
        .expect("compile");

        match &flow.body[0] {
            Statement::ExprAssign(ea) => {
                assert_eq!(ea.bind, "tree");
                match &ea.expr {
                    Expr::Call { func, children, .. } => {
                        assert_eq!(func, "ui.screen");
                        assert!(children.is_some());
                        assert_eq!(children.as_ref().unwrap().len(), 1);
                    }
                    other => panic!("expected Call, got: {other:?}"),
                }
            }
            other => panic!("expected ExprAssign, got: {other:?}"),
        }
    }

    #[test]
    fn parse_nested_ui_blocks() {
        let src = r#"
docs Nested
  Test nested UI blocks.
done

func Nested
  take x as text
  emit result as text
body
  tree = ui.screen() do
    ui.vstack(20) do
      ui.text("hello")
      ui.text("world")
    done
  done
  emit result
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let flow = parse_runtime_func_decl_v1(match &module.decls[1] {
            TopDecl::Func(f) => f,
            _ => panic!("expected func"),
        })
        .expect("compile");

        match &flow.body[0] {
            Statement::ExprAssign(ea) => {
                assert_eq!(ea.bind, "tree");
                match &ea.expr {
                    Expr::Call { func, children, .. } => {
                        assert_eq!(func, "ui.screen");
                        let kids = children.as_ref().unwrap();
                        assert_eq!(kids.len(), 1);
                        // The child should be a bare call with its own children
                        match &kids[0] {
                            Statement::ExprAssign(inner_ea) => match &inner_ea.expr {
                                Expr::Call { func, children, .. } => {
                                    assert_eq!(func, "ui.vstack");
                                    let inner_kids = children.as_ref().unwrap();
                                    assert_eq!(
                                        inner_kids.len(),
                                        2,
                                        "vstack should have 2 text children"
                                    );
                                }
                                other => panic!("expected inner Call, got: {other:?}"),
                            },
                            other => panic!("expected inner ExprAssign, got: {other:?}"),
                        }
                    }
                    other => panic!("expected Call, got: {other:?}"),
                }
            }
            other => panic!("expected ExprAssign, got: {other:?}"),
        }
    }

    #[test]
    fn parse_ui_block_with_context_binding() {
        let src = r##"
docs Ctx
  Test do-block context binding.
done

func Ctx
  emit result as dict
body
  tree = ui.vstack(8) do stack
    stack.padding(12)
    stack.backgroundColor("#333")
    ui.text("hello")
  done
  emit tree
done
"##;
        let module = parse_module_v1(src).expect("parse");
        let flow = parse_runtime_func_decl_v1(match &module.decls[1] {
            TopDecl::Func(f) => f,
            _ => panic!("expected func"),
        })
        .expect("compile");

        match &flow.body[0] {
            Statement::ExprAssign(ea) => match &ea.expr {
                Expr::Call { func, children, .. } => {
                    assert_eq!(func, "ui.vstack");
                    let kids = children.as_ref().expect("expected children");
                    assert!(matches!(
                        &kids[0],
                        Statement::ExprAssign(ExprAssign {
                            bind,
                            expr: Expr::Lit(v),
                            ..
                        }) if bind == "stack" && v == "__forai_ui_block_ctx__"
                    ));
                    assert!(
                        matches!(&kids[1], Statement::Node(NodeAssign { op, .. }) if op == "stack.padding")
                    );
                    assert!(
                        matches!(&kids[2], Statement::Node(NodeAssign { op, .. }) if op == "stack.backgroundColor")
                    );
                }
                other => panic!("expected Call, got: {other:?}"),
            },
            other => panic!("expected ExprAssign, got: {other:?}"),
        }
    }

    #[test]
    fn parse_emit_inside_ui_block() {
        let src = r#"
docs EmitBlock
  Test emit inside UI block.
done

func EmitBlock
  take x as text
  emit on_click as bool
body
  ui.button("Click") do
    v = true
    emit v
  done
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let flow = parse_runtime_func_decl_v1(match &module.decls[1] {
            TopDecl::Func(f) => f,
            _ => panic!("expected func"),
        })
        .expect("compile");

        match &flow.body[0] {
            Statement::ExprAssign(ea) => {
                match &ea.expr {
                    Expr::Call { func, children, .. } => {
                        assert_eq!(func, "ui.button");
                        let kids = children.as_ref().unwrap();
                        assert_eq!(kids.len(), 2, "should have assignment + emit");
                        // Second child should be an Emit
                        assert!(matches!(&kids[1], Statement::Emit(_)));
                    }
                    other => panic!("expected Call, got: {other:?}"),
                }
            }
            other => panic!("expected ExprAssign, got: {other:?}"),
        }
    }

    #[test]
    fn parse_empty_do_block() {
        let src = r#"
docs Empty
  Test empty do block.
done

func Empty
  take x as text
  emit result as text
body
  ui.vstack() do
  done
  emit result
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let flow = parse_runtime_func_decl_v1(match &module.decls[1] {
            TopDecl::Func(f) => f,
            _ => panic!("expected func"),
        })
        .expect("compile");

        match &flow.body[0] {
            Statement::ExprAssign(ea) => match &ea.expr {
                Expr::Call { children, .. } => {
                    let kids = children.as_ref().unwrap();
                    assert_eq!(kids.len(), 0, "empty do...done should produce 0 children");
                }
                other => panic!("expected Call, got: {other:?}"),
            },
            other => panic!("expected ExprAssign, got: {other:?}"),
        }
    }

    #[test]
    fn do_as_variable_name_not_confused() {
        // `do` should only trigger block parsing after a Call's `)`
        // Using `do` as a plain identifier should work fine elsewhere
        let src = r#"
docs DoVar
  Test do as variable.
done

func DoVar
  take x as text
  emit result as text
body
  result = str.upper(x)
  emit result
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let flow = parse_runtime_func_decl_v1(match &module.decls[1] {
            TopDecl::Func(f) => f,
            _ => panic!("expected func"),
        })
        .expect("compile");
        // Should parse without errors; `do` never appears here
        assert_eq!(flow.body.len(), 2);
    }

    #[test]
    fn parse_5_level_nested_ui_blocks() {
        let src = r#"
docs Deep
  Test deeply nested UI blocks.
done

func Deep
  take x as text
  emit result as text
body
  tree = ui.screen() do
    ui.zstack() do
      ui.vstack() do
        ui.hstack() do
          ui.text("deep")
        done
      done
    done
  done
  emit result
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let flow = parse_runtime_func_decl_v1(match &module.decls[1] {
            TopDecl::Func(f) => f,
            _ => panic!("expected func"),
        })
        .expect("compile");

        // Verify 5 levels parsed: screen > zstack > vstack > hstack > text
        match &flow.body[0] {
            Statement::ExprAssign(ea) => {
                match &ea.expr {
                    Expr::Call { func, children, .. } => {
                        assert_eq!(func, "ui.screen");
                        let l1 = children.as_ref().unwrap();
                        assert_eq!(l1.len(), 1);
                        // Level 2: zstack
                        if let Statement::ExprAssign(ea2) = &l1[0] {
                            if let Expr::Call {
                                func: f2,
                                children: c2,
                                ..
                            } = &ea2.expr
                            {
                                assert_eq!(f2, "ui.zstack");
                                let l2 = c2.as_ref().unwrap();
                                // Level 3: vstack
                                if let Statement::ExprAssign(ea3) = &l2[0] {
                                    if let Expr::Call {
                                        func: f3,
                                        children: c3,
                                        ..
                                    } = &ea3.expr
                                    {
                                        assert_eq!(f3, "ui.vstack");
                                        let l3 = c3.as_ref().unwrap();
                                        // Level 4: hstack
                                        if let Statement::ExprAssign(ea4) = &l3[0] {
                                            if let Expr::Call {
                                                func: f4,
                                                children: c4,
                                                ..
                                            } = &ea4.expr
                                            {
                                                assert_eq!(f4, "ui.hstack");
                                                let l4 = c4.as_ref().unwrap();
                                                assert_eq!(
                                                    l4.len(),
                                                    1,
                                                    "hstack has 1 child (text)"
                                                );
                                            } else {
                                                panic!("expected hstack Call");
                                            }
                                        } else {
                                            panic!("expected hstack ExprAssign");
                                        }
                                    } else {
                                        panic!("expected vstack Call");
                                    }
                                } else {
                                    panic!("expected vstack ExprAssign");
                                }
                            } else {
                                panic!("expected zstack Call");
                            }
                        } else {
                            panic!("expected zstack ExprAssign");
                        }
                    }
                    other => panic!("expected Call, got: {other:?}"),
                }
            }
            other => panic!("expected ExprAssign, got: {other:?}"),
        }
    }

    // --- emit to :port tests ---

    #[test]
    fn parse_emit_to_port_in_do_block() {
        let src = r#"
docs BtnEmit
  Test emit to port inside do block.
done

func BtnEmit
  take x as text
  emit view as dict
body
  btn = ui.button("+") do
    emit true to :on_inc
  done
  emit btn
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let flow = parse_runtime_func_decl_v1(match &module.decls[1] {
            TopDecl::Func(f) => f,
            _ => panic!("expected func"),
        })
        .expect("compile");

        match &flow.body[0] {
            Statement::ExprAssign(ea) => match &ea.expr {
                Expr::Call { children, .. } => {
                    let kids = children.as_ref().unwrap();
                    match &kids[0] {
                        Statement::Emit(e) => {
                            assert_eq!(e.output, "on_inc", "emit should use the named port");
                        }
                        other => panic!("expected Emit, got: {other:?}"),
                    }
                }
                other => panic!("expected Call, got: {other:?}"),
            },
            other => panic!("expected ExprAssign, got: {other:?}"),
        }
    }

    #[test]
    fn parse_emit_without_port_unchanged() {
        let src = r#"
docs Plain
  Test bare emit uses func port.
done

func Plain
  take x as text
  emit result as text
body
  emit x
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let flow = parse_runtime_func_decl_v1(match &module.decls[1] {
            TopDecl::Func(f) => f,
            _ => panic!("expected func"),
        })
        .expect("compile");

        match &flow.body[0] {
            Statement::Emit(e) => {
                assert_eq!(
                    e.output, "result",
                    "bare emit should use func's declared port"
                );
            }
            other => panic!("expected Emit, got: {other:?}"),
        }
    }

    #[test]
    fn parse_emit_to_port_multiple() {
        let src = r#"
docs Multi
  Test multiple emit to port.
done

func Multi
  take x as text
  emit view as dict
body
  btn = ui.button("+") do
    emit true to :on_inc
    emit true to :on_focus
  done
  emit btn
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let flow = parse_runtime_func_decl_v1(match &module.decls[1] {
            TopDecl::Func(f) => f,
            _ => panic!("expected func"),
        })
        .expect("compile");

        match &flow.body[0] {
            Statement::ExprAssign(ea) => match &ea.expr {
                Expr::Call { children, .. } => {
                    let kids = children.as_ref().unwrap();
                    assert_eq!(kids.len(), 2);
                    match &kids[0] {
                        Statement::Emit(e) => assert_eq!(e.output, "on_inc"),
                        other => panic!("expected Emit, got: {other:?}"),
                    }
                    match &kids[1] {
                        Statement::Emit(e) => assert_eq!(e.output, "on_focus"),
                        other => panic!("expected Emit, got: {other:?}"),
                    }
                }
                other => panic!("expected Call, got: {other:?}"),
            },
            other => panic!("expected ExprAssign, got: {other:?}"),
        }
    }

    // --- local declaration tests ---

    #[test]
    fn parses_local_decl_literal() {
        let src = r#"
flow Counter
body
  local count = 0
  step Display(count to :count) done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        assert_eq!(graph.body.len(), 2);
        match &graph.body[0] {
            FlowStatement::Local(l) => {
                assert_eq!(l.bind, "count");
                assert!(matches!(&l.value, Some(Expr::Lit(v)) if v == 0));
            }
            _ => panic!("expected Local"),
        }
    }

    #[test]
    fn parses_local_decl_op_call() {
        let src = r#"
flow App
body
  local items = list.new()
  step Display(items to :items) done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        match &graph.body[0] {
            FlowStatement::Local(l) => {
                assert_eq!(l.bind, "items");
                assert_eq!(l.callee, "list.new");
                assert!(l.value.is_none());
            }
            _ => panic!("expected Local"),
        }
    }

    #[test]
    fn local_and_state_coexist() {
        let src = r#"
flow App
body
  state counter = 0
  local total = 0
  step Display(counter to :count) done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        assert_eq!(graph.body.len(), 3);
        assert!(matches!(&graph.body[0], FlowStatement::State(_)));
        assert!(matches!(&graph.body[1], FlowStatement::Local(_)));
        assert!(matches!(&graph.body[2], FlowStatement::Step(_)));
    }

    #[test]
    fn local_lowers_to_expr_assign() {
        let src = r#"
flow App
body
  local total = 0
  step Display(total to :count) done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        let flow = lower_flow_graph_to_flow(&graph).expect("lowering should succeed");
        assert_eq!(flow.body.len(), 2);
        match &flow.body[0] {
            Statement::ExprAssign(ea) => {
                assert_eq!(ea.bind, "total");
            }
            _ => panic!("expected ExprAssign from local lowering"),
        }
    }

    // --- on block declaration tests ---

    #[test]
    fn parses_on_block_single() {
        let src = r#"
flow App
body
  state counter = 0
  step CounterView(counter to :count) then
    on :inc as val then
        step Increment(counter to :count) then
            next :count to counter
        done
    done
  done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        match &graph.body[1] {
            FlowStatement::Step(step) => {
                assert_eq!(step.then_body.len(), 1);
                match &step.then_body[0] {
                    StepThenItem::On(on) => {
                        assert_eq!(on.port, "inc");
                        assert_eq!(on.wire, "val");
                        assert_eq!(on.body.len(), 1);
                        match &on.body[0] {
                            StepThenItem::Step(nested) => {
                                assert_eq!(nested.callee, "Increment");
                            }
                            _ => panic!("expected nested Step"),
                        }
                    }
                    _ => panic!("expected On"),
                }
            }
            _ => panic!("expected Step"),
        }
    }

    #[test]
    fn parses_on_block_multiple() {
        let src = r#"
flow App
body
  state counter = 0
  step FormView(counter to :count) then
    on :valid as form_data then
        step Validate(counter to :input) then
            next :output to validated
        done
    done
    on :error as error_data then
        step HandleError(counter to :input) then
            next :output to error_msg
        done
    done
  done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        match &graph.body[1] {
            FlowStatement::Step(step) => {
                assert_eq!(step.then_body.len(), 2);
                match &step.then_body[0] {
                    StepThenItem::On(on) => {
                        assert_eq!(on.port, "valid");
                        assert_eq!(on.wire, "form_data");
                    }
                    _ => panic!("expected On"),
                }
                match &step.then_body[1] {
                    StepThenItem::On(on) => {
                        assert_eq!(on.port, "error");
                        assert_eq!(on.wire, "error_data");
                    }
                    _ => panic!("expected On"),
                }
            }
            _ => panic!("expected Step"),
        }
    }

    #[test]
    fn parses_next_without_via_unchanged() {
        let src = r#"
flow App
body
  step Display(x to :input) then
    next :result to res
  done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        match &graph.body[0] {
            FlowStatement::Step(step) => match &step.then_body[0] {
                StepThenItem::Next(next) => {
                    assert_eq!(next.port, "result");
                    assert_eq!(next.wire, "res");
                    assert!(next.via_callee.is_none());
                    assert!(next.via_inputs.is_empty());
                    assert!(next.via_outputs.is_empty());
                }
                _ => panic!("expected Next"),
            },
            _ => panic!("expected Step"),
        }
    }

    // --- state/local metadata in lowered Flow ---

    #[test]
    fn lowered_flow_collects_state_and_local_names() {
        let src = r#"
flow App
body
  state counter = 0
  state name = "hello"
  local total = 0
  step Display(counter to :count) done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        let flow = lower_flow_graph_to_flow(&graph).expect("lowering should succeed");
        assert_eq!(flow.state_names, vec!["counter", "name"]);
        assert_eq!(flow.local_names, vec!["total"]);
    }

    #[test]
    fn lowered_func_has_empty_state_local() {
        let src = r#"
docs Foo
  Test func.
done
func Foo
  take x as long
  emit result as long
body
  result = x + 1
  emit result
done
"#;
        let flow = parse_runtime_flow_v1(src).expect("flow should parse");
        assert!(flow.state_names.is_empty());
        assert!(flow.local_names.is_empty());
    }

    // --- on/step lowering tests ---

    #[test]
    fn on_with_nested_step_lowers_to_sequential() {
        // Single on block with nested step → sequential (no case routing)
        let src = r#"
flow TestFlow
  take input as Foo
  emit result as Bar
body
  step CounterView(input to :count) then
    on :inc as val then
        step Increment(input to :count) then
            next :count to counter
        done
    done
    emit counter to :result
  done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        let flow = lower_flow_graph_to_flow(&graph).expect("lowering should succeed");

        // Parent step + obj.get (event value bind) + nested step + emit = 4
        assert_eq!(flow.body.len(), 4, "body: {:?}", flow.body);

        match &flow.body[0] {
            Statement::Node(n) => {
                assert!(
                    n.bind.starts_with("_step_"),
                    "parent bind should be temp, got: {}",
                    n.bind
                );
                assert_eq!(n.op, "CounterView");
            }
            other => panic!("expected Statement::Node for parent step, got: {:?}", other),
        }

        // Event value binding: val = obj.get(_step_N, "value")
        match &flow.body[1] {
            Statement::Node(n) => {
                assert_eq!(n.bind, "val");
                assert_eq!(n.op, "obj.get");
            }
            other => panic!("expected Statement::Node for event bind, got: {:?}", other),
        }

        match &flow.body[2] {
            Statement::Node(n) => {
                assert_eq!(n.bind, "counter");
                assert_eq!(n.op, "Increment");
            }
            other => panic!("expected Statement::Node for nested step, got: {:?}", other),
        }

        match &flow.body[3] {
            Statement::Emit(e) => {
                assert_eq!(e.output, "result");
            }
            other => panic!("expected Statement::Emit, got: {:?}", other),
        }
    }

    #[test]
    fn multiple_on_blocks_lower_to_case_routing() {
        let src = r#"
flow TestFlow
  take input as Foo
  emit result as Bar
body
  step View(input to :data) then
    on :valid as form_data then
        step Validate(input to :input) then
            next :output to validated
        done
    done
    on :error as error_data then
        step HandleError(input to :input) then
            next :output to error_msg
        done
    done
  done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        let flow = lower_flow_graph_to_flow(&graph).expect("lowering should succeed");

        // step Node + type.of Node + outer safety Case = 3
        assert_eq!(
            flow.body.len(),
            3,
            "expected step + type_check + case, got: {:?}",
            flow.body
        );

        // Step call
        match &flow.body[0] {
            Statement::Node(n) => {
                assert!(n.bind.starts_with("_step_"), "step bind should be temp");
                assert_eq!(n.op, "View");
            }
            other => panic!("expected step Node, got: {:?}", other),
        }

        // Type check: _type_N = type.of(_step_N)
        match &flow.body[1] {
            Statement::Node(n) => {
                assert!(n.bind.starts_with("_type_"), "type bind should be _type_N");
                assert_eq!(n.op, "type.of");
            }
            other => panic!("expected type.of Node, got: {:?}", other),
        }

        // Outer safety case wraps the routing
        match &flow.body[2] {
            Statement::Case(case) => {
                assert_eq!(case.arms.len(), 1, "single arm for 'dict' type check");
                match &case.arms[0].pattern {
                    Pattern::Lit(serde_json::Value::String(s)) => assert_eq!(s, "dict"),
                    other => panic!("expected 'dict' pattern, got: {:?}", other),
                }
                assert!(
                    case.arms[0].body.len() >= 2,
                    "dict arm should have has check + case"
                );
            }
            other => panic!("expected Case statement, got: {:?}", other),
        }
    }

    #[test]
    fn on_with_nested_step_counter_ui_pattern() {
        let src = r#"
flow Main
body
  state counter = 0
  step View(counter to :count) then
    on :on_inc as inc then
        step Update(counter to :count, "inc" to :type) then
            next :count to counter
        done
    done
    on :on_dec as dec then
        step Update(counter to :count, "dec" to :type) then
            next :count to counter
        done
    done
  done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        let flow = lower_flow_graph_to_flow(&graph).expect("lowering should succeed");

        // Init (counter=0) + step + type.of + outer safety Case = 4
        assert_eq!(flow.body.len(), 4, "body: {:?}", flow.body);
        assert_eq!(flow.state_names, vec!["counter"]);

        // Step call
        match &flow.body[1] {
            Statement::Node(n) => {
                assert!(n.bind.starts_with("_step_"), "step bind should be temp");
            }
            other => panic!("expected step Node, got: {:?}", other),
        }

        // Type check
        match &flow.body[2] {
            Statement::Node(n) => {
                assert_eq!(n.op, "type.of");
            }
            other => panic!("expected type.of Node, got: {:?}", other),
        }

        // Outer safety case contains the routing logic
        match &flow.body[3] {
            Statement::Case(_) => { /* routing wrapped in safety checks */ }
            other => panic!("expected Case, got: {:?}", other),
        }
    }

    #[test]
    fn step_without_on_bind_unchanged() {
        let src = r#"
flow TestFlow
  take input as Foo
  emit result as Bar
body
  step AddTwo(input to :input) then
    next :result to added
    emit added to :result
  done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        let flow = lower_flow_graph_to_flow(&graph).expect("lowering should succeed");

        // 2 statements: Node + emit
        assert_eq!(flow.body.len(), 2);

        match &flow.body[0] {
            Statement::Node(n) => {
                assert_eq!(
                    n.bind, "added",
                    "non-on step should bind directly to wire name"
                );
                assert_eq!(n.op, "AddTwo");
            }
            other => panic!("expected Statement::Node, got: {:?}", other),
        }
    }

    #[test]
    fn nested_step_in_on_block() {
        // Full step → on → step → next pattern
        let src = r#"
flow TestFlow
  take input as Foo
  emit result as Bar
body
  step Parent(input to :data) then
    on :action_a as a_val then
        step ChildA(a_val to :input) then
            next :output to a_result
        done
        emit a_result to :result
    done
    on :action_b as b_val then
        step ChildB(b_val to :input) then
            next :output to b_result
        done
        emit b_result to :result
    done
  done
done
"#;
        let module = parse_module_v1(src).expect("module should parse");
        let graph = parse_flow_graph_from_module_v1(&module).expect("flow graph should parse");
        let flow = lower_flow_graph_to_flow(&graph).expect("lowering should succeed");

        // step + type.of + case = 3
        assert_eq!(flow.body.len(), 3, "body: {:?}", flow.body);

        // Verify step call
        match &flow.body[0] {
            Statement::Node(n) => {
                assert_eq!(n.op, "Parent");
            }
            other => panic!("expected Parent step, got: {:?}", other),
        }

        // Verify case routing exists with 2 arms (inside safety wrappers)
        match &flow.body[2] {
            Statement::Case(outer) => {
                // outer is dict type check
                assert_eq!(outer.arms.len(), 1);
                // Inside: has check → route case
                let dict_body = &outer.arms[0].body;
                assert!(dict_body.len() >= 2);
                match &dict_body[1] {
                    Statement::Case(has_case) => {
                        let has_body = &has_case.arms[0].body;
                        assert!(has_body.len() >= 2);
                        match &has_body[1] {
                            Statement::Case(route_case) => {
                                assert_eq!(route_case.arms.len(), 2, "should have 2 routing arms");
                                // Each arm should have: obj.get (wire bind) + nested step Node + emit
                                assert!(
                                    route_case.arms[0].body.len() >= 3,
                                    "arm should have wire bind + step + emit, got: {:?}",
                                    route_case.arms[0].body
                                );
                            }
                            other => panic!("expected route Case, got: {:?}", other),
                        }
                    }
                    other => panic!("expected has Case, got: {:?}", other),
                }
            }
            other => panic!("expected outer Case, got: {:?}", other),
        }
    }

    #[test]
    fn use_named_imports() {
        let src = "use { View, Loop } from \"./app\"\n";
        let module = parse_module_v1(src).unwrap();
        assert_eq!(module.decls.len(), 1);
        match &module.decls[0] {
            TopDecl::Uses(u) => {
                assert_eq!(u.name, "app");
                assert_eq!(u.path, "./app");
                assert_eq!(u.imports, vec!["View", "Loop"]);
            }
            other => panic!("expected Uses, got: {:?}", other),
        }
    }

    #[test]
    fn use_named_import_single() {
        let src = "use { Round } from \"./round.fa\"\n";
        let module = parse_module_v1(src).unwrap();
        match &module.decls[0] {
            TopDecl::Uses(u) => {
                assert_eq!(u.name, "round");
                assert_eq!(u.path, "./round.fa");
                assert_eq!(u.imports, vec!["Round"]);
            }
            other => panic!("expected Uses, got: {:?}", other),
        }
    }

    #[test]
    fn use_whole_module_has_empty_imports() {
        let src = "use app from \"./app\"\n";
        let module = parse_module_v1(src).unwrap();
        match &module.decls[0] {
            TopDecl::Uses(u) => {
                assert_eq!(u.name, "app");
                assert_eq!(u.path, "./app");
                assert!(u.imports.is_empty());
            }
            other => panic!("expected Uses, got: {:?}", other),
        }
    }
}
