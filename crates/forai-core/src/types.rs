use crate::ast::{ConstraintValue, ModuleAst, TopDecl, TypeKind};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PrimitiveType {
    Text,
    Bool,
    Long,
    Real,
    Uuid,
    Time,
    List,
    Dict,
    Void,
    DbConn,
    HttpServer,
    HttpConn,
    WsConn,
    Ptr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedConstraint {
    pub key: String,
    pub bool_val: Option<bool>,
    pub number_val: Option<f64>,
    pub regex_val: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedField {
    pub name: String,
    pub type_ref: String,
    pub constraints: Vec<ResolvedConstraint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TypeDef {
    Primitive(PrimitiveType),
    Scalar {
        base: PrimitiveType,
        constraints: Vec<ResolvedConstraint>,
    },
    Struct {
        open: bool,
        fields: Vec<ResolvedField>,
    },
    Enum {
        open: bool,
        variants: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationError {
    pub path: String,
    pub constraint: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeRegistry {
    types: HashMap<String, TypeDef>,
}

fn parse_primitive(name: &str) -> Option<PrimitiveType> {
    match name {
        "text" => Some(PrimitiveType::Text),
        "bool" => Some(PrimitiveType::Bool),
        "long" => Some(PrimitiveType::Long),
        "real" => Some(PrimitiveType::Real),
        "uuid" => Some(PrimitiveType::Uuid),
        "time" => Some(PrimitiveType::Time),
        "list" => Some(PrimitiveType::List),
        "dict" => Some(PrimitiveType::Dict),
        "void" => Some(PrimitiveType::Void),
        "db_conn" => Some(PrimitiveType::DbConn),
        "http_server" => Some(PrimitiveType::HttpServer),
        "http_conn" => Some(PrimitiveType::HttpConn),
        "ws_conn" => Some(PrimitiveType::WsConn),
        "ptr" => Some(PrimitiveType::Ptr),
        _ => None,
    }
}

fn resolve_constraint(c: &crate::ast::TypeConstraint) -> ResolvedConstraint {
    let (bool_val, number_val, regex_val) = match &c.value {
        ConstraintValue::Bool(b) => (Some(*b), None, None),
        ConstraintValue::Number(n) => (None, Some(*n), None),
        ConstraintValue::Regex(r) => (None, None, Some(r.clone())),
        ConstraintValue::Symbol(_) => (None, None, None),
    };
    ResolvedConstraint {
        key: c.key.clone(),
        bool_val,
        number_val,
        regex_val,
    }
}

fn builtin_types() -> HashMap<String, TypeDef> {
    let mut types = HashMap::new();
    types.insert("text".to_string(), TypeDef::Primitive(PrimitiveType::Text));
    types.insert("bool".to_string(), TypeDef::Primitive(PrimitiveType::Bool));
    types.insert("long".to_string(), TypeDef::Primitive(PrimitiveType::Long));
    types.insert("real".to_string(), TypeDef::Primitive(PrimitiveType::Real));
    types.insert("uuid".to_string(), TypeDef::Primitive(PrimitiveType::Uuid));
    types.insert("time".to_string(), TypeDef::Primitive(PrimitiveType::Time));
    types.insert("list".to_string(), TypeDef::Primitive(PrimitiveType::List));
    types.insert("dict".to_string(), TypeDef::Primitive(PrimitiveType::Dict));
    types.insert("void".to_string(), TypeDef::Primitive(PrimitiveType::Void));
    types.insert(
        "db_conn".to_string(),
        TypeDef::Primitive(PrimitiveType::DbConn),
    );
    types.insert(
        "http_server".to_string(),
        TypeDef::Primitive(PrimitiveType::HttpServer),
    );
    types.insert(
        "http_conn".to_string(),
        TypeDef::Primitive(PrimitiveType::HttpConn),
    );
    types.insert(
        "ws_conn".to_string(),
        TypeDef::Primitive(PrimitiveType::WsConn),
    );
    types.insert("ptr".to_string(), TypeDef::Primitive(PrimitiveType::Ptr));

    // Helper closures for building struct fields
    let req = || {
        vec![ResolvedConstraint {
            key: "required".to_string(),
            bool_val: Some(true),
            number_val: None,
            regex_val: None,
        }]
    };
    let field = |name: &str, ty: &str, required: bool| ResolvedField {
        name: name.to_string(),
        type_ref: ty.to_string(),
        constraints: if required { req() } else { vec![] },
    };

    // --- Tier 1: Core types ---

    // HttpRequest — returned by http.server.accept
    types.insert(
        "HttpRequest".to_string(),
        TypeDef::Struct {
            open: true,
            fields: vec![
                field("method", "text", true),
                field("path", "text", true),
                field("query", "text", false),
                field("headers", "dict", false),
                field("body", "text", false),
                field("conn_id", "text", true),
            ],
        },
    );

    // Date — returned by date.now, date.from_iso, date.from_parts, etc.
    types.insert(
        "Date".to_string(),
        TypeDef::Struct {
            open: true,
            fields: vec![
                field("unix_ms", "long", true),
                field("tz_offset_min", "long", true),
            ],
        },
    );

    // Stamp — returned by stamp.now, stamp.from_ns, etc.
    types.insert(
        "Stamp".to_string(),
        TypeDef::Struct {
            open: true,
            fields: vec![field("ns", "long", true)],
        },
    );

    // TimeRange — returned by trange.new
    types.insert(
        "TimeRange".to_string(),
        TypeDef::Struct {
            open: true,
            fields: vec![field("start", "Date", true), field("end", "Date", true)],
        },
    );

    // HttpResponse — returned by http.get, http.post, http.put, http.delete
    types.insert(
        "HttpResponse".to_string(),
        TypeDef::Struct {
            open: true,
            fields: vec![
                field("status", "long", true),
                field("headers", "dict", true),
                field("body", "text", true),
            ],
        },
    );

    // --- Tier 2: I/O & utility types ---

    // ProcessOutput — returned by exec.run
    types.insert(
        "ProcessOutput".to_string(),
        TypeDef::Struct {
            open: true,
            fields: vec![
                field("code", "long", true),
                field("stdout", "text", true),
                field("stderr", "text", true),
                field("ok", "bool", true),
            ],
        },
    );

    // WebSocketMessage — returned by ws.recv
    types.insert(
        "WebSocketMessage".to_string(),
        TypeDef::Struct {
            open: true,
            fields: vec![field("type", "text", true), field("data", "text", true)],
        },
    );

    // ErrorObject — returned by error.new
    types.insert(
        "ErrorObject".to_string(),
        TypeDef::Struct {
            open: true,
            fields: vec![
                field("code", "text", true),
                field("message", "text", true),
                field("details", "dict", false),
            ],
        },
    );

    // URLParts — returned by url.parse
    types.insert(
        "URLParts".to_string(),
        TypeDef::Struct {
            open: true,
            fields: vec![
                field("path", "text", true),
                field("query", "text", true),
                field("fragment", "text", true),
            ],
        },
    );

    // UiNode — the universal UI tree node
    types.insert(
        "UiNode".to_string(),
        TypeDef::Struct {
            open: true,
            fields: vec![
                field("type", "text", true),
                field("props", "dict", true),
                field("children", "list", false),
                field("events", "dict", false),
            ],
        },
    );

    types
}

/// Returns true if the given type name is a built-in type (primitive or stdlib struct).
pub fn is_builtin_type(name: &str) -> bool {
    matches!(
        name,
        "text"
            | "bool"
            | "long"
            | "real"
            | "uuid"
            | "time"
            | "list"
            | "dict"
            | "void"
            | "db_conn"
            | "http_server"
            | "http_conn"
            | "ws_conn"
            | "ptr"
            | "HttpRequest"
            | "HttpResponse"
            | "Date"
            | "Stamp"
            | "TimeRange"
            | "ProcessOutput"
            | "WebSocketMessage"
            | "ErrorObject"
            | "URLParts"
            | "UiNode"
    )
}

impl TypeRegistry {
    pub fn from_module(module: &ModuleAst) -> Result<Self, Vec<String>> {
        let mut types = builtin_types();
        let mut errors = Vec::new();

        for decl in &module.decls {
            match decl {
                TopDecl::Type(td) => {
                    if types.contains_key(&td.name) {
                        errors.push(format!(
                            "{}:{} duplicate type name `{}`",
                            td.span.line, td.span.col, td.name
                        ));
                        continue;
                    }
                    match &td.kind {
                        TypeKind::Scalar {
                            base_type,
                            constraints,
                        } => {
                            let Some(base) = parse_primitive(base_type) else {
                                errors.push(format!(
                                    "{}:{} unknown base type `{}` for scalar type `{}`",
                                    td.span.line, td.span.col, base_type, td.name
                                ));
                                continue;
                            };
                            let resolved: Vec<_> =
                                constraints.iter().map(resolve_constraint).collect();
                            types.insert(
                                td.name.clone(),
                                TypeDef::Scalar {
                                    base,
                                    constraints: resolved,
                                },
                            );
                        }
                        TypeKind::Struct { fields } => {
                            let resolved: Vec<_> = fields
                                .iter()
                                .map(|f| ResolvedField {
                                    name: f.name.clone(),
                                    type_ref: f.type_ref.clone(),
                                    constraints: f
                                        .constraints
                                        .iter()
                                        .map(resolve_constraint)
                                        .collect(),
                                })
                                .collect();
                            types.insert(
                                td.name.clone(),
                                TypeDef::Struct {
                                    open: td.open,
                                    fields: resolved,
                                },
                            );
                        }
                    }
                }
                TopDecl::Enum(ed) => {
                    if types.contains_key(&ed.name) {
                        errors.push(format!(
                            "{}:{} duplicate type name `{}`",
                            ed.span.line, ed.span.col, ed.name
                        ));
                        continue;
                    }
                    types.insert(
                        ed.name.clone(),
                        TypeDef::Enum {
                            open: ed.open,
                            variants: ed.variants.clone(),
                        },
                    );
                }
                _ => {}
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        let registry = TypeRegistry { types };

        // Validate that all type references in struct fields resolve
        for decl in &module.decls {
            if let TopDecl::Type(td) = decl {
                if let TypeKind::Struct { fields } = &td.kind {
                    for f in fields {
                        if !registry.type_exists(&f.type_ref) {
                            errors.push(format!(
                                "{}:{} field `{}` in type `{}` references unknown type `{}`",
                                f.span.line, f.span.col, f.name, td.name, f.type_ref
                            ));
                        }
                    }
                }
            }
        }

        // Validate that all type references in func/flow take/emit/fail resolve
        for decl in &module.decls {
            let (takes, emits, fails) = match decl {
                TopDecl::Func(fd) | TopDecl::Sink(fd) | TopDecl::Source(fd) => {
                    (&fd.takes, &fd.emits, &fd.fails)
                }
                TopDecl::Flow(fd) => (&fd.takes, &fd.emits, &fd.fails),
                _ => continue,
            };
            for take in takes {
                if !registry.type_exists(&take.type_name) {
                    errors.push(format!(
                        "{}:{} take `{}` references unknown type `{}`",
                        take.span.line, take.span.col, take.name, take.type_name
                    ));
                }
            }
            for emit in emits {
                if !registry.type_exists(&emit.type_name) {
                    errors.push(format!(
                        "{}:{} emit `{}` references unknown type `{}`",
                        emit.span.line, emit.span.col, emit.name, emit.type_name
                    ));
                }
            }
            for fail in fails {
                if !registry.type_exists(&fail.type_name) {
                    errors.push(format!(
                        "{}:{} fail `{}` references unknown type `{}`",
                        fail.span.line, fail.span.col, fail.name, fail.type_name
                    ));
                }
            }
        }

        if errors.is_empty() {
            Ok(registry)
        } else {
            Err(errors)
        }
    }

    pub fn empty() -> Self {
        TypeRegistry {
            types: builtin_types(),
        }
    }

    pub fn type_exists(&self, name: &str) -> bool {
        self.types.contains_key(name)
    }

    pub fn get(&self, name: &str) -> Option<&TypeDef> {
        self.types.get(name)
    }

    pub fn validate(&self, value: &Value, type_name: &str, path: &str) -> Vec<ValidationError> {
        let Some(typedef) = self.types.get(type_name) else {
            return vec![ValidationError {
                path: path.to_string(),
                constraint: "type".to_string(),
                message: format!("unknown type `{type_name}`"),
            }];
        };
        self.validate_against(value, typedef, path)
    }

    fn validate_against(
        &self,
        value: &Value,
        typedef: &TypeDef,
        path: &str,
    ) -> Vec<ValidationError> {
        match typedef {
            TypeDef::Primitive(prim) => self.validate_primitive(value, prim, path),
            TypeDef::Scalar { base, constraints } => {
                let mut errs = self.validate_primitive(value, base, path);
                if errs.is_empty() {
                    errs.extend(self.validate_constraints(value, constraints, path));
                }
                errs
            }
            TypeDef::Struct { open, fields } => self.validate_struct(value, *open, fields, path),
            TypeDef::Enum { open, variants } => self.validate_enum(value, *open, variants, path),
        }
    }

    fn validate_primitive(
        &self,
        value: &Value,
        prim: &PrimitiveType,
        path: &str,
    ) -> Vec<ValidationError> {
        let ok = match prim {
            PrimitiveType::Text => value.is_string(),
            PrimitiveType::Bool => value.is_boolean(),
            PrimitiveType::Long => value.is_i64(),
            PrimitiveType::Real => value.is_f64() || value.is_i64(),
            PrimitiveType::Uuid => value.is_string(),
            PrimitiveType::Time => value.is_string(),
            PrimitiveType::List => value.is_array(),
            PrimitiveType::Dict => value.is_object(),
            PrimitiveType::Void => value.is_null(),
            PrimitiveType::DbConn
            | PrimitiveType::HttpServer
            | PrimitiveType::HttpConn
            | PrimitiveType::WsConn
            | PrimitiveType::Ptr => value.is_string(),
        };
        if ok {
            vec![]
        } else {
            let expected = match prim {
                PrimitiveType::Text => "text",
                PrimitiveType::Bool => "bool",
                PrimitiveType::Long => "long",
                PrimitiveType::Real => "real",
                PrimitiveType::Uuid => "uuid",
                PrimitiveType::Time => "time",
                PrimitiveType::List => "list",
                PrimitiveType::Dict => "dict",
                PrimitiveType::Void => "void",
                PrimitiveType::DbConn => "db_conn",
                PrimitiveType::HttpServer => "http_server",
                PrimitiveType::HttpConn => "http_conn",
                PrimitiveType::WsConn => "ws_conn",
                PrimitiveType::Ptr => "ptr",
            };
            vec![ValidationError {
                path: path.to_string(),
                constraint: "type".to_string(),
                message: format!("expected {expected}, got {}", value_type_label(value)),
            }]
        }
    }

    fn validate_constraints(
        &self,
        value: &Value,
        constraints: &[ResolvedConstraint],
        path: &str,
    ) -> Vec<ValidationError> {
        let mut errs = Vec::new();
        for c in constraints {
            match c.key.as_str() {
                "matches" => {
                    if let Some(pattern) = &c.regex_val {
                        if let Some(s) = value.as_str() {
                            match Regex::new(pattern) {
                                Ok(re) => {
                                    if !re.is_match(s) {
                                        errs.push(ValidationError {
                                            path: path.to_string(),
                                            constraint: "matches".to_string(),
                                            message: format!(
                                                "value \"{s}\" does not match pattern /{pattern}/"
                                            ),
                                        });
                                    }
                                }
                                Err(e) => {
                                    errs.push(ValidationError {
                                        path: path.to_string(),
                                        constraint: "matches".to_string(),
                                        message: format!("invalid regex pattern: {e}"),
                                    });
                                }
                            }
                        }
                    }
                }
                "min" => {
                    if let Some(min) = c.number_val {
                        if let Some(n) = value.as_f64() {
                            if n < min {
                                errs.push(ValidationError {
                                    path: path.to_string(),
                                    constraint: "min".to_string(),
                                    message: format!("value {n} is less than minimum {min}"),
                                });
                            }
                        } else if let Some(s) = value.as_str() {
                            if (s.len() as f64) < min {
                                errs.push(ValidationError {
                                    path: path.to_string(),
                                    constraint: "min".to_string(),
                                    message: format!(
                                        "length {} is less than minimum {min}",
                                        s.len()
                                    ),
                                });
                            }
                        }
                    }
                }
                "max" => {
                    if let Some(max) = c.number_val {
                        if let Some(n) = value.as_f64() {
                            if n > max {
                                errs.push(ValidationError {
                                    path: path.to_string(),
                                    constraint: "max".to_string(),
                                    message: format!("value {n} exceeds maximum {max}"),
                                });
                            }
                        } else if let Some(s) = value.as_str() {
                            if (s.len() as f64) > max {
                                errs.push(ValidationError {
                                    path: path.to_string(),
                                    constraint: "max".to_string(),
                                    message: format!("length {} exceeds maximum {max}", s.len()),
                                });
                            }
                        }
                    }
                }
                "required" | "map" => {}
                _ => {}
            }
        }
        errs
    }

    fn validate_struct(
        &self,
        value: &Value,
        open: bool,
        fields: &[ResolvedField],
        path: &str,
    ) -> Vec<ValidationError> {
        let Some(obj) = value.as_object() else {
            return vec![ValidationError {
                path: path.to_string(),
                constraint: "type".to_string(),
                message: format!("expected object, got {}", value_type_label(value)),
            }];
        };

        let mut errs = Vec::new();
        for field in fields {
            let field_path = if path.is_empty() {
                field.name.clone()
            } else {
                format!("{}.{}", path, field.name)
            };

            let is_required = field
                .constraints
                .iter()
                .any(|c| c.key == "required" && c.bool_val == Some(true));

            match obj.get(&field.name) {
                None => {
                    if is_required {
                        errs.push(ValidationError {
                            path: field_path,
                            constraint: "required".to_string(),
                            message: "required field is missing".to_string(),
                        });
                    }
                }
                Some(field_value) => {
                    errs.extend(self.validate(field_value, &field.type_ref, &field_path));
                    errs.extend(self.validate_constraints(
                        field_value,
                        &field.constraints,
                        &field_path,
                    ));
                }
            }
        }

        // Closed types reject extra fields
        if !open {
            let declared: std::collections::HashSet<&str> =
                fields.iter().map(|f| f.name.as_str()).collect();
            for key in obj.keys() {
                if !declared.contains(key.as_str()) {
                    let field_path = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{}.{}", path, key)
                    };
                    errs.push(ValidationError {
                        path: field_path,
                        constraint: "closed".to_string(),
                        message: format!("unexpected field '{}' in closed type", key),
                    });
                }
            }
        }

        errs
    }

    fn validate_enum(
        &self,
        value: &Value,
        open: bool,
        variants: &[String],
        path: &str,
    ) -> Vec<ValidationError> {
        let Some(s) = value.as_str() else {
            return vec![ValidationError {
                path: path.to_string(),
                constraint: "type".to_string(),
                message: format!("expected string for enum, got {}", value_type_label(value)),
            }];
        };
        if !open && !variants.iter().any(|v| v == s) {
            vec![ValidationError {
                path: path.to_string(),
                constraint: "enum".to_string(),
                message: format!(
                    "value \"{s}\" is not a valid variant; expected one of: {}",
                    variants.join(", ")
                ),
            }]
        } else {
            vec![]
        }
    }
}

fn value_type_label(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

// Tests live in crates/forai where the parser is available.
#[cfg(any())]
mod tests {
    use super::*;
    use crate::parser::parse_module_v1;
    use serde_json::json;

    #[test]
    fn validates_required_field() {
        let src = r#"
type LoginRequest
  email text :required => true
  password text :required => true
done
"#;
        let module = parse_module_v1(src).unwrap();
        let registry = TypeRegistry::from_module(&module).unwrap();

        let value = json!({ "email": "test@example.com" });
        let errors = registry.validate(&value, "LoginRequest", "request");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].path, "request.password");
        assert_eq!(errors[0].constraint, "required");
    }

    #[test]
    fn validates_min_constraint() {
        let src = r#"
type Password as text :min => 8
"#;
        let module = parse_module_v1(src).unwrap();
        let registry = TypeRegistry::from_module(&module).unwrap();

        let errors = registry.validate(&json!("short"), "Password", "password");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].constraint, "min");
        assert!(errors[0].message.contains("less than minimum"));

        let errors = registry.validate(&json!("long-enough-password"), "Password", "password");
        assert!(errors.is_empty());
    }

    #[test]
    fn validates_regex_matches() {
        let src = r#"
type Email as text :matches => /@/
"#;
        let module = parse_module_v1(src).unwrap();
        let registry = TypeRegistry::from_module(&module).unwrap();

        let errors = registry.validate(&json!("not-an-email"), "Email", "email");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].constraint, "matches");

        let errors = registry.validate(&json!("test@example.com"), "Email", "email");
        assert!(errors.is_empty());
    }

    #[test]
    fn unknown_type_errors() {
        let registry = TypeRegistry::empty();
        let errors = registry.validate(&json!("anything"), "NoSuchType", "request");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("unknown type"));
    }

    #[test]
    fn validates_nested_struct() {
        let src = r#"
type Inner
  value long :required => true
done

type Outer
  inner Inner :required => true
done
"#;
        let module = parse_module_v1(src).unwrap();
        let registry = TypeRegistry::from_module(&module).unwrap();

        let value = json!({ "inner": {} });
        let errors = registry.validate(&value, "Outer", "root");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].path, "root.inner.value");
        assert_eq!(errors[0].constraint, "required");
    }

    #[test]
    fn validates_enum_value() {
        let src = r#"
enum Status
  active
  inactive
  banned
done
"#;
        let module = parse_module_v1(src).unwrap();
        let registry = TypeRegistry::from_module(&module).unwrap();

        let errors = registry.validate(&json!("active"), "Status", "status");
        assert!(errors.is_empty());

        let errors = registry.validate(&json!("unknown"), "Status", "status");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].constraint, "enum");
    }

    #[test]
    fn parse_primitive_handles() {
        assert_eq!(parse_primitive("db_conn"), Some(PrimitiveType::DbConn));
        assert_eq!(
            parse_primitive("http_server"),
            Some(PrimitiveType::HttpServer)
        );
        assert_eq!(parse_primitive("http_conn"), Some(PrimitiveType::HttpConn));
        assert_eq!(parse_primitive("ws_conn"), Some(PrimitiveType::WsConn));
    }

    #[test]
    fn validates_primitive_type_mismatch() {
        let registry = TypeRegistry::empty();
        let errors = registry.validate(&json!("not-a-number"), "long", "field");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].constraint, "type");
        assert!(errors[0].message.contains("expected long"));
    }
}
