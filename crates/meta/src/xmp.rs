//! XMP packets (embedded or a sidecar): ratings, keywords, hierarchies, people and face regions, dates. Properties are
//! found by namespace URI, so any prefix works, in element or attribute form; values come out as exiftool returns
//! them (`-api struct=1`).

use crate::rules::Tags;
use quick_xml::events::{BytesStart, Event};
use serde_json::{Map, Value, json};

const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const DC: &str = "http://purl.org/dc/elements/1.1/";
const XMP: &str = "http://ns.adobe.com/xap/1.0/";
const LR: &str = "http://ns.adobe.com/lightroom/1.0/";
const EXIF: &str = "http://ns.adobe.com/exif/1.0/";
const TIFF: &str = "http://ns.adobe.com/tiff/1.0/";
const PHOTOSHOP: &str = "http://ns.adobe.com/photoshop/1.0/";
const IPTC_EXT: &str = "http://iptc.org/std/Iptc4xmpExt/2008-02-29/";
const MWG_RS: &str = "http://www.metadataworkinggroup.com/schemas/regions/";
const MP: &str = "http://ns.microsoft.com/photo/1.2/";
const MPRI: &str = "http://ns.microsoft.com/photo/1.2/t/RegionInfo#";
const MPREG: &str = "http://ns.microsoft.com/photo/1.2/t/Region#";

/// Nesting deeper than this is hostile (REQ-011-10); such a packet is read up to there.
const MAX_DEPTH: usize = 64;
/// Values per property kept at most.
const MAX_VALUES: usize = 1000;

/// A parsed element tree, just enough of RDF for the properties read.
#[derive(Debug, Default)]
struct Node {
    ns: String,
    name: String,
    attrs: Vec<(String, String, String)>,
    text: String,
    children: Vec<Node>,
}

impl Node {
    fn is(&self, ns: &str, name: &str) -> bool {
        self.ns == ns && self.name == name
    }

    /// Simple values of a property: its text, or the `rdf:li` items of an `rdf:Bag` / `Seq` / `Alt`.
    fn values(&self) -> Vec<String> {
        let containers: Vec<&Node> = self.children.iter().filter(|c| c.ns == RDF).collect();
        if containers.is_empty() {
            let text = self.text.trim();
            return if text.is_empty() {
                Vec::new()
            } else {
                vec![text.to_string()]
            };
        }
        containers
            .iter()
            .flat_map(|container| container.children.iter().filter(|li| li.is(RDF, "li")))
            .map(|li| li.text.trim().to_string())
            .filter(|text| !text.is_empty())
            .take(MAX_VALUES)
            .collect()
    }

    /// A struct's field: a child, or a child of its `rdf:Description`.
    fn find_field(&self, ns: &str, name: &str) -> Option<&Node> {
        for child in &self.children {
            if child.is(ns, name) {
                return Some(child);
            }
            if child.is(RDF, "Description")
                && let Some(found) = child.children.iter().find(|c| c.is(ns, name))
            {
                return Some(found);
            }
        }
        None
    }

    /// A struct field's text, also from an attribute of the struct (`<rdf:Description mwg-rs:Name="Anna"/>`).
    fn field_text(&self, ns: &str, name: &str) -> Option<String> {
        let attr = |node: &Node| {
            node.attrs
                .iter()
                .find(|(n, local, _)| n == ns && local == name)
                .map(|(_, _, value)| value.clone())
        };
        attr(self)
            .or_else(|| self.children.iter().filter(|c| c.is(RDF, "Description")).find_map(attr))
            .or_else(|| {
                self.find_field(ns, name)
                    .and_then(|node| node.values().into_iter().next())
            })
    }

    /// The `rdf:li` items of the list property `ns:name` inside this struct.
    fn list_items(&self, ns: &str, name: &str) -> Vec<&Node> {
        self.find_field(ns, name)
            .into_iter()
            .flat_map(|list| list.children.iter().filter(|c| c.ns == RDF))
            .flat_map(|container| container.children.iter().filter(|li| li.is(RDF, "li")))
            .take(MAX_VALUES)
            .collect()
    }
}

