//! PrintTemplate documents: the templates bundled with Latte, kept in a company file as
//! Books keeps its own (`updatePrintTemplates` in src/utils/printTemplates.ts and
//! `getTemplates` in main/getPrintTemplates.ts @ v0.36.0), and which one prints a
//! document (`setTemplateFromDefault` in src/pages/PrintView/PrintView.vue).
//!
//! Differences from Books:
//! - The templates are Typst; Books' are HTML. They have Books' names and designs, so a
//!   file's Defaults and POS profiles name the same templates in both apps.
//! - Books writes its bundled templates again when the file on disk is newer than the
//!   row. Latte writes its own again whenever the row isn't Latte's (Books' HTML, or an
//!   older Latte's), and marks the row as modified in 2000, so Books, opening the file
//!   again, puts its HTML back. Each app prints with its own; custom templates are left
//!   alone by both.

use jiff::Timestamp;
use latte_core::fyo::model::helpers::is_truthy;
use latte_core::{Doc, Error, Fyo};
use latte_db::{DocValue, DocValueMap, Filter, GetAllOptions, Order};

/// A PrintTemplate document's values.
#[derive(Debug, Clone, PartialEq)]
pub struct PrintTemplate {
    pub name: String,
    /// The schema it prints, such as `SalesInvoice`.
    pub kind: String,
    pub template: String,
    /// The page, in centimetres.
    pub width: f64,
    pub height: f64,
    pub is_custom: bool,
}

impl PrintTemplate {
    /// Whether the template is a Books HTML template, which Latte converts rather than
    /// prints.
    pub fn is_html(&self) -> bool {
        is_html(&self.template)
    }

    /// The page size, with Books' defaults (A4) for a size that isn't set.
    pub fn size(&self) -> (f64, f64) {
        let or = |v: f64, d: f64| if v > 0.0 { v } else { d };
        (or(self.width, 21.0), or(self.height, 29.7))
    }

    pub fn from_doc(doc: &Doc) -> Self {
        let text = |f: &str| match doc.get(f) {
            Some(DocValue::Text(t)) => t.clone(),
            _ => String::new(),
        };
        let number = |f: &str| match doc.get(f) {
            Some(DocValue::Number(n)) => *n,
            _ => 0.0,
        };
        Self {
            name: doc.name().unwrap_or_default().to_owned(),
            kind: text("type"),
            template: text("template"),
            width: number("width"),
            height: number("height"),
            is_custom: is_truthy(doc.get("isCustom")),
        }
    }
}

/// Whether a template's source is HTML (a Books template) rather than Typst.
pub fn is_html(source: &str) -> bool {
    let start = source.trim_start();
    start.starts_with('<') && !start.starts_with("<<")
}

/// The template a new PrintTemplate starts with: `baseTemplate` in Typst.
pub const BASE_TEMPLATE: &str = include_str!("templates/new.typ");

/// One of the templates bundled with Latte, as Books names its files:
/// `Name[.Schema].template.html`.
struct Bundled {
    file: &'static str,
    source: &'static str,
}

/// Books' templates, in the order `fs.readdir` gives Books' files.
const BUNDLED: [Bundled; 6] = [
    Bundled {
        file: "Basic",
        source: include_str!("templates/basic.typ"),
    },
    Bundled {
        file: "Business-POS",
        source: include_str!("templates/pos.typ"),
    },
    Bundled {
        file: "Business.Payment",
        source: include_str!("templates/payment.typ"),
    },
    Bundled {
        file: "Business.Shipment",
        source: include_str!("templates/shipment.typ"),
    },
    Bundled {
        file: "Business",
        source: include_str!("templates/business.typ"),
    },
    Bundled {
        file: "Minimal",
        source: include_str!("templates/minimal.typ"),
    },
];

/// The schemas a template is for: `PrintTemplate.lists.type`.
pub use latte_core::models::base_models::print_template::TYPES as TEMPLATE_TYPES;

