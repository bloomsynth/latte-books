//! Print templates and what they read, without laying anything out: the PrintTemplate
//! documents and which one prints a document ([`templates`]), the values a template
//! reads ([`values`]), the Template Builder's hints ([`hints`]), Books HTML templates
//! converted to Typst ([`convert`]), and what a report's printout shows ([`report`]).
//!
//! `latte-print` lays these out with Typst. They are apart so that `latte-mcp` can pick
//! templates and gather values without Typst, which the app lays out for it.

pub mod convert;
pub mod hints;

/// What `latte-mcp` and the app agree on for a print job (see `latte_print::job`).
pub mod job {
    /// The version of the job's form.
    pub const VERSION: u64 = 1;
    /// The flag the app takes a job with.
    pub const FLAG: &str = "--print-job";
}

pub mod report;
pub mod templates;
mod values;
mod words;

pub use templates::{PrintTemplate, is_html};
pub use values::print_values;
pub use words::in_words;

/// Latte's print library, which templates import as `/latte.typ`.
pub const LIBRARY: &str = concat!(
    include_str!("templates/latte.typ"),
    include_str!("templates/latte-html.typ")
);

/// The strings a template translates: the string literals it passes to `t(…)` and
/// `tf(…)`.
pub fn translated_strings(template: &str) -> Vec<String> {
    let is_name = |c: char| c.is_alphanumeric() || c == '-' || c == '_' || c == '.';
    let mut strings = Vec::new();
    let mut i = 0;
    while let Some(found) = template[i..].find("(\"") {
        let open = i + found;
        i = open + 2;
        let before = &template[..open];
        let name_start = before
            .char_indices()
            .rev()
            .take_while(|(_, c)| is_name(*c))
            .last()
            .map_or(open, |(k, _)| k);
        if !matches!(&template[name_start..open], "t" | "tf") {
            continue;
        }
        let mut text = String::new();
        let mut chars = template[i..].char_indices();
        let mut closed = None;
        while let Some((k, c)) = chars.next() {
            match c {
                '"' => {
                    closed = Some(k);
                    break;
                }
                '\\' => match chars.next() {
                    Some((_, 'n')) => text.push('\n'),
                    Some((_, 't')) => text.push('\t'),
                    Some((_, other)) => text.push(other),
                    None => {}
                },
                c => text.push(c),
            }
        }
        if let Some(k) = closed {
            strings.push(text);
            i += k + 1;
        }
    }
    strings.sort();
    strings.dedup();
    strings
}

#[cfg(test)]
mod templates_tests;
