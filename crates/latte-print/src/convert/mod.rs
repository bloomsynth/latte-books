//! Converts a Frappe Books HTML print template (HTML with Vue's template syntax, styled
//! with Tailwind classes) to Typst source for Latte's print library.
//!
//! The conversion is done once, when someone asks for it, and the Typst is theirs to
//! edit after. It reads the same `doc` and `print` values. Vue's `{{ }}`, `v-if`,
//! `v-else-if`, `v-else`, `v-show`, `v-for`, `:class`, `:style` and `:src` become Typst
//! code, and the Tailwind classes Books' templates use become Typst layout: blocks for
//! block elements, and the print library's `css-flex` and `css-grid` for flex and grid
//! containers, which size their items as CSS does.
//!
//! What can't be converted is left out with a `// TODO(convert): …` note at its place,
//! and listed in [`Conversion::notes`]; the conversion never fails.

mod expr;
mod html;
mod style;

use expr::{Scope, string};
use html::{Element, Node};
use style::{Display, LineHeight, Style};

/// A converted template.
#[derive(Debug, Clone)]
pub struct Conversion {
    /// The Typst source.
    pub source: String,
    /// What couldn't be converted, as also noted in the source.
    pub notes: Vec<String>,
}

/// Converts a Books HTML template, named `name`, to Typst.
pub fn html_to_typst(html: &str, name: &str) -> Conversion {
    let nodes = html::parse(html);
    let mut generator = Generator {
        notes: Vec::new(),
        counter: 0,
    };
    let ctx = Ctx {
        scope: Scope::default(),
        family: None,
        line_height: LineHeight::Factor(1.5),
        size_px: 16.0,
        pre_line: false,
    };
    let body = generator.children(&nodes, &ctx, &Mode::Flow);
    let mut source = String::new();
    source.push_str(&format!(
        "// Converted by Latte Books from the Frappe Books HTML template \"{}\".\n",
        name.replace('\n', " ")
    ));
    source.push_str(
        "// It reads the same `doc` and `print` values; the Key Hints list them, and the\n\
         // helpers of Latte's print library (/latte.typ). Where the HTML couldn't be\n\
         // followed, a `TODO(convert)` note says what was left out.\n",
    );
    source.push_str("#import \"/latte.typ\": *\n#show: setup-html\n\n#{\n  let wfull = 100%\n");
    for statement in body {
        source.push_str(&indent(&statement));
        source.push('\n');
    }
    source.push_str("}\n");
    Conversion {
        source,
        notes: generator.notes,
    }
}

/// Where children are put.
enum Mode {
    /// Into the flow of their parent, as content.
    Flow,
    /// As items of a flex container: pushed to the named list.
    Flex(String),
    /// As items of a grid container.
    Grid(String),
}

/// What children inherit.
#[derive(Clone)]
struct Ctx {
    scope: Scope,
    /// The font, as a Typst expression of a family name, when the template set one.
    family: Option<String>,
    line_height: LineHeight,
    size_px: f64,
    pre_line: bool,
}

impl Ctx {
    fn family(&self) -> String {
        self.family
            .clone()
            .unwrap_or_else(|| "\"Inter\"".to_owned())
    }
}

struct Generator {
    notes: Vec<String>,
    counter: usize,
}