/// Parses a packet; a broken one yields what was read before the error.
fn parse(text: &str) -> Node {
    let mut reader = quick_xml::Reader::from_str(text);
    // Namespace declarations by prefix, innermost last.
    let mut scopes: Vec<Vec<(String, String)>> =
        vec![vec![("xml".into(), "http://www.w3.org/XML/1998/namespace".into())]];
    let mut stack = vec![Node::default()];
    let resolve = |scopes: &[Vec<(String, String)>], prefix: &str| {
        scopes
            .iter()
            .rev()
            .flat_map(|scope| scope.iter().rev())
            .find(|(p, _)| p == prefix)
            .map(|(_, uri)| uri.clone())
            .unwrap_or_default()
    };
    let open = |start: &BytesStart, scopes: &mut Vec<Vec<(String, String)>>| {
        let mut scope = Vec::new();
        let mut plain = Vec::new();
        for attr in start.attributes().flatten() {
            let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
            let value = attr
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map(|v| v.into_owned())
                .unwrap_or_default();
            if key == "xmlns" {
                scope.push((String::new(), value));
            } else if let Some(prefix) = key.strip_prefix("xmlns:") {
                scope.push((prefix.to_string(), value));
            } else {
                plain.push((key, value));
            }
        }
        scopes.push(scope);
        let split = |qname: &str| match qname.split_once(':') {
            Some((prefix, local)) => (prefix.to_string(), local.to_string()),
            None => (String::new(), qname.to_string()),
        };
        let (prefix, name) = split(&String::from_utf8_lossy(start.name().as_ref()));
        let attrs = plain
            .into_iter()
            .map(|(key, value)| {
                let (prefix, local) = split(&key);
                (resolve(scopes, &prefix), local, value)
            })
            .collect();
        Node {
            ns: resolve(scopes, &prefix),
            name,
            attrs,
            ..Node::default()
        }
    };
    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) => {
                if stack.len() > MAX_DEPTH {
                    break;
                }
                let node = open(&start, &mut scopes);
                stack.push(node);
            }
            Ok(Event::Empty(start)) => {
                let node = open(&start, &mut scopes);
                scopes.pop();
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                }
            }
            Ok(Event::End(_)) => {
                if stack.len() <= 1 {
                    break;
                }
                scopes.pop();
                let node = stack.pop().expect("checked above");
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                }
            }
            Ok(Event::Text(text)) => {
                if let (Some(node), Ok(text)) = (stack.last_mut(), text.decode()) {
                    node.text.push_str(&text);
                }
            }
            Ok(Event::CData(data)) => {
                if let (Some(node), Ok(text)) = (stack.last_mut(), data.decode()) {
                    node.text.push_str(&text);
                }
            }
            Ok(Event::GeneralRef(reference)) => {
                let resolved = match reference.resolve_char_ref() {
                    Ok(Some(c)) => Some(c.to_string()),
                    _ => reference.decode().ok().and_then(|name| match name.as_ref() {
                        "amp" => Some("&".into()),
                        "lt" => Some("<".into()),
                        "gt" => Some(">".into()),
                        "quot" => Some("\"".into()),
                        "apos" => Some("'".into()),
                        _ => None,
                    }),
                };
                if let (Some(node), Some(text)) = (stack.last_mut(), resolved) {
                    node.text.push_str(&text);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            Ok(_) => {}
        }
    }
    // Close what a broken packet left open.
    while stack.len() > 1 {
        let node = stack.pop().expect("checked by the loop");
        stack.last_mut().expect("the root stays").children.push(node);
    }
    stack.pop().unwrap_or_default()
}

/// Every property of every `rdf:Description`, in element and attribute form.
fn properties(root: &Node) -> (Vec<&Node>, Vec<(&str, &str, &str)>) {
    let mut elements = Vec::new();
    let mut attributes = Vec::new();
    fn walk<'a>(node: &'a Node, elements: &mut Vec<&'a Node>, attributes: &mut Vec<(&'a str, &'a str, &'a str)>) {
        for child in &node.children {
            if child.is(RDF, "Description") {
                elements.extend(child.children.iter());
                attributes.extend(
                    child
                        .attrs
                        .iter()
                        .filter(|(ns, _, _)| ns != RDF)
                        .map(|(ns, name, value)| (ns.as_str(), name.as_str(), value.as_str())),
                );
            } else {
                walk(child, elements, attributes);
            }
        }
    }
    walk(root, &mut elements, &mut attributes);
    (elements, attributes)
}

/// XMP dates (`2024-07-02T07:12:59.12+02:00`) as exiftool prints them (`2024:07:02 07:12:59.12+02:00`).
fn exif_date(text: &str) -> String {
    let text = text.trim();
    match text.split_once('T') {
        Some((date, time)) => format!("{} {time}", date.replace('-', ":")),
        None => text.replace('-', ":"),
    }
}

