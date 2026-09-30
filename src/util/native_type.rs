use serde_json::{Number, Value};

fn is_numeric(value: &str) -> bool {
    let b = value.as_bytes();
    let n = b.len();
    let mut i = 0;
    if n == 0 {
        return false;
    }
    if b[i] == b'-' {
        i += 1;
    }
    if i >= n {
        return false;
    }
    if b[i] == b'0' {
        i += 1;
    } else if b[i].is_ascii_digit() {
        i += 1;
        while i < n && b[i].is_ascii_digit() {
            i += 1;
        }
    } else {
        return false;
    }
    if i < n && b[i] == b'.' {
        i += 1;
        let frac_start = i;
        while i < n && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == frac_start {
            return false;
        }
    }
    if i < n && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < n && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let exp_start = i;
        while i < n && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == exp_start {
            return false;
        }
    }
    i == n
}

const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

pub fn native_type(value: &str) -> Value {
    if is_numeric(value) {
        if let Ok(n_value) = value.parse::<f64>() {
            if n_value.is_finite() {
                let is_integer = n_value.fract() == 0.0;
                let is_safe_integer = is_integer
                    && n_value.abs() <= MAX_SAFE_INTEGER as f64
                    && (n_value as i64) as f64 == n_value;
                if !is_integer || is_safe_integer {
                    if is_integer {
                        return Value::Number((n_value as i64).into());
                    }
                    if let Some(num) = Number::from_f64(n_value) {
                        return Value::Number(num);
                    }
                }
            }
        }
    }
    let lower = value.to_lowercase();
    if lower == "true" {
        return Value::Bool(true);
    }
    if lower == "false" {
        return Value::Bool(false);
    }
    Value::String(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coerces_numbers_bools_and_preserves_strings() {
        assert_eq!(native_type("42"), Value::Number(42.into()));
        assert_eq!(native_type("true"), Value::Bool(true));
        assert_eq!(native_type("False"), Value::Bool(false));
        assert_eq!(native_type("0755"), Value::String("0755".to_string()));
        assert_eq!(native_type("hello"), Value::String("hello".to_string()));
        assert_eq!(native_type(""), Value::String("".to_string()));
    }
}
