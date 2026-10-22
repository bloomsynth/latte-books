// Port of models/Transactional/Transactional.ts @ v0.36.0.
//
// A model that makes ledger entries implements `Transactional` and calls these functions
// from its `validate`, `after_submit`, `after_cancel` and `after_delete`, as Books' models
// get them by extending `Transactional`.

pub mod ledger_posting;

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::models::transactional::ledger_posting::{LedgerPosting, get_ledger_entry_names};

pub trait Transactional {
    /// `isTransactional`.
    fn is_transactional(&self, _doc: &Doc) -> bool {
        true
    }

    /// `getPosting`: the ledger entries the document makes, or `None` if it makes none.
    fn get_posting(&self, doc: &Doc, fyo: &mut Fyo) -> Result<Option<LedgerPosting>, Error>;
}

/// `validate`: the entries must balance.
pub fn validate(model: &dyn Transactional, doc: &Doc, fyo: &mut Fyo) -> Result<(), Error> {
    if !model.is_transactional(doc) {
        return Ok(());
    }

    match model.get_posting(doc, fyo)? {
        Some(posting) => posting.validate(fyo),
        None => Ok(()),
    }
}

/// `afterSubmit`: writes the ledger entries.
pub fn after_submit(model: &dyn Transactional, doc: &Doc, fyo: &mut Fyo) -> Result<(), Error> {
    if !model.is_transactional(doc) {
        return Ok(());
    }

    match model.get_posting(doc, fyo)? {
        Some(mut posting) => posting.post(fyo),
        None => Ok(()),
    }
}

/// `afterCancel`: reverts the ledger entries.
pub fn after_cancel(model: &dyn Transactional, doc: &Doc, fyo: &mut Fyo) -> Result<(), Error> {
    if !model.is_transactional(doc) {
        return Ok(());
    }

    match model.get_posting(doc, fyo)? {
        Some(mut posting) => posting.post_reverse(fyo),
        None => Ok(()),
    }
}

/// `afterDelete`: deletes the ledger entries and their reversals.
pub fn after_delete(model: &dyn Transactional, doc: &Doc, fyo: &mut Fyo) -> Result<(), Error> {
    if !model.is_transactional(doc) {
        return Ok(());
    }

    let name = doc.get("name").cloned().unwrap_or(latte_db::DocValue::Null);
    for name in get_ledger_entry_names(fyo, doc.schema_name(), &name, None)? {
        let mut entry = fyo.get_doc("AccountingLedgerEntry", Some(&name))?;
        entry.delete(fyo)?;
    }
    Ok(())
}
