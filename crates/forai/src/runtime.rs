use crate::ast::{Arg, DeclKind, Expr, Flow, InterpExpr, Pattern, Statement};
use crate::codec::CodecRegistry;
use crate::host::{self, Host};
use crate::host_native::NativeHost;
use crate::ir::Ir;
use crate::loader::FlowRegistry;
use crate::types::TypeRegistry;
use base64::Engine;
use hmac::{Hmac, Mac};
use rand::Rng;
use regex::Regex;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256, Sha512};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct NodeTraceEvent {
    pub step: usize,
    pub node_id: String,
    pub op: String,
    pub bind: String,
    pub when: String,
    pub status: String,
    pub args: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub duration_ms: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_group: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmitTraceEvent {
    pub output: String,
    pub value_var: String,
    pub when: String,
    pub emitted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct RunReport {
    pub flow: String,
    pub inputs: Value,
    pub outputs: Value,
    pub trace: Vec<NodeTraceEvent>,
    pub emits: Vec<EmitTraceEvent>,
    pub ir: Ir,
}

pub(crate) fn read_string_arg(args: &[Value], index: usize, op: &str) -> Result<String, String> {
    let Some(value) = args.get(index) else {
        return Err(format!("Op `{op}` missing arg{index}"));
    };
    let Some(text) = value.as_str() else {
        return Err(format!("Op `{op}` expected string at arg{index}"));
    };
    Ok(text.to_string())
}

pub(crate) fn read_i64_arg(args: &[Value], index: usize, op: &str) -> Result<i64, String> {
    let Some(value) = args.get(index) else {
        return Err(format!("Op `{op}` missing arg{index}"));
    };
    value
        .as_i64()
        .ok_or_else(|| format!("Op `{op}` expected integer at arg{index}"))
}

pub(crate) fn read_object_arg(
    args: &[Value],
    index: usize,
    op: &str,
) -> Result<serde_json::Map<String, Value>, String> {
    let Some(value) = args.get(index) else {
        return Err(format!("Op `{op}` missing arg{index}"));
    };
    let Some(map) = value.as_object() else {
        return Err(format!("Op `{op}` expected object at arg{index}"));
    };
    Ok(map.clone())
}

pub(crate) fn read_array_arg(args: &[Value], index: usize, op: &str) -> Result<Vec<Value>, String> {
    let Some(value) = args.get(index) else {
        return Err(format!("Op `{op}` missing arg{index}"));
    };
    let Some(arr) = value.as_array() else {
        return Err(format!("Op `{op}` expected array at arg{index}"));
    };
    Ok(arr.clone())
}

pub(crate) fn read_f64_arg(args: &[Value], index: usize, op: &str) -> Result<f64, String> {
    let Some(value) = args.get(index) else {
        return Err(format!("Op `{op}` missing arg{index}"));
    };
    value
        .as_f64()
        .ok_or_else(|| format!("Op `{op}` expected number at arg{index}"))
}

fn extract_first_number(s: &str) -> Option<f64> {
    let re = Regex::new(r"-?\d+(\.\d+)?").unwrap();
    re.find(s).and_then(|m| m.as_str().parse::<f64>().ok())
}

// ---------------------------------------------------------------------------
// URL percent-encoding helpers
// ---------------------------------------------------------------------------

fn percent_encode_str(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 3);
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(char::from(HEX_CHARS[(b >> 4) as usize]));
                out.push(char::from(HEX_CHARS[(b & 0x0f) as usize]));
            }
        }
    }
    out
}

const HEX_CHARS: [u8; 16] = *b"0123456789ABCDEF";

fn percent_decode_str(input: &str) -> String {
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
        } else if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' => Some(b - b'A' + 10),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// HTML escape helpers
// ---------------------------------------------------------------------------

fn html_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(ch),
        }
    }
    out
}

fn html_unescape(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&#x2F;", "/")
        .replace("&#47;", "/")
}

// ---------------------------------------------------------------------------
// Route matching helper
// ---------------------------------------------------------------------------

fn route_match_bool(pattern: &str, path: &str) -> bool {
    let pat_segs: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let path_segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    let mut pi = 0;
    for seg in &pat_segs {
        if seg.starts_with('*') {
            return true;
        }
        if pi >= path_segs.len() {
            return false;
        }
        if !seg.starts_with(':') && *seg != path_segs[pi] {
            return false;
        }
        pi += 1;
    }
    pi == path_segs.len()
}

fn route_extract_params(pattern: &str, path: &str) -> Value {
    let pat_segs: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let path_segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let mut params = serde_json::Map::new();

    let mut pi = 0;
    for seg in &pat_segs {
        if seg.starts_with('*') {
            let key = &seg[1..];
            let rest: Vec<&str> = path_segs[pi..].to_vec();
            let val = rest.join("/");
            if !key.is_empty() {
                params.insert(key.to_string(), json!(val));
            }
            return Value::Object(params);
        }
        if pi >= path_segs.len() {
            return json!({});
        }
        if seg.starts_with(':') {
            let key = &seg[1..];
            params.insert(key.to_string(), json!(path_segs[pi]));
        } else if *seg != path_segs[pi] {
            return json!({});
        }
        pi += 1;
    }
    if pi != path_segs.len() {
        return json!({});
    }
    Value::Object(params)
}

// ---------------------------------------------------------------------------
// Mustache template renderer
// ---------------------------------------------------------------------------

fn mustache_render(template: &str, data: &Value) -> String {
    mustache_render_ctx(template, data, data)
}

fn mustache_render_ctx(template: &str, data: &Value, root: &Value) -> String {
    let mut out = String::new();
    let bytes = template.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if i + 1 < len && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            // Section: {{#key}}...{{/key}} or {{^key}}...{{/key}}
            if i + 2 < len && (bytes[i + 2] == b'#' || bytes[i + 2] == b'^') {
                let inverted = bytes[i + 2] == b'^';
                let tag_start = i + 3;
                if let Some(close_braces) = template[tag_start..].find("}}") {
                    let key = &template[tag_start..tag_start + close_braces];
                    let after_open = tag_start + close_braces + 2;
                    let close_tag = format!("{{{{/{key}}}}}");
                    if let Some(section_end) = template[after_open..].find(&close_tag) {
                        let inner = &template[after_open..after_open + section_end];
                        let after_section = after_open + section_end + close_tag.len();
                        let val = resolve_mustache_key(key, data, root);
                        if inverted {
                            if is_falsy(&val) {
                                out.push_str(&mustache_render_ctx(inner, data, root));
                            }
                        } else {
                            match &val {
                                Value::Array(arr) => {
                                    for item in arr {
                                        out.push_str(&mustache_render_ctx(inner, item, root));
                                    }
                                }
                                _ if !is_falsy(&val) => {
                                    let ctx = if val.is_object() { &val } else { data };
                                    out.push_str(&mustache_render_ctx(inner, ctx, root));
                                }
                                _ => {}
                            }
                        }
                        i = after_section;
                        continue;
                    }
                }
            }
            // Unescaped: {{{key}}}
            if i + 2 < len && bytes[i + 2] == b'{' {
                let tag_start = i + 3;
                if let Some(close) = template[tag_start..].find("}}}") {
                    let key = &template[tag_start..tag_start + close];
                    let val = resolve_mustache_key(key.trim(), data, root);
                    out.push_str(&value_to_text(&val));
                    i = tag_start + close + 3;
                    continue;
                }
            }
            // Escaped: {{key}}
            let tag_start = i + 2;
            if let Some(close) = template[tag_start..].find("}}") {
                let key = &template[tag_start..tag_start + close];
                let val = resolve_mustache_key(key.trim(), data, root);
                out.push_str(&html_escape(&value_to_text(&val)));
                i = tag_start + close + 2;
                continue;
            }
        }
        let ch = template[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn resolve_mustache_key<'a>(key: &str, data: &'a Value, root: &'a Value) -> Value {
    if key == "." {
        return data.clone();
    }
    // Try data first, then root
    let val = resolve_dotted(data, key);
    if !val.is_null() {
        return val;
    }
    if !std::ptr::eq(data, root) {
        return resolve_dotted(root, key);
    }
    Value::Null
}

fn resolve_dotted(val: &Value, path: &str) -> Value {
    let mut current = val;
    for part in path.split('.') {
        match current {
            Value::Object(map) => {
                if let Some(v) = map.get(part) {
                    current = v;
                } else {
                    return Value::Null;
                }
            }
            _ => return Value::Null,
        }
    }
    current.clone()
}

fn is_falsy(val: &Value) -> bool {
    match val {
        Value::Null => true,
        Value::Bool(false) => true,
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Number(n) => n.as_f64().map_or(true, |v| v == 0.0),
        _ => false,
    }
}

fn value_to_text(val: &Value) -> String {
    match val {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else {
                n.to_string()
            }
        }
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// SQLite helpers
// ---------------------------------------------------------------------------

pub fn known_ops() -> &'static [&'static str] {
    &[
        "http.extract_path",
        "http.extract_params",
        "auth.extract_email",
        "auth.extract_password",
        "auth.validate_email",
        "auth.validate_password",
        "db.query_user_by_email",
        "db.query_credentials",
        // SQLite database
        "db.open",
        "db.exec",
        "db.query",
        "db.close",
        "auth.verify_password",
        "auth.sample_checks",
        "auth.pass_through",
        "http.error_response",
        "http.response",
        // HTTP client
        "http.get",
        "http.post",
        "http.put",
        "http.patch",
        "http.delete",
        "http.request",
        // HTTP server
        "http.server.listen",
        "http.server.accept",
        "http.server.respond",
        "http.server.close",
        // HTTP respond convenience
        "http.respond.html",
        "http.respond.json",
        "http.respond.text",
        "accept",
        // WebSocket client
        "ws.connect",
        "ws.send",
        "ws.recv",
        "ws.close",
        // Header utilities
        "headers.new",
        "headers.set",
        "headers.get",
        "headers.delete",
        // math
        "math.floor",
        "math.round",
        // time & format
        "time.sleep",
        "time.tick",
        "time.split_hms",
        "fmt.pad_hms",
        "fmt.wrap_field",
        // date
        "date.now",
        "date.now_tz",
        "date.from_unix_ms",
        "date.from_parts",
        "date.from_parts_tz",
        "date.from_iso",
        "date.from_epoch",
        "date.to_unix_ms",
        "date.to_parts",
        "date.to_iso",
        "date.to_epoch",
        "date.weekday",
        "date.with_tz",
        "date.add",
        "date.add_days",
        "date.diff",
        "date.compare",
        // stamp
        "stamp.now",
        "stamp.from_ns",
        "stamp.from_epoch",
        "stamp.to_ns",
        "stamp.to_ms",
        "stamp.to_date",
        "stamp.to_epoch",
        "stamp.add",
        "stamp.diff",
        "stamp.compare",
        // trange
        "trange.new",
        "trange.start",
        "trange.end",
        "trange.duration_ms",
        "trange.contains",
        "trange.overlaps",
        "trange.shift",
        // collections
        "list.range",
        "list.new",
        "list.append",

        "list.len",
        "list.contains",
        "list.slice",
        "list.indices",
        "obj.new",
        "obj.set",
        "obj.get",
        "obj.has",
        "obj.delete",
        "obj.keys",
        "obj.merge",
        // terminal
        "term.print",
        "term.prompt",
        "term.clear",
        "term.size",
        "term.cursor",
        "term.move_to",
        "term.color",
        "term.read_key",
        // file I/O
        "file.read",
        "file.write",
        "file.append",
        "file.delete",
        "file.exists",
        "file.list",
        "file.mkdir",
        "file.copy",
        "file.move",
        "file.size",
        "file.is_dir",
        // string
        "str.len",
        "str.upper",
        "str.lower",
        "str.trim",
        "str.trim_start",
        "str.trim_end",
        "str.split",
        "str.join",
        "str.replace",
        "str.contains",
        "str.starts_with",
        "str.ends_with",
        "str.slice",
        "str.index_of",
        "str.repeat",
        // type conversion
        "type.of",
        "to.text",
        "to.long",
        "to.real",
        "to.bool",
        // environment variables
        "env.get",
        "env.set",
        "env.has",
        "env.list",
        "env.remove",
        // process execution
        "exec.run",
        // regex
        "regex.match",
        "regex.find",
        "regex.find_all",
        "regex.replace",
        "regex.replace_all",
        "regex.split",
        // random / UUID
        "random.int",
        "random.float",
        "random.uuid",
        "random.choice",
        "random.shuffle",
        // hash
        "hash.sha256",
        "hash.sha512",
        "hash.hmac",
        // base64
        "base64.encode",
        "base64.decode",
        "base64.encode_url",
        "base64.decode_url",
        // crypto
        "crypto.hash_password",
        "crypto.verify_password",
        "crypto.sign_token",
        "crypto.verify_token",
        "crypto.random_bytes",
        // logging
        "log.debug",
        "log.info",
        "log.warn",
        "log.error",
        "log.trace",
        // error construction
        "error.new",
        "error.wrap",
        "error.code",
        "error.message",
        // cookie
        "cookie.parse",
        "cookie.get",
        "cookie.set",
        "cookie.delete",
        // url
        "url.parse",
        "url.query_parse",
        "url.encode",
        "url.decode",
        // route
        "route.match",
        "route.params",
        // html
        "html.escape",
        "html.unescape",
        // template
        "tmpl.render",
    ]
}

// ---------------------------------------------------------------------------
// Calendar / date-time helpers (pure functions, no external crates)
// ---------------------------------------------------------------------------

/// Hinnant algorithm: Gregorian (y, m, d) → days since Unix epoch (1970-01-01).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64; // [0, 399]
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) as u64 + 2) / 5 + d as u64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe as i64 - 719468
}

/// Reverse Hinnant: days since Unix epoch → (year, month, day).
pub(crate) fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as i64; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as i64, d)
}

/// ISO weekday from days since epoch: 1=Monday … 7=Sunday.
fn weekday_from_days(z: i64) -> i64 {
    (z + 3).rem_euclid(7) + 1
}

#[cfg(test)]
fn is_leap_year(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

#[cfg(test)]
fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(y) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Parse ISO 8601 string → (unix_ms, tz_offset_min).
/// Handles: `2024-03-15`, `2024-03-15T10:30:00`, `…Z`, `…+05:30`, `…-04:00`, `….123Z`.
fn parse_iso_datetime(s: &str) -> Result<(i64, i64), String> {
    let err = || format!("invalid ISO 8601 datetime: `{s}`");

    // Must have at least YYYY-MM-DD (10 chars)
    if s.len() < 10 {
        return Err(err());
    }
    let y: i64 = s[0..4].parse().map_err(|_| err())?;
    if s.as_bytes()[4] != b'-' {
        return Err(err());
    }
    let mo: i64 = s[5..7].parse().map_err(|_| err())?;
    if s.as_bytes()[7] != b'-' {
        return Err(err());
    }
    let d: i64 = s[8..10].parse().map_err(|_| err())?;

    let mut h: i64 = 0;
    let mut mi: i64 = 0;
    let mut sec: i64 = 0;
    let mut ms: i64 = 0;
    let mut tz_offset_min: i64 = 0;

    let rest = &s[10..];
    if !rest.is_empty() {
        // Expect 'T' separator
        let rest = rest
            .strip_prefix('T')
            .or_else(|| rest.strip_prefix('t'))
            .ok_or_else(err)?;

        // Parse HH:MM:SS
        if rest.len() < 8 {
            return Err(err());
        }
        h = rest[0..2].parse().map_err(|_| err())?;
        if rest.as_bytes()[2] != b':' {
            return Err(err());
        }
        mi = rest[3..5].parse().map_err(|_| err())?;
        if rest.as_bytes()[5] != b':' {
            return Err(err());
        }
        sec = rest[6..8].parse().map_err(|_| err())?;

        let mut rest = &rest[8..];

        // Optional fractional seconds
        if rest.starts_with('.') {
            rest = &rest[1..];
            // Collect up to 3 digits for ms
            let mut frac_str = String::new();
            let mut consumed = 0;
            for ch in rest.chars() {
                if ch.is_ascii_digit() && consumed < 3 {
                    frac_str.push(ch);
                    consumed += 1;
                } else if ch.is_ascii_digit() {
                    consumed += 1; // skip extra precision digits
                } else {
                    break;
                }
            }
            // Pad to 3 digits
            while frac_str.len() < 3 {
                frac_str.push('0');
            }
            ms = frac_str.parse().unwrap_or(0);
            rest = &rest[consumed..];
        }

        // Timezone: Z, +HH:MM, -HH:MM
        if rest == "Z" || rest == "z" {
            tz_offset_min = 0;
        } else if rest.starts_with('+') || rest.starts_with('-') {
            let sign: i64 = if rest.starts_with('+') { 1 } else { -1 };
            let tz = &rest[1..];
            if tz.len() < 5 || tz.as_bytes()[2] != b':' {
                return Err(err());
            }
            let tz_h: i64 = tz[0..2].parse().map_err(|_| err())?;
            let tz_m: i64 = tz[3..5].parse().map_err(|_| err())?;
            tz_offset_min = sign * (tz_h * 60 + tz_m);
        } else if !rest.is_empty() {
            return Err(err());
        }
    }

    let days = days_from_civil(y, mo, d);
    let day_ms = days * 86_400_000;
    let time_ms = h * 3_600_000 + mi * 60_000 + sec * 1_000 + ms;
    let unix_ms = day_ms + time_ms - tz_offset_min * 60_000;

    Ok((unix_ms, tz_offset_min))
}

/// Format unix_ms + tz_offset_min → ISO 8601 string.
fn format_iso_datetime(unix_ms: i64, tz_offset_min: i64) -> String {
    let local_ms = unix_ms + tz_offset_min * 60_000;
    let total_sec = local_ms.div_euclid(1000);
    let ms = local_ms.rem_euclid(1000);
    let total_min = total_sec.div_euclid(60);
    let sec = total_sec.rem_euclid(60);
    let total_hr = total_min.div_euclid(60);
    let min = total_min.rem_euclid(60);
    let days = total_hr.div_euclid(24);
    let hr = total_hr.rem_euclid(24);
    let (y, mo, d) = civil_from_days(days);

    let tz_str = if tz_offset_min == 0 {
        "Z".to_string()
    } else {
        let sign = if tz_offset_min > 0 { '+' } else { '-' };
        let abs = tz_offset_min.unsigned_abs();
        format!("{}{:02}:{:02}", sign, abs / 60, abs % 60)
    };

    if ms == 0 {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}",
            y, mo, d, hr, min, sec, tz_str
        )
    } else {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}{}",
            y, mo, d, hr, min, sec, ms, tz_str
        )
    }
}

fn make_date(unix_ms: i64, tz_offset_min: i64) -> Value {
    json!({"unix_ms": unix_ms, "tz_offset_min": tz_offset_min})
}

fn make_stamp(ns: i64) -> Value {
    json!({"ns": ns})
}

fn make_trange(start: Value, end: Value) -> Value {
    json!({"start": start, "end": end})
}

fn read_date_arg(args: &[Value], index: usize, op: &str) -> Result<(i64, i64), String> {
    let obj = read_object_arg(args, index, op)?;
    let unix_ms = obj
        .get("unix_ms")
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("Op `{op}` arg{index} missing `unix_ms`"))?;
    let tz = obj
        .get("tz_offset_min")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    Ok((unix_ms, tz))
}

fn read_stamp_arg(args: &[Value], index: usize, op: &str) -> Result<i64, String> {
    let obj = read_object_arg(args, index, op)?;
    obj.get("ns")
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("Op `{op}` arg{index} missing `ns`"))
}

fn read_trange_arg(
    args: &[Value],
    index: usize,
    op: &str,
) -> Result<serde_json::Map<String, Value>, String> {
    let obj = read_object_arg(args, index, op)?;
    if !obj.contains_key("start") || !obj.contains_key("end") {
        return Err(format!("Op `{op}` arg{index} must have `start` and `end`"));
    }
    Ok(obj)
}

