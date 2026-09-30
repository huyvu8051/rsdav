use crate::util::fetch::FetchOptions;
use crate::types::dav_types::{DavDepth, DavResponse};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DavObject {
    pub data: Option<String>,
    pub etag: Option<String>,
    pub url: String,
}

pub type DavVCard = DavObject;
pub type DavCalendarObject = DavObject;

#[derive(Debug, Clone, Default)]
pub struct DavCredentials {
    pub username: Option<String>,
    pub password: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub authorization_code: Option<String>,
    pub redirect_url: Option<String>,
    pub token_url: Option<String>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expiration: Option<i64>,
    pub digest_string: Option<String>,
    pub custom_data: Option<HashMap<String, Value>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DavAccountType {
    Caldav,
    Carddav,
}

impl Default for DavAccountType {
    fn default() -> Self {
        DavAccountType::Caldav
    }
}

pub struct FetchObjectsParams {
    pub collection: DavCalendar,
    pub headers: Option<HashMap<String, String>>,
    pub fetch_options: Option<FetchOptions>,
}

pub type FetchObjectsFn =
    Rc<dyn Fn(FetchObjectsParams) -> BoxFuture<'static, Result<Vec<DavObject>, crate::error::DavError>>>;

pub struct ObjectMultiGetParams {
    pub url: String,
    pub props: Value,
    pub object_urls: Vec<String>,
    pub filters: Option<Value>,
    pub timezone: Option<String>,
    pub depth: DavDepth,
    pub headers: Option<HashMap<String, String>>,
    pub fetch_options: Option<FetchOptions>,
}

pub type ObjectMultiGetFn =
    Rc<dyn Fn(ObjectMultiGetParams) -> BoxFuture<'static, Result<Vec<DavResponse>, crate::error::DavError>>>;

#[derive(Clone, Default)]
pub struct DavCollection {
    pub objects: Option<Vec<DavObject>>,
    pub ctag: Option<String>,
    pub description: Option<String>,
    pub display_name: Option<String>,
    pub reports: Option<Vec<String>>,
    pub resourcetype: Option<Vec<String>>,
    pub sync_token: Option<String>,
    pub url: String,
    pub fetch_options: Option<FetchOptions>,
    pub fetch_objects: Option<FetchObjectsFn>,
    pub object_multi_get: Option<ObjectMultiGetFn>,
}

impl std::fmt::Debug for DavCollection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DavCollection")
            .field("url", &self.url)
            .field("ctag", &self.ctag)
            .field("sync_token", &self.sync_token)
            .field("objects", &self.objects)
            .field("reports", &self.reports)
            .field("resourcetype", &self.resourcetype)
            .finish()
    }
}

pub type DavAddressBook = DavCollection;

#[derive(Clone, Default)]
pub struct DavCalendar {
    pub components: Option<Vec<String>>,
    pub timezone: Option<String>,
    pub projected_props: Option<HashMap<String, Value>>,
    pub calendar_color: Option<String>,
    pub objects: Option<Vec<DavObject>>,
    pub ctag: Option<String>,
    pub description: Option<String>,
    pub display_name: Option<String>,
    pub reports: Option<Vec<String>>,
    pub resourcetype: Option<Vec<String>>,
    pub sync_token: Option<String>,
    pub url: String,
    pub fetch_options: Option<FetchOptions>,
    pub fetch_objects: Option<FetchObjectsFn>,
    pub object_multi_get: Option<ObjectMultiGetFn>,
}

impl std::fmt::Debug for DavCalendar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DavCalendar")
            .field("url", &self.url)
            .field("display_name", &self.display_name)
            .field("ctag", &self.ctag)
            .field("sync_token", &self.sync_token)
            .field("components", &self.components)
            .finish()
    }
}

#[derive(Debug, Clone, Default)]
pub struct DavAccount {
    pub account_type: DavAccountType,
    pub server_url: String,
    pub credentials: Option<DavCredentials>,
    pub root_url: Option<String>,
    pub principal_url: Option<String>,
    pub home_url: Option<String>,
    pub calendars: Option<Vec<DavCalendar>>,
    pub address_books: Option<Vec<DavAddressBook>>,
}
