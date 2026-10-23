//! Renders invoices, quotes and payments to PDF with Typst.
//!
//! Replaces Books' HTML print templates (`templates/*.template.html` @ v0.36.0) with
//! Typst ports of them in `src/templates`; see docs/DEVIATIONS.md. The values a template
//! reads are built as in Books' `getPrintTemplatePropValues` (see [`values`]).

mod values;
mod words;
mod world;

use base64::Engine as _;
use latte_core::fyo::format::Formatter;
use latte_core::{Doc, Fyo};
use latte_db::DocValue;
use serde_json::{Map, Value};
use typst::utils::Scalar;
use typst_layout::PagedDocument;

pub use values::print_values;
pub use words::in_words;

const COMMON: &str = include_str!("templates/common.typ");

/// The words the templates translate with `t`.
const LABELS: [&str; 19] = [
    "Item",
    "Description",
    "HSN/SAC",
    "Quantity",
    "Rate",
    "Amount",
    "Notes",
    "Subtotal",
    "Discount",
    "Grand Total",
    "Grand Total In Words",
    "Terms and Conditions",
    "Bill",
    "party",
    "Payment Type",
    "Payment Method",
    "Tax Summary",
    "Total Ex.Tax",
    "Amount Paid",
];

/// One of Books' bundled print templates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Basic,
    Business,
    Minimal,
    BusinessPayment,
}

impl Kind {
    /// The template's name in its file name (`Business.Payment.template.html`).
    fn file_name(self) -> &'static str {
        match self {
            Self::Basic => "Basic",
            Self::Business | Self::BusinessPayment => "Business",
            Self::Minimal => "Minimal",
        }
    }

    fn source(self) -> &'static str {
        match self {
            Self::Basic => include_str!("templates/basic.typ"),
            Self::Business => include_str!("templates/business.typ"),
            Self::Minimal => include_str!("templates/minimal.typ"),
            Self::BusinessPayment => include_str!("templates/payment.typ"),
        }
    }
}

/// A template for one schema, named as Books names its PrintTemplate rows:
/// "Business - Sales Invoice".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    pub name: String,
    pub kind: Kind,
}

/// The templates that print `schema_name`, as `getNameAndTypeFromTemplateFile` gives them
/// out: invoice templates serve Sales Invoices, Quotes and Purchase Invoices.
pub fn templates(schema_name: &str) -> Vec<Template> {
    let kinds: &[Kind] = match schema_name {
        "SalesInvoice" | "SalesQuote" | "PurchaseInvoice" => {
            &[Kind::Basic, Kind::Business, Kind::Minimal]
        }
        "Payment" => &[Kind::BusinessPayment],
        _ => &[],
    };
    // The English label, as Books names the rows when it makes them at setup: the
    // names stay the same whatever the app's language.
    let label = match schema_name {
        "SalesInvoice" => "Sales Invoice",
        "SalesQuote" => "Sales Quote",
        "PurchaseInvoice" => "Purchase Invoice",
        _ => "Payment",
    };
    kinds
        .iter()
        .map(|&kind| Template {
            name: format!("{} - {label}", kind.file_name()),
            kind,
        })
        .collect()
}

/// `setTemplateFromDefault`: the template Defaults names for the schema.
///
/// Difference from Books: when Defaults names no template, or one Latte doesn't have,
/// the Business template is used, as Defaults' own default names it. Books shows no
/// template until one is picked.
pub fn default_template(fyo: &Fyo, schema_name: &str) -> Option<Template> {
    let list = templates(schema_name);
    let mut chars = schema_name.chars();
    let first = chars.next()?;
    let fieldname = format!("{}{}PrintTemplate", first.to_lowercase(), chars.as_str());
    let named = match fyo.get_value("Defaults", None, &fieldname) {
        Some(DocValue::Text(name)) => list.iter().find(|t| t.name == name).cloned(),
        _ => None,
    };
    named.or_else(|| {
        let business = list.iter().position(|t| t.kind.file_name() == "Business");
        list.into_iter().nth(business.unwrap_or(0))
    })
}

/// Whether Latte has a template for `schema_name`.
pub fn can_print(schema_name: &str) -> bool {
    !templates(schema_name).is_empty()
}

/// A laid out document, ready to save as PDF or show as images.
pub struct Printout {
    document: PagedDocument,
}

