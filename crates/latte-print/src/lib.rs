//! Prints documents and reports to PDF with Typst.
//!
//! A document prints with a PrintTemplate, as in Books, but its template is Typst source
//! rather than Books' HTML (see docs/DEVIATIONS.md). A template reads the values Books
//! gives its HTML templates (`getPrintTemplatePropValues`, see [`values`]) from Latte's
//! print library, `/latte.typ`. Latte's bundled templates ([`templates`]) are Typst ports
//! of Books' designs; a Books HTML template in a company file is converted to Typst once
//! ([`convert`]).
//!
//! A printout is the same on every device: it uses only the fonts bundled with Latte, the
//! images in the document, and the page size of the template.

pub mod convert;
pub mod hints;
pub mod report;
pub mod templates;
mod values;
mod words;
mod world;

use std::collections::HashMap;
use std::fmt;

use base64::Engine as _;
use latte_core::fyo::format::Formatter;
use latte_core::{Doc, Fyo};
use serde_json::{Map, Value, json};
use typst::WorldExt as _;
use typst::diag::{Severity, SourceDiagnostic};
use typst::utils::Scalar;
use typst_layout::PagedDocument;

pub use templates::{PrintTemplate, is_html};
pub use values::print_values;
pub use words::in_words;
pub use world::font_families;

/// Latte's print library, which templates import as `/latte.typ`.
pub const LIBRARY: &str = concat!(
    include_str!("templates/latte.typ"),
    include_str!("templates/latte-html.typ")
);

/// A template's own file, as its errors name it.
const TEMPLATE_PATH: &str = "/template.typ";

/// What went wrong laying out a template, with where in the template, from 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintError {
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub hints: Vec<String>,
}

impl PrintError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            line: None,
            column: None,
            hints: Vec::new(),
        }
    }
}

impl fmt::Display for PrintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let (Some(line), Some(column)) = (self.line, self.column) {
            write!(f, "Line {line}, column {column}: ")?;
        }
        write!(f, "{}", self.message)?;
        for hint in &self.hints {
            write!(f, "\nHint: {hint}")?;
        }
        Ok(())
    }
}