fn number_or_text(text: &str) -> Value {
    match text.trim().parse::<f64>() {
        Ok(n) if n.is_finite() && n.fract() == 0.0 => json!(n as i64),
        Ok(n) if n.is_finite() => json!(n),
        _ => Value::from(text.trim()),
    }
}

/// Reads the properties spec 004 uses from an XMP packet into `tags`, keeping values already there.
pub fn read(packet: &[u8], tags: &mut Tags) {
    let text = String::from_utf8_lossy(packet);
    // The packet wrapper and anything before it isn't XML the reader needs.
    let start = text.find("<x:xmpmeta").or_else(|| text.find("<rdf:RDF")).unwrap_or(0);
    let root = parse(&text[start..]);
    let (elements, attributes) = properties(&root);
    let mut found = Tags::new();
    let mut put = |name: &str, value: Value| {
        found.entry(name.to_string()).or_insert(value);
    };
    let list = |values: Vec<String>| match values.len() {
        0 => None,
        1 => Some(Value::from(values.into_iter().next().expect("one value"))),
        _ => Some(Value::from(values)),
    };
    for (ns, name, value) in &attributes {
        match (*ns, *name) {
            (XMP, "Rating") => put("Rating", number_or_text(value)),
            (EXIF, "DateTimeOriginal") => put("DateTimeOriginal", Value::from(exif_date(value))),
            (XMP, "CreateDate") => put("CreateDate", Value::from(exif_date(value))),
            (PHOTOSHOP, "DateCreated") => put("DateCreated", Value::from(exif_date(value))),
            (TIFF, "Orientation") => put("Orientation", number_or_text(value)),
            _ => {}
        }
    }
    for node in &elements {
        match (node.ns.as_str(), node.name.as_str()) {
            (XMP, "Rating") => {
                if let Some(value) = node.values().first() {
                    put("Rating", number_or_text(value));
                }
            }
            (DC, "subject") => {
                if let Some(value) = list(node.values()) {
                    put("Subject", value);
                }
            }
            (LR, "hierarchicalSubject") => {
                if let Some(value) = list(node.values()) {
                    put("HierarchicalSubject", value);
                }
            }
            (IPTC_EXT, "PersonInImage") => {
                if let Some(value) = list(node.values()) {
                    put("PersonInImage", value);
                }
            }
            (EXIF, "DateTimeOriginal") | (XMP, "CreateDate") | (PHOTOSHOP, "DateCreated") => {
                let name = match node.name.as_str() {
                    "DateTimeOriginal" => "DateTimeOriginal",
                    "CreateDate" => "CreateDate",
                    _ => "DateCreated",
                };
                if let Some(value) = node.values().first() {
                    put(name, Value::from(exif_date(value)));
                }
            }
            (TIFF, "Orientation") => {
                if let Some(value) = node.values().first() {
                    put("Orientation", number_or_text(value));
                }
            }
            (MWG_RS, "Regions") => {
                let regions: Vec<Value> = node
                    .list_items(MWG_RS, "RegionList")
                    .into_iter()
                    .map(|li| {
                        let mut region = Map::new();
                        if let Some(name) = li.field_text(MWG_RS, "Name") {
                            region.insert("Name".into(), Value::from(name));
                        }
                        if let Some(kind) = li.field_text(MWG_RS, "Type") {
                            region.insert("Type".into(), Value::from(kind));
                        }
                        Value::Object(region)
                    })
                    .collect();
                if !regions.is_empty() {
                    put("RegionInfo", json!({ "RegionList": regions }));
                }
            }
            (MP, "RegionInfo") => {
                let regions: Vec<Value> = node
                    .list_items(MPRI, "Regions")
                    .into_iter()
                    .filter_map(|li| li.field_text(MPREG, "PersonDisplayName"))
                    .map(|name| json!({ "PersonDisplayName": name }))
                    .collect();
                if !regions.is_empty() {
                    put("RegionInfoMP", json!({ "Regions": regions }));
                }
            }
            _ => {}
        }
    }
    for (name, value) in found {
        tags.entry(name).or_insert(value);
    }
}

