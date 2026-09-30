use crate::consts::DavNamespaceShort;
use crate::error::DavError;
use crate::util::native_type::native_type;
use quick_xml::events::{BytesCData, BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::reader::Reader;
use quick_xml::writer::Writer;
use serde_json::{Map, Value};
use std::io::Cursor;

fn apply_namespace(name: &str, namespace: Option<DavNamespaceShort>) -> String {
    match namespace {
        Some(ns) if !name.contains(':') => format!("{}:{}", ns.as_str(), name),
        _ => name.to_string(),
    }
}

fn io_err(e: std::io::Error) -> DavError {
    DavError::XmlWrite(e.to_string())
}

pub fn js_to_xml(body: &Map<String, Value>, namespace: Option<DavNamespaceShort>) -> Result<String, DavError> {
    let mut writer = Writer::new_with_indent(Cursor::new(Vec::new()), b' ', 2);
    writer
        .write_event(Event::Decl(BytesDecl::new("1.0", Some("utf-8"), None)))
        .map_err(io_err)?;
    for (key, value) in body.iter() {
        write_element(&mut writer, key, value, namespace)?;
    }
    let bytes = writer.into_inner().into_inner();
    String::from_utf8(bytes).map_err(|e| DavError::XmlWrite(e.to_string()))
}

fn write_element<W: std::io::Write>(
    writer: &mut Writer<W>,
    name: &str,
    value: &Value,
    namespace: Option<DavNamespaceShort>,
) -> Result<(), DavError> {
    match value {
        Value::Array(items) => {
            for item in items {
                write_element(writer, name, item, namespace)?;
            }
            Ok(())
        }
        Value::Object(map) => write_object_element(writer, name, map, namespace),
        Value::Null => Ok(()),
        Value::String(s) => write_leaf(writer, name, s, namespace),
        Value::Bool(b) => write_leaf(writer, name, &b.to_string(), namespace),
        Value::Number(n) => write_leaf(writer, name, &n.to_string(), namespace),
    }
}

fn write_leaf<W: std::io::Write>(
    writer: &mut Writer<W>,
    name: &str,
    text: &str,
    namespace: Option<DavNamespaceShort>,
) -> Result<(), DavError> {
    let tag = apply_namespace(name, namespace);
    writer
        .write_event(Event::Start(BytesStart::new(tag.clone())))
        .map_err(io_err)?;
    writer
        .write_event(Event::Text(BytesText::new(text)))
        .map_err(io_err)?;
    writer
        .write_event(Event::End(BytesEnd::new(tag)))
        .map_err(io_err)?;
    Ok(())
}

fn write_object_element<W: std::io::Write>(
    writer: &mut Writer<W>,
    name: &str,
    map: &Map<String, Value>,
    namespace: Option<DavNamespaceShort>,
) -> Result<(), DavError> {
    let tag = apply_namespace(name, namespace);
    let mut start = BytesStart::new(tag.clone());
    if let Some(Value::Object(attrs)) = map.get("_attributes") {
        for (k, v) in attrs {
            if let Some(s) = v.as_str() {
                start.push_attribute((k.as_str(), s));
            }
        }
    }
    let text = map.get("_text").and_then(value_as_text);
    let cdata = map.get("_cdata").and_then(|v| v.as_str());
    let children: Vec<(&String, &Value)> =
        map.iter().filter(|(k, _)| !k.starts_with('_')).collect();

    if children.is_empty() && text.is_none() && cdata.is_none() {
        writer.write_event(Event::Empty(start)).map_err(io_err)?;
        return Ok(());
    }

    writer
        .write_event(Event::Start(start))
        .map_err(io_err)?;
    if let Some(t) = &text {
        writer
            .write_event(Event::Text(BytesText::new(t)))
            .map_err(io_err)?;
    }
    if let Some(c) = cdata {
        writer
            .write_event(Event::CData(BytesCData::new(c)))
            .map_err(io_err)?;
    }
    for (k, v) in children {
        write_element(writer, k, v, namespace)?;
    }
    writer
        .write_event(Event::End(BytesEnd::new(tag)))
        .map_err(io_err)?;
    Ok(())
}

fn value_as_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

#[derive(Default)]
struct Frame {
    attributes: Vec<(String, String)>,
    children: Vec<(String, Value)>,
    text: String,
    cdata: Option<String>,
}

impl Frame {
    fn finalize(self) -> Value {
        let has_children = !self.children.is_empty();
        let has_attrs = !self.attributes.is_empty();
        let trimmed_text = self.text.trim();

        if !has_children && !has_attrs {
            if let Some(cdata) = self.cdata {
                let mut map = Map::new();
                map.insert("_cdata".to_string(), Value::String(cdata));
                return Value::Object(map);
            }
            if !trimmed_text.is_empty() {
                return native_type(trimmed_text);
            }
            return Value::Object(Map::new());
        }

        let mut map = Map::new();
        if has_attrs {
            let mut attr_map = Map::new();
            for (k, v) in self.attributes {
                if k != "xmlns" {
                    attr_map.insert(k, Value::String(v));
                }
            }
            if !attr_map.is_empty() {
                map.insert("_attributes".to_string(), Value::Object(attr_map));
            }
        }
        if !trimmed_text.is_empty() {
            map.insert("_text".to_string(), native_type(trimmed_text));
        }
        if let Some(cdata) = self.cdata {
            map.insert("_cdata".to_string(), Value::String(cdata));
        }
        for (name, value) in self.children {
            match map.get_mut(&name) {
                None => {
                    map.insert(name, value);
                }
                Some(Value::Array(arr)) => arr.push(value),
                Some(_) => {
                    let prev = map.remove(&name).unwrap();
                    map.insert(name, Value::Array(vec![prev, value]));
                }
            }
        }
        Value::Object(map)
    }
}

fn element_name(raw: &str) -> String {
    let stripped = match raw.rfind(':') {
        Some(idx) => &raw[idx + 1..],
        None => raw,
    };
    crate::util::camel_case::camel_case(stripped)
}

pub fn xml_to_js(xml_text: &str) -> Result<Value, DavError> {
    let mut reader = Reader::from_str(xml_text);
    reader.config_mut().trim_text(false);

    let mut stack: Vec<(String, Frame)> = Vec::new();
    let mut root: Option<(String, Value)> = None;

    loop {
        let event = reader
            .read_event()
            .map_err(|e| DavError::XmlParse(e.to_string()))?;
        match event {
            Event::Eof => break,
            Event::Decl(_) | Event::PI(_) | Event::DocType(_) | Event::Comment(_) => {}
            Event::GeneralRef(_) => {}
            Event::Start(start) => {
                let raw_name = start.name().as_ref().to_string();
                let mut frame = Frame::default();
                for attr in start.attributes().flatten() {
                    let key = attr.key.as_ref().to_string();
                    let value = attr
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .map_err(|e| DavError::XmlParse(e.to_string()))?
                        .to_string();
                    frame.attributes.push((key, value));
                }
                stack.push((element_name(&raw_name), frame));
            }
            Event::Empty(start) => {
                let raw_name = start.name().as_ref().to_string();
                let mut frame = Frame::default();
                for attr in start.attributes().flatten() {
                    let key = attr.key.as_ref().to_string();
                    let value = attr
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .map_err(|e| DavError::XmlParse(e.to_string()))?
                        .to_string();
                    frame.attributes.push((key, value));
                }
                let name = element_name(&raw_name);
                let value = frame.finalize();
                push_child(&mut stack, &mut root, name, value);
            }
            Event::Text(text) => {
                let raw = text.as_ref().to_string();
                let decoded = quick_xml::escape::unescape(&raw)
                    .map(|c| c.to_string())
                    .unwrap_or(raw);
                if let Some((_, frame)) = stack.last_mut() {
                    frame.text.push_str(&decoded);
                }
            }
            Event::CData(cdata) => {
                let raw = cdata.as_ref().to_string();
                if let Some((_, frame)) = stack.last_mut() {
                    frame.cdata.get_or_insert_with(String::new).push_str(&raw);
                }
            }
            Event::End(_) => {
                if let Some((name, frame)) = stack.pop() {
                    let value = frame.finalize();
                    push_child(&mut stack, &mut root, name, value);
                }
            }
        }
    }

    let (name, value) = root.ok_or_else(|| DavError::XmlParse("empty XML document".into()))?;
    let mut out = Map::new();
    out.insert(name, value);
    Ok(Value::Object(out))
}

fn push_child(
    stack: &mut Vec<(String, Frame)>,
    root: &mut Option<(String, Value)>,
    name: String,
    value: Value,
) {
    if let Some((_, parent)) = stack.last_mut() {
        parent.children.push((name, value));
    } else {
        *root = Some((name, value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multistatus_with_repeated_responses_and_cdata() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav">
  <d:response>
    <d:href>/cal/1.ics</d:href>
    <d:propstat>
      <d:prop>
        <d:getetag>"abc123"</d:getetag>
        <cal:calendar-data><![CDATA[BEGIN:VCALENDAR
END:VCALENDAR]]></cal:calendar-data>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
  <d:response>
    <d:href>/cal/2.ics</d:href>
    <d:propstat>
      <d:prop>
        <d:getetag>"def456"</d:getetag>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;
        let parsed = xml_to_js(xml).unwrap();
        let responses = parsed["multistatus"]["response"].as_array().unwrap();
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0]["href"], "/cal/1.ics");
        assert_eq!(
            responses[0]["propstat"]["prop"]["calendarData"]["_cdata"],
            "BEGIN:VCALENDAR\nEND:VCALENDAR"
        );
        assert_eq!(responses[0]["propstat"]["prop"]["getetag"], "\"abc123\"");
        assert_eq!(responses[0]["propstat"]["status"], "HTTP/1.1 200 OK");
    }

    #[test]
    fn empty_elements_become_empty_objects_for_resourcetype_style_checks() {
        let xml = r#"<d:resourcetype xmlns:d="DAV:"><d:collection/><d:calendar xmlns:cal="urn:ietf:params:xml:ns:caldav"/></d:resourcetype>"#;
        let parsed = xml_to_js(xml).unwrap();
        let rt = parsed["resourcetype"].as_object().unwrap();
        assert!(rt.contains_key("collection"));
        assert!(rt.contains_key("calendar"));
    }

    #[test]
    fn serializes_propfind_with_namespace_prefixing_and_attributes() {
        let mut root = Map::new();
        let mut propfind = Map::new();
        let mut attrs = Map::new();
        attrs.insert("xmlns:d".to_string(), Value::String("DAV:".to_string()));
        propfind.insert("_attributes".to_string(), Value::Object(attrs));
        let mut prop = Map::new();
        prop.insert("current-user-principal".to_string(), Value::Object(Map::new()));
        propfind.insert("prop".to_string(), Value::Object(prop));
        root.insert("propfind".to_string(), Value::Object(propfind));

        let xml = js_to_xml(&root, Some(DavNamespaceShort::Dav)).unwrap();
        assert!(xml.contains("<?xml version=\"1.0\" encoding=\"utf-8\"?>"));
        assert!(xml.contains("<d:propfind"));
        assert!(xml.contains("xmlns:d=\"DAV:\""));
        assert!(xml.contains("<d:prop>"));
        assert!(xml.contains("<d:current-user-principal/>"));
    }
}
