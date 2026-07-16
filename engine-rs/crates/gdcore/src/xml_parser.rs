//! A minimal pull/streaming XML parser mirroring Godot's `XMLParser`.
//!
//! Call [`XmlParser::read`] to advance to the next node, then query
//! [`node_type`](XmlParser::node_type), [`node_name`](XmlParser::node_name),
//! [`node_data`](XmlParser::node_data), and the attribute accessors. Handles
//! elements (with quoted/unquoted attributes and self-closing `/>`), closing
//! tags, character data, comments, CDATA, and skips `<?…?>` / `<!…>` as
//! `Unknown`. Common entities (`&lt; &gt; &amp; &quot; &apos;`) are decoded in
//! text and attribute values.

/// The kind of node the parser is currently positioned on
/// (Godot `XMLParser.NodeType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum XmlNodeType {
    /// No node — before the first read or at end of document.
    #[default]
    None,
    /// An opening element `<tag ...>` (also self-closing `<tag/>`; see
    /// [`XmlParser::is_empty`]).
    Element,
    /// A closing element `</tag>`.
    ElementEnd,
    /// Character data between elements.
    Text,
    /// A `<!-- comment -->`.
    Comment,
    /// A `<![CDATA[ ... ]]>` section.
    CData,
    /// An unrecognized construct (`<?xml?>` declaration, `<!DOCTYPE>`, …).
    Unknown,
}

/// A pull XML parser (Godot `XMLParser`).
#[derive(Debug, Clone)]
pub struct XmlParser {
    chars: Vec<char>,
    pos: usize,
    node_type: XmlNodeType,
    node_name: String,
    node_data: String,
    attributes: Vec<(String, String)>,
    empty: bool,
}

impl XmlParser {
    /// Creates a parser over `source`, positioned before the first node.
    pub fn new(source: &str) -> Self {
        Self {
            chars: source.chars().collect(),
            pos: 0,
            node_type: XmlNodeType::None,
            node_name: String::new(),
            node_data: String::new(),
            attributes: Vec::new(),
            empty: false,
        }
    }

    /// Advances to the next node. Returns `true` if a node was read, or `false`
    /// at end of document (Godot `XMLParser.read`).
    pub fn read(&mut self) -> bool {
        self.node_name.clear();
        self.node_data.clear();
        self.attributes.clear();
        self.empty = false;

        if self.pos >= self.chars.len() {
            self.node_type = XmlNodeType::None;
            return false;
        }

        if self.chars[self.pos] == '<' {
            self.read_tag();
        } else {
            self.read_text();
        }
        true
    }

    /// The type of the current node.
    pub fn node_type(&self) -> XmlNodeType {
        self.node_type
    }

    /// The current element / closing-element name (empty for text/comment/etc.).
    pub fn node_name(&self) -> &str {
        &self.node_name
    }

    /// The current node's character data (text / comment / CDATA content).
    pub fn node_data(&self) -> &str {
        &self.node_data
    }

    /// Whether the current element is self-closing (`<tag/>`).
    pub fn is_empty(&self) -> bool {
        self.empty
    }

    /// Number of attributes on the current element.
    pub fn attribute_count(&self) -> usize {
        self.attributes.len()
    }

    /// The name of the `i`-th attribute, if present.
    pub fn attribute_name(&self, i: usize) -> Option<&str> {
        self.attributes.get(i).map(|(n, _)| n.as_str())
    }

    /// The value of the `i`-th attribute, if present.
    pub fn attribute_value(&self, i: usize) -> Option<&str> {
        self.attributes.get(i).map(|(_, v)| v.as_str())
    }

    /// Whether the current element has an attribute named `name`.
    pub fn has_attribute(&self, name: &str) -> bool {
        self.attributes.iter().any(|(n, _)| n.as_str() == name)
    }

