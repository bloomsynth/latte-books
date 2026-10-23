// Port of models/inventory/Transfer.ts @ v0.36.0: the stock hooks of StockMovement,
// Shipment and PurchaseReceipt. Each model calls these from its own hooks, after the
// ledger ones (Transfer extends Transactional in Books).

use jiff::Timestamp;
use latte_db::DocValue;

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::models::inventory::helpers::create_serial_numbers;
use crate::models::inventory::stock_manager::{StockManager, TransferDetails};

/// `_getStockManager`.
pub(crate) fn stock_manager(doc: &Doc) -> StockManager {
    let date = match doc.get("date") {
        Some(DocValue::Date(date)) => *date,
        // Books makes an invalid date of a missing one, which no entry passes.
        _ => Timestamp::UNIX_EPOCH,
    };
    StockManager {
        date,
        reference_name: doc.name().unwrap_or_default().to_owned(),
        reference_type: doc.schema_name().to_owned(),
        is_cancelled: doc.is_cancelled(),
    }
}

/// `beforeSubmit`.
pub(crate) fn before_submit(
    doc: &Doc,
    fyo: &Fyo,
    transfers: &[TransferDetails],
) -> Result<(), Error> {
    stock_manager(doc).validate_transfers(fyo, transfers)
}

/// `afterSubmit`, after the ledger entries.
pub(crate) fn after_submit(
    doc: &Doc,
    fyo: &mut Fyo,
    transfers: &[TransferDetails],
) -> Result<(), Error> {
    create_serial_numbers(doc, fyo)?;
    stock_manager(doc).create_transfers(fyo, transfers)
}

/// `beforeCancel`.
pub(crate) fn before_cancel(
    doc: &Doc,
    fyo: &Fyo,
    transfers: &[TransferDetails],
) -> Result<(), Error> {
    let mut manager = stock_manager(doc);
    manager.is_cancelled = true;
    manager.validate_cancel(fyo, transfers)
}

/// `afterCancel`, after the ledger entries are reverted.
pub(crate) fn after_cancel(doc: &Doc, fyo: &mut Fyo) -> Result<(), Error> {
    stock_manager(doc).cancel_transfers(fyo)
}
