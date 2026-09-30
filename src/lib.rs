pub mod account;
pub mod address_book;
pub mod calendar;
pub mod client;
pub mod collection;
pub mod consts;
pub mod error;
pub mod request;
pub mod types;
pub mod util;
pub mod xml;

pub use account::{create_account, fetch_home_url, fetch_principal_url, service_discovery, CreateAccountOptions};
pub use address_book::{
    address_book_multi_get, address_book_query, create_vcard, delete_vcard, fetch_address_books,
    fetch_vcards, update_vcard,
};
pub use calendar::{
    calendar_multi_get, calendar_query, create_calendar_object, delete_calendar_object,
    fetch_calendar_objects, fetch_calendar_user_addresses, fetch_calendars, free_busy_query,
    make_calendar, sync_calendars, sync_calendars_detailed, update_calendar_object,
};
pub use client::{create_dav_client, AuthMethod, DavClient};
pub use collection::{
    collection_query, is_collection_dirty, make_collection, smart_collection_sync,
    smart_collection_sync_detailed, supported_report_set, sync_collection, DavCollectionLike,
};
pub use consts::{dav_attribute_map, DavNamespace, DavNamespaceShort, IcalObjects};
pub use error::DavError;
pub use request::{create_object, dav_request, delete_object, propfind, update_object, Ctx};
pub use types::{
    DavAccount, DavAccountType, DavAddressBook, DavCalendar, DavCalendarObject, DavCollection,
    DavCredentials, DavDepth, DavMethod, DavObject, DavRequest, DavResponse, DavTokens, DavVCard,
    SmartCollectionSyncDetailedResult, SyncCalendarsDetailedResult,
};
pub use util::auth_helpers::{
    fetch_oauth_tokens, get_basic_auth_headers, get_bearer_auth_headers, get_oauth_headers,
    refresh_access_token,
};
pub use util::fetch::{BrowserFetchClient, FetchClient, FetchOptions, HttpResponse};
pub use util::request_helpers::{
    cleanup_falsy, ensure_trailing_slash, exclude_headers, get_dav_attribute, merge_headers,
    url_contains, url_equals, url_matches,
};
