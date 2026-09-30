pub fn camel_case(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '-' || c == '_' {
            let mut j = i;
            while j < chars.len() && (chars[j] == '-' || chars[j] == '_') {
                j += 1;
            }
            if j < chars.len() {
                let nc = chars[j];
                if nc.is_alphanumeric() || nc == '_' {
                    out.extend(nc.to_uppercase());
                    i = j + 1;
                    continue;
                }
            }
            i = j;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_separator_runs() {
        assert_eq!(camel_case("foo-bar"), "fooBar");
        assert_eq!(camel_case("foo--bar"), "fooBar");
        assert_eq!(camel_case("foo_-bar"), "fooBar");
        assert_eq!(camel_case("getctag"), "getctag");
        assert_eq!(camel_case("current-user-principal"), "currentUserPrincipal");
        assert_eq!(camel_case("trailing-"), "trailing");
    }
}
