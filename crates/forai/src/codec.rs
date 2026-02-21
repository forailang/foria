use serde_json::Value;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Codec trait — decode/encode between text and serde_json::Value
// ---------------------------------------------------------------------------

pub trait Codec: Send + Sync {
    fn name(&self) -> &str;
    fn decode(&self, text: &str) -> Result<Value, String>;
    fn encode(&self, value: &Value) -> Result<String, String>;
    fn encode_pretty(&self, value: &Value) -> Result<String, String>;
}

// ---------------------------------------------------------------------------
// CodecRegistry — maps format names to Codec implementations
// ---------------------------------------------------------------------------

pub struct CodecRegistry {
    codecs: HashMap<String, Box<dyn Codec>>,
}

impl CodecRegistry {
    pub fn new() -> Self {
        Self {
            codecs: HashMap::new(),
        }
    }

    pub fn default_registry() -> Self {
        let mut r = Self::new();
        r.register(Box::new(JsonCodec));
        r
    }

    pub fn register(&mut self, codec: Box<dyn Codec>) {
        self.codecs.insert(codec.name().to_string(), codec);
    }

    pub fn get(&self, name: &str) -> Option<&dyn Codec> {
        self.codecs.get(name).map(|b| b.as_ref())
    }

    #[cfg(test)]
    pub fn names(&self) -> Vec<&str> {
        self.codecs.keys().map(|s| s.as_str()).collect()
    }

    /// Generate op names for compile-time validation.
    /// Returns `["{name}.decode", "{name}.encode", "{name}.encode_pretty"]` per codec,
    /// plus the three generic `codec.*` ops.
    pub fn known_ops(&self) -> Vec<String> {
        let mut ops = vec![
            "codec.decode".to_string(),
            "codec.encode".to_string(),
            "codec.encode_pretty".to_string(),
        ];
        for name in self.codecs.keys() {
            ops.push(format!("{name}.decode"));
            ops.push(format!("{name}.encode"));
            ops.push(format!("{name}.encode_pretty"));
        }
        ops
    }
}

// ---------------------------------------------------------------------------
// JsonCodec — built-in JSON codec wrapping serde_json
// ---------------------------------------------------------------------------

struct JsonCodec;

impl Codec for JsonCodec {
    fn name(&self) -> &str {
        "json"
    }

    fn decode(&self, text: &str) -> Result<Value, String> {
        serde_json::from_str(text).map_err(|e| format!("json.decode failed: {e}"))
    }

    fn encode(&self, value: &Value) -> Result<String, String> {
        serde_json::to_string(value).map_err(|e| format!("json.encode failed: {e}"))
    }

    fn encode_pretty(&self, value: &Value) -> Result<String, String> {
        serde_json::to_string_pretty(value).map_err(|e| format!("json.encode_pretty failed: {e}"))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_decode() {
        let codec = JsonCodec;
        let val = codec.decode(r#"{"a":1,"b":"hello"}"#).unwrap();
        assert_eq!(val, json!({"a": 1, "b": "hello"}));
    }

    #[test]
    fn json_decode_error() {
        let codec = JsonCodec;
        let err = codec.decode("not json").unwrap_err();
        assert!(err.contains("json.decode failed"));
    }

    #[test]
    fn json_encode() {
        let codec = JsonCodec;
        let text = codec.encode(&json!({"x": 42})).unwrap();
        let roundtrip: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(roundtrip, json!({"x": 42}));
    }

    #[test]
    fn json_encode_pretty() {
        let codec = JsonCodec;
        let text = codec.encode_pretty(&json!({"x": 42})).unwrap();
        assert!(text.contains('\n'));
        let roundtrip: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(roundtrip, json!({"x": 42}));
    }

    #[test]
    fn registry_default_has_json() {
        let reg = CodecRegistry::default_registry();
        assert!(reg.get("json").is_some());
        assert!(reg.get("toml").is_none());
    }

    #[test]
    fn registry_known_ops() {
        let reg = CodecRegistry::default_registry();
        let ops = reg.known_ops();
        assert!(ops.contains(&"codec.decode".to_string()));
        assert!(ops.contains(&"codec.encode".to_string()));
        assert!(ops.contains(&"codec.encode_pretty".to_string()));
        assert!(ops.contains(&"json.decode".to_string()));
        assert!(ops.contains(&"json.encode".to_string()));
        assert!(ops.contains(&"json.encode_pretty".to_string()));
    }

    #[test]
    fn registry_names() {
        let reg = CodecRegistry::default_registry();
        let names = reg.names();
        assert!(names.contains(&"json"));
    }
}
