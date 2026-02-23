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
    /// Named struct type (e.g. "ProcessOutput", "Date", "HttpResponse")
    Struct(String),
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
            OpType::Struct(name) => write!(f, "{name}"),
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
            args: vec![
                OpType::DbConn,
                OpType::Text,
                OpType::Optional(Box::new(OpType::List)),
            ],
            returns: OpType::Dict,
        },
        "db.query" => OpSignature {
            args: vec![
                OpType::DbConn,
                OpType::Text,
                OpType::Optional(Box::new(OpType::List)),
            ],
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
            returns: OpType::Struct("HttpRequest".to_string()),
        },
        "http.server.respond" => OpSignature {
            args: vec![OpType::HttpConn, OpType::Long, OpType::Dict, OpType::Text],
            returns: OpType::Bool,
        },
        "http.server.close" => OpSignature {
            args: vec![OpType::HttpServer],
            returns: OpType::Bool,
        },
        // HTTP respond convenience ops
        "http.respond.html" | "http.respond.json" | "http.respond.text" => OpSignature {
            args: vec![OpType::HttpConn, OpType::Long, OpType::Text],
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
            returns: OpType::Struct("WebSocketMessage".to_string()),
        },
        "ws.close" => OpSignature {
            args: vec![OpType::WsConn],
            returns: OpType::Bool,
        },
        // Process execution
        "exec.run" => OpSignature {
            args: vec![OpType::Text, OpType::Optional(Box::new(OpType::List))],
            returns: OpType::Struct("ProcessOutput".to_string()),
        },
        // HTTP client ops
        "http.get" | "http.delete" => OpSignature {
            args: vec![OpType::Text],
            returns: OpType::Struct("HttpResponse".to_string()),
        },
        "http.post" | "http.put" | "http.patch" => OpSignature {
            args: vec![OpType::Text],
            returns: OpType::Struct("HttpResponse".to_string()),
        },
        "http.request" => OpSignature {
            args: vec![OpType::Text, OpType::Text],
            returns: OpType::Struct("HttpResponse".to_string()),
        },
        "http.response" | "http.error_response" => OpSignature {
            args: vec![OpType::Long, OpType::Text],
            returns: OpType::Struct("HttpResponse".to_string()),
        },
        // Date ops
        "date.now" => OpSignature {
            args: vec![],
            returns: OpType::Struct("Date".to_string()),
        },
        "date.now_tz" => OpSignature {
            args: vec![OpType::Long],
            returns: OpType::Struct("Date".to_string()),
        },
        "date.from_unix_ms" => OpSignature {
            args: vec![OpType::Long],
            returns: OpType::Struct("Date".to_string()),
        },
        "date.from_parts" => OpSignature {
            args: vec![
                OpType::Long, OpType::Long, OpType::Long, OpType::Long,
                OpType::Long, OpType::Long, OpType::Long,
            ],
            returns: OpType::Struct("Date".to_string()),
        },
        "date.from_parts_tz" => OpSignature {
            args: vec![
                OpType::Long, OpType::Long, OpType::Long, OpType::Long,
                OpType::Long, OpType::Long, OpType::Long, OpType::Long,
            ],
            returns: OpType::Struct("Date".to_string()),
        },
        "date.from_iso" => OpSignature {
            args: vec![OpType::Text],
            returns: OpType::Struct("Date".to_string()),
        },
        "date.from_epoch" => OpSignature {
            args: vec![OpType::Struct("Date".to_string()), OpType::Long],
            returns: OpType::Struct("Date".to_string()),
        },
        "date.to_unix_ms" => OpSignature {
            args: vec![OpType::Struct("Date".to_string())],
            returns: OpType::Long,
        },
        "date.to_parts" => OpSignature {
            args: vec![OpType::Struct("Date".to_string())],
            returns: OpType::Dict,
        },
        "date.to_iso" => OpSignature {
            args: vec![OpType::Struct("Date".to_string())],
            returns: OpType::Text,
        },
        "date.to_epoch" => OpSignature {
            args: vec![OpType::Struct("Date".to_string()), OpType::Struct("Date".to_string())],
            returns: OpType::Long,
        },
        "date.weekday" => OpSignature {
            args: vec![OpType::Struct("Date".to_string())],
            returns: OpType::Long,
        },
        "date.with_tz" => OpSignature {
            args: vec![OpType::Struct("Date".to_string()), OpType::Long],
            returns: OpType::Struct("Date".to_string()),
        },
        "date.add" | "date.add_days" => OpSignature {
            args: vec![OpType::Struct("Date".to_string()), OpType::Long],
            returns: OpType::Struct("Date".to_string()),
        },
        "date.diff" | "date.compare" => OpSignature {
            args: vec![OpType::Struct("Date".to_string()), OpType::Struct("Date".to_string())],
            returns: OpType::Long,
        },
        // Stamp ops
        "stamp.now" => OpSignature {
            args: vec![],
            returns: OpType::Struct("Stamp".to_string()),
        },
        "stamp.from_ns" => OpSignature {
            args: vec![OpType::Long],
            returns: OpType::Struct("Stamp".to_string()),
        },
        "stamp.from_epoch" => OpSignature {
            args: vec![OpType::Struct("Stamp".to_string()), OpType::Long],
            returns: OpType::Struct("Stamp".to_string()),
        },
        "stamp.to_ns" | "stamp.to_ms" => OpSignature {
            args: vec![OpType::Struct("Stamp".to_string())],
            returns: OpType::Long,
        },
        "stamp.to_date" => OpSignature {
            args: vec![OpType::Struct("Stamp".to_string())],
            returns: OpType::Struct("Date".to_string()),
        },
        "stamp.to_epoch" => OpSignature {
            args: vec![OpType::Struct("Stamp".to_string()), OpType::Struct("Stamp".to_string())],
            returns: OpType::Long,
        },
        "stamp.add" => OpSignature {
            args: vec![OpType::Struct("Stamp".to_string()), OpType::Long],
            returns: OpType::Struct("Stamp".to_string()),
        },
        "stamp.diff" | "stamp.compare" => OpSignature {
            args: vec![OpType::Struct("Stamp".to_string()), OpType::Struct("Stamp".to_string())],
            returns: OpType::Long,
        },
        // Time range ops
        "trange.new" => OpSignature {
            args: vec![OpType::Struct("Date".to_string()), OpType::Struct("Date".to_string())],
            returns: OpType::Struct("TimeRange".to_string()),
        },
        "trange.start" | "trange.end" => OpSignature {
            args: vec![OpType::Struct("TimeRange".to_string())],
            returns: OpType::Struct("Date".to_string()),
        },
        "trange.duration_ms" => OpSignature {
            args: vec![OpType::Struct("TimeRange".to_string())],
            returns: OpType::Long,
        },
        "trange.contains" => OpSignature {
            args: vec![OpType::Struct("TimeRange".to_string()), OpType::Struct("Date".to_string())],
            returns: OpType::Bool,
        },
        "trange.overlaps" => OpSignature {
            args: vec![OpType::Struct("TimeRange".to_string()), OpType::Struct("TimeRange".to_string())],
            returns: OpType::Bool,
        },
        "trange.shift" => OpSignature {
            args: vec![OpType::Struct("TimeRange".to_string()), OpType::Long],
            returns: OpType::Struct("TimeRange".to_string()),
        },
        // Error ops
        "error.new" => OpSignature {
            args: vec![OpType::Text, OpType::Text],
            returns: OpType::Struct("ErrorObject".to_string()),
        },
        "error.wrap" => OpSignature {
            args: vec![OpType::Struct("ErrorObject".to_string()), OpType::Text],
            returns: OpType::Struct("ErrorObject".to_string()),
        },
        "error.code" | "error.message" => OpSignature {
            args: vec![OpType::Struct("ErrorObject".to_string())],
            returns: OpType::Text,
        },
        // URL ops
        "url.parse" => OpSignature {
            args: vec![OpType::Text],
            returns: OpType::Struct("URLParts".to_string()),
        },
        "url.query_parse" => OpSignature {
            args: vec![OpType::Text],
            returns: OpType::Dict,
        },
        "url.encode" | "url.decode" => OpSignature {
            args: vec![OpType::Text],
            returns: OpType::Text,
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
