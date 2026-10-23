// Port of models/baseModels/SalesQuote/SalesQuote.ts @ v0.36.0. The rest of the model is
// Invoice's (invoice.rs), with no posting; its `afterSubmit` is there too. SalesQuoteItem
// is InvoiceItem's.
//
// Not ported yet, as the UI needs them: `getListViewSettings` and `getActions`.
//
// Deviation: `getInvoice` leaves out the quote's name and meta fields, and its rows' name
// and parent fields, so the invoice and its rows start as new documents. Books copies
// them, and its database sets the parent fields again on saving.

use jiff::Timestamp;
use latte_db::{DocValue, DocValueMap};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::types::DocContext;
use crate::models::base_models::invoice::Invoice;
use crate::models::helpers::single_text;

pub const MODEL: Invoice = Invoice::without_posting(validate_field);

fn validate_field(_: &str, _: &DocValue, _: &DocContext<'_>) -> Result<(), Error> {
    Ok(())
}

/// Fields a new document sets for itself.
const META: [&str; 6] = [
    "name",
    "created",
    "createdBy",
    "modified",
    "modifiedBy",
    "cancelled",
];

/// `getInvoice`: a sales invoice for a submitted quote, or `None` if it has no items.
pub fn get_invoice(quote: &Doc, fyo: &mut Fyo) -> Result<Option<Doc>, Error> {
    if !quote.is_submitted() {
        return Ok(None);
    }

    let mut data = quote.get_valid_dict(false, true);
    for fieldname in META {
        data.shift_remove(fieldname);
    }
    let rows = match data.shift_remove("items") {
        Some(DocValue::Table(rows)) => rows,
        _ => Vec::new(),
    };
    data.insert("date".to_owned(), DocValue::Date(Timestamp::now()));
    data.insert(
        "terms".to_owned(),
        DocValue::Text(single_text(fyo, "Defaults", "salesInvoiceTerms").unwrap_or_default()),
    );
    match single_text(fyo, "Defaults", "salesInvoiceNumberSeries") {
        Some(series) => data.insert("numberSeries".to_owned(), DocValue::Text(series)),
        None => data.shift_remove("numberSeries"),
    };
    data.insert(
        "quote".to_owned(),
        DocValue::Text(quote.name().unwrap_or_default().to_owned()),
    );
    data.insert("submitted".to_owned(), DocValue::Bool(false));

    let mut invoice = fyo.get_new_doc("SalesInvoice", data)?;
    for row in rows {
        let row: DocValueMap = row
            .into_iter()
            .filter(|(fieldname, _)| {
                !matches!(
                    fieldname.as_str(),
                    "name" | "parent" | "parentSchemaName" | "parentFieldname"
                )
            })
            .collect();
        invoice.append(fyo, "items", row)?;
    }
    if invoice.table("items").is_empty() {
        return Ok(None);
    }
    Ok(Some(invoice))
}
