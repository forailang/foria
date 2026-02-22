use crate::ast::{
    Arg, BareLoopBlock, BinOp, CaseArm, CaseBlock, ConstraintValue, ContinuationWire, DocsDecl,
    Emit, EnumDecl, Expr, ExprAssign, FieldDecl, FieldDocsEntry, Flow, FlowBranchBlock, FlowDecl,
    FlowEmitStmt, FlowGraph, FlowSendNowait, FlowStateDecl, FlowStatement, FuncDecl, InterpExpr,
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
        // v1 func: named `emit`/`fail` ports
        let emit_decl = func_decl.emits.first().ok_or_else(|| {
            format!(
                "{}:{} func `{}` is missing `emit <name> as <Type>` declaration",
                func_decl.span.line, func_decl.span.col, func_decl.name
            )
        })?;
        let fail_decl = func_decl.fails.first().ok_or_else(|| {
            format!(
                "{}:{} func `{}` is missing `fail <name> as <Type>` declaration",
                func_decl.span.line, func_decl.span.col, func_decl.name
            )
        })?;

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
        (emit_decl.name.clone(), fail_decl.name.clone(), outs)
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

    Ok(Flow {
        name: graph.name.clone(),
        inputs: graph.inputs.clone(),
        outputs,
        body,
    })
}