fn indent(s: &str) -> String {
    s.lines()
        .map(|l| {
            if l.is_empty() {
                String::new()
            } else {
                format!("  {l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A code block of statements.
fn block_of(statements: &[String]) -> String {
    if statements.is_empty() {
        return "none".to_owned();
    }
    let mut s = String::from("{\n");
    for statement in statements {
        s.push_str(&indent(statement));
        s.push('\n');
    }
    s.push('}');
    s
}

/// Tags laid out inline.
const INLINE: [&str; 20] = [
    "span", "a", "b", "strong", "i", "em", "small", "label", "code", "sub", "sup", "u", "s",
    "mark", "abbr", "time", "font", "bdi", "q", "cite",
];
/// Tags laid out as blocks.
const BLOCK: [&str; 29] = [
    "div",
    "p",
    "section",
    "header",
    "footer",
    "main",
    "article",
    "aside",
    "nav",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "ul",
    "ol",
    "li",
    "dl",
    "dt",
    "dd",
    "figure",
    "figcaption",
    "form",
    "address",
    "blockquote",
    "pre",
    "caption",
    "center",
];

/// A piece of text: literal text, or a `{{ }}` expression.
enum Piece {
    Text(String),
    Expr(String),
}

fn pieces(text: &str) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        if start > 0 {
            out.push(Piece::Text(rest[..start].to_owned()));
        }
        let after = &rest[start + 2..];
        let end = after.find("}}").unwrap_or(after.len());
        out.push(Piece::Expr(after[..end].trim().to_owned()));
        rest = after.get(end + 2..).unwrap_or("");
    }
    if !rest.is_empty() {
        out.push(Piece::Text(rest.to_owned()));
    }
    out
}

/// Whitespace as CSS collapses it: runs of it become a space, and with `pre-line` line
/// breaks are kept, without the spaces around them.
fn collapse(text: &str, pre_line: bool) -> String {
    let mut out = String::new();
    let mut space = false;
    for c in text.chars() {
        if c == '\n' && pre_line {
            while out.ends_with(' ') {
                out.pop();
            }
            out.push('\n');
            space = false;
        } else if c.is_whitespace() && c != '\u{a0}' {
            space = true;
        } else {
            if space && !out.is_empty() && !out.ends_with('\n') {
                out.push(' ');
            }
            if space && out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.push(c);
        }
    }
    if space && !out.ends_with('\n') {
        out.push(' ');
    }
    out
}

/// The display an element has.
fn display_of(el: &Element, style: &Style) -> Display {
    if let Some(display) = style.display {
        return display;
    }
    let tag = el.tag.as_str();
    if INLINE.contains(&tag) || tag == "img" || tag == "br" {
        Display::Inline
    } else {
        Display::Block
    }
}

impl Generator {
    fn next_name(&mut self, prefix: &str) -> String {
        self.counter += 1;
        format!("{prefix}-{}", self.counter)
    }

    /// An element's style from its `class`, `:class`, `style` and `:style`.
    fn style(&mut self, el: &Element, ctx: &Ctx, notes: &mut Vec<String>) -> Style {
        let mut style = Style::default();
        if let Some(classes) = el.attr("class") {
            for class in classes.split_whitespace() {
                style.class(class, None);
            }
        }
        if let Some(binding) = el.attr(":class").or_else(|| el.attr("v-bind:class")) {
            self.bound_classes(binding, &mut style, ctx, notes);
        }
        if let Some(css) = el.attr("style") {
            style.css(css);
        }
        if let Some(binding) = el.attr(":style").or_else(|| el.attr("v-bind:style")) {
            style.bound_css(binding, &ctx.scope, notes);
        }
        notes.append(&mut style.unknown);
        style
    }

    /// `:class`: an object of classes and conditions, a list, a string, or a choice.
    fn bound_classes(
        &mut self,
        binding: &str,
        style: &mut Style,
        ctx: &Ctx,
        notes: &mut Vec<String>,
    ) {
        let Ok(parsed) = expr::parse(binding) else {
            notes.push(format!(":class=\"{binding}\" isn't converted"));
            return;
        };
        self.class_expr(&parsed, None, style, ctx, notes, binding);
    }

    fn class_expr(
        &mut self,
        e: &expr::Expr,
        cond: Option<&str>,
        style: &mut Style,
        ctx: &Ctx,
        notes: &mut Vec<String>,
        binding: &str,
    ) {
        use expr::Expr;
        match e {
            Expr::Str(classes) => {
                for class in classes.split_whitespace() {
                    style.class(class, cond);
                }
            }
            Expr::Array(items) => {
                for item in items {
                    self.class_expr(item, cond, style, ctx, notes, binding);
                }
            }
            Expr::Object(entries) => {
                for (classes, value) in entries {
                    let mut c = expr::to_typst(value, &ctx.scope, notes);
                    if let Some(outer) = cond {
                        c = format!("(has({outer}) and has({c}))");
                    }
                    for class in classes.split_whitespace() {
                        style.class(class, Some(&c));
                    }
                }
            }
            Expr::Cond(test, a, b) => {
                let test = expr::to_typst(test, &ctx.scope, notes);
                let yes = match cond {
                    Some(outer) => format!("(has({outer}) and has({test}))"),
                    None => test.clone(),
                };
                let no = match cond {
                    Some(outer) => format!("(has({outer}) and not has({test}))"),
                    None => format!("(not has({test}))"),
                };
                self.class_expr(a, Some(&yes), style, ctx, notes, binding);
                self.class_expr(b, Some(&no), style, ctx, notes, binding);
            }
            _ => notes.push(format!(":class=\"{binding}\" isn't converted")),
        }
    }

    /// The statements for a list of sibling nodes.
    fn children(&mut self, nodes: &[Node], ctx: &Ctx, mode: &Mode) -> Vec<String> {
        let mut statements = Vec::new();
        // The indices of the nodes that count, for whitespace and v-else.
        let mut i = 0;
        let len = nodes.len();
        while i < len {
            match &nodes[i] {
                Node::Comment(text) => {
                    statements.extend(text.lines().map(|l| format!("// {}", l.trim())));
                    i += 1;
                }
                Node::Text(text) => {
                    let block_before = nodes[..i]
                        .iter()
                        .rev()
                        .find(|n| !matches!(n, Node::Comment(_)))
                        .is_none_or(|n| !self.is_inline(n, ctx));
                    let block_after = nodes[i + 1..]
                        .iter()
                        .find(|n| !matches!(n, Node::Comment(_)))
                        .is_none_or(|n| !self.is_inline(n, ctx));
                    if let Some(s) = self.text(text, ctx, mode, block_before, block_after) {
                        statements.push(s);
                    }
                    i += 1;
                }
                Node::Element(el) => {
                    if el.has_attr("v-if") {
                        // The v-if, and the v-else-if and v-else after it.
                        let mut chain = vec![el];
                        let mut j = i + 1;
                        loop {
                            let next = nodes[j.min(len)..].iter().position(|n| match n {
                                Node::Comment(_) => false,
                                Node::Text(t) => !t.trim().is_empty(),
                                Node::Element(_) => true,
                            });
                            let Some(offset) = next else { break };
                            match &nodes[j + offset] {
                                Node::Element(e)
                                    if e.has_attr("v-else-if") || e.has_attr("v-else") =>
                                {
                                    chain.push(e);
                                    j += offset + 1;
                                    if e.has_attr("v-else") {
                                        break;
                                    }
                                }
                                _ => break,
                            }
                        }
                        statements.extend(self.if_chain(&chain, ctx, mode));
                        i = j;
                    } else if el.has_attr("v-else") || el.has_attr("v-else-if") {
                        // A v-else without its v-if.
                        statements.push("// TODO(convert): a v-else without a v-if".to_owned());
                        self.notes.push("a v-else without a v-if".to_owned());
                        statements.extend(self.element(el, ctx, mode));
                        i += 1;
                    } else {
                        statements.extend(self.element(el, ctx, mode));
                        i += 1;
                    }
                }
            }
        }
        statements
    }

    /// Whether a node is laid out inline (text, or an inline element).
    fn is_inline(&self, node: &Node, _ctx: &Ctx) -> bool {
        match node {
            Node::Text(t) => !t.trim().is_empty(),
            Node::Comment(_) => false,
            Node::Element(el) => {
                if el.tag == "template" {
                    return false;
                }
                let mut style = Style::default();
                if let Some(classes) = el.attr("class") {
                    for class in classes.split_whitespace() {
                        style.class(class, None);
                    }
                }
                matches!(
                    display_of(el, &style),
                    Display::Inline | Display::InlineBlock
                )
            }
        }
    }

    /// `v-if`, `v-else-if` and `v-else` as an `if`.
    fn if_chain(&mut self, chain: &[&Element], ctx: &Ctx, mode: &Mode) -> Vec<String> {
        let mut out = String::new();
        for (n, el) in chain.iter().enumerate() {
            let body = block_of(&self.element(el, ctx, mode));
            let cond = el.attr("v-if").or_else(|| el.attr("v-else-if"));
            match (n, cond) {
                (0, Some(c)) => {
                    let mut notes = Vec::new();
                    let c = expr::convert(c, &ctx.scope, &mut notes);
                    out.push_str(&self.noted(notes));
                    out.push_str(&format!("if has({c}) {body}"));
                }
                (_, Some(c)) if el.has_attr("v-else-if") => {
                    let mut notes = Vec::new();
                    let c = expr::convert(c, &ctx.scope, &mut notes);
                    for note in &notes {
                        self.notes.push(note.clone());
                    }
                    out.push_str(&format!(" else if has({c}) {body}"));
                }
                _ => out.push_str(&format!(" else {body}")),
            }
        }
        vec![out]
    }

    /// Notes as comments, kept in the list too.
    fn noted(&mut self, notes: Vec<String>) -> String {
        let mut out = String::new();
        for note in notes {
            out.push_str(&format!("// TODO(convert): {note}\n"));
            self.notes.push(note);
        }
        out
    }

    /// An element, with its `v-for`, as statements for `mode`. (Its `v-if` is the
    /// caller's.)
    fn element(&mut self, el: &Element, ctx: &Ctx, mode: &Mode) -> Vec<String> {
        let Some(each) = el.attr("v-for") else {
            return self.element_once(el, ctx, mode);
        };
        let mut notes = Vec::new();
        let (vars, list) = match each.split_once(" in ").or_else(|| each.split_once(" of ")) {
            Some((vars, list)) => (vars.trim(), list.trim()),
            None => {
                notes.push(format!("v-for=\"{each}\" isn't converted"));
                let mut out = vec![self.noted(notes)];
                out.extend(self.element_once(el, ctx, mode));
                return out;
            }
        };
        let names: Vec<&str> = vars
            .trim_start_matches('(')
            .trim_end_matches(')')
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        let list = expr::convert(list, &ctx.scope, &mut notes);
        let mut inner = ctx.clone();
        inner.scope = ctx.scope.with(&names);
        let value = names.first().map_or("item".to_owned(), |n| expr::name(n));
        let key = names.get(1).map_or("_".to_owned(), |n| expr::name(n));
        let body = block_of(&self.element_once(el, &inner, mode));
        let noted = self.noted(notes);
        vec![format!(
            "{noted}for ({value}, {key}) in js-each({list}) {body}"
        )]
    }

    /// An element, once.
    fn element_once(&mut self, el: &Element, ctx: &Ctx, mode: &Mode) -> Vec<String> {
        let mut notes = Vec::new();
        if el.tag == "template" {
            return self.children(&el.children, ctx, mode);
        }
        if matches!(
            el.tag.as_str(),
            "script" | "style" | "link" | "meta" | "title" | "head"
        ) {
            notes.push(format!("<{}> isn't converted", el.tag));
            return vec![self.noted(notes).trim_end().to_owned()];
        }
        let style = self.style(el, ctx, &mut notes);
        if let Some(show) = el.attr("v-show") {
            let c = expr::convert(show, &ctx.scope, &mut notes);
            let mut s = style.clone();
            s.hidden_if.push(format!("not has({c})"));
            return self.element_styled(el, s, ctx, mode, notes);
        }
        self.element_styled(el, style, ctx, mode, notes)
    }

    fn element_styled(
        &mut self,
        el: &Element,
        style: Style,
        ctx: &Ctx,
        mode: &Mode,
        mut notes: Vec<String>,
    ) -> Vec<String> {
        let display = display_of(el, &style);
        if display == Display::Hidden {
            return Vec::new();
        }
        let known = INLINE.contains(&el.tag.as_str())
            || BLOCK.contains(&el.tag.as_str())
            || matches!(
                el.tag.as_str(),
                "img" | "br" | "hr" | "table" | "thead" | "tbody" | "tfoot" | "tr" | "td" | "th"
            );
        if !known {
            notes.push(format!("the tag <{}> is laid out as a <div>", el.tag));
        }

        // The element as an expression of the width it is given.
        let (expr_of_width, inline) = match el.tag.as_str() {
            "br" => (WidthExpr::Fixed("linebreak()".to_owned()), true),
            "hr" => (
                WidthExpr::Fixed(format!(
                    "line(length: 100%, stroke: {})",
                    border_stroke(&style).unwrap_or_else(|| "border".to_owned())
                )),
                false,
            ),
            "img" => (
                WidthExpr::Fixed(self.image(el, &style, ctx, &mut notes)),
                true,
            ),
            "table" => (
                WidthExpr::Of(self.table(el, &style, ctx, &mut notes)),
                false,
            ),
            _ => {
                if matches!(display, Display::Inline) && matches!(mode, Mode::Flow) {
                    (WidthExpr::Fixed(self.inline(el, &style, ctx)), true)
                } else {
                    (WidthExpr::Of(self.block(el, &style, ctx)), false)
                }
            }
        };

        let noted = self.noted(notes);
        let mut statement = match mode {
            Mode::Flow => {
                let width = match &style.width {
                    Some(w) => w.clone(),
                    // A block fills its container: `wfull`, which is `auto` while a
                    // flex item is measured for what it needs.
                    None if inline || display == Display::InlineBlock => "auto".to_owned(),
                    None => "wfull".to_owned(),
                };
                let body = expr_of_width.with(&width, &style);
                let body = if !inline && (style.margin_left_auto || style.margin_right_auto) {
                    let side = match (style.margin_left_auto, style.margin_right_auto) {
                        (true, true) => "center",
                        (true, false) => "right",
                        _ => "left",
                    };
                    format!("align({side}, {body})")
                } else {
                    body
                };
                let mut parts = Vec::new();
                if let Some(top) = &style.margin.top {
                    parts.push(format!("v({top})"));
                }
                if style.margin.left.is_some() || style.margin.right.is_some() {
                    let l = style.margin.left.as_deref().unwrap_or("0pt");
                    let r = style.margin.right.as_deref().unwrap_or("0pt");
                    if inline {
                        parts.push(format!("h({l})"));
                        parts.push(body);
                        parts.push(format!("h({r})"));
                    } else {
                        parts.push(format!("pad(left: {l}, right: {r}, {body})"));
                    }
                } else {
                    parts.push(body);
                }
                if let Some(bottom) = &style.margin.bottom {
                    parts.push(format!("v({bottom})"));
                }
                if parts.len() == 1 {
                    parts.remove(0)
                } else {
                    block_of(&parts)
                }
            }
            Mode::Flex(list) => {
                let mut spec = vec![];
                if let Some(basis) = &style.basis {
                    spec.push(format!("basis: {basis}"));
                }
                if let Some(width) = &style.width {
                    spec.push(format!("width: {width}"));
                }
                if let Some(grow) = &style.grow {
                    spec.push(format!("grow: {grow}"));
                }
                if let Some(shrink) = &style.shrink {
                    spec.push(format!("shrink: {shrink}"));
                }
                if let Some(s) = style.align_self {
                    spec.push(format!("self: \"{s}\""));
                }
                if let Some(l) = &style.margin.left {
                    spec.push(format!("ml: {l}"));
                }
                if let Some(r) = &style.margin.right {
                    spec.push(format!("mr: {r}"));
                }
                if style.margin_left_auto {
                    spec.push("ml-auto: true".to_owned());
                }
                if style.margin_right_auto {
                    spec.push("mr-auto: true".to_owned());
                }
                let body = expr_of_width.with("w", &style);
                let body = vertical_margins(&style, body);
                spec.push(format!("body: {}", item_body(&body)));
                format!("{list}.push(({}))", spec.join(", "))
            }
            Mode::Grid(list) => {
                let mut spec = vec![];
                for (key, value) in [
                    ("col-span", &style.col_span),
                    ("col-start", &style.col_start),
                    ("row-start", &style.row_start),
                    ("row-span", &style.row_span),
                ] {
                    if let Some(v) = value {
                        spec.push(format!("{key}: {v}"));
                    }
                }
                let body = vertical_margins(&style, expr_of_width.with("w", &style));
                spec.push(format!("body: {}", item_body(&body)));
                format!("{list}.push(({}))", spec.join(", "))
            }
        };
        if !style.hidden_if.is_empty() {
            let conds: Vec<String> = style
                .hidden_if
                .iter()
                .map(|c| format!("has({c})"))
                .collect();
            statement = format!("if not ({}) {}", conds.join(" or "), block_of(&[statement]));
        }
        vec![format!("{noted}{statement}")]
    }

    /// `set` rules for what an element changes of the text, and the context its
    /// children have.
    fn text_sets(&mut self, style: &Style, ctx: &Ctx) -> (Vec<String>, Ctx) {
        let mut sets = Vec::new();
        let mut inner = ctx.clone();
        if let Some(family) = &style.font_family {
            inner.family = Some(family.clone());
            sets.push(format!(
                "set text(font: ({family}, \"Liberation Sans\", \"Inter\"))"
            ));
        }
        for (cond, name, value) in &style.text {
            let rule = format!("set text({name}: {value})");
            sets.push(match cond {
                Some(c) => format!("{rule} if has({c})"),
                None => rule,
            });
        }
        if let Some(px) = style.font_size_px {
            inner.size_px = px;
        }
        if let Some(lh) = style.line_height {
            inner.line_height = lh;
        }
        let size_changed = style.font_size_px.is_some();
        let edges_change = style.font_family.is_some()
            || style.line_height.is_some()
            || (size_changed && matches!(inner.line_height, LineHeight::Px(_)));
        if edges_change {
            let factor = match inner.line_height {
                LineHeight::Factor(f) => f,
                LineHeight::Px(px) => px / inner.size_px,
            };
            sets.push(format!(
                "set text(..line-edges({}, {}))",
                trim(factor),
                inner.family()
            ));
        }
        for (cond, side) in &style.align {
            let rule = format!("set align({side})");
            sets.push(match cond {
                Some(c) => format!("{rule} if has({c})"),
                None => rule,
            });
        }
        if let Some(ws) = style.whitespace {
            inner.pre_line = ws == "pre-line" || ws == "pre";
        }
        (sets, inner)
    }

    /// Wraps content in what an element does to all of its text.
    fn decorate(style: &Style, body: String) -> String {
        let mut body = body;
        match style.transform {
            Some("upper") => body = format!("upper({body})"),
            Some("lower") => body = format!("lower({body})"),
            _ => {}
        }
        for d in &style.decoration {
            body = format!("{d}({body})");
        }
        body
    }

    /// A block element: a function of its width.
    fn block(&mut self, el: &Element, style: &Style, ctx: &Ctx) -> String {
        let (mut statements, inner) = self.text_sets(style, ctx);
        let display = display_of(el, style);
        match display {
            Display::Flex if style.direction != Some("col") => {
                let list = self.next_name("items");
                statements.push(format!("let {list} = ()"));
                statements.extend(self.children(&el.children, &inner, &Mode::Flex(list.clone())));
                let mut args = vec![list];
                if let Some(gap) = style.gap_x.as_ref().or(style.space_x.as_ref()) {
                    args.push(format!("gap: {gap}"));
                }
                if let Some(j) = style.justify {
                    args.push(format!("justify: \"{j}\""));
                }
                if let Some(a) = style.align_items {
                    args.push(format!("align: \"{a}\""));
                }
                let vertical = format!(
                    "{} + {}",
                    style.padding.top.as_deref().unwrap_or("0pt"),
                    style.padding.bottom.as_deref().unwrap_or("0pt")
                );
                args.push(format!("height: css-inner(HEIGHT, {vertical})"));
                statements.push(format!("css-flex({})", args.join(", ")));
            }
            Display::Flex => {
                let list = self.next_name("items");
                statements.push(format!("let {list} = ()"));
                statements.extend(self.children(&el.children, &inner, &Mode::Flex(list.clone())));
                let mut args = vec![list];
                if let Some(gap) = style.gap_y.as_ref().or(style.space_y.as_ref()) {
                    args.push(format!("gap: {gap}"));
                }
                if let Some(a) = style.align_items {
                    args.push(format!("align: \"{a}\""));
                }
                statements.push(format!("css-flex-col({})", args.join(", ")));
            }
            Display::Grid => {
                let list = self.next_name("cells");
                statements.push(format!("let {list} = ()"));
                statements.extend(self.children(&el.children, &inner, &Mode::Grid(list.clone())));
                let mut args = vec![list];
                if let Some(cols) = &style.grid_cols {
                    args.push(format!("cols: {cols}"));
                }
                if let Some(rows) = &style.grid_rows {
                    args.push(format!("rows: {rows}"));
                }
                if let Some(gap) = &style.gap_x {
                    args.push(format!("gap-x: {gap}"));
                }
                if let Some(gap) = &style.gap_y {
                    args.push(format!("gap-y: {gap}"));
                }
                statements.push(format!("css-grid({})", args.join(", ")));
            }
            _ => {
                let mut children = self.children(&el.children, &inner, &Mode::Flow);
                if let Some(gap) = &style.space_y {
                    // `space-y-*`: a gap between the children.
                    let n = children.len();
                    children = children
                        .into_iter()
                        .enumerate()
                        .flat_map(|(i, c)| {
                            if i + 1 < n && !c.starts_with("//") {
                                vec![c, format!("v({gap})")]
                            } else {
                                vec![c]
                            }
                        })
                        .collect();
                }
                statements.extend(children);
            }
        }
        let body = Self::decorate(style, block_of(&statements));
        let mut args = vec!["width: WIDTH".to_owned()];
        match &style.height {
            Some(h) if h != "auto" && el.tag != "main" && h != "100%" => {
                args.push(format!("height: {h}"))
            }
            // A flex item is as high as its row, as CSS stretches it.
            _ => args.push("height: HEIGHT".to_owned()),
        }
        if let Some(fill) = &style.fill {
            args.push(format!("fill: {fill}"));
        }
        if let Some(stroke) = border_stroke(style) {
            args.push(format!("stroke: {stroke}"));
        }
        if let Some(radius) = &style.radius {
            args.push(format!("radius: {radius}"));
        }
        if let Some(inset) = style.padding.dict() {
            args.push(format!("inset: {inset}"));
        }
        if style.whitespace == Some("nowrap") {
            args.push("breakable: false".to_owned());
        }
        format!("block({}, {body})", args.join(", "))
    }

    /// An inline element: styled text.
    fn inline(&mut self, el: &Element, style: &Style, ctx: &Ctx) -> String {
        let (mut statements, inner) = self.text_sets(style, ctx);
        statements.extend(self.children(&el.children, &inner, &Mode::Flow));
        let mut body = block_of(&statements);
        if matches!(el.tag.as_str(), "b" | "strong") {
            body = format!("strong({body})");
        }
        if matches!(el.tag.as_str(), "i" | "em") {
            body = format!("emph({body})");
        }
        body = Self::decorate(style, body);
        let boxed = style.fill.is_some()
            || !style.padding.is_empty()
            || border_stroke(style).is_some()
            || style.width.is_some();
        if boxed {
            let mut args = Vec::new();
            if let Some(w) = &style.width {
                args.push(format!("width: {w}"));
            }
            if let Some(fill) = &style.fill {
                args.push(format!("fill: {fill}"));
            }
            if let Some(stroke) = border_stroke(style) {
                args.push(format!("stroke: {stroke}"));
            }
            if let Some(radius) = &style.radius {
                args.push(format!("radius: {radius}"));
            }
            if let Some(inset) = style.padding.dict() {
                args.push(format!("inset: {inset}"));
            }
            body = format!("box({}, {body})", args.join(", "));
        }
        body
    }

    /// An image from the document's values (`:src`).
    fn image(&mut self, el: &Element, style: &Style, ctx: &Ctx, notes: &mut Vec<String>) -> String {
        let src = match (
            el.attr(":src").or_else(|| el.attr("v-bind:src")),
            el.attr("src"),
        ) {
            (Some(bound), _) => expr::convert(bound, &ctx.scope, notes),
            (None, Some(src)) => {
                notes.push(format!(
                    "the image \"{}\" isn't one of the document's images, so it isn't printed",
                    src.chars().take(60).collect::<String>()
                ));
                return "none".to_owned();
            }
            (None, None) => return "none".to_owned(),
        };
        let mut args = vec![src];
        if let Some(h) = &style.height {
            args.push(format!("height: {h}"));
        }
        if let Some(w) = &style.width {
            args.push(format!("width: {w}"));
        }
        if let Some(mw) = &style.max_width {
            args.push(format!("max-width: {mw}"));
        }
        if let Some(fit) = style.object_fit {
            args.push(format!("fit: \"{fit}\""));
        }
        let mut image = format!("css-img({})", args.join(", "));
        if let Some(inset) = style.padding.dict() {
            image = format!("box(inset: {inset}, {image})");
        }
        image
    }

    /// A table: its rows of cells, as a grid.
    fn table(&mut self, el: &Element, style: &Style, ctx: &Ctx, notes: &mut Vec<String>) -> String {
        let (mut statements, inner) = self.text_sets(style, ctx);
        let rows = self.next_name("rows");
        statements.push(format!("let {rows} = ()"));
        let mut columns = 0;
        statements.extend(self.table_rows(&el.children, &inner, &rows, &mut columns, notes));
        statements.push(format!(
            "css-table({rows}, columns: {}, full: {})",
            columns.max(1),
            style.width.as_deref() == Some("100%")
        ));
        let body = Self::decorate(style, block_of(&statements));
        let mut args = vec!["width: WIDTH".to_owned()];
        if let Some(stroke) = border_stroke(style) {
            args.push(format!("stroke: {stroke}"));
        }
        if let Some(fill) = &style.fill {
            args.push(format!("fill: {fill}"));
        }
        format!("block({}, {body})", args.join(", "))
    }

    fn table_rows(
        &mut self,
        nodes: &[Node],
        ctx: &Ctx,
        rows: &str,
        columns: &mut usize,
        notes: &mut Vec<String>,
    ) -> Vec<String> {
        let mut out = Vec::new();
        for node in nodes {
            match node {
                Node::Element(el)
                    if matches!(el.tag.as_str(), "thead" | "tbody" | "tfoot" | "template") =>
                {
                    out.extend(self.wrap_directives(el, ctx, |g, inner| {
                        g.table_rows(&el.children, inner, rows, columns, &mut Vec::new())
                    }));
                }
                Node::Element(el) if el.tag == "tr" => {
                    let count = el
                        .children
                        .iter()
                        .filter_map(|c| match c {
                            Node::Element(e) if e.tag == "td" || e.tag == "th" => {
                                Some(e.attr("colspan").and_then(|n| n.parse().ok()).unwrap_or(1))
                            }
                            _ => None,
                        })
                        .sum::<usize>();
                    *columns = (*columns).max(count);
                    out.extend(self.wrap_directives(el, ctx, |g, inner| {
                        let mut tr_notes = Vec::new();
                        let tr_style = g.style(el, inner, &mut tr_notes);
                        let (sets, cell_ctx) = g.text_sets(&tr_style, inner);
                        let cells = g.next_name("cells");
                        let mut s = vec![g.noted(tr_notes).trim_end().to_owned()];
                        s.retain(|l| !l.is_empty());
                        s.push(format!("let {cells} = ()"));
                        for child in &el.children {
                            if let Node::Element(td) = child
                                && (td.tag == "td" || td.tag == "th")
                            {
                                s.extend(g.wrap_directives(td, &cell_ctx, |g, cctx| {
                                    let mut cell_notes = Vec::new();
                                    let mut cs = g.style(td, cctx, &mut cell_notes);
                                    if td.tag == "th" {
                                        cs.text.insert(0, (None, "weight", "700".to_owned()));
                                    }
                                    let body = g.block(td, &cs, cctx).replace("WIDTH", "w").replace("HEIGHT", "auto");
                                    let colspan = td.attr("colspan").and_then(|n| n.parse::<usize>().ok()).unwrap_or(1);
                                    let noted = g.noted(cell_notes);
                                    let align = cs.align.iter().rev().find(|(c, _)| c.is_none()).map_or("left", |(_, a)| *a);
                                    vec![format!("{noted}{cells}.push((colspan: {colspan}, align: {align}, body: {}))", wrap_sets(&sets, body))]
                                }));
                            }
                        }
                        let fill = tr_style.fill.clone().unwrap_or_else(|| "none".to_owned());
                        s.push(format!("{rows}.push((fill: {fill}, cells: {cells}))"));
                        vec![block_of(&s)]
                    }));
                }
                Node::Comment(text) => out.push(format!("// {}", text.trim())),
                Node::Text(t) if t.trim().is_empty() => {}
                _ => notes.push("a table's content that isn't in a row".to_owned()),
            }
        }
        out
    }

    /// `f`'s statements, inside the element's `v-if` and `v-for`.
    fn wrap_directives(
        &mut self,
        el: &Element,
        ctx: &Ctx,
        f: impl FnOnce(&mut Self, &Ctx) -> Vec<String>,
    ) -> Vec<String> {
        let mut notes = Vec::new();
        let mut inner = ctx.clone();
        let mut for_head = None;
        if let Some(each) = el.attr("v-for")
            && let Some((vars, list)) = each.split_once(" in ").or_else(|| each.split_once(" of "))
        {
            let names: Vec<&str> = vars
                .trim()
                .trim_start_matches('(')
                .trim_end_matches(')')
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            let list = expr::convert(list.trim(), &ctx.scope, &mut notes);
            inner.scope = ctx.scope.with(&names);
            let value = names.first().map_or("item".to_owned(), |n| expr::name(n));
            let key = names.get(1).map_or("_".to_owned(), |n| expr::name(n));
            for_head = Some(format!("for ({value}, {key}) in js-each({list})"));
        }
        let mut statements = f(self, &inner);
        if let Some(head) = for_head {
            statements = vec![format!("{head} {}", block_of(&statements))];
        }
        if let Some(cond) = el.attr("v-if") {
            let c = expr::convert(cond, &ctx.scope, &mut notes);
            statements = vec![format!("if has({c}) {}", block_of(&statements))];
        }
        let noted = self.noted(notes);
        if !noted.is_empty() {
            statements.insert(0, noted.trim_end().to_owned());
        }
        statements
    }

    /// A text node's statement.
    fn text(
        &mut self,
        text: &str,
        ctx: &Ctx,
        mode: &Mode,
        block_before: bool,
        block_after: bool,
    ) -> Option<String> {
        let mut parts = Vec::new();
        let pieces = pieces(text);
        let n = pieces.len();
        let mut notes = Vec::new();
        for (i, piece) in pieces.into_iter().enumerate() {
            match piece {
                Piece::Text(t) => {
                    let mut t = collapse(&t, ctx.pre_line);
                    if i == 0 && block_before {
                        t = t.trim_start_matches(' ').to_owned();
                        if !ctx.pre_line {
                            t = t.trim_start().to_owned();
                        }
                    }
                    if i + 1 == n && block_after {
                        t = t.trim_end().to_owned();
                    }
                    if t.is_empty() {
                        continue;
                    }
                    if ctx.pre_line && t.contains('\n') {
                        let lines: Vec<String> = t
                            .split('\n')
                            .enumerate()
                            .map(|(k, l)| match (k, l.is_empty()) {
                                // A blank first line keeps its height.
                                (0, true) => "sym.zws; ".to_owned(),
                                (_, true) => String::new(),
                                _ => format!("{}; ", string(l)),
                            })
                            .collect();
                        parts.push(format!("{{ {}}}", lines.join("linebreak(); ")));
                    } else {
                        parts.push(string(&t));
                    }
                }
                Piece::Expr(e) => {
                    let value = expr::convert(&e, &ctx.scope, &mut notes);
                    parts.push(if ctx.pre_line {
                        format!("lines(val({value}))")
                    } else {
                        format!("val({value})")
                    });
                }
            }
        }
        let noted = self.noted(notes);
        if parts.is_empty() {
            return (!noted.is_empty()).then(|| noted.trim_end().to_owned());
        }
        let content = if parts.len() == 1 {
            parts.remove(0)
        } else {
            format!("{{ {} }}", parts.join("; "))
        };
        Some(match mode {
            Mode::Flow => format!("{noted}{content}"),
            Mode::Flex(list) | Mode::Grid(list) => {
                format!("{noted}{list}.push((body: (w, h: auto) => block(width: w, {content})))")
            }
        })
    }
}

/// An element's expression, of the width it is laid out at.
enum WidthExpr {
    /// Laid out at its own size.
    Fixed(String),
    /// With `WIDTH` for the width.
    Of(String),
}

impl WidthExpr {
    fn with(&self, width: &str, style: &Style) -> String {
        // A flex or grid item is given its height (`h`) as well as its width.
        let height = if width == "w" { "h" } else { "auto" };
        match self {
            Self::Fixed(e) => e.clone(),
            Self::Of(e) => {
                let e = &if height == "auto" {
                    // In the flow a block is as high as its content: no height.
                    drop_height(e)
                } else {
                    e.replace("HEIGHT", height)
                };
                let width = match (&style.max_width, width) {
                    (Some(max), "wfull") => {
                        return format!(
                            "layout(size => {})",
                            e.replace("WIDTH", &format!("calc.min(size.width, {max})"))
                        );
                    }
                    _ => width,
                };
                e.replace("WIDTH", width)
            }
        }
    }
}

/// `e` without its `height: HEIGHT` arguments, which a block in the flow doesn't need.
fn drop_height(e: &str) -> String {
    let mut out = e.replace(", height: HEIGHT", "");
    while let Some(start) = out.find(", height: css-inner(HEIGHT, ") {
        let rest = &out[start + 2..];
        let mut depth = 0;
        let mut end = rest.len();
        for (i, c) in rest.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        out.replace_range(start..start + 2 + end, "");
    }
    out.replace("HEIGHT", "auto")
}

/// A flex or grid item's body: a function of its width. Its blocks fill it (`wfull`),
/// except while it is measured (`auto`).
fn item_body(body: &str) -> String {
    if body.contains("wfull") {
        format!(
            "(w, h: auto) => {{\n  let wfull = if w == auto {{ auto }} else {{ 100% }}\n{}\n}}",
            indent(body)
        )
    } else {
        format!("(w, h: auto) => {body}")
    }
}

/// The vertical margins of a flex or grid item, around it.
fn vertical_margins(style: &Style, body: String) -> String {
    if style.margin.top.is_none() && style.margin.bottom.is_none() {
        return body;
    }
    let mut parts = Vec::new();
    if let Some(top) = &style.margin.top {
        parts.push(format!("v({top})"));
    }
    parts.push(body);
    if let Some(bottom) = &style.margin.bottom {
        parts.push(format!("v({bottom})"));
    }
    format!("{{ {} }}", parts.join("; "))
}

/// Applies a table row's text styles to a cell.
fn wrap_sets(sets: &[String], body: String) -> String {
    let body = if sets.is_empty() {
        body
    } else {
        format!("{{ {}; {body} }}", sets.join("; "))
    };
    item_body(&body)
}

/// An element's borders as a Typst stroke.
fn border_stroke(style: &Style) -> Option<String> {
    if style.border.is_empty() {
        return None;
    }
    let color = style
        .border_color
        .clone()
        .unwrap_or_else(|| "tw(\"gray\", 200)".to_owned());
    let dash = if style.border_dashed {
        ", dash: \"dashed\""
    } else {
        ""
    };
    let side = |w: &Option<String>| {
        w.as_ref().map(|w| {
            if w == "none" {
                "none".to_owned()
            } else if w.starts_with("(if") {
                format!("css-stroke({w}, {color}{dash})")
            } else {
                format!("(thickness: {w}, paint: {color}{dash})")
            }
        })
    };
    let parts: Vec<String> = [
        ("top", side(&style.border.top)),
        ("right", side(&style.border.right)),
        ("bottom", side(&style.border.bottom)),
        ("left", side(&style.border.left)),
    ]
    .into_iter()
    .filter_map(|(k, v)| v.map(|v| format!("{k}: {v}")))
    .collect();
    Some(format!("({})", parts.join(", ")))
}

fn trim(n: f64) -> String {
    let s = format!("{n:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests;
