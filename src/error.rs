#[derive(Debug, thiserror::Error)]
pub enum DavError {
    #[error("{0}")]
    Message(String),
    #[error("fetch failed: {0}")]
    Fetch(String),
    #[error("failed to parse DAV response XML: {0}")]
    XmlParse(String),
    #[error("failed to serialize DAV request XML: {0}")]
    XmlWrite(String),
}

impl DavError {
    pub fn msg(s: impl Into<String>) -> Self {
        DavError::Message(s.into())
    }
}

pub type DavResult<T> = Result<T, DavError>;
