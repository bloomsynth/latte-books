// Port of models/ @ v0.36.0: the models of Books' own schemas.
//
// Only the models journal entries need are ported so far. Schemas whose model isn't
// ported get the base `Doc`.

pub mod base_models;
pub mod helpers;
pub mod transactional;

use std::collections::HashMap;
use std::sync::Arc;

use crate::fyo::model::types::Model;
use crate::models::base_models::{
    account, accounting_ledger_entry, journal_entry, journal_entry_account,
};

/// `models`, to pass to [`Fyo::register_models`](crate::Fyo::register_models).
pub fn models() -> HashMap<String, Arc<dyn Model>> {
    let models: [(&str, Arc<dyn Model>); 4] = [
        ("Account", Arc::new(account::Account)),
        (
            "AccountingLedgerEntry",
            Arc::new(accounting_ledger_entry::AccountingLedgerEntry),
        ),
        ("JournalEntry", Arc::new(journal_entry::JournalEntry)),
        (
            "JournalEntryAccount",
            Arc::new(journal_entry_account::JournalEntryAccount),
        ),
    ];
    models
        .into_iter()
        .map(|(name, model)| (name.to_owned(), model))
        .collect()
}

#[cfg(test)]
mod tests;
