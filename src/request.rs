use crate::consts::{DavNamespace, DavNamespaceShort};
use crate::error::DavError;
use crate::types::dav_types::{DavDepth, DavMethod, DavRequest, DavResponse};
use crate::util::fetch::{BrowserFetchClient, FetchClient, FetchOptions};
use crate::util::request_helpers::{exclude_headers, get_dav_attribute, merge_headers};
use crate::xml::{js_to_xml, xml_to_js};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone, Default)]
pub struct Ctx {
    pub headers: HashMap<String, String>,
    pub headers_to_exclude: Vec<String>,
    pub fetch_options: FetchOptions,
    pub fetch: Option<Rc<dyn FetchClient>>,
}

impl Ctx {
    pub fn headers(&self) -> HashMap<String, String> {
        exclude_headers(&self.headers, Some(&self.headers_to_exclude))
    }

    pub fn client(&self) -> Rc<dyn FetchClient> {
        self.fetch
            .clone()
            .unwrap_or_else(|| Rc::new(BrowserFetchClient) as Rc<dyn FetchClient>)
    }
}

fn filter_falsy_headers(headers: &HashMap<String, String>) -> HashMap<String, String> {
    headers
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

fn parse_status_line(line: Option<&str>) -> Option<(u16, String)> {
    let line = line?;
    let bytes = line.as_bytes();
    let mut idx = 0;
    let start = idx;
    while idx < bytes.len() && !bytes[idx].is_ascii_whitespace() {
        idx += 1;
    }
    if idx == start {
        return None;
    }
    let ws_start = idx;
    while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
        idx += 1;
    }
    if idx == ws_start {
        return None;
    }
    if idx + 3 > bytes.len() {
        return None;
    }
    let status_str = &line[idx..idx + 3];
    if !status_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let status: u16 = status_str.parse().ok()?;
    idx += 3;
    if idx == bytes.len() {
        return Some((status, String::new()));
    }
    if !bytes[idx].is_ascii_whitespace() {
        return None;
    }
    while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
        idx += 1;
    }
    Some((status, line[idx..].to_string()))
}

fn value_as_str(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        Some(Value::Bool(b)) => Some(b.to_string()),
        _ => None,
    }
}

pub async fn dav_request(
    url: &str,
    mut init: DavRequest,
    convert_incoming: bool,
    parse_outgoing: bool,
    fetch_options: &FetchOptions,
    client: &Rc<dyn FetchClient>,
) -> Result<Vec<DavResponse>, DavError> {
    let method = init
        .method
        .ok_or_else(|| DavError::msg("DavRequest.method is required"))?;

    if let (Some(attrs), Some(Value::Object(body_map))) =
        (init.attributes.clone(), init.body.as_mut())
    {
        for (_key, value) in body_map.iter_mut() {
            if let Value::Object(el) = value {
                let mut merged = Map::new();
                for (k, v) in &attrs {
                    merged.insert(k.clone(), Value::String(v.clone()));
                }
                if let Some(Value::Object(existing)) = el.get("_attributes") {
                    for (k, v) in existing {
                        merged.insert(k.clone(), v.clone());
                    }
                }
                el.insert("_attributes".to_string(), Value::Object(merged));
            }
        }
    }

    let xml_body: Option<String> = if convert_incoming {
        match &init.body {
            Some(Value::Object(map)) => Some(js_to_xml(map, init.namespace)?),
            _ => None,
        }
    } else {
        value_as_str(init.body.as_ref())
    };

    let mut content_type = HashMap::new();
    content_type.insert(
        "Content-Type".to_string(),
        "text/xml;charset=UTF-8".to_string(),
    );
    let cleaned_init_headers = filter_falsy_headers(&init.headers);
    let merged_headers = merge_headers(&[
        &content_type,
        &cleaned_init_headers,
        &fetch_options.headers,
    ]);

    let (http_resp, res_text) = client
        .send(url, method.as_str(), &merged_headers, xml_body.as_deref(), fetch_options)
        .await?;

    let content_type_header = http_resp
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.to_lowercase());
    let looks_like_xml = content_type_header
        .map(|ct| ct.contains("xml"))
        .unwrap_or(false);

    if !http_resp.ok || !looks_like_xml || !parse_outgoing || res_text.is_empty() {
        const MAX_RAW: usize = 4096;
        let raw = if res_text.chars().count() > MAX_RAW {
            let truncated: String = res_text.chars().take(MAX_RAW).collect();
            format!("{truncated}…")
        } else {
            res_text
        };
        return Ok(vec![DavResponse {
            href: Some(http_resp.url),
            ok: http_resp.ok,
            status: http_resp.status,
            status_text: http_resp.status_text,
            raw: Some(Value::String(raw)),
            ..Default::default()
        }]);
    }

    let result = match xml_to_js(&res_text) {
        Ok(v) => v,
        Err(_) => {
            return Ok(vec![DavResponse {
                href: Some(http_resp.url),
                ok: http_resp.ok,
                status: http_resp.status,
                status_text: http_resp.status_text,
                raw: Some(Value::String(res_text)),
                ..Default::default()
            }]);
        }
    };

    let multistatus = match result.get("multistatus") {
        Some(m) => m,
        None => {
            return Ok(vec![DavResponse {
                href: Some(http_resp.url),
                ok: http_resp.ok,
                status: http_resp.status,
                status_text: http_resp.status_text,
                raw: Some(result),
                ..Default::default()
            }]);
        }
    };

    let response_field = multistatus.get("response");
    let response_bodies: Vec<Value> = match response_field {
        Some(Value::Array(items)) => items.clone(),
        Some(other) => vec![other.clone()],
        None => vec![Value::Null],
    };

    let out = response_bodies
        .into_iter()
        .map(|response_body| {
            if response_body.is_null() {
                return DavResponse {
                    raw: Some(result.clone()),
                    status: http_resp.status,
                    status_text: http_resp.status_text.clone(),
                    ok: http_resp.ok,
                    ..Default::default()
                };
            }

            let status_line = response_body.get("status").and_then(|v| v.as_str());
            let parsed_status = parse_status_line(status_line);
            let status = parsed_status.as_ref().map(|(s, _)| *s).unwrap_or(http_resp.status);
            let status_text = parsed_status
                .map(|(_, t)| t)
                .unwrap_or_else(|| http_resp.status_text.clone());

            let propstat = response_body.get("propstat");
            let propstat_list: Vec<&Value> = match propstat {
                Some(Value::Array(items)) => items.iter().collect(),
                Some(other) => vec![other],
                None => vec![],
            };
            let mut props = Map::new();
            for ps in propstat_list {
                let ps_status = ps.get("status").and_then(|v| v.as_str());
                if let Some((code, _)) = parse_status_line(ps_status) {
                    if !(200..300).contains(&code) {
                        continue;
                    }
                }
                if let Some(Value::Object(prop)) = ps.get("prop") {
                    for (k, v) in prop {
                        props.insert(k.clone(), v.clone());
                    }
                }
            }

            DavResponse {
                raw: Some(result.clone()),
                href: response_body.get("href").and_then(|v| v.as_str()).map(String::from),
                status,
                status_text,
                ok: (200..300).contains(&status),
                error: response_body.get("error").cloned(),
                responsedescription: response_body
                    .get("responsedescription")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                props: Some(Value::Object(props)),
            }
        })
        .collect();

    Ok(out)
}

