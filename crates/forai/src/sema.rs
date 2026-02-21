use crate::ast::{DocsDecl, ModuleAst, TopDecl, TypeKind};
use std::collections::{HashMap, HashSet};

pub fn validate_module(module: &ModuleAst, filename: Option<&str>) -> Result<(), Vec<String>> {
    let mut symbol_names = HashSet::new();
    let mut docs_targets = HashMap::<String, usize>::new();
    let mut errors = Vec::new();

    // Collect flow and func names for single-decl and name-match checks
    let flow_names: Vec<String> = module
        .decls
        .iter()
        .filter_map(|d| match d {
            TopDecl::Flow(f) => Some(f.name.clone()),
            _ => None,
        })
        .collect();

    let func_names: Vec<String> = module
        .decls
        .iter()
        .filter_map(|d| match d {
            TopDecl::Func(f) | TopDecl::Sink(f) | TopDecl::Source(f) => Some(f.name.clone()),
            _ => None,
        })
        .collect();

    let test_names: HashSet<String> = module
        .decls
        .iter()
        .filter_map(|d| match d {
            TopDecl::Test(t) => Some(t.name.clone()),
            _ => None,
        })
        .collect();

    let callable_count = flow_names.len() + func_names.len();
    if callable_count > 1 {
        let all_names: Vec<_> = flow_names.iter().chain(func_names.iter()).collect();
        errors.push(format!(
            "1:1 file contains {} flow/func declarations ({}); only one per file is allowed",
            callable_count,
            all_names
                .iter()
                .map(|n| n.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    if callable_count == 1 {
        let the_name = flow_names.first().or(func_names.first()).unwrap();
        if let Some(fname) = filename {
            let stem = std::path::Path::new(fname)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            if !stem.is_empty() && stem != the_name.as_str() {
                errors.push(format!(
                    "1:1 name '{}' does not match filename '{}'; rename to '{}' or rename the file to '{}.fa'",
                    the_name, fname, stem, the_name
                ));
            }
        }
    }

    for decl in &module.decls {
        match decl {
            TopDecl::Func(d) if d.name == "main" => {
                errors.push("1:1 `main` must be declared as a flow, not a func".to_string());
            }
            TopDecl::Sink(d) if d.name == "main" => {
                errors.push("1:1 `main` must be declared as a flow, not a sink".to_string());
            }
            TopDecl::Source(d) if d.name == "main" => {
                errors.push("1:1 `main` must be declared as a flow, not a source".to_string());
            }
            _ => {}
        }
    }

    for decl in &module.decls {
        match decl {
            TopDecl::Flow(d) => {
                symbol_names.insert(d.name.clone());
                // Flows may have zero take/emit/fail — they are pure wiring
            }
            TopDecl::Func(d) => {
                symbol_names.insert(d.name.clone());
                if d.return_type.is_some() || d.fail_type.is_some() {
                    // v2 func: must have both return_type and fail_type
                    if d.return_type.is_none() {
                        errors.push(format!(
                            "{}:{} func `{}` has `fail` type but is missing `return <Type>`",
                            d.span.line, d.span.col, d.name
                        ));
                    }
                    if d.fail_type.is_none() {
                        errors.push(format!(
                            "{}:{} func `{}` has `return` type but is missing `fail <Type>`",
                            d.span.line, d.span.col, d.name
                        ));
                    }
                    if !d.emits.is_empty() {
                        errors.push(format!(
                            "{}:{} func `{}` uses v2 `return`/`fail` syntax; cannot also have named `emit` ports",
                            d.span.line, d.span.col, d.name
                        ));
                    }
                    if !d.fails.is_empty() {
                        errors.push(format!(
                            "{}:{} func `{}` uses v2 `return`/`fail` syntax; cannot also have named `fail` ports",
                            d.span.line, d.span.col, d.name
                        ));
                    }
                } else {
                    // v1 func: named emit/fail ports required
                    if d.emits.is_empty() {
                        errors.push(format!(
                            "{}:{} func `{}` is missing `emit <name> as <Type>`",
                            d.span.line, d.span.col, d.name
                        ));
                    }
                    if d.fails.is_empty() {
                        errors.push(format!(
                            "{}:{} func `{}` is missing `fail <name> as <Type>`",
                            d.span.line, d.span.col, d.name
                        ));
                    }
                }
            }
            TopDecl::Sink(d) => {
                symbol_names.insert(d.name.clone());
                if d.emits.is_empty() {
                    errors.push(format!(
                        "{}:{} sink `{}` is missing `emit <name> as <Type>`",
                        d.span.line, d.span.col, d.name
                    ));
                }
                if d.fails.is_empty() {
                    errors.push(format!(
                        "{}:{} sink `{}` is missing `fail <name> as <Type>`",
                        d.span.line, d.span.col, d.name
                    ));
                }
            }
            TopDecl::Source(d) => {
                symbol_names.insert(d.name.clone());
                if d.return_type.is_some() || d.fail_type.is_some() {
                    if d.return_type.is_none() {
                        errors.push(format!(
                            "{}:{} source `{}` has `fail` type but is missing `return <Type>`",
                            d.span.line, d.span.col, d.name
                        ));
                    }
                    if d.fail_type.is_none() {
                        errors.push(format!(
                            "{}:{} source `{}` has `return` type but is missing `fail <Type>`",
                            d.span.line, d.span.col, d.name
                        ));
                    }
                } else {
                    if d.emits.is_empty() {
                        errors.push(format!(
                            "{}:{} source `{}` is missing `emit <name> as <Type>`",
                            d.span.line, d.span.col, d.name
                        ));
                    }
                    if d.fails.is_empty() {
                        errors.push(format!(
                            "{}:{} source `{}` is missing `fail <name> as <Type>`",
                            d.span.line, d.span.col, d.name
                        ));
                    }
                }
            }
            TopDecl::Type(d) => {
                symbol_names.insert(d.name.clone());
            }
            TopDecl::Enum(d) => {
                symbol_names.insert(d.name.clone());
            }
            TopDecl::Test(d) => {
                symbol_names.insert(d.name.clone());
            }
            TopDecl::Docs(d) => {
                *docs_targets.entry(d.name.clone()).or_insert(0) += 1;
            }
            TopDecl::Uses(_) => {}
        }
    }

    for decl in &module.decls {
        if let TopDecl::Docs(docs) = decl
            && !symbol_names.contains(&docs.name)
        {
            errors.push(format!(
                "{}:{} docs target `{}` is not defined in this module",
                docs.span.line, docs.span.col, docs.name
            ));
        }
    }

    for (name, count) in &docs_targets {
        if *count > 1 {
            errors.push(format!(
                "1:1 docs target `{}` is declared {} times; expected one docs block per symbol",
                name, count
            ));
        }
    }

    for decl in &module.decls {
        match decl {
            TopDecl::Flow(d) => {
                if !docs_targets.contains_key(&d.name) {
                    errors.push(format!(
                        "{}:{} flow `{}` is undocumented; add `docs {}`",
                        d.span.line, d.span.col, d.name, d.name
                    ));
                }
            }
            TopDecl::Func(d) => {
                if !docs_targets.contains_key(&d.name) {
                    errors.push(format!(
                        "{}:{} func `{}` is undocumented; add `docs {}`",
                        d.span.line, d.span.col, d.name, d.name
                    ));
                }
            }
            TopDecl::Sink(d) => {
                if !docs_targets.contains_key(&d.name) {
                    errors.push(format!(
                        "{}:{} sink `{}` is undocumented; add `docs {}`",
                        d.span.line, d.span.col, d.name, d.name
                    ));
                }
            }
            TopDecl::Source(d) => {
                if !docs_targets.contains_key(&d.name) {
                    errors.push(format!(
                        "{}:{} source `{}` is undocumented; add `docs {}`",
                        d.span.line, d.span.col, d.name, d.name
                    ));
                }
            }
            TopDecl::Type(d) => {
                if !docs_targets.contains_key(&d.name) {
                    errors.push(format!(
                        "{}:{} type `{}` is undocumented; add `docs {}`",
                        d.span.line, d.span.col, d.name, d.name
                    ));
                }
            }
            TopDecl::Test(d) => {
                if !docs_targets.contains_key(&d.name) {
                    errors.push(format!(
                        "{}:{} test `{}` is undocumented; add `docs {}`",
                        d.span.line, d.span.col, d.name, d.name
                    ));
                }
            }
            TopDecl::Docs(_) | TopDecl::Enum(_) | TopDecl::Uses(_) => {}
        }
    }

    // Build a map from name → DocsDecl for field-docs validation
    let docs_decls: HashMap<&str, &DocsDecl> = module
        .decls
        .iter()
        .filter_map(|d| match d {
            TopDecl::Docs(dd) => Some((dd.name.as_str(), dd)),
            _ => None,
        })
        .collect();

    // Validate field docs for struct types
    for decl in &module.decls {
        if let TopDecl::Type(d) = decl {
            if let TypeKind::Struct { fields } = &d.kind {
                if let Some(dd) = docs_decls.get(d.name.as_str()) {
                    let field_names: HashSet<&str> =
                        fields.iter().map(|f| f.name.as_str()).collect();
                    let doc_field_names: HashSet<&str> =
                        dd.field_docs.iter().map(|f| f.name.as_str()).collect();

                    for field in fields {
                        if !doc_field_names.contains(field.name.as_str()) {
                            errors.push(format!(
                                "{}:{} type `{}` field `{}` is undocumented; add `docs {}` inside `docs {}`",
                                d.span.line, d.span.col, d.name, field.name, field.name, d.name
                            ));
                        }
                    }

                    for fd in &dd.field_docs {
                        if !field_names.contains(fd.name.as_str()) {
                            errors.push(format!(
                                "{}:{} docs `{}` has field docs for `{}` but no such field exists in the type",
                                dd.span.line, dd.span.col, d.name, fd.name
                            ));
                        }
                    }
                }
            }
        }
    }

    // Check that every callable (except `main`) has a corresponding `test {Name}` block
    for decl in &module.decls {
        match decl {
            TopDecl::Flow(d) if d.name != "main" => {
                if !test_names.contains(&d.name) {
                    errors.push(format!(
                        "{}:{} flow `{}` has no test block; add `test {}`",
                        d.span.line, d.span.col, d.name, d.name
                    ));
                }
            }
            TopDecl::Func(d) => {
                if !test_names.contains(&d.name) {
                    errors.push(format!(
                        "{}:{} func `{}` has no test block; add `test {}`",
                        d.span.line, d.span.col, d.name, d.name
                    ));
                }
            }
            TopDecl::Sink(d) => {
                if !test_names.contains(&d.name) {
                    errors.push(format!(
                        "{}:{} sink `{}` has no test block; add `test {}`",
                        d.span.line, d.span.col, d.name, d.name
                    ));
                }
            }
            TopDecl::Source(d) => {
                if !test_names.contains(&d.name) {
                    errors.push(format!(
                        "{}:{} source `{}` has no test block; add `test {}`",
                        d.span.line, d.span.col, d.name, d.name
                    ));
                }
            }
            _ => {}
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::validate_module;
    use crate::parser::parse_module_v1;

    #[test]
    fn accepts_documented_func_and_test() {
        let src = r#"
docs LoginFunc
  Login docs.
done

func LoginFunc
  take request as HttpRequest
  emit response as HttpResponse
  fail error as AuthError
body
  emit request
done

test LoginFunc
  result = LoginFunc(request)
done
"#;
        let module = parse_module_v1(src).expect("parse");
        validate_module(&module, None).expect("sema");
    }

    #[test]
    fn rejects_undocumented_func() {
        let src = r#"
func LoginFunc
  take request as HttpRequest
  emit response as HttpResponse
  fail error as AuthError
body
  emit request
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let err = validate_module(&module, None).expect_err("should fail sema");
        assert!(
            err.iter()
                .any(|e| e.contains("func `LoginFunc` is undocumented"))
        );
    }

    #[test]
    fn rejects_multiple_funcs() {
        let src = r#"
docs Foo
  docs
done

func Foo
  take x as HttpRequest
  emit y as HttpResponse
  fail e as AuthError
body
  emit x
done

docs Bar
  docs
done

func Bar
  take x as HttpRequest
  emit y as HttpResponse
  fail e as AuthError
body
  emit x
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let err = validate_module(&module, None).expect_err("should reject multiple funcs");
        assert!(err.iter().any(|e| e.contains("2 flow/func declarations")));
    }

    #[test]
    fn rejects_name_mismatch() {
        let src = r#"
docs Foo
  docs
done

func Foo
  take x as HttpRequest
  emit y as HttpResponse
  fail e as AuthError
body
  emit x
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let err =
            validate_module(&module, Some("Bar.fa")).expect_err("should reject name mismatch");
        assert!(err.iter().any(|e| e.contains("does not match filename")));
    }

    #[test]
    fn accepts_matching_func_name() {
        let src = r#"
docs Foo
  docs
done

func Foo
  take x as HttpRequest
  emit y as HttpResponse
  fail e as AuthError
body
  emit x
done

test Foo
  r = Foo(x)
done
"#;
        let module = parse_module_v1(src).expect("parse");
        validate_module(&module, Some("Foo.fa")).expect("should accept matching name");
    }

    #[test]
    fn rejects_undocumented_sink() {
        let src = r#"
sink Greet
  take name as Text
  emit greeting as Text
  fail error as Error
body
  emit name
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let err = validate_module(&module, None).expect_err("should fail sema");
        assert!(
            err.iter()
                .any(|e| e.contains("sink `Greet` is undocumented"))
        );
    }

    #[test]
    fn rejects_func_named_main() {
        let src = r#"
docs main
  docs
done

func main
  emit y as HttpResponse
  fail e as AuthError
body
  emit y
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let errors = validate_module(&module, Some("main.fa")).unwrap_err();
        assert!(
            errors.iter().any(|e| e.contains("must be declared as a flow")),
            "should reject func main, got: {errors:?}"
        );
    }

    #[test]
    fn rejects_sink_named_main() {
        let src = r#"
docs main
  docs
done

sink main
  take input as Foo
  emit y as HttpResponse
  fail e as AuthError
body
  emit y
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let errors = validate_module(&module, Some("main.fa")).unwrap_err();
        assert!(
            errors.iter().any(|e| e.contains("must be declared as a flow")),
            "should reject sink main, got: {errors:?}"
        );
    }

    #[test]
    fn accepts_flow_named_main() {
        let src = r#"
docs main
  Flow docs.
done

flow main
  take input as Foo
  emit result as Bar
  fail error as Baz
body
  step passthrough.Id(:input => input) then
    next :result => output
    emit :result => output
  done
done
"#;
        let module = parse_module_v1(src).expect("parse");
        validate_module(&module, Some("main.fa")).expect("flow main should be accepted");
    }

    #[test]
    fn accepts_documented_sink() {
        let src = r#"
docs Greet
  A greeting sink.
done

sink Greet
  take name as Text
  emit greeting as Text
  fail error as Error
body
  emit name
done

test Greet
  r = Greet(name)
done
"#;
        let module = parse_module_v1(src).expect("parse");
        validate_module(&module, None).expect("sema should pass for documented sink");
    }

    #[test]
    fn rejects_undocumented_type() {
        let src = r#"
type Foo
  bar text
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let err = validate_module(&module, None).expect_err("should reject undocumented type");
        assert!(
            err.iter().any(|e| e.contains("type `Foo` is undocumented")),
            "got: {err:?}"
        );
    }

    #[test]
    fn rejects_undocumented_field() {
        let src = r#"
docs Foo
  A struct type.
done

type Foo
  bar text
  baz long
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let err = validate_module(&module, None).expect_err("should reject undocumented field");
        assert!(
            err.iter().any(|e| e.contains("field `bar` is undocumented")),
            "got: {err:?}"
        );
        assert!(
            err.iter().any(|e| e.contains("field `baz` is undocumented")),
            "got: {err:?}"
        );
    }

    #[test]
    fn rejects_orphan_field_docs() {
        let src = r#"
docs Foo
  A struct type.

  docs bar
    The bar field.
  done

  docs ghost
    Does not exist.
  done
done

type Foo
  bar text
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let err = validate_module(&module, None).expect_err("should reject orphan field docs");
        assert!(
            err.iter().any(|e| e.contains("field docs for `ghost`") && e.contains("no such field")),
            "got: {err:?}"
        );
    }

    #[test]
    fn accepts_fully_documented_struct() {
        let src = r#"
docs Foo
  A struct type.

  docs bar
    The bar field.
  done

  docs baz
    The baz field.
  done
done

type Foo
  bar text
  baz long
done
"#;
        let module = parse_module_v1(src).expect("parse");
        validate_module(&module, None).expect("sema should pass for fully documented struct");
    }

    #[test]
    fn accepts_documented_scalar() {
        let src = r#"
docs Email
  An email address.
done

type Email as text :matches => /@/
"#;
        let module = parse_module_v1(src).expect("parse");
        validate_module(&module, None).expect("sema should pass for documented scalar");
    }

    #[test]
    fn accepts_v2_func_with_return_and_fail() {
        let src = r#"
docs Compute
  A v2 func.
done

func Compute
  take x as long
  return dict
  fail text
body
  r = obj.new()
  return r
done

test Compute
  r = Compute(42)
done
"#;
        let module = parse_module_v1(src).expect("parse");
        validate_module(&module, None).expect("v2 func should pass sema");
    }

    #[test]
    fn rejects_v2_func_missing_fail_type() {
        let src = r#"
docs Compute
  docs
done

func Compute
  take x as long
  return dict
body
  r = obj.new()
  return r
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let err = validate_module(&module, None).expect_err("should reject missing fail type");
        assert!(
            err.iter().any(|e| e.contains("missing `fail <Type>`")),
            "got: {err:?}"
        );
    }

    #[test]
    fn rejects_v2_func_with_named_emit_ports() {
        let src = r#"
docs Mixed
  docs
done

func Mixed
  take x as long
  return dict
  fail text
  emit extra as dict
body
  r = obj.new()
  return r
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let err = validate_module(&module, None).expect_err("should reject mixed v2+v1");
        assert!(
            err.iter().any(|e| e.contains("cannot also have named `emit` ports")),
            "got: {err:?}"
        );
    }

    #[test]
    fn rejects_source_named_main() {
        let src = r#"
docs main
  docs
done

source main
  take port as long
  emit req as dict
  fail error as text
body
  emit req
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let errors = validate_module(&module, Some("main.fa")).unwrap_err();
        assert!(
            errors.iter().any(|e| e.contains("must be declared as a flow")),
            "should reject source main, got: {errors:?}"
        );
    }

    #[test]
    fn rejects_undocumented_source() {
        let src = r#"
source HTTPRequests
  take port as long
  emit req as dict
  fail error as text
body
  emit req
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let err = validate_module(&module, None).expect_err("should fail sema");
        assert!(
            err.iter().any(|e| e.contains("source `HTTPRequests` is undocumented")),
            "got: {err:?}"
        );
    }

    #[test]
    fn accepts_documented_source() {
        let src = r#"
docs HTTPRequests
  An HTTP source.
done

source HTTPRequests
  take port as long
  emit req as dict
  fail error as text
body
  emit req
done

test HTTPRequests
  r = HTTPRequests(8080)
done
"#;
        let module = parse_module_v1(src).expect("parse");
        validate_module(&module, None).expect("sema should pass for documented source");
    }

    #[test]
    fn rejects_func_missing_test() {
        let src = r#"
docs Greet
  A greeting func.
done

func Greet
  take name as text
  emit result as text
  fail error as text
body
  emit name
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let err = validate_module(&module, None).expect_err("should reject missing test");
        assert!(
            err.iter().any(|e| e.contains("func `Greet` has no test block")),
            "got: {err:?}"
        );
    }

    #[test]
    fn rejects_flow_missing_test() {
        let src = r#"
docs Pipeline
  A pipeline flow.
done

flow Pipeline
  take input as text
  emit result as text
  fail error as text
body
  step passthrough.Id(:input => x) then
    next :result => y
    emit :result => y
  done
done
"#;
        let module = parse_module_v1(src).expect("parse");
        let err = validate_module(&module, None).expect_err("should reject missing test");
        assert!(
            err.iter().any(|e| e.contains("flow `Pipeline` has no test block")),
            "got: {err:?}"
        );
    }

    #[test]
    fn accepts_flow_main_without_test() {
        let src = r#"
docs main
  Entry point.
done

flow main
  take input as text
  emit result as text
  fail error as text
body
  step passthrough.Id(:input => x) then
    next :result => y
    emit :result => y
  done
done
"#;
        let module = parse_module_v1(src).expect("parse");
        validate_module(&module, Some("main.fa")).expect("flow main should not require a test");
    }

}


