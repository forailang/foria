use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeclKind {
    Func,
    Flow,
    Sink,
    Source,
}

impl std::fmt::Display for DeclKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeclKind::Func => write!(f, "func"),
            DeclKind::Flow => write!(f, "flow"),
            DeclKind::Sink => write!(f, "sink"),
            DeclKind::Source => write!(f, "source"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Clone)]
pub struct ModuleAst {
    pub decls: Vec<TopDecl>,
}

#[derive(Debug, Clone)]
pub enum TopDecl {
    Uses(UsesDecl),
    Docs(DocsDecl),
    Flow(FlowDecl),
    Func(FuncDecl),
    Sink(FuncDecl),
    Source(FuncDecl),
    Type(TypeDecl),
    Enum(EnumDecl),
    Test(TestDecl),
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct UsesDecl {
    pub module: String,
    pub imports: Vec<String>, // named imports; empty = import whole module
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FieldDocsEntry {
    pub name: String,
    pub markdown: String,
}

#[derive(Debug, Clone)]
pub struct DocsDecl {
    pub name: String,
    pub markdown: String,
    pub field_docs: Vec<FieldDocsEntry>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FlowDecl {
    pub name: String,
    pub takes: Vec<TakeDecl>,
    pub emits: Vec<PortDecl>,
    pub fails: Vec<PortDecl>,
    pub body_text: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FuncDecl {
    pub name: String,
    pub takes: Vec<TakeDecl>,
    pub emits: Vec<PortDecl>,
    pub fails: Vec<PortDecl>,
    pub return_type: Option<String>,
    pub fail_type: Option<String>,
    pub body_text: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TakeDecl {
    pub name: String,
    pub type_name: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct PortDecl {
    pub name: String,
    pub type_name: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ConstraintValue {
    Bool(bool),
    Number(f64),
    Regex(String),
    Symbol(String),
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TypeConstraint {
    pub key: String,
    pub value: ConstraintValue,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FieldDecl {
    pub name: String,
    pub type_ref: String,
    pub constraints: Vec<TypeConstraint>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TypeKind {
    Scalar { base_type: String, constraints: Vec<TypeConstraint> },
    Struct { fields: Vec<FieldDecl> },
}

#[derive(Debug, Clone)]
pub struct TypeDecl {
    pub open: bool,
    pub name: String,
    pub kind: TypeKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumDecl {
    pub open: bool,
    pub name: String,
    pub variants: Vec<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TestDecl {
    pub name: String,
    pub body_text: String,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Port {
    pub name: String,
    pub type_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Arg {
    Var { var: String },
    Lit { lit: Value },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinOp {
    Add, Sub, Mul, Div, Mod, Pow,    // arithmetic
    Eq, Neq, Lt, Gt, LtEq, GtEq,    // comparison
    And, Or,                          // logical
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryOp {
    Neg, Not,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InterpExpr {
    Lit(String),
    Expr(Box<Expr>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Expr {
    Var(String),
    Lit(Value),
    BinOp { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },
    UnaryOp { op: UnaryOp, expr: Box<Expr> },
    Call { func: String, args: Vec<Expr> },
    Interp(Vec<InterpExpr>),
    Ternary { cond: Box<Expr>, then_expr: Box<Expr>, else_expr: Box<Expr> },
    ListLit(Vec<Expr>),
    DictLit(Vec<(String, Expr)>),
    Index { expr: Box<Expr>, index: Box<Expr> },
    Coalesce { lhs: Box<Expr>, rhs: Box<Expr> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExprAssign {
    pub bind: String,
    pub expr: Expr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Pattern {
    Ident(String),
    Lit(Value),
    Or(Vec<Pattern>),
    Range { lo: i64, hi: i64 },
    Type(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeAssign {
    pub bind: String,
    pub node_id: String,
    pub op: String,
    pub args: Vec<Arg>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Emit {
    pub output: String,
    pub value_var: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseArm {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseBlock {
    pub expr: Expr,
    pub arms: Vec<CaseArm>,
    pub else_body: Vec<Statement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopBlock {
    pub collection: Expr,
    pub item: String,
    pub index: Option<String>,
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BareLoopBlock {
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncOptions {
    pub timeout: Option<String>,
    pub retry: Option<i64>,
    pub safe: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncBlock {
    pub targets: Vec<String>,
    pub options: SyncOptions,
    pub body: Vec<Statement>,
    pub exports: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendNowait {
    pub target: String,
    pub args: Vec<Expr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLoopBlock {
    pub source_op: String,
    pub source_args: Vec<Arg>,
    pub bind: String,
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Statement {
    Node(NodeAssign),
    ExprAssign(ExprAssign),
    Emit(Emit),
    Case(CaseBlock),
    Loop(LoopBlock),
    Sync(SyncBlock),
    SendNowait(SendNowait),
    Break,
    Continue,
    BareLoop(BareLoopBlock),
    SourceLoop(SourceLoopBlock),
}

// --- Flow-level state/send-nowait AST types ---

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FlowStateDecl {
    pub bind: String,
    pub callee: String,
    pub args: Vec<Arg>,
    pub value: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FlowSendNowait {
    pub target: String,
    pub args: Vec<String>,
    pub span: Span,
}

// --- New flow-body AST types for step-based wiring ---

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PortMapping {
    pub port: String,
    pub value: Arg,
    pub span: Span,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct NextWire {
    pub port: String,
    pub wire: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ContinuationWire {
    pub port: String,
    pub callee: String,
    pub args: Vec<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FlowOnBlock {
    pub port: String,
    pub wire: String,
    pub body: Vec<StepThenItem>,
    pub span: Span,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum StepThenItem {
    Next(NextWire),
    On(FlowOnBlock),
    Step(StepBlock),
    Continuation(ContinuationWire),
    Emit(FlowEmitStmt),
    Fail(FlowEmitStmt),
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct StepBlock {
    pub callee: String,
    pub inputs: Vec<PortMapping>,
    pub then_body: Vec<StepThenItem>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum FlowStatement {
    Step(StepBlock),
    Emit(FlowEmitStmt),
    Fail(FlowEmitStmt),
    State(FlowStateDecl),
    SendNowait(FlowSendNowait),
    Branch(FlowBranchBlock),
    Log(FlowLogStmt),
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FlowLogStmt {
    pub args: Vec<Arg>,
    pub span: Span,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FlowBranchBlock {
    pub condition: Option<Expr>,
    pub body: Vec<FlowStatement>,
    pub span: Span,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FlowEmitStmt {
    pub port: String,
    pub wire: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FlowGraph {
    pub name: String,
    pub inputs: Vec<Port>,
    pub emit_ports: Vec<Port>,
    pub fail_ports: Vec<Port>,
    pub body: Vec<FlowStatement>,
}

// --- Existing func (formerly flow) runtime form ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Flow {
    pub name: String,
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
    pub body: Vec<Statement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub state_names: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub local_names: Vec<String>,
}
