/// Static op signature registry for handle-producing and handle-consuming ops.
/// Returns `None` for ops not in the registry — the type checker skips those.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpType {
    Text,
    Bool,
    Long,
    Real,
    List,
    Dict,
    DbConn,
    HttpServer,
    HttpConn,
    WsConn,
    /// Argument is optional (caller may omit it)
    Optional(Box<OpType>),
}

impl std::fmt::Display for OpType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpType::Text => write!(f, "text"),
            OpType::Bool => write!(f, "bool"),
            OpType::Long => write!(f, "long"),
            OpType::Real => write!(f, "real"),
            OpType::List => write!(f, "list"),
            OpType::Dict => write!(f, "dict"),
            OpType::DbConn => write!(f, "db_conn"),
            OpType::HttpServer => write!(f, "http_server"),
            OpType::HttpConn => write!(f, "http_conn"),
            OpType::WsConn => write!(f, "ws_conn"),
            OpType::Optional(inner) => write!(f, "{}?", inner),
        }
    }
}

pub struct OpSignature {
    pub args: Vec<OpType>,
    pub returns: OpType,
}

pub fn op_signature(op: &str) -> Option<OpSignature> {
    let sig = match op {
        // Database ops
        "db.open" => OpSignature {
            args: vec![OpType::Text],
            returns: OpType::DbConn,
        },
        "db.exec" => OpSignature {
            args: vec![OpType::DbConn, OpType::Text, OpType::Optional(Box::new(OpType::List))],
            returns: OpType::Dict,
        },
        "db.query" => OpSignature {
            args: vec![OpType::DbConn, OpType::Text, OpType::Optional(Box::new(OpType::List))],
            returns: OpType::List,
        },
        "db.close" => OpSignature {
            args: vec![OpType::DbConn],
            returns: OpType::Bool,
        },
        // HTTP server ops
        "http.server.listen" => OpSignature {
            args: vec![OpType::Long],
            returns: OpType::HttpServer,
        },
        "http.server.accept" => OpSignature {
            args: vec![OpType::HttpServer],
            returns: OpType::Dict,
        },
        "http.server.respond" => OpSignature {
            args: vec![OpType::HttpConn, OpType::Long, OpType::Dict, OpType::Text],
            returns: OpType::Bool,
        },
        "http.server.close" => OpSignature {
            args: vec![OpType::HttpServer],
            returns: OpType::Bool,
        },
        // WebSocket ops
        "ws.connect" => OpSignature {
            args: vec![OpType::Text],
            returns: OpType::WsConn,
        },
        "ws.send" => OpSignature {
            args: vec![OpType::WsConn, OpType::Text],
            returns: OpType::Bool,
        },
        "ws.recv" => OpSignature {
            args: vec![OpType::WsConn],
            returns: OpType::Dict,
        },
        "ws.close" => OpSignature {
            args: vec![OpType::WsConn],
            returns: OpType::Bool,
        },
        _ => return None,
    };
    Some(sig)
}

/// Check if two types are compatible. A handle type is only compatible with itself.
/// `Optional` unwraps for checking.
pub fn types_compatible(expected: &OpType, actual: &OpType) -> bool {
    match expected {
        OpType::Optional(inner) => types_compatible(inner, actual),
        _ => expected == actual,
    }
}

