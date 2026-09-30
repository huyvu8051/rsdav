use crate::calendar::{fetch_calendar_objects, fetch_calendars};
use crate::address_book::{fetch_address_books, fetch_vcards};
use crate::consts::DavNamespaceShort;
use crate::error::DavError;
use crate::request::{propfind, Ctx};
use crate::types::models::{DavAccount, DavAccountType};
use crate::util::request_helpers::{ensure_trailing_slash, merge_headers, url_matches};
use serde_json::{Map, Value};
use url::Url;

fn get_candidate_root_urls(server_url: &str, discovered_root_url: &str) -> Vec<String> {
    let root = Url::parse(server_url).ok().map(|u| {
        let mut u = u.clone();
        u.set_path("/");
        u.set_query(None);
        u.set_fragment(None);
        u.to_string()
    });
    let mut candidates = vec![discovered_root_url.to_string(), server_url.to_string()];
    if let Some(r) = root {
        candidates.push(r);
    }
    let mut seen = std::collections::HashSet::new();
    candidates.retain(|c| seen.insert(c.clone()));
    candidates
}

fn extract_redirect(status: u16, headers: &std::collections::HashMap<String, String>, endpoint: &Url, well_known_uri: &Url) -> Option<String> {
    if !(300..400).contains(&status) {
        return None;
    }
    let location = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("location")).map(|(_, v)| v.clone())?;
    if location.is_empty() {
        return None;
    }
    let has_explicit_scheme = location.split(':').next().map(|s| !s.is_empty() && s.chars().next().unwrap().is_ascii_alphabetic() && s.chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))).unwrap_or(false)
        && location.contains(':');
    let mut service_url = Url::parse(&location).ok().or_else(|| endpoint.join(&location).ok())?;

    if service_url.host_str() == well_known_uri.host_str() {
        if let Some(port) = well_known_uri.port() {
            if service_url.port().is_none() {
                let _ = service_url.set_port(Some(port));
            }
        }
    }
    if !has_explicit_scheme {
        let _ = service_url.set_scheme(endpoint.scheme());
    }
    Some(service_url.to_string())
}

pub async fn service_discovery(account: &DavAccount, ctx: &Ctx) -> Result<String, DavError> {
    let endpoint = Url::parse(&account.server_url).map_err(|e| DavError::msg(e.to_string()))?;
    let account_type_str = match account.account_type {
        DavAccountType::Caldav => "caldav",
        DavAccountType::Carddav => "carddav",
    };
    let mut well_known = endpoint.join(&format!("/.well-known/{account_type_str}")).map_err(|e| DavError::msg(e.to_string()))?;
    let _ = well_known.set_scheme(endpoint.scheme());

    let mut manual_opts = ctx.fetch_options.clone();
    manual_opts.redirect = Some(web_sys::RequestRedirect::Manual);

    let mut content_type = std::collections::HashMap::new();
    content_type.insert("Content-Type".to_string(), "text/xml;charset=UTF-8".to_string());
    let merged = merge_headers(&[&ctx.headers(), &content_type, &ctx.fetch_options.headers]);
    let body = "<?xml version=\"1.0\" encoding=\"utf-8\" ?>\n<d:propfind xmlns:d=\"DAV:\">\n  <d:prop>\n    <d:resourcetype/>\n  </d:prop>\n</d:propfind>";
    if let Ok((resp, _text)) = ctx.client().send(well_known.as_str(), "PROPFIND", &merged, Some(body), &manual_opts).await {
        if let Some(redirect) = extract_redirect(resp.status, &resp.headers, &endpoint, &well_known) {
            return Ok(redirect);
        }
    }

    let get_headers = merge_headers(&[&ctx.headers(), &ctx.fetch_options.headers]);
    if let Ok((resp, _text)) = ctx.client().send(well_known.as_str(), "GET", &get_headers, None, &manual_opts).await {
        if let Some(redirect) = extract_redirect(resp.status, &resp.headers, &endpoint, &well_known) {
            return Ok(redirect);
        }
    }

    Ok(endpoint.to_string())
}

fn extract_href(raw: Option<&Value>) -> Option<String> {
    match raw? {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Array(items) => items.iter().find_map(|v| extract_href(Some(v))),
        Value::Object(m) => {
            if let Some(Value::String(s)) = m.get("_cdata") {
                if !s.trim().is_empty() {
                    return Some(s.trim().to_string());
                }
            }
            if let Some(Value::String(s)) = m.get("_text") {
                if !s.trim().is_empty() {
                    return Some(s.trim().to_string());
                }
            }
            None
        }
        _ => None,
    }
}

pub async fn fetch_principal_url(account: &DavAccount, ctx: &Ctx) -> Result<String, DavError> {
    let root_url = account.root_url.as_deref().ok_or_else(|| DavError::msg("account must have rootUrl before fetchPrincipalUrl"))?;
    let mut prop = Map::new();
    prop.insert("d:current-user-principal".to_string(), Value::Object(Map::new()));
    let responses = propfind(root_url, Value::Object(prop), Some(crate::types::dav_types::DavDepth::Zero), ctx).await?;
    let response = responses.first();
    let ok = response.map(|r| r.ok).unwrap_or(false);
    if !ok {
        if response.map(|r| r.status) == Some(401) {
            return Err(DavError::msg(format!("Invalid credentials: PROPFIND {root_url} returned 401 Unauthorized")));
        }
        return Err(DavError::msg("cannot find principalUrl"));
    }
    let principal_href = response
        .and_then(|r| r.props.as_ref())
        .and_then(|p| p.get("currentUserPrincipal"))
        .and_then(|c| c.get("href"))
        .and_then(|h| extract_href(Some(h)))
        .ok_or_else(|| DavError::msg("cannot find principalUrl"))?;
    Ok(crate::util::request_helpers::resolve_href(&principal_href, &ensure_trailing_slash(root_url)))
}

