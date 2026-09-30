use crate::collection::{collection_query, smart_collection_sync, supported_report_set};
use crate::consts::{DavNamespace, DavNamespaceShort, IcalObjects};
use crate::error::DavError;
use crate::request::{create_object, dav_request, delete_object, propfind, update_object, Ctx, HttpResult};
use crate::types::dav_types::{DavDepth, DavMethod, DavRequest, DavResponse};
use crate::types::function_overloads::SyncCalendarsDetailedResult;
use crate::types::models::{DavAccount, DavCalendar, DavCalendarObject};
use crate::util::request_helpers::{ensure_trailing_slash, get_dav_attribute, url_matches};
use serde_json::{Map, Value};
use url::Url;
use wasm_bindgen::JsValue;

fn matches_iso8601(s: &str) -> bool {
    let b = s.as_bytes();
    let n = b.len();
    let digits = |from: usize, to: usize| to <= n && b[from..to].iter().all(u8::is_ascii_digit);
    if !digits(0, 4.min(n)) || n < 4 {
        return false;
    }
    let mut i = 4;
    if i == n {
        return true;
    }
    if b[i] != b'-' || !digits(i + 1, i + 3) {
        return false;
    }
    i += 3;
    if i == n {
        return true;
    }
    if b[i] != b'-' || !digits(i + 1, i + 3) {
        return false;
    }
    i += 3;
    if i == n {
        return true;
    }
    if !(b[i] == b'T' || b[i] == b't') {
        return false;
    }
    i += 1;
    if !digits(i, i + 2) || i + 2 >= n || b[i + 2] != b':' || !digits(i + 3, i + 5) {
        return false;
    }
    i += 5;
    if i == n {
        return true;
    }
    if i < n && b[i] == b':' {
        if !digits(i + 1, i + 3) {
            return false;
        }
        i += 3;
    }
    if i == n {
        return true;
    }
    if i < n && b[i] == b'.' {
        let start = i + 1;
        let mut j = start;
        while j < n && b[j].is_ascii_digit() {
            j += 1;
        }
        if j == start {
            return false;
        }
        i = j;
    }
    if i == n {
        return true;
    }
    if i < n {
        if b[i] == b'Z' || b[i] == b'z' {
            i += 1;
        } else if b[i] == b'+' || b[i] == b'-' {
            if i + 6 > n || !digits(i + 1, i + 3) || b[i + 3] != b':' || !digits(i + 4, i + 6) {
                return false;
            }
            i += 6;
        } else {
            return false;
        }
    }
    i == n
}

fn js_date_time(iso: &str) -> f64 {
    js_sys::Date::new(&JsValue::from_str(iso)).get_time()
}

fn validate_time_range(start: &str, end: &str) -> Result<(), DavError> {
    if !(matches_iso8601(start) && matches_iso8601(end)) {
        return Err(DavError::msg("invalid timeRange format, not in ISO8601"));
    }
    let start_ms = js_date_time(start);
    let end_ms = js_date_time(end);
    if start_ms.is_nan() || end_ms.is_nan() {
        return Err(DavError::msg("invalid timeRange: start or end is not a valid date"));
    }
    if start_ms >= end_ms {
        return Err(DavError::msg("invalid timeRange: start must be before end"));
    }
    Ok(())
}

fn format_caldav_utc(iso: &str) -> String {
    let full: String = js_sys::Date::new(&JsValue::from_str(iso)).to_iso_string().into();
    let sliced: String = full.chars().take(19).collect();
    let cleaned: String = sliced.chars().filter(|c| *c != '-' && *c != ':' && *c != '.').collect();
    format!("{cleaned}Z")
}

