use crate::collection::{collection_query, supported_report_set};
use crate::consts::{DavNamespace, DavNamespaceShort};
use crate::error::DavError;
use crate::request::{create_object, delete_object, propfind, update_object, Ctx, HttpResult};
use crate::types::dav_types::{DavDepth, DavResponse};
use crate::types::models::{DavAccount, DavAddressBook, DavVCard};
use crate::util::request_helpers::{ensure_trailing_slash, get_dav_attribute, url_equals};
use serde_json::{Map, Value};
use url::Url;

fn ns_attrs(ns: &[DavNamespace]) -> Value {
    Value::Object(
        get_dav_attribute(ns)
            .into_iter()
            .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
            .collect(),
    )
}

pub async fn address_book_query(
    url: &str,
    props: Value,
    filters: Option<Value>,
    depth: Option<DavDepth>,
    ctx: &Ctx,
) -> Result<Vec<DavResponse>, DavError> {
    let mut body = Map::new();
    body.insert("_attributes".to_string(), ns_attrs(&[DavNamespace::Carddav, DavNamespace::Dav]));
    body.insert("d:prop".to_string(), props);
    body.insert(
        "filter".to_string(),
        filters.unwrap_or_else(|| {
            let mut prop_filter = Map::new();
            prop_filter.insert("_attributes".to_string(), {
                let mut a = Map::new();
                a.insert("name".to_string(), Value::String("FN".to_string()));
                Value::Object(a)
            });
            let mut m = Map::new();
            m.insert("prop-filter".to_string(), Value::Object(prop_filter));
            Value::Object(m)
        }),
    );
    let mut root = Map::new();
    root.insert("addressbook-query".to_string(), Value::Object(body));
    collection_query(url, root, depth, DavNamespaceShort::Carddav, ctx).await
}

pub async fn address_book_multi_get(
    url: &str,
    props: Value,
    object_urls: &[String],
    depth: DavDepth,
    ctx: &Ctx,
) -> Result<Vec<DavResponse>, DavError> {
    let mut body = Map::new();
    body.insert("_attributes".to_string(), ns_attrs(&[DavNamespace::Dav, DavNamespace::Carddav]));
    body.insert("d:prop".to_string(), props);
    body.insert(
        "d:href".to_string(),
        Value::Array(object_urls.iter().map(|u| Value::String(u.clone())).collect()),
    );
    let mut root = Map::new();
    root.insert("addressbook-multiget".to_string(), Value::Object(body));
    collection_query(url, root, Some(depth), DavNamespaceShort::Carddav, ctx).await
}

pub async fn fetch_address_books(
    account: &DavAccount,
    custom_props: Option<Value>,
    ctx: &Ctx,
) -> Result<Vec<DavAddressBook>, DavError> {
    let (Some(home_url), Some(root_url)) = (&account.home_url, &account.root_url) else {
        return Err(DavError::msg("account must have homeUrl,rootUrl before fetchAddressBooks"));
    };
    let props = custom_props.unwrap_or_else(|| {
        let mut m = Map::new();
        for key in ["d:displayname", "cs:getctag", "d:resourcetype", "d:sync-token"] {
            m.insert(key.to_string(), Value::Object(Map::new()));
        }
        Value::Object(m)
    });
    let res = propfind(home_url, props, Some(DavDepth::One), ctx).await?;

    let mut out = Vec::new();
    for r in res.iter() {
        let props = r.props.as_ref();
        let is_addressbook = props
            .and_then(|p| p.get("resourcetype"))
            .and_then(|rt| rt.as_object())
            .map(|m| m.contains_key("addressbook"))
            .unwrap_or(false);
        if !is_addressbook {
            continue;
        }
        let display_name = props
            .and_then(|p| p.get("displayname"))
            .map(|v| v.get("_cdata").and_then(|c| c.as_str()).map(String::from).unwrap_or_else(|| value_to_string(v)))
            .unwrap_or_default();
        let url = r
            .href
            .as_deref()
            .map(|h| crate::util::request_helpers::resolve_href(h, &ensure_trailing_slash(root_url)))
            .unwrap_or_default();
        let mut addr = DavAddressBook {
            url,
            ctag: props.and_then(|p| p.get("getctag")).map(value_to_string),
            display_name: Some(display_name),
            resourcetype: props.and_then(|p| p.get("resourcetype")).and_then(|v| v.as_object()).map(|m| m.keys().cloned().collect()),
            sync_token: props.and_then(|p| p.get("syncToken")).map(value_to_string),
            ..Default::default()
        };
        addr.reports = Some(supported_report_set(&addr.url, ctx).await?);
        out.push(addr);
    }
    Ok(out)
}