pub async fn fetch_home_url(account: &DavAccount, ctx: &Ctx) -> Result<String, DavError> {
    let (Some(principal_url), Some(root_url)) = (&account.principal_url, &account.root_url) else {
        return Err(DavError::msg("account must have principalUrl,rootUrl before fetchHomeUrl"));
    };
    let mut prop = Map::new();
    let key = if matches!(account.account_type, DavAccountType::Caldav) {
        "c:calendar-home-set"
    } else {
        "card:addressbook-home-set"
    };
    prop.insert(key.to_string(), Value::Object(Map::new()));
    let responses = propfind(principal_url, Value::Object(prop), Some(crate::types::dav_types::DavDepth::Zero), ctx).await?;
    let matched = responses
        .iter()
        .find(|r| url_matches(Some(principal_url), r.href.as_deref(), Some(root_url)))
        .filter(|r| r.ok)
        .ok_or_else(|| DavError::msg("cannot find homeUrl"))?;
    let field = if matches!(account.account_type, DavAccountType::Caldav) {
        "calendarHomeSet"
    } else {
        "addressbookHomeSet"
    };
    let home_href = matched
        .props
        .as_ref()
        .and_then(|p| p.get(field))
        .and_then(|h| h.get("href"))
        .and_then(|h| extract_href(Some(h)))
        .ok_or_else(|| DavError::msg("cannot find homeUrl"))?;
    Ok(crate::util::request_helpers::resolve_href(&home_href, &ensure_trailing_slash(root_url)))
}

pub struct CreateAccountOptions {
    pub load_collections: bool,
    pub load_objects: bool,
}

impl Default for CreateAccountOptions {
    fn default() -> Self {
        CreateAccountOptions { load_collections: false, load_objects: false }
    }
}

pub async fn create_account(account: &DavAccount, opts: CreateAccountOptions, ctx: &Ctx) -> Result<DavAccount, DavError> {
    let mut new_account = account.clone();

    let discovered_root_url = match &account.root_url {
        Some(r) => r.clone(),
        None => service_discovery(account, ctx).await?,
    };

    if let Some(r) = &account.root_url {
        new_account.root_url = Some(r.clone());
    } else if account.principal_url.is_some() {
        new_account.root_url = Some(discovered_root_url.clone());
    } else {
        let candidates = get_candidate_root_urls(&account.server_url, &discovered_root_url);
        let mut found: Option<(String, String)> = None;
        let mut last_err: Option<DavError> = None;
        for root_url in candidates {
            let probe = DavAccount { root_url: Some(root_url.clone()), ..new_account.clone() };
            match fetch_principal_url(&probe, ctx).await {
                Ok(principal_url) => {
                    found = Some((root_url, principal_url));
                    break;
                }
                Err(e) => last_err = Some(e),
            }
        }
        let (root_url, principal_url) = found.ok_or_else(|| last_err.unwrap_or_else(|| DavError::msg("cannot find principalUrl")))?;
        new_account.root_url = Some(root_url);
        new_account.principal_url = Some(principal_url);
    }

    new_account.principal_url = match (&account.principal_url, &new_account.principal_url) {
        (Some(p), _) => Some(p.clone()),
        (None, Some(p)) => Some(p.clone()),
        (None, None) => Some(fetch_principal_url(&new_account, ctx).await?),
    };
    new_account.home_url = match &account.home_url {
        Some(h) => Some(h.clone()),
        None => Some(fetch_home_url(&new_account, ctx).await?),
    };

    if opts.load_collections || opts.load_objects {
        match account.account_type {
            DavAccountType::Caldav => {
                new_account.calendars = Some(fetch_calendars(&new_account, None, None, ctx).await?);
            }
            DavAccountType::Carddav => {
                new_account.address_books = Some(fetch_address_books(&new_account, None, ctx).await?);
            }
        }
    }
    if opts.load_objects {
        match account.account_type {
            DavAccountType::Caldav => {
                if let Some(cals) = new_account.calendars.clone() {
                    let mut out = Vec::new();
                    for mut cal in cals {
                        let objects = fetch_calendar_objects(&cal, None, None, None, false, None, true, ctx).await?;
                        cal.objects = Some(objects);
                        out.push(cal);
                    }
                    new_account.calendars = Some(out);
                }
            }
            DavAccountType::Carddav => {
                if let Some(books) = new_account.address_books.clone() {
                    let mut out = Vec::new();
                    for mut ab in books {
                        let objects = fetch_vcards(&ab, None, None, true, ctx).await?;
                        ab.objects = Some(objects);
                        out.push(ab);
                    }
                    new_account.address_books = Some(out);
                }
            }
        }
    }

    let _ = DavNamespaceShort::Dav;
    Ok(new_account)
}