/// `printSizes` and `paperSizeMap` (src/utils/ui.ts): name, width and height in cm.
pub const PRINT_SIZES: [(&str, f64, f64); 30] = [
    ("A0", 84.1, 118.9),
    ("A1", 59.4, 84.1),
    ("A2", 42.0, 59.4),
    ("A3", 29.7, 42.0),
    ("A4", 21.0, 29.7),
    ("A5", 14.8, 21.0),
    ("A6", 10.5, 14.8),
    ("A7", 7.4, 10.5),
    ("A8", 5.2, 7.4),
    ("A9", 3.7, 5.2),
    ("B0", 100.0, 141.4),
    ("B1", 70.7, 100.0),
    ("B2", 50.0, 70.7),
    ("B3", 35.3, 50.0),
    ("B4", 25.0, 35.3),
    ("B5", 17.6, 25.0),
    ("B6", 12.5, 17.6),
    ("B7", 8.8, 12.5),
    ("B8", 6.2, 8.8),
    ("B9", 4.4, 6.2),
    ("POS", 8.0, 22.0),
    ("Letter", 21.59, 27.94),
    ("Legal", 21.59, 35.56),
    ("Executive", 19.05, 25.4),
    ("C5E", 16.3, 22.9),
    ("Comm10", 10.5, 24.1),
    ("DLE", 11.0, 22.0),
    ("Folio", 21.0, 33.0),
    ("Ledger", 43.2, 27.9),
    ("Tabloid", 27.9, 43.2),
];

/// The name of a page size, or "Custom".
pub fn size_name(width: f64, height: f64) -> &'static str {
    PRINT_SIZES
        .iter()
        .find(|(_, w, h)| *w == width && *h == height)
        .map_or("Custom", |(name, _, _)| name)
}

/// `getNameAndTypeFromTemplateFile`: a file without a schema in its name serves Sales
/// Invoices, Quotes and Purchase Invoices.
fn names_and_types(fyo: &Fyo, file: &str) -> Vec<(String, String)> {
    let (name, schema) = match file.split_once('.') {
        Some((name, schema)) => (name, Some(schema)),
        None => (file, None),
    };
    let label = |schema: &str| {
        fyo.schema(schema)
            .map_or_else(|| schema.to_owned(), |s| s.label.clone())
    };
    match schema {
        Some(schema) => vec![(format!("{name} - {}", label(schema)), schema.to_owned())],
        None => ["SalesInvoice", "SalesQuote", "PurchaseInvoice"]
            .iter()
            .map(|s| (format!("{name} - {}", label(s)), (*s).to_owned()))
            .collect(),
    }
}

/// The templates bundled with Latte, as PrintTemplate values: `getTemplates`. The POS
/// receipt is `posPrintWidth` wide and 22cm high; the others leave the size unset (A4).
pub fn bundled_templates(fyo: &Fyo) -> Vec<PrintTemplate> {
    let pos_width = match fyo.get_value("PrintSettings", None, "posPrintWidth") {
        Some(DocValue::Number(n)) => n,
        _ => 0.0,
    };
    let mut templates = Vec::new();
    for bundled in &BUNDLED {
        let pos = bundled.file.split('-').nth(1) == Some("POS");
        for (name, kind) in names_and_types(fyo, bundled.file) {
            templates.push(PrintTemplate {
                name,
                kind,
                template: bundled.source.to_owned(),
                width: if pos { pos_width } else { 0.0 },
                height: if pos { 22.0 } else { 0.0 },
                is_custom: false,
            });
        }
    }
    templates
}

/// When Latte marks its bundled rows as modified, so that Books always writes its own
/// templates back over them.
fn bundled_modified() -> Timestamp {
    "2000-01-01T00:00:00Z".parse().expect("a valid timestamp")
}

/// `updatePrintTemplates`: adds Latte's templates to the file, and writes them again over
/// bundled rows that aren't Latte's own. Custom templates aren't touched.
pub fn update_print_templates(fyo: &mut Fyo) -> Result<(), Error> {
    if fyo.schema("PrintTemplate").is_none() {
        return Ok(());
    }
    for template in bundled_templates(fyo) {
        let existing = match fyo.db.exists("PrintTemplate", Some(&template.name))? {
            true => Some(fyo.get_doc("PrintTemplate", Some(&template.name))?),
            false => None,
        };
        let mut doc = match existing {
            Some(doc) => {
                let current = PrintTemplate::from_doc(&doc);
                if current.is_custom {
                    continue;
                }
                let unchanged = current.template == template.template
                    && current.kind == template.kind
                    && (template.width == 0.0 || current.width == template.width)
                    && (template.height == 0.0 || current.height == template.height);
                if unchanged {
                    continue;
                }
                doc
            }
            None => fyo.get_new_doc("PrintTemplate", DocValueMap::new())?,
        };
        let mut values = DocValueMap::new();
        values.insert("name".to_owned(), DocValue::Text(template.name.clone()));
        values.insert("type".to_owned(), DocValue::Text(template.kind.clone()));
        values.insert("template".to_owned(), DocValue::Text(template.template));
        values.insert("isCustom".to_owned(), DocValue::Bool(false));
        if template.width > 0.0 {
            values.insert("width".to_owned(), DocValue::Number(template.width));
        }
        if template.height > 0.0 {
            values.insert("height".to_owned(), DocValue::Number(template.height));
        }
        doc.set_multiple(fyo, values)?;
        doc.sync(fyo)?;
        let mut modified = DocValueMap::new();
        modified.insert("name".to_owned(), DocValue::Text(template.name));
        modified.insert("modified".to_owned(), DocValue::Date(bundled_modified()));
        fyo.db.update("PrintTemplate", &modified)?;
    }
    Ok(())
}

