//! A forgiving parser for the HTML of Vue templates: elements, attributes, text and
//! comments. `{{ … }}` in text is kept whole, so an expression may hold `<` or `>`.
//! Mismatched or missing end tags close what they can rather than fail.

/// A node of the template.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Element(Element),
    Text(String),
    Comment(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    /// The tag, lower-cased.
    pub tag: String,
    /// Attributes in order, with `None` for one without a value.
    pub attrs: Vec<(String, Option<String>)>,
    pub children: Vec<Node>,
}

impl Element {
    /// An attribute's value; `Some("")` for one without a value.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_deref().unwrap_or(""))
    }

    pub fn has_attr(&self, name: &str) -> bool {
        self.attrs.iter().any(|(n, _)| n == name)
    }
}

/// Elements that have no end tag.
const VOID: [&str; 14] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// Parses a template into its top-level nodes.
pub fn parse(html: &str) -> Vec<Node> {
    let mut parser = Parser { src: html, pos: 0 };
    // A stack of open elements; the bottom one holds the top-level nodes.
    let mut stack: Vec<Element> = vec![Element {
        tag: String::new(),
        attrs: Vec::new(),
        children: Vec::new(),
    }];
    while parser.pos < html.len() {
        let rest = parser.rest();
        if let Some(comment) = rest.strip_prefix("<!--") {
            let end = comment.find("-->").unwrap_or(comment.len());
            let text = comment[..end].trim().to_owned();
            parser.pos += (4 + end + 3).min(rest.len());
            push(&mut stack, Node::Comment(text));
        } else if rest.starts_with("</") {
            let end = rest.find('>').unwrap_or(rest.len());
            let tag = rest[2..end].trim().to_lowercase();
            parser.pos += (end + 1).min(rest.len());
            // Close up to the matching element, if it is open.
            if let Some(index) = stack.iter().rposition(|e| e.tag == tag)
                && index > 0
            {
                while stack.len() > index {
                    close(&mut stack);
                }
            }
        } else if rest.starts_with("<!") || rest.starts_with("<?") {
            let end = rest.find('>').unwrap_or(rest.len());
            parser.pos += (end + 1).min(rest.len());
        } else if rest.starts_with('<') && rest[1..].starts_with(|c: char| c.is_ascii_alphabetic())
        {
            let (element, self_closing) = parser.start_tag();
            let void = self_closing || VOID.contains(&element.tag.as_str());
            if void {
                push(&mut stack, Node::Element(element));
            } else {
                stack.push(element);
            }
        } else {
            let text = parser.text();
            push(&mut stack, Node::Text(text));
        }
    }
    while stack.len() > 1 {
        close(&mut stack);
    }
    stack.pop().map(|root| root.children).unwrap_or_default()
}

fn push(stack: &mut [Element], node: Node) {
    if let Some(top) = stack.last_mut() {
        top.children.push(node);
    }
}

fn close(stack: &mut Vec<Element>) {
    if let Some(element) = stack.pop() {
        push(stack, Node::Element(element));
    }
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
}

