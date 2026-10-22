// Port of models/baseModels/AccountingLedgerEntry/AccountingLedgerEntry.ts @ v0.36.0.

use jiff::Timestamp;
use latte_db::{DocValue, DocValueMap};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::helpers::is_truthy;
use crate::fyo::model::types::Model;

pub struct AccountingLedgerEntry;

impl Model for AccountingLedgerEntry {}

/// `revert`: marks the entry reverted and adds one that swaps its debit and credit.
pub fn revert(doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
    if is_truthy(doc.get("reverted")) {
        return Ok(());
    }

    doc.set(fyo, "reverted", DocValue::Bool(true))?;
    let value = |fieldname| doc.get(fieldname).cloned().unwrap_or(DocValue::Null);
    let data = DocValueMap::from([
        ("account".to_owned(), value("account")),
        ("party".to_owned(), value("party")),
        ("date".to_owned(), DocValue::Date(Timestamp::now())),
        ("referenceType".to_owned(), value("referenceType")),
        ("referenceName".to_owned(), value("referenceName")),
        ("debit".to_owned(), value("credit")),
        ("credit".to_owned(), value("debit")),
        ("reverted".to_owned(), DocValue::Bool(true)),
        ("reverts".to_owned(), value("name")),
    ]);
    let mut reverted_entry = fyo.get_new_doc("AccountingLedgerEntry", data)?;

    doc.sync(fyo)?;
    reverted_entry.sync(fyo)
}