fn extract_component_names(comp_set: Option<&Value>) -> Vec<String> {
    let items: Vec<&Value> = match comp_set {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(other) => vec![other],
        None => vec![],
    };
    items
        .into_iter()
        .filter_map(|sc| sc.get("_attributes")?.get("name")?.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

pub async fn fetch_calendar_user_addresses(account: &DavAccount, ctx: &Ctx) -> Result<Vec<String>, DavError> {
    let (Some(principal_url), Some(root_url)) = (&account.principal_url, &account.root_url) else {
        return Err(DavError::msg(
            "account must have principalUrl,rootUrl before fetchUserAddresses",
        ));
    };
    let mut prop = Map::new();
    prop.insert("c:calendar-user-address-set".to_string(), Value::Object(Map::new()));
    let responses = propfind(principal_url, Value::Object(prop), Some(DavDepth::Zero), ctx).await?;

    let matched = responses
        .iter()
        .find(|r| url_matches(Some(principal_url), r.href.as_deref(), Some(root_url)))
        .filter(|r| r.ok)
        .ok_or_else(|| DavError::msg("cannot find calendarUserAddresses"))?;

    let raw_hrefs = matched
        .props
        .as_ref()
        .and_then(|p| p.get("calendarUserAddressSet"))
        .and_then(|s| s.get("href"));
    let hrefs: Vec<&Value> = match raw_hrefs {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(other) => vec![other],
        None => vec![],
    };
    Ok(hrefs
        .into_iter()
        .filter_map(|h| h.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect())
}

pub async fn calendar_query(
    url: &str,
    props: Value,
    filters: Option<Value>,
    timezone: Option<&str>,
    depth: Option<DavDepth>,
    ctx: &Ctx,
) -> Result<Vec<DavResponse>, DavError> {
    let mut body = Map::new();
    body.insert(
        "_attributes".to_string(),
        Value::Object(
            get_dav_attribute(&[
                DavNamespace::Caldav,
                DavNamespace::CalendarServer,
                DavNamespace::CaldavApple,
                DavNamespace::Dav,
            ])
            .into_iter()
            .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
            .collect(),
        ),
    );
    body.insert("d:prop".to_string(), props);
    if let Some(f) = filters {
        body.insert("filter".to_string(), f);
    }
    if let Some(tz) = timezone {
        body.insert("timezone".to_string(), Value::String(tz.to_string()));
    }

    let mut root = Map::new();
    root.insert("calendar-query".to_string(), Value::Object(body));
    collection_query(url, root, depth, DavNamespaceShort::Caldav, ctx).await
}

pub async fn calendar_multi_get(
    url: &str,
    props: Value,
    object_urls: Option<&[String]>,
    filters: Option<Value>,
    timezone: Option<&str>,
    depth: DavDepth,
    ctx: &Ctx,
) -> Result<Vec<DavResponse>, DavError> {
    let mut body = Map::new();
    body.insert(
        "_attributes".to_string(),
        Value::Object(
            get_dav_attribute(&[DavNamespace::Dav, DavNamespace::Caldav])
                .into_iter()
                .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
                .collect(),
        ),
    );
    body.insert("d:prop".to_string(), props);
    if let Some(urls) = object_urls {
        body.insert(
            "d:href".to_string(),
            Value::Array(urls.iter().map(|u| Value::String(u.clone())).collect()),
        );
    }
    if let Some(f) = filters {
        body.insert("filter".to_string(), f);
    }
    if let Some(tz) = timezone {
        body.insert("timezone".to_string(), Value::String(tz.to_string()));
    }

    let mut root = Map::new();
    root.insert("calendar-multiget".to_string(), Value::Object(body));
    collection_query(url, root, Some(depth), DavNamespaceShort::Caldav, ctx).await
}

pub async fn make_calendar(
    url: &str,
    props: Value,
    depth: Option<DavDepth>,
    ctx: &Ctx,
) -> Result<Vec<DavResponse>, DavError> {
    let mut headers = ctx.headers();
    if let Some(d) = depth {
        headers.insert("depth".to_string(), d.as_str().to_string());
    }
    let mut mkcalendar = Map::new();
    mkcalendar.insert(
        "_attributes".to_string(),
        Value::Object(
            get_dav_attribute(&[DavNamespace::Dav, DavNamespace::Caldav, DavNamespace::CaldavApple])
                .into_iter()
                .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
                .collect(),
        ),
    );
    let mut set = Map::new();
    set.insert("prop".to_string(), props);
    mkcalendar.insert("set".to_string(), Value::Object(set));
    let mut root = Map::new();
    root.insert("c:mkcalendar".to_string(), Value::Object(mkcalendar));

    dav_request(
        url,
        DavRequest {
            method: Some(DavMethod::Mkcalendar),
            headers,
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

pub async fn fetch_calendars(
    account: &DavAccount,
    custom_props: Option<Value>,
    projected_props: Option<&[String]>,
    ctx: &Ctx,
) -> Result<Vec<DavCalendar>, DavError> {
    let (Some(home_url), Some(root_url)) = (&account.home_url, &account.root_url) else {
        return Err(DavError::msg("account must have homeUrl,rootUrl before fetchCalendars"));
    };

    let props = custom_props.unwrap_or_else(|| {
        let mut m = Map::new();
        for key in [
            "c:calendar-description",
            "c:calendar-timezone",
            "d:displayname",
            "ca:calendar-color",
            "cs:getctag",
            "d:resourcetype",
            "c:supported-calendar-component-set",
            "d:sync-token",
        ] {
            m.insert(key.to_string(), Value::Object(Map::new()));
        }
        Value::Object(m)
    });

    let res = propfind(home_url, props, Some(DavDepth::One), ctx).await?;

    let ical_values: Vec<&str> = IcalObjects::all().iter().map(|o| o.as_str()).collect();
    let mut calendars = Vec::new();
    for r in res.iter() {
        let props = r.props.as_ref();
        let is_calendar = props
            .and_then(|p| p.get("resourcetype"))
            .and_then(|rt| rt.as_object())
            .map(|m| m.contains_key("calendar"))
            .unwrap_or(false);
        if !is_calendar {
            continue;
        }
        let comp_set = props
            .and_then(|p| p.get("supportedCalendarComponentSet"))
            .and_then(|s| s.get("comp"));
        let components = extract_component_names(comp_set);
        if !(components.is_empty() || components.iter().any(|c| ical_values.contains(&c.as_str()))) {
            continue;
        }

        let url = r
            .href
            .as_deref()
            .map(|h| crate::util::request_helpers::resolve_href(h, &ensure_trailing_slash(root_url)))
            .unwrap_or_default();
        let display_name = props
            .and_then(|p| p.get("displayname"))
            .map(|v| v.get("_cdata").and_then(|c| c.as_str()).map(String::from).unwrap_or_else(|| value_to_string(v)));
        let mut cal = DavCalendar {
            description: props.and_then(|p| p.get("calendarDescription")).and_then(|v| v.as_str()).map(String::from).or(Some(String::new())),
            timezone: props.and_then(|p| p.get("calendarTimezone")).and_then(|v| v.as_str()).map(String::from).or(Some(String::new())),
            url,
            ctag: props.and_then(|p| p.get("getctag")).map(value_to_string),
            calendar_color: props.and_then(|p| p.get("calendarColor")).map(value_to_string),
            display_name,
            components: Some(components),
            resourcetype: props.and_then(|p| p.get("resourcetype")).and_then(|v| v.as_object()).map(|m| m.keys().cloned().collect()),
            sync_token: props.and_then(|p| p.get("syncToken")).map(value_to_string),
            projected_props: projected_props.map(|keys| {
                let mut m = std::collections::HashMap::new();
                if let Some(p) = props {
                    for k in keys {
                        if let Some(v) = p.get(k) {
                            m.insert(k.clone(), v.clone());
                        }
                    }
                }
                m
            }).filter(|m| !m.is_empty()),
            ..Default::default()
        };
        cal.reports = Some(supported_report_set(&cal.url, ctx).await?);
        calendars.push(cal);
    }
    Ok(calendars)
}

pub async fn fetch_calendar_objects(
    calendar: &DavCalendar,
    object_urls: Option<&[String]>,
    filters: Option<Value>,
    time_range: Option<(&str, &str)>,
    expand: bool,
    url_filter: Option<&dyn Fn(&str) -> bool>,
    use_multi_get: bool,
    ctx: &Ctx,
) -> Result<Vec<DavCalendarObject>, DavError> {
    if let Some((start, end)) = time_range {
        validate_time_range(start, end)?;
    }
    if calendar.url.is_empty() {
        return Err(DavError::msg("calendar must have url before fetchCalendarObjects"));
    }

    let default_filter = |u: &str| u.contains(".ics");
    let url_filter: &dyn Fn(&str) -> bool = url_filter.unwrap_or(&default_filter);

    let filters = filters.unwrap_or_else(|| {
        let mut vevent = Map::new();
        vevent.insert("_attributes".to_string(), {
            let mut a = Map::new();
            a.insert("name".to_string(), Value::String("VEVENT".to_string()));
            Value::Object(a)
        });
        if let Some((start, end)) = time_range {
            let mut tr = Map::new();
            tr.insert("_attributes".to_string(), {
                let mut a = Map::new();
                a.insert("start".to_string(), Value::String(format_caldav_utc(start)));
                a.insert("end".to_string(), Value::String(format_caldav_utc(end)));
                Value::Object(a)
            });
            vevent.insert("time-range".to_string(), Value::Object(tr));
        }
        let mut vcalendar = Map::new();
        vcalendar.insert("_attributes".to_string(), {
            let mut a = Map::new();
            a.insert("name".to_string(), Value::String("VCALENDAR".to_string()));
            Value::Object(a)
        });
        vcalendar.insert("comp-filter".to_string(), Value::Object(vevent));
        let mut comp_filter = Map::new();
        comp_filter.insert("comp-filter".to_string(), Value::Object(vcalendar));
        Value::Array(vec![Value::Object(comp_filter)])
    });

    let mut initial_responses: Vec<DavResponse> = vec![];
    if object_urls.is_none() {
        let mut props = Map::new();
        props.insert("d:getetag".to_string(), Value::Object(Map::new()));
        if expand && time_range.is_some() {
            let (start, end) = time_range.unwrap();
            let mut expand_el = Map::new();
            expand_el.insert("_attributes".to_string(), {
                let mut a = Map::new();
                a.insert("start".to_string(), Value::String(format_caldav_utc(start)));
                a.insert("end".to_string(), Value::String(format_caldav_utc(end)));
                Value::Object(a)
            });
            let mut cal_data = Map::new();
            cal_data.insert("c:expand".to_string(), Value::Object(expand_el));
            props.insert("c:calendar-data".to_string(), Value::Object(cal_data));
        }
        initial_responses = calendar_query(&calendar.url, Value::Object(props), Some(filters.clone()), None, Some(DavDepth::One), ctx).await?;
    }

    let base = ensure_trailing_slash(&calendar.url);
    let calendar_object_urls: Vec<String> = (if let Some(urls) = object_urls {
        urls.to_vec()
    } else {
        initial_responses.iter().filter_map(|r| r.href.clone()).collect()
    })
    .into_iter()
    .filter(|u| !u.trim().is_empty())
    .map(|u| if u.starts_with("http") { u } else { Url::parse(&base).ok().and_then(|b| b.join(&u).ok()).map(|x| x.to_string()).unwrap_or(u) })
    .filter(|u| url_filter(u))
    .map(|u| {
        Url::parse(&u)
            .map(|parsed| format!("{}{}", parsed.path(), parsed.query().map(|q| format!("?{q}")).unwrap_or_default()))
            .unwrap_or(u)
    })
    .collect();

    let mut calendar_object_results: Vec<DavResponse> = vec![];
    if !calendar_object_urls.is_empty() {
        if expand && object_urls.is_none() {
            calendar_object_results = initial_responses
                .into_iter()
                .filter(|res| {
                    let href = res.href.clone().unwrap_or_default();
                    let full = if href.starts_with("http") {
                        href
                    } else {
                        Url::parse(&base).ok().and_then(|b| b.join(&href).ok()).map(|x| x.to_string()).unwrap_or(href)
                    };
                    url_filter(&full)
                })
                .collect();
        } else if !use_multi_get {
            let mut props = Map::new();
            props.insert("d:getetag".to_string(), Value::Object(Map::new()));
            let mut cal_data = Map::new();
            if expand && time_range.is_some() {
                let (start, end) = time_range.unwrap();
                let mut expand_el = Map::new();
                expand_el.insert("_attributes".to_string(), {
                    let mut a = Map::new();
                    a.insert("start".to_string(), Value::String(format_caldav_utc(start)));
                    a.insert("end".to_string(), Value::String(format_caldav_utc(end)));
                    Value::Object(a)
                });
                cal_data.insert("c:expand".to_string(), Value::Object(expand_el));
            }
            props.insert("c:calendar-data".to_string(), Value::Object(cal_data));
            let mut results = calendar_query(&calendar.url, Value::Object(props), Some(filters), None, Some(DavDepth::One), ctx).await?;
            if object_urls.is_some() {
                results.retain(|res| {
                    let href = res.href.clone().unwrap_or_default();
                    let full = if href.starts_with("http") {
                        href
                    } else {
                        Url::parse(&base).ok().and_then(|b| b.join(&href).ok()).map(|x| x.to_string()).unwrap_or(href)
                    };
                    calendar_object_urls.iter().any(|t| url_matches(Some(&full), Some(t), Some(&calendar.url)))
                });
            }
            calendar_object_results = results;
        } else {
            let mut props = Map::new();
            props.insert("d:getetag".to_string(), Value::Object(Map::new()));
            let mut cal_data = Map::new();
            if expand && time_range.is_some() {
                let (start, end) = time_range.unwrap();
                let mut expand_el = Map::new();
                expand_el.insert("_attributes".to_string(), {
                    let mut a = Map::new();
                    a.insert("start".to_string(), Value::String(format_caldav_utc(start)));
                    a.insert("end".to_string(), Value::String(format_caldav_utc(end)));
                    Value::Object(a)
                });
                cal_data.insert("c:expand".to_string(), Value::Object(expand_el));
            }
            props.insert("c:calendar-data".to_string(), Value::Object(cal_data));
            calendar_object_results = calendar_multi_get(&calendar.url, Value::Object(props), Some(&calendar_object_urls), None, None, DavDepth::One, ctx).await?;
        }
    }

    Ok(calendar_object_results
        .into_iter()
        .map(|res| {
            let href = res.href.clone().unwrap_or_default();
            let url = Url::parse(&base).ok().and_then(|b| b.join(&href).ok()).map(|u| u.to_string()).unwrap_or(href);
            let etag = res.props.as_ref().and_then(|p| p.get("getetag")).map(value_to_string);
            let data = res.props.as_ref().and_then(|p| p.get("calendarData")).map(|v| {
                v.get("_cdata").and_then(|c| c.as_str()).map(String::from).unwrap_or_else(|| value_to_string(v))
            });
            DavCalendarObject { url, etag, data }
        })
        .collect())
}

pub async fn create_calendar_object(calendar: &DavCalendar, ical_string: &str, filename: &str, ctx: &Ctx) -> Result<HttpResult, DavError> {
    let base = ensure_trailing_slash(&calendar.url);
    let url = Url::parse(&base).ok().and_then(|b| b.join(filename).ok()).map(|u| u.to_string()).unwrap_or_default();
    let mut headers = ctx.headers.clone();
    headers.insert("content-type".to_string(), "text/calendar; charset=utf-8".to_string());
    headers.insert("If-None-Match".to_string(), "*".to_string());
    let ctx2 = Ctx { headers, ..ctx.clone() };
    create_object(&url, ical_string, &ctx2).await
}

pub async fn update_calendar_object(calendar_object: &DavCalendarObject, ctx: &Ctx) -> Result<HttpResult, DavError> {
    let mut headers = ctx.headers.clone();
    headers.insert("content-type".to_string(), "text/calendar; charset=utf-8".to_string());
    let ctx2 = Ctx { headers, ..ctx.clone() };
    update_object(&calendar_object.url, calendar_object.data.as_deref().unwrap_or(""), calendar_object.etag.as_deref(), &ctx2).await
}

pub async fn delete_calendar_object(calendar_object: &DavCalendarObject, ctx: &Ctx) -> Result<HttpResult, DavError> {
    delete_object(&calendar_object.url, calendar_object.etag.as_deref(), ctx).await
}

pub async fn sync_calendars_detailed(old_calendars: &[DavCalendar], account: &DavAccount, ctx: &Ctx) -> Result<SyncCalendarsDetailedResult, DavError> {
    let root_url = account.root_url.as_deref();
    let local_calendars = if old_calendars.is_empty() {
        account.calendars.clone().unwrap_or_default()
    } else {
        old_calendars.to_vec()
    };
    let remote_calendars = fetch_calendars(account, None, None, ctx).await?;

    let created: Vec<DavCalendar> = remote_calendars
        .iter()
        .filter(|rc| local_calendars.iter().all(|lc| !url_matches(Some(&lc.url), Some(&rc.url), root_url)))
        .cloned()
        .collect();

    let updated_pairs: Vec<(DavCalendar, DavCalendar)> = local_calendars
        .iter()
        .filter_map(|curr| {
            let found = remote_calendars.iter().find(|rc| url_matches(Some(&rc.url), Some(&curr.url), root_url))?;
            let sync_token_changed = found.sync_token.is_some() && found.sync_token != curr.sync_token;
            let ctag_changed = found.ctag.is_some() && found.ctag != curr.ctag;
            if sync_token_changed || ctag_changed {
                Some((curr.clone(), found.clone()))
            } else {
                None
            }
        })
        .collect();

    let mut updated_with_objects = Vec::new();
    for (local, remote) in updated_pairs {
        let mut collection = remote.clone();
        collection.ctag = local.ctag.clone();
        collection.sync_token = local.sync_token.clone();
        collection.objects = local.objects.clone();
        collection.object_multi_get = Some(std::rc::Rc::new(move |p: crate::types::models::ObjectMultiGetParams| {
            Box::pin(async move {
                calendar_multi_get(&p.url, p.props, Some(&p.object_urls), p.filters, p.timezone.as_deref(), p.depth, &Ctx { headers: p.headers.unwrap_or_default(), fetch_options: p.fetch_options.unwrap_or_default(), ..Default::default() }).await
            }) as crate::types::models::BoxFuture<'static, Result<Vec<DavResponse>, DavError>>
        }));
        let account_type = account.account_type;
        let result = smart_collection_sync(&collection, None, account_type, ctx).await?;
        let mut merged = result.clone();
        if merged.ctag.is_none() {
            merged.ctag = remote.ctag.clone();
        }
        if merged.sync_token.is_none() {
            merged.sync_token = remote.sync_token.clone();
        }
        updated_with_objects.push(merged);
    }

    let deleted: Vec<DavCalendar> = local_calendars
        .iter()
        .filter(|cal| remote_calendars.iter().all(|rc| !url_matches(Some(&rc.url), Some(&cal.url), root_url)))
        .cloned()
        .collect();

    Ok(SyncCalendarsDetailedResult { created, updated: updated_with_objects, deleted })
}

pub async fn sync_calendars(old_calendars: &[DavCalendar], account: &DavAccount, ctx: &Ctx) -> Result<Vec<DavCalendar>, DavError> {
    let root_url = account.root_url.as_deref();
    let local_calendars = if old_calendars.is_empty() {
        account.calendars.clone().unwrap_or_default()
    } else {
        old_calendars.to_vec()
    };
    let detailed = sync_calendars_detailed(old_calendars, account, ctx).await?;
    let unchanged: Vec<DavCalendar> = local_calendars
        .iter()
        .filter(|cal| {
            detailed.created.iter().chain(detailed.updated.iter()).chain(detailed.deleted.iter())
                .all(|other| !url_matches(Some(&other.url), Some(&cal.url), root_url))
                && detailed.deleted.iter().all(|d| !url_matches(Some(&cal.url), Some(&d.url), root_url))
        })
        .cloned()
        .collect();
    let mut out = unchanged;
    out.extend(detailed.created);
    out.extend(detailed.updated);
    Ok(out)
}

pub async fn free_busy_query(url: &str, time_range: (&str, &str), depth: Option<DavDepth>, ctx: &Ctx) -> Result<DavResponse, DavError> {
    validate_time_range(time_range.0, time_range.1)?;
    let mut body = Map::new();
    body.insert(
        "_attributes".to_string(),
        Value::Object(
            get_dav_attribute(&[DavNamespace::Caldav])
                .into_iter()
                .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
                .collect(),
        ),
    );
    let mut tr = Map::new();
    tr.insert("_attributes".to_string(), {
        let mut a = Map::new();
        a.insert("start".to_string(), Value::String(format_caldav_utc(time_range.0)));
        a.insert("end".to_string(), Value::String(format_caldav_utc(time_range.1)));
        Value::Object(a)
    });
    body.insert("c:time-range".to_string(), Value::Object(tr));

    let mut root = Map::new();
    root.insert("free-busy-query".to_string(), Value::Object(body));
    let result = collection_query(url, root, depth, DavNamespaceShort::Caldav, ctx).await?;
    result.into_iter().next().ok_or_else(|| DavError::msg("freeBusyQuery returned no response"))
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
