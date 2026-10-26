//! The Template Builder's Key Hints: `getPrintTemplatePropHints` in
//! src/utils/printTemplates.ts @ v0.36.0, and Latte's print library.
//!
//! Besides Books' hints, the document's hints list the values Books adds to every
//! document (`grandTotalInWords`, `date`, …), which Books leaves out of its hints.

use latte_core::Fyo;
use latte_schema::{FieldType, Schema};

/// A key and what it holds.
#[derive(Debug, Clone, PartialEq)]
pub enum Hint {
    /// A value, with its label.
    Value(String),
    /// An object's keys.
    Object(Vec<(String, Hint)>),
    /// A list of objects with these keys, such as a table's rows.
    Array(Vec<(String, Hint)>),
}

/// `printSettingsFields` and `accountingSettingsFields`.
const PRINT_FIELDS: [&str; 11] = [
    "logo",
    "displayLogo",
    "color",
    "font",
    "email",
    "phone",
    "address",
    "companyName",
    "amountInWords",
    "displaytermsandconditions",
    "termsAndConditions",
];
const ACCOUNTING_FIELDS: [&str; 2] = ["gstin", "taxId"];

/// `{ doc, print }`'s keys for a template of `schema_name`.
pub fn hints(fyo: &Fyo, schema_name: &str, t: &dyn Fn(&str) -> String) -> Vec<(String, Hint)> {
    let mut doc = match fyo.schema(schema_name) {
        Some(schema) => doc_hints(fyo, schema, None, 0, t),
        None => Vec::new(),
    };
    if schema_name.ends_with("Invoice") {
        doc.push(("totalDiscount".to_owned(), Hint::Value(t("Total Discount"))));
        doc.push(("showHSN".to_owned(), Hint::Value(t("Show HSN"))));
    }
    // What `getPrintTemplatePropValues` adds, which Books' hints leave out.
    let mut extra = vec![
        ("date", "Date"),
        ("time", "Time"),
        ("subTotal", "Subtotal"),
        ("grandTotalInWords", "Grand Total In Words"),
        ("description", "Display Description"),
    ];
    if schema_name == "Payment" {
        extra.push(("amountPaidInWords", "Amount Paid In Words"));
    }
    for (key, label) in extra {
        if !doc.iter().any(|(k, _)| k == key) {
            doc.push((key.to_owned(), Hint::Value(t(label))));
        }
    }
    if schema_name == "SalesInvoice" || schema_name == "PurchaseInvoice" {
        doc.push((
            "paymentDetails".to_owned(),
            Hint::Array(vec![
                ("amount".to_owned(), Hint::Value(t("Amount"))),
                ("amountPaid".to_owned(), Hint::Value(t("Amount Paid"))),
                ("paymentMethod".to_owned(), Hint::Value(t("Payment Method"))),
                (
                    "outstandingAmount".to_owned(),
                    Hint::Value(t("Outstanding Amount")),
                ),
            ]),
        ));
    }

    let mut print = Vec::new();
    if let Some(schema) = fyo.schema("PrintSettings") {
        print.extend(doc_hints(fyo, schema, Some(&PRINT_FIELDS), 0, t));
    }
    if let Some(schema) = fyo.schema("AccountingSettings") {
        print.extend(doc_hints(fyo, schema, Some(&ACCOUNTING_FIELDS), 0, t));
    }
    vec![
        ("doc".to_owned(), Hint::Object(doc)),
        ("print".to_owned(), Hint::Object(print)),
    ]
}

/// `getPrintTemplateDocHints`.
fn doc_hints(
    fyo: &Fyo,
    schema: &Schema,
    fieldnames: Option<&[&str]>,
    link_level: usize,
    t: &dyn Fn(&str) -> String,
) -> Vec<(String, Hint)> {
    let mut hints = Vec::new();
    let mut links = Vec::new();
    for field in &schema.fields {
        if fieldnames.is_some_and(|names| !names.contains(&field.fieldname.as_str())) {
            continue;
        }
        if field.fieldtype == Some(FieldType::Attachment) || field.meta {
            continue;
        }
        let label = field
            .label
            .clone()
            .unwrap_or_else(|| field.fieldname.clone());
        let target = field.target.as_deref().and_then(|t| fyo.schema(t));
        match (field.fieldtype, target) {
            (Some(FieldType::Table), Some(target)) => {
                let rows = doc_hints(fyo, target, None, 0, t);
                hints.push((field.fieldname.clone(), Hint::Array(rows)));
            }
            (Some(FieldType::Link), Some(target)) if link_level < 2 => {
                hints.push((field.fieldname.clone(), Hint::Value(label)));
                let linked = doc_hints(fyo, target, None, link_level + 1, t);
                links.push((field.fieldname.clone(), Hint::Object(linked)));
            }
            _ => hints.push((field.fieldname.clone(), Hint::Value(label))),
        }
    }
    hints.push(("submitted".to_owned(), Hint::Value(t("Submitted"))));
    hints.push(("entryType".to_owned(), Hint::Value(t("Entry Type"))));
    hints.push(("entryLabel".to_owned(), Hint::Value(t("Entry Label"))));
    if !links.is_empty() {
        hints.push(("links".to_owned(), Hint::Object(links)));
    }
    hints
}

/// What Latte's print library gives a template besides `doc` and `print`, for the Key
/// Hints: the name and what it is.
pub const LIBRARY_HINTS: [(&str, &str); 19] = [
    ("setup", "#show: setup lays the page out as Books does"),
    ("t(\"Text\")", "Text in the app's language"),
    ("has(value)", "Whether a value is there, as v-if decides"),
    ("get(doc, \"links\", \"party\")", "A key's value, or none"),
    ("rows(doc.items)", "A table's rows, or none"),
    ("lines(text)", "Text with its line breaks"),
    ("px(16)", "A CSS pixel: 0.75pt"),
    ("sp(4)", "Tailwind spacing: 4 is 16px"),
    (
        "text-size.lg",
        "Books' text sizes: xs, sm, base, lg, xl, 2xl, 3xl, 4xl",
    ),
    ("tw(\"gray\", 600)", "A colour of Books' palette"),
    ("brand", "The Print Settings colour"),
    ("border", "Tailwind's 1px gray border"),
    ("show-logo", "Whether to show the logo"),
    ("logo()", "The company's logo"),
    ("picture(src, height: px(48))", "An image from the document"),
    (
        "font-family(print.font)",
        "The bundled font for a font name",
    ),
    (
        "line-height(1.625, body)",
        "Lines 1.625 times the text size",
    ),
    ("page-size.width", "The template's page width"),
    (
        "Fonts",
        "Liberation Sans (Arial), Liberation Serif (Times New Roman), Liberation Mono (Courier), Inter, Libertinus Serif, New Computer Modern, DejaVu Sans Mono",
    ),
];
