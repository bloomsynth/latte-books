//! The JavaScript of Vue templates' expressions, parsed and written as Typst that reads
//! the same values: `doc.links.party.address`, `a ? b : c`, `t\`Item\``, `x.length`,
//! comparisons, `&&`/`||` (which give an operand, as in JavaScript), and string `+`.
//!
//! Keys are read with `get`, which gives `none` for a key that isn't there, as optional
//! chaining does; Books stops the template with "Invalid Key Error" instead.

use std::collections::HashSet;

/// An expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Str(String),
    Num(f64),
    Bool(bool),
    Null,
    Ident(String),
    Member(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    Unary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    /// A template literal's strings and the expressions between them.
    Template(Vec<String>, Vec<Expr>),
    /// A tagged template, such as t`Item`.
    Tagged(String, Vec<String>, Vec<Expr>),
    Object(Vec<(String, Expr)>),
    Array(Vec<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(String),
    Num(f64),
    Str(String),
    Template(Vec<String>, Vec<String>),
    Punct(&'static str),
}

const PUNCTS: [&str; 30] = [
    "===", "!==", "...", "?.", "??", "==", "!=", "<=", ">=", "&&", "||", "=>", ".", "?", ":", "(",
    ")", "[", "]", "{", "}", ",", "!", "+", "-", "*", "/", "%", "<", ">",
];

fn tokenize(src: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_alphabetic() || c == '_' || c == '$' {
            let start = i;
            while i < chars.len()
                && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$')
            {
                i += 1;
            }
            tokens.push(Token::Ident(chars[start..i].iter().collect()));
        } else if c.is_ascii_digit()
            || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit))
        {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            tokens.push(Token::Num(
                text.parse().map_err(|_| format!("bad number {text}"))?,
            ));
        } else if c == '\'' || c == '"' {
            i += 1;
            let mut s = String::new();
            while i < chars.len() && chars[i] != c {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 1;
                    s.push(match chars[i] {
                        'n' => '\n',
                        't' => '\t',
                        other => other,
                    });
                } else {
                    s.push(chars[i]);
                }
                i += 1;
            }
            if i >= chars.len() {
                return Err("a string isn't closed".to_owned());
            }
            i += 1;
            tokens.push(Token::Str(s));
        } else if c == '`' {
            i += 1;
            let mut strings = vec![String::new()];
            let mut exprs = Vec::new();
            while i < chars.len() && chars[i] != '`' {
                if chars[i] == '$' && chars.get(i + 1) == Some(&'{') {
                    i += 2;
                    let mut depth = 1;
                    let mut e = String::new();
                    while i < chars.len() {
                        match chars[i] {
                            '{' => depth += 1,
                            '}' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        e.push(chars[i]);
                        i += 1;
                    }
                    i += 1;
                    exprs.push(e);
                    strings.push(String::new());
                } else {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        i += 1;
                    }
                    if let Some(last) = strings.last_mut() {
                        last.push(chars[i]);
                    }
                    i += 1;
                }
            }
            if i >= chars.len() {
                return Err("a template string isn't closed".to_owned());
            }
            i += 1;
            tokens.push(Token::Template(strings, exprs));
        } else {
            let rest: String = chars[i..chars.len().min(i + 3)].iter().collect();
            let Some(p) = PUNCTS.iter().find(|p| rest.starts_with(**p)) else {
                return Err(format!("unexpected \"{c}\""));
            };
            i += p.chars().count();
            tokens.push(Token::Punct(p));
        }
    }
    Ok(tokens)
}