pub async fn propfind(
    url: &str,
    props: Value,
    depth: Option<DavDepth>,
    ctx: &Ctx,
) -> Result<Vec<DavResponse>, DavError> {
    let mut headers = ctx.headers();
    if let Some(d) = depth {
        headers.insert("depth".to_string(), d.as_str().to_string());
    }

    let attrs = get_dav_attribute(&[
        DavNamespace::Caldav,
        DavNamespace::CaldavApple,
        DavNamespace::CalendarServer,
        DavNamespace::Carddav,
        DavNamespace::Dav,
    ]);
    let mut attr_map = Map::new();
    for (k, v) in attrs {
        attr_map.insert(k.to_string(), Value::String(v.to_string()));
    }

    let mut propfind_el = Map::new();
    propfind_el.insert("_attributes".to_string(), Value::Object(attr_map));
    propfind_el.insert("prop".to_string(), props);
    let mut body = Map::new();
    body.insert("propfind".to_string(), Value::Object(propfind_el));

    dav_request(
        url,
        DavRequest {
            method: Some(DavMethod::Propfind),
            headers,
            namespace: Some(DavNamespaceShort::Dav),
            body: Some(Value::Object(body)),
            attributes: None,
        },
        true,
        true,
        &ctx.fetch_options,
        &ctx.client(),
    )
    .await
}

pub struct HttpResult {
    pub status: u16,
    pub status_text: String,
    pub ok: bool,
    pub url: String,
    pub headers: HashMap<String, String>,
}

impl From<crate::util::fetch::HttpResponse> for HttpResult {
    fn from(r: crate::util::fetch::HttpResponse) -> Self {
        HttpResult {
            status: r.status,
            status_text: r.status_text,
            ok: r.ok,
            url: r.url,
            headers: r.headers,
        }
    }
}

pub async fn create_object(
    url: &str,
    data: &str,
    ctx: &Ctx,
) -> Result<HttpResult, DavError> {
    let headers = merge_headers(&[&ctx.headers(), &ctx.fetch_options.headers]);
    let (resp, _text) = ctx
        .client()
        .send(url, "PUT", &headers, Some(data), &ctx.fetch_options)
        .await?;
    Ok(resp.into())
}

pub async fn update_object(
    url: &str,
    data: &str,
    etag: Option<&str>,
    ctx: &Ctx,
) -> Result<HttpResult, DavError> {
    let mut headers = ctx.headers.clone();
    if let Some(e) = etag {
        if !e.is_empty() {
            headers.insert("If-Match".to_string(), e.to_string());
        }
    }
    let headers = exclude_headers(
        &merge_headers(&[&cleanup_falsy_str_map(&headers), &ctx.fetch_options.headers]),
        Some(&ctx.headers_to_exclude),
    );
    let (resp, _text) = ctx
        .client()
        .send(url, "PUT", &headers, Some(data), &ctx.fetch_options)
        .await?;
    Ok(resp.into())
}

pub async fn delete_object(url: &str, etag: Option<&str>, ctx: &Ctx) -> Result<HttpResult, DavError> {
    let mut headers = ctx.headers.clone();
    if let Some(e) = etag {
        if !e.is_empty() {
            headers.insert("If-Match".to_string(), e.to_string());
        }
    }
    let headers = exclude_headers(
        &merge_headers(&[&cleanup_falsy_str_map(&headers), &ctx.fetch_options.headers]),
        Some(&ctx.headers_to_exclude),
    );
    let (resp, _text) = ctx
        .client()
        .send(url, "DELETE", &headers, None, &ctx.fetch_options)
        .await?;
    Ok(resp.into())
}

fn cleanup_falsy_str_map(m: &HashMap<String, String>) -> HashMap<String, String> {
    m.iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}