fn lower_flow_statements(
    stmts: &[FlowStatement],
    counter: &mut usize,
) -> Result<Vec<Statement>, String> {
    let mut out = Vec::new();
    for stmt in stmts {
        match stmt {
            FlowStatement::Step(step) => {
                // Find first Next item for the bind name
                let bind = if let Some(StepThenItem::Next(first)) = step
                    .then_body
                    .iter()
                    .find(|i| matches!(i, StepThenItem::Next(_)))
                {
                    first.wire.clone()
                } else {
                    let name = format!("_step_{}", *counter);
                    *counter += 1;
                    name
                };

                let args: Vec<Arg> = step.inputs.iter().map(|pm| pm.value.clone()).collect();

                out.push(Statement::Node(NodeAssign {
                    bind: bind.clone(),
                    node_id: bind,
                    op: step.callee.clone(),
                    args,
                }));

                // Lower then_body items (emit, fail)
                for item in &step.then_body {
                    match item {
                        StepThenItem::Next(_) | StepThenItem::Continuation(_) => {}
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
                    }
                }
            }
            FlowStatement::Emit(e) => {
                out.push(Statement::Emit(Emit {
                    output: e.port.clone(),
                    value_expr: Expr::Var(e.wire.clone()),
                }));
            }
            FlowStatement::Fail(f) => {
                out.push(Statement::Emit(Emit {
                    output: f.port.clone(),
                    value_expr: Expr::Var(f.wire.clone()),
                }));
            }
            FlowStatement::State(state) => {
                let args: Vec<Arg> = state.args.clone();
                let bind = state.bind.clone();
                out.push(Statement::Node(NodeAssign {
                    bind: bind.clone(),
                    node_id: bind,
                    op: state.callee.clone(),
                    args,
                }));
            }
            FlowStatement::SendNowait(sn) => {
                let args: Vec<Expr> = sn.args.iter().map(|a| Expr::Var(a.clone())).collect();
                out.push(Statement::SendNowait(SendNowait {
                    target: sn.target.clone(),
                    args,
                }));
            }
            FlowStatement::Branch(branch) => {
                let lowered_body = lower_flow_statements(&branch.body, counter)?;
                match &branch.condition {
                    Some(cond) => {
                        out.push(Statement::Case(CaseBlock {
                            expr: cond.clone(),
                            arms: vec![CaseArm {
                                pattern: Pattern::Lit(serde_json::json!(true)),
                                body: lowered_body,
                            }],
                            else_body: vec![],
                        }));
                    }
                    None => {
                        out.extend(lowered_body);
                    }
                }
            }
        }
    }
    Ok(out)
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
                let pattern = self.parse_pattern_atom()?;
                self.expect_line_end("expected newline after `when` pattern")?;
                let body = self.parse_block(&[BodyStop::When, BodyStop::Else, BodyStop::Done])?;
                arms.push(CaseArm { pattern, body });
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
        self.expect_line_end("expected newline after loop header")?;

        let body = self.parse_block(&[BodyStop::Done])?;
        self.expect_keyword("done")?;
        self.expect_line_end("expected newline after loop `done`")?;

        Ok(Statement::Loop(LoopBlock {
            collection,
            item,
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
            Expr::Call { func, args } => {
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
        self.expect_line_end("expected newline after emit statement")?;
        Ok(Statement::Emit(Emit {
            output: self.emit_output_name.clone(),
            value_expr,
        }))
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

    fn parse_assign_stmt(&mut self) -> Result<Statement, ParseError> {
        let bind = self.expect_ident("expected assignment target identifier")?;
        self.expect_symbol('=')?;

        let expr = self.parse_pratt_expr(0)?;
        self.expect_line_end("expected newline after assignment")?;

        // Backward compat: if the expression is a simple Call with only Var/Lit args,
        // produce a Statement::Node to keep the old IR/runtime path.
        if let Expr::Call { func, args } = &expr {
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
                }));
            }
        }

        Ok(Statement::ExprAssign(ExprAssign { bind, expr }))
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
                // Parse dotted path (e.g. input.apr)
                let path = self.parse_var_path("expected expression")?;
                // Check if it's a function call: path(...)
                if self.peek_symbol('(') {
                    self.bump(); // consume '('
                    let args = self.parse_expr_args()?;
                    self.expect_symbol(')')?;
                    return Ok(Expr::Call { func: path, args });
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

    fn parse_pattern_atom(&mut self) -> Result<Pattern, ParseError> {
        match self.current().kind.clone() {
            TokenKind::StringLit(s) => {
                self.bump();
                Ok(Pattern::Lit(json!(s)))
            }
            TokenKind::Number(n) => {
                self.bump();
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
        if self.peek_keyword("send") {
            return self.parse_flow_send_nowait();
        }
        if self.peek_keyword("emit") {
            let emit = self.parse_flow_emit_inner()?;
            return Ok(FlowStatement::Emit(emit));
        }
        if self.peek_keyword("fail") {
            let fail = self.parse_flow_fail_inner()?;
            return Ok(FlowStatement::Fail(fail));
        }
        if self.peek_keyword("branch") {
            return self.parse_flow_branch();
        }
        self.err_here("expected `step`, `state`, `send`, `emit`, `fail`, or `branch` in flow body")
    }

    // Parse: state <bind> = <callee>(<arg1>, <arg2>, ...)
    fn parse_state_decl(&mut self) -> Result<FlowStatement, ParseError> {
        let span = self.current().span;
        self.expect_keyword("state")?;
        let bind = self.expect_ident("expected variable name after `state`")?;
        self.expect_symbol('=')?;
        let callee = self.parse_var_path("expected callee after `=` in state declaration")?;
        self.expect_symbol('(')?;
        let args = self.parse_arg_list(')')?;
        self.expect_symbol(')')?;
        self.expect_line_end("expected newline after state declaration")?;
        Ok(FlowStatement::State(FlowStateDecl {
            bind,
            callee,
            args,
            span,
        }))
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
                    span: next_span,
                }));
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
            return self.err_here("expected `next`, `emit`, `fail`, `:event to callee(...)`, or `done` in step then block");
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
                    let ident = self.expect_ident("expected wire label in port mapping")?;
                    Arg::Var { var: ident }
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
                let path = self.parse_var_path("expected expression")?;
                if self.peek_symbol('(') {
                    self.bump();
                    let args = self.parse_expr_args()?;
                    self.expect_symbol(')')?;
                    return Ok(Expr::Call { func: path, args });
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

    // --- Branch parsing ---

    fn parse_flow_branch(&mut self) -> Result<FlowStatement, ParseError> {
        let span = self.current().span;
        self.expect_keyword("branch")?;

        let condition = if self.peek_keyword("when") {
            self.bump(); // consume `when`
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
            } else {
                return self.err_here("expected top-level declaration");
            }
        }
        Ok(ModuleAst { decls })
    }

    fn parse_uses(&mut self) -> Result<UsesDecl, ParseError> {
        let span = self.expect_keyword("use")?.span;
        let name = self.expect_ident_value("expected name after `use`")?;
        self.expect_keyword("from")?;
        let path = self.expect_string_lit_value("expected path string after `from`")?;
        self.expect_line_end("expected newline after use declaration")?;
        Ok(UsesDecl { name, path, span })
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

        let body_text = self.collect_body_text(kw_span, &["case", "loop", "sync", "if"])?;

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

        let body_text = self.collect_body_text(kw_span, &["case", "loop", "sync", "if"])?;

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
        let body_text = self.collect_body_text(kw_span, &["case", "loop", "sync", "if", "on"])?;

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

        let body_text = self.collect_body_text(kw_span, &["step", "case", "if", "branch"])?;

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
        let mut paren_depth = 0i32; // track ( ) to skip keywords inside function call args

        while !self.at_eof() {
            // Track parenthesis depth to avoid treating `:branch`, `:step` etc. as keywords
            match &self.current().kind {
                TokenKind::Symbol('(') => {
                    paren_depth += 1;
                    self.bump();
                    prev_was_else = false;
                    continue;
                }
                TokenKind::Symbol(')') => {
                    paren_depth -= 1;
                    self.bump();
                    prev_was_else = false;
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
                } else if word == "if" && nesting_keywords.contains(&"if") && prev_was_else {
                    // "else if" — continuation, not a new nesting level
                    prev_was_else = false;
                } else if nesting_keywords.contains(&word) {
                    depth += 1;
                    prev_was_else = false;
                } else {
                    prev_was_else = word == "else";
                }
            } else {
                prev_was_else = false;
            }
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
        let flow = parse_runtime_func_decl_v1(
            match &module.decls[1] {
                TopDecl::Func(f) => f,
                _ => panic!("expected func"),
            },
        )
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
        let flow = parse_runtime_func_decl_v1(
            match &module.decls[1] {
                TopDecl::Func(f) => f,
                _ => panic!("expected func"),
            },
        )
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
        let flow = parse_runtime_func_decl_v1(
            match &module.decls[1] {
                TopDecl::Func(f) => f,
                _ => panic!("expected func"),
            },
        )
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
        let flow = parse_runtime_func_decl_v1(
            match &module.decls[1] {
                TopDecl::Func(f) => f,
                _ => panic!("expected func"),
            },
        )
        .expect("compile");
        match &flow.body[0] {
            Statement::Emit(e) => {
                assert_eq!(e.output, "error");
                assert!(matches!(&e.value_expr, Expr::Interp(_)));
            }
            other => panic!("expected Emit (fail), got: {other:?}"),
        }
    }
}
