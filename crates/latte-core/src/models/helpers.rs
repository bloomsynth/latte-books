// Port of the parts of models/helpers.ts @ v0.36.0 that the ported models use.

use latte_db::DocValue;

use crate::fyo::Fyo;

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
