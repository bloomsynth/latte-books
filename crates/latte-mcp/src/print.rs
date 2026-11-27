//! Printing: documents saved as PDF with their print templates, a look at a printout's
//! pages, reports saved as PDF, and what an assistant needs to write a template. The
//! same `latte-print` layout as the app's Print View, so a PDF from an assistant is the
//! one the app would save.

use std::path::PathBuf;
use std::sync::OnceLock;

use base64::Engine as _;
use latte_core::fyo::format::Formatter;
use latte_core::reports::Align;
use latte_core::{Doc, Fyo, config};
use latte_db::DocValue;
use latte_i18n::translation::{DEFAULT_LANGUAGE, LanguageMap, stitch};
use latte_print::hints::{Hint, LIBRARY_HINTS};
use latte_print::report::{ReportColumn, ReportPrint, print_report};
use latte_print::templates::{
    self, BASE_TEMPLATE, PRINT_SIZES, PrintTemplate, TEMPLATE_TYPES, size_name,
};
use latte_print::{Printout, errors_text};
use serde_json::{Map, Value, json};

use crate::tools::{Outcome, doc_label, load, opt_text, prepared_report, pretty, schema, text_arg};

/// Pixels to a point for a preview: an A4 page is about 890 pixels wide.
const PREVIEW_SCALE: f64 = 1.5;

/// The most pages a preview shows.
const PREVIEW_PAGES: usize = 6;

/// `t`, in the app's language, as the app prints.
fn t(text: &str) -> String {
    static MAP: OnceLock<Option<LanguageMap>> = OnceLock::new();
    let map = MAP.get_or_init(|| {
        let language =
            config::config_value("language").unwrap_or_else(|| DEFAULT_LANGUAGE.to_owned());
        LanguageMap::for_language(&language)
    });
    match map {
        Some(map) => map.translate(text, &[]),
        None => stitch(text, &[]),
    }
}

/// The text content of a reply.
pub fn text_content(text: String) -> Value {
    json!({ "type": "text", "text": text })
}

/// Puts Latte's bundled templates in the file, as the app does when it opens it, for a
/// file the app hasn't opened since it was made or since Frappe Books had it. Custom
/// templates are left alone, and nothing is written when the templates are Latte's.
fn ready(fyo: &mut Fyo) -> Result<(), String> {
    templates::update_print_templates(fyo).map_err(|e| e.to_string())
}

/// The template a document prints with: the one asked for, or its default as the Print
/// View picks it.
fn pick_template(fyo: &mut Fyo, doc: &Doc, asked: Option<&str>) -> Result<PrintTemplate, String> {
    let names = templates::template_names_for(fyo, doc);
    let label = doc.schema().label.clone();
    if names.is_empty() {
        return Err(format!(
            "{label} documents have no print template; they print as {}",
            TEMPLATE_TYPES.join(", ")
        ));
    }
    let name = match asked {
        Some(asked) if names.iter().any(|n| n == asked) => asked.to_owned(),
        Some(asked) => {
            return Err(format!(
                "There's no {label} template \"{asked}\"; the templates are: {}",
                names.join(", ")
            ));
        }
        None => templates::default_template_name(fyo, doc)
            .or_else(|| names.first().cloned())
            .ok_or("No template")?,
    };
    templates::load(fyo, &name).map_err(|e| e.to_string())
}

fn lay_out(fyo: &mut Fyo, doc: &Doc, template: &PrintTemplate) -> Result<Printout, String> {
    if template.is_html() {
        return Err(format!(
            "\"{}\" is a Frappe Books HTML template, which Latte can't print. \
             convert_print_template makes a Typst copy of it that prints.",
            template.name
        ));
    }
    let formatter = Formatter::new(fyo).map_err(|e| e.to_string())?;
    latte_print::print(fyo, &formatter, doc, template, &t).map_err(|e| {
        format!(
            "The template \"{}\" didn't lay out:\n{}",
            template.name,
            errors_text(&e)
        )
    })
}

