// Port of models/ @ v0.36.0: the models of Books' own schemas.
//
// Only the models journal entries, invoices and payments need are ported so far. Schemas whose
// model isn't ported get the base `Doc`.

pub mod base_models;
pub mod helpers;
pub mod transactional;

use std::collections::HashMap;
use std::sync::Arc;

use crate::fyo::model::types::Model;
use crate::models::base_models::{
    account, accounting_ledger_entry, invoice_item, item, journal_entry, journal_entry_account,
    party, payment, payment_for, purchase_invoice, sales_invoice, tax_summary,
};

/// `models`, to pass to [`Fyo::register_models`](crate::Fyo::register_models).
pub fn models() -> HashMap<String, Arc<dyn Model>> {
    let models: [(&str, Arc<dyn Model>); 13] = [
        ("Account", Arc::new(account::Account)),
        (
            "AccountingLedgerEntry",
            Arc::new(accounting_ledger_entry::AccountingLedgerEntry),
        ),
        ("Item", Arc::new(item::Item)),
        ("JournalEntry", Arc::new(journal_entry::JournalEntry)),
        (
            "JournalEntryAccount",
            Arc::new(journal_entry_account::JournalEntryAccount),
        ),
        ("Party", Arc::new(party::Party)),
        ("Payment", Arc::new(payment::Payment)),
        ("PaymentFor", Arc::new(payment_for::PaymentFor)),
        ("PurchaseInvoice", Arc::new(purchase_invoice::MODEL)),
        ("PurchaseInvoiceItem", Arc::new(invoice_item::InvoiceItem)),
        ("SalesInvoice", Arc::new(sales_invoice::MODEL)),
        ("SalesInvoiceItem", Arc::new(invoice_item::InvoiceItem)),
        ("TaxSummary", Arc::new(tax_summary::TaxSummary)),
    ];
    models
        .into_iter()
        .map(|(name, model)| (name.to_owned(), model))
        .collect()
}

#[cfg(test)]
mod invoice_tests;
#[cfg(test)]
mod payment_tests;
#[cfg(test)]
mod tests;