    /// The value of the attribute named `name`, if present
    /// (Godot `get_named_attribute_value`).
    pub fn named_attribute_value(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(n, _)| n.as_str() == name)
            .map(|(_, v)| v.as_str())
    }

    // -- internals ----------------------------------------------------------

    fn peek(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    /// Whether the input at the cursor begins with `s`.
    fn matches(&self, s: &str) -> bool {
        s.chars()
            .enumerate()
            .all(|(i, c)| self.chars.get(self.pos + i) == Some(&c))
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.chars.get(self.pos), Some(c) if c.is_whitespace()) {
            self.pos += 1;
        }
    }

    /// Consumes input up to and including `marker`, returning the text before
    /// it. If `marker` is never found, consumes the rest of the input.
    fn take_until(&mut self, marker: &str) -> String {
        let mm: Vec<char> = marker.chars().collect();
        let start = self.pos;
        while self.pos < self.chars.len() {
            if self.pos + mm.len() <= self.chars.len()
                && self.chars[self.pos..self.pos + mm.len()] == mm[..]
            {
                let content: String = self.chars[start..self.pos].iter().collect();
                self.pos += mm.len();
                return content;
            }
            self.pos += 1;
        }
        self.chars[start..].iter().collect()
    }

    fn read_name(&mut self) -> String {
        let start = self.pos;
        while matches!(self.chars.get(self.pos),
            Some(&c) if c.is_alphanumeric() || matches!(c, '_' | '-' | ':' | '.'))
        {
            self.pos += 1;
        }
        self.chars[start..self.pos].iter().collect()
    }

    fn read_text(&mut self) {
        let start = self.pos;
        while matches!(self.chars.get(self.pos), Some(&c) if c != '<') {
            self.pos += 1;
        }
        let raw: String = self.chars[start..self.pos].iter().collect();
        self.node_data = unescape_xml(&raw);
        self.node_type = XmlNodeType::Text;
    }

    fn read_tag(&mut self) {
        if self.matches("<!--") {
            self.pos += 4;
            self.node_data = self.take_until("-->");
            self.node_type = XmlNodeType::Comment;
            return;
        }
        if self.matches("<![CDATA[") {
            self.pos += 9;
            self.node_data = self.take_until("]]>");
            self.node_type = XmlNodeType::CData;
            return;
        }
        if self.matches("<?") {
            self.pos += 2;
            self.take_until("?>");
            self.node_type = XmlNodeType::Unknown;
            return;
        }
        if self.matches("<!") {
            self.pos += 2;
            self.take_until(">");
            self.node_type = XmlNodeType::Unknown;
            return;
        }
        if self.matches("</") {
            self.pos += 2;
            self.skip_whitespace();
            self.node_name = self.read_name();
            self.take_until(">");
            self.node_type = XmlNodeType::ElementEnd;
            return;
        }
        // Opening element: `<name attr=...>` or `<name .../>`.
        self.pos += 1; // consume '<'
        self.skip_whitespace();
        self.node_name = self.read_name();
        self.read_attributes();
        self.node_type = XmlNodeType::Element;
    }

    fn read_attributes(&mut self) {
        loop {
            self.skip_whitespace();
            match self.chars.get(self.pos).copied() {
                None => break,
                Some('>') => {
                    self.pos += 1;
                    break;
                }
                Some('/') => {
                    if self.peek(1) == Some('>') {
                        self.empty = true;
                        self.pos += 2;
                    } else {
                        self.pos += 1;
                    }
                    break;
                }
                Some(c) if c.is_alphanumeric() || matches!(c, '_' | ':') => {
                    let name = self.read_name();
                    self.skip_whitespace();
                    let mut value = String::new();
                    if self.chars.get(self.pos) == Some(&'=') {
                        self.pos += 1;
                        self.skip_whitespace();
                        value = self.read_attr_value();
                    }
                    self.attributes.push((name, value));
                }
                Some(_) => self.pos += 1, // skip an unexpected char
            }
        }
    }

    fn read_attr_value(&mut self) -> String {
        match self.chars.get(self.pos).copied() {
            Some(q @ ('"' | '\'')) => {
                self.pos += 1; // opening quote
                let start = self.pos;
                while matches!(self.chars.get(self.pos), Some(&c) if c != q) {
                    self.pos += 1;
                }
                let raw: String = self.chars[start..self.pos].iter().collect();
                if self.chars.get(self.pos) == Some(&q) {
                    self.pos += 1; // closing quote
                }
                unescape_xml(&raw)
            }
            _ => {
                let start = self.pos;
                while matches!(self.chars.get(self.pos),
                    Some(&c) if !c.is_whitespace() && c != '>' && c != '/')
                {
                    self.pos += 1;
                }
                let raw: String = self.chars[start..self.pos].iter().collect();
                unescape_xml(&raw)
            }
        }
    }
}

/// Decodes the common XML entities. `&amp;` is resolved last so that
/// double-encoded input (`&amp;lt;` → `&lt;`) survives correctly.
fn unescape_xml(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_parser_elements_attrs_text() {
        let src = r#"<?xml version="1.0"?>
<root count="2">
  <item id="1" name="first">Hello</item>
  <!-- a comment -->
  <empty flag="yes"/>
</root>"#;
        let mut p = XmlParser::new(src);
        let mut opens: Vec<(String, bool)> = Vec::new();
        let mut text_seen = String::new();
        while p.read() {
            match p.node_type() {
                XmlNodeType::Element => {
                    opens.push((p.node_name().to_string(), p.is_empty()));
                    match p.node_name() {
                        "root" => assert_eq!(p.named_attribute_value("count"), Some("2")),
                        "item" => {
                            assert_eq!(p.attribute_count(), 2);
                            assert_eq!(p.named_attribute_value("id"), Some("1"));
                            assert_eq!(p.named_attribute_value("name"), Some("first"));
                            assert!(p.has_attribute("name"));
                            assert!(!p.has_attribute("nope"));
                        }
                        "empty" => {
                            assert!(p.is_empty(), "self-closing element must be empty");
                            assert_eq!(p.named_attribute_value("flag"), Some("yes"));
                        }
                        _ => {}
                    }
                }
                XmlNodeType::Text => {
                    let t = p.node_data().trim();
                    if !t.is_empty() {
                        text_seen.push_str(t);
                    }
                }
                _ => {}
            }
        }
        assert!(opens.iter().any(|(n, _)| n == "root"));
        assert!(opens.iter().any(|(n, e)| n == "item" && !*e));
        assert!(opens.iter().any(|(n, e)| n == "empty" && *e));
        assert_eq!(text_seen, "Hello");
    }

    #[test]
    fn xml_parser_entities_comment_and_eof() {
        let src = "<a b=\"x&amp;y\">1 &lt; 2</a><!--c-->";
        let mut p = XmlParser::new(src);

        assert!(p.read()); // <a b="x&amp;y">
        assert_eq!(p.node_type(), XmlNodeType::Element);
        assert_eq!(p.node_name(), "a");
        assert_eq!(p.named_attribute_value("b"), Some("x&y")); // &amp; decoded

        assert!(p.read()); // "1 &lt; 2"
        assert_eq!(p.node_type(), XmlNodeType::Text);
        assert_eq!(p.node_data(), "1 < 2"); // &lt; decoded

        assert!(p.read()); // </a>
        assert_eq!(p.node_type(), XmlNodeType::ElementEnd);
        assert_eq!(p.node_name(), "a");

        assert!(p.read()); // <!--c-->
        assert_eq!(p.node_type(), XmlNodeType::Comment);
        assert_eq!(p.node_data(), "c");

        assert!(!p.read()); // end of document
        assert_eq!(p.node_type(), XmlNodeType::None);
    }
}