impl Parser<'_> {
    fn rest(&self) -> &str {
        &self.src[self.pos..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.bump();
        }
    }

    /// Text up to the next tag, with `{{ … }}` kept whole.
    fn text(&mut self) -> String {
        let start = self.pos;
        loop {
            let rest = self.rest();
            if rest.is_empty() {
                break;
            }
            if rest.starts_with("{{") {
                let end = rest.find("}}").map_or(rest.len(), |e| e + 2);
                self.pos += end;
                continue;
            }
            if rest.starts_with('<') && self.pos > start {
                let next = rest[1..].chars().next();
                if next.is_some_and(|c| c.is_ascii_alphabetic() || c == '/' || c == '!' || c == '?')
                {
                    break;
                }
            }
            self.bump();
        }
        decode_entities(&self.src[start..self.pos])
    }

    /// A start tag, after its `<`; and whether it closes itself.
    fn start_tag(&mut self) -> (Element, bool) {
        self.bump();
        let mut tag = String::new();
        while let Some(c) = self.peek() {
            if c.is_whitespace() || c == '>' || c == '/' {
                break;
            }
            tag.push(c);
            self.bump();
        }
        let mut attrs = Vec::new();
        let mut self_closing = false;
        loop {
            self.skip_space();
            match self.peek() {
                None => break,
                Some('>') => {
                    self.bump();
                    break;
                }
                Some('/') => {
                    self.bump();
                    if self.peek() == Some('>') {
                        self.bump();
                        self_closing = true;
                        break;
                    }
                }
                Some(_) => {
                    let mut name = String::new();
                    while let Some(c) = self.peek() {
                        if c.is_whitespace()
                            || c == '='
                            || c == '>'
                            || (c == '/' && name.is_empty())
                        {
                            break;
                        }
                        if c == '/' && self.rest()[1..].starts_with('>') {
                            break;
                        }
                        name.push(c);
                        self.bump();
                    }
                    self.skip_space();
                    let value = if self.peek() == Some('=') {
                        self.bump();
                        self.skip_space();
                        Some(self.attr_value())
                    } else {
                        None
                    };
                    if name.is_empty() {
                        self.bump();
                    } else {
                        attrs.push((name, value));
                    }
                }
            }
        }
        (
            Element {
                tag: tag.to_lowercase(),
                attrs,
                children: Vec::new(),
            },
            self_closing,
        )
    }

    fn attr_value(&mut self) -> String {
        match self.peek() {
            Some(q @ ('"' | '\'')) => {
                self.bump();
                let start = self.pos;
                while self.peek().is_some_and(|c| c != q) {
                    self.bump();
                }
                let value = self.src[start..self.pos].to_owned();
                self.bump();
                decode_entities(&value)
            }
            _ => {
                let start = self.pos;
                while self.peek().is_some_and(|c| !c.is_whitespace() && c != '>') {
                    self.bump();
                }
                decode_entities(&self.src[start..self.pos])
            }
        }
    }
}

/// The character references a template is likely to use.
fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest.find(';').filter(|&e| e <= 10);
        let decoded = end.and_then(|e| {
            let name = &rest[1..e];
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                "copy" => Some('©'),
                "reg" => Some('®'),
                "times" => Some('×'),
                "middot" => Some('·'),
                "bull" => Some('•'),
                "ndash" => Some('–'),
                "mdash" => Some('—'),
                "rupee" => Some('₹'),
                _ => name
                    .strip_prefix("#x")
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()))
                    .and_then(char::from_u32),
            };
            c.map(|c| (c, e))
        });
        match decoded {
            Some((c, e)) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(node: &Node) -> &Element {
        match node {
            Node::Element(e) => e,
            other => panic!("not an element: {other:?}"),
        }
    }

    #[test]
    fn parses_elements_attributes_and_text() {
        let nodes = parse(
            r#"<main class="a b" :style="{ color: print.color }"><img v-if="x" :src='y' /><p>Hi &amp; {{ a < b ? 'x' : "y" }}</p></main>"#,
        );
        assert_eq!(nodes.len(), 1);
        let main = element(&nodes[0]);
        assert_eq!(main.tag, "main");
        assert_eq!(main.attr("class"), Some("a b"));
        assert_eq!(main.attr(":style"), Some("{ color: print.color }"));
        let img = element(&main.children[0]);
        assert_eq!(img.attr("v-if"), Some("x"));
        assert_eq!(img.attr(":src"), Some("y"));
        let p = element(&main.children[1]);
        assert_eq!(
            p.children,
            vec![Node::Text("Hi & {{ a < b ? 'x' : \"y\" }}".to_owned())]
        );
    }

    #[test]
    fn forgives_missing_and_stray_end_tags() {
        let nodes = parse("<div><p>one<p>two</div></span><br>three<!-- note -->");
        let div = element(&nodes[0]);
        assert_eq!(div.children.len(), 1);
        assert_eq!(element(&nodes[1]).tag, "br");
        assert_eq!(nodes[2], Node::Text("three".to_owned()));
        assert_eq!(nodes[3], Node::Comment("note".to_owned()));
    }

    #[test]
    fn boolean_and_unquoted_attributes() {
        let nodes = parse("<input disabled value=5>");
        let input = element(&nodes[0]);
        assert_eq!(input.attr("disabled"), Some(""));
        assert_eq!(input.attr("value"), Some("5"));
    }
}
