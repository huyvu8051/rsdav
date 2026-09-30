use crate::consts::{DavNamespace, DavNamespaceShort};
use crate::error::DavError;
use crate::request::{dav_request, propfind, Ctx};
use crate::types::dav_types::{DavDepth, DavMethod, DavRequest, DavResponse};
use crate::types::function_overloads::SmartCollectionSyncDetailedResult;
use crate::types::models::{
    DavAccountType, DavCalendar, DavCollection, DavObject, FetchObjectsParams, ObjectMultiGetParams,
};
use crate::util::request_helpers::{ensure_trailing_slash, get_dav_attribute, url_matches};
use serde_json::{Map, Value};
use url::Url;

fn resolve_dav_href(href: &str, base_url: &str) -> String {
    crate::util::request_helpers::resolve_href(href, base_url)
}

fn href_has_extension(href: &str, extension: &str, base_url: &str) -> bool {
    let resolve = || -> Option<bool> {
        let base = Url::parse(&ensure_trailing_slash(base_url)).ok()?;
        let joined = base.join(href).ok()?;
        Some(joined.path().to_lowercase().ends_with(extension))
    };
    resolve().unwrap_or_else(|| {
        href.split(['?', '#'])
            .next()
            .unwrap_or("")
            .to_lowercase()
            .ends_with(extension)
    })
}

fn attrs_map(pairs: Vec<(&'static str, &'static str)>) -> Value {
    let mut m = Map::new();
    for (k, v) in pairs {
        m.insert(k.to_string(), Value::String(v.to_string()));
    }
    Value::Object(m)
}

pub async fn collection_query(
    url: &str,
    body: Map<String, Value>,
    depth: Option<DavDepth>,
    default_namespace: DavNamespaceShort,
    ctx: &Ctx,
) -> Result<Vec<DavResponse>, DavError> {
    let mut headers = ctx.headers();
    if let Some(d) = depth {
        headers.insert("depth".to_string(), d.as_str().to_string());
    }

    let query_results = dav_request(
        url,
        DavRequest {
            method: Some(DavMethod::Report),
            headers,
            namespace: Some(default_namespace),
            body: Some(Value::Object(body.clone())),
            attributes: None,
        },
        true,
        true,
        &ctx.fetch_options,
        &ctx.client(),
    )
    .await?;

    if default_namespace == DavNamespaceShort::Caldav && body.contains_key("calendar-query") {
        if let [only] = query_results.as_slice() {
            let empty_props = only
                .props
                .as_ref()
                .and_then(|p| p.as_object())
                .map(|m| m.is_empty())
                .unwrap_or(true);
            let raw_has_no_propstat = only
                .raw
                .as_ref()
                .and_then(|r| r.get("multistatus"))
                .and_then(|m| m.get("response"))
                .and_then(|r| r.get("propstat"))
                .is_none();
            if only.status == 404
                && url_matches(Some(url), only.href.as_deref(), Some(url))
                && only.error.is_none()
                && empty_props
                && raw_has_no_propstat
            {
                return Ok(vec![]);
            }
        }
    }

    if let Some(err) = query_results
        .iter()
        .find(|r| !r.ok || r.status >= 400)
    {
        return Err(DavError::msg(format!(
            "Collection query failed: {} {}. {}",
            err.status,
            err.status_text,
            err.raw
                .as_ref()
                .map(|r| format!("Raw response: {r}"))
                .unwrap_or_default()
        )));
    }

    if let [only] = query_results.as_slice() {
        let no_response = only
            .raw
            .as_ref()
            .and_then(|r| r.get("multistatus"))
            .map(|m| m.get("response").is_none())
            .unwrap_or(true);
        if no_response && only.status < 300 {
            return Ok(vec![]);
        }
    }

    Ok(query_results)
}

