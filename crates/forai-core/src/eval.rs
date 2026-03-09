use crate::ast::{BinOp, Pattern, UnaryOp};
use serde_json::{Value, json};
use std::cmp::Ordering;

pub fn eval_binop(op: BinOp, l: &Value, r: &Value) -> Result<Value, String> {
    match op {
        BinOp::Add => {
            // Numbers: add (preserving integer when both inputs are integer)
            if let (Some(a), Some(b)) = (l.as_f64(), r.as_f64()) {
                if l.is_i64() && r.is_i64() {
                    return Ok(json!(l.as_i64().unwrap() + r.as_i64().unwrap()));
                }
                return Ok(json!(a + b));
            }
            // Strings: concatenate (both must be strings — no implicit coercion)
            if let (Some(a), Some(b)) = (l.as_str(), r.as_str()) {
                return Ok(json!(format!("{a}{b}")));
            }
            Err(format!("Cannot add {l} and {r}"))
        }

        BinOp::Sub => num_binop(l, r, |a, b| a - b, "subtract"),
        BinOp::Mul => num_binop(l, r, |a, b| a * b, "multiply"),

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
            // Preserve integer when both inputs are integer and result is exact
            if l.is_i64() && r.is_i64() && l.as_i64().unwrap() % r.as_i64().unwrap() == 0 {
                return Ok(json!(l.as_i64().unwrap() / r.as_i64().unwrap()));
            }
            Ok(json!(a / b))
        }

        BinOp::Mod => {
            // Prefer integer path when both are integer
            if let (Some(a), Some(b)) = (l.as_i64(), r.as_i64()) {
                if b == 0 {
                    return Err("Modulo by zero".to_string());
                }
                return Ok(json!(a % b));
            }
            let a = l
                .as_f64()
                .ok_or_else(|| format!("Cannot mod: left operand {l} is not a number"))?;
            let b = r
                .as_f64()
                .ok_or_else(|| format!("Cannot mod: right operand {r} is not a number"))?;
            if b == 0.0 {
                return Err("Modulo by zero".to_string());
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
            let result = a.powf(b);
            // Preserve integer when both inputs are integer and result is exact
            if l.is_i64() && r.is_i64() {
                let ri = result as i64;
                if ri as f64 == result {
                    return Ok(json!(ri));
                }
            }
            Ok(json!(result))
        }

        // Equality: compare numbers by value (f64), everything else structurally
        BinOp::Eq => Ok(json!(values_equal(l, r))),
        BinOp::Neq => Ok(json!(!values_equal(l, r))),

        // Ordering: numbers by value, strings lexicographically, mixed types error
        BinOp::Lt => compare_values(l, r, |ord| ord == Ordering::Less),
        BinOp::Gt => compare_values(l, r, |ord| ord == Ordering::Greater),
        BinOp::LtEq => compare_values(l, r, |ord| ord != Ordering::Greater),
        BinOp::GtEq => compare_values(l, r, |ord| ord != Ordering::Less),

        // Logical: strict boolean, short-circuit on LHS
        BinOp::And => {
            let a = l
                .as_bool()
                .ok_or_else(|| format!("Cannot AND: {l} is not a boolean"))?;
            if !a {
                return Ok(json!(false));
            }
            let b = r
                .as_bool()
                .ok_or_else(|| format!("Cannot AND: {r} is not a boolean"))?;
            Ok(json!(b))
        }
        BinOp::Or => {
            let a = l
                .as_bool()
                .ok_or_else(|| format!("Cannot OR: {l} is not a boolean"))?;
            if a {
                return Ok(json!(true));
            }
            let b = r
                .as_bool()
                .ok_or_else(|| format!("Cannot OR: {r} is not a boolean"))?;
            Ok(json!(b))
        }
    }
}

