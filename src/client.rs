use crate::account::{create_account, CreateAccountOptions};
use crate::address_book::{create_vcard, delete_vcard, fetch_address_books, fetch_vcards, update_vcard};
use crate::calendar::{
    create_calendar_object, delete_calendar_object, fetch_calendar_objects, fetch_calendars,
    free_busy_query, sync_calendars, sync_calendars_detailed, update_calendar_object,
};
use crate::collection::{smart_collection_sync, smart_collection_sync_detailed, DavCollectionLike};
use crate::error::DavError;
use crate::request::{propfind, Ctx, HttpResult};
use crate::types::dav_types::DavDepth;
use crate::types::function_overloads::{SmartCollectionSyncDetailedResult, SyncCalendarsDetailedResult};
use crate::types::models::{
    DavAccount, DavAccountType, DavAddressBook, DavCalendar, DavCalendarObject, DavCredentials, DavVCard,
};
use crate::util::auth_helpers::{get_bearer_auth_headers, get_oauth_headers, get_basic_auth_headers};
use crate::util::fetch::{FetchClient, FetchOptions};
use serde_json::Value;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    Basic,
    Oauth,
    Digest,
    Custom,
    Bearer,
}

#[async_trait::async_trait(?Send)]
pub trait CustomAuthFn {
    async fn call(&self, credentials: &DavCredentials) -> Result<HashMap<String, String>, DavError>;
}

pub struct DavClient {
    pub server_url: String,
    pub credentials: DavCredentials,
    pub auth_method: AuthMethod,
    pub account_type: DavAccountType,
    pub auth_function: Option<Rc<dyn CustomAuthFn>>,
    pub fetch_options: FetchOptions,
    pub fetch_override: Option<Rc<dyn FetchClient>>,
    pub auth_headers: HashMap<String, String>,
    pub account: Option<DavAccount>,
}

impl DavClient {
    pub fn new(
        server_url: impl Into<String>,
        credentials: DavCredentials,
        auth_method: AuthMethod,
        account_type: DavAccountType,
    ) -> Self {
        DavClient {
            server_url: server_url.into(),
            credentials,
            auth_method,
            account_type,
            auth_function: None,
            fetch_options: FetchOptions::default(),
            fetch_override: None,
            auth_headers: HashMap::new(),
            account: None,
        }
    }

    fn ctx(&self) -> Ctx {
        Ctx {
            headers: self.auth_headers.clone(),
            headers_to_exclude: vec![],
            fetch_options: self.fetch_options.clone(),
            fetch: self.fetch_override.clone(),
        }
    }

    pub async fn login(&mut self, load_collections: bool, load_objects: bool) -> Result<(), DavError> {
        self.auth_headers = match self.auth_method {
            AuthMethod::Basic => get_basic_auth_headers(&self.credentials)?,
            AuthMethod::Bearer => get_bearer_auth_headers(&self.credentials),
            AuthMethod::Oauth => {
                let default_client: Rc<dyn FetchClient> = Rc::new(crate::util::fetch::BrowserFetchClient);
                let client = self.fetch_override.clone().unwrap_or(default_client);
                get_oauth_headers(&mut self.credentials, Some(&self.fetch_options), self.fetch_override.as_ref(), &client)
                    .await?
                    .headers
            }
            AuthMethod::Digest => {
                let mut h = HashMap::new();
                h.insert(
                    "Authorization".to_string(),
                    format!("Digest {}", self.credentials.digest_string.clone().unwrap_or_default()),
                );
                h
            }
            AuthMethod::Custom => {
                let f = self
                    .auth_function
                    .clone()
                    .ok_or_else(|| DavError::msg("authMethod 'Custom' requires an authFunction to produce request headers"))?;
                f.call(&self.credentials).await?
            }
        };

        let account = DavAccount {
            server_url: self.server_url.clone(),
            credentials: Some(self.credentials.clone()),
            account_type: self.account_type,
            ..Default::default()
        };
        self.account = Some(
            create_account(
                &account,
                CreateAccountOptions { load_collections, load_objects },
                &self.ctx(),
            )
            .await?,
        );
        Ok(())
    }

    pub async fn propfind(&self, url: &str, props: Value, depth: Option<DavDepth>) -> Result<Vec<crate::types::dav_types::DavResponse>, DavError> {
        propfind(url, props, depth, &self.ctx()).await
    }

    pub async fn create_object(&self, url: &str, data: &str) -> Result<HttpResult, DavError> {
        crate::request::create_object(url, data, &self.ctx()).await
    }