pub async fn make_collection(
    url: &str,
    props: Option<Value>,
    depth: Option<DavDepth>,
    ctx: &Ctx,
) -> Result<Vec<DavResponse>, DavError> {
    let mut headers = ctx.headers();
    if let Some(d) = depth {
        headers.insert("depth".to_string(), d.as_str().to_string());
    }

    let body = props.map(|p| {
        let mut mkcol = Map::new();
        mkcol.insert(
            "_attributes".to_string(),
            attrs_map(get_dav_attribute(&[
                DavNamespace::Dav,
                DavNamespace::Caldav,
                DavNamespace::Carddav,
                DavNamespace::CalendarServer,
                DavNamespace::CaldavApple,
            ])),
        );
        let mut set = Map::new();
        set.insert("prop".to_string(), p);
        mkcol.insert("set".to_string(), Value::Object(set));
        let mut root = Map::new();
        root.insert("mkcol".to_string(), Value::Object(mkcol));
        Value::Object(root)
    });

    dav_request(
        url,
        DavRequest {
            method: Some(DavMethod::Mkcol),
            headers,
            namespace: Some(DavNamespaceShort::Dav),
            body,
            attributes: None,
        },
        true,
        true,
        &ctx.fetch_options,
        &ctx.client(),
    )
    .await
}

pub async fn supported_report_set(collection_url: &str, ctx: &Ctx) -> Result<Vec<String>, DavError> {
    let mut prop = Map::new();
    prop.insert("d:supported-report-set".to_string(), Value::Object(Map::new()));
    let res = propfind(collection_url, Value::Object(prop), Some(DavDepth::Zero), ctx).await?;

    let supported_report = res
        .first()
        .and_then(|r| r.props.as_ref())
        .and_then(|p| p.get("supportedReportSet"))
        .and_then(|s| s.get("supportedReport"));
    let Some(supported_report) = supported_report else {
        return Ok(vec![]);
    };
    let reports: Vec<&Value> = match supported_report {
        Value::Array(items) => items.iter().collect(),
        other => vec![other],
    };
    Ok(reports
        .into_iter()
        .filter_map(|sr| sr.get("report"))
        .filter_map(|r| r.as_object())
        .filter_map(|m| m.keys().next())
        .filter(|s| !s.is_empty())
        .cloned()
        .collect())
}

pub async fn is_collection_dirty(
    collection_url: &str,
    collection_ctag: Option<&str>,
    ctx: &Ctx,
) -> Result<(bool, Option<String>), DavError> {
    let mut prop = Map::new();
    prop.insert("cs:getctag".to_string(), Value::Object(Map::new()));
    let responses = propfind(collection_url, Value::Object(prop), Some(DavDepth::Zero), ctx).await?;

    let res = responses
        .iter()
        .find(|r| url_matches(Some(collection_url), r.href.as_deref(), Some(collection_url)))
        .ok_or_else(|| DavError::msg("Collection does not exist on server"))?;
    if !res.ok {
        return Err(DavError::msg(format!(
            "Collection status check failed: {} {}",
            res.status, res.status_text
        )));
    }
    let remote_ctag = res
        .props
        .as_ref()
        .and_then(|p| p.get("getctag"))
        .and_then(|v| v.as_str().map(String::from).or_else(|| Some(v.to_string())));

    let is_dirty = collection_ctag.is_none()
        || remote_ctag.is_none()
        || collection_ctag != remote_ctag.as_deref();
    Ok((is_dirty, remote_ctag))
}

pub async fn sync_collection(
    url: &str,
    props: Value,
    sync_level: Option<i64>,
    sync_token: Option<&str>,
    ctx: &Ctx,
) -> Result<Vec<DavResponse>, DavError> {
    let mut body = Map::new();
    body.insert(
        "_attributes".to_string(),
        attrs_map(get_dav_attribute(&[
            DavNamespace::Caldav,
            DavNamespace::Carddav,
            DavNamespace::Dav,
        ])),
    );
    if let Some(l) = sync_level {
        body.insert("sync-level".to_string(), Value::Number(l.into()));
    }
    if let Some(t) = sync_token {
        body.insert("sync-token".to_string(), Value::String(t.to_string()));
    }
    body.insert("d:prop".to_string(), props);

    let mut root = Map::new();
    root.insert("sync-collection".to_string(), Value::Object(body));

    dav_request(
        url,
        DavRequest {
            method: Some(DavMethod::Report),
            headers: ctx.headers(),
            namespace: Some(DavNamespaceShort::Dav),
            body: Some(Value::Object(root)),
            attributes: None,
        },
        true,
        true,
        &ctx.fetch_options,
        &ctx.client(),
    )
    .await
}

