use crate::consts::{dav_attribute_map, DavNamespace};
use serde_json::{Map, Value};
use std::collections::HashMap;
use url::Url;

pub fn cleanup_falsy(obj: Map<String, Value>) -> Map<String, Value> {
    obj.into_iter()
        .filter(|(_, v)| match v {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
            Value::String(s) => !s.is_empty(),
            Value::Array(_) | Value::Object(_) => true,
        })
        .collect()
}

pub fn conditional_param(key: &str, param: Option<Value>) -> Map<String, Value> {
    let mut m = Map::new();
    if let Some(v) = param {
        let truthy = !matches!(&v, Value::Null)
            && !matches!(&v, Value::Bool(false))
            && !matches!(&v, Value::String(s) if s.is_empty())
            && !matches!(&v, Value::Number(n) if n.as_f64() == Some(0.0));
        if truthy {
            m.insert(key.to_string(), v);
        }
    }
    m
}

fn normalize_url(url: &str) -> String {
    let trimmed = url.trim();
    trimmed.strip_suffix('/').unwrap_or(trimmed).to_string()
}

pub fn ensure_trailing_slash(url: &str) -> String {
    let trimmed = url.trim();
    let suffix_index = trimmed.find(|c| c == '?' || c == '#');
    let (pathname, suffix) = match suffix_index {
        Some(idx) => (&trimmed[..idx], &trimmed[idx..]),
        None => (trimmed, ""),
    };
    let pathname = if pathname.ends_with('/') {
        pathname.to_string()
    } else {
        format!("{pathname}/")
    };
    format!("{pathname}{suffix}")
}

pub fn url_equals(url_a: Option<&str>, url_b: Option<&str>) -> bool {
    match (url_a, url_b) {
        (None, None) => true,
        (Some(_), None) | (None, Some(_)) => false,
        (Some(a), Some(b)) => normalize_url(a) == normalize_url(b),
    }
}

pub fn url_contains(url_a: Option<&str>, url_b: Option<&str>) -> bool {
    match (url_a, url_b) {
        (None, None) => true,
        (Some(_), None) | (None, Some(_)) => false,
        (Some(a), Some(b)) => {
            let na = normalize_url(a);
            let nb = normalize_url(b);
            na.contains(&nb) || nb.contains(&na)
        }
    }
}

pub fn url_matches(url_a: Option<&str>, url_b: Option<&str>, base_url: Option<&str>) -> bool {
    let (a, b, base) = match (url_a, url_b, base_url) {
        (Some(a), Some(b), Some(base)) => (a, b, base),
        _ => return url_equals(url_a, url_b),
    };
    let resolve = |href: &str| -> Option<String> {
        Url::parse(base)
            .ok()
            .and_then(|base_url| base_url.join(href).ok())
            .map(|u| u.to_string())
    };
    match (resolve(a), resolve(b)) {
        (Some(ra), Some(rb)) => url_equals(Some(&ra), Some(&rb)),
        _ => url_equals(url_a, url_b),
    }
}

pub fn resolve_href(href: &str, base_url: &str) -> String {
    Url::parse(&ensure_trailing_slash(base_url))
        .ok()
        .and_then(|base| base.join(href).ok())
        .map(|u| u.to_string())
        .unwrap_or_else(|| href.to_string())
}

pub fn get_dav_attribute(ns_arr: &[DavNamespace]) -> Vec<(&'static str, &'static str)> {
    ns_arr
        .iter()
        .map(|&ns| (dav_attribute_map(ns), ns.as_str()))
        .collect()
}

pub fn exclude_headers(
    headers: &HashMap<String, String>,
    headers_to_exclude: Option<&[String]>,
) -> HashMap<String, String> {
    match headers_to_exclude {
        None => headers.clone(),
        Some(exclude) if exclude.is_empty() => headers.clone(),
        Some(exclude) => {
            let exclude_set: std::collections::HashSet<String> =
                exclude.iter().map(|h| h.to_lowercase()).collect();
            headers
                .iter()
                .filter(|(k, _)| !exclude_set.contains(&k.to_lowercase()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        }
    }
}

pub fn merge_headers(sources: &[&HashMap<String, String>]) -> HashMap<String, String> {
    let mut by_lower: HashMap<String, (String, String)> = HashMap::new();
    for source in sources {
        for (name, value) in source.iter() {
            by_lower.insert(name.to_lowercase(), (name.clone(), value.clone()));
        }
    }
    by_lower.into_values().collect()
}
