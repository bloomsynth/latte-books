// Port of models/inventory/StockMovement.ts @ v0.36.0. A stock movement makes no ledger
// entries (`isTransactional` is false), only stock ledger entries.
//
// Books bug Latte fixes: a new stock movement takes the number series set in Defaults.
// Books' StockMovement has no `numberSeries` default, so it always takes the schema's.
//
// Not ported yet, as the UI needs them: `getListViewSettings`, `getActions` and `addItem`
// (the barcode scanner).

use jiff::Timestamp;
use latte_db::{DocValue, Filter, QueryFilter};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::types::{DocContext, Formula, Model};
use crate::models::helpers::{get_number_series, query, text};
use crate::models::inventory::helpers::{
    can_validate_serial_number, get_serial_number_from_doc, serial_number_status,
    update_serial_numbers, validate_batch, validate_serial_number,
};
use crate::models::inventory::stock_manager::TransferDetails;
use crate::models::inventory::transfer;

pub struct StockMovement;

fn non_empty(row: &Doc, fieldname: &str) -> Option<String> {
    text(row, fieldname)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

/// `_getTransferDetails`.
fn transfer_details(doc: &Doc) -> Vec<TransferDetails> {
    doc.table("items")
        .iter()
        .map(|row| TransferDetails {
            item: text(row, "item").unwrap_or_default().to_owned(),
            rate: crate::models::helpers::money(row, "rate"),
            quantity: crate::models::helpers::number(row, "quantity").unwrap_or(0.0),
            batch: non_empty(row, "batch"),
            serial_number: non_empty(row, "serialNumber"),
            from_location: non_empty(row, "fromLocation"),
            to_location: non_empty(row, "toLocation"),
            is_return: false,
        })
        .collect()
}

/// `validateManufacture`.
fn validate_manufacture(doc: &Doc) -> Result<(), Error> {
    if text(doc, "movementType") != Some("Manufacture") {
        return Ok(());
    }
    let items = doc.table("items");
    if !items
        .iter()
        .any(|row| non_empty(row, "fromLocation").is_some())
    {
        return Err(Error::Validation(
            "Item with From location not found".into(),
        ));
    }
    if !items
        .iter()
        .any(|row| non_empty(row, "toLocation").is_some())
    {
        return Err(Error::Validation("Item with To location not found".into()));
    }
    Ok(())
}

/// `validateSerialNumberStatus`.
fn validate_serial_number_status(doc: &Doc, fyo: &Fyo) -> Result<(), Error> {
    if doc.is_cancelled() {
        return Ok(());
    }

    let movement_type = text(doc, "movementType");
    for (serial_number, row) in get_serial_number_from_doc(doc) {
        if !can_validate_serial_number(fyo, doc.schema_name(), row, &serial_number)? {
            continue;
        }
        if !fyo.db.exists("SerialNumber", Some(&serial_number))? {
            continue;
        }

        let status = serial_number_status(fyo, &serial_number);
        let message = match movement_type {
            Some("MaterialReceipt") if status != "Inactive" => Some(format!(
                "Non Inactive Serial Number {serial_number} cannot be used for Material Receipt"
            )),
            Some("MaterialIssue") if status != "Active" => Some(format!(
                "Non Active Serial Number {serial_number} cannot be used for Material Issue"
            )),
            Some("MaterialTransfer") if status != "Active" => Some(format!(
                "Non Active Serial Number {serial_number} cannot be used for Material Transfer"
            )),
            _ => None,
        };
        if let Some(message) = message {
            return Err(Error::Validation(message));
        }
        if non_empty(row, "fromLocation").is_some() && status != "Active" {
            return Err(Error::Validation(format!(
                "Non Active Serial Number {serial_number} cannot be used as Manufacture raw material"
            )));
        }
    }
    Ok(())
}

impl Model for StockMovement {
    fn default(&self, fieldname: &str, doc: &Doc, fyo: &Fyo) -> Option<DocValue> {
        match fieldname {
            "numberSeries" => get_number_series(doc.schema_name(), fyo).map(DocValue::Text),
            "date" => Some(DocValue::Date(Timestamp::now())),
            _ => None,
        }
    }

    fn link_filters(&self, fieldname: &str, _ctx: &DocContext<'_>) -> Option<QueryFilter> {
        (fieldname == "numberSeries")
            .then(|| query([("referenceType", Filter::from("StockMovement"))]))
    }

    fn formulas(&self) -> Vec<Formula> {
        vec![Formula {
            fieldname: "amount",
            depends_on: Some(&["items"]),
            formula: |ctx, _| {
                let amount = ctx.doc.get_sum(ctx.fyo, "items", "amount")?;
                Ok(Some(DocValue::Money(amount).into()))
            },
        }]
    }

    fn validate(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        validate_manufacture(doc)?;
        validate_batch(doc, fyo)?;
        validate_serial_number(doc, fyo)?;
        validate_serial_number_status(doc, fyo)
    }

    fn before_submit(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        transfer::before_submit(doc, fyo, &transfer_details(doc))
    }

    fn after_submit(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        transfer::after_submit(doc, fyo, &transfer_details(doc))?;
        update_serial_numbers(doc, fyo, false, false)
    }

    fn before_cancel(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        transfer::before_cancel(doc, fyo, &transfer_details(doc))
    }

    fn after_cancel(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        transfer::after_cancel(doc, fyo)?;
        update_serial_numbers(doc, fyo, true, false)
    }
}