pub trait DavCollectionLike: Clone {
    fn url(&self) -> &str;
    fn reports(&self) -> &[String];
    fn sync_token(&self) -> Option<&str>;
    fn ctag(&self) -> Option<&str>;
    fn objects(&self) -> &[DavObject];
    fn fetch_objects(&self) -> Option<&crate::types::models::FetchObjectsFn>;
    fn object_multi_get(&self) -> Option<&crate::types::models::ObjectMultiGetFn>;
    fn with_sync_result(&self, objects: Vec<DavObject>, ctag: Option<String>, sync_token: Option<String>) -> Self;
}

macro_rules! impl_collection_like {
    ($ty:ty) => {
        impl DavCollectionLike for $ty {
            fn url(&self) -> &str {
                &self.url
            }
            fn reports(&self) -> &[String] {
                self.reports.as_deref().unwrap_or(&[])
            }
            fn sync_token(&self) -> Option<&str> {
                self.sync_token.as_deref()
            }
            fn ctag(&self) -> Option<&str> {
                self.ctag.as_deref()
            }
            fn objects(&self) -> &[DavObject] {
                self.objects.as_deref().unwrap_or(&[])
            }
            fn fetch_objects(&self) -> Option<&crate::types::models::FetchObjectsFn> {
                self.fetch_objects.as_ref()
            }
            fn object_multi_get(&self) -> Option<&crate::types::models::ObjectMultiGetFn> {
                self.object_multi_get.as_ref()
            }
            fn with_sync_result(
                &self,
                objects: Vec<DavObject>,
                ctag: Option<String>,
                sync_token: Option<String>,
            ) -> Self {
                let mut next = self.clone();
                next.objects = Some(objects);
                if ctag.is_some() {
                    next.ctag = ctag;
                }
                if sync_token.is_some() {
                    next.sync_token = sync_token;
                }
                next
            }
        }
    };
}

impl_collection_like!(DavCalendar);
impl_collection_like!(DavCollection);

fn url_matches_within(local: &str, remote: &str, base: &str) -> bool {
    url_matches(Some(local), Some(remote), Some(base))
}