    pub async fn update_object(&self, url: &str, data: &str, etag: Option<&str>) -> Result<HttpResult, DavError> {
        crate::request::update_object(url, data, etag, &self.ctx()).await
    }

    pub async fn delete_object(&self, url: &str, etag: Option<&str>) -> Result<HttpResult, DavError> {
        crate::request::delete_object(url, etag, &self.ctx()).await
    }

    pub async fn fetch_calendars(&self, custom_props: Option<Value>) -> Result<Vec<DavCalendar>, DavError> {
        let account = self.account.clone().ok_or_else(|| DavError::msg("DavClient has no account; call login() first"))?;
        fetch_calendars(&account, custom_props, None, &self.ctx()).await
    }

    pub async fn fetch_calendar_objects(
        &self,
        calendar: &DavCalendar,
        object_urls: Option<&[String]>,
        time_range: Option<(&str, &str)>,
    ) -> Result<Vec<DavCalendarObject>, DavError> {
        fetch_calendar_objects(calendar, object_urls, None, time_range, false, None, true, &self.ctx()).await
    }

    pub async fn create_calendar_object(&self, calendar: &DavCalendar, ical_string: &str, filename: &str) -> Result<HttpResult, DavError> {
        create_calendar_object(calendar, ical_string, filename, &self.ctx()).await
    }

    pub async fn update_calendar_object(&self, calendar_object: &DavCalendarObject) -> Result<HttpResult, DavError> {
        update_calendar_object(calendar_object, &self.ctx()).await
    }

    pub async fn delete_calendar_object(&self, calendar_object: &DavCalendarObject) -> Result<HttpResult, DavError> {
        delete_calendar_object(calendar_object, &self.ctx()).await
    }

    pub async fn sync_calendars(&self, old_calendars: &[DavCalendar]) -> Result<Vec<DavCalendar>, DavError> {
        let account = self.account.clone().ok_or_else(|| DavError::msg("DavClient has no account; call login() first"))?;
        sync_calendars(old_calendars, &account, &self.ctx()).await
    }

    pub async fn sync_calendars_detailed(&self, old_calendars: &[DavCalendar]) -> Result<SyncCalendarsDetailedResult, DavError> {
        let account = self.account.clone().ok_or_else(|| DavError::msg("DavClient has no account; call login() first"))?;
        sync_calendars_detailed(old_calendars, &account, &self.ctx()).await
    }

    pub async fn free_busy_query(&self, url: &str, time_range: (&str, &str)) -> Result<crate::types::dav_types::DavResponse, DavError> {
        free_busy_query(url, time_range, None, &self.ctx()).await
    }

    pub async fn fetch_address_books(&self, custom_props: Option<Value>) -> Result<Vec<DavAddressBook>, DavError> {
        let account = self.account.clone().ok_or_else(|| DavError::msg("DavClient has no account; call login() first"))?;
        fetch_address_books(&account, custom_props, &self.ctx()).await
    }

    pub async fn fetch_vcards(&self, address_book: &DavAddressBook) -> Result<Vec<DavVCard>, DavError> {
        fetch_vcards(address_book, None, None, true, &self.ctx()).await
    }

    pub async fn create_vcard(&self, address_book: &DavAddressBook, vcard_string: &str, filename: &str) -> Result<HttpResult, DavError> {
        create_vcard(address_book, vcard_string, filename, &self.ctx()).await
    }

    pub async fn update_vcard(&self, vcard: &DavVCard) -> Result<HttpResult, DavError> {
        update_vcard(vcard, &self.ctx()).await
    }

    pub async fn delete_vcard(&self, vcard: &DavVCard) -> Result<HttpResult, DavError> {
        delete_vcard(vcard, &self.ctx()).await
    }

    pub async fn smart_collection_sync<T: DavCollectionLike>(&self, collection: &T, method: Option<&str>) -> Result<T, DavError> {
        smart_collection_sync(collection, method, self.account_type, &self.ctx()).await
    }

    pub async fn smart_collection_sync_detailed<T: DavCollectionLike>(
        &self,
        collection: &T,
        method: Option<&str>,
    ) -> Result<SmartCollectionSyncDetailedResult<T>, DavError> {
        smart_collection_sync_detailed(collection, method, self.account_type, &self.ctx()).await
    }
}

pub async fn create_dav_client(
    server_url: impl Into<String>,
    credentials: DavCredentials,
    auth_method: AuthMethod,
    account_type: DavAccountType,
) -> Result<DavClient, DavError> {
    let mut client = DavClient::new(server_url, credentials, auth_method, account_type);
    client.login(false, false).await?;
    Ok(client)
}