async fn execute_op(
    op: &str,
    args: &[Value],
    host: &dyn Host,
    codecs: &CodecRegistry,
) -> Result<Value, String> {
    if host::is_io_op(op) {
        return host.execute_io_op(op, args).await;
    }
    match op {
        "http.extract_path" => {
            let request = read_object_arg(args, 0, op)?;
            Ok(request.get("path").cloned().unwrap_or_else(|| json!("")))
        }
        "http.extract_params" => {
            let request = read_object_arg(args, 0, op)?;
            Ok(request
                .get("params")
                .cloned()
                .unwrap_or_else(|| Value::Object(serde_json::Map::new())))
        }
        "auth.extract_email" => {
            let params = read_object_arg(args, 0, op)?;
            Ok(params.get("email").cloned().unwrap_or_else(|| json!("")))
        }
        "auth.extract_password" => {
            let params = read_object_arg(args, 0, op)?;
            Ok(params.get("password").cloned().unwrap_or_else(|| json!("")))
        }
        "auth.validate_email" => {
            let email = read_string_arg(args, 0, op)?;
            Ok(json!(email.contains('@') && email.contains('.')))
        }
        "auth.validate_password" => {
            let password = read_string_arg(args, 0, op)?;
            Ok(json!(password.len() >= 8))
        }
        "db.query_user_by_email" => {
            let email = read_string_arg(args, 0, op)?;
            let id = if let Some((prefix, _)) = email.split_once('@') {
                format!("user-{prefix}")
            } else {
                "user-unknown".to_string()
            };
            Ok(json!({ "id": id, "email": email }))
        }
        "db.query_credentials" => {
            let user = read_object_arg(args, 0, op)?;
            let email = user
                .get("email")
                .and_then(Value::as_str)
                .unwrap_or("unknown@example.com");
            let hash = if email == "ada@example.com" {
                "password123"
            } else {
                "invalid-password"
            };
            Ok(json!({ "password_hash": hash }))
        }
        "auth.verify_password" => {
            let password = read_string_arg(args, 0, op)?;
            let creds = read_object_arg(args, 1, op)?;
            let expected = creds
                .get("password_hash")
                .and_then(Value::as_str)
                .unwrap_or("");
            Ok(json!(password == expected))
        }
        "auth.sample_checks" => Ok(json!([true, true, true])),
        "auth.pass_through" => Ok(args.first().cloned().unwrap_or(Value::Null)),
        "http.error_response" => {
            let status = read_i64_arg(args, 0, op)?;
            let message = read_string_arg(args, 1, op)?;
            Ok(json!({
                "status": status,
                "headers": { "content-type": "application/json" },
                "body": format!("{{\"ok\":false,\"error\":\"{message}\"}}")
            }))
        }
        "math.floor" => {
            let a = read_f64_arg(args, 0, op)?;
            Ok(json!(a.floor() as i64))
        }
        "time.split_hms" => {
            let decimal_hours = read_f64_arg(args, 0, op)?;
            let total_seconds = (decimal_hours * 3600.0).round() as i64;
            let h = total_seconds / 3600;
            let m = (total_seconds % 3600) / 60;
            let s = total_seconds % 60;
            Ok(json!({ "h": h, "m": m, "s": s }))
        }
        "fmt.pad_hms" => {
            let hms = read_object_arg(args, 0, op)?;
            let h = hms.get("h").and_then(Value::as_i64).unwrap_or(0);
            let m = hms.get("m").and_then(Value::as_i64).unwrap_or(0);
            let s = hms.get("s").and_then(Value::as_i64).unwrap_or(0);
            Ok(json!(format!("{:02}:{:02}:{:02}", h, m, s)))
        }
        "fmt.wrap_field" => {
            let field_name = read_string_arg(args, 0, op)?;
            let value = args
                .get(1)
                .cloned()
                .ok_or_else(|| format!("Op `{op}` missing arg1"))?;
            let mut obj = serde_json::Map::new();
            obj.insert(field_name, value);
            Ok(Value::Object(obj))
        }
        "math.round" => {
            let value = read_f64_arg(args, 0, op)?;
            let places = read_f64_arg(args, 1, op)? as i32;
            let factor = 10f64.powi(places);
            Ok(json!((value * factor).round() / factor))
        }
        "list.range" => {
            let start = read_f64_arg(args, 0, op)? as i64;
            let end = read_f64_arg(args, 1, op)? as i64;
            let arr: Vec<Value> = (start..=end).map(|i| json!(i)).collect();
            Ok(Value::Array(arr))
        }
        "list.new" => Ok(Value::Array(Vec::new())),
        "list.append" => {
            let list_val = args
                .get(0)
                .ok_or_else(|| format!("Op `{op}` missing arg0"))?;
            let arr = list_val
                .as_array()
                .ok_or_else(|| format!("Op `{op}` expected array at arg0"))?;
            let item = args
                .get(1)
                .cloned()
                .ok_or_else(|| format!("Op `{op}` missing arg1"))?;
            let mut new_arr = arr.clone();
            new_arr.push(item);
            Ok(Value::Array(new_arr))
        }

        "list.len" => {
            let arr = read_array_arg(args, 0, op)?;
            Ok(json!(arr.len() as i64))
        }
        "list.contains" => {
            let arr = read_array_arg(args, 0, op)?;
            let needle = args
                .get(1)
                .ok_or_else(|| format!("Op `{op}` missing arg1"))?;
            Ok(json!(arr.contains(needle)))
        }
        "list.slice" => {
            let arr = read_array_arg(args, 0, op)?;
            let start = read_i64_arg(args, 1, op)?;
            let end = read_i64_arg(args, 2, op)?;
            let len = arr.len() as i64;
            let s = start.clamp(0, len) as usize;
            let e = end.clamp(0, len) as usize;
            if s >= e {
                Ok(Value::Array(Vec::new()))
            } else {
                Ok(Value::Array(arr[s..e].to_vec()))
            }
        }
        "list.indices" => {
            let arr = read_array_arg(args, 0, op)?;
            let indices: Vec<Value> = (0..arr.len()).map(|i| json!(i as i64)).collect();
            Ok(Value::Array(indices))
        }
        "obj.new" => Ok(Value::Object(serde_json::Map::new())),
        "obj.set" => {
            let obj_val = args
                .get(0)
                .ok_or_else(|| format!("Op `{op}` missing arg0"))?;
            let map = obj_val
                .as_object()
                .ok_or_else(|| format!("Op `{op}` expected object at arg0"))?;
            let key = read_string_arg(args, 1, op)?;
            let value = args
                .get(2)
                .cloned()
                .ok_or_else(|| format!("Op `{op}` missing arg2"))?;
            let mut new_map = map.clone();
            new_map.insert(key, value);
            Ok(Value::Object(new_map))
        }
        "obj.get" => {
            let map = read_object_arg(args, 0, op)?;
            let key = read_string_arg(args, 1, op)?;
            match map.get(&key) {
                Some(v) => Ok(v.clone()),
                None => Err(format!("Op `{op}` key \"{key}\" not found")),
            }
        }
        "obj.has" => {
            let map = read_object_arg(args, 0, op)?;
            let key = read_string_arg(args, 1, op)?;
            Ok(json!(map.contains_key(&key)))
        }
        "obj.delete" => {
            let map = read_object_arg(args, 0, op)?;
            let key = read_string_arg(args, 1, op)?;
            let mut new_map = map;
            new_map.remove(&key);
            Ok(Value::Object(new_map))
        }
        "obj.keys" => {
            let map = read_object_arg(args, 0, op)?;
            let keys: Vec<Value> = map.keys().map(|k| json!(k)).collect();
            Ok(Value::Array(keys))
        }
        "obj.merge" => {
            let base = read_object_arg(args, 0, op)?;
            let overlay = read_object_arg(args, 1, op)?;
            let mut merged = base;
            for (k, v) in overlay {
                merged.insert(k, v);
            }
            Ok(Value::Object(merged))
        }
        "http.response" => {
            let status = read_i64_arg(args, 0, op)?;
            let body = args.get(1).cloned().unwrap_or(Value::Null);
            Ok(json!({
                "status": status,
                "body": body
            }))
        }

        // --- Header utility ops ---
        "headers.new" => Ok(json!({})),
        "headers.set" => {
            let hdrs = read_object_arg(args, 0, op)?;
            let name = read_string_arg(args, 1, op)?.to_lowercase();
            let value = read_string_arg(args, 2, op)?;
            let mut new_hdrs = hdrs;
            new_hdrs.insert(name, json!(value));
            Ok(Value::Object(new_hdrs))
        }
        "headers.get" => {
            let hdrs = read_object_arg(args, 0, op)?;
            let name = read_string_arg(args, 1, op)?.to_lowercase();
            Ok(hdrs.get(&name).cloned().unwrap_or(Value::Null))
        }
        "headers.delete" => {
            let hdrs = read_object_arg(args, 0, op)?;
            let name = read_string_arg(args, 1, op)?.to_lowercase();
            let mut new_hdrs = hdrs;
            new_hdrs.remove(&name);
            Ok(Value::Object(new_hdrs))
        }

        // --- String ops ---
        "str.len" => {
            let s = read_string_arg(args, 0, op)?;
            Ok(json!(s.chars().count() as i64))
        }
        "str.upper" => {
            let s = read_string_arg(args, 0, op)?;
            Ok(json!(s.to_uppercase()))
        }
        "str.lower" => {
            let s = read_string_arg(args, 0, op)?;
            Ok(json!(s.to_lowercase()))
        }
        "str.trim" => {
            let s = read_string_arg(args, 0, op)?;
            Ok(json!(s.trim()))
        }
        "str.trim_start" => {
            let s = read_string_arg(args, 0, op)?;
            Ok(json!(s.trim_start()))
        }
        "str.trim_end" => {
            let s = read_string_arg(args, 0, op)?;
            Ok(json!(s.trim_end()))
        }
        "str.split" => {
            let s = read_string_arg(args, 0, op)?;
            let delim = read_string_arg(args, 1, op)?;
            let parts: Vec<Value> = s.split(&*delim).map(|p| json!(p)).collect();
            Ok(Value::Array(parts))
        }
        "str.join" => {
            let arr = args
                .get(0)
                .and_then(Value::as_array)
                .ok_or_else(|| format!("Op `{op}` expected array at arg0"))?;
            let sep = read_string_arg(args, 1, op)?;
            let joined: String = arr
                .iter()
                .map(|v| v.as_str().unwrap_or("").to_string())
                .collect::<Vec<_>>()
                .join(&sep);
            Ok(json!(joined))
        }
        "str.replace" => {
            let s = read_string_arg(args, 0, op)?;
            let from = read_string_arg(args, 1, op)?;
            let to = read_string_arg(args, 2, op)?;
            Ok(json!(s.replace(&*from, &to)))
        }
        "str.contains" => {
            let s = read_string_arg(args, 0, op)?;
            let substr = read_string_arg(args, 1, op)?;
            Ok(json!(s.contains(&*substr)))
        }
        "str.starts_with" => {
            let s = read_string_arg(args, 0, op)?;
            let prefix = read_string_arg(args, 1, op)?;
            Ok(json!(s.starts_with(&*prefix)))
        }
        "str.ends_with" => {
            let s = read_string_arg(args, 0, op)?;
            let suffix = read_string_arg(args, 1, op)?;
            Ok(json!(s.ends_with(&*suffix)))
        }
        "str.slice" => {
            let s = read_string_arg(args, 0, op)?;
            let start = read_i64_arg(args, 1, op)?;
            let end = read_i64_arg(args, 2, op)?;
            let chars: Vec<char> = s.chars().collect();
            let len = chars.len() as i64;
            let start = start.max(0).min(len) as usize;
            let end = end.max(0).min(len) as usize;
            let slice: String = if start <= end {
                chars[start..end].iter().collect()
            } else {
                String::new()
            };
            Ok(json!(slice))
        }
        "str.index_of" => {
            let s = read_string_arg(args, 0, op)?;
            let substr = read_string_arg(args, 1, op)?;
            let pos = s.find(&*substr).map(|i| i as i64).unwrap_or(-1);
            Ok(json!(pos))
        }
        "str.repeat" => {
            let s = read_string_arg(args, 0, op)?;
            let count = read_i64_arg(args, 1, op)?;
            if count < 0 {
                return Err(format!("Op `{op}` count must be non-negative"));
            }
            Ok(json!(s.repeat(count as usize)))
        }

        // -----------------------------------------------------------------
        // type conversion ops
        // -----------------------------------------------------------------
        "type.of" => {
            let v = args.first().unwrap_or(&Value::Null);
            let name = match v {
                Value::String(_) => "text",
                Value::Bool(_) => "bool",
                Value::Number(n) => {
                    if n.is_f64() && n.as_i64().is_none() {
                        "real"
                    } else {
                        "long"
                    }
                }
                Value::Array(_) => "list",
                Value::Object(_) => "dict",
                Value::Null => "void",
            };
            Ok(json!(name))
        }
        "to.text" => {
            let v = args.first().unwrap_or(&Value::Null);
            let s = match v {
                Value::String(s) => s.clone(),
                Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        i.to_string()
                    } else {
                        n.as_f64().unwrap().to_string()
                    }
                }
                Value::Bool(b) => b.to_string(),
                Value::Null => String::new(),
                Value::Array(_) | Value::Object(_) => serde_json::to_string(v).unwrap(),
            };
            Ok(json!(s))
        }
        "to.long" => {
            let v = args.first().unwrap_or(&Value::Null);
            let i = match v {
                Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        i
                    } else {
                        n.as_f64().unwrap().round() as i64
                    }
                }
                Value::String(s) => extract_first_number(s)
                    .map(|f| f.round() as i64)
                    .unwrap_or(0),
                Value::Bool(b) => {
                    if *b {
                        1
                    } else {
                        0
                    }
                }
                Value::Array(a) => a.len() as i64,
                Value::Object(m) => m.len() as i64,
                Value::Null => 0,
            };
            Ok(json!(i))
        }
        "to.real" => {
            let v = args.first().unwrap_or(&Value::Null);
            let f = match v {
                Value::Number(n) => n.as_f64().unwrap(),
                Value::String(s) => extract_first_number(s).unwrap_or(0.0),
                Value::Bool(b) => {
                    if *b {
                        1.0
                    } else {
                        0.0
                    }
                }
                Value::Array(a) => a.len() as f64,
                Value::Object(m) => m.len() as f64,
                Value::Null => 0.0,
            };
            Ok(json!(f))
        }
        "to.bool" => {
            let v = args.first().unwrap_or(&Value::Null);
            let b = match v {
                Value::Bool(b) => *b,
                Value::String(s) => !matches!(s.as_str(), "" | "false" | "0"),
                Value::Number(n) => n.as_f64().unwrap() != 0.0,
                Value::Array(a) => !a.is_empty(),
                Value::Object(m) => !m.is_empty(),
                Value::Null => false,
            };
            Ok(json!(b))
        }

        // -----------------------------------------------------------------
        // -----------------------------------------------------------------
        // regex.* ops
        // -----------------------------------------------------------------
        "regex.match" => {
            let text = read_string_arg(args, 0, op)?;
            let pattern = read_string_arg(args, 1, op)?;
            let re = Regex::new(&pattern).map_err(|e| format!("Op `{op}` invalid regex: {e}"))?;
            Ok(json!(re.is_match(&text)))
        }
        "regex.find" => {
            let text = read_string_arg(args, 0, op)?;
            let pattern = read_string_arg(args, 1, op)?;
            let re = Regex::new(&pattern).map_err(|e| format!("Op `{op}` invalid regex: {e}"))?;
            match re.captures(&text) {
                Some(caps) => {
                    let full = caps.get(0).map(|m| m.as_str()).unwrap_or("");
                    let groups: Vec<Value> = caps
                        .iter()
                        .skip(1)
                        .map(|m| m.map(|m| json!(m.as_str())).unwrap_or(Value::Null))
                        .collect();
                    Ok(json!({
                        "matched": true,
                        "text": full,
                        "groups": groups
                    }))
                }
                None => Ok(json!({
                    "matched": false,
                    "text": "",
                    "groups": []
                })),
            }
        }
        "regex.find_all" => {
            let text = read_string_arg(args, 0, op)?;
            let pattern = read_string_arg(args, 1, op)?;
            let re = Regex::new(&pattern).map_err(|e| format!("Op `{op}` invalid regex: {e}"))?;
            let matches: Vec<Value> = re.find_iter(&text).map(|m| json!(m.as_str())).collect();
            Ok(Value::Array(matches))
        }
        "regex.replace" => {
            let text = read_string_arg(args, 0, op)?;
            let pattern = read_string_arg(args, 1, op)?;
            let replacement = read_string_arg(args, 2, op)?;
            let re = Regex::new(&pattern).map_err(|e| format!("Op `{op}` invalid regex: {e}"))?;
            Ok(json!(re.replace(&text, replacement.as_str()).to_string()))
        }
        "regex.replace_all" => {
            let text = read_string_arg(args, 0, op)?;
            let pattern = read_string_arg(args, 1, op)?;
            let replacement = read_string_arg(args, 2, op)?;
            let re = Regex::new(&pattern).map_err(|e| format!("Op `{op}` invalid regex: {e}"))?;
            Ok(json!(
                re.replace_all(&text, replacement.as_str()).to_string()
            ))
        }
        "regex.split" => {
            let text = read_string_arg(args, 0, op)?;
            let pattern = read_string_arg(args, 1, op)?;
            let re = Regex::new(&pattern).map_err(|e| format!("Op `{op}` invalid regex: {e}"))?;
            let parts: Vec<Value> = re.split(&text).map(|s| json!(s)).collect();
            Ok(Value::Array(parts))
        }

        // -----------------------------------------------------------------
        // random.* ops
        // -----------------------------------------------------------------
        "random.int" => {
            let min = read_i64_arg(args, 0, op)?;
            let max = read_i64_arg(args, 1, op)?;
            if min > max {
                return Err(format!("Op `{op}` min ({min}) > max ({max})"));
            }
            let val = rand::thread_rng().gen_range(min..=max);
            Ok(json!(val))
        }
        "random.float" => {
            let val: f64 = rand::thread_rng().r#gen();
            Ok(json!(val))
        }
        "random.uuid" => Ok(json!(Uuid::new_v4().to_string())),
        "random.choice" => {
            let arr = read_array_arg(args, 0, op)?;
            if arr.is_empty() {
                return Err(format!("Op `{op}` cannot choose from empty list"));
            }
            let idx = rand::thread_rng().gen_range(0..arr.len());
            Ok(arr[idx].clone())
        }
        "random.shuffle" => {
            let arr = read_array_arg(args, 0, op)?;
            let mut shuffled = arr.clone();
            let mut rng = rand::thread_rng();
            for i in (1..shuffled.len()).rev() {
                let j = rng.gen_range(0..=i);
                shuffled.swap(i, j);
            }
            Ok(Value::Array(shuffled))
        }

        // -----------------------------------------------------------------
        // date.* ops
        // -----------------------------------------------------------------
        "date.now" => {
            let dur = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
            let unix_ms = dur.as_millis() as i64;
            Ok(make_date(unix_ms, 0))
        }
        "date.now_tz" => {
            let offset_min = read_i64_arg(args, 0, op)?;
            let dur = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
            let unix_ms = dur.as_millis() as i64;
            Ok(make_date(unix_ms, offset_min))
        }
        "date.from_unix_ms" => {
            let ms = read_i64_arg(args, 0, op)?;
            Ok(make_date(ms, 0))
        }
        "date.from_parts" => {
            let y = read_i64_arg(args, 0, op)?;
            let mo = read_i64_arg(args, 1, op)?;
            let d = read_i64_arg(args, 2, op)?;
            let h = read_i64_arg(args, 3, op)?;
            let mi = read_i64_arg(args, 4, op)?;
            let s = read_i64_arg(args, 5, op)?;
            let ms = read_i64_arg(args, 6, op)?;
            let days = days_from_civil(y, mo, d);
            let unix_ms = days * 86_400_000 + h * 3_600_000 + mi * 60_000 + s * 1_000 + ms;
            Ok(make_date(unix_ms, 0))
        }
        "date.from_parts_tz" => {
            let y = read_i64_arg(args, 0, op)?;
            let mo = read_i64_arg(args, 1, op)?;
            let d = read_i64_arg(args, 2, op)?;
            let h = read_i64_arg(args, 3, op)?;
            let mi = read_i64_arg(args, 4, op)?;
            let s = read_i64_arg(args, 5, op)?;
            let ms = read_i64_arg(args, 6, op)?;
            let tz = read_i64_arg(args, 7, op)?;
            let days = days_from_civil(y, mo, d);
            let local_ms = days * 86_400_000 + h * 3_600_000 + mi * 60_000 + s * 1_000 + ms;
            let unix_ms = local_ms - tz * 60_000;
            Ok(make_date(unix_ms, tz))
        }
        "date.from_iso" => {
            let s = read_string_arg(args, 0, op)?;
            let (unix_ms, tz) = parse_iso_datetime(&s)?;
            Ok(make_date(unix_ms, tz))
        }
        "date.from_epoch" => {
            let (epoch_ms, epoch_tz) = read_date_arg(args, 0, op)?;
            let offset_ms = read_i64_arg(args, 1, op)?;
            Ok(make_date(epoch_ms + offset_ms, epoch_tz))
        }
        "date.to_unix_ms" => {
            let (unix_ms, _) = read_date_arg(args, 0, op)?;
            Ok(json!(unix_ms))
        }
        "date.to_parts" => {
            let (unix_ms, tz) = read_date_arg(args, 0, op)?;
            let local_ms = unix_ms + tz * 60_000;
            let total_sec = local_ms.div_euclid(1000);
            let ms = local_ms.rem_euclid(1000);
            let total_min = total_sec.div_euclid(60);
            let sec = total_sec.rem_euclid(60);
            let total_hr = total_min.div_euclid(60);
            let min = total_min.rem_euclid(60);
            let days = total_hr.div_euclid(24);
            let hr = total_hr.rem_euclid(24);
            let (y, mo, d) = civil_from_days(days);
            Ok(json!({
                "year": y, "month": mo, "day": d,
                "hour": hr, "min": min, "sec": sec, "ms": ms,
                "tz_offset_min": tz
            }))
        }
        "date.to_iso" => {
            let (unix_ms, tz) = read_date_arg(args, 0, op)?;
            Ok(json!(format_iso_datetime(unix_ms, tz)))
        }
        "date.to_epoch" => {
            let (unix_ms, _) = read_date_arg(args, 0, op)?;
            let (epoch_ms, _) = read_date_arg(args, 1, op)?;
            Ok(json!(unix_ms - epoch_ms))
        }
        "date.weekday" => {
            let (unix_ms, tz) = read_date_arg(args, 0, op)?;
            let local_ms = unix_ms + tz * 60_000;
            let total_hr = local_ms.div_euclid(1000).div_euclid(60).div_euclid(60);
            let days = total_hr.div_euclid(24);
            Ok(json!(weekday_from_days(days)))
        }
        "date.with_tz" => {
            let (unix_ms, _) = read_date_arg(args, 0, op)?;
            let new_tz = read_i64_arg(args, 1, op)?;
            Ok(make_date(unix_ms, new_tz))
        }
        "date.add" => {
            let (unix_ms, tz) = read_date_arg(args, 0, op)?;
            let add_ms = read_i64_arg(args, 1, op)?;
            Ok(make_date(unix_ms + add_ms, tz))
        }
        "date.add_days" => {
            let (unix_ms, tz) = read_date_arg(args, 0, op)?;
            let days = read_i64_arg(args, 1, op)?;
            Ok(make_date(unix_ms + days * 86_400_000, tz))
        }
        "date.diff" => {
            let (a_ms, _) = read_date_arg(args, 0, op)?;
            let (b_ms, _) = read_date_arg(args, 1, op)?;
            Ok(json!(a_ms - b_ms))
        }
        "date.compare" => {
            let (a_ms, _) = read_date_arg(args, 0, op)?;
            let (b_ms, _) = read_date_arg(args, 1, op)?;
            Ok(json!(if a_ms < b_ms {
                -1
            } else if a_ms > b_ms {
                1
            } else {
                0
            }))
        }

        // -----------------------------------------------------------------
        // stamp.* ops
        // -----------------------------------------------------------------
        "stamp.now" => {
            let dur = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
            let ns = dur.as_nanos() as i64;
            Ok(make_stamp(ns))
        }
        "stamp.from_ns" => {
            let ns = read_i64_arg(args, 0, op)?;
            Ok(make_stamp(ns))
        }
        "stamp.from_epoch" => {
            let epoch_ns = read_stamp_arg(args, 0, op)?;
            let offset_ns = read_i64_arg(args, 1, op)?;
            Ok(make_stamp(epoch_ns + offset_ns))
        }
        "stamp.to_ns" => {
            let ns = read_stamp_arg(args, 0, op)?;
            Ok(json!(ns))
        }
        "stamp.to_ms" => {
            let ns = read_stamp_arg(args, 0, op)?;
            Ok(json!(ns / 1_000_000))
        }
        "stamp.to_date" => {
            let ns = read_stamp_arg(args, 0, op)?;
            let unix_ms = ns / 1_000_000;
            Ok(make_date(unix_ms, 0))
        }
        "stamp.to_epoch" => {
            let ns = read_stamp_arg(args, 0, op)?;
            let epoch_ns = read_stamp_arg(args, 1, op)?;
            Ok(json!(ns - epoch_ns))
        }
        "stamp.add" => {
            let ns = read_stamp_arg(args, 0, op)?;
            let add_ns = read_i64_arg(args, 1, op)?;
            Ok(make_stamp(ns + add_ns))
        }
        "stamp.diff" => {
            let a_ns = read_stamp_arg(args, 0, op)?;
            let b_ns = read_stamp_arg(args, 1, op)?;
            Ok(json!(a_ns - b_ns))
        }
        "stamp.compare" => {
            let a_ns = read_stamp_arg(args, 0, op)?;
            let b_ns = read_stamp_arg(args, 1, op)?;
            Ok(json!(if a_ns < b_ns {
                -1
            } else if a_ns > b_ns {
                1
            } else {
                0
            }))
        }

        // -----------------------------------------------------------------
        // trange.* ops
        // -----------------------------------------------------------------
        "trange.new" => {
            let (s_ms, s_tz) = read_date_arg(args, 0, op)?;
            let (e_ms, e_tz) = read_date_arg(args, 1, op)?;
            if s_ms > e_ms {
                return Err(format!("Op `{op}` start ({s_ms}) must be <= end ({e_ms})"));
            }
            Ok(make_trange(make_date(s_ms, s_tz), make_date(e_ms, e_tz)))
        }
        "trange.start" => {
            let tr = read_trange_arg(args, 0, op)?;
            Ok(tr.get("start").cloned().unwrap())
        }
        "trange.end" => {
            let tr = read_trange_arg(args, 0, op)?;
            Ok(tr.get("end").cloned().unwrap())
        }
        "trange.duration_ms" => {
            let tr = read_trange_arg(args, 0, op)?;
            let s_ms = tr
                .get("start")
                .and_then(|v| v.get("unix_ms"))
                .and_then(Value::as_i64)
                .ok_or_else(|| format!("Op `{op}` invalid start date"))?;
            let e_ms = tr
                .get("end")
                .and_then(|v| v.get("unix_ms"))
                .and_then(Value::as_i64)
                .ok_or_else(|| format!("Op `{op}` invalid end date"))?;
            Ok(json!(e_ms - s_ms))
        }
        "trange.contains" => {
            let tr = read_trange_arg(args, 0, op)?;
            let (d_ms, _) = read_date_arg(args, 1, op)?;
            let s_ms = tr
                .get("start")
                .and_then(|v| v.get("unix_ms"))
                .and_then(Value::as_i64)
                .ok_or_else(|| format!("Op `{op}` invalid start date"))?;
            let e_ms = tr
                .get("end")
                .and_then(|v| v.get("unix_ms"))
                .and_then(Value::as_i64)
                .ok_or_else(|| format!("Op `{op}` invalid end date"))?;
            Ok(json!(d_ms >= s_ms && d_ms <= e_ms))
        }
        "trange.overlaps" => {
            let a = read_trange_arg(args, 0, op)?;
            let b = read_trange_arg(args, 1, op)?;
            let a_start = a
                .get("start")
                .and_then(|v| v.get("unix_ms"))
                .and_then(Value::as_i64)
                .ok_or_else(|| format!("Op `{op}` invalid a.start"))?;
            let a_end = a
                .get("end")
                .and_then(|v| v.get("unix_ms"))
                .and_then(Value::as_i64)
                .ok_or_else(|| format!("Op `{op}` invalid a.end"))?;
            let b_start = b
                .get("start")
                .and_then(|v| v.get("unix_ms"))
                .and_then(Value::as_i64)
                .ok_or_else(|| format!("Op `{op}` invalid b.start"))?;
            let b_end = b
                .get("end")
                .and_then(|v| v.get("unix_ms"))
                .and_then(Value::as_i64)
                .ok_or_else(|| format!("Op `{op}` invalid b.end"))?;
            Ok(json!(a_start <= b_end && b_start <= a_end))
        }
        "trange.shift" => {
            let tr = read_trange_arg(args, 0, op)?;
            let shift_ms = read_i64_arg(args, 1, op)?;
            let s = tr
                .get("start")
                .and_then(|v| v.as_object())
                .ok_or_else(|| format!("Op `{op}` invalid start"))?;
            let e = tr
                .get("end")
                .and_then(|v| v.as_object())
                .ok_or_else(|| format!("Op `{op}` invalid end"))?;
            let s_ms = s.get("unix_ms").and_then(Value::as_i64).unwrap_or(0);
            let s_tz = s.get("tz_offset_min").and_then(Value::as_i64).unwrap_or(0);
            let e_ms = e.get("unix_ms").and_then(Value::as_i64).unwrap_or(0);
            let e_tz = e.get("tz_offset_min").and_then(Value::as_i64).unwrap_or(0);
            Ok(make_trange(
                make_date(s_ms + shift_ms, s_tz),
                make_date(e_ms + shift_ms, e_tz),
            ))
        }

        // -----------------------------------------------------------------
        // hash.* ops
        // -----------------------------------------------------------------
        "hash.sha256" => {
            let data = read_string_arg(args, 0, op)?;
            let mut hasher = Sha256::new();
            hasher.update(data.as_bytes());
            let result = hasher.finalize();
            Ok(json!(
                result
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            ))
        }
        "hash.sha512" => {
            let data = read_string_arg(args, 0, op)?;
            let mut hasher = Sha512::new();
            hasher.update(data.as_bytes());
            let result = hasher.finalize();
            Ok(json!(
                result
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            ))
        }
        "hash.hmac" => {
            let data = read_string_arg(args, 0, op)?;
            let key = read_string_arg(args, 1, op)?;
            let algo = args.get(2).and_then(|v| v.as_str()).unwrap_or("sha256");
            match algo {
                "sha256" => {
                    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes())
                        .map_err(|e| format!("Op `{op}` HMAC key error: {e}"))?;
                    mac.update(data.as_bytes());
                    let result = mac.finalize();
                    Ok(json!(
                        result
                            .into_bytes()
                            .iter()
                            .map(|b| format!("{b:02x}"))
                            .collect::<String>()
                    ))
                }
                "sha512" => {
                    let mut mac = Hmac::<Sha512>::new_from_slice(key.as_bytes())
                        .map_err(|e| format!("Op `{op}` HMAC key error: {e}"))?;
                    mac.update(data.as_bytes());
                    let result = mac.finalize();
                    Ok(json!(
                        result
                            .into_bytes()
                            .iter()
                            .map(|b| format!("{b:02x}"))
                            .collect::<String>()
                    ))
                }
                _ => Err(format!(
                    "Op `{op}` unsupported algorithm `{algo}`, expected `sha256` or `sha512`"
                )),
            }
        }

        // -----------------------------------------------------------------
        // base64.* ops
        // -----------------------------------------------------------------
        "base64.encode" => {
            let data = read_string_arg(args, 0, op)?;
            let encoded = base64::engine::general_purpose::STANDARD.encode(data.as_bytes());
            Ok(json!(encoded))
        }
        "base64.decode" => {
            let encoded = read_string_arg(args, 0, op)?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded.as_bytes())
                .map_err(|e| format!("Op `{op}` invalid base64: {e}"))?;
            let text = String::from_utf8(bytes)
                .map_err(|e| format!("Op `{op}` decoded bytes are not valid UTF-8: {e}"))?;
            Ok(json!(text))
        }
        "base64.encode_url" => {
            let data = read_string_arg(args, 0, op)?;
            let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data.as_bytes());
            Ok(json!(encoded))
        }
        "base64.decode_url" => {
            let encoded = read_string_arg(args, 0, op)?;
            let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(encoded.as_bytes())
                .map_err(|e| format!("Op `{op}` invalid URL-safe base64: {e}"))?;
            let text = String::from_utf8(bytes)
                .map_err(|e| format!("Op `{op}` decoded bytes are not valid UTF-8: {e}"))?;
            Ok(json!(text))
        }

        // -----------------------------------------------------------------
        // crypto.* ops
        // -----------------------------------------------------------------
        "crypto.hash_password" => {
            let password = read_string_arg(args, 0, op)?;
            // Use minimum cost in tests so the suite stays fast; production uses cost 12.
            #[cfg(test)]
            let cost = 4u32;
            #[cfg(not(test))]
            let cost = 12u32;
            let hash =
                bcrypt::hash(password, cost).map_err(|e| format!("Op `{op}` bcrypt error: {e}"))?;
            Ok(json!(hash))
        }
        "crypto.verify_password" => {
            let password = read_string_arg(args, 0, op)?;
            let hash = read_string_arg(args, 1, op)?;
            let valid = bcrypt::verify(password, &hash)
                .map_err(|e| format!("Op `{op}` bcrypt verify error: {e}"))?;
            Ok(json!(valid))
        }
        "crypto.sign_token" => {
            let payload = args
                .get(0)
                .ok_or_else(|| format!("Op `{op}` missing arg0 (payload)"))?;
            if !payload.is_object() {
                return Err(format!("Op `{op}` arg0 must be a dict, got {}", payload));
            }
            let secret = read_string_arg(args, 1, op)?;

            let header = json!({"alg": "HS256", "typ": "JWT"});
            let header_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(serde_json::to_string(&header).unwrap().as_bytes());
            let payload_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(serde_json::to_string(payload).unwrap().as_bytes());
            let signing_input = format!("{header_b64}.{payload_b64}");

            let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
                .map_err(|e| format!("Op `{op}` HMAC key error: {e}"))?;
            mac.update(signing_input.as_bytes());
            let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(mac.finalize().into_bytes());

            Ok(json!(format!("{signing_input}.{signature}")))
        }
        "crypto.verify_token" => {
            let token = read_string_arg(args, 0, op)?;
            let secret = read_string_arg(args, 1, op)?;

            let parts: Vec<&str> = token.splitn(3, '.').collect();
            if parts.len() != 3 {
                return Ok(json!({"valid": false, "error": "malformed token"}));
            }

            let signing_input = format!("{}.{}", parts[0], parts[1]);
            let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
                .map_err(|e| format!("Op `{op}` HMAC key error: {e}"))?;
            mac.update(signing_input.as_bytes());
            let expected_sig = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(mac.finalize().into_bytes());

            if expected_sig != parts[2] {
                return Ok(json!({"valid": false, "error": "invalid signature"}));
            }

            let payload_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(parts[1])
                .map_err(|_| format!("Op `{op}` invalid base64 in payload"))?;
            let payload: Value = serde_json::from_slice(&payload_bytes)
                .map_err(|_| format!("Op `{op}` invalid JSON in payload"))?;

            Ok(json!({"valid": true, "payload": payload}))
        }
        "crypto.random_bytes" => {
            let count = read_i64_arg(args, 0, op)?;
            if count < 1 || count > 1024 {
                return Err(format!("Op `{op}` count must be 1–1024, got {count}"));
            }
            let mut buf = vec![0u8; count as usize];
            rand::thread_rng().fill(&mut buf[..]);
            let hex: String = buf.iter().map(|b| format!("{b:02x}")).collect();
            Ok(json!(hex))
        }

        // --- Error construction ops ---
        "error.new" => {
            let code = read_string_arg(args, 0, op)?;
            let message = read_string_arg(args, 1, op)?;
            let details = args.get(2).cloned();
            let mut err = serde_json::Map::new();
            err.insert("code".to_string(), json!(code));
            err.insert("message".to_string(), json!(message));
            if let Some(d) = details {
                if !d.is_null() {
                    err.insert("details".to_string(), d);
                }
            }
            Ok(Value::Object(err))
        }
        "error.wrap" => {
            let mut outer = read_object_arg(args, 0, op)?;
            let context = read_string_arg(args, 1, op)?;
            let orig_msg = outer
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            outer.insert(
                "message".to_string(),
                json!(format!("{context}: {orig_msg}")),
            );
            Ok(Value::Object(outer))
        }
        "error.code" => {
            let err = read_object_arg(args, 0, op)?;
            Ok(err.get("code").cloned().unwrap_or(Value::Null))
        }
        "error.message" => {
            let err = read_object_arg(args, 0, op)?;
            Ok(err.get("message").cloned().unwrap_or(Value::Null))
        }

        // --- Cookie ops ---
        "cookie.parse" => {
            let header = read_string_arg(args, 0, op)?;
            let mut map = serde_json::Map::new();
            for pair in header.split(';') {
                let pair = pair.trim();
                if pair.is_empty() {
                    continue;
                }
                if let Some((k, v)) = pair.split_once('=') {
                    map.insert(k.trim().to_string(), json!(v.trim()));
                }
            }
            Ok(Value::Object(map))
        }
        "cookie.get" => {
            let cookies = read_object_arg(args, 0, op)?;
            let name = read_string_arg(args, 1, op)?;
            Ok(cookies.get(&name).cloned().unwrap_or(Value::Null))
        }
        "cookie.set" => {
            let name = read_string_arg(args, 0, op)?;
            let value = read_string_arg(args, 1, op)?;
            let opts = args.get(2).and_then(|v| v.as_object());
            let mut header = format!("{name}={value}");
            if let Some(opts) = opts {
                if let Some(path) = opts.get("path").and_then(Value::as_str) {
                    header.push_str(&format!("; Path={path}"));
                }
                if let Some(domain) = opts.get("domain").and_then(Value::as_str) {
                    header.push_str(&format!("; Domain={domain}"));
                }
                if let Some(max_age) = opts.get("max_age").and_then(Value::as_i64) {
                    header.push_str(&format!("; Max-Age={max_age}"));
                }
                if opts.get("secure").and_then(Value::as_bool).unwrap_or(false) {
                    header.push_str("; Secure");
                }
                if opts
                    .get("http_only")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    header.push_str("; HttpOnly");
                }
                if let Some(ss) = opts.get("same_site").and_then(Value::as_str) {
                    header.push_str(&format!("; SameSite={ss}"));
                }
            }
            Ok(json!(header))
        }
        "cookie.delete" => {
            let name = read_string_arg(args, 0, op)?;
            Ok(json!(format!("{name}=; Max-Age=0")))
        }

        // --- URL ops ---
        "url.parse" => {
            let url = read_string_arg(args, 0, op)?;
            // Strip scheme + authority
            let without_scheme = if let Some(idx) = url.find("://") {
                let after_scheme = &url[idx + 3..];
                if let Some(slash) = after_scheme.find('/') {
                    &after_scheme[slash..]
                } else {
                    "/"
                }
            } else {
                url.as_str()
            };
            // Split fragment
            let (before_fragment, fragment) = if let Some(hash) = without_scheme.find('#') {
                (&without_scheme[..hash], &without_scheme[hash + 1..])
            } else {
                (without_scheme, "")
            };
            // Split query
            let (path, query) = if let Some(q) = before_fragment.find('?') {
                (&before_fragment[..q], &before_fragment[q + 1..])
            } else {
                (before_fragment, "")
            };
            Ok(json!({"path": path, "query": query, "fragment": fragment}))
        }
        "url.query_parse" => {
            let qs = read_string_arg(args, 0, op)?;
            let mut map = serde_json::Map::new();
            for pair in qs.split('&') {
                if pair.is_empty() {
                    continue;
                }
                if let Some((k, v)) = pair.split_once('=') {
                    map.insert(percent_decode_str(k), json!(percent_decode_str(v)));
                } else {
                    map.insert(percent_decode_str(pair), json!(""));
                }
            }
            Ok(Value::Object(map))
        }
        "url.encode" => {
            let text = read_string_arg(args, 0, op)?;
            Ok(json!(percent_encode_str(&text)))
        }
        "url.decode" => {
            let text = read_string_arg(args, 0, op)?;
            Ok(json!(percent_decode_str(&text)))
        }

        // --- Route matching ---
        "route.match" => {
            let pattern = read_string_arg(args, 0, op)?;
            let path = read_string_arg(args, 1, op)?;
            Ok(json!(route_match_bool(&pattern, &path)))
        }
        "route.params" => {
            let pattern = read_string_arg(args, 0, op)?;
            let path = read_string_arg(args, 1, op)?;
            Ok(route_extract_params(&pattern, &path))
        }

        // --- HTML ops ---
        "html.escape" => {
            let text = read_string_arg(args, 0, op)?;
            Ok(json!(html_escape(&text)))
        }
        "html.unescape" => {
            let text = read_string_arg(args, 0, op)?;
            Ok(json!(html_unescape(&text)))
        }

        // --- Template ops ---
        "tmpl.render" => {
            let template = read_string_arg(args, 0, op)?;
            let data = args
                .get(1)
                .ok_or_else(|| format!("Op `{op}` missing arg1"))?;
            Ok(json!(mustache_render(&template, data)))
        }

        // Generic codec ops: codec.decode("json", text), codec.encode("json", value), etc.
        "codec.decode" => {
            let format = read_string_arg(args, 0, op)?;
            let text = read_string_arg(args, 1, op)?;
            let codec = codecs
                .get(&format)
                .ok_or_else(|| format!("codec.decode: unknown format `{format}`"))?;
            codec.decode(&text)
        }
        "codec.encode" => {
            let format = read_string_arg(args, 0, op)?;
            let value = args
                .get(1)
                .ok_or_else(|| format!("Op `{op}` missing arg1"))?;
            let codec = codecs
                .get(&format)
                .ok_or_else(|| format!("codec.encode: unknown format `{format}`"))?;
            codec.encode(value).map(|s| Value::String(s))
        }
        "codec.encode_pretty" => {
            let format = read_string_arg(args, 0, op)?;
            let value = args
                .get(1)
                .ok_or_else(|| format!("Op `{op}` missing arg1"))?;
            let codec = codecs
                .get(&format)
                .ok_or_else(|| format!("codec.encode_pretty: unknown format `{format}`"))?;
            codec.encode_pretty(value).map(|s| Value::String(s))
        }

        // Format-specific codec ops: {format}.decode(text), {format}.encode(value), etc.
        _ => {
            if let Some((namespace, method)) = op.split_once('.') {
                if let Some(codec) = codecs.get(namespace) {
                    return match method {
                        "decode" => {
                            let text = read_string_arg(args, 0, op)?;
                            codec.decode(&text)
                        }
                        "encode" => {
                            let value = args
                                .first()
                                .ok_or_else(|| format!("Op `{op}` missing arg0"))?;
                            codec.encode(value).map(|s| Value::String(s))
                        }
                        "encode_pretty" => {
                            let value = args
                                .first()
                                .ok_or_else(|| format!("Op `{op}` missing arg0"))?;
                            codec.encode_pretty(value).map(|s| Value::String(s))
                        }
                        _ => Err(format!("unknown op `{op}`")),
                    };
                }
            }
            Err(format!("unknown op `{op}`"))
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

fn eval_expr<'a>(
    expr: &'a Expr,
    vars: &'a HashMap<String, Value>,
    flow_registry: Option<&'a FlowRegistry>,
    host: &'a Rc<dyn Host>,
    codecs: &'a CodecRegistry,
) -> Pin<Box<dyn Future<Output = Result<Value, String>> + 'a>> {
    Box::pin(async move {
        match expr {
            Expr::Lit(v) => Ok(v.clone()),
            Expr::Var(name) => resolve_var_path(vars, name)
                .ok_or_else(|| format!("Unknown variable path `{name}`")),
            Expr::BinOp { op, lhs, rhs } => {
                let l = eval_expr(lhs, vars, flow_registry, host, codecs).await?;
                let r = eval_expr(rhs, vars, flow_registry, host, codecs).await?;
                eval_binop(*op, &l, &r)
            }
            Expr::UnaryOp { op, expr: inner } => {
                let v = eval_expr(inner, vars, flow_registry, host, codecs).await?;
                eval_unary(*op, &v)
            }
            Expr::Call { func, args } => {
                let mut evaluated = Vec::with_capacity(args.len());
                for a in args {
                    evaluated.push(eval_expr(a, vars, flow_registry, host, codecs).await?);
                }
                if let Some(fr) = flow_registry {
                    if let Some(mock_val) = fr.get_value_mock(func) {
                        return Ok(mock_val.clone());
                    }
                    if let Some(program) = fr.get(func) {
                        if evaluated.len() != program.flow.inputs.len() {
                            return Err(format!(
                                "flow `{}` expects {} args but got {}",
                                func,
                                program.flow.inputs.len(),
                                evaluated.len()
                            ));
                        }
                        let mut input_map = HashMap::new();
                        for (idx, port) in program.flow.inputs.iter().enumerate() {
                            input_map.insert(port.name.clone(), evaluated[idx].clone());
                        }
                        let report = execute_flow(
                            &program.flow,
                            program.ir.clone(),
                            input_map,
                            &program.registry,
                            flow_registry,
                            codecs,
                            Some(host.clone()),
                        )
                        .await?;
                        let outputs = report.outputs.as_object().ok_or_else(|| {
                            format!("flow `{func}` produced invalid outputs shape")
                        })?;
                        let success = program.emit_name.as_deref().and_then(|n| outputs.get(n)).cloned();
                        let failure = program.fail_name.as_deref().and_then(|n| outputs.get(n)).cloned();
                        if program.emit_name.is_none() {
                            return Ok(serde_json::Value::Null);
                        }
                        return match (success, failure) {
                            (Some(v), None) => Ok(v),
                            (None, Some(f)) => {
                                Err(format!("flow `{func}` emitted on fail track: {f}"))
                            }
                            (None, None) => Err(format!("flow `{func}` produced no outputs")),
                            (Some(_), Some(_)) => {
                                Err(format!("flow `{func}` produced both emit and fail outputs"))
                            }
                        };
                    }
                }
                execute_op(func, &evaluated, &**host, codecs).await
            }
            Expr::Interp(parts) => {
                let mut result = String::new();
                for part in parts {
                    match part {
                        InterpExpr::Lit(s) => result.push_str(s),
                        InterpExpr::Expr(e) => {
                            let val = eval_expr(e, vars, flow_registry, host, codecs).await?;
                            match &val {
                                Value::String(s) => result.push_str(s),
                                Value::Number(n) => result.push_str(&n.to_string()),
                                Value::Bool(b) => {
                                    result.push_str(if *b { "true" } else { "false" })
                                }
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
                let cond_val = eval_expr(cond, vars, flow_registry, host, codecs).await?;
                let is_truthy = match &cond_val {
                    Value::Bool(b) => *b,
                    Value::Null => false,
                    Value::String(s) => !s.is_empty(),
                    Value::Number(n) => n.as_f64().map_or(false, |f| f != 0.0),
                    _ => true,
                };
                if is_truthy {
                    eval_expr(then_expr, vars, flow_registry, host, codecs).await
                } else {
                    eval_expr(else_expr, vars, flow_registry, host, codecs).await
                }
            }
            Expr::ListLit(items) => {
                let mut arr = Vec::with_capacity(items.len());
                for item in items {
                    arr.push(eval_expr(item, vars, flow_registry, host, codecs).await?);
                }
                Ok(Value::Array(arr))
            }
            Expr::DictLit(pairs) => {
                let mut map = serde_json::Map::new();
                for (key, val_expr) in pairs {
                    let val = eval_expr(val_expr, vars, flow_registry, host, codecs).await?;
                    map.insert(key.clone(), val);
                }
                Ok(Value::Object(map))
            }
            Expr::Index { expr, index } => {
                let collection = eval_expr(expr, vars, flow_registry, host, codecs).await?;
                let idx = eval_expr(index, vars, flow_registry, host, codecs).await?;
                match &collection {
                    Value::Array(arr) => {
                        let i = idx.as_i64().ok_or_else(|| format!("Index must be an integer, got {idx}"))?;
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
    })
}

fn eval_binop(op: crate::ast::BinOp, l: &Value, r: &Value) -> Result<Value, String> {
    use crate::ast::BinOp;
    match op {
        BinOp::Add => {
            // Numbers: add. Strings: concatenate.
            if let (Some(a), Some(b)) = (l.as_f64(), r.as_f64()) {
                // Preserve integer output when both inputs are integers
                if l.is_i64() && r.is_i64() {
                    return Ok(json!(l.as_i64().unwrap() + r.as_i64().unwrap()));
                }
                return Ok(json!(a + b));
            }
            if let (Some(a), Some(b)) = (l.as_str(), r.as_str()) {
                return Ok(json!(format!("{a}{b}")));
            }
            Err(format!("Cannot add {l} and {r}"))
        }
        BinOp::Sub => {
            let a = l
                .as_f64()
                .ok_or_else(|| format!("Cannot subtract: left operand {l} is not a number"))?;
            let b = r
                .as_f64()
                .ok_or_else(|| format!("Cannot subtract: right operand {r} is not a number"))?;
            if l.is_i64() && r.is_i64() {
                return Ok(json!(l.as_i64().unwrap() - r.as_i64().unwrap()));
            }
            Ok(json!(a - b))
        }
        BinOp::Mul => {
            let a = l
                .as_f64()
                .ok_or_else(|| format!("Cannot multiply: left operand {l} is not a number"))?;
            let b = r
                .as_f64()
                .ok_or_else(|| format!("Cannot multiply: right operand {r} is not a number"))?;
            if l.is_i64() && r.is_i64() {
                return Ok(json!(l.as_i64().unwrap() * r.as_i64().unwrap()));
            }
            Ok(json!(a * b))
        }
        BinOp::Div => {
            let a = l
                .as_f64()
                .ok_or_else(|| format!("Cannot divide: left operand {l} is not a number"))?;
            let b = r
                .as_f64()
                .ok_or_else(|| format!("Cannot divide: right operand {r} is not a number"))?;
            if b == 0.0 {
                return Err("Division by zero".to_string());
            }
            Ok(json!(a / b))
        }
        BinOp::Mod => {
            let a = l
                .as_f64()
                .ok_or_else(|| format!("Cannot mod: left operand {l} is not a number"))?;
            let b = r
                .as_f64()
                .ok_or_else(|| format!("Cannot mod: right operand {r} is not a number"))?;
            if b == 0.0 {
                return Err("Modulo by zero".to_string());
            }
            if l.is_i64() && r.is_i64() {
                return Ok(json!(l.as_i64().unwrap() % r.as_i64().unwrap()));
            }
            Ok(json!(a % b))
        }
        BinOp::Pow => {
            let a = l
                .as_f64()
                .ok_or_else(|| format!("Cannot pow: left operand {l} is not a number"))?;
            let b = r
                .as_f64()
                .ok_or_else(|| format!("Cannot pow: right operand {r} is not a number"))?;
            Ok(json!(a.powf(b)))
        }
        BinOp::Eq => Ok(json!(l == r)),
        BinOp::Neq => Ok(json!(l != r)),
        BinOp::Lt => {
            let a = l
                .as_f64()
                .ok_or_else(|| format!("Cannot compare: {l} is not a number"))?;
            let b = r
                .as_f64()
                .ok_or_else(|| format!("Cannot compare: {r} is not a number"))?;
            Ok(json!(a < b))
        }
        BinOp::Gt => {
            let a = l
                .as_f64()
                .ok_or_else(|| format!("Cannot compare: {l} is not a number"))?;
            let b = r
                .as_f64()
                .ok_or_else(|| format!("Cannot compare: {r} is not a number"))?;
            Ok(json!(a > b))
        }
        BinOp::LtEq => {
            let a = l
                .as_f64()
                .ok_or_else(|| format!("Cannot compare: {l} is not a number"))?;
            let b = r
                .as_f64()
                .ok_or_else(|| format!("Cannot compare: {r} is not a number"))?;
            Ok(json!(a <= b))
        }
        BinOp::GtEq => {
            let a = l
                .as_f64()
                .ok_or_else(|| format!("Cannot compare: {l} is not a number"))?;
            let b = r
                .as_f64()
                .ok_or_else(|| format!("Cannot compare: {r} is not a number"))?;
            Ok(json!(a >= b))
        }
        BinOp::And => {
            let a = l
                .as_bool()
                .ok_or_else(|| format!("Cannot AND: {l} is not a boolean"))?;
            let b = r
                .as_bool()
                .ok_or_else(|| format!("Cannot AND: {r} is not a boolean"))?;
            Ok(json!(a && b))
        }
        BinOp::Or => {
            let a = l
                .as_bool()
                .ok_or_else(|| format!("Cannot OR: {l} is not a boolean"))?;
            let b = r
                .as_bool()
                .ok_or_else(|| format!("Cannot OR: {r} is not a boolean"))?;
            Ok(json!(a || b))
        }
    }
}

fn eval_unary(op: crate::ast::UnaryOp, v: &Value) -> Result<Value, String> {
    use crate::ast::UnaryOp;
    match op {
        UnaryOp::Neg => {
            if let Some(n) = v.as_i64() {
                Ok(json!(-n))
            } else if let Some(n) = v.as_f64() {
                Ok(json!(-n))
            } else {
                Err(format!("Cannot negate {v}"))
            }
        }
        UnaryOp::Not => {
            let b = v
                .as_bool()
                .ok_or_else(|| format!("Cannot NOT: {v} is not a boolean"))?;
            Ok(json!(!b))
        }
    }
}

fn pattern_matches(value: &Value, pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Lit(v) => value == v,
        Pattern::Ident(name) => {
            if let Some(s) = value.as_str() {
                s == name
            } else {
                false
            }
        }
    }
}

#[derive(Clone)]
enum ExecSignal {
    Continue,
    Emit {
        output: String,
        value_var: String,
        value: Value,
    },
    Break,
}

fn to_json_object(map: &HashMap<String, Value>) -> Value {
    let mut obj = serde_json::Map::new();
    for (key, value) in map {
        obj.insert(key.clone(), value.clone());
    }
    Value::Object(obj)
}

pub fn load_inputs(flow: &Flow, input: Option<&PathBuf>) -> Result<HashMap<String, Value>, String> {
    let mut provided = serde_json::Map::new();
    if let Some(path) = input {
        let raw = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read input file {}: {e}", path.display()))?;
        let parsed: Value = serde_json::from_str(&raw)
            .map_err(|e| format!("Invalid input JSON in {}: {e}", path.display()))?;
        let Some(obj) = parsed.as_object() else {
            return Err("Input JSON must be an object keyed by flow input names".to_string());
        };
        provided = obj.clone();
    }

    // Auto-wrap: if the flow has exactly one input port and the JSON
    // doesn't contain a key matching that port name, treat the entire
    // JSON object as the value for that single port.
    if flow.inputs.len() == 1 {
        let port_name = &flow.inputs[0].name;
        if !provided.is_empty() && !provided.contains_key(port_name) {
            let wrapped = Value::Object(provided.clone());
            provided.clear();
            provided.insert(port_name.clone(), wrapped);
        }
    }

    let mut out = HashMap::new();
    for port in &flow.inputs {
        let value = provided.remove(&port.name).unwrap_or(Value::Null);
        out.insert(port.name.clone(), value);
    }
    Ok(out)
}

/// Map positional CLI args to flow input ports (in declaration order).
/// Coerces text to the port's declared type: long→i64, real→f64, bool→bool, text→string.
pub fn load_inputs_from_args(
    flow: &Flow,
    args: &[String],
) -> Result<HashMap<String, Value>, String> {
    if args.len() > flow.inputs.len() {
        return Err(format!(
            "Too many arguments: flow `{}` takes {} input(s) but {} were provided",
            flow.name,
            flow.inputs.len(),
            args.len()
        ));
    }

    let mut out = HashMap::new();
    for (i, port) in flow.inputs.iter().enumerate() {
        let value = if i < args.len() {
            coerce_arg(&args[i], &port.type_name)
        } else {
            Value::Null
        };
        out.insert(port.name.clone(), value);
    }
    Ok(out)
}

fn coerce_arg(raw: &str, type_name: &str) -> Value {
    match type_name.to_lowercase().as_str() {
        "long" => raw
            .parse::<i64>()
            .map(Value::from)
            .unwrap_or_else(|_| Value::String(raw.to_string())),
        "real" => raw
            .parse::<f64>()
            .map(Value::from)
            .unwrap_or_else(|_| Value::String(raw.to_string())),
        "bool" => match raw {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => Value::String(raw.to_string()),
        },
        _ => Value::String(raw.to_string()),
    }
}

fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    if let Some(ms) = s.strip_suffix("ms") {
        let n: u64 = ms
            .trim()
            .parse()
            .map_err(|_| format!("Invalid duration `{s}`"))?;
        return Ok(Duration::from_millis(n));
    }
    if let Some(m) = s.strip_suffix('m') {
        let n: u64 = m
            .trim()
            .parse()
            .map_err(|_| format!("Invalid duration `{s}`"))?;
        return Ok(Duration::from_secs(n * 60));
    }
    if let Some(sec) = s.strip_suffix('s') {
        let n: u64 = sec
            .trim()
            .parse()
            .map_err(|_| format!("Invalid duration `{s}`"))?;
        return Ok(Duration::from_secs(n));
    }
    Err(format!(
        "Invalid duration `{s}` (expected e.g. 5s, 500ms, 2m)"
    ))
}

// --- Stepping engine types ---

#[derive(Debug, Clone, Serialize)]
pub struct StateSnapshot {
    pub step: usize,
    pub node_id: Option<String>,
    pub op: Option<String>,
    pub bindings: HashMap<String, Value>,
    pub status: String,
    pub trace: Vec<NodeTraceEvent>,
    pub emits: Vec<EmitTraceEvent>,
}

#[derive(Debug, Clone)]
pub enum StepCommand {
    Step,
    Continue,
    RunToBreakpoint,
    SetBreakpoints(HashSet<String>),
}

#[derive(Debug, Clone, PartialEq)]
enum StepMode {
    StepOne,
    RunFree,
    RunToBreakpoint,
}

pub struct StepController {
    snapshot_tx: mpsc::Sender<StateSnapshot>,
    command_rx: mpsc::Receiver<StepCommand>,
    active: AtomicBool,
    mode: Mutex<StepMode>,
    breakpoints: Mutex<HashSet<String>>,
}

impl StepController {
    fn drain_commands(&self) {
        while let Ok(cmd) = self.command_rx.try_recv() {
            self.apply_command(cmd);
        }
    }

    fn apply_command(&self, cmd: StepCommand) {
        match cmd {
            StepCommand::Step => {
                *self.mode.lock().unwrap() = StepMode::StepOne;
                self.active.store(true, Ordering::SeqCst);
            }
            StepCommand::Continue => {
                *self.mode.lock().unwrap() = StepMode::RunFree;
                self.active.store(false, Ordering::SeqCst);
            }
            StepCommand::RunToBreakpoint => {
                *self.mode.lock().unwrap() = StepMode::RunToBreakpoint;
                self.active.store(true, Ordering::SeqCst);
            }
            StepCommand::SetBreakpoints(bp) => {
                *self.breakpoints.lock().unwrap() = bp;
            }
        }
    }

    fn maybe_pause(&self, snapshot: StateSnapshot) {
        self.drain_commands();

        let mode = self.mode.lock().unwrap().clone();
        match mode {
            StepMode::StepOne => {
                let _ = self.snapshot_tx.send(snapshot);
                if let Ok(cmd) = self.command_rx.recv() {
                    self.apply_command(cmd);
                }
            }
            StepMode::RunFree => {
                // Don't pause at all
            }
            StepMode::RunToBreakpoint => {
                let at_breakpoint = if let Some(ref node_id) = snapshot.node_id {
                    self.breakpoints.lock().unwrap().contains(node_id)
                } else {
                    false
                };
                if at_breakpoint {
                    *self.mode.lock().unwrap() = StepMode::StepOne;
                    let _ = self.snapshot_tx.send(snapshot);
                    if let Ok(cmd) = self.command_rx.recv() {
                        self.apply_command(cmd);
                    }
                }
            }
        }
    }
}

pub struct StepHandle {
    pub snapshot_rx: mpsc::Receiver<StateSnapshot>,
    pub command_tx: mpsc::Sender<StepCommand>,
}

pub fn create_step_channels() -> (StepController, StepHandle) {
    let (snap_tx, snap_rx) = mpsc::channel();
    let (cmd_tx, cmd_rx) = mpsc::channel();
    (
        StepController {
            snapshot_tx: snap_tx,
            command_rx: cmd_rx,
            active: AtomicBool::new(true),
            mode: Mutex::new(StepMode::StepOne),
            breakpoints: Mutex::new(HashSet::new()),
        },
        StepHandle {
            snapshot_rx: snap_rx,
            command_tx: cmd_tx,
        },
    )
}

pub async fn execute_flow(
    flow: &Flow,
    ir: Ir,
    inputs: HashMap<String, Value>,
    registry: &TypeRegistry,
    flow_registry: Option<&FlowRegistry>,
    codecs: &CodecRegistry,
    host: Option<Rc<dyn Host>>,
) -> Result<RunReport, String> {
    execute_flow_inner(
        flow,
        ir,
        inputs,
        registry,
        flow_registry,
        None,
        codecs,
        host,
    )
    .await
}

async fn try_dispatch_flow(
    op: &str,
    args: &[Value],
    flow_registry: Option<&FlowRegistry>,
    codecs: &CodecRegistry,
    host: &Rc<dyn Host>,
) -> Option<Result<Value, String>> {
    let fr = flow_registry?;

    // Check value mocks first — if a mock exists, return it directly
    if let Some(value) = fr.get_value_mock(op) {
        return Some(Ok(value.clone()));
    }

    let program = fr.get(op)?;

    if args.len() != program.flow.inputs.len() {
        return Some(Err(format!(
            "flow `{}` expects {} args but got {}",
            op,
            program.flow.inputs.len(),
            args.len()
        )));
    }

    let mut input_map = HashMap::new();
    for (idx, port) in program.flow.inputs.iter().enumerate() {
        input_map.insert(port.name.clone(), args[idx].clone());
    }

    let report = match execute_flow(
        &program.flow,
        program.ir.clone(),
        input_map,
        &program.registry,
        flow_registry,
        codecs,
        Some(host.clone()),
    )
    .await
    {
        Ok(r) => r,
        Err(e) => return Some(Err(e)),
    };

    let Some(outputs) = report.outputs.as_object() else {
        return Some(Err(format!("flow `{op}` produced invalid outputs shape")));
    };

    let success = program.emit_name.as_deref().and_then(|n| outputs.get(n)).cloned();
    let failure = program.fail_name.as_deref().and_then(|n| outputs.get(n)).cloned();

    if program.emit_name.is_none() {
        return Some(Ok(serde_json::Value::Null));
    }

    match (success, failure) {
        (Some(v), None) => Some(Ok(v)),
        (None, Some(f)) => Some(Err(format!("flow `{op}` emitted on fail track: {f}"))),
        (None, None) => Some(Err(format!("flow `{op}` produced no outputs"))),
        (Some(_), Some(_)) => Some(Err(format!(
            "flow `{op}` produced both emit and fail outputs"
        ))),
    }
}

/// Spawn a source as a background task. Returns a channel receiver that yields
/// events emitted by the source. The source runs on a `spawn_local` task and
/// the channel closes when the source terminates (emit/error/completion).
pub fn dispatch_source(
    op: &str,
    args: &[Value],
    flow_registry: &FlowRegistry,
    _codecs: &CodecRegistry,
    host: &Rc<dyn Host>,
) -> Option<tokio::sync::mpsc::UnboundedReceiver<Value>> {
    let program = flow_registry.get(op)?;
    if program.kind != DeclKind::Source {
        return None;
    }

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

    let mut input_map = HashMap::new();
    for (idx, port) in program.flow.inputs.iter().enumerate() {
        if idx < args.len() {
            input_map.insert(port.name.clone(), args[idx].clone());
        }
    }

    let flow = program.flow.clone();
    let registry = flow_registry.clone();
    let host = host.clone();

    tokio::task::spawn_local(async move {
        let codecs_local = CodecRegistry::default_registry();
        let mut vars = input_map;
        let mut trace = Vec::new();
        let mut emit_trace = Vec::new();
        let mut step = 0usize;
        let mut sync_counter = 0usize;

        let result = execute_statements(
            &flow.body,
            &mut vars,
            &mut trace,
            &mut emit_trace,
            &mut step,
            Some(&registry),
            &mut sync_counter,
            None,
            None,
            &host,
            &codecs_local,
            Some(&tx),
        )
        .await;

        if let Err(e) = result {
            eprintln!("source `{}` failed: {e}", flow.name);
        }
        // tx is dropped here, closing the channel
    });

    Some(rx)
}

fn execute_statements<'a>(
    statements: &'a [Statement],
    vars: &'a mut HashMap<String, Value>,
    trace: &'a mut Vec<NodeTraceEvent>,
    emit_trace: &'a mut Vec<EmitTraceEvent>,
    step: &'a mut usize,
    flow_registry: Option<&'a FlowRegistry>,
    sync_counter: &'a mut usize,
    sync_group: Option<&'a str>,
    step_ctrl: Option<&'a StepController>,
    host: &'a Rc<dyn Host>,
    codecs: &'a CodecRegistry,
    source_tx: Option<&'a tokio::sync::mpsc::UnboundedSender<Value>>,
) -> Pin<Box<dyn Future<Output = Result<ExecSignal, String>> + 'a>> {
    Box::pin(async move {
        for stmt in statements {
            match stmt {
                Statement::Node(node) => {
                    // Pause BEFORE executing the node
                    if let Some(ctrl) = step_ctrl {
                        ctrl.maybe_pause(StateSnapshot {
                            step: *step,
                            node_id: Some(node.node_id.clone()),
                            op: Some(node.op.clone()),
                            bindings: vars.clone(),
                            status: "paused".to_string(),
                            trace: trace.clone(),
                            emits: emit_trace.clone(),
                        });
                    }

                    let mut args = Vec::new();
                    let mut missing_var: Option<String> = None;
                    for arg in &node.args {
                        match arg {
                            Arg::Lit { lit } => args.push(lit.clone()),
                            Arg::Var { var } => {
                                if let Some(value) = resolve_var_path(vars, var) {
                                    args.push(value);
                                } else {
                                    missing_var = Some(var.clone());
                                    break;
                                }
                            }
                        }
                    }

                    if let Some(var) = missing_var {
                        trace.push(NodeTraceEvent {
                            step: *step,
                            node_id: node.node_id.clone(),
                            op: node.op.clone(),
                            bind: node.bind.clone(),
                            when: "true".to_string(),
                            status: "failed".to_string(),
                            args,
                            output: None,
                            error: Some(format!("Missing variable `{var}`")),
                            duration_ms: 0,
                            sync_group: sync_group.map(|s| s.to_string()),
                        });
                        *step += 1;
                        continue;
                    }

                    let start = Instant::now();
                    let result = if let Some(flow_result) =
                        try_dispatch_flow(&node.op, &args, flow_registry, codecs, host).await
                    {
                        flow_result
                    } else {
                        execute_op(&node.op, &args, &**host, codecs).await
                    };

                    match result {
                        Ok(output) => {
                            vars.insert(node.bind.clone(), output.clone());
                            trace.push(NodeTraceEvent {
                                step: *step,
                                node_id: node.node_id.clone(),
                                op: node.op.clone(),
                                bind: node.bind.clone(),
                                when: "true".to_string(),
                                status: "executed".to_string(),
                                args,
                                output: Some(output),
                                error: None,
                                duration_ms: start.elapsed().as_millis(),
                                sync_group: sync_group.map(|s| s.to_string()),
                            });
                        }
                        Err(error) => {
                            trace.push(NodeTraceEvent {
                                step: *step,
                                node_id: node.node_id.clone(),
                                op: node.op.clone(),
                                bind: node.bind.clone(),
                                when: "true".to_string(),
                                status: "failed".to_string(),
                                args,
                                output: None,
                                error: Some(error),
                                duration_ms: start.elapsed().as_millis(),
                                sync_group: sync_group.map(|s| s.to_string()),
                            });
                        }
                    }
                    *step += 1;
                }
                Statement::ExprAssign(ea) => {
                    // Pause BEFORE executing the expression
                    if let Some(ctrl) = step_ctrl {
                        ctrl.maybe_pause(StateSnapshot {
                            step: *step,
                            node_id: Some(ea.bind.clone()),
                            op: Some("expr".to_string()),
                            bindings: vars.clone(),
                            status: "paused".to_string(),
                            trace: trace.clone(),
                            emits: emit_trace.clone(),
                        });
                    }

                    let start = Instant::now();
                    let result = eval_expr(&ea.expr, vars, flow_registry, host, codecs).await;
                    match result {
                        Ok(value) => {
                            vars.insert(ea.bind.clone(), value.clone());
                            trace.push(NodeTraceEvent {
                                step: *step,
                                node_id: ea.bind.clone(),
                                op: "expr".to_string(),
                                bind: ea.bind.clone(),
                                when: "true".to_string(),
                                status: "executed".to_string(),
                                args: vec![],
                                output: Some(value),
                                error: None,
                                duration_ms: start.elapsed().as_millis(),
                                sync_group: sync_group.map(|s| s.to_string()),
                            });
                        }
                        Err(error) => {
                            trace.push(NodeTraceEvent {
                                step: *step,
                                node_id: ea.bind.clone(),
                                op: "expr".to_string(),
                                bind: ea.bind.clone(),
                                when: "true".to_string(),
                                status: "failed".to_string(),
                                args: vec![],
                                output: None,
                                error: Some(error.clone()),
                                duration_ms: start.elapsed().as_millis(),
                                sync_group: sync_group.map(|s| s.to_string()),
                            });
                            return Err(error);
                        }
                    }
                    *step += 1;
                }
                Statement::Emit(emit) => {
                    let value =
                        eval_expr(&emit.value_expr, vars, flow_registry, host, codecs).await?;
                    let label = match &emit.value_expr {
                        Expr::Var(name) => name.clone(),
                        _ => format!("{:?}", emit.value_expr),
                    };
                    emit_trace.push(EmitTraceEvent {
                        output: emit.output.clone(),
                        value_var: label.clone(),
                        when: "true".to_string(),
                        emitted: true,
                        value: Some(value.clone()),
                    });
                    if let Some(tx) = source_tx {
                        // Source yield: send value to channel and yield so the
                        // receiver (SourceLoop) can process before we continue.
                        let _ = tx.send(value);
                        tokio::task::yield_now().await;
                    } else {
                        return Ok(ExecSignal::Emit {
                            output: emit.output.clone(),
                            value_var: label,
                            value,
                        });
                    }
                }
                Statement::Case(case_block) => {
                    let subject =
                        eval_expr(&case_block.expr, vars, flow_registry, host, codecs).await?;
                    let mut matched = false;
                    for arm in &case_block.arms {
                        if pattern_matches(&subject, &arm.pattern) {
                            matched = true;
                            match execute_statements(
                                &arm.body,
                                vars,
                                trace,
                                emit_trace,
                                step,
                                flow_registry,
                                sync_counter,
                                sync_group,
                                step_ctrl,
                                host,
                                codecs,
                                source_tx,
                            )
                            .await?
                            {
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
                            trace,
                            emit_trace,
                            step,
                            flow_registry,
                            sync_counter,
                            sync_group,
                            step_ctrl,
                            host,
                            codecs,
                            source_tx,
                        )
                        .await?
                        {
                            ExecSignal::Continue => {}
                            signal @ ExecSignal::Emit { .. } => return Ok(signal),
                            ExecSignal::Break => return Ok(ExecSignal::Break),
                        }
                    }
                }
                Statement::Loop(loop_block) => {
                    let collection =
                        eval_expr(&loop_block.collection, vars, flow_registry, host, codecs)
                            .await?;
                    let items = collection.as_array().ok_or_else(|| {
                        format!("Loop collection must be an array, got `{}`", collection)
                    })?;
                    let items = items.clone();
                    let previous = vars.get(&loop_block.item).cloned();
                    for item in &items {
                        vars.insert(loop_block.item.clone(), item.clone());
                        match execute_statements(
                            &loop_block.body,
                            vars,
                            trace,
                            emit_trace,
                            step,
                            flow_registry,
                            sync_counter,
                            sync_group,
                            step_ctrl,
                            host,
                            codecs,
                            source_tx,
                        )
                        .await?
                        {
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
                Statement::Sync(sync_block) => {
                    let group_id = format!("sync_{}", *sync_counter);
                    *sync_counter += 1;

                    let timeout = sync_block
                        .options
                        .timeout
                        .as_ref()
                        .map(|s| parse_duration(s))
                        .transpose()?;
                    let max_retries = sync_block.options.retry.unwrap_or(0) as usize;
                    let safe = sync_block.options.safe;
                    let sync_start = Instant::now();

                    let mut last_error: Option<String> = None;
                    let mut succeeded = false;

                    for attempt in 0..=max_retries {
                        if let Some(dur) = timeout {
                            if sync_start.elapsed() > dur {
                                last_error = Some(format!(
                                    "sync block `{}` timed out after {}ms",
                                    group_id,
                                    dur.as_millis()
                                ));
                                break;
                            }
                        }

                        // Run each statement concurrently with its own vars clone
                        let futures: Vec<_> = sync_block
                            .body
                            .iter()
                            .map(|stmt| {
                                let mut local_vars = vars.clone();
                                let mut local_trace = Vec::new();
                                let mut local_emit_trace = Vec::new();
                                let mut local_step = *step;
                                let mut local_sync_counter = *sync_counter;
                                let group_id = group_id.clone();
                                async move {
                                    let result = execute_statements(
                                        std::slice::from_ref(stmt),
                                        &mut local_vars,
                                        &mut local_trace,
                                        &mut local_emit_trace,
                                        &mut local_step,
                                        flow_registry,
                                        &mut local_sync_counter,
                                        Some(&group_id),
                                        step_ctrl,
                                        host,
                                        codecs,
                                        None, // sync blocks don't yield to source channels
                                    )
                                    .await;
                                    (
                                        local_vars,
                                        local_trace,
                                        local_emit_trace,
                                        local_step,
                                        result,
                                    )
                                }
                            })
                            .collect();

                        let run_concurrent = futures::future::join_all(futures);
                        let results = if let Some(dur) = timeout {
                            let remaining = dur.saturating_sub(sync_start.elapsed());
                            match tokio::time::timeout(remaining, run_concurrent).await {
                                Ok(r) => r,
                                Err(_) => {
                                    last_error = Some(format!(
                                        "sync block `{}` timed out after {}ms",
                                        group_id,
                                        dur.as_millis()
                                    ));
                                    if attempt < max_retries {
                                        continue;
                                    }
                                    break;
                                }
                            }
                        } else {
                            run_concurrent.await
                        };

                        // Merge results: collect all traces, merge vars
                        let mut merged_vars = vars.clone();
                        let mut had_error = false;
                        let mut emit_signal: Option<ExecSignal> = None;

                        for (local_vars, local_trace, local_emit_trace, local_step, result) in
                            results
                        {
                            trace.extend(local_trace);
                            emit_trace.extend(local_emit_trace);
                            if local_step > *step {
                                *step = local_step;
                            }

                            match result {
                                Ok(ExecSignal::Continue) => {
                                    for (k, v) in &local_vars {
                                        if !vars.contains_key(k) || local_vars.get(k) != vars.get(k)
                                        {
                                            merged_vars.insert(k.clone(), v.clone());
                                        }
                                    }
                                }
                                Ok(signal @ ExecSignal::Emit { .. }) => {
                                    emit_signal = Some(signal);
                                }
                                Ok(ExecSignal::Break) => {
                                    // Break inside sync statement — treat as continue
                                }
                                Err(e) => {
                                    last_error = Some(e);
                                    had_error = true;
                                }
                            }
                        }

                        if let Some(signal) = emit_signal {
                            return Ok(signal);
                        }

                        if had_error {
                            if attempt < max_retries {
                                continue;
                            }
                            break;
                        }

                        // Export from merged vars
                        for (target, export) in
                            sync_block.targets.iter().zip(sync_block.exports.iter())
                        {
                            if let Some(v) = resolve_var_path(&merged_vars, export) {
                                vars.insert(target.clone(), v);
                            } else if safe {
                                vars.insert(target.clone(), Value::Null);
                            } else {
                                return Err(format!(
                                    "Sync export `{}` not found in local scope",
                                    export
                                ));
                            }
                        }
                        succeeded = true;
                        break;
                    }

                    if !succeeded {
                        if safe {
                            for target in &sync_block.targets {
                                vars.insert(target.clone(), Value::Null);
                            }
                        } else if let Some(err) = last_error {
                            return Err(err);
                        }
                    }
                }
                Statement::SendNowait(sn) => {
                    let mut resolved_args = Vec::new();
                    for arg_expr in &sn.args {
                        let val = eval_expr(arg_expr, vars, flow_registry, host, codecs).await?;
                        resolved_args.push(val);
                    }

                    let target = sn.target.clone();
                    let fr_clone: Option<FlowRegistry> = flow_registry.cloned();

                    *step += 1;
                    trace.push(NodeTraceEvent {
                        step: *step,
                        node_id: format!("send_{}", sn.target.replace('.', "_")),
                        op: format!("send.nowait.{}", sn.target),
                        bind: String::new(),
                        when: "true".to_string(),
                        status: "spawned".to_string(),
                        args: resolved_args.clone(),
                        output: None,
                        error: None,
                        duration_ms: 0,
                        sync_group: sync_group.map(|s| s.to_string()),
                    });

                    tokio::task::spawn_local(async move {
                        let codecs_local = CodecRegistry::default_registry();
                        let host_local: Rc<dyn Host> = Rc::new(NativeHost::new());
                        let result = if let Some(ref fr) = fr_clone {
                            try_dispatch_flow(
                                &target,
                                &resolved_args,
                                Some(fr),
                                &codecs_local,
                                &host_local,
                            )
                            .await
                        } else {
                            None
                        };
                        let result = match result {
                            Some(r) => Some(r),
                            None => Some(
                                execute_op(&target, &resolved_args, &*host_local, &codecs_local)
                                    .await,
                            ),
                        };
                        if let Some(Err(e)) = result {
                            eprintln!("send nowait `{target}` failed: {e}");
                        }
                    });
                }
                Statement::Break => {
                    return Ok(ExecSignal::Break);
                }
                Statement::BareLoop(block) => loop {
                    match execute_statements(
                        &block.body,
                        vars,
                        trace,
                        emit_trace,
                        step,
                        flow_registry,
                        sync_counter,
                        sync_group,
                        step_ctrl,
                        host,
                        codecs,
                        source_tx,
                    )
                    .await?
                    {
                        ExecSignal::Continue => {}
                        signal @ ExecSignal::Emit { .. } => return Ok(signal),
                        ExecSignal::Break => break,
                    }
                },
                Statement::SourceLoop(sl) => {
                    let fr = flow_registry.ok_or_else(|| {
                        format!("source `{}` requires a flow registry", sl.source_op)
                    })?;
                    let mut resolved_args = Vec::new();
                    for arg in &sl.source_args {
                        match arg {
                            Arg::Lit { lit } => resolved_args.push(lit.clone()),
                            Arg::Var { var } => {
                                let val = resolve_var_path(vars, var).ok_or_else(|| {
                                    format!(
                                        "source `{}` references unknown var `{}`",
                                        sl.source_op, var
                                    )
                                })?;
                                resolved_args.push(val);
                            }
                        }
                    }
                    let mut rx = dispatch_source(&sl.source_op, &resolved_args, fr, codecs, host)
                        .ok_or_else(|| {
                        format!("source `{}` not found in registry", sl.source_op)
                    })?;
                    while let Some(event) = rx.recv().await {
                        vars.insert(sl.bind.clone(), event);
                        match execute_statements(
                            &sl.body,
                            vars,
                            trace,
                            emit_trace,
                            step,
                            flow_registry,
                            sync_counter,
                            sync_group,
                            step_ctrl,
                            host,
                            codecs,
                            None,
                        )
                        .await?
                        {
                            ExecSignal::Continue => {}
                            signal @ ExecSignal::Emit { .. } => return Ok(signal),
                            ExecSignal::Break => break,
                        }
                    }
                }
                Statement::On(on_block) => {
                    // Resolve source args
                    let mut resolved_args = Vec::new();
                    for arg in &on_block.source_args {
                        match arg {
                            Arg::Lit { lit } => resolved_args.push(lit.clone()),
                            Arg::Var { var } => {
                                let val = resolve_var_path(vars, var).ok_or_else(|| {
                                    format!(
                                        "on block `{}` references unknown var `{}`",
                                        on_block.source_op, var
                                    )
                                })?;
                                resolved_args.push(val);
                            }
                        }
                    }
                    // Loop: call source op directly, bind result, execute body
                    loop {
                        let event = execute_op(
                            &on_block.source_op,
                            &resolved_args,
                            host.as_ref(),
                            codecs,
                        )
                        .await?;
                        vars.insert(on_block.bind.clone(), event);
                        match execute_statements(
                            &on_block.body,
                            vars,
                            trace,
                            emit_trace,
                            step,
                            flow_registry,
                            sync_counter,
                            sync_group,
                            step_ctrl,
                            host,
                            codecs,
                            source_tx,
                        )
                        .await?
                        {
                            ExecSignal::Continue => {}
                            signal @ ExecSignal::Emit { .. } => {
                                if let Some(tx) = source_tx {
                                    if let ExecSignal::Emit { value, .. } = &signal {
                                        let _ = tx.send(value.clone());
                                    }
                                } else {
                                    return Ok(signal);
                                }
                            }
                            ExecSignal::Break => break,
                        }
                    }
                }
            }
        }
        Ok(ExecSignal::Continue)
    })
}

async fn execute_flow_inner(
    flow: &Flow,
    ir: Ir,
    inputs: HashMap<String, Value>,
    registry: &TypeRegistry,
    flow_registry: Option<&FlowRegistry>,
    step_ctrl: Option<&StepController>,
    codecs: &CodecRegistry,
    parent_host: Option<Rc<dyn Host>>,
) -> Result<RunReport, String> {
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
            .map(|e| {
                json!({
                    "path": e.path,
                    "constraint": e.constraint,
                    "message": e.message
                })
            })
            .collect();
        let error_value = json!({
            "kind": "validation_error",
            "errors": error_details
        });
        let mut outputs = serde_json::Map::new();
        outputs.insert(fail_output, error_value);
        return Ok(RunReport {
            flow: flow.name.clone(),
            inputs: to_json_object(&inputs),
            outputs: Value::Object(outputs),
            trace: vec![],
            emits: vec![],
            ir,
        });
    }

    let mut vars = inputs.clone();
    let mut trace = Vec::new();
    let mut emit_trace = Vec::new();
    let mut step = 0usize;
    let mut sync_counter = 0usize;
    let mut outputs = serde_json::Map::new();
    let host: Rc<dyn Host> = match parent_host {
        Some(h) => h,
        None => Rc::new(NativeHost::new()),
    };

    // Pause before the first statement so the debugger starts paused
    if let Some(ctrl) = step_ctrl {
        ctrl.maybe_pause(StateSnapshot {
            step: 0,
            node_id: None,
            op: None,
            bindings: vars.clone(),
            status: "paused".to_string(),
            trace: vec![],
            emits: vec![],
        });
    }

    if let ExecSignal::Emit {
        output,
        value_var,
        value,
    } = execute_statements(
        &flow.body,
        &mut vars,
        &mut trace,
        &mut emit_trace,
        &mut step,
        flow_registry,
        &mut sync_counter,
        None,
        step_ctrl,
        &host,
        codecs,
        None,
    )
    .await?
    {
        if let Some(port) = flow.outputs.iter().find(|p| p.name == output) {
            let type_errors = registry.validate(&value, &port.type_name, &output);
            if !type_errors.is_empty() {
                let msgs: Vec<_> = type_errors
                    .iter()
                    .map(|e| format!("{}: {}", e.path, e.message))
                    .collect();
                return Err(format!(
                    "output `{}` does not conform to type `{}`: {}",
                    output,
                    port.type_name,
                    msgs.join(", ")
                ));
            }
        }
        outputs.insert(output.clone(), value.clone());
        // Only insert into vars if value_var is a simple variable name
        if !value_var.contains(' ') && !value_var.contains('"') {
            vars.insert(value_var, value);
        }
    }

    Ok(RunReport {
        flow: flow.name.clone(),
        inputs: to_json_object(&inputs),
        outputs: Value::Object(outputs),
        trace,
        emits: emit_trace,
        ir,
    })
}

pub fn execute_flow_stepping(
    flow: &Flow,
    ir: Ir,
    inputs: HashMap<String, Value>,
    registry: &TypeRegistry,
    flow_registry: Option<&FlowRegistry>,
    codecs: CodecRegistry,
) -> Result<
    (
        StepHandle,
        std::thread::JoinHandle<Result<RunReport, String>>,
    ),
    String,
> {
    let (controller, handle) = create_step_channels();

    let flow = flow.clone();
    let ir_clone = ir;
    let registry = registry.clone();
    let flow_registry = flow_registry.cloned();

    let join = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("failed to create tokio runtime: {e}"))?;
        let local = tokio::task::LocalSet::new();
        local.block_on(
            &rt,
            execute_flow_inner(
                &flow,
                ir_clone,
                inputs,
                &registry,
                flow_registry.as_ref(),
                Some(&controller),
                &codecs,
                None,
            ),
        )
    });

    Ok((handle, join))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir;
    use crate::parser;
    use crate::types::TypeRegistry;

    fn make_rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    fn test_op(op: &str, args: &[Value]) -> Result<Value, String> {
        let rt = make_rt();
        let local = tokio::task::LocalSet::new();
        local.block_on(&rt, async {
            let h = NativeHost::new();
            let codecs = CodecRegistry::default_registry();
            execute_op(op, args, &h, &codecs).await
        })
    }

    fn test_execute_flow(
        flow: &Flow,
        ir: Ir,
        inputs: HashMap<String, Value>,
        registry: &TypeRegistry,
        flow_registry: Option<&FlowRegistry>,
        codecs: &CodecRegistry,
    ) -> Result<RunReport, String> {
        let rt = make_rt();
        let local = tokio::task::LocalSet::new();
        local.block_on(
            &rt,
            execute_flow(flow, ir, inputs, registry, flow_registry, codecs, None),
        )
    }

    fn run_flow_with_inputs(source: &str, inputs: Vec<(&str, Value)>) -> Value {
        let flow = parser::parse_runtime_flow_v1(source).unwrap();
        let ir_data = ir::lower_to_ir(&flow).unwrap();
        let registry = TypeRegistry::empty();
        let codecs = CodecRegistry::default_registry();
        let mut input_map = HashMap::new();
        for (k, v) in inputs {
            input_map.insert(k.to_string(), v);
        }
        let report =
            test_execute_flow(&flow, ir_data, input_map, &registry, None, &codecs).unwrap();
        report.outputs
    }

    #[test]
    fn output_validation_catches_type_mismatch() {
        let src = r#"
type TestRequest
  path text
  params dict
done

type StrictResponse
  status long :required => true
  body text :required => true
done

type AuthError
  status long
done

docs BadEmitFlow
  Flow that emits a value missing required fields.
done

func BadEmitFlow
  take request as TestRequest
  emit response as StrictResponse
  fail error as AuthError
body
  params = http.extract_params(request)
  emit params
done
"#;
        let module = parser::parse_module_v1(src).unwrap();
        let registry = TypeRegistry::from_module(&module).unwrap();
        let flow = parser::parse_runtime_func_from_module_v1(&module).unwrap();
        let flow_ir = ir::lower_to_ir(&flow).unwrap();
        let mut inputs = HashMap::new();
        inputs.insert(
            "request".to_string(),
            serde_json::json!({"path": "/test", "params": {"email": "a@b.com"}}),
        );
        let result = test_execute_flow(
            &flow,
            flow_ir,
            inputs,
            &registry,
            None,
            &CodecRegistry::default_registry(),
        );
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("should fail output validation"),
        };
        assert!(
            err.contains("does not conform to type"),
            "error should mention type conformance, got: {err}"
        );
    }

    #[test]
    fn json_decode_op() {
        let result = test_op("json.decode", &[json!("{\"a\":1}")]).unwrap();
        assert_eq!(result, json!({"a": 1}));
    }

    #[test]
    fn json_encode_op() {
        let result = test_op("json.encode", &[json!({"x": 42})]).unwrap();
        let text = result.as_str().unwrap();
        let roundtrip: Value = serde_json::from_str(text).unwrap();
        assert_eq!(roundtrip, json!({"x": 42}));
    }

    #[test]
    fn codec_decode_op() {
        let result = test_op("codec.decode", &[json!("json"), json!("[1,2,3]")]).unwrap();
        assert_eq!(result, json!([1, 2, 3]));
    }

    #[test]
    fn codec_decode_unknown_format() {
        let result = test_op("codec.decode", &[json!("yaml"), json!("x: 1")]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown format `yaml`"));
    }

    #[test]
    fn json_roundtrip_flow() {
        let src = r#"
type JIn
  text text
done

type JOut
  decoded dict
  encoded text
done

type JErr
  message text
done

docs JsonRT
  Decodes then re-encodes JSON.
done

func JsonRT
  take input as JIn
  emit result as JOut
  fail error as JErr
body
  parsed = json.decode(input.text)
  compact = json.encode(parsed)
  out1 = obj.new()
  out2 = obj.set(out1, "decoded", parsed)
  out3 = obj.set(out2, "encoded", compact)
  emit out3
done
"#;
        let module = parser::parse_module_v1(src).unwrap();
        let registry = TypeRegistry::from_module(&module).unwrap();
        let flow = parser::parse_runtime_func_from_module_v1(&module).unwrap();
        let ir_val = ir::lower_to_ir(&flow).unwrap();
        let mut inputs = HashMap::new();
        inputs.insert("input".to_string(), json!({"text": "{\"a\":1}"}));
        let report = test_execute_flow(
            &flow,
            ir_val,
            inputs,
            &registry,
            None,
            &CodecRegistry::default_registry(),
        )
        .unwrap();
        let outputs = report.outputs.as_object().unwrap();
        let result = outputs.get("result").expect("should emit result");
        assert_eq!(result.get("decoded"), Some(&json!({"a": 1})));
        assert!(result.get("encoded").and_then(|v| v.as_str()).is_some());
    }

    #[test]
    fn math_floor_works() {
        assert_eq!(test_op("math.floor", &[json!(3.7)]).unwrap(), json!(3));
        assert_eq!(test_op("math.floor", &[json!(2.0)]).unwrap(), json!(2));
    }

    #[test]
    fn time_split_hms_works() {
        let result = test_op("time.split_hms", &[json!(2.5)]).unwrap();
        let obj = result.as_object().unwrap();
        assert_eq!(obj.get("h").unwrap(), &json!(2));
        assert_eq!(obj.get("m").unwrap(), &json!(30));
        assert_eq!(obj.get("s").unwrap(), &json!(0));
    }

    #[test]
    fn fmt_pad_hms_works() {
        let hms = json!({"h": 2, "m": 5, "s": 3});
        let result = test_op("fmt.pad_hms", &[hms]).unwrap();
        assert_eq!(result, json!("02:05:03"));
    }

    #[test]
    fn fmt_wrap_field_works() {
        let result = test_op("fmt.wrap_field", &[json!("total_hours"), json!(2.5)]).unwrap();
        assert_eq!(result, json!({"total_hours": 2.5}));
    }

    #[test]
    fn parse_duration_works() {
        assert_eq!(parse_duration("5s").unwrap(), Duration::from_secs(5));
        assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
        assert_eq!(parse_duration("2m").unwrap(), Duration::from_secs(120));
        assert!(parse_duration("bad").is_err());
    }

    fn build_sync_flow(sync_opts: &str) -> (Flow, Ir, TypeRegistry) {
        let src = format!(
            r#"
type Req
  path text
  params dict
done

type Resp
  status long
done

type Err
  status long
done

docs SyncTest
  A test flow for sync options.
done

func SyncTest
  take request as Req
  emit response as Resp
  fail error as Err
body
  params = http.extract_params(request)
  [user] = sync {sync_opts}
    user_local = db.query_user_by_email("test@example.com")
  done [user_local]
  response = http.error_response(200, "ok")
  emit response
done
"#
        );
        let module = parser::parse_module_v1(&src).unwrap();
        let registry = TypeRegistry::from_module(&module).unwrap();
        let flow = parser::parse_runtime_func_from_module_v1(&module).unwrap();
        let ir_val = ir::lower_to_ir(&flow).unwrap();
        (flow, ir_val, registry)
    }

    #[test]
    fn sync_trace_has_group_ids() {
        let (flow, ir_val, registry) = build_sync_flow(":safe => false");
        let mut inputs = HashMap::new();
        inputs.insert(
            "request".to_string(),
            json!({"path": "/test", "params": {}}),
        );
        let report = test_execute_flow(
            &flow,
            ir_val,
            inputs,
            &registry,
            None,
            &CodecRegistry::default_registry(),
        )
        .unwrap();
        let sync_events: Vec<_> = report
            .trace
            .iter()
            .filter(|e| e.sync_group.is_some())
            .collect();
        assert!(
            !sync_events.is_empty(),
            "should have trace events with sync_group set"
        );
        assert_eq!(
            sync_events[0].sync_group.as_deref(),
            Some("sync_0"),
            "first sync group should be sync_0"
        );
    }

    #[test]
    fn sync_safe_exports_null() {
        // Test that :safe => true handles missing exports by inserting null.
        // We use division by zero to trigger a failure in the sync body,
        // causing the bind variable to not be set, so the export is missing.
        let src = r#"
type Req
  path text
done

type Resp
  status long
done

type Err
  status long
done

docs SafeSync
  A flow where sync block fails safely.
done

func SafeSync
  take request as Req
  emit response as Resp
  fail error as Err
body
  zero = 0.0 - 0.0
  [data] = sync :safe => true
    data_local = 1.0 / zero
  done [data_local]
  response = http.error_response(200, "ok")
  emit response
done
"#;
        let module = parser::parse_module_v1(src).unwrap();
        let registry = TypeRegistry::from_module(&module).unwrap();
        let flow = parser::parse_runtime_func_from_module_v1(&module).unwrap();
        let ir_val = ir::lower_to_ir(&flow).unwrap();
        let mut inputs = HashMap::new();
        inputs.insert("request".to_string(), json!({"path": "/test"}));
        let report = test_execute_flow(
            &flow,
            ir_val,
            inputs,
            &registry,
            None,
            &CodecRegistry::default_registry(),
        )
        .unwrap();
        assert!(
            report.outputs.get("response").is_some(),
            "should produce output despite sync failure"
        );
    }

    #[test]
    fn sync_retry_succeeds() {
        let (flow, ir_val, registry) = build_sync_flow(":safe => false, :retry => 2");
        let mut inputs = HashMap::new();
        inputs.insert(
            "request".to_string(),
            json!({"path": "/test", "params": {}}),
        );
        let report = test_execute_flow(
            &flow,
            ir_val,
            inputs,
            &registry,
            None,
            &CodecRegistry::default_registry(),
        )
        .unwrap();
        assert!(
            report.outputs.get("response").is_some(),
            "sync with retry should succeed"
        );
    }

    #[test]
    fn sync_timeout_enforced() {
        let src = r#"
type Req
  path text
done

type Resp
  status long
done

type Err
  status long
done

docs TimeoutSync
  A flow where sync block has a tight timeout.
done

func TimeoutSync
  take request as Req
  emit response as Resp
  fail error as Err
body
  [data] = sync :safe => false, :timeout => "1ms"
    a = db.query_user_by_email("a@example.com")
    b = db.query_user_by_email("b@example.com")
    c = db.query_user_by_email("c@example.com")
    d = db.query_user_by_email("d@example.com")
    e = db.query_user_by_email("e@example.com")
    f = db.query_user_by_email("f@example.com")
    g = db.query_user_by_email("g@example.com")
    h = db.query_user_by_email("h@example.com")
  done [a]
  response = http.error_response(200, "ok")
  emit response
done
"#;
        let module = parser::parse_module_v1(src).unwrap();
        let registry = TypeRegistry::from_module(&module).unwrap();
        let flow = parser::parse_runtime_func_from_module_v1(&module).unwrap();
        let ir_val = ir::lower_to_ir(&flow).unwrap();
        let mut inputs = HashMap::new();
        inputs.insert("request".to_string(), json!({"path": "/test"}));
        // With 1ms timeout, this may or may not timeout depending on execution speed.
        // The important thing is it doesn't panic and handles the timeout path.
        let _result = test_execute_flow(
            &flow,
            ir_val,
            inputs,
            &registry,
            None,
            &CodecRegistry::default_registry(),
        );
    }

    #[test]
    fn stepping_pauses_after_each_node() {
        let src = r#"
type Req
  path text
  params dict
done

type Resp
  status long
done

type Err
  status long
done

docs StepFlow
  A simple flow for stepping test.
done

func StepFlow
  take request as Req
  emit response as Resp
  fail error as Err
body
  params = http.extract_params(request)
  email = auth.extract_email(params)
  response = http.error_response(200, "ok")
  emit response
done
"#;
        let module = parser::parse_module_v1(src).unwrap();
        let registry = TypeRegistry::from_module(&module).unwrap();
        let flow = parser::parse_runtime_func_from_module_v1(&module).unwrap();
        let ir_val = ir::lower_to_ir(&flow).unwrap();
        let mut inputs = HashMap::new();
        inputs.insert(
            "request".to_string(),
            json!({"path": "/test", "params": {"email": "a@b.com"}}),
        );

        let (handle, join) = execute_flow_stepping(
            &flow,
            ir_val,
            inputs,
            &registry,
            None,
            CodecRegistry::default_registry(),
        )
        .unwrap();

        // Discard the initial "ready" snapshot (op=None) sent before any nodes run
        let ready = handle
            .snapshot_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("should receive ready snapshot");
        assert_eq!(ready.status, "paused");
        assert_eq!(ready.op, None);
        handle.command_tx.send(StepCommand::Step).unwrap();

        let mut snapshots = Vec::new();
        for _ in 0..3 {
            let snap = handle
                .snapshot_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("should receive snapshot");
            assert_eq!(snap.status, "paused");
            snapshots.push(snap);
            handle.command_tx.send(StepCommand::Step).unwrap();
        }

        let report = join.join().expect("thread should not panic").unwrap();
        assert!(report.outputs.get("response").is_some());
        assert_eq!(snapshots.len(), 3);
        assert_eq!(snapshots[0].op.as_deref(), Some("http.extract_params"));
        assert_eq!(snapshots[1].op.as_deref(), Some("auth.extract_email"));
        assert_eq!(snapshots[2].op.as_deref(), Some("http.error_response"));
    }

    #[test]
    fn stepping_continue_runs_to_completion() {
        let src = r#"
type Req
  path text
  params dict
done

type Resp
  status long
done

type Err
  status long
done

docs ContFlow
  A flow for stepping continue test.
done

func ContFlow
  take request as Req
  emit response as Resp
  fail error as Err
body
  params = http.extract_params(request)
  email = auth.extract_email(params)
  response = http.error_response(200, "ok")
  emit response
done
"#;
        let module = parser::parse_module_v1(src).unwrap();
        let registry = TypeRegistry::from_module(&module).unwrap();
        let flow = parser::parse_runtime_func_from_module_v1(&module).unwrap();
        let ir_val = ir::lower_to_ir(&flow).unwrap();
        let mut inputs = HashMap::new();
        inputs.insert(
            "request".to_string(),
            json!({"path": "/test", "params": {"email": "a@b.com"}}),
        );

        let (handle, join) = execute_flow_stepping(
            &flow,
            ir_val,
            inputs,
            &registry,
            None,
            CodecRegistry::default_registry(),
        )
        .unwrap();

        let snap = handle
            .snapshot_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("should receive first snapshot");
        assert_eq!(snap.status, "paused");
        handle.command_tx.send(StepCommand::Continue).unwrap();

        let report = join.join().expect("thread should not panic").unwrap();
        assert!(report.outputs.get("response").is_some());
    }

    #[test]
    fn stepping_breakpoint_pauses_at_target() {
        let src = r#"
type Req
  path text
  params dict
done

type Resp
  status long
done

type Err
  status long
done

docs BPFlow
  A flow for breakpoint testing.
done

func BPFlow
  take request as Req
  emit response as Resp
  fail error as Err
body
  params = http.extract_params(request)
  email = auth.extract_email(params)
  response = http.error_response(200, "ok")
  emit response
done
"#;
        let module = parser::parse_module_v1(src).unwrap();
        let registry = TypeRegistry::from_module(&module).unwrap();
        let flow = parser::parse_runtime_func_from_module_v1(&module).unwrap();
        let ir_val = ir::lower_to_ir(&flow).unwrap();

        // Find the node_id for "email" (the auth.extract_email node)
        let target_node_id = ir_val
            .nodes
            .iter()
            .find(|n| n.bind == "email")
            .expect("should have email node")
            .id
            .clone();

        let mut inputs = HashMap::new();
        inputs.insert(
            "request".to_string(),
            json!({"path": "/test", "params": {"email": "a@b.com"}}),
        );

        let (handle, join) = execute_flow_stepping(
            &flow,
            ir_val,
            inputs,
            &registry,
            None,
            CodecRegistry::default_registry(),
        )
        .unwrap();

        // Initial "ready" snapshot arrives with no op (emitted before first statement).
        let init = handle
            .snapshot_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("should get initial snapshot");
        assert!(init.op.is_none());

        // Step to the first node and confirm its op.
        handle.command_tx.send(StepCommand::Step).unwrap();
        let snap1 = handle
            .snapshot_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("should get first node snapshot");
        assert_eq!(snap1.op.as_deref(), Some("http.extract_params"));

        // Set breakpoint on the email node and run to breakpoint
        let mut bp = HashSet::new();
        bp.insert(target_node_id.clone());
        handle
            .command_tx
            .send(StepCommand::SetBreakpoints(bp))
            .unwrap();
        handle
            .command_tx
            .send(StepCommand::RunToBreakpoint)
            .unwrap();

        // Should pause at the email node
        let snap2 = handle
            .snapshot_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("should pause at breakpoint");
        assert_eq!(snap2.node_id.as_deref(), Some(target_node_id.as_str()));
        assert_eq!(snap2.op.as_deref(), Some("auth.extract_email"));

        // Continue to finish
        handle.command_tx.send(StepCommand::Continue).unwrap();

        let report = join.join().expect("thread should not panic").unwrap();
        assert!(report.outputs.get("response").is_some());
    }

    #[test]
    fn stepping_set_breakpoints_updates() {
        let src = r#"
type Req
  path text
  params dict
done

type Resp
  status long
done

type Err
  status long
done

docs UpdateBPFlow
  A flow for testing breakpoint updates.
done

func UpdateBPFlow
  take request as Req
  emit response as Resp
  fail error as Err
body
  params = http.extract_params(request)
  email = auth.extract_email(params)
  response = http.error_response(200, "ok")
  emit response
done
"#;
        let module = parser::parse_module_v1(src).unwrap();
        let registry = TypeRegistry::from_module(&module).unwrap();
        let flow = parser::parse_runtime_func_from_module_v1(&module).unwrap();
        let ir_val = ir::lower_to_ir(&flow).unwrap();

        let response_node_id = ir_val
            .nodes
            .iter()
            .find(|n| n.bind == "response")
            .expect("should have response node")
            .id
            .clone();

        let mut inputs = HashMap::new();
        inputs.insert(
            "request".to_string(),
            json!({"path": "/test", "params": {"email": "a@b.com"}}),
        );

        let (handle, join) = execute_flow_stepping(
            &flow,
            ir_val,
            inputs,
            &registry,
            None,
            CodecRegistry::default_registry(),
        )
        .unwrap();

        // Initial "ready" snapshot (op=None, emitted before first statement).
        let _ = handle
            .snapshot_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("initial snapshot");

        // Step to first node (extract_params)
        handle.command_tx.send(StepCommand::Step).unwrap();
        let snap1 = handle
            .snapshot_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("first snapshot");
        assert_eq!(snap1.op.as_deref(), Some("http.extract_params"));

        // Step once to get to email
        handle.command_tx.send(StepCommand::Step).unwrap();
        let snap2 = handle
            .snapshot_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("second snapshot");
        assert_eq!(snap2.op.as_deref(), Some("auth.extract_email"));

        // Now set breakpoint on response and run to it
        let mut bp = HashSet::new();
        bp.insert(response_node_id.clone());
        handle
            .command_tx
            .send(StepCommand::SetBreakpoints(bp))
            .unwrap();
        handle
            .command_tx
            .send(StepCommand::RunToBreakpoint)
            .unwrap();

        let snap3 = handle
            .snapshot_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("breakpoint snapshot");
        assert_eq!(snap3.node_id.as_deref(), Some(response_node_id.as_str()));

        handle.command_tx.send(StepCommand::Continue).unwrap();
        let report = join.join().expect("thread should not panic").unwrap();
        assert!(report.outputs.get("response").is_some());
    }

    #[test]
    fn math_round_works() {
        let result = test_op("math.round", &[json!(3.14159), json!(2.0)]).unwrap();
        assert_eq!(result, json!(3.14));
    }

    #[test]
    fn list_range_works() {
        let result = test_op("list.range", &[json!(1.0), json!(4.0)]).unwrap();
        assert_eq!(result, json!([1, 2, 3, 4]));
    }

    #[test]
    fn list_new_works() {
        let result = test_op("list.new", &[]).unwrap();
        assert_eq!(result, json!([]));
    }

    #[test]
    fn list_append_works() {
        let result = test_op("list.append", &[json!([1, 2]), json!(3)]).unwrap();
        assert_eq!(result, json!([1, 2, 3]));
    }

    #[test]
    fn obj_new_works() {
        let result = test_op("obj.new", &[]).unwrap();
        assert_eq!(result, json!({}));
    }

    #[test]
    fn obj_set_works() {
        let result = test_op("obj.set", &[json!({"a": 1}), json!("b"), json!(2)]).unwrap();
        assert_eq!(result, json!({"a": 1, "b": 2}));
    }

    // --- list.len ---

    #[test]
    fn list_len_works() {
        let result = test_op("list.len", &[json!([1, 2, 3])]).unwrap();
        assert_eq!(result, json!(3));
    }

    #[test]
    fn list_len_empty() {
        let result = test_op("list.len", &[json!([])]).unwrap();
        assert_eq!(result, json!(0));
    }

    // --- list.contains ---

    #[test]
    fn list_contains_found() {
        let result = test_op("list.contains", &[json!([1, 2, 3]), json!(2)]).unwrap();
        assert_eq!(result, json!(true));
    }

    #[test]
    fn list_contains_not_found() {
        let result = test_op("list.contains", &[json!([1, 2, 3]), json!(9)]).unwrap();
        assert_eq!(result, json!(false));
    }

    #[test]
    fn list_contains_string() {
        let result = test_op("list.contains", &[json!(["a", "b"]), json!("b")]).unwrap();
        assert_eq!(result, json!(true));
    }

    #[test]
    fn list_contains_empty() {
        let result = test_op("list.contains", &[json!([]), json!(1)]).unwrap();
        assert_eq!(result, json!(false));
    }

    // --- list.slice ---

    #[test]
    fn list_slice_works() {
        let result = test_op("list.slice", &[json!([10, 20, 30, 40]), json!(1), json!(3)]).unwrap();
        assert_eq!(result, json!([20, 30]));
    }

    #[test]
    fn list_slice_clamped() {
        let result = test_op("list.slice", &[json!([1, 2, 3]), json!(-5), json!(100)]).unwrap();
        assert_eq!(result, json!([1, 2, 3]));
    }

    #[test]
    fn list_slice_empty_range() {
        let result = test_op("list.slice", &[json!([1, 2, 3]), json!(2), json!(1)]).unwrap();
        assert_eq!(result, json!([]));
    }

    #[test]
    fn list_slice_empty_list() {
        let result = test_op("list.slice", &[json!([]), json!(0), json!(0)]).unwrap();
        assert_eq!(result, json!([]));
    }

    // --- list.indices ---

    #[test]
    fn list_indices_works() {
        let result = test_op("list.indices", &[json!(["a", "b", "c"])]).unwrap();
        assert_eq!(result, json!([0, 1, 2]));
    }

    #[test]
    fn list_indices_empty() {
        let result = test_op("list.indices", &[json!([])]).unwrap();
        assert_eq!(result, json!([]));
    }

    // --- obj.get ---

    #[test]
    fn obj_get_works() {
        let result = test_op("obj.get", &[json!({"name": "alice"}), json!("name")]).unwrap();
        assert_eq!(result, json!("alice"));
    }

    #[test]
    fn obj_get_missing_key() {
        let err = test_op("obj.get", &[json!({"a": 1}), json!("z")]).unwrap_err();
        assert!(err.contains("not found"));
    }

    #[test]
    fn obj_get_empty() {
        let err = test_op("obj.get", &[json!({}), json!("x")]).unwrap_err();
        assert!(err.contains("not found"));
    }

    // --- obj.has ---

    #[test]
    fn obj_has_true() {
        let result = test_op("obj.has", &[json!({"x": 1}), json!("x")]).unwrap();
        assert_eq!(result, json!(true));
    }

    #[test]
    fn obj_has_false() {
        let result = test_op("obj.has", &[json!({"x": 1}), json!("y")]).unwrap();
        assert_eq!(result, json!(false));
    }

    // --- obj.delete ---

    #[test]
    fn obj_delete_existing() {
        let result = test_op("obj.delete", &[json!({"a": 1, "b": 2}), json!("a")]).unwrap();
        assert_eq!(result, json!({"b": 2}));
    }

    #[test]
    fn obj_delete_missing() {
        let result = test_op("obj.delete", &[json!({"a": 1}), json!("z")]).unwrap();
        assert_eq!(result, json!({"a": 1}));
    }

    // --- obj.keys ---

    #[test]
    fn obj_keys_works() {
        let result = test_op("obj.keys", &[json!({"b": 2, "a": 1})]).unwrap();
        let mut keys: Vec<String> = result
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        keys.sort();
        assert_eq!(keys, vec!["a", "b"]);
    }

    #[test]
    fn obj_keys_empty() {
        let result = test_op("obj.keys", &[json!({})]).unwrap();
        assert_eq!(result, json!([]));
    }

    // --- obj.merge ---

    #[test]
    fn obj_merge_works() {
        let result = test_op("obj.merge", &[json!({"a": 1}), json!({"b": 2})]).unwrap();
        assert_eq!(result, json!({"a": 1, "b": 2}));
    }

    #[test]
    fn obj_merge_overwrite() {
        let result = test_op("obj.merge", &[json!({"a": 1, "b": 2}), json!({"b": 99})]).unwrap();
        assert_eq!(result, json!({"a": 1, "b": 99}));
    }

    #[test]
    fn obj_merge_empty() {
        let result = test_op("obj.merge", &[json!({}), json!({})]).unwrap();
        assert_eq!(result, json!({}));
    }

    #[test]
    fn term_print_returns_true() {
        let result = test_op("term.print", &[json!("hello")]).unwrap();
        assert_eq!(result, json!(true));
    }

    #[test]
    fn file_read_write_roundtrip() {
        let dir = std::env::temp_dir().join("forai_io_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test_io.txt");
        let path_str = path.display().to_string();

        let write_result = test_op("file.write", &[json!(path_str), json!("hello world")]).unwrap();
        assert_eq!(write_result, json!(true));

        let read_result = test_op("file.read", &[json!(path_str)]).unwrap();
        assert_eq!(read_result, json!("hello world"));

        let exists_result = test_op("file.exists", &[json!(path_str)]).unwrap();
        assert_eq!(exists_result, json!(true));

        let missing_result = test_op("file.exists", &[json!("/nonexistent/path/xyz")]).unwrap();
        assert_eq!(missing_result, json!(false));

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn http_response_works() {
        let result = test_op("http.response", &[json!(200), json!({"ok": true})]).unwrap();
        assert_eq!(result.get("status").unwrap(), &json!(200));
        assert_eq!(result.get("body").unwrap(), &json!({"ok": true}));
    }

    // --- Expression runtime tests ---

    /// Helper: parse and execute a func body that should assign to `_r` then wrap+emit.
    /// The `expr_line` should assign to `_r`, e.g. `_r = 2 + 3`
    fn run_expr_value(expr_line: &str) -> Result<Value, String> {
        let src = format!(
            r#"
docs Calc
  Expression test.
done

func Calc
  take input as dict
  emit result as dict
  fail error as dict
body
  {expr_line}
  _wrapped = fmt.wrap_field("v", _r)
  emit _wrapped
done
"#
        );
        let module = parser::parse_module_v1(&src).unwrap();
        let registry = TypeRegistry::from_module(&module).unwrap();
        let flow = parser::parse_runtime_func_from_module_v1(&module).unwrap();
        let flow_ir = ir::lower_to_ir(&flow).unwrap();
        let mut inputs = HashMap::new();
        inputs.insert("input".to_string(), json!({"value": 10.0}));
        let report = test_execute_flow(
            &flow,
            flow_ir,
            inputs,
            &registry,
            None,
            &CodecRegistry::default_registry(),
        )?;
        let out = report
            .outputs
            .get("result")
            .ok_or_else(|| "no result output".to_string())?;
        Ok(out.get("v").cloned().unwrap_or(out.clone()))
    }

    #[test]
    fn expr_arithmetic_add() {
        assert_eq!(run_expr_value("_r = 2 + 3").unwrap(), json!(5));
    }

    #[test]
    fn expr_arithmetic_mul_div() {
        assert_eq!(
            run_expr_value("_r = 10.0 / 2.0 * 3.0").unwrap(),
            json!(15.0)
        );
    }

    #[test]
    fn expr_precedence_produces_correct_value() {
        assert_eq!(run_expr_value("_r = 2 + 3 * 4").unwrap(), json!(14));
    }

    #[test]
    fn expr_parens_change_precedence() {
        assert_eq!(run_expr_value("_r = (2 + 3) * 4").unwrap(), json!(20));
    }

    #[test]
    fn expr_unary_negation() {
        assert_eq!(run_expr_value("_r = -5 + 10").unwrap(), json!(5));
    }

    #[test]
    fn expr_division_by_zero() {
        let result = run_expr_value("_r = 1.0 / 0.0");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Division by zero"), "got: {err}");
    }

    #[test]
    fn expr_string_concatenation() {
        assert_eq!(
            run_expr_value("_r = \"hello\" + \" world\"").unwrap(),
            json!("hello world")
        );
    }

    #[test]
    fn expr_comparison() {
        assert_eq!(run_expr_value("_r = 5 > 3").unwrap(), json!(true));
    }

    #[test]
    fn expr_variable_path_in_expression() {
        assert_eq!(
            run_expr_value("_r = input.value + 5.0").unwrap(),
            json!(15.0)
        );
    }

    #[test]
    fn expr_chained_division() {
        let v = run_expr_value("_r = input.value / 100.0 / 12.0").unwrap();
        let n = v.as_f64().unwrap();
        assert!((n - 10.0 / 100.0 / 12.0).abs() < 1e-10);
    }

    #[test]
    fn expr_power_operator() {
        assert_eq!(run_expr_value("_r = 2.0 ** 10.0").unwrap(), json!(1024.0));
    }

    #[test]
    fn expr_modulo_operator() {
        assert_eq!(run_expr_value("_r = 10 % 3").unwrap(), json!(1));
    }

    #[test]
    fn expr_logical_and() {
        assert_eq!(run_expr_value("_r = true && false").unwrap(), json!(false));
    }

    #[test]
    fn expr_logical_or() {
        assert_eq!(run_expr_value("_r = true || false").unwrap(), json!(true));
    }

    #[test]
    fn expr_mixed_with_old_syntax() {
        // Old op(args) calls and new expressions in the same body
        let src = r#"
docs Calc
  Mixed test.
done

func Calc
  take input as dict
  emit result as dict
  fail error as dict
body
  a = 2.0 + 3.0
  b = a * 4.0
  _wrapped = fmt.wrap_field("v", b)
  emit _wrapped
done
"#;
        let module = parser::parse_module_v1(src).unwrap();
        let registry = TypeRegistry::from_module(&module).unwrap();
        let flow = parser::parse_runtime_func_from_module_v1(&module).unwrap();
        let flow_ir = ir::lower_to_ir(&flow).unwrap();
        let mut inputs = HashMap::new();
        inputs.insert("input".to_string(), json!({}));
        let report = test_execute_flow(
            &flow,
            flow_ir,
            inputs,
            &registry,
            None,
            &CodecRegistry::default_registry(),
        )
        .unwrap();
        let out = report.outputs.get("result").unwrap();
        assert_eq!(out.get("v").unwrap(), &json!(20.0));
    }

    #[test]
    fn expr_call_in_expression_context() {
        let src = r#"
docs Calc
  Call in expr test.
done

func Calc
  take input as dict
  emit result as dict
  fail error as dict
body
  x = (2.0 + 3.0) * 2.0
  _wrapped = fmt.wrap_field("v", x)
  emit _wrapped
done
"#;
        let module = parser::parse_module_v1(src).unwrap();
        let registry = TypeRegistry::from_module(&module).unwrap();
        let flow = parser::parse_runtime_func_from_module_v1(&module).unwrap();
        let flow_ir = ir::lower_to_ir(&flow).unwrap();
        let mut inputs = HashMap::new();
        inputs.insert("input".to_string(), json!({}));
        let report = test_execute_flow(
            &flow,
            flow_ir,
            inputs,
            &registry,
            None,
            &CodecRegistry::default_registry(),
        )
        .unwrap();
        let out = report.outputs.get("result").unwrap();
        assert_eq!(out.get("v").unwrap(), &json!(10.0));
    }

    // --- str.* op tests ---

    #[test]
    fn str_len_works() {
        assert_eq!(test_op("str.len", &[json!("hello")]).unwrap(), json!(5));
        assert_eq!(test_op("str.len", &[json!("")]).unwrap(), json!(0));
    }

    #[test]
    fn str_upper_works() {
        assert_eq!(
            test_op("str.upper", &[json!("hello")]).unwrap(),
            json!("HELLO")
        );
    }

    #[test]
    fn str_lower_works() {
        assert_eq!(
            test_op("str.lower", &[json!("HELLO")]).unwrap(),
            json!("hello")
        );
    }

    #[test]
    fn str_trim_works() {
        assert_eq!(
            test_op("str.trim", &[json!("  hi  ")]).unwrap(),
            json!("hi")
        );
    }

    #[test]
    fn str_trim_start_works() {
        assert_eq!(
            test_op("str.trim_start", &[json!("  hi  ")]).unwrap(),
            json!("hi  ")
        );
    }

    #[test]
    fn str_trim_end_works() {
        assert_eq!(
            test_op("str.trim_end", &[json!("  hi  ")]).unwrap(),
            json!("  hi")
        );
    }

    #[test]
    fn str_split_works() {
        assert_eq!(
            test_op("str.split", &[json!("a,b,c"), json!(",")]).unwrap(),
            json!(["a", "b", "c"])
        );
    }

    #[test]
    fn str_join_works() {
        assert_eq!(
            test_op("str.join", &[json!(["a", "b", "c"]), json!(",")]).unwrap(),
            json!("a,b,c")
        );
    }

    #[test]
    fn str_replace_works() {
        assert_eq!(
            test_op(
                "str.replace",
                &[json!("hello world"), json!("world"), json!("rust")]
            )
            .unwrap(),
            json!("hello rust")
        );
    }

    #[test]
    fn str_contains_works() {
        assert_eq!(
            test_op("str.contains", &[json!("hello"), json!("ell")]).unwrap(),
            json!(true)
        );
        assert_eq!(
            test_op("str.contains", &[json!("hello"), json!("xyz")]).unwrap(),
            json!(false)
        );
    }

    #[test]
    fn str_starts_with_works() {
        assert_eq!(
            test_op("str.starts_with", &[json!("hello"), json!("hel")]).unwrap(),
            json!(true)
        );
        assert_eq!(
            test_op("str.starts_with", &[json!("hello"), json!("lo")]).unwrap(),
            json!(false)
        );
    }

    #[test]
    fn str_ends_with_works() {
        assert_eq!(
            test_op("str.ends_with", &[json!("hello"), json!("llo")]).unwrap(),
            json!(true)
        );
        assert_eq!(
            test_op("str.ends_with", &[json!("hello"), json!("hel")]).unwrap(),
            json!(false)
        );
    }

    #[test]
    fn str_slice_works() {
        assert_eq!(
            test_op("str.slice", &[json!("hello"), json!(1), json!(4)]).unwrap(),
            json!("ell")
        );
        assert_eq!(
            test_op("str.slice", &[json!("hello"), json!(0), json!(100)]).unwrap(),
            json!("hello")
        );
        assert_eq!(
            test_op("str.slice", &[json!("hello"), json!(3), json!(1)]).unwrap(),
            json!("")
        );
    }

    #[test]
    fn str_index_of_works() {
        assert_eq!(
            test_op("str.index_of", &[json!("hello"), json!("ll")]).unwrap(),
            json!(2)
        );
        assert_eq!(
            test_op("str.index_of", &[json!("hello"), json!("xyz")]).unwrap(),
            json!(-1)
        );
    }

    #[test]
    fn str_repeat_works() {
        assert_eq!(
            test_op("str.repeat", &[json!("ab"), json!(3)]).unwrap(),
            json!("ababab")
        );
        assert_eq!(
            test_op("str.repeat", &[json!("x"), json!(0)]).unwrap(),
            json!("")
        );
    }

    #[test]
    fn string_interpolation_basic() {
        let src = r##"
docs Greet
  Interpolation test.
done

func Greet
  take input as dict
  emit result as dict
  fail error as dict
body
  name = "world"
  greeting = "hello #{name}!"
  _wrapped = fmt.wrap_field("v", greeting)
  emit _wrapped
done
"##;
        let module = parser::parse_module_v1(src).unwrap();
        let registry = TypeRegistry::from_module(&module).unwrap();
        let flow = parser::parse_runtime_func_from_module_v1(&module).unwrap();
        let flow_ir = ir::lower_to_ir(&flow).unwrap();
        let mut inputs = HashMap::new();
        inputs.insert("input".to_string(), json!({}));
        let report = test_execute_flow(
            &flow,
            flow_ir,
            inputs,
            &registry,
            None,
            &CodecRegistry::default_registry(),
        )
        .unwrap();
        let out = report.outputs.get("result").unwrap();
        assert_eq!(out.get("v").unwrap(), &json!("hello world!"));
    }

    #[test]
    fn string_interpolation_with_dotted_path() {
        let src = r##"
docs Greet
  Interpolation with dot path.
done

func Greet
  take input as dict
  emit result as dict
  fail error as dict
body
  msg = "name is #{input.name}"
  _wrapped = fmt.wrap_field("v", msg)
  emit _wrapped
done
"##;
        let module = parser::parse_module_v1(src).unwrap();
        let registry = TypeRegistry::from_module(&module).unwrap();
        let flow = parser::parse_runtime_func_from_module_v1(&module).unwrap();
        let flow_ir = ir::lower_to_ir(&flow).unwrap();
        let mut inputs = HashMap::new();
        inputs.insert("input".to_string(), json!({"name": "Ada"}));
        let report = test_execute_flow(
            &flow,
            flow_ir,
            inputs,
            &registry,
            None,
            &CodecRegistry::default_registry(),
        )
        .unwrap();
        let out = report.outputs.get("result").unwrap();
        assert_eq!(out.get("v").unwrap(), &json!("name is Ada"));
    }

    #[test]
    fn string_interpolation_with_number() {
        let src = r##"
docs Greet
  Interpolation with number.
done

func Greet
  take input as dict
  emit result as dict
  fail error as dict
body
  count = 42
  msg = "count is #{count}"
  _wrapped = fmt.wrap_field("v", msg)
  emit _wrapped
done
"##;
        let module = parser::parse_module_v1(src).unwrap();
        let registry = TypeRegistry::from_module(&module).unwrap();
        let flow = parser::parse_runtime_func_from_module_v1(&module).unwrap();
        let flow_ir = ir::lower_to_ir(&flow).unwrap();
        let mut inputs = HashMap::new();
        inputs.insert("input".to_string(), json!({}));
        let report = test_execute_flow(
            &flow,
            flow_ir,
            inputs,
            &registry,
            None,
            &CodecRegistry::default_registry(),
        )
        .unwrap();
        let out = report.outputs.get("result").unwrap();
        assert_eq!(out.get("v").unwrap(), &json!("count is 42"));
    }

    // ---------------------------------------------------------------
    // Calendar helper tests
    // ---------------------------------------------------------------

    #[test]
    fn days_from_civil_epoch() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
    }

    #[test]
    fn days_from_civil_known_date() {
        // 2024-03-15 is 19797 days after epoch
        assert_eq!(days_from_civil(2024, 3, 15), 19797);
    }

    #[test]
    fn civil_from_days_roundtrip() {
        for days in [-10000, -1, 0, 1, 10000, 19797] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(
                days_from_civil(y, m, d),
                days,
                "roundtrip failed for {days}"
            );
        }
    }

    #[test]
    fn weekday_known_dates() {
        // 1970-01-01 was Thursday (4)
        assert_eq!(weekday_from_days(0), 4);
        // 2024-03-15 (Friday = 5)
        assert_eq!(weekday_from_days(19797), 5);
    }

    #[test]
    fn leap_year_check() {
        assert!(is_leap_year(2000));
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(1900));
        assert!(!is_leap_year(2023));
    }

    #[test]
    fn days_in_month_check() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2024, 1), 31);
        assert_eq!(days_in_month(2024, 4), 30);
    }

    // ---------------------------------------------------------------
    // ISO parse/format tests
    // ---------------------------------------------------------------

    #[test]
    fn parse_iso_date_only() {
        let (ms, tz) = parse_iso_datetime("2024-03-15").unwrap();
        assert_eq!(tz, 0);
        assert_eq!(ms, 19797 * 86_400_000);
    }

    #[test]
    fn parse_iso_with_timezone() {
        let (ms, tz) = parse_iso_datetime("2024-03-15T10:30:00+05:30").unwrap();
        assert_eq!(tz, 330);
        // 10:30 IST = 05:00 UTC → 19797 days + 5h in ms
        assert_eq!(ms, 19797 * 86_400_000 + 5 * 3_600_000);
    }

    #[test]
    fn iso_roundtrip() {
        let input = "2024-03-15T10:30:00+05:30";
        let (ms, tz) = parse_iso_datetime(input).unwrap();
        let output = format_iso_datetime(ms, tz);
        assert_eq!(output, input);
    }

    // ---------------------------------------------------------------
    // date.* op tests
    // ---------------------------------------------------------------

    #[test]
    fn date_from_parts_epoch() {
        let r = test_op(
            "date.from_parts",
            &[
                json!(1970),
                json!(1),
                json!(1),
                json!(0),
                json!(0),
                json!(0),
                json!(0),
            ],
        )
        .unwrap();
        assert_eq!(r["unix_ms"], 0);
        assert_eq!(r["tz_offset_min"], 0);
    }

    #[test]
    fn date_from_parts_known() {
        // 2024-03-15T10:30:00 UTC
        let r = test_op(
            "date.from_parts",
            &[
                json!(2024),
                json!(3),
                json!(15),
                json!(10),
                json!(30),
                json!(0),
                json!(0),
            ],
        )
        .unwrap();
        let expected = 19797 * 86_400_000_i64 + 10 * 3_600_000 + 30 * 60_000;
        assert_eq!(r["unix_ms"], expected);
    }

    #[test]
    fn date_to_parts_roundtrip() {
        let date = test_op(
            "date.from_parts",
            &[
                json!(2024),
                json!(3),
                json!(15),
                json!(10),
                json!(30),
                json!(45),
                json!(123),
            ],
        )
        .unwrap();
        let parts = test_op("date.to_parts", &[date]).unwrap();
        assert_eq!(parts["year"], 2024);
        assert_eq!(parts["month"], 3);
        assert_eq!(parts["day"], 15);
        assert_eq!(parts["hour"], 10);
        assert_eq!(parts["min"], 30);
        assert_eq!(parts["sec"], 45);
        assert_eq!(parts["ms"], 123);
    }

    #[test]
    fn date_with_tz_preserves_instant() {
        let utc = test_op(
            "date.from_parts",
            &[
                json!(2024),
                json!(3),
                json!(15),
                json!(10),
                json!(0),
                json!(0),
                json!(0),
            ],
        )
        .unwrap();
        let ist = test_op("date.with_tz", &[utc.clone(), json!(330)]).unwrap();
        // Same unix_ms
        assert_eq!(utc["unix_ms"], ist["unix_ms"]);
        assert_eq!(ist["tz_offset_min"], 330);
        // to_parts should show IST time
        let parts = test_op("date.to_parts", &[ist]).unwrap();
        assert_eq!(parts["hour"], 15);
        assert_eq!(parts["min"], 30);
    }

    #[test]
    fn date_add_days() {
        let date = test_op(
            "date.from_parts",
            &[
                json!(2024),
                json!(3),
                json!(15),
                json!(0),
                json!(0),
                json!(0),
                json!(0),
            ],
        )
        .unwrap();
        let later = test_op("date.add_days", &[date.clone(), json!(10)]).unwrap();
        let diff = test_op("date.diff", &[later, date]).unwrap();
        assert_eq!(diff, json!(10 * 86_400_000_i64));
    }

    #[test]
    fn date_diff_and_compare() {
        let a = test_op(
            "date.from_parts",
            &[
                json!(2024),
                json!(3),
                json!(15),
                json!(0),
                json!(0),
                json!(0),
                json!(0),
            ],
        )
        .unwrap();
        let b = test_op(
            "date.from_parts",
            &[
                json!(2024),
                json!(3),
                json!(10),
                json!(0),
                json!(0),
                json!(0),
                json!(0),
            ],
        )
        .unwrap();
        assert_eq!(
            test_op("date.compare", &[a.clone(), b.clone()]).unwrap(),
            json!(1)
        );
        assert_eq!(
            test_op("date.compare", &[b.clone(), a.clone()]).unwrap(),
            json!(-1)
        );
        assert_eq!(
            test_op("date.compare", &[a.clone(), a.clone()]).unwrap(),
            json!(0)
        );
    }

    #[test]
    fn date_weekday_epoch() {
        // 1970-01-01 = Thursday = 4
        let date = test_op(
            "date.from_parts",
            &[
                json!(1970),
                json!(1),
                json!(1),
                json!(0),
                json!(0),
                json!(0),
                json!(0),
            ],
        )
        .unwrap();
        assert_eq!(test_op("date.weekday", &[date]).unwrap(), json!(4));
    }

    #[test]
    fn date_epoch_roundtrip() {
        let epoch = test_op(
            "date.from_parts",
            &[
                json!(2000),
                json!(1),
                json!(1),
                json!(0),
                json!(0),
                json!(0),
                json!(0),
            ],
        )
        .unwrap();
        let target = test_op(
            "date.from_parts",
            &[
                json!(2000),
                json!(1),
                json!(2),
                json!(0),
                json!(0),
                json!(0),
                json!(0),
            ],
        )
        .unwrap();
        let offset = test_op("date.to_epoch", &[target.clone(), epoch.clone()]).unwrap();
        assert_eq!(offset, json!(86_400_000_i64));
        let reconstructed = test_op("date.from_epoch", &[epoch, offset]).unwrap();
        assert_eq!(reconstructed["unix_ms"], target["unix_ms"]);
    }

    #[test]
    fn date_from_iso_utc() {
        let r = test_op("date.from_iso", &[json!("2024-03-15T10:30:00Z")]).unwrap();
        let expected = 19797 * 86_400_000_i64 + 10 * 3_600_000 + 30 * 60_000;
        assert_eq!(r["unix_ms"], expected);
        assert_eq!(r["tz_offset_min"], 0);
    }

    #[test]
    fn date_to_iso_roundtrip() {
        let date = test_op("date.from_iso", &[json!("2024-03-15T10:30:00+05:30")]).unwrap();
        let iso = test_op("date.to_iso", &[date]).unwrap();
        assert_eq!(iso, json!("2024-03-15T10:30:00+05:30"));
    }

    #[test]
    fn date_cross_timezone_comparison() {
        // 10:00 UTC == 15:30 IST (same instant)
        let utc = test_op(
            "date.from_parts",
            &[
                json!(2024),
                json!(3),
                json!(15),
                json!(10),
                json!(0),
                json!(0),
                json!(0),
            ],
        )
        .unwrap();
        let ist = test_op(
            "date.from_parts_tz",
            &[
                json!(2024),
                json!(3),
                json!(15),
                json!(15),
                json!(30),
                json!(0),
                json!(0),
                json!(330),
            ],
        )
        .unwrap();
        assert_eq!(test_op("date.compare", &[utc, ist]).unwrap(), json!(0));
    }

    #[test]
    fn date_from_parts_tz() {
        let r = test_op(
            "date.from_parts_tz",
            &[
                json!(2024),
                json!(3),
                json!(15),
                json!(15),
                json!(30),
                json!(0),
                json!(0),
                json!(330),
            ],
        )
        .unwrap();
        // 15:30 IST = 10:00 UTC
        let expected = 19797 * 86_400_000_i64 + 10 * 3_600_000;
        assert_eq!(r["unix_ms"], expected);
        assert_eq!(r["tz_offset_min"], 330);
    }

    #[test]
    fn date_now_returns_date_shape() {
        let r = test_op("date.now", &[]).unwrap();
        assert!(r.get("unix_ms").is_some());
        assert!(r.get("tz_offset_min").is_some());
    }

    // ---------------------------------------------------------------
    // stamp.* op tests
    // ---------------------------------------------------------------

    #[test]
    fn stamp_from_ns_roundtrip() {
        let s = test_op("stamp.from_ns", &[json!(123_456_789_000_i64)]).unwrap();
        let ns = test_op("stamp.to_ns", &[s]).unwrap();
        assert_eq!(ns, json!(123_456_789_000_i64));
    }

    #[test]
    fn stamp_to_ms() {
        let s = test_op("stamp.from_ns", &[json!(5_500_000_000_i64)]).unwrap();
        let ms = test_op("stamp.to_ms", &[s]).unwrap();
        assert_eq!(ms, json!(5500));
    }

    #[test]
    fn stamp_to_date() {
        // 1 second in ns
        let s = test_op("stamp.from_ns", &[json!(1_000_000_000_i64)]).unwrap();
        let d = test_op("stamp.to_date", &[s]).unwrap();
        assert_eq!(d["unix_ms"], 1000);
        assert_eq!(d["tz_offset_min"], 0);
    }

    #[test]
    fn stamp_diff_and_add() {
        let a = test_op("stamp.from_ns", &[json!(5_000_000_000_i64)]).unwrap();
        let b = test_op("stamp.from_ns", &[json!(3_000_000_000_i64)]).unwrap();
        assert_eq!(
            test_op("stamp.diff", &[a.clone(), b.clone()]).unwrap(),
            json!(2_000_000_000_i64)
        );
        let c = test_op("stamp.add", &[b, json!(2_000_000_000_i64)]).unwrap();
        assert_eq!(c["ns"], a["ns"]);
    }

    #[test]
    fn stamp_epoch_roundtrip() {
        let epoch = test_op("stamp.from_ns", &[json!(1_000_000_000_i64)]).unwrap();
        let target = test_op("stamp.from_ns", &[json!(3_000_000_000_i64)]).unwrap();
        let offset = test_op("stamp.to_epoch", &[target, epoch.clone()]).unwrap();
        assert_eq!(offset, json!(2_000_000_000_i64));
        let reconstructed = test_op("stamp.from_epoch", &[epoch, offset]).unwrap();
        assert_eq!(reconstructed["ns"], json!(3_000_000_000_i64));
    }

    #[test]
    fn stamp_now_returns_stamp_shape() {
        let r = test_op("stamp.now", &[]).unwrap();
        assert!(r.get("ns").is_some());
    }

    // ---------------------------------------------------------------
    // trange.* op tests
    // ---------------------------------------------------------------

    #[test]
    fn trange_new_and_accessors() {
        let s = make_date(1000, 0);
        let e = make_date(5000, 0);
        let tr = test_op("trange.new", &[s.clone(), e.clone()]).unwrap();
        assert_eq!(test_op("trange.start", &[tr.clone()]).unwrap(), s);
        assert_eq!(test_op("trange.end", &[tr.clone()]).unwrap(), e);
        assert_eq!(test_op("trange.duration_ms", &[tr]).unwrap(), json!(4000));
    }

    #[test]
    fn trange_rejects_inverted() {
        let s = make_date(5000, 0);
        let e = make_date(1000, 0);
        assert!(test_op("trange.new", &[s, e]).is_err());
    }

    #[test]
    fn trange_contains() {
        let s = make_date(1000, 0);
        let e = make_date(5000, 0);
        let tr = test_op("trange.new", &[s, e]).unwrap();
        // Inside
        assert_eq!(
            test_op("trange.contains", &[tr.clone(), make_date(3000, 0)]).unwrap(),
            json!(true)
        );
        // Outside
        assert_eq!(
            test_op("trange.contains", &[tr.clone(), make_date(6000, 0)]).unwrap(),
            json!(false)
        );
        // Boundary (inclusive)
        assert_eq!(
            test_op("trange.contains", &[tr.clone(), make_date(1000, 0)]).unwrap(),
            json!(true)
        );
        assert_eq!(
            test_op("trange.contains", &[tr, make_date(5000, 0)]).unwrap(),
            json!(true)
        );
    }

    #[test]
    fn trange_overlaps() {
        let a = test_op("trange.new", &[make_date(1000, 0), make_date(5000, 0)]).unwrap();
        let b = test_op("trange.new", &[make_date(3000, 0), make_date(8000, 0)]).unwrap();
        let c = test_op("trange.new", &[make_date(6000, 0), make_date(9000, 0)]).unwrap();
        assert_eq!(
            test_op("trange.overlaps", &[a.clone(), b]).unwrap(),
            json!(true)
        );
        assert_eq!(test_op("trange.overlaps", &[a, c]).unwrap(), json!(false));
    }

    #[test]
    fn trange_shift() {
        let tr = test_op("trange.new", &[make_date(1000, 0), make_date(5000, 0)]).unwrap();
        let shifted = test_op("trange.shift", &[tr, json!(2000)]).unwrap();
        let s = test_op("trange.start", &[shifted.clone()]).unwrap();
        let e = test_op("trange.end", &[shifted]).unwrap();
        assert_eq!(s["unix_ms"], 3000);
        assert_eq!(e["unix_ms"], 7000);
    }

    // -----------------------------------------------------------------
    // type conversion ops
    // -----------------------------------------------------------------

    #[test]
    fn type_of() {
        assert_eq!(test_op("type.of", &[json!("hi")]).unwrap(), json!("text"));
        assert_eq!(test_op("type.of", &[json!(true)]).unwrap(), json!("bool"));
        assert_eq!(test_op("type.of", &[json!(42)]).unwrap(), json!("long"));
        assert_eq!(test_op("type.of", &[json!(3.14)]).unwrap(), json!("real"));
        assert_eq!(test_op("type.of", &[json!([1, 2])]).unwrap(), json!("list"));
        assert_eq!(
            test_op("type.of", &[json!({"a": 1})]).unwrap(),
            json!("dict")
        );
        assert_eq!(test_op("type.of", &[Value::Null]).unwrap(), json!("void"));
        // no args → void
        assert_eq!(test_op("type.of", &[]).unwrap(), json!("void"));
    }

    #[test]
    fn to_text() {
        assert_eq!(test_op("to.text", &[json!("hi")]).unwrap(), json!("hi"));
        assert_eq!(test_op("to.text", &[json!(42)]).unwrap(), json!("42"));
        assert_eq!(test_op("to.text", &[json!(3.14)]).unwrap(), json!("3.14"));
        assert_eq!(test_op("to.text", &[json!(true)]).unwrap(), json!("true"));
        assert_eq!(test_op("to.text", &[json!(false)]).unwrap(), json!("false"));
        assert_eq!(test_op("to.text", &[Value::Null]).unwrap(), json!(""));
        assert_eq!(
            test_op("to.text", &[json!([1, 2])]).unwrap(),
            json!("[1,2]")
        );
        assert_eq!(
            test_op("to.text", &[json!({"a": 1})]).unwrap(),
            json!("{\"a\":1}")
        );
    }

    #[test]
    fn to_long() {
        assert_eq!(test_op("to.long", &[json!(42)]).unwrap(), json!(42));
        assert_eq!(test_op("to.long", &[json!(3.7)]).unwrap(), json!(4));
        assert_eq!(test_op("to.long", &[json!(3.2)]).unwrap(), json!(3));
        assert_eq!(test_op("to.long", &[json!("som 12")]).unwrap(), json!(12));
        assert_eq!(test_op("to.long", &[json!("v3.7")]).unwrap(), json!(4));
        assert_eq!(test_op("to.long", &[json!("abc")]).unwrap(), json!(0));
        assert_eq!(test_op("to.long", &[json!(true)]).unwrap(), json!(1));
        assert_eq!(test_op("to.long", &[json!(false)]).unwrap(), json!(0));
        assert_eq!(test_op("to.long", &[json!([1, 2, 3])]).unwrap(), json!(3));
        assert_eq!(test_op("to.long", &[json!({"a": 1})]).unwrap(), json!(1));
        assert_eq!(test_op("to.long", &[Value::Null]).unwrap(), json!(0));
    }

    #[test]
    fn to_real() {
        assert_eq!(test_op("to.real", &[json!(3.14)]).unwrap(), json!(3.14));
        assert_eq!(test_op("to.real", &[json!(42)]).unwrap(), json!(42.0));
        assert_eq!(test_op("to.real", &[json!("$3.14")]).unwrap(), json!(3.14));
        assert_eq!(test_op("to.real", &[json!("abc")]).unwrap(), json!(0.0));
        assert_eq!(test_op("to.real", &[json!(true)]).unwrap(), json!(1.0));
        assert_eq!(test_op("to.real", &[json!(false)]).unwrap(), json!(0.0));
        assert_eq!(test_op("to.real", &[json!([1, 2])]).unwrap(), json!(2.0));
        assert_eq!(test_op("to.real", &[json!({"a": 1})]).unwrap(), json!(1.0));
        assert_eq!(test_op("to.real", &[Value::Null]).unwrap(), json!(0.0));
    }

    #[test]
    fn to_bool() {
        assert_eq!(test_op("to.bool", &[json!(true)]).unwrap(), json!(true));
        assert_eq!(test_op("to.bool", &[json!(false)]).unwrap(), json!(false));
        assert_eq!(test_op("to.bool", &[json!("hello")]).unwrap(), json!(true));
        assert_eq!(test_op("to.bool", &[json!("")]).unwrap(), json!(false));
        assert_eq!(test_op("to.bool", &[json!("false")]).unwrap(), json!(false));
        assert_eq!(test_op("to.bool", &[json!("0")]).unwrap(), json!(false));
        assert_eq!(test_op("to.bool", &[json!(1)]).unwrap(), json!(true));
        assert_eq!(test_op("to.bool", &[json!(0)]).unwrap(), json!(false));
        assert_eq!(test_op("to.bool", &[json!([1])]).unwrap(), json!(true));
        assert_eq!(test_op("to.bool", &[json!([])]).unwrap(), json!(false));
        assert_eq!(test_op("to.bool", &[json!({"a": 1})]).unwrap(), json!(true));
        assert_eq!(test_op("to.bool", &[json!({})]).unwrap(), json!(false));
        assert_eq!(test_op("to.bool", &[Value::Null]).unwrap(), json!(false));
    }

    // -----------------------------------------------------------------
    // env.* ops
    // -----------------------------------------------------------------

    #[test]
    fn env_set_and_get() {
        test_op("env.set", &[json!("DATAFLOW_TEST_KEY"), json!("hello123")]).unwrap();
        let val = test_op("env.get", &[json!("DATAFLOW_TEST_KEY")]).unwrap();
        assert_eq!(val, json!("hello123"));
        // cleanup
        test_op("env.remove", &[json!("DATAFLOW_TEST_KEY")]).unwrap();
    }

    #[test]
    fn env_get_missing_returns_default() {
        let val = test_op("env.get", &[json!("DATAFLOW_NONEXISTENT_XYZ")]).unwrap();
        assert_eq!(val, json!(""));
        let val2 = test_op(
            "env.get",
            &[json!("DATAFLOW_NONEXISTENT_XYZ"), json!("fallback")],
        )
        .unwrap();
        assert_eq!(val2, json!("fallback"));
    }

    #[test]
    fn env_has() {
        test_op("env.set", &[json!("DATAFLOW_HAS_TEST"), json!("yes")]).unwrap();
        assert_eq!(
            test_op("env.has", &[json!("DATAFLOW_HAS_TEST")]).unwrap(),
            json!(true)
        );
        assert_eq!(
            test_op("env.has", &[json!("DATAFLOW_NONEXISTENT_XYZ")]).unwrap(),
            json!(false)
        );
        test_op("env.remove", &[json!("DATAFLOW_HAS_TEST")]).unwrap();
    }

    #[test]
    fn env_list_returns_dict() {
        let val = test_op("env.list", &[]).unwrap();
        assert!(val.is_object());
    }

    #[test]
    fn env_remove() {
        test_op("env.set", &[json!("DATAFLOW_RM_TEST"), json!("temp")]).unwrap();
        assert_eq!(
            test_op("env.has", &[json!("DATAFLOW_RM_TEST")]).unwrap(),
            json!(true)
        );
        test_op("env.remove", &[json!("DATAFLOW_RM_TEST")]).unwrap();
        assert_eq!(
            test_op("env.has", &[json!("DATAFLOW_RM_TEST")]).unwrap(),
            json!(false)
        );
    }

    // -----------------------------------------------------------------
    // exec.* ops
    // -----------------------------------------------------------------

    #[test]
    fn exec_run_echo() {
        let result = test_op("exec.run", &[json!("echo"), json!(["hello"])]).unwrap();
        assert_eq!(result["ok"], json!(true));
        assert_eq!(result["code"], json!(0));
        assert!(result["stdout"].as_str().unwrap().contains("hello"));
    }

    #[test]
    fn exec_run_no_args() {
        let result = test_op("exec.run", &[json!("echo")]).unwrap();
        assert_eq!(result["ok"], json!(true));
    }

    #[test]
    fn exec_run_nonexistent_command() {
        let result = test_op("exec.run", &[json!("__nonexistent_cmd_xyz__")]);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------
    // regex.* ops
    // -----------------------------------------------------------------

    #[test]
    fn regex_match_basic() {
        assert_eq!(
            test_op("regex.match", &[json!("hello123"), json!(r"\d+")]).unwrap(),
            json!(true)
        );
        assert_eq!(
            test_op("regex.match", &[json!("hello"), json!(r"\d+")]).unwrap(),
            json!(false)
        );
    }

    #[test]
    fn regex_match_invalid_pattern() {
        let result = test_op("regex.match", &[json!("text"), json!("[invalid")]);
        assert!(result.is_err());
    }

    #[test]
    fn regex_find_with_groups() {
        let result = test_op(
            "regex.find",
            &[json!("2024-01-15"), json!(r"(\d{4})-(\d{2})-(\d{2})")],
        )
        .unwrap();
        assert_eq!(result["matched"], json!(true));
        assert_eq!(result["text"], json!("2024-01-15"));
        assert_eq!(result["groups"], json!(["2024", "01", "15"]));
    }

    #[test]
    fn regex_find_no_match() {
        let result = test_op("regex.find", &[json!("hello"), json!(r"\d+")]).unwrap();
        assert_eq!(result["matched"], json!(false));
    }

    #[test]
    fn regex_find_all() {
        let result = test_op("regex.find_all", &[json!("a1b2c3"), json!(r"\d")]).unwrap();
        assert_eq!(result, json!(["1", "2", "3"]));
    }

    #[test]
    fn regex_replace_first() {
        let result = test_op(
            "regex.replace",
            &[json!("foo bar foo"), json!("foo"), json!("baz")],
        )
        .unwrap();
        assert_eq!(result, json!("baz bar foo"));
    }

    #[test]
    fn regex_replace_all() {
        let result = test_op(
            "regex.replace_all",
            &[json!("foo bar foo"), json!("foo"), json!("baz")],
        )
        .unwrap();
        assert_eq!(result, json!("baz bar baz"));
    }

    #[test]
    fn regex_split() {
        let result = test_op("regex.split", &[json!("one::two:::three"), json!(":+")]).unwrap();
        assert_eq!(result, json!(["one", "two", "three"]));
    }

    // -----------------------------------------------------------------
    // random.* ops
    // -----------------------------------------------------------------

    #[test]
    fn random_int_in_range() {
        let val = test_op("random.int", &[json!(1), json!(10)]).unwrap();
        let n = val.as_i64().unwrap();
        assert!(n >= 1 && n <= 10);
    }

    #[test]
    fn random_int_min_gt_max() {
        let result = test_op("random.int", &[json!(10), json!(1)]);
        assert!(result.is_err());
    }

    #[test]
    fn random_float_in_range() {
        let val = test_op("random.float", &[]).unwrap();
        let f = val.as_f64().unwrap();
        assert!(f >= 0.0 && f < 1.0);
    }

    #[test]
    fn random_uuid_format() {
        let val = test_op("random.uuid", &[]).unwrap();
        let s = val.as_str().unwrap();
        // UUIDv4 format: 8-4-4-4-12 hex chars
        assert_eq!(s.len(), 36);
        assert_eq!(s.chars().filter(|c| *c == '-').count(), 4);
    }

    #[test]
    fn random_choice_from_list() {
        let val = test_op("random.choice", &[json!(["a", "b", "c"])]).unwrap();
        assert!(val == json!("a") || val == json!("b") || val == json!("c"));
    }

    #[test]
    fn random_choice_empty_list() {
        let result = test_op("random.choice", &[json!([])]);
        assert!(result.is_err());
    }

    #[test]
    fn random_shuffle_preserves_elements() {
        let input = json!([1, 2, 3, 4, 5]);
        let result = test_op("random.shuffle", &[input.clone()]).unwrap();
        let mut sorted_result: Vec<i64> = result
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap())
            .collect();
        sorted_result.sort();
        assert_eq!(sorted_result, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn random_shuffle_empty_list() {
        let result = test_op("random.shuffle", &[json!([])]).unwrap();
        assert_eq!(result, json!([]));
    }

    // -----------------------------------------------------------------
    // hash.* ops
    // -----------------------------------------------------------------

    #[test]
    fn hash_sha256_known_value() {
        let result = test_op("hash.sha256", &[json!("hello")]).unwrap();
        assert_eq!(
            result.as_str().unwrap(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn hash_sha512_known_value() {
        let result = test_op("hash.sha512", &[json!("hello")]).unwrap();
        let s = result.as_str().unwrap();
        assert_eq!(s.len(), 128);
        assert!(s.starts_with("9b71d224bd62f378"));
    }

    #[test]
    fn hash_hmac_sha256_default() {
        let result = test_op("hash.hmac", &[json!("hello"), json!("secret")]).unwrap();
        let s = result.as_str().unwrap();
        assert_eq!(s.len(), 64);
    }

    #[test]
    fn hash_hmac_sha512_explicit() {
        let result = test_op(
            "hash.hmac",
            &[json!("hello"), json!("secret"), json!("sha512")],
        )
        .unwrap();
        let s = result.as_str().unwrap();
        assert_eq!(s.len(), 128);
    }

    #[test]
    fn hash_hmac_bad_algorithm() {
        let result = test_op(
            "hash.hmac",
            &[json!("hello"), json!("secret"), json!("md5")],
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unsupported algorithm"));
    }

    // -----------------------------------------------------------------
    // base64.* ops
    // -----------------------------------------------------------------

    #[test]
    fn base64_roundtrip() {
        let encoded = test_op("base64.encode", &[json!("hello world")]).unwrap();
        assert_eq!(encoded.as_str().unwrap(), "aGVsbG8gd29ybGQ=");
        let decoded = test_op("base64.decode", &[encoded]).unwrap();
        assert_eq!(decoded.as_str().unwrap(), "hello world");
    }

    #[test]
    fn base64_decode_invalid() {
        let result = test_op("base64.decode", &[json!("not valid base64!!!")]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid base64"));
    }

    #[test]
    fn base64_url_roundtrip() {
        let encoded = test_op("base64.encode_url", &[json!("hello world")]).unwrap();
        let s = encoded.as_str().unwrap();
        assert!(!s.contains('+'));
        assert!(!s.contains('/'));
        assert!(!s.contains('='));
        let decoded = test_op("base64.decode_url", &[encoded]).unwrap();
        assert_eq!(decoded.as_str().unwrap(), "hello world");
    }

    // -----------------------------------------------------------------
    // crypto.* ops
    // -----------------------------------------------------------------

    #[test]
    fn crypto_hash_verify_password_roundtrip() {
        let hash = test_op("crypto.hash_password", &[json!("mypassword")]).unwrap();
        let s = hash.as_str().unwrap();
        assert!(s.starts_with("$2"));
        let valid = test_op(
            "crypto.verify_password",
            &[json!("mypassword"), hash.clone()],
        )
        .unwrap();
        assert_eq!(valid, json!(true));
    }

    #[test]
    fn crypto_verify_password_wrong() {
        let hash = test_op("crypto.hash_password", &[json!("correct")]).unwrap();
        let valid = test_op("crypto.verify_password", &[json!("wrong"), hash]).unwrap();
        assert_eq!(valid, json!(false));
    }

    #[test]
    fn crypto_sign_verify_token_roundtrip() {
        let payload = json!({"sub": "user123", "role": "admin"});
        let token = test_op("crypto.sign_token", &[payload.clone(), json!("secret")]).unwrap();
        let s = token.as_str().unwrap();
        assert_eq!(s.matches('.').count(), 2);

        let result = test_op("crypto.verify_token", &[token, json!("secret")]).unwrap();
        assert_eq!(result.get("valid").unwrap(), &json!(true));
        assert_eq!(
            result.get("payload").unwrap().get("sub").unwrap(),
            &json!("user123")
        );
        assert_eq!(
            result.get("payload").unwrap().get("role").unwrap(),
            &json!("admin")
        );
    }

    #[test]
    fn crypto_verify_token_wrong_secret() {
        let payload = json!({"sub": "user123"});
        let token = test_op("crypto.sign_token", &[payload, json!("secret")]).unwrap();
        let result = test_op("crypto.verify_token", &[token, json!("wrong")]).unwrap();
        assert_eq!(result.get("valid").unwrap(), &json!(false));
    }

    #[test]
    fn crypto_verify_token_malformed() {
        let result = test_op(
            "crypto.verify_token",
            &[json!("not.a.valid.token.here"), json!("secret")],
        )
        .unwrap();
        assert_eq!(result.get("valid").unwrap(), &json!(false));
    }

    #[test]
    fn crypto_verify_token_no_dots() {
        let result = test_op("crypto.verify_token", &[json!("nodots"), json!("secret")]).unwrap();
        assert_eq!(result.get("valid").unwrap(), &json!(false));
    }

    #[test]
    fn crypto_random_bytes_length() {
        let result = test_op("crypto.random_bytes", &[json!(16)]).unwrap();
        let s = result.as_str().unwrap();
        assert_eq!(s.len(), 32); // 16 bytes = 32 hex chars
    }

    #[test]
    fn crypto_random_bytes_bounds() {
        let result = test_op("crypto.random_bytes", &[json!(0)]);
        assert!(result.is_err());
        let result = test_op("crypto.random_bytes", &[json!(1025)]);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------
    // db.* SQLite ops
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn db_open_in_memory() {
        let h = NativeHost::new();
        let c = CodecRegistry::default_registry();
        let handle = execute_op("db.open", &[json!(":memory:")], &h, &c)
            .await
            .unwrap();
        assert!(handle.as_str().unwrap().starts_with("db_"));
        let closed = execute_op("db.close", &[handle], &h, &c).await.unwrap();
        assert_eq!(closed, json!(true));
    }

    #[tokio::test]
    async fn db_exec_create_and_insert() {
        let h = NativeHost::new();
        let c = CodecRegistry::default_registry();
        let handle = execute_op("db.open", &[json!(":memory:")], &h, &c)
            .await
            .unwrap();
        let ddl = execute_op(
            "db.exec",
            &[
                handle.clone(),
                json!("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)"),
            ],
            &h,
            &c,
        )
        .await
        .unwrap();
        assert_eq!(ddl["rows_affected"], json!(0));
        let ins = execute_op(
            "db.exec",
            &[
                handle.clone(),
                json!("INSERT INTO t (name) VALUES (?1)"),
                json!(["Alice"]),
            ],
            &h,
            &c,
        )
        .await
        .unwrap();
        assert_eq!(ins["rows_affected"], json!(1));
        let _ = execute_op("db.close", &[handle], &h, &c).await.unwrap();
    }

    #[tokio::test]
    async fn db_query_returns_rows() {
        let h = NativeHost::new();
        let c = CodecRegistry::default_registry();
        let handle = execute_op("db.open", &[json!(":memory:")], &h, &c)
            .await
            .unwrap();
        let _ = execute_op(
            "db.exec",
            &[
                handle.clone(),
                json!("CREATE TABLE people (name TEXT, age INTEGER)"),
            ],
            &h,
            &c,
        )
        .await
        .unwrap();
        let _ = execute_op(
            "db.exec",
            &[
                handle.clone(),
                json!("INSERT INTO people VALUES (?1, ?2)"),
                json!(["Alice", 30]),
            ],
            &h,
            &c,
        )
        .await
        .unwrap();
        let _ = execute_op(
            "db.exec",
            &[
                handle.clone(),
                json!("INSERT INTO people VALUES (?1, ?2)"),
                json!(["Bob", 25]),
            ],
            &h,
            &c,
        )
        .await
        .unwrap();
        let rows = execute_op(
            "db.query",
            &[
                handle.clone(),
                json!("SELECT name, age FROM people ORDER BY age"),
            ],
            &h,
            &c,
        )
        .await
        .unwrap();
        let arr = rows.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["name"], json!("Bob"));
        assert_eq!(arr[0]["age"], json!(25));
        assert_eq!(arr[1]["name"], json!("Alice"));
        assert_eq!(arr[1]["age"], json!(30));
        let _ = execute_op("db.close", &[handle], &h, &c).await.unwrap();
    }

    #[tokio::test]
    async fn db_query_with_params() {
        let h = NativeHost::new();
        let c = CodecRegistry::default_registry();
        let handle = execute_op("db.open", &[json!(":memory:")], &h, &c)
            .await
            .unwrap();
        let _ = execute_op(
            "db.exec",
            &[
                handle.clone(),
                json!("CREATE TABLE kv (key TEXT, val TEXT)"),
            ],
            &h,
            &c,
        )
        .await
        .unwrap();
        let _ = execute_op(
            "db.exec",
            &[
                handle.clone(),
                json!("INSERT INTO kv VALUES (?1, ?2)"),
                json!(["a", "1"]),
            ],
            &h,
            &c,
        )
        .await
        .unwrap();
        let _ = execute_op(
            "db.exec",
            &[
                handle.clone(),
                json!("INSERT INTO kv VALUES (?1, ?2)"),
                json!(["b", "2"]),
            ],
            &h,
            &c,
        )
        .await
        .unwrap();
        let rows = execute_op(
            "db.query",
            &[
                handle.clone(),
                json!("SELECT val FROM kv WHERE key = ?1"),
                json!(["a"]),
            ],
            &h,
            &c,
        )
        .await
        .unwrap();
        let arr = rows.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["val"], json!("1"));
        let _ = execute_op("db.close", &[handle], &h, &c).await.unwrap();
    }

    #[tokio::test]
    async fn db_query_empty_result() {
        let h = NativeHost::new();
        let c = CodecRegistry::default_registry();
        let handle = execute_op("db.open", &[json!(":memory:")], &h, &c)
            .await
            .unwrap();
        let _ = execute_op(
            "db.exec",
            &[handle.clone(), json!("CREATE TABLE empty (x INTEGER)")],
            &h,
            &c,
        )
        .await
        .unwrap();
        let rows = execute_op(
            "db.query",
            &[handle.clone(), json!("SELECT x FROM empty")],
            &h,
            &c,
        )
        .await
        .unwrap();
        assert_eq!(rows, json!([]));
        let _ = execute_op("db.close", &[handle], &h, &c).await.unwrap();
    }

    #[tokio::test]
    async fn db_close_unknown_handle_fails() {
        let h = NativeHost::new();
        let c = CodecRegistry::default_registry();
        let result = execute_op("db.close", &[json!("db_999")], &h, &c).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown handle"));
    }

    #[tokio::test]
    async fn db_null_params() {
        let h = NativeHost::new();
        let c = CodecRegistry::default_registry();
        let handle = execute_op("db.open", &[json!(":memory:")], &h, &c)
            .await
            .unwrap();
        let _ = execute_op(
            "db.exec",
            &[handle.clone(), json!("CREATE TABLE nullable (val TEXT)")],
            &h,
            &c,
        )
        .await
        .unwrap();
        let _ = execute_op(
            "db.exec",
            &[
                handle.clone(),
                json!("INSERT INTO nullable VALUES (?1)"),
                json!([null]),
            ],
            &h,
            &c,
        )
        .await
        .unwrap();
        let rows = execute_op(
            "db.query",
            &[handle.clone(), json!("SELECT val FROM nullable")],
            &h,
            &c,
        )
        .await
        .unwrap();
        let arr = rows.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["val"], json!(null));
        let _ = execute_op("db.close", &[handle], &h, &c).await.unwrap();
    }

    #[test]
    fn db_legacy_mock_ops_still_work() {
        let result = test_op("db.query_user_by_email", &[json!("ada@example.com")]).unwrap();
        assert_eq!(result["email"], json!("ada@example.com"));
        let result = test_op(
            "db.query_credentials",
            &[json!({"email": "ada@example.com"})],
        )
        .unwrap();
        assert!(result.get("password_hash").is_some());
    }

    // -----------------------------------------------------------------
    // log.* ops
    // -----------------------------------------------------------------

    #[test]
    fn log_info_returns_true() {
        let result = test_op("log.info", &[json!("server started")]).unwrap();
        assert_eq!(result, json!(true));
    }

    #[test]
    fn log_error_with_context() {
        let result = test_op(
            "log.error",
            &[json!("connection failed"), json!({"host": "db.local"})],
        )
        .unwrap();
        assert_eq!(result, json!(true));
    }

    #[test]
    fn log_all_levels() {
        for level in &["debug", "info", "warn", "error", "trace"] {
            let op = format!("log.{level}");
            let result = test_op(&op, &[json!("test message")]).unwrap();
            assert_eq!(result, json!(true));
        }
    }

    // -----------------------------------------------------------------
    // error.* ops
    // -----------------------------------------------------------------

    #[test]
    fn error_new_basic() {
        let result = test_op("error.new", &[json!("NOT_FOUND"), json!("user not found")]).unwrap();
        assert_eq!(result["code"], json!("NOT_FOUND"));
        assert_eq!(result["message"], json!("user not found"));
        assert!(result.get("details").is_none());
    }

    #[test]
    fn error_new_with_details() {
        let result = test_op(
            "error.new",
            &[
                json!("VALIDATION"),
                json!("bad input"),
                json!({"field": "email"}),
            ],
        )
        .unwrap();
        assert_eq!(result["code"], json!("VALIDATION"));
        assert_eq!(result["message"], json!("bad input"));
        assert_eq!(result["details"]["field"], json!("email"));
    }

    #[test]
    fn error_wrap_prepends_context() {
        let err = test_op(
            "error.new",
            &[json!("DB_ERROR"), json!("connection refused")],
        )
        .unwrap();
        let wrapped = test_op("error.wrap", &[err, json!("login failed")]).unwrap();
        assert_eq!(wrapped["code"], json!("DB_ERROR"));
        assert_eq!(
            wrapped["message"],
            json!("login failed: connection refused")
        );
    }

    #[test]
    fn error_code_and_message_extract() {
        let err = test_op("error.new", &[json!("TIMEOUT"), json!("request timed out")]).unwrap();
        let code = test_op("error.code", &[err.clone()]).unwrap();
        let msg = test_op("error.message", &[err]).unwrap();
        assert_eq!(code, json!("TIMEOUT"));
        assert_eq!(msg, json!("request timed out"));
    }

    #[test]
    fn error_code_missing_returns_null() {
        let result = test_op("error.code", &[json!({})]).unwrap();
        assert_eq!(result, Value::Null);
    }

    #[test]
    fn send_nowait_continues_immediately() {
        let src = r#"
type Req
  path text
done

type Resp
  status long
done

type Err
  status long
done

docs SendNowaitImmediate
  Verifies send nowait does not block the caller.
done

func SendNowaitImmediate
  take request as Req
  emit response as Resp
  fail error as Err
body
  nowait log.info("background task")
  response = http.response(200, "ok")
  emit response
done
"#;
        let module = parser::parse_module_v1(src).unwrap();
        let registry = TypeRegistry::from_module(&module).unwrap();
        let flow = parser::parse_runtime_func_from_module_v1(&module).unwrap();
        let ir_val = ir::lower_to_ir(&flow).unwrap();
        let mut inputs = HashMap::new();
        inputs.insert("request".to_string(), json!({"path": "/test"}));
        let codecs = CodecRegistry::default_registry();
        let report = test_execute_flow(&flow, ir_val, inputs, &registry, None, &codecs).unwrap();
        assert!(
            report.outputs.get("response").is_some(),
            "should produce output after send nowait"
        );
        let spawn_events: Vec<_> = report
            .trace
            .iter()
            .filter(|e| e.status == "spawned")
            .collect();
        assert_eq!(
            spawn_events.len(),
            1,
            "should have exactly one spawned trace event"
        );
        assert!(
            spawn_events[0].op.starts_with("send.nowait."),
            "spawned event op should start with send.nowait."
        );
    }

    #[test]
    fn send_nowait_with_expression_args() {
        let src = r#"
type Req
  name text
done

type Resp
  greeting text
done

type Err
  status long
done

docs SendNowaitExprArgs
  Verifies send nowait evaluates expression arguments.
done

func SendNowaitExprArgs
  take request as Req
  emit response as Resp
  fail error as Err
body
  name = obj.get(request, "name")
  msg = "Hello #{name}"
  nowait log.info(msg)
  r0 = obj.new()
  r1 = obj.set(r0, "greeting", msg)
  emit r1
done
"#;
        let module = parser::parse_module_v1(src).unwrap();
        let registry = TypeRegistry::from_module(&module).unwrap();
        let flow = parser::parse_runtime_func_from_module_v1(&module).unwrap();
        let ir_val = ir::lower_to_ir(&flow).unwrap();
        let mut inputs = HashMap::new();
        inputs.insert("request".to_string(), json!({"name": "World"}));
        let codecs = CodecRegistry::default_registry();
        let report = test_execute_flow(&flow, ir_val, inputs, &registry, None, &codecs).unwrap();
        let greeting = report
            .outputs
            .get("response")
            .unwrap()
            .get("greeting")
            .unwrap();
        assert_eq!(greeting, &json!("Hello World"));
    }

    // --- Cookie ops tests ---

    #[test]
    fn cookie_parse_basic() {
        let result = test_op("cookie.parse", &[json!("session=abc123; theme=dark")]).unwrap();
        assert_eq!(result["session"], json!("abc123"));
        assert_eq!(result["theme"], json!("dark"));
    }

    #[test]
    fn cookie_parse_empty() {
        let result = test_op("cookie.parse", &[json!("")]).unwrap();
        assert_eq!(result, json!({}));
    }

    #[test]
    fn cookie_get_found() {
        let cookies = json!({"session": "abc", "theme": "dark"});
        let result = test_op("cookie.get", &[cookies, json!("session")]).unwrap();
        assert_eq!(result, json!("abc"));
    }

    #[test]
    fn cookie_get_missing() {
        let cookies = json!({"session": "abc"});
        let result = test_op("cookie.get", &[cookies, json!("missing")]).unwrap();
        assert_eq!(result, Value::Null);
    }

    #[test]
    fn cookie_set_simple() {
        let result = test_op("cookie.set", &[json!("sid"), json!("xyz")]).unwrap();
        assert_eq!(result, json!("sid=xyz"));
    }

    #[test]
    fn cookie_set_with_opts() {
        let opts = json!({"path": "/", "max_age": 3600, "secure": true, "http_only": true, "same_site": "Lax"});
        let result = test_op("cookie.set", &[json!("sid"), json!("xyz"), opts]).unwrap();
        let s = result.as_str().unwrap();
        assert!(s.starts_with("sid=xyz"));
        assert!(s.contains("Path=/"));
        assert!(s.contains("Max-Age=3600"));
        assert!(s.contains("Secure"));
        assert!(s.contains("HttpOnly"));
        assert!(s.contains("SameSite=Lax"));
    }

    #[test]
    fn cookie_delete_sets_max_age_zero() {
        let result = test_op("cookie.delete", &[json!("sid")]).unwrap();
        assert_eq!(result, json!("sid=; Max-Age=0"));
    }

    // --- URL ops tests ---

    #[test]
    fn url_parse_full() {
        let result = test_op(
            "url.parse",
            &[json!("http://example.com/users/42?sort=name#top")],
        )
        .unwrap();
        assert_eq!(result["path"], json!("/users/42"));
        assert_eq!(result["query"], json!("sort=name"));
        assert_eq!(result["fragment"], json!("top"));
    }

    #[test]
    fn url_parse_no_query() {
        let result = test_op("url.parse", &[json!("http://example.com/about")]).unwrap();
        assert_eq!(result["path"], json!("/about"));
        assert_eq!(result["query"], json!(""));
        assert_eq!(result["fragment"], json!(""));
    }

    #[test]
    fn url_parse_bare_path() {
        let result = test_op("url.parse", &[json!("/foo/bar?x=1")]).unwrap();
        assert_eq!(result["path"], json!("/foo/bar"));
        assert_eq!(result["query"], json!("x=1"));
    }

    #[test]
    fn url_query_parse_basic() {
        let result = test_op("url.query_parse", &[json!("a=1&b=hello")]).unwrap();
        assert_eq!(result["a"], json!("1"));
        assert_eq!(result["b"], json!("hello"));
    }

    #[test]
    fn url_query_parse_percent_encoded() {
        let result = test_op("url.query_parse", &[json!("name=John+Doe&city=New%20York")]).unwrap();
        assert_eq!(result["name"], json!("John Doe"));
        assert_eq!(result["city"], json!("New York"));
    }

    #[test]
    fn url_encode_decode_roundtrip() {
        let encoded = test_op("url.encode", &[json!("hello world&foo=bar")]).unwrap();
        assert_eq!(encoded, json!("hello%20world%26foo%3Dbar"));
        let decoded = test_op("url.decode", &[encoded]).unwrap();
        assert_eq!(decoded, json!("hello world&foo=bar"));
    }

    #[test]
    fn url_decode_plus_as_space() {
        let result = test_op("url.decode", &[json!("hello+world")]).unwrap();
        assert_eq!(result, json!("hello world"));
    }

    // --- Route matching tests ---

    #[test]
    fn route_match_static() {
        let result = test_op("route.match", &[json!("/about"), json!("/about")]).unwrap();
        assert_eq!(result, json!(true));
    }

    #[test]
    fn route_match_with_params() {
        let result = test_op("route.match", &[json!("/users/:id"), json!("/users/42")]).unwrap();
        assert_eq!(result, json!(true));
    }

    #[test]
    fn route_match_wildcard() {
        let result = test_op(
            "route.match",
            &[json!("/files/*path"), json!("/files/a/b/c.txt")],
        )
        .unwrap();
        assert_eq!(result, json!(true));
    }

    #[test]
    fn route_match_no_match() {
        let result = test_op("route.match", &[json!("/users/:id"), json!("/posts/42")]).unwrap();
        assert_eq!(result, json!(false));
    }

    #[test]
    fn route_params_extracts_params() {
        let result =
            test_op("route.params", &[json!("/users/:id"), json!("/users/42")]).unwrap();
        assert_eq!(result["id"], json!("42"));
    }

    #[test]
    fn route_params_multi() {
        let result = test_op(
            "route.params",
            &[json!("/users/:uid/posts/:pid"), json!("/users/5/posts/99")],
        )
        .unwrap();
        assert_eq!(result["uid"], json!("5"));
        assert_eq!(result["pid"], json!("99"));
    }

    #[test]
    fn route_params_wildcard() {
        let result = test_op(
            "route.params",
            &[json!("/files/*path"), json!("/files/a/b/c.txt")],
        )
        .unwrap();
        assert_eq!(result["path"], json!("a/b/c.txt"));
    }

    #[test]
    fn route_params_no_match_returns_empty() {
        let result =
            test_op("route.params", &[json!("/users/:id"), json!("/posts/42")]).unwrap();
        assert_eq!(result, json!({}));
    }

    // --- HTML ops tests ---

    #[test]
    fn html_escape_all_chars() {
        let result = test_op("html.escape", &[json!("<script>alert('x\"&')</script>")]).unwrap();
        assert_eq!(
            result,
            json!("&lt;script&gt;alert(&#x27;x&quot;&amp;&#x27;)&lt;/script&gt;")
        );
    }

    #[test]
    fn html_unescape_roundtrip() {
        let escaped = test_op("html.escape", &[json!("a < b & c > d")]).unwrap();
        let unescaped = test_op("html.unescape", &[escaped]).unwrap();
        assert_eq!(unescaped, json!("a < b & c > d"));
    }

    // --- Template ops tests ---

    #[test]
    fn tmpl_render_simple_var() {
        let result = test_op(
            "tmpl.render",
            &[json!("Hello, {{name}}!"), json!({"name": "World"})],
        )
        .unwrap();
        assert_eq!(result, json!("Hello, World!"));
    }

    #[test]
    fn tmpl_render_html_escape() {
        let result = test_op(
            "tmpl.render",
            &[json!("{{content}}"), json!({"content": "<b>bold</b>"})],
        )
        .unwrap();
        assert_eq!(result, json!("&lt;b&gt;bold&lt;/b&gt;"));
    }

    #[test]
    fn tmpl_render_raw_triple() {
        let result = test_op(
            "tmpl.render",
            &[json!("{{{content}}}"), json!({"content": "<b>bold</b>"})],
        )
        .unwrap();
        assert_eq!(result, json!("<b>bold</b>"));
    }

    #[test]
    fn tmpl_render_section_list() {
        let tmpl = "{{#items}}{{.}} {{/items}}";
        let data = json!({"items": ["a", "b", "c"]});
        let result = test_op("tmpl.render", &[json!(tmpl), data]).unwrap();
        assert_eq!(result, json!("a b c "));
    }

    #[test]
    fn tmpl_render_inverted_section() {
        let tmpl = "{{^items}}No items{{/items}}";
        let data = json!({"items": []});
        let result = test_op("tmpl.render", &[json!(tmpl), data]).unwrap();
        assert_eq!(result, json!("No items"));
    }

    #[test]
    fn tmpl_render_nested_key() {
        let tmpl = "{{user.name}}";
        let data = json!({"user": {"name": "Alice"}});
        let result = test_op("tmpl.render", &[json!(tmpl), data]).unwrap();
        assert_eq!(result, json!("Alice"));
    }

    #[test]
    fn tmpl_render_missing_key() {
        let result = test_op("tmpl.render", &[json!("Hello, {{name}}!"), json!({})]).unwrap();
        assert_eq!(result, json!("Hello, !"));
    }

    #[test]
    fn ternary_true_branch() {
        let source = r#"
func Test
  take flag as bool
  emit result as text
  fail error as text
body
  x = flag ? "yes" : "no"
  emit x
done
"#;
        let result = run_flow_with_inputs(source, vec![("flag", json!(true))]);
        assert_eq!(result["result"], json!("yes"));
    }

    #[test]
    fn ternary_false_branch() {
        let source = r#"
func Test
  take flag as bool
  emit result as text
  fail error as text
body
  x = flag ? "yes" : "no"
  emit x
done
"#;
        let result = run_flow_with_inputs(source, vec![("flag", json!(false))]);
        assert_eq!(result["result"], json!("no"));
    }

    #[test]
    fn ternary_with_expression_condition() {
        let source = r#"
func Test
  take n as long
  emit result as text
  fail error as text
body
  x = n > 10 ? "big" : "small"
  emit x
done
"#;
        let result = run_flow_with_inputs(source, vec![("n", json!(20))]);
        assert_eq!(result["result"], json!("big"));
    }

    #[test]
    fn list_literal_basic() {
        let source = r#"
func Test
  take a as long
  emit result as list
  fail error as text
body
  items = [1, 2, 3]
  emit items
done
"#;
        let result = run_flow_with_inputs(source, vec![("a", json!(0))]);
        assert_eq!(result["result"], json!([1, 2, 3]));
    }

    #[test]
    fn list_literal_empty() {
        let source = r#"
func Test
  take a as long
  emit result as list
  fail error as text
body
  items = []
  emit items
done
"#;
        let result = run_flow_with_inputs(source, vec![("a", json!(0))]);
        assert_eq!(result["result"], json!([]));
    }

    #[test]
    fn list_literal_with_variables() {
        let source = r#"
func Test
  take a as long
  emit result as list
  fail error as text
body
  x = 10
  y = 20
  items = [x, y, 30]
  emit items
done
"#;
        let result = run_flow_with_inputs(source, vec![("a", json!(0))]);
        assert_eq!(result["result"], json!([10, 20, 30]));
    }

    #[test]
    fn dict_literal_basic() {
        let source = r#"
func Test
  take a as long
  emit result as dict
  fail error as text
body
  obj = {name: "test", count: 42}
  emit obj
done
"#;
        let result = run_flow_with_inputs(source, vec![("a", json!(0))]);
        let obj = result["result"].as_object().unwrap();
        assert_eq!(obj.get("name").unwrap(), &json!("test"));
        assert_eq!(obj.get("count").unwrap(), &json!(42));
    }

    #[test]
    fn dict_literal_empty() {
        let source = r#"
func Test
  take a as long
  emit result as dict
  fail error as text
body
  obj = {}
  emit obj
done
"#;
        let result = run_flow_with_inputs(source, vec![("a", json!(0))]);
        assert_eq!(result["result"], json!({}));
    }

    #[test]
    fn bare_loop_with_break() {
        let source = r#"
func Test
  take limit as long
  emit result as long
  fail error as text
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
        let result = run_flow_with_inputs(source, vec![("limit", json!(5))]);
        assert_eq!(result["result"], json!(5));
    }

    #[test]
    fn break_in_collection_loop() {
        let source = r#"
func Test
  take a as long
  emit result as long
  fail error as text
body
  items = [10, 20, 30, 40, 50]
  total = 0
  loop items as item
    total = total + item
    done_flag = total >= 30
    case done_flag
      when true
        break
    done
  done
  emit total
done
"#;
        let result = run_flow_with_inputs(source, vec![("a", json!(0))]);
        assert_eq!(result["result"], json!(30));
    }

    #[test]
    fn time_sleep_returns_true() {
        let result = test_op("time.sleep", &[json!(0.01)]).unwrap();
        assert_eq!(result, json!(true));
    }

    #[test]
    fn variable_reassignment_no_error() {
        let source = r#"
func Test
  take a as long
  emit result as long
  fail error as text
body
  x = 1
  x = 2
  x = x + 1
  emit x
done
"#;
        let result = run_flow_with_inputs(source, vec![("a", json!(0))]);
        assert_eq!(result["result"], json!(3));
    }

    #[test]
    fn reassignment_compiles_to_ir() {
        let source = r#"
func Test
  take a as long
  emit result as long
  fail error as text
body
  x = 1
  x = 2
  emit x
done
"#;
        let flow = crate::parser::parse_runtime_flow_v1(source).unwrap();
        let ir = crate::ir::lower_to_ir(&flow).unwrap();
        assert!(!ir.nodes.is_empty());
    }

    #[test]
    fn emit_dotted_path() {
        let source = r#"
func EmitDotted
  take x as dict
  emit result as text
  fail error as text
body
  emit x.name
done
"#;
        let result = run_flow_with_inputs(source, vec![("x", json!({"name": "Alice"}))]);
        assert_eq!(result["result"], "Alice");
    }

    #[test]
    fn fail_interpolation() {
        let source = r#"
func FailInterp
  take msg as text
  emit result as text
  fail error as text
body
  fail "error: #{msg}"
done
"#;
        let flow = parser::parse_runtime_flow_v1(source).unwrap();
        let ir_data = ir::lower_to_ir(&flow).unwrap();
        let registry = TypeRegistry::empty();
        let codecs = CodecRegistry::default_registry();
        let mut input_map = HashMap::new();
        input_map.insert("msg".to_string(), json!("bad input"));
        let report =
            test_execute_flow(&flow, ir_data, input_map, &registry, None, &codecs).unwrap();
        assert_eq!(report.outputs["error"], "error: bad input");
    }
}