async fn smart_collection_sync_impl<T: DavCollectionLike>(
    collection: &T,
    method: Option<&str>,
    account_type: DavAccountType,
    ctx: &Ctx,
) -> Result<(T, SmartCollectionSyncDetailedResult<T>), DavError> {

    let sync_method = method
        .map(String::from)
        .unwrap_or_else(|| {
            if collection.reports().iter().any(|r| r == "syncCollection") {
                "webdav".to_string()
            } else {
                "basic".to_string()
            }
        });

    if sync_method == "webdav" {
        let is_caldav = matches!(account_type, DavAccountType::Caldav);
        let mut props = Map::new();
        props.insert("d:getetag".to_string(), Value::Object(Map::new()));
        let data_key = if is_caldav {
            "c:calendar-data"
        } else {
            "card:address-data"
        };
        props.insert(data_key.to_string(), Value::Object(Map::new()));
        props.insert("d:displayname".to_string(), Value::Object(Map::new()));

        let result = sync_collection(
            collection.url(),
            Value::Object(props),
            Some(1),
            collection.sync_token(),
            ctx,
        )
        .await?;

        let ext = if is_caldav { ".ics" } else { ".vcf" };
        let is_object_response = |r: &DavResponse| {
            r.href
                .as_deref()
                .map(|h| href_has_extension(h, ext, collection.url()))
                .unwrap_or(false)
        };

        if let Some(err) = result
            .iter()
            .find(|r| (!r.ok || r.status >= 400) && !(r.status == 404 && is_object_response(r)))
        {
            return Err(DavError::msg(format!(
                "Collection sync failed: {} {}",
                err.status, err.status_text
            )));
        }

        let object_responses: Vec<&DavResponse> = result.iter().filter(|r| is_object_response(r)).collect();
        let changed_object_urls: Vec<String> = object_responses
            .iter()
            .filter(|r| r.status != 404)
            .filter_map(|r| r.href.clone())
            .collect();
        let deleted_object_urls: Vec<String> = object_responses
            .iter()
            .filter(|r| r.status == 404)
            .filter_map(|r| r.href.clone())
            .collect();

        if !changed_object_urls.is_empty() && collection.object_multi_get().is_none() {
            return Err(DavError::msg(
                "collection.objectMultiGet is required for webdav sync changes",
            ));
        }

        let multi_get_response = if !changed_object_urls.is_empty() {
            let mut props = Map::new();
            props.insert("d:getetag".to_string(), Value::Object(Map::new()));
            props.insert(data_key.to_string(), Value::Object(Map::new()));
            let f = collection.object_multi_get().unwrap();
            f(ObjectMultiGetParams {
                url: collection.url().to_string(),
                props: Value::Object(props),
                object_urls: changed_object_urls,
                filters: None,
                timezone: None,
                depth: DavDepth::One,
                headers: Some(ctx.headers()),
                fetch_options: Some(ctx.fetch_options.clone()),
            })
            .await?
        } else {
            vec![]
        };

        if let Some(err) = multi_get_response.iter().find(|r| !r.ok || r.status >= 400) {
            return Err(DavError::msg(format!(
                "Collection sync multi-get failed: {} {}",
                err.status, err.status_text
            )));
        }

        let data_key_camel = crate::util::camel_case::camel_case(
            data_key.rsplit(':').next().unwrap_or(data_key),
        );
        let remote_objects: Vec<DavObject> = multi_get_response
            .iter()
            .map(|res| {
                let etag = res.props.as_ref().and_then(|p| p.get("getetag")).map(value_to_string);
                let data = res
                    .props
                    .as_ref()
                    .and_then(|p| p.get(&data_key_camel))
                    .map(|v| {
                        v.get("_cdata")
                            .and_then(|c| c.as_str())
                            .map(String::from)
                            .unwrap_or_else(|| value_to_string(v))
                    });
                DavObject {
                    url: resolve_dav_href(res.href.as_deref().unwrap_or(""), collection.url()),
                    etag,
                    data,
                }
            })
            .collect();

        let local_objects = collection.objects().to_vec();
        let created: Vec<DavObject> = remote_objects
            .iter()
            .filter(|o| local_objects.iter().all(|lo| !url_matches_within(&lo.url, &o.url, collection.url())))
            .cloned()
            .collect();
        let updated: Vec<DavObject> = local_objects
            .iter()
            .filter_map(|curr| {
                remote_objects
                    .iter()
                    .find(|ro| url_matches_within(&ro.url, &curr.url, collection.url()))
                    .filter(|found| found.etag.is_some() && found.etag != curr.etag)
                    .cloned()
            })
            .collect();
        let deleted: Vec<DavObject> = deleted_object_urls
            .iter()
            .map(|o| DavObject {
                url: resolve_dav_href(o, collection.url()),
                etag: Some(String::new()),
                data: None,
            })
            .collect();
        let unchanged: Vec<DavObject> = local_objects
            .iter()
            .filter(|lo| {
                deleted.iter().all(|d| !url_matches_within(&lo.url, &d.url, collection.url()))
                    && updated.iter().all(|u| !url_matches_within(&lo.url, &u.url, collection.url()))
            })
            .cloned()
            .collect();

        let sync_token = result
            .first()
            .and_then(|r| r.raw.as_ref())
            .and_then(|r| r.get("multistatus"))
            .and_then(|m| m.get("syncToken"))
            .map(value_to_string)
            .or_else(|| collection.sync_token().map(String::from));

        let mut all_objects = unchanged.clone();
        all_objects.extend(created.iter().cloned());
        all_objects.extend(updated.iter().cloned());
        let plain = collection.with_sync_result(all_objects, collection.ctag().map(String::from), sync_token.clone());

        let detailed = SmartCollectionSyncDetailedResult {
            base: collection.with_sync_result(vec![], collection.ctag().map(String::from), sync_token),
            created,
            updated,
            deleted,
        };
        return Ok((plain, detailed));
    }

    if sync_method == "basic" {
        let (is_dirty, new_ctag) = is_collection_dirty(collection.url(), collection.ctag(), ctx).await?;

        if !is_dirty {
            let plain = collection.clone();
            let detailed = SmartCollectionSyncDetailedResult {
                base: collection.clone(),
                created: vec![],
                updated: vec![],
                deleted: vec![],
            };
            return Ok((plain, detailed));
        }

        let local_objects = collection.objects().to_vec();
        let fetch_objects = collection
            .fetch_objects()
            .ok_or_else(|| DavError::msg("collection.fetchObjects is required for basic sync changes"))?;

        let remote_objects = fetch_objects(FetchObjectsParams {
            collection: DavCalendar {
                url: collection.url().to_string(),
                ..Default::default()
            },
            headers: Some(ctx.headers()),
            fetch_options: Some(ctx.fetch_options.clone()),
        })
        .await?;

        let created: Vec<DavObject> = remote_objects
            .iter()
            .filter(|ro| local_objects.iter().all(|lo| !url_matches_within(&lo.url, &ro.url, collection.url())))
            .cloned()
            .collect();
        let updated: Vec<DavObject> = local_objects
            .iter()
            .filter_map(|curr| {
                remote_objects
                    .iter()
                    .find(|ro| url_matches_within(&ro.url, &curr.url, collection.url()))
                    .filter(|found| found.etag.is_some() && found.etag != curr.etag)
                    .cloned()
            })
            .collect();
        let deleted: Vec<DavObject> = local_objects
            .iter()
            .filter(|cal| remote_objects.iter().all(|ro| !url_matches_within(&ro.url, &cal.url, collection.url())))
            .cloned()
            .collect();
        let unchanged: Vec<DavObject> = local_objects
            .iter()
            .filter(|lo| {
                remote_objects
                    .iter()
                    .any(|ro| url_matches_within(&lo.url, &ro.url, collection.url()) && ro.etag == lo.etag)
            })
            .cloned()
            .collect();

        let mut all_objects = unchanged;
        all_objects.extend(created.iter().cloned());
        all_objects.extend(updated.iter().cloned());
        let plain = collection.with_sync_result(all_objects, new_ctag.clone(), collection.sync_token().map(String::from));
        let detailed = SmartCollectionSyncDetailedResult {
            base: collection.with_sync_result(vec![], new_ctag, collection.sync_token().map(String::from)),
            created,
            updated,
            deleted,
        };
        return Ok((plain, detailed));
    }

    let plain = collection.clone();
    let detailed = SmartCollectionSyncDetailedResult {
        base: collection.clone(),
        created: vec![],
        updated: vec![],
        deleted: vec![],
    };
    Ok((plain, detailed))
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

pub async fn smart_collection_sync<T: DavCollectionLike>(
    collection: &T,
    method: Option<&str>,
    account_type: DavAccountType,
    ctx: &Ctx,
) -> Result<T, DavError> {
    let (plain, _) = smart_collection_sync_impl(collection, method, account_type, ctx).await?;
    Ok(plain)
}

pub async fn smart_collection_sync_detailed<T: DavCollectionLike>(
    collection: &T,
    method: Option<&str>,
    account_type: DavAccountType,
    ctx: &Ctx,
) -> Result<SmartCollectionSyncDetailedResult<T>, DavError> {
    let (_, detailed) = smart_collection_sync_impl(collection, method, account_type, ctx).await?;
    Ok(detailed)
}
