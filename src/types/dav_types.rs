use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DavDepth {
    Zero,
    One,
    Infinity,
}

impl DavDepth {
    pub fn as_str(self) -> &'static str {
        match self {
            DavDepth::Zero => "0",
            DavDepth::One => "1",
            DavDepth::Infinity => "infinity",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DavMethod {
    Copy,
    Lock,
    Mkcol,
    Move,
    Propfind,
    Proppatch,
    Unlock,
    Report,
    Search,
    Mkcalendar,
    Get,
    Head,
    Post,
    Put,
    Delete,
    Connect,
    Options,
    Trace,
    Patch,
}

impl DavMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            DavMethod::Copy => "COPY",
            DavMethod::Lock => "LOCK",
            DavMethod::Mkcol => "MKCOL",
            DavMethod::Move => "MOVE",
            DavMethod::Propfind => "PROPFIND",
            DavMethod::Proppatch => "PROPPATCH",
            DavMethod::Unlock => "UNLOCK",
            DavMethod::Report => "REPORT",
            DavMethod::Search => "SEARCH",
            DavMethod::Mkcalendar => "MKCALENDAR",
            DavMethod::Get => "GET",
            DavMethod::Head => "HEAD",
            DavMethod::Post => "POST",
            DavMethod::Put => "PUT",
            DavMethod::Delete => "DELETE",
            DavMethod::Connect => "CONNECT",
            DavMethod::Options => "OPTIONS",
            DavMethod::Trace => "TRACE",
            DavMethod::Patch => "PATCH",
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct DavResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    pub status: u16,
    #[serde(rename = "statusText")]
    pub status_text: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub responsedescription: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub props: Option<Value>,
}

#[derive(Debug, Clone, Default)]
pub struct DavRequest {
    pub headers: HashMap<String, String>,
    pub method: Option<DavMethod>,
    pub body: Option<Value>,
    pub namespace: Option<crate::consts::DavNamespaceShort>,
    pub attributes: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct DavTokens {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}
