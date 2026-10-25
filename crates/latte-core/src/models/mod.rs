// Port of models/ @ v0.36.0: the models of Books' own schemas.
//
// The models of accounting, inventory, the point of sale, pricing rules, coupons, loyalty
// points, leads and quotes are ported so far. Schemas whose
// model isn't ported get the base `Doc`.

pub mod base_models;
pub mod helpers;
pub mod inventory;
pub mod transactional;

use std::collections::HashMap;
use std::sync::Arc;

use crate::fyo::model::types::Model;
use crate::models::base_models::{
    account, accounting_ledger_entry, address, coupon_code, invoice_item, item, journal_entry,
    journal_entry_account, lead, loyalty, party, payment, payment_for, pricing_rule,
    print_template, purchase_invoice, sales_invoice, sales_quote, tax_summary,
};
use crate::models::inventory::{
    inventory_settings, point_of_sale, stock_movement, stock_movement_item, stock_transfer,
    stock_transfer_item,
};

/// `getRegionalModels`: the models of a country's own schemas, to pass to
/// [`Fyo::register_models`](crate::Fyo::register_models). Only India has some.
pub fn regional_models(country_code: &str) -> HashMap<String, Arc<dyn Model>> {
    if country_code != "in" {
        return HashMap::new();
    }
    let models: [(&str, Arc<dyn Model>); 2] = [
        ("Address", Arc::new(address::IndianAddress)),
        ("Party", Arc::new(party::IndianParty)),
    ];
    models
        .into_iter()
        .map(|(name, model)| (name.to_owned(), model))
        .collect()
}

/// `models`, to pass to [`Fyo::register_models`](crate::Fyo::register_models).
pub fn models() -> HashMap<String, Arc<dyn Model>> {
    let models: [(&str, Arc<dyn Model>); 33] = [
        ("Account", Arc::new(account::Account)),
        ("Address", Arc::new(address::Address)),
        (
            "AppliedCouponCodes",
            Arc::new(coupon_code::AppliedCouponCodes),
        ),
        ("CouponCode", Arc::new(coupon_code::CouponCode)),
        (
            "AccountingLedgerEntry",
            Arc::new(accounting_ledger_entry::AccountingLedgerEntry),
        ),
        (
            "InventorySettings",
            Arc::new(inventory_settings::InventorySettings),
        ),
        ("ClosingAmounts", Arc::new(point_of_sale::ClosingAmounts)),
        ("Item", Arc::new(item::Item)),
        ("Lead", Arc::new(lead::Lead)),
        ("LoyaltyProgram", Arc::new(loyalty::LoyaltyProgram)),
        ("JournalEntry", Arc::new(journal_entry::JournalEntry)),
        (
            "JournalEntryAccount",
            Arc::new(journal_entry_account::JournalEntryAccount),
        ),
        ("Party", Arc::new(party::Party)),
        ("Payment", Arc::new(payment::Payment)),
        ("PaymentFor", Arc::new(payment_for::PaymentFor)),
        ("POSSettings", Arc::new(point_of_sale::PosSettings)),
        ("PriceListItem", Arc::new(pricing_rule::PricingRuleItem)),
        ("PricingRule", Arc::new(pricing_rule::PricingRule)),
        ("PricingRuleItem", Arc::new(pricing_rule::PricingRuleItem)),
        ("PrintTemplate", Arc::new(print_template::PrintTemplate)),
        ("PurchaseInvoice", Arc::new(purchase_invoice::MODEL)),
        ("PurchaseInvoiceItem", Arc::new(invoice_item::InvoiceItem)),
        ("PurchaseReceipt", Arc::new(stock_transfer::StockTransfer)),
        (
            "PurchaseReceiptItem",
            Arc::new(stock_transfer_item::StockTransferItem),
        ),
        ("SalesInvoice", Arc::new(sales_invoice::MODEL)),
        ("SalesInvoiceItem", Arc::new(invoice_item::InvoiceItem)),
        ("SalesQuote", Arc::new(sales_quote::MODEL)),
        ("SalesQuoteItem", Arc::new(invoice_item::InvoiceItem)),
        ("Shipment", Arc::new(stock_transfer::StockTransfer)),
        (
            "ShipmentItem",
            Arc::new(stock_transfer_item::StockTransferItem),
        ),
        ("StockMovement", Arc::new(stock_movement::StockMovement)),
        (
            "StockMovementItem",
            Arc::new(stock_movement_item::StockMovementItem),
        ),
        ("TaxSummary", Arc::new(tax_summary::TaxSummary)),
    ];
    models
        .into_iter()
        .map(|(name, model)| (name.to_owned(), model))
        .collect()
}

#[cfg(test)]
mod inventory_tests;
#[cfg(test)]
mod invoice_tests;
#[cfg(test)]
mod payment_tests;
#[cfg(test)]
mod pricing_tests;
#[cfg(test)]
mod regional_tests;
#[cfg(test)]
mod report_tests;
#[cfg(test)]
mod return_tests;
#[cfg(test)]
mod tests;
