//! The values a print template reads: `getPrintTemplatePropValues` in
//! `src/utils/printTemplates.ts` @ v0.36.0.
//!
//! Differences from Books:
//! - Linked documents are followed two links deep (`doc.links.party.links.address`),
//!   which is as deep as the templates read. Books follows every link.
//! - A Payment's `taxes` are the Sales Invoice's tax rows formatted like other rows,
//!   and an empty list when the payment isn't for a Sales Invoice. Books passes the
//!   invoice's raw rows, and nothing at all otherwise, which the Payment template reads
//!   as `doc.taxes.length`.

use jiff::tz::TimeZone;
use latte_core::fyo::format::{Formatter, local_date};
use latte_core::fyo::model::helpers::{is_truthy, js_number_string};
use latte_core::models::base_models::{invoice, payment};
use latte_core::{Doc, Error, Fyo};
use latte_db::DocValue;
use latte_db::pesa::Money;
use latte_schema::FieldType;
use serde_json::{Map, Value, json};

use crate::words::in_words;

/// How deep `links` go.
const LINK_DEPTH: usize = 2;

/// `printSettingsFields`.
const PRINT_SETTINGS_FIELDS: [&str; 11] = [
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

/// `accountingSettingsFields`.
const ACCOUNTING_SETTINGS_FIELDS: [&str; 2] = ["gstin", "taxId"];

/// `{ doc, print }` for `doc`, as the templates read them.
pub fn print_values(
    fyo: &mut Fyo,
    formatter: &Formatter,
    doc: &Doc,
    t: &dyn Fn(&str) -> String,
) -> Result<Value, Error> {
    let schema_name = doc.schema_name().to_owned();
    let mut values = doc_values(fyo, formatter, doc, None, 0)?;

    if schema_name == "SalesInvoice" || schema_name == "PurchaseInvoice" {
        let ids = invoice::get_payment_ids(doc, fyo)?;
        if !ids.is_empty() {
            let details = payment_details(fyo, formatter, doc, ids)?;
            values.insert("paymentDetails".to_owned(), Value::Array(details));
        }
    }

    let mut invoice_doc = None;
    if schema_name == "Payment" {
        values.insert("taxes".to_owned(), Value::Array(Vec::new()));
    }
    if text(doc.get("referenceType")) == "SalesInvoice" {
        let reference = doc
            .table("for")
            .first()
            .map(|row| text(row.get("referenceName")));
        if let Some(reference) = reference.filter(|r| !r.is_empty()) {
            let sinv = fyo.get_doc("SalesInvoice", Some(&reference))?;
            let taxes = sinv
                .table("taxes")
                .iter()
                .map(|row| doc_values(fyo, formatter, row, None, LINK_DEPTH).map(Value::Object))
                .collect::<Result<Vec<_>, _>>()?;
            values.insert("taxes".to_owned(), Value::Array(taxes));
            invoice_doc = Some(sinv);
        }
    }

    let total_tax = if schema_name == "Shipment" {
        None
    } else if let Some(sinv) = &invoice_doc {
        Some(sum_amounts(fyo, invoice::get_tax_summary(sinv, fyo)?)?)
    } else if schema_name == "Payment" {
        Some(sum_amounts(fyo, payment::get_tax_summary(doc, fyo)?)?)
    } else if doc.schema().field("taxes").is_some() {
        Some(sum_amounts(fyo, invoice::get_tax_summary(doc, fyo)?)?)
    } else {
        None
    };

    if schema_name == "Payment" {
        let paid = money(doc.get("amountPaid")).map_or(0.0, |m| m.float());
        values.insert("amountPaidInWords".to_owned(), json!(in_words(paid, t)));
    }

    let total = money(doc.get("grandTotal")).or_else(|| money(doc.get("amount")));
    if let Some(total) = &total {
        let sub_total = match &total_tax {
            Some(tax) => total.sub(tax).map_err(Error::from)?,
            None => total.clone(),
        };
        values.insert("subTotal".to_owned(), json!(formatter.currency(&sub_total)));
    }

    let print_settings = fyo.get_doc("PrintSettings", None)?;
    let mut print = doc_values(
        fyo,
        formatter,
        &print_settings,
        Some(&PRINT_SETTINGS_FIELDS),
        0,
    )?;
    let accounting = fyo.get_doc("AccountingSettings", None)?;
    print.extend(doc_values(
        fyo,
        formatter,
        &accounting,
        Some(&ACCOUNTING_SETTINGS_FIELDS),
        0,
    )?);

    if schema_name.ends_with("Invoice") || schema_name.ends_with("Quote") {
        let discount = if doc.schema().field("enableDiscounting").is_some() {
            invoice::get_total_discount(doc, fyo)?
        } else {
            fyo.pesa().from_f64(0.0).map_err(Error::from)?
        };
        let discount = if discount.float() == 0.0 {
            String::new()
        } else {
            formatter.currency(&discount)
        };
        values.insert("totalDiscount".to_owned(), json!(discount));
    }

    let items = doc.table("items");
    let show_hsn = !items.is_empty() && items.iter().all(|row| is_truthy(row.get("hsnCode")));
    values.insert("showHSN".to_owned(), json!(show_hsn));
    let total = total.map_or(0.0, |m| m.float());
    values.insert("grandTotalInWords".to_owned(), json!(in_words(total, t)));

    if let Some(DocValue::Date(date)) = doc.get("date") {
        let zoned = date.to_zoned(TimeZone::system());
        values.insert(
            "date".to_owned(),
            json!(zoned.strftime("%b %-d, %Y").to_string()),
        );
        if is_truthy(print_settings.get("displayTime")) {
            values.insert(
                "time".to_owned(),
                json!(zoned.strftime("%H:%M:%S").to_string()),
            );
        }
    }
    if is_truthy(print_settings.get("displayDescription")) {
        let any = items.iter().any(|row| is_truthy(row.get("description")));
        values.insert("description".to_owned(), json!(any));
    }

    Ok(json!({ "doc": values, "print": print }))
}

/// `getPaymentDetails`.
fn payment_details(
    fyo: &mut Fyo,
    formatter: &Formatter,
    doc: &Doc,
    mut ids: Vec<String>,
) -> Result<Vec<Value>, Error> {
    ids.sort();
    let mut outstanding = match money(doc.get("grandTotal")) {
        Some(total) => total,
        None => fyo.pesa().from_f64(0.0).map_err(Error::from)?,
    };
    let mut details = Vec::new();
    for id in ids {
        let payment = fyo.get_doc("Payment", Some(&id))?;
        let amount = money(payment.get("amount"));
        if let Some(amount) = &amount {
            outstanding = outstanding.sub(amount).map_err(Error::from)?;
        }
        let shown = |m: Option<Money>| m.map(|m| formatter.currency(&m)).unwrap_or_default();
        details.push(json!({
            "amount": shown(amount),
            "amountPaid": shown(money(payment.get("amountPaid"))),
            "paymentMethod": text(payment.get("paymentMethod")),
            "outstandingAmount": formatter.currency(&outstanding),
        }));
    }
    Ok(details)
}

fn sum_amounts(fyo: &Fyo, rows: Vec<latte_db::DocValueMap>) -> Result<Money, Error> {
    let mut sum = fyo.pesa().from_f64(0.0).map_err(Error::from)?;
    for row in rows {
        if let Some(amount) = money(row.get("amount")) {
            sum = sum.add(&amount).map_err(Error::from)?;
        }
    }
    Ok(sum)
}

fn money(value: Option<&DocValue>) -> Option<Money> {
    match value {
        Some(DocValue::Money(money)) => Some(money.clone()),
        _ => None,
    }
}

fn text(value: Option<&DocValue>) -> String {
    match value {
        Some(DocValue::Text(text)) => text.clone(),
        _ => String::new(),
    }
}

/// `getPrintTemplateDocValues`: every field formatted, tables as lists of rows, and the
/// linked documents under `links`.
fn doc_values(
    fyo: &mut Fyo,
    formatter: &Formatter,
    doc: &Doc,
    fieldnames: Option<&[&str]>,
    depth: usize,
) -> Result<Map<String, Value>, Error> {
    let schema = doc.schema().clone();
    let fields = schema
        .fields
        .iter()
        .filter(|f| fieldnames.is_none_or(|names| names.contains(&f.fieldname.as_str())))
        .filter(|f| f.fieldtype != Some(FieldType::Attachment) && !f.meta);

    let mut values = Map::new();
    for field in fields.clone() {
        let value = if field.fieldtype == Some(FieldType::Table) {
            let rows = doc
                .table(&field.fieldname)
                .iter()
                .map(|row| doc_values(fyo, formatter, row, None, LINK_DEPTH).map(Value::Object))
                .collect::<Result<Vec<_>, _>>()?;
            if rows.is_empty() {
                Value::from("")
            } else {
                Value::Array(rows)
            }
        } else {
            Value::from(format_value(formatter, field, doc.get(&field.fieldname)))
        };
        values.insert(field.fieldname.clone(), value);
    }
    values.insert("submitted".to_owned(), json!(doc.is_submitted()));
    values.insert("entryType".to_owned(), json!(schema.name));
    values.insert("entryLabel".to_owned(), json!(schema.label));

    if depth >= LINK_DEPTH {
        return Ok(values);
    }
    let mut links = Map::new();
    for field in fields {
        let target = match field.fieldtype {
            Some(FieldType::Link) => field.target.clone(),
            Some(FieldType::DynamicLink) => field
                .references
                .as_deref()
                .map(|r| text(doc.get(r)))
                .filter(|t| !t.is_empty()),
            _ => None,
        };
        let name = text(doc.get(&field.fieldname));
        let Some(target) = target.filter(|_| !name.is_empty()) else {
            continue;
        };
        if fyo.schema(&target).is_none() {
            continue;
        }
        let Ok(linked) = fyo.get_doc(&target, Some(&name)) else {
            continue;
        };
        let linked = doc_values(fyo, formatter, &linked, None, depth + 1)?;
        links.insert(field.fieldname.clone(), Value::Object(linked));
    }
    if !links.is_empty() {
        values.insert("links".to_owned(), Value::Object(links));
    }
    Ok(values)
}

/// `fyo.format` for a print template: an empty value (`!value` in JavaScript) is an
/// empty string.
fn format_value(
    formatter: &Formatter,
    field: &latte_schema::Field,
    value: Option<&DocValue>,
) -> String {
    let Some(value) = value else {
        return String::new();
    };
    match (field.fieldtype, value) {
        (_, DocValue::Null) | (_, DocValue::Bool(false)) => String::new(),
        (_, DocValue::Text(text)) if text.is_empty() => String::new(),
        (_, DocValue::Number(n)) if *n == 0.0 || n.is_nan() => String::new(),
        (Some(FieldType::Float), DocValue::Number(n)) => {
            format!("{n:.precision$}", precision = formatter.display_precision)
        }
        (Some(FieldType::Int), DocValue::Number(n)) => format!("{}", n.trunc() as i64),
        (_, DocValue::Money(money)) => formatter.currency(money),
        (Some(FieldType::Datetime), DocValue::Date(ts)) => {
            let zoned = ts.to_zoned(TimeZone::system());
            format!(
                "{} {}",
                formatter.date(zoned.date()),
                zoned.time().strftime("%H:%M")
            )
        }
        (_, DocValue::Date(ts)) => formatter.date(local_date(*ts)),
        (Some(FieldType::Check), _) | (_, DocValue::Bool(true)) => "True".to_owned(),
        (_, DocValue::Number(n)) => js_number_string(*n),
        (_, DocValue::Text(text)) => text.clone(),
        (_, DocValue::Attachment(value)) => value
            .get("name")
            .and_then(|name| name.as_str())
            .unwrap_or_default()
            .to_owned(),
        (_, DocValue::Table(rows)) => format!("{} rows", rows.len()),
    }
}