/// The errors as one message, one to a line.
pub fn errors_text(errors: &[PrintError]) -> String {
    errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

/// A laid out document, ready to save as PDF or show as images.
pub struct Printout {
    document: PagedDocument,
}

/// The values for printing `doc`: `{ doc, print }` as Books passes them.
pub fn values(
    fyo: &mut Fyo,
    formatter: &Formatter,
    doc: &Doc,
    t: &dyn Fn(&str) -> String,
) -> Result<Value, PrintError> {
    print_values(fyo, formatter, doc, t).map_err(|e| PrintError::new(e.to_string()))
}

/// Lays out `doc` with a PrintTemplate.
pub fn print(
    fyo: &mut Fyo,
    formatter: &Formatter,
    doc: &Doc,
    template: &PrintTemplate,
    t: &dyn Fn(&str) -> String,
) -> Result<Printout, Vec<PrintError>> {
    let values = values(fyo, formatter, doc, t).map_err(|e| vec![e])?;
    render(&template.template, &values, template.size(), t)
}

/// Lays out a template's Typst source with `{ doc, print }` values on pages of `size`
/// (width and height in centimetres). `t` translates the strings the template passes
/// to `t("…")`.
pub fn render(
    template: &str,
    values: &Value,
    size: (f64, f64),
    t: &dyn Fn(&str) -> String,
) -> Result<Printout, Vec<PrintError>> {
    if is_html(template) {
        return Err(vec![PrintError::new(
            "This is a Frappe Books HTML template, which Latte can't print. \
             Convert it to Typst from the template menu.",
        )]);
    }
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut images = Map::new();
    collect_images(values, &mut images, &mut files);

    let translations: Map<String, Value> = translated_strings(template)
        .into_iter()
        .map(|s| {
            let translated = t(&s);
            (s, Value::from(translated))
        })
        .collect();

    let (width, height) = size;
    let mut data = match values {
        Value::Object(map) => map.clone(),
        _ => Map::new(),
    };
    data.insert("t".to_owned(), Value::Object(translations));
    data.insert("images".to_owned(), Value::Object(images));
    data.insert(
        "page".to_owned(),
        json!({ "width": width, "height": height }),
    );
    let data = serde_json::to_vec(&Value::Object(data))
        .map_err(|e| vec![PrintError::new(e.to_string())])?;
    files.push(("/data.json".to_owned(), data));

    let main = format!(
        "#set page(width: {width}cm, height: {height}cm, margin: 0pt)\n#include \"{TEMPLATE_PATH}\"\n"
    );
    let world = world::PrintWorld::new(
        &[
            (world::sources_main(), main.as_str()),
            (TEMPLATE_PATH, template),
            ("/latte.typ", LIBRARY),
        ],
        files
            .iter()
            .map(|(path, data)| (path.as_str(), data.clone()))
            .collect(),
    );
    let document = typst::compile::<PagedDocument>(&world)
        .output
        .map_err(|errors| {
            errors
                .iter()
                .filter(|e| e.severity == Severity::Error)
                .map(|e| print_error(&world, e))
                .collect::<Vec<_>>()
        })?;
    Ok(Printout { document })
}

/// A diagnostic as a [`PrintError`], placed in the template: at the error itself, or at
/// the template's call that led to it inside the print library.
fn print_error(world: &world::PrintWorld, diagnostic: &SourceDiagnostic) -> PrintError {
    let template = world::file_id(TEMPLATE_PATH);
    let spans =
        std::iter::once(diagnostic.span).chain(diagnostic.trace.iter().map(|t| t.span.into()));
    let mut error = PrintError::new(diagnostic.message.to_string());
    for span in spans {
        if span.id() != Some(template) {
            continue;
        }
        let Some(range) = world.range(span) else {
            continue;
        };
        if let Ok(source) = typst::World::source(world, template)
            && let Some((line, column)) = source.lines().byte_to_line_column(range.start)
        {
            error.line = Some(line + 1);
            error.column = Some(column + 1);
            break;
        }
    }
    error.hints = diagnostic.hints.iter().map(|h| h.v.to_string()).collect();
    error
}

/// The strings a template translates: the string literals it passes to `t(…)` and
/// `tf(…)`.
fn translated_strings(template: &str) -> Vec<String> {
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

/// The `data:` images among the values, written as files for `picture`.
fn collect_images(
    value: &Value,
    images: &mut Map<String, Value>,
    files: &mut Vec<(String, Vec<u8>)>,
) {
    match value {
        Value::String(s) if s.starts_with("data:") && !images.contains_key(s) => {
            if let Some(bytes) = decode_data_url(s) {
                let path = format!("/images/{}", files.len());
                images.insert(s.clone(), Value::from(path.clone()));
                files.push((path, bytes));
            }
        }
        Value::Array(items) => items.iter().for_each(|v| collect_images(v, images, files)),
        Value::Object(map) => map.values().for_each(|v| collect_images(v, images, files)),
        _ => {}
    }
}

/// The bytes of a `data:` URL, as an AttachImage field stores an image.
fn decode_data_url(url: &str) -> Option<Vec<u8>> {
    let (_, data) = url.strip_prefix("data:")?.split_once(";base64,")?;
    base64::engine::general_purpose::STANDARD.decode(data).ok()
}

impl Printout {
    pub fn page_count(&self) -> usize {
        self.document.pages().len()
    }

    /// Each page's width and height, in points.
    pub fn page_sizes(&self) -> Vec<(f64, f64)> {
        self.document
            .pages()
            .iter()
            .map(|page| {
                let size = page.frame.size();
                (size.x.to_pt(), size.y.to_pt())
            })
            .collect()
    }

    /// The document as a PDF file.
    pub fn pdf(&self) -> Result<Vec<u8>, String> {
        typst_pdf::pdf(&self.document, &typst_pdf::PdfOptions::default()).map_err(|errors| {
            errors
                .iter()
                .map(|e| e.message.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        })
    }

    /// Each page as a PNG image, `pixel_per_pt` pixels to a point.
    pub fn png_pages(&self, pixel_per_pt: f64) -> Result<Vec<Vec<u8>>, String> {
        let options = typst_render::RenderOptions {
            pixel_per_pt: Scalar::new(pixel_per_pt),
            ..Default::default()
        };
        self.document
            .pages()
            .iter()
            .map(|page| {
                typst_render::render(page, &options)
                    .encode_png()
                    .map_err(|e| e.to_string())
            })
            .collect()
    }
}

/// Lays out Typst source that needs no values, such as a report's printout.
pub(crate) fn render_plain(
    source: &str,
    files: HashMap<String, Vec<u8>>,
) -> Result<Printout, Vec<PrintError>> {
    let world = world::PrintWorld::new(
        &[(world::sources_main(), source)],
        files
            .iter()
            .map(|(path, data)| (path.as_str(), data.clone()))
            .collect(),
    );
    let document = typst::compile::<PagedDocument>(&world)
        .output
        .map_err(|errors| {
            errors
                .iter()
                .filter(|e| e.severity == Severity::Error)
                .map(|e| PrintError::new(e.message.to_string()))
                .collect::<Vec<_>>()
        })?;
    Ok(Printout { document })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod templates_tests;
