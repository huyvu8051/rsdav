use crate::error::DavError;
use std::collections::HashMap;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

#[derive(Debug, Clone, Default)]
pub struct FetchOptions {
    pub redirect: Option<web_sys::RequestRedirect>,
    pub credentials: Option<web_sys::RequestCredentials>,
    pub mode: Option<web_sys::RequestMode>,
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub url: String,
    pub status: u16,
    pub status_text: String,
    pub ok: bool,
    pub headers: HashMap<String, String>,
}

fn js_err_to_string(e: &JsValue) -> String {
    e.as_string()
        .or_else(|| js_sys::Error::from(e.clone()).message().as_string())
        .unwrap_or_else(|| format!("{e:?}"))
}

fn headers_to_map(headers: &web_sys::Headers) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Ok(Some(iter)) = js_sys::try_iter(headers) {
        for entry in iter.flatten() {
            if let Ok(arr) = entry.dyn_into::<js_sys::Array>() {
                if arr.length() >= 2 {
                    if let (Some(k), Some(v)) = (arr.get(0).as_string(), arr.get(1).as_string()) {
                        map.insert(k, v);
                    }
                }
            }
        }
    }
    map
}

#[async_trait::async_trait(?Send)]
pub trait FetchClient {
    async fn send(
        &self,
        url: &str,
        method: &str,
        headers: &HashMap<String, String>,
        body: Option<&str>,
        options: &FetchOptions,
    ) -> Result<(HttpResponse, String), DavError>;
}

pub struct BrowserFetchClient;

#[async_trait::async_trait(?Send)]
impl FetchClient for BrowserFetchClient {
    async fn send(
        &self,
        url: &str,
        method: &str,
        headers: &HashMap<String, String>,
        body: Option<&str>,
        options: &FetchOptions,
    ) -> Result<(HttpResponse, String), DavError> {
        let init = web_sys::RequestInit::new();
        init.set_method(method);
        if let Some(b) = body {
            init.set_body(&JsValue::from_str(b));
        }
        if let Some(redirect) = options.redirect {
            init.set_redirect(redirect);
        }
        if let Some(credentials) = options.credentials {
            init.set_credentials(credentials);
        }
        if let Some(mode) = options.mode {
            init.set_mode(mode);
        }

        let js_headers =
            web_sys::Headers::new().map_err(|e| DavError::Fetch(js_err_to_string(&e)))?;
        for (k, v) in headers.iter() {
            js_headers
                .append(k, v)
                .map_err(|e| DavError::Fetch(js_err_to_string(&e)))?;
        }
        init.set_headers(&js_headers);

        let request = web_sys::Request::new_with_str_and_init(url, &init)
            .map_err(|e| DavError::Fetch(js_err_to_string(&e)))?;

        let window = web_sys::window()
            .ok_or_else(|| DavError::Fetch("no `window` in this environment".into()))?;
        let resp_value = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| DavError::Fetch(js_err_to_string(&e)))?;
        let response: web_sys::Response = resp_value
            .dyn_into()
            .map_err(|e| DavError::Fetch(js_err_to_string(&e)))?;

        let text = match response.text() {
            Ok(promise) => JsFuture::from(promise)
                .await
                .ok()
                .and_then(|v| v.as_string())
                .unwrap_or_default(),
            Err(_) => String::new(),
        };

        Ok((
            HttpResponse {
                url: response.url(),
                status: response.status(),
                status_text: response.status_text(),
                ok: response.ok(),
                headers: headers_to_map(&response.headers()),
            },
            text,
        ))
    }
}