pub fn eval_unary(op: UnaryOp, v: &Value) -> Result<Value, String> {
    match op {
        UnaryOp::Neg => {
            if let Some(n) = v.as_i64() {
                return Ok(json!(-n));
            }
            if let Some(n) = v.as_f64() {
                return Ok(json!(-n));
            }
            Err(format!("Cannot negate {v}"))
        }
        // Strict boolean — no truthiness coercion
        UnaryOp::Not => {
            let b = v
                .as_bool()
                .ok_or_else(|| format!("Cannot NOT: {v} is not a boolean"))?;
            Ok(json!(!b))
        }
    }
}

pub fn pattern_matches(value: &Value, pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Lit(lit) => values_equal(value, lit),
        Pattern::Ident(name) => {
            if name == "_" {
                return true;
            }
            if let Some(s) = value.as_str() {
                s == name
            } else {
                false
            }
        }
        Pattern::Or(alts) => alts.iter().any(|p| pattern_matches(value, p)),
        Pattern::Range { lo, hi } => {
            if let Some(n) = value.as_i64() {
                n >= *lo && n < *hi
            } else if let Some(f) = value.as_f64() {
                f >= *lo as f64 && f < *hi as f64
            } else {
                false
            }
        }
        Pattern::Type(t) => match t.as_str() {
            "text" => value.is_string(),
            "bool" => value.is_boolean(),
            "long" => value.is_i64(),
            "real" => value.is_f64(),
            "list" => value.is_array(),
            "dict" => value.is_object(),
            "void" => value.is_null(),
            _ => false,
        },
    }
}

pub fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        _ => a == b,
    }
}

pub fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Null => false,
        Value::String(s) => !s.is_empty(),
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        _ => true, // arrays, objects → truthy
    }
}

pub fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else {
                n.to_string()
            }
        }
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

pub fn coerce_index(idx: &Value) -> Result<i64, String> {
    idx.as_i64()
        .or_else(|| {
            idx.as_f64().and_then(|f| {
                let r = f as i64;
                if r as f64 == f { Some(r) } else { None }
            })
        })
        .ok_or_else(|| format!("Index must be an integer, got {idx}"))
}

fn num_binop(l: &Value, r: &Value, f: fn(f64, f64) -> f64, name: &str) -> Result<Value, String> {
    let a = l
        .as_f64()
        .ok_or_else(|| format!("Cannot {name}: left operand {l} is not a number"))?;
    let b = r
        .as_f64()
        .ok_or_else(|| format!("Cannot {name}: right operand {r} is not a number"))?;
    let result = f(a, b);
    // Preserve integer when both inputs are integer and result is exact
    if l.is_i64() && r.is_i64() {
        let ri = result as i64;
        if ri as f64 == result {
            return Ok(json!(ri));
        }
    }
    Ok(json!(result))
}

