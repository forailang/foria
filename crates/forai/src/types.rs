// Re-export all types from forai-core
pub use forai_core::types::*;

#[cfg(test)]
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
        // These are tested via TypeRegistry::from_module with primitive type names
        let registry = TypeRegistry::empty();
        assert!(registry.type_exists("db_conn"));
        assert!(registry.type_exists("http_server"));
        assert!(registry.type_exists("http_conn"));
        assert!(registry.type_exists("ws_conn"));
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