/// A name that is safe as a file name, as the app saves its PDFs.
fn file_stem(name: &str) -> String {
    let name: String = name
        .chars()
        .map(|c| {
            if "/\\:*?\"<>|".contains(c) || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    if name.trim().is_empty() {
        t("Entry")
    } else {
        name.trim().to_owned()
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// Where the app's Save as PDF starts: the Documents folder, or the home folder
/// without one.
fn documents() -> PathBuf {
    let home = home().unwrap_or_default();
    let documents = home.join("Documents");
    if documents.is_dir() { documents } else { home }
}

/// Where to save a PDF named `stem`: the path given, a `.pdf` file or a folder (`~` is
/// the home folder, and a relative path is in Documents), else Documents. An existing
/// file is kept, and the new one numbered beside it, unless `overwrite`.
fn pdf_path(given: Option<&str>, stem: &str, overwrite: bool) -> Result<PathBuf, String> {
    let file_name = format!("{}.pdf", file_stem(stem));
    let path = match given {
        None => documents().join(&file_name),
        Some(given) => {
            let expanded = match given.strip_prefix("~") {
                Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => home()
                    .ok_or("There's no home folder to save in")?
                    .join(rest.trim_start_matches(['/', '\\'])),
                _ => PathBuf::from(given),
            };
            let path = if expanded.is_absolute() {
                expanded
            } else {
                documents().join(expanded)
            };
            let is_pdf = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
            if is_pdf { path } else { path.join(&file_name) }
        }
    };
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)
            .map_err(|e| format!("Couldn't make the folder {}: {e}", folder.display()))?;
    }
    if overwrite || !path.exists() {
        return Ok(path);
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    (2..)
        .map(|n| path.with_file_name(format!("{stem} ({n}).pdf")))
        .find(|p| !p.exists())
        .ok_or_else(|| "No free file name".to_owned())
}

fn save_pdf(
    printout: &Printout,
    given: Option<&str>,
    stem: &str,
    overwrite: bool,
) -> Result<PathBuf, String> {
    let pdf = printout.pdf()?;
    let path = pdf_path(given, stem, overwrite)?;
    std::fs::write(&path, pdf).map_err(|e| format!("Couldn't save {}: {e}", path.display()))?;
    Ok(path)
}

fn overwrite(args: &Value) -> bool {
    args.get("overwrite").and_then(Value::as_bool) == Some(true)
}

fn document(fyo: &mut Fyo, args: &Value) -> Result<Doc, String> {
    let schema = schema(fyo, text_arg(args, "type")?)?;
    if schema.is_single {
        return Err(format!("{} is settings, which don't print", schema.label));
    }
    load(fyo, &schema, Some(text_arg(args, "name")?))
}

/// `print_document`: Save as PDF.
pub fn print_document(fyo: &mut Fyo, args: &Value) -> Outcome {
    ready(fyo)?;
    let doc = document(fyo, args)?;
    let template = pick_template(fyo, &doc, opt_text(args, "template"))?;
    let printout = lay_out(fyo, &doc, &template)?;
    let name = doc.name().unwrap_or_default().to_owned();
    let path = save_pdf(&printout, opt_text(args, "path"), &name, overwrite(args))?;
    pretty(&json!({
        "saved": path.display().to_string(),
        "document": doc_label(&doc),
        "template": template.name,
        "pages": printout.page_count(),
    }))
}

/// A page size from its name, or a width and height in centimetres.
fn page_size(args: &Value, fallback: (f64, f64)) -> Result<(f64, f64), String> {
    let number = |key: &str| args.get(key).and_then(Value::as_f64).filter(|n| *n > 0.0);
    let size = match opt_text(args, "page_size") {
        Some(name) => PRINT_SIZES
            .iter()
            .find(|(n, _, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, w, h)| (*w, *h))
            .ok_or_else(|| {
                let names: Vec<&str> = PRINT_SIZES.iter().map(|(n, _, _)| *n).collect();
                format!(
                    "There's no page size \"{name}\"; the sizes are {}",
                    names.join(", ")
                )
            })?,
        None => fallback,
    };
    Ok((
        number("width").unwrap_or(size.0),
        number("height").unwrap_or(size.1),
    ))
}

/// `preview_print`: the printout's pages as images, with a saved template or Typst
/// source being tried out.
pub fn preview(fyo: &mut Fyo, args: &Value) -> Result<Vec<Value>, String> {
    ready(fyo)?;
    let doc = document(fyo, args)?;
    let mut template = pick_template(fyo, &doc, opt_text(args, "template"))?;
    if let Some(source) = args.get("template_source").and_then(Value::as_str) {
        template = PrintTemplate {
            name: format!("{} (unsaved changes)", template.name),
            template: source.to_owned(),
            ..template
        };
    }
    let has_size = ["page_size", "width", "height"]
        .iter()
        .any(|k| args.get(*k).is_some_and(|v| !v.is_null()));
    if has_size {
        let (width, height) = page_size(args, template.size())?;
        template.width = width;
        template.height = height;
    }
    let printout = lay_out(fyo, &doc, &template)?;
    let shown = args
        .get("pages")
        .and_then(Value::as_u64)
        .map_or(2, |n| n as usize)
        .clamp(1, PREVIEW_PAGES);
    let images = printout.png_pages_up_to(PREVIEW_SCALE, shown)?;
    let (width, height) = template.size();
    let summary = json!({
        "document": doc_label(&doc),
        "template": template.name,
        "pageSize": format!("{} ({width} × {height} cm)", size_name(width, height)),
        "pages": printout.page_count(),
        "shown": images.len(),
    });
    let mut content = vec![text_content(
        serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())?,
    )];
    content.extend(images.iter().map(|png| {
        json!({
            "type": "image",
            "data": base64::engine::general_purpose::STANDARD.encode(png),
            "mimeType": "image/png",
        })
    }));
    Ok(content)
}

/// `print_report`: a report's Save as PDF.
pub fn report_pdf(fyo: &Fyo, args: &Value) -> Outcome {
    let name = text_arg(args, "report")?;
    let mut report = prepared_report(fyo, name, args.get("filters"))?;
    let formatter = Formatter::new(fyo).map_err(|e| e.to_string())?;
    let columns = report.columns(fyo, &formatter).map_err(|e| e.to_string())?;
    let rows = report.run(fyo, &formatter).map_err(|e| e.to_string())?;

    let picked: Vec<usize> = match args.get("columns").and_then(Value::as_array) {
        None => (0..columns.len()).collect(),
        Some(wanted) => {
            let mut picked = Vec::new();
            for want in wanted.iter().filter_map(Value::as_str) {
                let found = columns
                    .iter()
                    .position(|c| c.fieldname == want || c.label.eq_ignore_ascii_case(want));
                match found {
                    Some(i) if !picked.contains(&i) => picked.push(i),
                    Some(_) => {}
                    None => {
                        let labels: Vec<&str> = columns.iter().map(|c| c.label.as_str()).collect();
                        return Err(format!(
                            "{name} has no column \"{want}\"; its columns are: {}",
                            labels.join(", ")
                        ));
                    }
                }
            }
            picked
        }
    };
    let total = rows.len();
    let start = args
        .get("start_row")
        .and_then(Value::as_u64)
        .map_or(0, |n| (n as usize).saturating_sub(1))
        .min(total);
    let count = args
        .get("row_count")
        .and_then(Value::as_u64)
        .map_or(total, |n| n as usize);
    let end = start.saturating_add(count).min(total);

    let (w, h) = page_size(args, (21.0, 29.7))?;
    let (long, short) = (w.max(h), w.min(h));
    let landscape = args.get("landscape").and_then(Value::as_bool) == Some(true);
    let (width, height) = if landscape {
        (long, short)
    } else {
        (short, long)
    };
    let company_name = match fyo.get_value("PrintSettings", None, "companyName") {
        Some(DocValue::Text(name)) => name,
        _ => String::new(),
    };
    let title = t(report.title());
    let print = ReportPrint {
        company_name,
        title: title.clone(),
        columns: picked
            .iter()
            .map(|&i| ReportColumn {
                label: columns[i].label.clone(),
                align: match columns[i].align {
                    Align::Right => "right",
                    Align::Center => "center",
                    Align::Left => "left",
                },
            })
            .collect(),
        rows: rows[start..end]
            .iter()
            .map(|row| {
                picked
                    .iter()
                    .map(|&i| {
                        row.cells
                            .get(i)
                            .map(|c| c.value.clone())
                            .unwrap_or_default()
                    })
                    .collect()
            })
            .collect(),
        printed: formatter.datetime(jiff::Timestamp::now()),
        width,
        height,
    };
    let printout = print_report(&print).map_err(|e| errors_text(&e))?;
    let today = jiff::ZonedDateTime::now().date();
    let stem = format!("{title} - {}", formatter.date(today));
    let path = save_pdf(&printout, opt_text(args, "path"), &stem, overwrite(args))?;
    pretty(&json!({
        "saved": path.display().to_string(),
        "report": title,
        "rows": end - start,
        "pages": printout.page_count(),
    }))
}

fn hint_json(hints: &[(String, Hint)]) -> Value {
    let mut map = Map::new();
    for (key, hint) in hints {
        let value = match hint {
            Hint::Value(label) => json!(label),
            Hint::Object(keys) => hint_json(keys),
            Hint::Array(keys) => json!([hint_json(keys)]),
        };
        map.insert(key.clone(), value);
    }
    Value::Object(map)
}

/// The values with each image's data left out.
fn without_images(value: Value) -> Value {
    match value {
        Value::String(s) if s.starts_with("data:") => json!("(an image)"),
        Value::Array(items) => Value::Array(items.into_iter().map(without_images).collect()),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, without_images(v)))
                .collect(),
        ),
        other => other,
    }
}

