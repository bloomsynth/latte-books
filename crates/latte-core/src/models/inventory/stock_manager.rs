// Port of models/inventory/StockManager.ts @ v0.36.0: validates a document's stock
// transfers and writes them to the stock ledger.

use jiff::Timestamp;
use latte_db::pesa::Money;
use latte_db::{DocValue, DocValueMap, Filter};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::format::Formatter;
use crate::fyo::model::helpers::is_truthy;
use crate::models::helpers::query;
use crate::models::inventory::helpers::{
    get_item_visibility, get_serial_numbers, get_stock_quantity, js_number,
};

/// `SMTransferDetails`: one row's transfer.
#[derive(Debug, Clone)]
pub struct TransferDetails {
    pub item: String,
    pub rate: Option<Money>,
    pub quantity: f64,
    pub batch: Option<String>,
    pub serial_number: Option<String>,
    pub from_location: Option<String>,
    pub to_location: Option<String>,
    pub is_return: bool,
}

/// `StockManager`: the transfers of one document.
pub struct StockManager {
    pub date: Timestamp,
    pub reference_name: String,
    pub reference_type: String,
    pub is_cancelled: bool,
}

fn non_empty(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|v| !v.is_empty())
}

impl StockManager {
    /// `validateTransfers`.
    pub fn validate_transfers(
        &self,
        fyo: &Fyo,
        transfers: &[TransferDetails],
    ) -> Result<(), Error> {
        for details in transfers {
            self.validate(fyo, details)?;
        }
        Ok(())
    }

    /// `createTransfers`: validates the transfers, then writes their stock ledger entries.
    pub fn create_transfers(
        &self,
        fyo: &mut Fyo,
        transfers: &[TransferDetails],
    ) -> Result<(), Error> {
        self.validate_transfers(fyo, transfers)?;

        let mut entries = Vec::new();
        for details in transfers {
            entries.push(self.transfer_stock(details));
        }
        // Each row's outward entries are written before its inward ones.
        for row in entries {
            let (outward, inward): (Vec<_>, Vec<_>) = row
                .into_iter()
                .partition(|e| matches!(e.get("quantity"), Some(DocValue::Number(q)) if *q <= 0.0));
            for data in outward.into_iter().chain(inward) {
                let mut entry = fyo.get_new_doc("StockLedgerEntry", data)?;
                entry.sync(fyo)?;
            }
        }
        Ok(())
    }

    /// `cancelTransfers`: deletes the document's stock ledger entries.
    pub fn cancel_transfers(&self, fyo: &mut Fyo) -> Result<(), Error> {
        let filters = query([
            ("referenceType", Filter::from(self.reference_type.as_str())),
            ("referenceName", Filter::from(self.reference_name.as_str())),
        ]);
        fyo.db.delete_all("StockLedgerEntry", &filters)?;
        Ok(())
    }

    /// `validateCancel`: the transfers reversed must be possible.
    pub fn validate_cancel(&self, fyo: &Fyo, transfers: &[TransferDetails]) -> Result<(), Error> {
        let reversed: Vec<TransferDetails> = transfers
            .iter()
            .map(|d| TransferDetails {
                item: d.item.clone(),
                rate: d.rate.clone(),
                quantity: d.quantity,
                // Books leaves the batch and serial numbers out of the reversal.
                batch: None,
                serial_number: None,
                from_location: d.to_location.clone(),
                to_location: d.from_location.clone(),
                is_return: d.is_return,
            })
            .collect();
        self.validate_transfers(fyo, &reversed)
    }

    fn validate(&self, fyo: &Fyo, details: &TransferDetails) -> Result<(), Error> {
        validate_rate(details)?;
        validate_quantity(fyo, details)?;
        validate_location(details)?;
        self.validate_stock_availability(fyo, details)
    }

    fn validate_stock_availability(
        &self,
        fyo: &Fyo,
        details: &TransferDetails,
    ) -> Result<(), Error> {
        let track_item = is_truthy(
            fyo.get_value("Item", Some(&details.item), "trackItem")
                .as_ref(),
        );
        let Some(from_location) = non_empty(&details.from_location) else {
            return Ok(());
        };
        if !track_item {
            return Ok(());
        }

        let formatted_date = Formatter::new(fyo)?.datetime(self.date);
        let batch = non_empty(&details.batch);
        let serial_numbers = get_serial_numbers(details.serial_number.as_deref().unwrap_or(""));

        let mut quantity_before = get_stock_quantity(
            fyo,
            &details.item,
            Some(from_location),
            None,
            Some(self.date),
            batch,
            &serial_numbers,
        )?
        .unwrap_or(0.0);
        if self.is_cancelled {
            quantity_before += details.quantity;
        }

        let batch_message = batch.map(|b| format!(" in Batch {b}")).unwrap_or_default();
        if !details.is_return && quantity_before < details.quantity {
            return Err(Error::Validation(format!(
                "Insufficient Quantity.\nAdditional quantity ({}) required{batch_message} to make \
                 outward transfer of item {} from {from_location} on {formatted_date}",
                js_number(details.quantity - quantity_before),
                details.item
            )));
        }

        let Some(quantity_after) = get_stock_quantity(
            fyo,
            &details.item,
            Some(from_location),
            Some(self.date),
            None,
            batch,
            &serial_numbers,
        )?
        else {
            // No future transactions.
            return Ok(());
        };

        let future_quantity = quantity_before - details.quantity + quantity_after;
        if future_quantity < 0.0 {
            return Err(Error::Validation(format!(
                "Insufficient Quantity.\nTransfer will cause future entries to have negative \
                 stock.\nAdditional quantity ({}) required{batch_message} to make outward \
                 transfer of item {} from {from_location} on {formatted_date}",
                js_number(-future_quantity),
                details.item
            )));
        }
        Ok(())
    }

