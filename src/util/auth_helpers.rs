use crate::error::DavError;
use crate::types::dav_types::DavTokens;
use crate::types::models::DavCredentials;
use crate::util::fetch::{FetchClient, FetchOptions};
use crate::util::request_helpers::merge_headers;
use crate::util::type_helpers::{find_missing_field_names, has_fields};
use std::collections::HashMap;
use std::rc::Rc;

fn assert_latin1(s: &str) -> Result<(), DavError> {
    if s.chars().any(|c| (c as u32) > 0xff) {
        return Err(DavError::msg(
            "The string to be encoded contains characters outside of the Latin1 range.",
        ));
    }
    Ok(())
}

pub fn get_basic_auth_headers(
    credentials: &DavCredentials,
) -> Result<HashMap<String, String>, DavError> {
    let username = credentials.username.clone().unwrap_or_default();
    let password = credentials.password.clone().unwrap_or_default();
    let raw = format!("{username}:{password}");
    assert_latin1(&raw)?;
    let mut headers = HashMap::new();
    headers.insert(
        "authorization".to_string(),
        format!("Basic {}", base64::Engine::encode(&base64::engine::general_purpose::STANDARD, raw)),
    );
    Ok(headers)
}

pub fn get_bearer_auth_headers(credentials: &DavCredentials) -> HashMap<String, String> {
    let mut headers = HashMap::new();
    headers.insert(
        "authorization".to_string(),
        format!("Bearer {}", credentials.access_token.clone().unwrap_or_default()),
    );
    headers
}

fn form_urlencode(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub async fn fetch_oauth_tokens(
    credentials: &DavCredentials,
    fetch_options: Option<&FetchOptions>,
    fetch_override: Option<&Rc<dyn FetchClient>>,
    default_client: &Rc<dyn FetchClient>,
) -> Result<DavTokens, DavError> {
    let required: [(&str, bool); 5] = [
        ("authorizationCode", credentials.authorization_code.is_some()),
        ("redirectUrl", credentials.redirect_url.is_some()),
        ("clientId", credentials.client_id.is_some()),
        ("clientSecret", credentials.client_secret.is_some()),
        ("tokenUrl", credentials.token_url.is_some()),
    ];
    if !has_fields(&required) {
        return Err(DavError::msg(format!(
            "Oauth credentials missing: {}",
            find_missing_field_names(&required)
        )));
    }

    let body = form_urlencode(&[
        ("grant_type", "authorization_code"),
        ("code", credentials.authorization_code.as_deref().unwrap_or("")),
        ("redirect_uri", credentials.redirect_url.as_deref().unwrap_or("")),
        ("client_id", credentials.client_id.as_deref().unwrap_or("")),
        ("client_secret", credentials.client_secret.as_deref().unwrap_or("")),
    ]);

    let mut headers = HashMap::new();
    headers.insert(
        "content-type".to_string(),
        "application/x-www-form-urlencoded".to_string(),
    );
    let empty_opts = FetchOptions::default();
    let opts = fetch_options.unwrap_or(&empty_opts);
    let merged_headers = merge_headers(&[&headers, &opts.headers]);

    let client = fetch_override.unwrap_or(default_client);
    let (resp, text) = client
        .send(
            credentials.token_url.as_deref().unwrap_or(""),
            "POST",
            &merged_headers,
            Some(&body),
            opts,
        )
        .await?;

    if resp.ok {
        let tokens: DavTokens =
            serde_json::from_str(&text).map_err(|e| DavError::msg(e.to_string()))?;
        Ok(tokens)
    } else {
        Ok(DavTokens::default())
    }
}

pub async fn refresh_access_token(
    credentials: &DavCredentials,
    fetch_options: Option<&FetchOptions>,
    fetch_override: Option<&Rc<dyn FetchClient>>,
    default_client: &Rc<dyn FetchClient>,
) -> Result<DavTokens, DavError> {
    let required: [(&str, bool); 4] = [
        ("refreshToken", credentials.refresh_token.is_some()),
        ("clientId", credentials.client_id.is_some()),
        ("clientSecret", credentials.client_secret.is_some()),
        ("tokenUrl", credentials.token_url.is_some()),
    ];
    if !has_fields(&required) {
        return Err(DavError::msg(format!(
            "Oauth credentials missing: {}",
            find_missing_field_names(&required)
        )));
    }

    let body = form_urlencode(&[
        ("client_id", credentials.client_id.as_deref().unwrap_or("")),
        ("client_secret", credentials.client_secret.as_deref().unwrap_or("")),
        ("refresh_token", credentials.refresh_token.as_deref().unwrap_or("")),
        ("grant_type", "refresh_token"),
    ]);

    let mut headers = HashMap::new();
    headers.insert(
        "Content-Type".to_string(),
        "application/x-www-form-urlencoded".to_string(),
    );
    let empty_opts = FetchOptions::default();
    let opts = fetch_options.unwrap_or(&empty_opts);
    let merged_headers = merge_headers(&[&headers, &opts.headers]);

    let client = fetch_override.unwrap_or(default_client);
    let (resp, text) = client
        .send(
            credentials.token_url.as_deref().unwrap_or(""),
            "POST",
            &merged_headers,
            Some(&body),
            opts,
        )
        .await?;

    if resp.ok {
        let tokens: DavTokens =
            serde_json::from_str(&text).map_err(|e| DavError::msg(e.to_string()))?;
        Ok(tokens)
    } else {
        Ok(DavTokens::default())
    }
}

pub struct OauthHeadersResult {
    pub tokens: DavTokens,
    pub headers: HashMap<String, String>,
}

pub async fn get_oauth_headers(
    credentials: &mut DavCredentials,
    fetch_options: Option<&FetchOptions>,
    fetch_override: Option<&Rc<dyn FetchClient>>,
    default_client: &Rc<dyn FetchClient>,
) -> Result<OauthHeadersResult, DavError> {
    let now_ms = js_sys::Date::now() as i64;
    let mut did_refresh = false;
    let tokens = if credentials.refresh_token.is_none() {
        did_refresh = true;
        fetch_oauth_tokens(credentials, fetch_options, fetch_override, default_client).await?
    } else if (credentials.refresh_token.is_some() && credentials.access_token.is_none())
        || now_ms > credentials.expiration.unwrap_or(0)
    {
        did_refresh = true;
        refresh_access_token(credentials, fetch_options, fetch_override, default_client).await?
    } else {
        DavTokens {
            access_token: credentials.access_token.clone(),
            refresh_token: credentials.refresh_token.clone(),
            ..Default::default()
        }
    };

    if did_refresh {
        if let Some(at) = &tokens.access_token {
            credentials.access_token = Some(at.clone());
        }
        if let Some(rt) = &tokens.refresh_token {
            credentials.refresh_token = Some(rt.clone());
        }
        if let Some(expires_in) = tokens.expires_in {
            credentials.expiration = Some(now_ms + expires_in * 1000);
        }
    }

    let mut headers = HashMap::new();
    if let Some(at) = &tokens.access_token {
        headers.insert("authorization".to_string(), format!("Bearer {at}"));
    }
    Ok(OauthHeadersResult { tokens, headers })
}