fn compare_values(l: &Value, r: &Value, pred: fn(Ordering) -> bool) -> Result<Value, String> {
    if let (Some(a), Some(b)) = (l.as_f64(), r.as_f64()) {
        return Ok(json!(pred(a.partial_cmp(&b).unwrap_or(Ordering::Equal))));
    }
    if let (Some(a), Some(b)) = (l.as_str(), r.as_str()) {
        return Ok(json!(pred(a.cmp(b))));
    }
    Err(format!("Cannot compare {l} and {r}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Arithmetic ---

    #[test]
    fn add_integers() {
        let r = eval_binop(BinOp::Add, &json!(3), &json!(4)).unwrap();
        assert_eq!(r, json!(7));
        assert!(r.is_i64());
    }

    #[test]
    fn add_floats() {
        let r = eval_binop(BinOp::Add, &json!(1.5), &json!(2.5)).unwrap();
        assert_eq!(r, json!(4.0));
    }

    #[test]
    fn add_mixed_int_float() {
        let r = eval_binop(BinOp::Add, &json!(1), &json!(2.5)).unwrap();
        assert_eq!(r, json!(3.5));
    }

    #[test]
    fn add_strings() {
        let r = eval_binop(BinOp::Add, &json!("hello"), &json!(" world")).unwrap();
        assert_eq!(r, json!("hello world"));
    }

    #[test]
    fn add_string_number_errors() {
        assert!(eval_binop(BinOp::Add, &json!("hi"), &json!(42)).is_err());
        assert!(eval_binop(BinOp::Add, &json!(42), &json!("hi")).is_err());
    }

    #[test]
    fn sub_integers() {
        let r = eval_binop(BinOp::Sub, &json!(10), &json!(3)).unwrap();
        assert_eq!(r, json!(7));
        assert!(r.is_i64());
    }

    #[test]
    fn mul_integers() {
        let r = eval_binop(BinOp::Mul, &json!(3), &json!(4)).unwrap();
        assert_eq!(r, json!(12));
        assert!(r.is_i64());
    }

    // --- Division: integer preservation ---

    #[test]
    fn div_exact_preserves_integer() {
        let r = eval_binop(BinOp::Div, &json!(10), &json!(2)).unwrap();
        assert_eq!(r, json!(5));
        assert!(r.is_i64());
    }

    #[test]
    fn div_inexact_returns_float() {
        let r = eval_binop(BinOp::Div, &json!(10), &json!(3)).unwrap();
        assert!(r.is_f64());
        let f = r.as_f64().unwrap();
        assert!((f - 3.333333333333333).abs() < 1e-10);
    }

    #[test]
    fn div_negative_exact_preserves_integer() {
        let r = eval_binop(BinOp::Div, &json!(-10), &json!(2)).unwrap();
        assert_eq!(r, json!(-5));
        assert!(r.is_i64(), "-10 / 2 should stay integer");
    }

    #[test]
    fn div_negative_inexact_returns_float() {
        // -7 / 2 → -3.5 (no floor/truncation — exact float result)
        let r = eval_binop(BinOp::Div, &json!(-7), &json!(2)).unwrap();
        assert_eq!(r, json!(-3.5));
    }

    #[test]
    fn div_mixed_int_float_returns_float() {
        // When one operand is float, result is always float
        let r = eval_binop(BinOp::Div, &json!(10), &json!(2.0)).unwrap();
        assert_eq!(r.as_f64().unwrap(), 5.0);
        assert!(!r.is_i64(), "10 / 2.0 should be float (mixed operands)");
    }

    #[test]
    fn div_by_zero_errors() {
        assert!(eval_binop(BinOp::Div, &json!(10), &json!(0)).is_err());
    }

    #[test]
    fn div_by_zero_float_errors() {
        assert!(eval_binop(BinOp::Div, &json!(10), &json!(0.0)).is_err());
    }

    #[test]
    fn div_large_exact_preserves_integer() {
        let r = eval_binop(BinOp::Div, &json!(1000000), &json!(1000)).unwrap();
        assert_eq!(r, json!(1000));
        assert!(r.is_i64());
    }

    // --- Power: integer preservation ---

    #[test]
    fn pow_integer_preserves() {
        let r = eval_binop(BinOp::Pow, &json!(2), &json!(3)).unwrap();
        assert_eq!(r, json!(8));
        assert!(r.is_i64());
    }

    #[test]
    fn pow_large_integer_preserves() {
        let r = eval_binop(BinOp::Pow, &json!(2), &json!(10)).unwrap();
        assert_eq!(r, json!(1024));
        assert!(r.is_i64());
    }

    #[test]
    fn pow_negative_base_preserves_integer() {
        let r = eval_binop(BinOp::Pow, &json!(-2), &json!(3)).unwrap();
        assert_eq!(r, json!(-8));
        assert!(r.is_i64());
    }

    #[test]
    fn pow_zero_exponent_preserves_integer() {
        let r = eval_binop(BinOp::Pow, &json!(5), &json!(0)).unwrap();
        assert_eq!(r, json!(1));
        assert!(r.is_i64());
    }

    #[test]
    fn pow_fractional_returns_float() {
        let r = eval_binop(BinOp::Pow, &json!(2), &json!(-1)).unwrap();
        assert_eq!(r, json!(0.5));
    }

    #[test]
    fn pow_mixed_float_returns_float() {
        let r = eval_binop(BinOp::Pow, &json!(4.0), &json!(0.5)).unwrap();
        assert_eq!(r, json!(2.0));
    }

    // --- Modulo ---

    #[test]
    fn mod_integers() {
        let r = eval_binop(BinOp::Mod, &json!(10), &json!(3)).unwrap();
        assert_eq!(r, json!(1));
        assert!(r.is_i64());
    }

    #[test]
    fn mod_by_zero_errors() {
        assert!(eval_binop(BinOp::Mod, &json!(10), &json!(0)).is_err());
    }

    // --- Equality: value-based numeric comparison ---

    #[test]
    fn eq_int_float_same_value() {
        let r = eval_binop(BinOp::Eq, &json!(42), &json!(42.0)).unwrap();
        assert_eq!(r, json!(true));
    }

    #[test]
    fn eq_int_int() {
        assert_eq!(
            eval_binop(BinOp::Eq, &json!(5), &json!(5)).unwrap(),
            json!(true)
        );
        assert_eq!(
            eval_binop(BinOp::Eq, &json!(5), &json!(6)).unwrap(),
            json!(false)
        );
    }

    #[test]
    fn neq_int_float() {
        let r = eval_binop(BinOp::Neq, &json!(42), &json!(42.0)).unwrap();
        assert_eq!(r, json!(false));
    }

    #[test]
    fn eq_strings() {
        assert_eq!(
            eval_binop(BinOp::Eq, &json!("a"), &json!("a")).unwrap(),
            json!(true)
        );
        assert_eq!(
            eval_binop(BinOp::Eq, &json!("a"), &json!("b")).unwrap(),
            json!(false)
        );
    }

    #[test]
    fn eq_zero_int_float() {
        // 0 == 0.0 must be true
        assert_eq!(
            eval_binop(BinOp::Eq, &json!(0), &json!(0.0)).unwrap(),
            json!(true)
        );
    }

    #[test]
    fn eq_negative_int_float() {
        assert_eq!(
            eval_binop(BinOp::Eq, &json!(-5), &json!(-5.0)).unwrap(),
            json!(true)
        );
    }

    #[test]
    fn neq_different_values_int_float() {
        // 5 != 6.0 must be true
        assert_eq!(
            eval_binop(BinOp::Neq, &json!(5), &json!(6.0)).unwrap(),
            json!(true)
        );
    }

    #[test]
    fn eq_float_float() {
        assert_eq!(
            eval_binop(BinOp::Eq, &json!(3.14), &json!(3.14)).unwrap(),
            json!(true)
        );
        assert_eq!(
            eval_binop(BinOp::Eq, &json!(3.14), &json!(3.15)).unwrap(),
            json!(false)
        );
    }

    #[test]
    fn eq_different_types_not_equal() {
        // string "42" != number 42
        assert_eq!(
            eval_binop(BinOp::Eq, &json!("42"), &json!(42)).unwrap(),
            json!(false)
        );
    }

    #[test]
    fn eq_bool_not_equal_to_number() {
        assert_eq!(
            eval_binop(BinOp::Eq, &json!(true), &json!(1)).unwrap(),
            json!(false)
        );
        assert_eq!(
            eval_binop(BinOp::Eq, &json!(false), &json!(0)).unwrap(),
            json!(false)
        );
    }

    #[test]
    fn eq_null_values() {
        assert_eq!(
            eval_binop(BinOp::Eq, &json!(null), &json!(null)).unwrap(),
            json!(true)
        );
        assert_eq!(
            eval_binop(BinOp::Eq, &json!(null), &json!(0)).unwrap(),
            json!(false)
        );
    }

    // --- Division then equality (the classic footgun) ---

    #[test]
    fn div_result_equals_integer() {
        let div = eval_binop(BinOp::Div, &json!(10), &json!(2)).unwrap();
        let eq = eval_binop(BinOp::Eq, &div, &json!(5)).unwrap();
        assert_eq!(eq, json!(true));
    }

    // --- Ordering: cross-type numeric comparison ---

    #[test]
    fn lt_int_float_cross_type() {
        assert_eq!(
            eval_binop(BinOp::Lt, &json!(1), &json!(2.5)).unwrap(),
            json!(true)
        );
        assert_eq!(
            eval_binop(BinOp::Gt, &json!(3), &json!(2.5)).unwrap(),
            json!(true)
        );
    }

    #[test]
    fn lteq_int_float_equal_value() {
        assert_eq!(
            eval_binop(BinOp::LtEq, &json!(5), &json!(5.0)).unwrap(),
            json!(true)
        );
        assert_eq!(
            eval_binop(BinOp::GtEq, &json!(5), &json!(5.0)).unwrap(),
            json!(true)
        );
    }

    // --- Pattern matching: value-based numeric comparison ---

    #[test]
    fn pattern_lit_int_matches_float_value() {
        // case 42.0 / when 42 should match
        assert!(pattern_matches(&json!(42.0), &Pattern::Lit(json!(42))));
        assert!(pattern_matches(&json!(42), &Pattern::Lit(json!(42.0))));
    }

    #[test]
    fn pattern_lit_zero_int_matches_float() {
        assert!(pattern_matches(&json!(0), &Pattern::Lit(json!(0.0))));
        assert!(pattern_matches(&json!(0.0), &Pattern::Lit(json!(0))));
    }

    // --- Ordering: numbers and strings ---

    #[test]
    fn lt_numbers() {
        assert_eq!(
            eval_binop(BinOp::Lt, &json!(1), &json!(2)).unwrap(),
            json!(true)
        );
        assert_eq!(
            eval_binop(BinOp::Lt, &json!(2), &json!(1)).unwrap(),
            json!(false)
        );
    }

    #[test]
    fn lt_strings() {
        assert_eq!(
            eval_binop(BinOp::Lt, &json!("a"), &json!("b")).unwrap(),
            json!(true)
        );
        assert_eq!(
            eval_binop(BinOp::Lt, &json!("b"), &json!("a")).unwrap(),
            json!(false)
        );
    }

    #[test]
    fn compare_mixed_types_errors() {
        assert!(eval_binop(BinOp::Lt, &json!("a"), &json!(1)).is_err());
    }

    // --- Logical: strict boolean ---

    #[test]
    fn and_booleans() {
        assert_eq!(
            eval_binop(BinOp::And, &json!(true), &json!(true)).unwrap(),
            json!(true)
        );
        assert_eq!(
            eval_binop(BinOp::And, &json!(true), &json!(false)).unwrap(),
            json!(false)
        );
        assert_eq!(
            eval_binop(BinOp::And, &json!(false), &json!(true)).unwrap(),
            json!(false)
        );
    }

    #[test]
    fn and_non_bool_errors() {
        assert!(eval_binop(BinOp::And, &json!(0), &json!(true)).is_err());
        assert!(eval_binop(BinOp::And, &json!(1), &json!(true)).is_err());
    }

    #[test]
    fn and_short_circuits_on_false_lhs() {
        // false && (non-bool) should return false without checking RHS type
        let r = eval_binop(BinOp::And, &json!(false), &json!(42));
        assert_eq!(r.unwrap(), json!(false));
    }

    #[test]
    fn or_booleans() {
        assert_eq!(
            eval_binop(BinOp::Or, &json!(false), &json!(false)).unwrap(),
            json!(false)
        );
        assert_eq!(
            eval_binop(BinOp::Or, &json!(false), &json!(true)).unwrap(),
            json!(true)
        );
        assert_eq!(
            eval_binop(BinOp::Or, &json!(true), &json!(false)).unwrap(),
            json!(true)
        );
    }

    #[test]
    fn or_short_circuits_on_true_lhs() {
        let r = eval_binop(BinOp::Or, &json!(true), &json!(42));
        assert_eq!(r.unwrap(), json!(true));
    }

    #[test]
    fn or_non_bool_errors() {
        assert!(eval_binop(BinOp::Or, &json!(0), &json!(true)).is_err());
    }

    // --- Unary ---

    #[test]
    fn negate_integer() {
        assert_eq!(eval_unary(UnaryOp::Neg, &json!(5)).unwrap(), json!(-5));
        assert!(eval_unary(UnaryOp::Neg, &json!(5)).unwrap().is_i64());
    }

    #[test]
    fn negate_float() {
        assert_eq!(
            eval_unary(UnaryOp::Neg, &json!(3.14)).unwrap(),
            json!(-3.14)
        );
    }

    #[test]
    fn negate_non_number_errors() {
        assert!(eval_unary(UnaryOp::Neg, &json!("hi")).is_err());
    }

    #[test]
    fn not_boolean() {
        assert_eq!(
            eval_unary(UnaryOp::Not, &json!(true)).unwrap(),
            json!(false)
        );
        assert_eq!(
            eval_unary(UnaryOp::Not, &json!(false)).unwrap(),
            json!(true)
        );
    }

    #[test]
    fn not_non_bool_errors() {
        assert!(eval_unary(UnaryOp::Not, &json!(0)).is_err());
        assert!(eval_unary(UnaryOp::Not, &json!("")).is_err());
    }

    // --- Pattern matching ---

    #[test]
    fn pattern_lit_matches() {
        assert!(pattern_matches(&json!(42), &Pattern::Lit(json!(42))));
        assert!(pattern_matches(
            &json!("hello"),
            &Pattern::Lit(json!("hello"))
        ));
        assert!(!pattern_matches(&json!(42), &Pattern::Lit(json!(43))));
    }

    #[test]
    fn pattern_lit_numeric_value_equality() {
        assert!(pattern_matches(&json!(42), &Pattern::Lit(json!(42.0))));
    }

    #[test]
    fn pattern_ident_matches_string() {
        assert!(pattern_matches(
            &json!("active"),
            &Pattern::Ident("active".to_string())
        ));
        assert!(!pattern_matches(
            &json!("inactive"),
            &Pattern::Ident("active".to_string())
        ));
    }

    #[test]
    fn pattern_wildcard() {
        assert!(pattern_matches(
            &json!(42),
            &Pattern::Ident("_".to_string())
        ));
        assert!(pattern_matches(
            &json!("anything"),
            &Pattern::Ident("_".to_string())
        ));
        assert!(pattern_matches(
            &json!(null),
            &Pattern::Ident("_".to_string())
        ));
    }

    // --- Truthiness ---

    #[test]
    fn truthy_values() {
        assert!(is_truthy(&json!(true)));
        assert!(is_truthy(&json!(1)));
        assert!(is_truthy(&json!(-1)));
        assert!(is_truthy(&json!(0.1)));
        assert!(is_truthy(&json!("hello")));
        assert!(is_truthy(&json!([1, 2])));
        assert!(is_truthy(&json!({"a": 1})));
    }

    #[test]
    fn falsy_values() {
        assert!(!is_truthy(&json!(false)));
        assert!(!is_truthy(&json!(null)));
        assert!(!is_truthy(&json!("")));
        assert!(!is_truthy(&json!(0)));
        assert!(!is_truthy(&json!(0.0)));
    }

    // --- values_equal ---

    #[test]
    fn values_equal_numbers() {
        assert!(values_equal(&json!(42), &json!(42.0)));
        assert!(values_equal(&json!(0), &json!(0.0)));
        assert!(!values_equal(&json!(42), &json!(43)));
    }

    #[test]
    fn values_equal_non_numbers() {
        assert!(values_equal(&json!("a"), &json!("a")));
        assert!(!values_equal(&json!("a"), &json!("b")));
        assert!(values_equal(&json!(true), &json!(true)));
        assert!(!values_equal(&json!(true), &json!(false)));
        assert!(values_equal(&json!(null), &json!(null)));
    }

    // --- coerce_index ---

    #[test]
    fn coerce_index_integer() {
        assert_eq!(coerce_index(&json!(3)).unwrap(), 3);
    }

    #[test]
    fn coerce_index_exact_float() {
        assert_eq!(coerce_index(&json!(5.0)).unwrap(), 5);
    }

    #[test]
    fn coerce_index_inexact_float_errors() {
        assert!(coerce_index(&json!(3.5)).is_err());
    }

    #[test]
    fn coerce_index_string_errors() {
        assert!(coerce_index(&json!("3")).is_err());
    }

    // --- value_to_string ---

    #[test]
    fn value_to_string_types() {
        assert_eq!(value_to_string(&json!("hello")), "hello");
        assert_eq!(value_to_string(&json!(42)), "42");
        assert_eq!(value_to_string(&json!(3.14)), "3.14");
        assert_eq!(value_to_string(&json!(true)), "true");
        assert_eq!(value_to_string(&json!(null)), "null");
    }

    // --- OR pattern ---

    #[test]
    fn pattern_or_matches() {
        let pat = Pattern::Or(vec![
            Pattern::Lit(json!("yes")),
            Pattern::Lit(json!("y")),
            Pattern::Lit(json!("true")),
        ]);
        assert!(pattern_matches(&json!("yes"), &pat));
        assert!(pattern_matches(&json!("y"), &pat));
        assert!(pattern_matches(&json!("true"), &pat));
        assert!(!pattern_matches(&json!("no"), &pat));
    }

    // --- Range pattern ---

    #[test]
    fn pattern_range_inclusive_lo_exclusive_hi() {
        let pat = Pattern::Range { lo: 90, hi: 101 };
        assert!(pattern_matches(&json!(90), &pat));
        assert!(pattern_matches(&json!(100), &pat));
        assert!(!pattern_matches(&json!(101), &pat));
        assert!(!pattern_matches(&json!(89), &pat));
    }

    #[test]
    fn pattern_range_float_value() {
        let pat = Pattern::Range { lo: 0, hi: 10 };
        assert!(pattern_matches(&json!(5.5), &pat));
        assert!(!pattern_matches(&json!(10.0), &pat));
        assert!(pattern_matches(&json!(0.0), &pat));
    }

    #[test]
    fn pattern_range_negative_bounds() {
        let pat = Pattern::Range { lo: -10, hi: 0 };
        assert!(pattern_matches(&json!(-10), &pat));
        assert!(pattern_matches(&json!(-1), &pat));
        assert!(!pattern_matches(&json!(0), &pat));
    }

    #[test]
    fn pattern_range_non_number_no_match() {
        let pat = Pattern::Range { lo: 0, hi: 10 };
        assert!(!pattern_matches(&json!("5"), &pat));
    }

    // --- Type pattern ---

    #[test]
    fn pattern_type_text() {
        let pat = Pattern::Type("text".to_string());
        assert!(pattern_matches(&json!("hello"), &pat));
        assert!(!pattern_matches(&json!(42), &pat));
    }

    #[test]
    fn pattern_type_bool() {
        let pat = Pattern::Type("bool".to_string());
        assert!(pattern_matches(&json!(true), &pat));
        assert!(!pattern_matches(&json!(1), &pat));
    }

    #[test]
    fn pattern_type_long() {
        let pat = Pattern::Type("long".to_string());
        assert!(pattern_matches(&json!(42), &pat));
        assert!(!pattern_matches(&json!(3.14), &pat));
    }

    #[test]
    fn pattern_type_real() {
        let pat = Pattern::Type("real".to_string());
        assert!(pattern_matches(&json!(3.14), &pat));
        assert!(!pattern_matches(&json!(42), &pat));
    }

    #[test]
    fn pattern_type_list() {
        let pat = Pattern::Type("list".to_string());
        assert!(pattern_matches(&json!([1, 2]), &pat));
        assert!(!pattern_matches(&json!("list"), &pat));
    }

    #[test]
    fn pattern_type_dict() {
        let pat = Pattern::Type("dict".to_string());
        assert!(pattern_matches(&json!({"a": 1}), &pat));
        assert!(!pattern_matches(&json!([1]), &pat));
    }

    #[test]
    fn pattern_type_void() {
        let pat = Pattern::Type("void".to_string());
        assert!(pattern_matches(&json!(null), &pat));
        assert!(!pattern_matches(&json!(0), &pat));
    }

    #[test]
    fn pattern_type_unknown_no_match() {
        let pat = Pattern::Type("unknown".to_string());
        assert!(!pattern_matches(&json!("anything"), &pat));
    }
}