/// `print_template_guide`: how templates are written, and what one of a type reads.
pub fn guide(fyo: &mut Fyo, args: &Value) -> Outcome {
    ready(fyo)?;
    let kind = text_arg(args, "type")?;
    if !TEMPLATE_TYPES.contains(&kind) {
        return Err(format!(
            "Templates are for {}; not {kind}",
            TEMPLATE_TYPES.join(", ")
        ));
    }
    let library: Map<String, Value> = LIBRARY_HINTS
        .iter()
        .map(|(name, what)| ((*name).to_owned(), json!(what)))
        .collect();
    let sizes: Map<String, Value> = PRINT_SIZES
        .iter()
        .map(|(name, w, h)| ((*name).to_owned(), json!(format!("{w} × {h} cm"))))
        .collect();
    let mut guide = json!({
        "language": "Templates are Typst (typst.app/docs), not Frappe Books' HTML. A template starts with `#import \"/latte.typ\": *` and `#show: setup`, and reads the document as `doc` and the company's print settings as `print`; a value is shown already formatted, as text. A table's rows are `rows(doc.items)`, and a linked document's values are under `doc.links`, such as `doc.links.party.email`.",
        "saving": format!(
            "A template is a PrintTemplate document: create_document with type PrintTemplate and values name, type ({kind}), template (the Typst source), and width and height in cm (A4 if left out). Bundled templates can't be changed: copy one by reading it with get_document and creating a new one from its template. preview_print with template_source tries a source out on a real document before it's saved. To print {kind} documents with it by default, update_document Defaults with {} set to its name.",
            templates::default_field(kind)
        ),
        "starter": BASE_TEMPLATE,
        "values": hint_json(&latte_print::hints::hints(fyo, kind, &t)),
        "library": library,
        "pageSizes": sizes,
        "templates": templates::template_names(fyo, kind),
    });
    if let Some(name) = opt_text(args, "name") {
        let schema = schema(fyo, kind)?;
        let doc = load(fyo, &schema, Some(name))?;
        let formatter = Formatter::new(fyo).map_err(|e| e.to_string())?;
        let values = latte_print::values(fyo, &formatter, &doc, &t).map_err(|e| e.to_string())?;
        guide["documentValues"] = without_images(values);
    }
    pretty(&guide)
}