    /// `StockManagerItem.transferStock`: the stock ledger entries of one row.
    fn transfer_stock(&self, details: &TransferDetails) -> Vec<DocValueMap> {
        let mut entries = Vec::new();
        if let Some(from) = non_empty(&details.from_location) {
            self.move_stock(details, from, true, &mut entries);
        }
        if let Some(to) = non_empty(&details.to_location) {
            self.move_stock(details, to, false, &mut entries);
        }
        entries
    }

    /// `#moveStockForSingleLocation`.
    fn move_stock(
        &self,
        details: &TransferDetails,
        location: &str,
        is_outward: bool,
        entries: &mut Vec<DocValueMap>,
    ) {
        let quantity = details.quantity;
        if quantity == 0.0 {
            return;
        }

        let serial_numbers = get_serial_numbers(details.serial_number.as_deref().unwrap_or(""));
        if !serial_numbers.is_empty() {
            // `#getSerialNumberedStockLedgerEntries`: one entry for each serial number, out
            // unless the quantity is negative, as on a return.
            let quantity = if quantity < 0.0 || !is_outward {
                1.0
            } else {
                -1.0
            };
            for serial_number in serial_numbers {
                entries.push(self.entry(details, location, quantity, Some(serial_number)));
            }
            return;
        }

        let quantity = if is_outward { -quantity } else { quantity };
        entries.push(self.entry(details, location, quantity, None));
    }

    /// `#getStockLedgerEntry`.
    fn entry(
        &self,
        details: &TransferDetails,
        location: &str,
        quantity: f64,
        serial_number: Option<String>,
    ) -> DocValueMap {
        let text_or_null = |value: Option<String>| match value.filter(|v| !v.is_empty()) {
            Some(value) => DocValue::Text(value),
            None => DocValue::Null,
        };
        DocValueMap::from([
            ("date".to_owned(), DocValue::Date(self.date)),
            ("item".to_owned(), DocValue::Text(details.item.clone())),
            (
                "rate".to_owned(),
                details.rate.clone().map_or(DocValue::Null, DocValue::Money),
            ),
            ("batch".to_owned(), text_or_null(details.batch.clone())),
            ("serialNumber".to_owned(), text_or_null(serial_number)),
            ("quantity".to_owned(), DocValue::Number(quantity)),
            ("location".to_owned(), DocValue::Text(location.to_owned())),
            (
                "referenceName".to_owned(),
                DocValue::Text(self.reference_name.clone()),
            ),
            (
                "referenceType".to_owned(),
                DocValue::Text(self.reference_type.clone()),
            ),
        ])
    }
}

/// `#validateRate`: a rate must be set, though it may be zero.
fn validate_rate(details: &TransferDetails) -> Result<(), Error> {
    let Some(rate) = details.rate.as_ref() else {
        return Err(Error::Validation("Rate needs to be set".into()));
    };
    if rate.lt(0.0)? {
        return Err(Error::Validation(format!(
            "Rate ({}) has to be greater than zero",
            js_number(rate.float())
        )));
    }
    Ok(())
}

/// `#validateQuantity`: only when POS shows inventory items alone.
fn validate_quantity(fyo: &Fyo, details: &TransferDetails) -> Result<(), Error> {
    if get_item_visibility(fyo).as_deref() != Some("Inventory Items") {
        return Ok(());
    }
    if details.quantity == 0.0 || details.quantity.is_nan() {
        return Err(Error::Validation("Quantity needs to be set".into()));
    }
    if !details.is_return && details.quantity <= 0.0 {
        return Err(Error::Validation(format!(
            "Quantity ({}) has to be greater than zero",
            js_number(details.quantity)
        )));
    }
    Ok(())
}

/// `#validateLocation`.
fn validate_location(details: &TransferDetails) -> Result<(), Error> {
    if non_empty(&details.from_location).is_some() || non_empty(&details.to_location).is_some() {
        return Ok(());
    }
    Err(Error::Validation(
        "Both From and To Location cannot be undefined".into(),
    ))
}