/// The templates for a schema, newest first: `setTemplateList`.
pub fn template_names(fyo: &Fyo, schema_name: &str) -> Vec<String> {
    if fyo.schema("PrintTemplate").is_none() {
        return Vec::new();
    }
    let mut filters = latte_db::QueryFilter::new();
    filters.insert("type".to_owned(), Filter::from(schema_name));
    let options = GetAllOptions {
        filters,
        order: Some(Order::Desc),
        ..GetAllOptions::default()
    };
    fyo.db
        .get_all_docs("PrintTemplate", &options)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|row| match row.get("name") {
            Some(DocValue::Text(name)) => Some(name.clone()),
            _ => None,
        })
        .collect()
}

/// `isPrintable`: whether any template prints the schema.
pub fn can_print(fyo: &Fyo, schema_name: &str) -> bool {
    !template_names(fyo, schema_name).is_empty()
}

/// A PrintTemplate by its name.
pub fn load(fyo: &mut Fyo, name: &str) -> Result<PrintTemplate, Error> {
    let doc = fyo.get_doc("PrintTemplate", Some(name))?;
    Ok(PrintTemplate::from_doc(&doc))
}

/// `setTemplateFromDefault`: the template Defaults names for the document's schema, or
/// for a POS sale the POS profile's or Defaults' POS template. When none is named or
/// the one named isn't there, the first of the schema's templates, as Books falls back
/// (without Books' error dialog for a missing one).
pub fn default_template_name(fyo: &mut Fyo, doc: &Doc) -> Option<String> {
    let schema_name = doc.schema_name().to_owned();
    let list = template_names(fyo, &schema_name);
    let text = |v: Option<DocValue>| match v {
        Some(DocValue::Text(t)) if !t.is_empty() => Some(t),
        _ => None,
    };
    // `fyo.singles.Defaults`: with its defaults for what isn't set.
    let defaults = fyo.get_doc("Defaults", None).ok();
    let default = |field: &str| text(defaults.as_ref().and_then(|d| d.get(field).cloned()));
    let named = if schema_name == "SalesInvoice" && is_truthy(doc.get("isPOS")) {
        let profile = text(fyo.get_value("POSSettings", None, "posProfile"));
        let from_profile = profile.and_then(|p| {
            fyo.get_doc("POSProfile", Some(&p))
                .ok()
                .and_then(|d| text(d.get("posPrintTemplate").cloned()))
        });
        from_profile.or_else(|| default("posPrintTemplate"))
    } else {
        let mut chars = schema_name.chars();
        let first = chars.next()?;
        let fieldname = format!("{}{}PrintTemplate", first.to_lowercase(), chars.as_str());
        default(&fieldname)
    };
    match named {
        Some(name) if list.contains(&name) => Some(name),
        _ => list.into_iter().next(),
    }
}

/// A new PrintTemplate for a schema (`New Template`), or a custom copy of a template
/// (`Duplicate Template`): the document to open in the Template Builder.
pub fn new_template(fyo: &Fyo, kind: &str, template: Option<&str>) -> Result<Doc, Error> {
    let mut values = DocValueMap::new();
    values.insert("type".to_owned(), DocValue::Text(kind.to_owned()));
    values.insert(
        "template".to_owned(),
        DocValue::Text(template.unwrap_or(BASE_TEMPLATE).to_owned()),
    );
    values.insert("isCustom".to_owned(), DocValue::Bool(true));
    fyo.get_new_doc("PrintTemplate", values)
}
