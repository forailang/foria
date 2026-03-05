use crate::codec::CodecRegistry;
use base64::Engine;
use hmac::{Hmac, Mac};
use rand::RngExt;
use regex::Regex;
use serde_json::{Value, json};
use sha2::{Digest, Sha256, Sha512};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub fn read_string_arg(args: &[Value], index: usize, op: &str) -> Result<String, String> {
    let Some(value) = args.get(index) else {
        return Err(format!("Op `{op}` missing arg{index}"));
    };
    let Some(text) = value.as_str() else {
        return Err(format!("Op `{op}` expected string at arg{index}"));
    };
    Ok(text.to_string())
}

pub fn read_i64_arg(args: &[Value], index: usize, op: &str) -> Result<i64, String> {
    let Some(value) = args.get(index) else {
        return Err(format!("Op `{op}` missing arg{index}"));
    };
    value
        .as_i64()
        .or_else(|| value.as_f64().and_then(|f| { let r = f as i64; if r as f64 == f { Some(r) } else { None } }))
        .ok_or_else(|| format!("Op `{op}` expected integer at arg{index}"))
}

pub fn read_object_arg(
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

pub fn read_array_arg(args: &[Value], index: usize, op: &str) -> Result<Vec<Value>, String> {
    let Some(value) = args.get(index) else {
        return Err(format!("Op `{op}` missing arg{index}"));
    };
    let Some(arr) = value.as_array() else {
        return Err(format!("Op `{op}` expected array at arg{index}"));
    };
    Ok(arr.clone())
}

pub fn read_f64_arg(args: &[Value], index: usize, op: &str) -> Result<f64, String> {
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
        out.push(bytes[i] as char);
        i += 1;
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

/// Execute a pure (non-I/O) op synchronously.
pub fn execute_pure_op(op: &str, args: &[Value], codecs: &CodecRegistry) -> Result<Value, String> {
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
            let val = rand::rng().random_range(min..=max);
            Ok(json!(val))
        }
        "random.float" => {
            let val: f64 = rand::rng().random();
            Ok(json!(val))
        }
        "random.uuid" => Ok(json!(Uuid::new_v4().to_string())),
        "random.choice" => {
            let arr = read_array_arg(args, 0, op)?;
            if arr.is_empty() {
                return Err(format!("Op `{op}` cannot choose from empty list"));
            }
            let idx = rand::rng().random_range(0..arr.len());
            Ok(arr[idx].clone())
        }
        "random.shuffle" => {
            let arr = read_array_arg(args, 0, op)?;
            let mut shuffled = arr.clone();
            let mut rng = rand::rng();
            for i in (1..shuffled.len()).rev() {
                let j = rng.random_range(0..=i);
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
            let hash =
                bcrypt::hash(password, 12).map_err(|e| format!("Op `{op}` bcrypt error: {e}"))?;
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
            rand::rng().fill(&mut buf[..]);
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
