// Port of models/inventory/helpers.ts @ v0.36.0, `getStockQuantity` in
// backend/database/bespoke.ts and `getItemVisibility` in models/helpers.ts.

use jiff::Timestamp;
use latte_db::{DocValue, Filter, FilterValue, GetAllOptions, QueryFilter};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::helpers::is_truthy;
use crate::models::helpers::{number, single_text, text};

/// `getItemVisibility`: the POS profile's item visibility, or POS Settings'.
pub fn get_item_visibility(fyo: &Fyo) -> Option<String> {
    let value = match single_text(fyo, "POSSettings", "posProfile") {
        Some(profile) => fyo.get_value("POSProfile", Some(&profile), "itemVisibility"),
        None => fyo.get_value("POSSettings", None, "itemVisibility"),
    };
    match value {
        Some(DocValue::Text(visibility)) => Some(visibility),
        _ => None,
    }
}

/// `getSerialNumbers`: the serial numbers in a Text field, one to a line.
pub fn get_serial_numbers(serial_number: &str) -> Vec<String> {
    serial_number
        .split('\n')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `getStockQuantity`: the sum of an item's stock ledger quantities, after `from` and
/// before `to` (neither included), or `None` if there are no entries.
pub fn get_stock_quantity(
    fyo: &Fyo,
    item: &str,
    location: Option<&str>,
    from: Option<Timestamp>,
    to: Option<Timestamp>,
    batch: Option<&str>,
    serial_numbers: &[String],
) -> Result<Option<f64>, Error> {
    let mut filters = QueryFilter::from([("item".to_owned(), Filter::from(item))]);
    if let Some(location) = location.filter(|l| !l.is_empty()) {
        filters.insert("location".to_owned(), Filter::from(location));
    }
    if let Some(batch) = batch.filter(|b| !b.is_empty()) {
        filters.insert("batch".to_owned(), Filter::from(batch));
    }
    if !serial_numbers.is_empty() {
        let values = serial_numbers
            .iter()
            .map(|sn| FilterValue::from(sn.as_str()))
            .collect();
        filters.insert("serialNumber".to_owned(), Filter::list("in", values));
    }
    let options = GetAllOptions {
        fields: Some(vec!["date".to_owned(), "quantity".to_owned()]),
        filters,
        ..GetAllOptions::default()
    };

    let mut total = None;
    for row in fyo.db.get_all_docs("StockLedgerEntry", &options)? {
        let date = match row.get("date") {
            Some(DocValue::Date(date)) => Some(*date),
            _ => None,
        };
        if from.is_some_and(|from| date.is_none_or(|date| date <= from)) {
            continue;
        }
        if to.is_some_and(|to| date.is_none_or(|date| date >= to)) {
            continue;
        }
        let quantity = match row.get("quantity") {
            Some(DocValue::Number(quantity)) => *quantity,
            _ => 0.0,
        };
        total = Some(total.unwrap_or(0.0) + quantity);
    }
    Ok(total)
}

fn item_flag(fyo: &Fyo, item: &str, fieldname: &str) -> bool {
    is_truthy(fyo.get_value("Item", Some(item), fieldname).as_ref())
}

/// `validateBatch`: a batched item needs a batch on every row, and only it can have one.
pub fn validate_batch(doc: &Doc, fyo: &Fyo) -> Result<(), Error> {
    if doc.schema_name() == "SalesQuote" {
        return Ok(());
    }
    for (idx, row) in doc.table("items").iter().enumerate() {
        let Some(item) = text(row, "item").filter(|i| !i.is_empty()) else {
            continue;
        };
        let has_batch = item_flag(fyo, item, "hasBatch");
        let batch = text(row, "batch").filter(|b| !b.is_empty());
        if !has_batch && batch.is_some() {
            return Err(Error::Validation(format!(
                "Batch set for row {}. Item {item} is not a batched item",
                idx + 1
            )));
        }
        if has_batch && batch.is_none() {
            return Err(Error::Validation(format!(
                "Batch not set for row {}. Item {item} is a batched item",
                idx + 1
            )));
        }
    }
    Ok(())
}

/// `isSerialNumberIncoming`: whether a row brings serial numbers in.
pub fn is_serial_number_incoming(parent_schema: &str, row: &Doc) -> bool {
    match parent_schema {
        "Shipment" => false,
        "PurchaseReceipt" => true,
        _ => {
            text(row, "toLocation").is_some_and(|l| !l.is_empty())
                && text(row, "fromLocation").is_none_or(str::is_empty)
        }
    }
}

/// `validateSerialNumber`: the serial numbers on each row of a stock movement, shipment
/// or receipt.
pub fn validate_serial_number(doc: &Doc, fyo: &Fyo) -> Result<(), Error> {
    if doc.schema_name() == "SalesQuote" || doc.is_cancelled() {
        return Ok(());
    }

    let is_return = is_truthy(doc.get("returnAgainst"));
    let is_submitted = doc.is_submitted();
    for (idx, row) in doc.table("items").iter().enumerate() {
        let Some(item) = text(row, "item").filter(|i| !i.is_empty()) else {
            continue;
        };
        let has_serial_number = item_flag(fyo, item, "hasSerialNumber");
        let serial_number = text(row, "serialNumber").filter(|s| !s.is_empty());
        if has_serial_number && serial_number.is_none() {
            return Err(Error::Validation(format!(
                "Serial Number not set for row {}. Serial Number is enabled for Item {item}",
                idx + 1
            )));
        }
        if !has_serial_number && serial_number.is_some() {
            return Err(Error::Validation(format!(
                "Serial Number set for row {}. Serial Number is not enabled for Item {item}",
                idx + 1
            )));
        }
        let Some(serial_number) = serial_number.filter(|_| has_serial_number) else {
            continue;
        };

        let serial_numbers = get_serial_numbers(serial_number);
        let quantity = number(row, "quantity").unwrap_or(0.0).abs();
        if serial_numbers.len() as f64 != quantity {
            return Err(Error::Validation(format!(
                "Additional {} Serial Numbers required for {} quantity of {item}.",
                js_number(quantity - serial_numbers.len() as f64),
                js_number(quantity)
            )));
        }

        let incoming = is_serial_number_incoming(doc.schema_name(), row);
        let mut new_incoming = Vec::new();
        for serial_number in &serial_numbers {
            if fyo.db.exists("SerialNumber", Some(serial_number))? {
                continue;
            }
            if incoming {
                new_incoming.push(serial_number.clone());
                continue;
            }
            return Err(Error::Validation(format!(
                "Serial Number {serial_number} does not exist."
            )));
        }

        for serial_number in &serial_numbers {
            if new_incoming.contains(serial_number) {
                continue;
            }
            let sn_item = fyo.get_value("SerialNumber", Some(serial_number), "item");
            if sn_item.as_ref() != Some(&DocValue::Text(item.to_owned())) {
                return Err(Error::Validation(format!(
                    "Serial Number {serial_number} does not belong to the item {item}."
                )));
            }
            let status = serial_number_status(fyo, serial_number);
            if doc.schema_name() == "PurchaseReceipt"
                && status != "Inactive"
                && !is_submitted
                && !is_return
            {
                return Err(Error::Validation(format!(
                    "Serial Number {serial_number} is not Inactive"
                )));
            }
            if doc.schema_name() == "Shipment" && status != "Active" && !is_submitted && !is_return
            {
                return Err(Error::Validation(format!(
                    "Serial Number {serial_number} is not Active."
                )));
            }
        }
    }
    Ok(())
}

/// A serial number's status, `Inactive` if it has none.
pub fn serial_number_status(fyo: &Fyo, serial_number: &str) -> String {
    match fyo.get_value("SerialNumber", Some(serial_number), "status") {
        Some(DocValue::Text(status)) if !status.is_empty() => status,
        _ => "Inactive".to_owned(),
    }
}

/// `canValidateSerialNumber`: an incoming serial number is only checked once it exists.
pub fn can_validate_serial_number(
    fyo: &Fyo,
    parent_schema: &str,
    row: &Doc,
    serial_number: &str,
) -> Result<bool, Error> {
    if !is_serial_number_incoming(parent_schema, row) {
        return Ok(true);
    }
    Ok(fyo.db.exists("SerialNumber", Some(serial_number))?)
}

/// `getSerialNumberFromDoc`: each serial number with its row.
pub fn get_serial_number_from_doc(doc: &Doc) -> Vec<(String, &Doc)> {
    doc.table("items")
        .iter()
        .flat_map(|row| {
            get_serial_numbers(text(row, "serialNumber").unwrap_or_default())
                .into_iter()
                .map(move |sn| (sn, row))
        })
        .collect()
}

/// `createSerialNumbers`: makes the incoming serial numbers that don't exist yet, as
/// Active.
pub fn create_serial_numbers(doc: &Doc, fyo: &mut Fyo) -> Result<(), Error> {
    let mut to_create = Vec::new();
    for row in doc.table("items") {
        let item = text(row, "item").unwrap_or_default();
        if item.is_empty() || !is_serial_number_incoming(doc.schema_name(), row) {
            continue;
        }
        for serial_number in get_serial_numbers(text(row, "serialNumber").unwrap_or_default()) {
            to_create.push((item.to_owned(), serial_number));
        }
    }

    for (item, serial_number) in to_create {
        if fyo.db.exists("SerialNumber", Some(&serial_number))? {
            continue;
        }
        let data = latte_db::DocValueMap::from([
            ("name".to_owned(), DocValue::Text(serial_number)),
            ("item".to_owned(), DocValue::Text(item)),
        ]);
        let mut sn = fyo.get_new_doc("SerialNumber", data)?;
        sn.set(fyo, "status", DocValue::Text("Active".to_owned()))?;
        sn.sync(fyo)?;
    }
    Ok(())
}

/// `updateSerialNumbers`: sets the status of each row's serial numbers after a submit or
/// cancel.
pub fn update_serial_numbers(
    doc: &Doc,
    fyo: &mut Fyo,
    is_cancel: bool,
    is_return: bool,
) -> Result<(), Error> {
    let mut updates = Vec::new();
    for row in doc.table("items") {
        let Some(serial_number) = text(row, "serialNumber").filter(|s| !s.is_empty()) else {
            continue;
        };
        let status = get_serial_number_status(doc, row, is_cancel, is_return);
        for name in get_serial_numbers(serial_number) {
            updates.push((name, status));
        }
    }
    for (name, status) in updates {
        let mut sn = fyo.get_doc("SerialNumber", Some(&name))?;
        sn.set_and_sync(fyo, "status", DocValue::Text(status.to_owned()))?;
    }
    Ok(())
}

/// `getSerialNumberStatus`.
fn get_serial_number_status(
    doc: &Doc,
    row: &Doc,
    is_cancel: bool,
    is_return: bool,
) -> &'static str {
    let pick = |cancelled: &'static str, submitted: &'static str| {
        if is_cancel { cancelled } else { submitted }
    };
    match doc.schema_name() {
        "Shipment" if is_return => pick("Delivered", "Active"),
        "Shipment" => pick("Active", "Delivered"),
        "PurchaseReceipt" if is_return => pick("Active", "Delivered"),
        "PurchaseReceipt" => pick("Inactive", "Active"),
        _ => match text(doc, "movementType") {
            Some("MaterialIssue") => pick("Active", "Delivered"),
            Some("MaterialReceipt") => pick("Inactive", "Active"),
            Some("MaterialTransfer") => "Active",
            _ if text(row, "fromLocation").is_some_and(|l| !l.is_empty()) => {
                pick("Active", "Delivered")
            }
            _ => pick("Inactive", "Active"),
        },
    }
}

/// A number as JavaScript writes it in a message.
pub(crate) fn js_number(n: f64) -> String {
    crate::fyo::model::helpers::js_number_string(n)
}
