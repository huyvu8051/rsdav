#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DavNamespace {
    CalendarServer,
    CaldavApple,
    Caldav,
    Carddav,
    Dav,
}

impl DavNamespace {
    pub fn as_str(self) -> &'static str {
        match self {
            DavNamespace::CalendarServer => "http://calendarserver.org/ns/",
            DavNamespace::CaldavApple => "http://apple.com/ns/ical/",
            DavNamespace::Caldav => "urn:ietf:params:xml:ns:caldav",
            DavNamespace::Carddav => "urn:ietf:params:xml:ns:carddav",
            DavNamespace::Dav => "DAV:",
        }
    }
}

pub fn dav_attribute_map(ns: DavNamespace) -> &'static str {
    match ns {
        DavNamespace::Caldav => "xmlns:c",
        DavNamespace::Carddav => "xmlns:card",
        DavNamespace::CalendarServer => "xmlns:cs",
        DavNamespace::CaldavApple => "xmlns:ca",
        DavNamespace::Dav => "xmlns:d",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DavNamespaceShort {
    Caldav,
    Carddav,
    CalendarServer,
    CaldavApple,
    Dav,
}

impl DavNamespaceShort {
    pub fn as_str(self) -> &'static str {
        match self {
            DavNamespaceShort::Caldav => "c",
            DavNamespaceShort::Carddav => "card",
            DavNamespaceShort::CalendarServer => "cs",
            DavNamespaceShort::CaldavApple => "ca",
            DavNamespaceShort::Dav => "d",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IcalObjects {
    Vevent,
    Vtodo,
    Vjournal,
    Vfreebusy,
    Vtimezone,
    Valarm,
}

impl IcalObjects {
    pub fn as_str(self) -> &'static str {
        match self {
            IcalObjects::Vevent => "VEVENT",
            IcalObjects::Vtodo => "VTODO",
            IcalObjects::Vjournal => "VJOURNAL",
            IcalObjects::Vfreebusy => "VFREEBUSY",
            IcalObjects::Vtimezone => "VTIMEZONE",
            IcalObjects::Valarm => "VALARM",
        }
    }

    pub fn all() -> [IcalObjects; 6] {
        [
            IcalObjects::Vevent,
            IcalObjects::Vtodo,
            IcalObjects::Vjournal,
            IcalObjects::Vfreebusy,
            IcalObjects::Vtimezone,
            IcalObjects::Valarm,
        ]
    }
}