/// The XMP packet inside `data`, if there is one (`<x:xmpmeta …</x:xmpmeta>`): MP4 boxes and GIF blocks hold one.
pub fn find_packet(data: &[u8]) -> Option<&[u8]> {
    let start = find(data, b"<x:xmpmeta")?;
    let end = find(&data[start..], b"</x:xmpmeta>")? + start + b"</x:xmpmeta>".len();
    Some(&data[start..end])
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(packet: &str) -> Tags {
        let mut tags = Tags::new();
        read(packet.as_bytes(), &mut tags);
        tags
    }

    const LIGHTROOM: &str = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
 <rdf:Description rdf:about="" xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmlns:dc="http://purl.org/dc/elements/1.1/"
   xmlns:lr="http://ns.adobe.com/lightroom/1.0/" xmlns:exif="http://ns.adobe.com/exif/1.0/"
   xmp:Rating="4" exif:DateTimeOriginal="2024-07-02T07:12:59.12+02:00">
  <dc:subject><rdf:Bag><rdf:li>Holiday</rdf:li><rdf:li>Rock &amp; Roll</rdf:li></rdf:Bag></dc:subject>
  <lr:hierarchicalSubject><rdf:Bag><rdf:li>Places|France|Paris</rdf:li></rdf:Bag></lr:hierarchicalSubject>
 </rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#;

    #[test]
    fn req_019_04_reads_ratings_keywords_hierarchies_and_dates() {
        let tags = tags(LIGHTROOM);
        assert_eq!(tags["Rating"], json!(4));
        assert_eq!(tags["Subject"], json!(["Holiday", "Rock & Roll"]));
        assert_eq!(tags["HierarchicalSubject"], json!("Places|France|Paris"));
        assert_eq!(tags["DateTimeOriginal"], json!("2024:07:02 07:12:59.12+02:00"));
    }

    #[test]
    fn req_019_04_reads_face_regions_of_mwg_and_microsoft_and_people() {
        let tags = tags(
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
 <rdf:Description xmlns:mwg-rs="http://www.metadataworkinggroup.com/schemas/regions/"
   xmlns:stArea="http://ns.adobe.com/xmp/sType/Area#" xmlns:MP="http://ns.microsoft.com/photo/1.2/"
   xmlns:MPRI="http://ns.microsoft.com/photo/1.2/t/RegionInfo#" xmlns:MPReg="http://ns.microsoft.com/photo/1.2/t/Region#"
   xmlns:Iptc4xmpExt="http://iptc.org/std/Iptc4xmpExt/2008-02-29/">
  <mwg-rs:Regions rdf:parseType="Resource"><mwg-rs:RegionList><rdf:Bag>
   <rdf:li><rdf:Description mwg-rs:Name="Anna" mwg-rs:Type="Face"/></rdf:li>
   <rdf:li rdf:parseType="Resource"><mwg-rs:Name>Rex</mwg-rs:Name><mwg-rs:Type>Pet</mwg-rs:Type></rdf:li>
  </rdf:Bag></mwg-rs:RegionList></mwg-rs:Regions>
  <MP:RegionInfo rdf:parseType="Resource"><MPRI:Regions><rdf:Bag>
   <rdf:li MPReg:PersonDisplayName="Ben"/>
  </rdf:Bag></MPRI:Regions></MP:RegionInfo>
  <Iptc4xmpExt:PersonInImage><rdf:Bag><rdf:li>Cleo</rdf:li></rdf:Bag></Iptc4xmpExt:PersonInImage>
 </rdf:Description></rdf:RDF></x:xmpmeta>"#,
        );
        assert_eq!(
            tags["RegionInfo"],
            json!({ "RegionList": [{ "Name": "Anna", "Type": "Face" }, { "Name": "Rex", "Type": "Pet" }] })
        );
        assert_eq!(
            tags["RegionInfoMP"],
            json!({ "Regions": [{ "PersonDisplayName": "Ben" }] })
        );
        assert_eq!(tags["PersonInImage"], json!("Cleo"));
    }

    /// REQ-011-10: broken, deep and huge packets don't panic.
    #[test]
    fn req_011_10_survives_hostile_packets() {
        for cut in 0..LIGHTROOM.len() {
            if LIGHTROOM.is_char_boundary(cut) {
                tags(&LIGHTROOM[..cut]);
            }
        }
        let deep = "<a>".repeat(10_000);
        tags(&deep);
        let many = format!(
            r#"<rdf:RDF xmlns:rdf="{RDF}"><rdf:Description xmlns:dc="{DC}"><dc:subject><rdf:Bag>{}</rdf:Bag></dc:subject></rdf:Description></rdf:RDF>"#,
            "<rdf:li>x</rdf:li>".repeat(5000)
        );
        assert_eq!(tags(&many)["Subject"].as_array().unwrap().len(), MAX_VALUES);
    }
}
