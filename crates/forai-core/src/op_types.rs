/// Static op signature registry for all built-in ops.
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
    /// Polymorphic — compatible with any type (for generic ops like obj.get, to.text)
    Any,
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
            OpType::Any => write!(f, "any"),
        }
    }
}

pub struct OpSignature {
    pub args: Vec<OpType>,
    pub returns: OpType,
}

/// Shorthand helpers for constructing signatures
fn sig(args: Vec<OpType>, returns: OpType) -> OpSignature {
    OpSignature { args, returns }
}

fn opt(t: OpType) -> OpType {
    OpType::Optional(Box::new(t))
}

pub fn op_signature(op: &str) -> Option<OpSignature> {
    use OpType::*;

    let sig = match op {
        // ── Database ──
        "db.open" => sig(vec![Text], DbConn),
        "db.exec" => sig(vec![DbConn, Text, opt(List)], Dict),
        "db.query" => sig(vec![DbConn, Text, opt(List)], List),
        "db.close" => sig(vec![DbConn], Bool),
        "db.query_user_by_email" => sig(vec![DbConn, Text], Any),
        "db.query_credentials" => sig(vec![DbConn, Text], Any),

        // ── HTTP server ──
        "http.server.listen" => sig(vec![Long], HttpServer),
        "http.server.accept" => sig(vec![HttpServer], Struct("HttpRequest".into())),
        "http.server.respond" => sig(vec![HttpConn, Long, Dict, Text], Bool),
        "http.server.close" => sig(vec![HttpServer], Bool),

        // ── HTTP respond convenience ──
        "http.respond.html" | "http.respond.json" | "http.respond.text" => {
            sig(vec![HttpConn, Long, Text, opt(Dict)], Bool)
        }
        "http.respond.file" => sig(vec![HttpConn, Long, Text, Text], Bool),

        // ── WebSocket ──
        "ws.connect" => sig(vec![Text], WsConn),
        "ws.send" => sig(vec![WsConn, Text], Bool),
        "ws.recv" => sig(vec![WsConn], Struct("WebSocketMessage".into())),
        "ws.close" => sig(vec![WsConn], Bool),

        // ── Process execution ──
        "exec.run" => sig(vec![Text, opt(List)], Struct("ProcessOutput".into())),

        // ── HTTP client ──
        "http.get" | "http.delete" => sig(vec![Text, opt(Dict)], Struct("HttpResponse".into())),
        "http.post" | "http.put" | "http.patch" => {
            sig(vec![Text, opt(Any), opt(Dict)], Struct("HttpResponse".into()))
        }
        "http.request" => sig(vec![Text, Text, opt(Dict)], Struct("HttpResponse".into())),
        "http.response" => sig(vec![Long, opt(Any)], Struct("HttpResponse".into())),
        "http.error_response" => sig(vec![Long, Text], Struct("HttpResponse".into())),
        "http.extract_path" => sig(vec![Dict], Text),
        "http.extract_params" => sig(vec![Dict], Dict),

        // ── String ops ──
        "str.len" => sig(vec![Text], Long),
        "str.upper" | "str.lower" | "str.trim" | "str.trim_start" | "str.trim_end" => {
            sig(vec![Text], Text)
        }
        "str.split" => sig(vec![Text, Text], List),
        "str.join" => sig(vec![List, Text], Text),
        "str.replace" => sig(vec![Text, Text, Text], Text),
        "str.contains" | "str.starts_with" | "str.ends_with" => sig(vec![Text, Text], Bool),
        "str.slice" => sig(vec![Text, Long, Long], Text),
        "str.index_of" => sig(vec![Text, Text], Long),
        "str.repeat" => sig(vec![Text, Long], Text),

        // ── Object ops (accept Any for first arg since structs are dicts at runtime) ──
        "obj.new" => sig(vec![], Dict),
        "obj.set" => sig(vec![Any, Text, Any], Dict),
        "obj.get" => sig(vec![Any, Text], Any),
        "obj.has" => sig(vec![Any, Text], Bool),
        "obj.delete" => sig(vec![Any, Text], Dict),
        "obj.keys" => sig(vec![Any], List),
        "obj.merge" => sig(vec![Any, Any], Dict),

        // ── List ops ──
        "list.new" => sig(vec![], List),
        "list.range" => sig(vec![Long, Long], List),
        "list.append" => sig(vec![List, Any], List),
        "list.len" => sig(vec![List], Long),
        "list.contains" => sig(vec![List, Any], Bool),
        "list.slice" => sig(vec![List, Long, Long], List),
        "list.indices" => sig(vec![List], List),

        // ── Math ──
        "math.floor" => sig(vec![Any], Long),
        "math.round" => sig(vec![Any, Any], Real),

        // ── Type introspection ──
        "type.of" => sig(vec![Any], Text),

        // ── Type conversion ──
        "to.text" => sig(vec![Any], Text),
        "to.long" => sig(vec![Any], Long),
        "to.real" => sig(vec![Any], Real),
        "to.bool" => sig(vec![Any], Bool),

        // ── JSON codec ──
        "json.decode" => sig(vec![Text], Any),
        "json.encode" | "json.encode_pretty" => sig(vec![Any], Text),

        // ── Generic codec ──
        "codec.decode" => sig(vec![Text, Text], Any),
        "codec.encode" | "codec.encode_pretty" => sig(vec![Text, Any], Text),

        // ── Regex ──
        "regex.match" => sig(vec![Text, Text], Bool),
        "regex.find" => sig(vec![Text, Text], Dict),
        "regex.find_all" => sig(vec![Text, Text], List),
        "regex.replace" | "regex.replace_all" => sig(vec![Text, Text, Text], Text),
        "regex.split" => sig(vec![Text, Text], List),

        // ── Random ──
        "random.int" => sig(vec![Long, Long], Long),
        "random.float" => sig(vec![], Real),
        "random.uuid" => sig(vec![], Text),
        "random.choice" => sig(vec![List], Any),
        "random.shuffle" => sig(vec![List], List),

        // ── Hash ──
        "hash.sha256" | "hash.sha512" => sig(vec![Text], Text),
        "hash.hmac" => sig(vec![Text, Text, opt(Text)], Text),

        // ── Base64 ──
        "base64.encode" | "base64.decode" | "base64.encode_url" | "base64.decode_url" => {
            sig(vec![Text], Text)
        }

        // ── Crypto ──
        "crypto.hash_password" => sig(vec![Text], Text),
        "crypto.verify_password" => sig(vec![Text, Text], Bool),
        "crypto.sign_token" => sig(vec![Dict, Text], Text),
        "crypto.verify_token" => sig(vec![Text, Text], Dict),
        "crypto.random_bytes" => sig(vec![Long], Text),

        // ── Logging ──
        "log.debug" | "log.info" | "log.warn" | "log.error" | "log.trace" => {
            sig(vec![Any, opt(Any)], Bool)
        }

        // ── Cookies ──
        "cookie.parse" => sig(vec![Text], Dict),
        "cookie.get" => sig(vec![Dict, Text], Any),
        "cookie.set" => sig(vec![Text, Text, opt(Dict)], Text),
        "cookie.delete" => sig(vec![Text], Text),

        // ── Routing ──
        "route.match" => sig(vec![Text, Text], Bool),
        "route.params" => sig(vec![Text, Text], Dict),

        // ── HTML ──
        "html.escape" | "html.unescape" => sig(vec![Text], Text),

        // ── Templating ──
        "tmpl.render" => sig(vec![Text, Any], Text),

        // ── Environment ──
        "env.get" => sig(vec![Text, opt(Any)], Text),
        "env.set" => sig(vec![Text, Text], Bool),
        "env.has" => sig(vec![Text], Bool),
        "env.list" => sig(vec![], Dict),
        "env.remove" => sig(vec![Text], Bool),

        // ── File I/O ──
        "file.read" => sig(vec![Text], Text),
        "file.write" | "file.append" => sig(vec![Text, Text], Bool),
        "file.delete" | "file.exists" | "file.mkdir" | "file.is_dir" => sig(vec![Text], Bool),
        "file.list" => sig(vec![Text], List),
        "file.copy" | "file.move" => sig(vec![Text, Text], Bool),
        "file.size" => sig(vec![Text], Long),

        // ── Terminal I/O ──
        "term.print" => sig(vec![Any], Bool),
        "term.prompt" => sig(vec![Text], Text),
        "term.clear" => sig(vec![], Bool),
        "term.size" | "term.cursor" => sig(vec![], Dict),
        "term.move_to" => sig(vec![Long, Long], Bool),
        "term.color" => sig(vec![Text, Text], Bool),
        "term.read_key" => sig(vec![], Text),

        // ── Formatting ──
        "fmt.pad_hms" => sig(vec![Dict], Text),
        "fmt.wrap_field" => sig(vec![Text, Any], Dict),

        // ── Time utilities ──
        "time.split_hms" => sig(vec![Real], Dict),
        "time.sleep" | "time.tick" => sig(vec![Real], Bool),

        // ── Headers ──
        "headers.new" => sig(vec![], Dict),
        "headers.set" => sig(vec![Dict, Text, Text], Dict),
        "headers.get" => sig(vec![Dict, Text], Any),
        "headers.delete" => sig(vec![Dict, Text], Dict),

        // ── Auth (simulation) ──
        "auth.extract_email" | "auth.extract_password" => sig(vec![Dict], Text),
        "auth.validate_email" | "auth.validate_password" => sig(vec![Text], Bool),
        "auth.verify_password" => sig(vec![Text, Dict], Bool),
        "auth.sample_checks" => sig(vec![], List),
        "auth.pass_through" => sig(vec![Any], Any),

        // ── Date ops ──
        "date.now" => sig(vec![], Struct("Date".into())),
        "date.now_tz" => sig(vec![Long], Struct("Date".into())),
        "date.from_unix_ms" => sig(vec![Long], Struct("Date".into())),
        "date.from_parts" => sig(
            vec![Long, Long, Long, Long, Long, Long, Long],
            Struct("Date".into()),
        ),
        "date.from_parts_tz" => sig(
            vec![Long, Long, Long, Long, Long, Long, Long, Long],
            Struct("Date".into()),
        ),
        "date.from_iso" => sig(vec![Text], Struct("Date".into())),
        "date.from_epoch" => sig(vec![Struct("Date".into()), Long], Struct("Date".into())),
        "date.to_unix_ms" => sig(vec![Struct("Date".into())], Long),
        "date.to_parts" => sig(vec![Struct("Date".into())], Dict),
        "date.to_iso" => sig(vec![Struct("Date".into())], Text),
        "date.to_epoch" => sig(vec![Struct("Date".into()), Struct("Date".into())], Long),
        "date.weekday" => sig(vec![Struct("Date".into())], Long),
        "date.with_tz" => sig(vec![Struct("Date".into()), Long], Struct("Date".into())),
        "date.add" | "date.add_days" => sig(vec![Struct("Date".into()), Long], Struct("Date".into())),
        "date.diff" | "date.compare" => {
            sig(vec![Struct("Date".into()), Struct("Date".into())], Long)
        }

        // ── Stamp ops ──
        "stamp.now" => sig(vec![], Struct("Stamp".into())),
        "stamp.from_ns" => sig(vec![Long], Struct("Stamp".into())),
        "stamp.from_epoch" => sig(vec![Struct("Stamp".into()), Long], Struct("Stamp".into())),
        "stamp.to_ns" | "stamp.to_ms" => sig(vec![Struct("Stamp".into())], Long),
        "stamp.to_date" => sig(vec![Struct("Stamp".into())], Struct("Date".into())),
        "stamp.to_epoch" => sig(vec![Struct("Stamp".into()), Struct("Stamp".into())], Long),
        "stamp.add" => sig(vec![Struct("Stamp".into()), Long], Struct("Stamp".into())),
        "stamp.diff" | "stamp.compare" => {
            sig(vec![Struct("Stamp".into()), Struct("Stamp".into())], Long)
        }

        // ── Time range ops ──
        "trange.new" => sig(
            vec![Struct("Date".into()), Struct("Date".into())],
            Struct("TimeRange".into()),
        ),
        "trange.start" | "trange.end" => {
            sig(vec![Struct("TimeRange".into())], Struct("Date".into()))
        }
        "trange.duration_ms" => sig(vec![Struct("TimeRange".into())], Long),
        "trange.contains" => sig(
            vec![Struct("TimeRange".into()), Struct("Date".into())],
            Bool,
        ),
        "trange.overlaps" => sig(
            vec![Struct("TimeRange".into()), Struct("TimeRange".into())],
            Bool,
        ),
        "trange.shift" => sig(
            vec![Struct("TimeRange".into()), Long],
            Struct("TimeRange".into()),
        ),

        // ── Error ops ──
        "error.new" => sig(vec![Text, Text, opt(Any)], Struct("ErrorObject".into())),
        "error.wrap" => sig(
            vec![Struct("ErrorObject".into()), Text],
            Struct("ErrorObject".into()),
        ),
        "error.code" | "error.message" => sig(vec![Struct("ErrorObject".into())], Text),

        // ── URL ops ──
        "url.parse" => sig(vec![Text], Struct("URLParts".into())),
        "url.query_parse" => sig(vec![Text], Dict),
        "url.encode" | "url.decode" => sig(vec![Text], Text),

        _ => return None,
    };
    Some(sig)
}

/// Check if two types are compatible. A handle type is only compatible with itself.
/// `Optional` unwraps for checking. `Any` is compatible with everything.
pub fn types_compatible(expected: &OpType, actual: &OpType) -> bool {
    match (expected, actual) {
        (OpType::Any, _) | (_, OpType::Any) => true,
        (OpType::Optional(inner), _) => types_compatible(inner, actual),
        // Structs are dicts at runtime — allow dict↔struct compatibility
        (OpType::Struct(_), OpType::Dict) | (OpType::Dict, OpType::Struct(_)) => true,
        _ => expected == actual,
    }
}