/// Parses an expression.
pub fn parse(src: &str) -> Result<Expr, String> {
    let tokens = tokenize(src)?;
    let mut parser = Parser { tokens, pos: 0 };
    let expr = parser.expr()?;
    if parser.pos < parser.tokens.len() {
        return Err(format!("didn't expect {:?}", parser.tokens[parser.pos]));
    }
    Ok(expr)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

/// Binary operators by precedence, lowest first.
const LEVELS: [&[&str]; 7] = [
    &["??"],
    &["||"],
    &["&&"],
    &["==", "!=", "===", "!=="],
    &["<", ">", "<=", ">="],
    &["+", "-"],
    &["*", "/", "%"],
];

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn is(&self, p: &str) -> bool {
        matches!(self.peek(), Some(Token::Punct(q)) if *q == p)
    }

    fn eat(&mut self, p: &str) -> bool {
        if self.is(p) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, p: &str) -> Result<(), String> {
        if self.eat(p) {
            Ok(())
        } else {
            Err(format!("expected \"{p}\""))
        }
    }

    fn expr(&mut self) -> Result<Expr, String> {
        let cond = self.binary(0)?;
        if self.eat("?") {
            let a = self.expr()?;
            self.expect(":")?;
            let b = self.expr()?;
            return Ok(Expr::Cond(Box::new(cond), Box::new(a), Box::new(b)));
        }
        if self.is("=>") {
            return Err("arrow functions aren't converted".to_owned());
        }
        Ok(cond)
    }

    fn binary(&mut self, level: usize) -> Result<Expr, String> {
        if level == LEVELS.len() {
            return self.unary();
        }
        let mut left = self.binary(level + 1)?;
        loop {
            let op = LEVELS[level].iter().find(|op| self.is(op)).copied();
            let Some(op) = op else { break };
            self.pos += 1;
            let right = self.binary(level + 1)?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        for op in ["!", "-", "+"] {
            if self.eat(op) {
                return Ok(Expr::Unary(op, Box::new(self.unary()?)));
            }
        }
        if matches!(self.peek(), Some(Token::Ident(s)) if s == "typeof") {
            return Err("typeof isn't converted".to_owned());
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Expr, String> {
        let mut e = self.primary()?;
        loop {
            if self.eat(".") || self.eat("?.") {
                if self.eat("[") {
                    let index = self.expr()?;
                    self.expect("]")?;
                    e = Expr::Index(Box::new(e), Box::new(index));
                    continue;
                }
                if self.eat("(") {
                    let args = self.args()?;
                    e = Expr::Call(Box::new(e), args);
                    continue;
                }
                match self.tokens.get(self.pos).cloned() {
                    Some(Token::Ident(name)) => {
                        self.pos += 1;
                        e = Expr::Member(Box::new(e), name);
                    }
                    _ => return Err("expected a key after \".\"".to_owned()),
                }
            } else if self.eat("[") {
                let index = self.expr()?;
                self.expect("]")?;
                e = Expr::Index(Box::new(e), Box::new(index));
            } else if self.eat("(") {
                let args = self.args()?;
                e = Expr::Call(Box::new(e), args);
            } else if let Some(Token::Template(strings, exprs)) = self.peek().cloned() {
                let Expr::Ident(tag) = &e else {
                    return Err("only a name can tag a template string".to_owned());
                };
                self.pos += 1;
                let exprs = exprs
                    .iter()
                    .map(|s| parse(s))
                    .collect::<Result<Vec<_>, _>>()?;
                e = Expr::Tagged(tag.clone(), strings, exprs);
            } else {
                break;
            }
        }
        Ok(e)
    }

    fn args(&mut self) -> Result<Vec<Expr>, String> {
        let mut args = Vec::new();
        while !self.eat(")") {
            args.push(self.expr()?);
            if !self.eat(",") {
                self.expect(")")?;
                break;
            }
        }
        Ok(args)
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let Some(token) = self.tokens.get(self.pos).cloned() else {
            return Err("the expression ends too soon".to_owned());
        };
        self.pos += 1;
        Ok(match token {
            Token::Num(n) => Expr::Num(n),
            Token::Str(s) => Expr::Str(s),
            Token::Template(strings, exprs) => Expr::Template(
                strings,
                exprs
                    .iter()
                    .map(|s| parse(s))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            Token::Ident(name) => match name.as_str() {
                "true" => Expr::Bool(true),
                "false" => Expr::Bool(false),
                "null" | "undefined" => Expr::Null,
                _ => Expr::Ident(name),
            },
            Token::Punct("(") => {
                let e = self.expr()?;
                self.expect(")")?;
                e
            }
            Token::Punct("[") => {
                let mut items = Vec::new();
                while !self.eat("]") {
                    items.push(self.expr()?);
                    if !self.eat(",") {
                        self.expect("]")?;
                        break;
                    }
                }
                Expr::Array(items)
            }
            Token::Punct("{") => {
                let mut entries = Vec::new();
                while !self.eat("}") {
                    let key = match self.tokens.get(self.pos).cloned() {
                        Some(Token::Ident(k)) | Some(Token::Str(k)) => k,
                        Some(Token::Num(n)) => n.to_string(),
                        _ => return Err("expected a key in an object".to_owned()),
                    };
                    self.pos += 1;
                    let value = if self.eat(":") {
                        self.expr()?
                    } else {
                        Expr::Ident(key.clone())
                    };
                    entries.push((key, value));
                    if !self.eat(",") {
                        self.expect("}")?;
                        break;
                    }
                }
                Expr::Object(entries)
            }
            other => return Err(format!("didn't expect {other:?}")),
        })
    }
}

/// A Typst string literal.
pub fn string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Typst's keywords and the names a converted template uses, which a JavaScript name
/// mustn't take.
const RESERVED: [&str; 40] = [
    "let", "set", "show", "if", "else", "for", "in", "while", "break", "continue", "return",
    "import", "include", "not", "and", "or", "none", "auto", "true", "false", "context", "as",
    "get", "has", "val", "rows", "lines", "str", "type", "t", "tf", "calc", "data", "text",
    "block", "box", "grid", "image", "items", "cells",
];

/// A JavaScript name as a Typst one.
pub fn name(js: &str) -> String {
    let mut out = js.replace('$', "dollar-");
    if RESERVED.contains(&out.as_str()) || out.starts_with("js-") {
        out = format!("js-{out}");
    }
    out
}

/// The names in scope: `doc`, `print`, and the `v-for` variables.
#[derive(Debug, Clone)]
pub struct Scope {
    names: HashSet<String>,
}

impl Default for Scope {
    fn default() -> Self {
        Self {
            names: ["doc", "print"].iter().map(|s| (*s).to_owned()).collect(),
        }
    }
}

impl Scope {
    pub fn with(&self, names: &[&str]) -> Self {
        let mut scope = self.clone();
        scope.names.extend(names.iter().map(|s| (*s).to_owned()));
        scope
    }
}

/// Writes an expression as Typst. What can't be converted is written as `none`, with
/// a note of what it was in `notes`.
pub fn to_typst(e: &Expr, scope: &Scope, notes: &mut Vec<String>) -> String {
    let mut w = Writer { scope, notes };
    w.expr(e)
}

/// Parses and writes an expression; one that can't be parsed is `none`, with a note.
pub fn convert(src: &str, scope: &Scope, notes: &mut Vec<String>) -> String {
    match parse(src) {
        Ok(e) => to_typst(&e, scope, notes),
        Err(err) => {
            notes.push(format!(
                "the expression `{}` wasn't converted: {err}",
                src.trim()
            ));
            "none".to_owned()
        }
    }
}

struct Writer<'a> {
    scope: &'a Scope,
    notes: &'a mut Vec<String>,
}

impl Writer<'_> {
    fn unsupported(&mut self, what: String) -> String {
        self.notes.push(what);
        "none".to_owned()
    }

    fn expr(&mut self, e: &Expr) -> String {
        match e {
            Expr::Str(s) => string(s),
            Expr::Num(n) => number(*n),
            Expr::Bool(b) => b.to_string(),
            Expr::Null => "none".to_owned(),
            Expr::Ident(n) => {
                if self.scope.names.contains(n) {
                    name(n)
                } else {
                    self.unsupported(format!("`{n}` isn't a value a template has"))
                }
            }
            Expr::Member(base, key) if key == "length" => format!("js-len({})", self.expr(base)),
            Expr::Member(..) | Expr::Index(..) => self.path(e),
            Expr::Call(callee, args) => self.call(callee, args),
            Expr::Unary(op, a) => {
                let a = self.expr(a);
                match *op {
                    "!" => format!("not has({a})"),
                    "-" => format!("(-js-number({a}))"),
                    _ => format!("js-number({a})"),
                }
            }
            Expr::Binary(op, a, b) => {
                let (a, b) = (self.expr(a), self.expr(b));
                match *op {
                    "+" => format!("js-add({a}, {b})"),
                    "-" | "*" | "/" => format!("(js-number({a}) {op} js-number({b}))"),
                    "%" => format!("calc.rem(js-number({a}), js-number({b}))"),
                    "==" | "===" => format!("js-eq({a}, {b})"),
                    "!=" | "!==" => format!("not js-eq({a}, {b})"),
                    "<" | ">" | "<=" | ">=" => format!("js-compare({a}, \"{op}\", {b})"),
                    "&&" => format!("(if has({a}) {{ {b} }} else {{ {a} }})"),
                    "||" => format!("(if has({a}) {{ {a} }} else {{ {b} }})"),
                    _ => format!("(if {a} != none {{ {a} }} else {{ {b} }})"),
                }
            }
            Expr::Cond(c, a, b) => {
                let (c, a, b) = (self.expr(c), self.expr(a), self.expr(b));
                format!("(if has({c}) {{ {a} }} else {{ {b} }})")
            }
            Expr::Template(strings, exprs) => {
                let mut parts = Vec::new();
                for (i, s) in strings.iter().enumerate() {
                    if !s.is_empty() {
                        parts.push(string(s));
                    }
                    if let Some(e) = exprs.get(i) {
                        parts.push(format!("val({})", self.expr(e)));
                    }
                }
                if parts.is_empty() {
                    "\"\"".to_owned()
                } else {
                    format!("({})", parts.join(" + "))
                }
            }
            Expr::Tagged(tag, strings, exprs) if tag == "t" => {
                let mut key = String::new();
                for (i, s) in strings.iter().enumerate() {
                    key.push_str(s);
                    if i < exprs.len() {
                        key.push_str(&format!("${{{i}}}"));
                    }
                }
                if exprs.is_empty() {
                    format!("t({})", string(&key))
                } else {
                    let args: Vec<String> = exprs.iter().map(|e| self.expr(e)).collect();
                    format!("tf({}, {})", string(&key), args.join(", "))
                }
            }
            Expr::Tagged(tag, ..) => self.unsupported(format!("the tag `{tag}` isn't converted")),
            Expr::Array(items) => {
                let items: Vec<String> = items.iter().map(|e| self.expr(e)).collect();
                format!(
                    "({}{})",
                    items.join(", "),
                    if items.len() == 1 { "," } else { "" }
                )
            }
            Expr::Object(entries) => {
                if entries.is_empty() {
                    return "(:)".to_owned();
                }
                let entries: Vec<String> = entries
                    .iter()
                    .map(|(k, v)| format!("{}: {}", string(k), self.expr(v)))
                    .collect();
                format!("({})", entries.join(", "))
            }
        }
    }

    /// `a.b[c].d` as `get(a, "b", c, "d")`.
    fn path(&mut self, e: &Expr) -> String {
        let mut keys = Vec::new();
        let mut base = e;
        loop {
            match base {
                Expr::Member(b, key) if key != "length" => {
                    keys.push(string(key));
                    base = b;
                }
                Expr::Index(b, index) => {
                    keys.push(self.expr(index));
                    base = b;
                }
                _ => break,
            }
        }
        keys.reverse();
        let base = self.expr(base);
        format!("get({base}, {})", keys.join(", "))
    }

    fn call(&mut self, callee: &Expr, args: &[Expr]) -> String {
        let arg = |i: usize, w: &mut Self| args.get(i).map_or("none".to_owned(), |a| w.expr(a));
        match callee {
            Expr::Ident(f) => match f.as_str() {
                "String" => format!("val({})", arg(0, self)),
                "Number" | "parseFloat" | "parseInt" => format!("js-number({})", arg(0, self)),
                "Boolean" => format!("has({})", arg(0, self)),
                "t" => format!("t(val({}))", arg(0, self)),
                _ => self.unsupported(format!("the function `{f}` isn't converted")),
            },
            Expr::Member(base, method) => {
                let base = self.expr(base);
                match method.as_str() {
                    "toUpperCase" => format!("upper(val({base}))"),
                    "toLowerCase" => format!("lower(val({base}))"),
                    "trim" => format!("val({base}).trim()"),
                    "toString" => format!("val({base})"),
                    "toFixed" => format!("js-fixed({base}, {})", arg(0, self)),
                    "includes" => format!("js-includes({base}, {})", arg(0, self)),
                    "startsWith" => format!("val({base}).starts-with(val({}))", arg(0, self)),
                    "endsWith" => format!("val({base}).ends-with(val({}))", arg(0, self)),
                    "join" => format!("js-join({base}, {})", arg(0, self)),
                    "split" => format!("val({base}).split(val({}))", arg(0, self)),
                    "replace" | "replaceAll" => {
                        let count = if method == "replace" {
                            ", count: 1"
                        } else {
                            ""
                        };
                        format!(
                            "val({base}).replace(val({}), val({}){count})",
                            arg(0, self),
                            arg(1, self)
                        )
                    }
                    "slice" | "substring" => {
                        let end = args.get(1).map(|a| self.expr(a));
                        match end {
                            Some(end) => format!("js-slice({base}, {}, {end})", arg(0, self)),
                            None => format!("js-slice({base}, {})", arg(0, self)),
                        }
                    }
                    _ => self.unsupported(format!("the method `{method}` isn't converted")),
                }
            }
            _ => self.unsupported("a call that isn't to a name or method".to_owned()),
        }
    }
}

fn number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typst(src: &str) -> String {
        let scope = Scope::default().with(&["row", "index"]);
        let mut notes = Vec::new();
        let out = convert(src, &scope, &mut notes);
        assert!(notes.is_empty(), "{src}: {notes:?}");
        out
    }

    #[test]
    fn member_access_is_null_safe() {
        assert_eq!(
            typst("doc.links.party.links.address.addressDisplay"),
            r#"get(doc, "links", "party", "links", "address", "addressDisplay")"#
        );
        assert_eq!(typst("doc?.time"), r#"get(doc, "time")"#);
        assert_eq!(
            typst("doc.items[0].item"),
            r#"get(doc, "items", 0, "item")"#
        );
    }

    #[test]
    fn operators() {
        assert_eq!(
            typst("doc.entryType === 'SalesInvoice' ? 'Invoice' : 'Bill'"),
            r#"(if has(js-eq(get(doc, "entryType"), "SalesInvoice")) { "Invoice" } else { "Bill" })"#
        );
        assert_eq!(
            typst("doc.totalDiscount && !doc.discountAfterTax"),
            r#"(if has(get(doc, "totalDiscount")) { not has(get(doc, "discountAfterTax")) } else { get(doc, "totalDiscount") })"#
        );
        assert_eq!(typst("index + 1"), "js-add(index, 1)");
        assert_eq!(typst("doc.taxes.length"), r#"js-len(get(doc, "taxes"))"#);
        assert_eq!(
            typst("doc.x ?? ''"),
            r#"(if get(doc, "x") != none { get(doc, "x") } else { "" })"#
        );
        assert_eq!(
            typst("row.qty > 1 || index"),
            r#"(if has(js-compare(get(row, "qty"), ">", 1)) { js-compare(get(row, "qty"), ">", 1) } else { index })"#
        );
    }

    #[test]
    fn translations_and_template_strings() {
        assert_eq!(typst("t`Item`"), r#"t("Item")"#);
        assert_eq!(
            typst("t`Pay ${doc.name} now`"),
            r#"tf("Pay ${0} now", get(doc, "name"))"#
        );
        assert_eq!(typst("`#${index}`"), r##"("#" + val(index))"##);
        assert_eq!(
            typst("row.item.toUpperCase()"),
            r#"upper(val(get(row, "item")))"#
        );
    }

    #[test]
    fn unknown_names_are_noted() {
        let mut notes = Vec::new();
        let out = convert("fyo.format(x)", &Scope::default(), &mut notes);
        assert_eq!(out, "none");
        assert!(!notes.is_empty());
        let mut notes = Vec::new();
        assert_eq!(convert("(a => a)", &Scope::default(), &mut notes), "none");
        assert!(!notes.is_empty());
    }

    #[test]
    fn javascript_names_dont_take_typst_ones() {
        assert_eq!(name("row"), "row");
        assert_eq!(name("type"), "js-type");
        assert_eq!(name("$x"), "dollar-x");
    }
}
