use crate::ast::{Arg, Expr, Flow, InterpExpr, Pattern, Statement};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone)]
struct Producer {
    kind: String,
    id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ir {
    pub forai_ir: String,
    pub flow: String,
    pub inputs: Vec<IrPort>,
    pub outputs: Vec<IrPort>,
    pub nodes: Vec<IrNode>,
    pub edges: Vec<IrEdge>,
    pub emits: Vec<IrEmit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrPort {
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrNode {
    pub id: String,
    pub op: String,
    pub bind: String,
    pub args: Vec<Arg>,
    pub when: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrEndpoint {
    pub kind: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrEdge {
    pub from: IrEndpoint,
    pub to: IrEndpoint,
    pub when: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrEmit {
    pub output: String,
    pub value_var: String,
    pub when: String,
}

fn combine_cond(base: Option<&str>, extra: &str) -> String {
    match base {
        Some(b) => format!("({b}) and ({extra})"),
        None => extra.to_string(),
    }
}

fn expr_to_text(expr: &Expr) -> String {
    match expr {
        Expr::Var(v) => v.clone(),
        Expr::Lit(v) => v.to_string(),
        Expr::BinOp { op, lhs, rhs } => {
            let op_str = match op {
                crate::ast::BinOp::Add => "+",
                crate::ast::BinOp::Sub => "-",
                crate::ast::BinOp::Mul => "*",
                crate::ast::BinOp::Div => "/",
                crate::ast::BinOp::Mod => "%",
                crate::ast::BinOp::Pow => "**",
                crate::ast::BinOp::Eq => "==",
                crate::ast::BinOp::Neq => "!=",
                crate::ast::BinOp::Lt => "<",
                crate::ast::BinOp::Gt => ">",
                crate::ast::BinOp::LtEq => "<=",
                crate::ast::BinOp::GtEq => ">=",
                crate::ast::BinOp::And => "&&",
                crate::ast::BinOp::Or => "||",
            };
            format!("({} {} {})", expr_to_text(lhs), op_str, expr_to_text(rhs))
        }
        Expr::UnaryOp { op, expr: inner } => {
            let op_str = match op {
                crate::ast::UnaryOp::Neg => "-",
                crate::ast::UnaryOp::Not => "!",
            };
            format!("({}{})", op_str, expr_to_text(inner))
        }
        Expr::Call { func, args, .. } => {
            let arg_strs: Vec<String> = args.iter().map(expr_to_text).collect();
            format!("{}({})", func, arg_strs.join(", "))
        }
        Expr::Interp(parts) => {
            let mut out = String::from("\"");
            for part in parts {
                match part {
                    crate::ast::InterpExpr::Lit(s) => out.push_str(s),
                    crate::ast::InterpExpr::Expr(e) => {
                        out.push('{');
                        out.push_str(&expr_to_text(e));
                        out.push('}');
                    }
                }
            }
            out.push('"');
            out
        }
        Expr::Ternary { cond, then_expr, else_expr } => {
            format!("({} ? {} : {})", expr_to_text(cond), expr_to_text(then_expr), expr_to_text(else_expr))
        }
        Expr::ListLit(items) => {
            let strs: Vec<String> = items.iter().map(expr_to_text).collect();
            format!("[{}]", strs.join(", "))
        }
        Expr::DictLit(pairs) => {
            let strs: Vec<String> = pairs.iter().map(|(k, v)| format!("{}: {}", k, expr_to_text(v))).collect();
            format!("{{{}}}", strs.join(", "))
        }
        Expr::Index { expr, index } => {
            format!("{}[{}]", expr_to_text(expr), expr_to_text(index))
        }
        Expr::Coalesce { lhs, rhs } => {
            format!("({} ?? {})", expr_to_text(lhs), expr_to_text(rhs))
        }
    }
}

fn pattern_to_text(pattern: &Pattern) -> String {
    match pattern {
        Pattern::Ident(v) => v.clone(),
        Pattern::Lit(v) => v.to_string(),
        Pattern::Or(alts) => alts.iter().map(pattern_to_text).collect::<Vec<_>>().join(" | "),
        Pattern::Range { lo, hi } => format!("{lo}..{hi}"),
        Pattern::Type(t) => format!(":{t}"),
    }
}

fn collect_expr_vars(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Var(v) => out.push(v.clone()),
        Expr::Lit(_) => {}
        Expr::BinOp { lhs, rhs, .. } => {
            collect_expr_vars(lhs, out);
            collect_expr_vars(rhs, out);
        }
        Expr::UnaryOp { expr: inner, .. } => {
            collect_expr_vars(inner, out);
        }
        Expr::Call { args, .. } => {
            for a in args {
                collect_expr_vars(a, out);
            }
        }
        Expr::Interp(parts) => {
            for part in parts {
                if let InterpExpr::Expr(e) = part {
                    collect_expr_vars(e, out);
                }
            }
        }
        Expr::Ternary { cond, then_expr, else_expr } => {
            collect_expr_vars(cond, out);
            collect_expr_vars(then_expr, out);
            collect_expr_vars(else_expr, out);
        }
        Expr::ListLit(items) => {
            for item in items {
                collect_expr_vars(item, out);
            }
        }
        Expr::DictLit(pairs) => {
            for (_, v) in pairs {
                collect_expr_vars(v, out);
            }
        }
        Expr::Index { expr, index } => {
            collect_expr_vars(expr, out);
            collect_expr_vars(index, out);
        }
        Expr::Coalesce { lhs, rhs } => {
            collect_expr_vars(lhs, out);
            collect_expr_vars(rhs, out);
        }
    }
}

pub fn lower_to_ir(flow: &Flow) -> Result<Ir, String> {
    let mut nodes = Vec::<IrNode>::new();
    let mut edges = Vec::<IrEdge>::new();
    let mut emits = Vec::<IrEmit>::new();

    let mut producers = HashMap::<String, Producer>::new();
    for input in &flow.inputs {
        producers.insert(
            input.name.clone(),
            Producer {
                kind: "input".to_string(),
                id: input.name.clone(),
            },
        );
    }

    fn walk(
        body: &[Statement],
        condition: Option<&str>,
        scope: &HashMap<String, Producer>,
        nodes: &mut Vec<IrNode>,
        edges: &mut Vec<IrEdge>,
        emits: &mut Vec<IrEmit>,
        in_loop: bool,
    ) -> Result<HashMap<String, Producer>, String> {
        let mut local_scope = scope.clone();

        for stmt in body {
            match stmt {
                Statement::Node(node) => {
                    let when = condition.unwrap_or("true").to_string();
                    nodes.push(IrNode {
                        id: node.node_id.clone(),
                        op: node.op.clone(),
                        bind: node.bind.clone(),
                        args: node.args.clone(),
                        when: when.clone(),
                    });

                    for (idx, arg) in node.args.iter().enumerate() {
                        if let Arg::Var { var } = arg {
                            let base = var.split('.').next().unwrap_or(var);
                            let Some(source) = local_scope.get(base) else {
                                return Err(format!(
                                    "Node `{}` references unknown var `{}`",
                                    node.node_id, var
                                ));
                            };

                            edges.push(IrEdge {
                                from: IrEndpoint {
                                    kind: source.kind.clone(),
                                    id: source.id.clone(),
                                    port: None,
                                },
                                to: IrEndpoint {
                                    kind: "node".to_string(),
                                    id: node.node_id.clone(),
                                    port: Some(format!("arg{idx}")),
                                },
                                when: when.clone(),
                            });
                        }
                    }

                    local_scope.insert(
                        node.bind.clone(),
                        Producer {
                            kind: "node".to_string(),
                            id: node.node_id.clone(),
                        },
                    );
                }
                Statement::ExprAssign(ea) => {
                    let when = condition.unwrap_or("true").to_string();
                    // Create a synthetic node for the expression
                    nodes.push(IrNode {
                        id: ea.bind.clone(),
                        op: format!("expr:{}", expr_to_text(&ea.expr)),
                        bind: ea.bind.clone(),
                        args: vec![],
                        when: when.clone(),
                    });

                    let mut vars_used = Vec::new();
                    collect_expr_vars(&ea.expr, &mut vars_used);
                    for (idx, var) in vars_used.iter().enumerate() {
                        let base = var.split('.').next().unwrap_or(var);
                        if let Some(source) = local_scope.get(base) {
                            edges.push(IrEdge {
                                from: IrEndpoint {
                                    kind: source.kind.clone(),
                                    id: source.id.clone(),
                                    port: None,
                                },
                                to: IrEndpoint {
                                    kind: "node".to_string(),
                                    id: ea.bind.clone(),
                                    port: Some(format!("arg{idx}")),
                                },
                                when: when.clone(),
                            });
                        }
                    }

                    local_scope.insert(
                        ea.bind.clone(),
                        Producer {
                            kind: "node".to_string(),
                            id: ea.bind.clone(),
                        },
                    );
                }
                Statement::Emit(emit) => {
                    let when = condition.unwrap_or("true").to_string();

                    if let Expr::Var(ref name) = emit.value_expr {
                        // Simple variable — use existing scope lookup (support dotted paths)
                        let base = name.split('.').next().unwrap_or(name);
                        let Some(source) = local_scope.get(base) else {
                            return Err(format!("Emit references unknown var `{}`", name));
                        };
                        emits.push(IrEmit {
                            output: emit.output.clone(),
                            value_var: name.clone(),
                            when: when.clone(),
                        });
                        edges.push(IrEdge {
                            from: IrEndpoint {
                                kind: source.kind.clone(),
                                id: source.id.clone(),
                                port: None,
                            },
                            to: IrEndpoint {
                                kind: "output".to_string(),
                                id: emit.output.clone(),
                                port: None,
                            },
                            when,
                        });
                    } else {
                        // Complex expression — desugar to synthetic node + edge
                        let synth_id = format!("_emit_{}", emit.output);
                        let expr_text = expr_to_text(&emit.value_expr);
                        nodes.push(IrNode {
                            id: synth_id.clone(),
                            op: format!("expr:{}", expr_text),
                            bind: synth_id.clone(),
                            args: vec![],
                            when: when.clone(),
                        });

                        let mut vars_used = Vec::new();
                        collect_expr_vars(&emit.value_expr, &mut vars_used);
                        for (idx, var) in vars_used.iter().enumerate() {
                            let base = var.split('.').next().unwrap_or(var);
                            if let Some(source) = local_scope.get(base) {
                                edges.push(IrEdge {
                                    from: IrEndpoint {
                                        kind: source.kind.clone(),
                                        id: source.id.clone(),
                                        port: None,
                                    },
                                    to: IrEndpoint {
                                        kind: "node".to_string(),
                                        id: synth_id.clone(),
                                        port: Some(format!("arg{idx}")),
                                    },
                                    when: when.clone(),
                                });
                            }
                        }

                        emits.push(IrEmit {
                            output: emit.output.clone(),
                            value_var: expr_text,
                            when: when.clone(),
                        });
                        edges.push(IrEdge {
                            from: IrEndpoint {
                                kind: "node".to_string(),
                                id: synth_id,
                                port: None,
                            },
                            to: IrEndpoint {
                                kind: "output".to_string(),
                                id: emit.output.clone(),
                                port: None,
                            },
                            when,
                        });
                    }
                }
                Statement::Case(case_block) => {
                    let expr_text = expr_to_text(&case_block.expr);
                    for arm in &case_block.arms {
                        let mut arm_cond = format!("case({expr_text} == {})", pattern_to_text(&arm.pattern));
                        if let Some(guard) = &arm.guard {
                            arm_cond = format!("{arm_cond} && {}", expr_to_text(guard));
                        }
                        let cond = combine_cond(condition, &arm_cond);
                        walk(&arm.body, Some(&cond), &local_scope, nodes, edges, emits, in_loop)?;
                    }
                    if !case_block.else_body.is_empty() {
                        let cond = combine_cond(condition, &format!("case_else({expr_text})"));
                        walk(
                            &case_block.else_body,
                            Some(&cond),
                            &local_scope,
                            nodes,
                            edges,
                            emits,
                            in_loop,
                        )?;
                    }
                }
                Statement::Loop(loop_block) => {
                    let index_suffix = if let Some(idx) = &loop_block.index {
                        format!(" with index {}", idx)
                    } else {
                        String::new()
                    };
                    let cond = combine_cond(
                        condition,
                        &format!(
                            "loop({} as {}{})",
                            expr_to_text(&loop_block.collection),
                            loop_block.item,
                            index_suffix
                        ),
                    );
                    let mut loop_scope = local_scope.clone();
                    loop_scope.insert(
                        loop_block.item.clone(),
                        Producer {
                            kind: "loop_item".to_string(),
                            id: loop_block.item.clone(),
                        },
                    );
                    if let Some(idx) = &loop_block.index {
                        loop_scope.insert(
                            idx.clone(),
                            Producer {
                                kind: "loop_index".to_string(),
                                id: idx.clone(),
                            },
                        );
                    }
                    walk(
                        &loop_block.body,
                        Some(&cond),
                        &loop_scope,
                        nodes,
                        edges,
                        emits,
                        true,
                    )?;
                }
                Statement::Sync(sync_block) => {
                    let cond = combine_cond(
                        condition,
                        &format!("sync({})", sync_block.targets.join(",")),
                    );
                    let sync_scope = walk(
                        &sync_block.body,
                        Some(&cond),
                        &local_scope,
                        nodes,
                        edges,
                        emits,
                        in_loop,
                    )?;
                    for (target, export) in sync_block.targets.iter().zip(sync_block.exports.iter())
                    {
                        if let Some(producer) = sync_scope.get(export) {
                            local_scope.insert(
                                target.clone(),
                                Producer {
                                    kind: producer.kind.clone(),
                                    id: producer.id.clone(),
                                },
                            );
                        }
                    }
                }
                Statement::SendNowait(sn) => {
                    let node_id = format!("send_{}", sn.target.replace('.', "_"));
                    nodes.push(IrNode {
                        id: node_id,
                        op: format!("send.nowait.{}", sn.target),
                        bind: String::new(),
                        args: vec![],
                        when: condition.unwrap_or("true").to_string(),
                    });
                }
                Statement::Break | Statement::Continue => {
                    // runtime-only; IR is a static graph, no-op
                }
                Statement::BareLoop(block) => {
                    let cond = combine_cond(condition, "bare_loop");
                    walk(
                        &block.body,
                        Some(&cond),
                        &local_scope,
                        nodes,
                        edges,
                        emits,
                        true,
                    )?;
                }
                Statement::SourceLoop(sl) => {
                    let when = condition.unwrap_or("true").to_string();
                    for (idx, arg) in sl.source_args.iter().enumerate() {
                        if let Arg::Var { var } = arg {
                            let base = var.split('.').next().unwrap_or(var);
                            let Some(source) = local_scope.get(base) else {
                                return Err(format!(
                                    "SourceLoop `{}` references unknown var `{}`",
                                    sl.source_op, var
                                ));
                            };
                            edges.push(IrEdge {
                                from: IrEndpoint {
                                    kind: source.kind.clone(),
                                    id: source.id.clone(),
                                    port: None,
                                },
                                to: IrEndpoint {
                                    kind: "source_loop".to_string(),
                                    id: sl.source_op.clone(),
                                    port: Some(format!("arg{idx}")),
                                },
                                when: when.clone(),
                            });
                        }
                    }
                    let cond = combine_cond(
                        condition,
                        &format!("source_loop({} as {})", sl.source_op, sl.bind),
                    );
                    let mut loop_scope = local_scope.clone();
                    loop_scope.insert(
                        sl.bind.clone(),
                        Producer {
                            kind: "source_event".to_string(),
                            id: sl.bind.clone(),
                        },
                    );
                    walk(
                        &sl.body,
                        Some(&cond),
                        &loop_scope,
                        nodes,
                        edges,
                        emits,
                        true,
                    )?;
                }
                Statement::On(on_block) => {
                    let cond = combine_cond(
                        condition,
                        &format!("on(:{} from {} as {})", on_block.event_tag, on_block.source_op, on_block.bind),
                    );
                    let mut on_scope = local_scope.clone();
                    on_scope.insert(
                        on_block.bind.clone(),
                        Producer {
                            kind: "on_event".to_string(),
                            id: on_block.bind.clone(),
                        },
                    );
                    walk(
                        &on_block.body,
                        Some(&cond),
                        &on_scope,
                        nodes,
                        edges,
                        emits,
                        true,
                    )?;
                }
            }
        }

        Ok(local_scope)
    }

    walk(
        &flow.body, None, &producers, &mut nodes, &mut edges, &mut emits, false,
    )?;

    Ok(Ir {
        forai_ir: "0.1".to_string(),
        flow: flow.name.clone(),
        inputs: flow
            .inputs
            .iter()
            .map(|p| IrPort {
                name: p.name.clone(),
                type_name: p.type_name.clone(),
            })
            .collect(),
        outputs: flow
            .outputs
            .iter()
            .map(|p| IrPort {
                name: p.name.clone(),
                type_name: p.type_name.clone(),
            })
            .collect(),
        nodes,
        edges,
        emits,
    })
}
