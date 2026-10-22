// Port of the parts of models/helpers.ts @ v0.36.0 that the ported models use, and the
// reads of document and single values they share.

use latte_db::DocValue;
use latte_db::pesa::Money;

use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::helpers::is_truthy;

/// `DEFAULT_CURRENCY` in fyo/utils/consts.ts.
pub const DEFAULT_CURRENCY: &str = "INR";

/// `numberSeriesDefaultsMap` in models/baseModels/Defaults/Defaults.ts.
fn number_series_defaults_key(schema_name: &str) -> Option<&'static str> {
    Some(match schema_name {
        "SalesInvoice" => "salesInvoiceNumberSeries",
        "PurchaseInvoice" => "purchaseInvoiceNumberSeries",
        "JournalEntry" => "journalEntryNumberSeries",
        "Payment" => "paymentNumberSeries",
        "StockMovement" => "stockMovementNumberSeries",
        "Shipment" => "shipmentNumberSeries",
        "PurchaseReceipt" => "purchaseReceiptNumberSeries",
        "SalesQuote" => "salesQuoteNumberSeries",
        _ => return None,
    })
}

/// `getNumberSeries`: the series set in Defaults, or the schema's default.
pub fn get_number_series(schema_name: &str, fyo: &Fyo) -> Option<String> {
    let key = number_series_defaults_key(schema_name)?;
    match fyo.get_value("Defaults", None, key) {
        Some(DocValue::Text(series)) => Some(series),
        _ => match fyo
            .schema(schema_name)?
            .field("numberSeries")?
            .default
            .as_ref()?
        {
            latte_schema::RawValue::Text(series) => Some(series.clone()),
            _ => None,
        },
    }
}

/// `doc[fieldname]` as text, if it is text.
pub(crate) fn text<'a>(doc: &'a Doc, fieldname: &str) -> Option<&'a str> {
    match doc.get(fieldname) {
        Some(DocValue::Text(text)) => Some(text),
        _ => None,
    }
}

/// `doc[fieldname]` as a number, if it is one.
pub(crate) fn number(doc: &Doc, fieldname: &str) -> Option<f64> {
    match doc.get(fieldname) {
        Some(DocValue::Number(number)) => Some(*number),
        _ => None,
    }
}

/// `doc[fieldname]` as money, if it is money.
pub(crate) fn money(doc: &Doc, fieldname: &str) -> Option<Money> {
    match doc.get(fieldname) {
        Some(DocValue::Money(money)) => Some(money.clone()),
        _ => None,
    }
}

/// `doc[fieldname] ?? fyo.pesa(0)`.
pub(crate) fn money_or_zero(doc: &Doc, fieldname: &str, fyo: &Fyo) -> Money {
    money(doc, fieldname).unwrap_or_else(|| fyo.pesa().zero())
}

/// `fyo.singles[schema_name]?.[fieldname]`: a single's stored value.
pub(crate) fn single_value(fyo: &Fyo, schema_name: &str, fieldname: &str) -> Option<DocValue> {
    fyo.get_value(schema_name, None, fieldname)
}

/// `!!fyo.singles[schema_name]?.[fieldname]`.
pub(crate) fn single_is_truthy(fyo: &Fyo, schema_name: &str, fieldname: &str) -> bool {
    is_truthy(single_value(fyo, schema_name, fieldname).as_ref())
}

/// A single's value if it is text that isn't empty.
pub(crate) fn single_text(fyo: &Fyo, schema_name: &str, fieldname: &str) -> Option<String> {
    match single_value(fyo, schema_name, fieldname) {
        Some(DocValue::Text(text)) if !text.is_empty() => Some(text),
        _ => None,
    }
}

/// `fyo.singles.SystemSettings?.currency ?? DEFAULT_CURRENCY`.
pub(crate) fn company_currency(fyo: &Fyo) -> String {
    match single_value(fyo, "SystemSettings", "currency") {
        Some(DocValue::Text(currency)) => currency,
        _ => DEFAULT_CURRENCY.to_owned(),
    }
}

/// `fyo.getValue(schema_name, name, fieldname)` for a document named by a field: `None`
/// if the field isn't set, as Books returns `undefined` for a null name.
pub(crate) fn linked_value(
    fyo: &Fyo,
    schema_name: &str,
    name: Option<&str>,
    fieldname: &str,
) -> Option<DocValue> {
    fyo.get_value(schema_name, Some(name?), fieldname)
}