/// Lays out `doc` with `template`. `t` translates the template's words.
pub fn print(
    fyo: &mut Fyo,
    formatter: &Formatter,
    doc: &Doc,
    template: &Template,
    t: &dyn Fn(&str) -> String,
) -> Result<Printout, String> {
    let mut values = print_values(fyo, formatter, doc, t).map_err(|e| e.to_string())?;
    let logo = values
        .pointer("/print/logo")
        .and_then(Value::as_str)
        .and_then(decode_data_url);

    let labels: Map<String, Value> = LABELS
        .iter()
        .map(|label| ((*label).to_owned(), Value::from(t(label))))
        .collect();
    if let Value::Object(map) = &mut values {
        map.insert("t".to_owned(), Value::Object(labels));
        map.insert("hasLogo".to_owned(), Value::Bool(logo.is_some()));
    }
    render(template.kind, &values, logo)
}

/// Lays out a template with the given values.
pub fn render(kind: Kind, values: &Value, logo: Option<Vec<u8>>) -> Result<Printout, String> {
    let data = serde_json::to_vec(values).map_err(|e| e.to_string())?;
    let mut files = vec![("/data.json", data)];
    if let Some(logo) = logo {
        files.push(("/logo", logo));
    }
    let world = world::PrintWorld::new(
        &[
            (world::sources_main(), kind.source()),
            ("/common.typ", COMMON),
        ],
        files,
    );
    let document = typst::compile::<PagedDocument>(&world)
        .output
        .map_err(|errors| {
            errors
                .iter()
                .map(|e| e.message.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        })?;
    Ok(Printout { document })
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Kind, LABELS, render};

    fn values() -> serde_json::Value {
        let item = |name: &str, hsn: &str| {
            json!({
                "item": name, "description": "", "hsnCode": hsn, "quantity": "2.00",
                "rate": "₹ 150.00", "amount": "₹ 300.00",
            })
        };
        json!({
            "doc": {
                "name": "SINV-1001", "date": "Oct 23, 2026", "party": "Acme Traders",
                "entryType": "SalesInvoice", "entryLabel": "Sales Invoice",
                "items": [item("Widget", "8471"), item("Gadget", "")],
                "taxes": [{ "account": "CGST", "amount": "₹ 27.00" }],
                "netTotal": "₹ 600.00", "grandTotal": "₹ 654.00", "totalDiscount": "",
                "discountAfterTax": "", "showHSN": false, "terms": "Pay within\n30 days",
                "grandTotalInWords": "Six Hundred And Fifty Four only",
                "paymentType": "Receive", "paymentMethod": "Cash", "amount": "₹ 654.00",
                "amountPaid": "₹ 654.00", "amountPaidInWords": "Six Hundred And Fifty Four only",
                "subTotal": "₹ 600.00",
                "links": { "party": { "address": "A1", "gstin": "",
                    "links": { "address": { "addressDisplay": "1 Main Road\nChennai" } } } },
            },
            "print": {
                "companyName": "Latte Test Co", "email": "a@b.co", "phone": "", "address": "",
                "gstin": "", "color": "#112B42", "font": "Inter", "displayLogo": "",
                "amountInWords": "True", "displaytermsandconditions": "True",
                "termsAndConditions": "Goods once sold\nare not returned",
            },
            "t": LABELS.iter().map(|l| (l.to_string(), json!(l))).collect::<serde_json::Map<_, _>>(),
            "hasLogo": false,
        })
    }

    #[test]
    fn every_template_lays_out() {
        for kind in [
            Kind::Basic,
            Kind::Business,
            Kind::Minimal,
            Kind::BusinessPayment,
        ] {
            let printout =
                render(kind, &values(), None).unwrap_or_else(|e| panic!("{kind:?}: {e}"));
            assert_eq!(printout.page_count(), 1, "{kind:?}");
            assert!(printout.pdf().unwrap().starts_with(b"%PDF"));
        }
    }

    /// A payment that isn't for a Sales Invoice gets no tax rows, which Books leaves out
    /// and its Payment template then fails on.
    #[test]
    fn a_payment_without_taxes_prints() {
        let mut values = values();
        values["doc"]["entryType"] = json!("Payment");
        values["doc"]["taxes"] = json!([]);
        let printout = render(Kind::BusinessPayment, &values, None).unwrap();
        assert_eq!(printout.page_count(), 1);
    }
}
