use crate::types::models::{DavCalendar, DavObject};

#[derive(Debug, Clone)]
pub struct SmartCollectionSyncDetailedResult<T> {
    pub base: T,
    pub created: Vec<DavObject>,
    pub updated: Vec<DavObject>,
    pub deleted: Vec<DavObject>,
}

#[derive(Debug, Clone, Default)]
pub struct SyncCalendarsDetailedResult {
    pub created: Vec<DavCalendar>,
    pub updated: Vec<DavCalendar>,
    pub deleted: Vec<DavCalendar>,
}