fn template_json(fyo: &mut Fyo, name: &str, default: Option<&str>) -> Value {
    let template = match templates::load(fyo, name) {
        Ok(template) => template,
        Err(e) => return json!({ "name": name, "error": e.to_string() }),
    };
    let (width, height) = template.size();
    let mut entry = json!({
        "name": name,
        "type": template.kind,
        "pageSize": format!("{} ({width} × {height} cm)", size_name(width, height)),
        "custom": template.is_custom,
    });
    if template.is_html() {
        entry["printable"] = json!(false);
        entry["note"] = json!(
            "A Frappe Books HTML template: convert_print_template makes a Typst copy that prints"
        );
    }
    if default == Some(name) {
        entry["default"] = json!(true);
    }
    entry
}

/// `list_print_templates`: the templates, for a type or a document, and which prints by
/// default.
pub fn list_templates(fyo: &mut Fyo, args: &Value) -> Outcome {
    ready(fyo)?;
    if opt_text(args, "name").is_some() {
        let doc = document(fyo, args)?;
        let names = templates::template_names_for(fyo, &doc);
        let default = templates::default_template_name(fyo, &doc);
        let list: Vec<Value> = names
            .iter()
            .map(|n| template_json(fyo, n, default.as_deref()))
            .collect();
        return pretty(&json!({ "document": doc_label(&doc), "templates": list }));
    }
    let kinds: Vec<&str> = match opt_text(args, "type") {
        Some(kind) if TEMPLATE_TYPES.contains(&kind) => vec![kind],
        Some(kind) => {
            return Err(format!(
                "Templates are for {}; not {kind}",
                TEMPLATE_TYPES.join(", ")
            ));
        }
        None => TEMPLATE_TYPES.to_vec(),
    };
    let mut by_type = Map::new();
    for kind in kinds {
        let names = templates::template_names(fyo, kind);
        let field = templates::default_field(kind);
        let named = match fyo.get_value("Defaults", None, &field) {
            Some(DocValue::Text(name)) if names.contains(&name) => Some(name),
            _ => names.first().cloned(),
        };
        let list: Vec<Value> = names
            .iter()
            .map(|n| template_json(fyo, n, named.as_deref()))
            .collect();
        by_type.insert(
            kind.to_owned(),
            json!({ "defaultSetting": format!("Defaults.{field}"), "templates": list }),
        );
    }
    pretty(&Value::Object(by_type))
}

/// `convert_print_template`: a Typst copy of a Frappe Books HTML template.
pub fn convert(fyo: &mut Fyo, args: &Value) -> Outcome {
    ready(fyo)?;
    let name = text_arg(args, "name")?;
    let template = templates::load(fyo, name).map_err(|e| e.to_string())?;
    if !template.is_html() {
        return Err(format!("\"{name}\" is already a Typst template"));
    }
    let (copy, notes) = templates::convert_template(fyo, &template)?;
    pretty(&json!({
        "created": copy,
        "type": template.kind,
        "notConverted": notes,
    }))
}