pub async fn fetch_vcards(
    address_book: &DavAddressBook,
    object_urls: Option<&[String]>,
    url_filter: Option<&dyn Fn(&str) -> bool>,
    use_multi_get: bool,
    ctx: &Ctx,
) -> Result<Vec<DavVCard>, DavError> {
    if address_book.url.is_empty() {
        return Err(DavError::msg("addressBook must have url before fetchVCards"));
    }
    let default_filter = |_: &str| true;
    let url_filter: &dyn Fn(&str) -> bool = url_filter.unwrap_or(&default_filter);
    let base = ensure_trailing_slash(&address_book.url);

    let vcard_urls: Vec<String> = if let Some(urls) = object_urls {
        urls.to_vec()
    } else {
        let mut prop = Map::new();
        prop.insert("d:getetag".to_string(), Value::Object(Map::new()));
        address_book_query(&address_book.url, Value::Object(prop), None, Some(DavDepth::One), ctx)
            .await?
            .into_iter()
            .filter_map(|r| r.href)
            .collect()
    }
    .into_iter()
    .filter(|u| !u.trim().is_empty())
    .map(|u| if u.starts_with("http") { u } else { Url::parse(&base).ok().and_then(|b| b.join(&u).ok()).map(|x| x.to_string()).unwrap_or(u) })
    .filter(|u| !url_equals(Some(u), Some(&address_book.url)))
    .filter(|u| url_filter(u))
    .map(|u| Url::parse(&u).map(|p| format!("{}{}", p.path(), p.query().map(|q| format!("?{q}")).unwrap_or_default())).unwrap_or(u))
    .collect();

    let mut vcard_results: Vec<DavResponse> = vec![];
    if !vcard_urls.is_empty() {
        let mut props = Map::new();
        props.insert("d:getetag".to_string(), Value::Object(Map::new()));
        props.insert("card:address-data".to_string(), Value::Object(Map::new()));
        if use_multi_get {
            vcard_results = address_book_multi_get(&address_book.url, Value::Object(props), &vcard_urls, DavDepth::One, ctx).await?;
        } else {
            vcard_results = address_book_query(&address_book.url, Value::Object(props), None, Some(DavDepth::One), ctx).await?;
        }
    }

    Ok(vcard_results
        .into_iter()
        .map(|res| {
            let href = res.href.clone().unwrap_or_default();
            let url = Url::parse(&base).ok().and_then(|b| b.join(&href).ok()).map(|u| u.to_string()).unwrap_or(href);
            let etag = res.props.as_ref().and_then(|p| p.get("getetag")).map(value_to_string);
            let data = res.props.as_ref().and_then(|p| p.get("addressData")).map(|v| {
                v.get("_cdata").and_then(|c| c.as_str()).map(String::from).unwrap_or_else(|| value_to_string(v))
            });
            DavVCard { url, etag, data }
        })
        .collect())
}

pub async fn create_vcard(address_book: &DavAddressBook, vcard_string: &str, filename: &str, ctx: &Ctx) -> Result<HttpResult, DavError> {
    let base = ensure_trailing_slash(&address_book.url);
    let url = Url::parse(&base).ok().and_then(|b| b.join(filename).ok()).map(|u| u.to_string()).unwrap_or_default();
    let mut headers = ctx.headers.clone();
    headers.insert("content-type".to_string(), "text/vcard; charset=utf-8".to_string());
    headers.insert("If-None-Match".to_string(), "*".to_string());
    let ctx2 = Ctx { headers, ..ctx.clone() };
    create_object(&url, vcard_string, &ctx2).await
}

pub async fn update_vcard(vcard: &DavVCard, ctx: &Ctx) -> Result<HttpResult, DavError> {
    let mut headers = ctx.headers.clone();
    headers.insert("content-type".to_string(), "text/vcard; charset=utf-8".to_string());
    let ctx2 = Ctx { headers, ..ctx.clone() };
    update_object(&vcard.url, vcard.data.as_deref().unwrap_or(""), vcard.etag.as_deref(), &ctx2).await
}

pub async fn delete_vcard(vcard: &DavVCard, ctx: &Ctx) -> Result<HttpResult, DavError> {
    delete_object(&vcard.url, vcard.etag.as_deref(), ctx).await
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
