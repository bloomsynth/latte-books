// Port of schemas/schemas.ts and schemas/regional/*/index.ts @ v0.36.0.
//
// The schema files are embedded in the binary, as Books bundles them. The lists keep Books'
// order, which is the order of the final schema map.

macro_rules! schemas {
    ($($path:literal),* $(,)?) => {
        &[$(($path, include_str!(concat!("../../../vendor/books/schemas/", $path)))),*]
    };
}

/// A schema file: its path under `vendor/books/schemas`, and its text.
pub(crate) type Source = (&'static str, &'static str);

pub(crate) const CORE: &[Source] = schemas![
    "core/PatchRun.json",
    "core/SingleValue.json",
    "core/SystemSettings.json",
];

pub(crate) const META: &[Source] = schemas![
    "meta/base.json",
    "meta/child.json",
    "meta/submittable.json",
    "meta/tree.json",
];

pub(crate) const APP: &[Source] = schemas![
    "app/Misc.json",
    "app/SetupWizard.json",
    "app/GetStarted.json",
    "app/PrintTemplate.json",
    "app/Color.json",
    "app/Currency.json",
    "app/Defaults.json",
    "app/NumberSeries.json",
    "app/PrintSettings.json",
    "app/Account.json",
    "app/AccountingSettings.json",
    "app/AccountingLedgerEntry.json",
    "app/Party.json",
    "app/Lead.json",
    "app/Address.json",
    "app/ItemGroup.json",
    "app/Item.json",
    "app/UOM.json",
    "app/inventory/UOMConversionItem.json",
    "app/LoyaltyProgram.json",
    "app/LoyaltyPointEntry.json",
    "app/CollectionRulesItems.json",
    "app/Payment.json",
    "app/PaymentMethod.json",
    "app/PaymentFor.json",
    "app/JournalEntry.json",
    "app/JournalEntryAccount.json",
    "app/Invoice.json",
    "app/ItemEnquiry.json",
    "app/SalesInvoice.json",
    "app/PurchaseInvoice.json",
    "app/SalesQuote.json",
    "app/InvoiceItem.json",
    "app/SalesInvoiceItem.json",
    "app/PurchaseInvoiceItem.json",
    "app/SalesQuoteItem.json",
    "app/CouponCode.json",
    "app/AppliedCouponCodes.json",
    "app/PriceList.json",
    "app/PriceListItem.json",
    "app/PricingRule.json",
    "app/PricingRuleItem.json",
    "app/PricingRuleDetail.json",
    "app/Tax.json",
    "app/TaxDetail.json",
    "app/TaxSummary.json",
    "app/inventory/InventorySettings.json",
    "app/inventory/Location.json",
    "app/inventory/StockLedgerEntry.json",
    "app/inventory/StockMovement.json",
    "app/inventory/StockMovementItem.json",
    "app/inventory/StockTransfer.json",
    "app/inventory/StockTransferItem.json",
    "app/inventory/Shipment.json",
    "app/inventory/ShipmentItem.json",
    "app/inventory/PurchaseReceipt.json",
    "app/inventory/PurchaseReceiptItem.json",
    "app/Batch.json",
    "app/inventory/SerialNumber.json",
    "core/CustomForm.json",
    "core/CustomField.json",
    "app/inventory/Point of Sale/CashDenominations.json",
    "app/inventory/Point of Sale/ClosingAmounts.json",
    "app/inventory/Point of Sale/ClosingCash.json",
    "app/inventory/Point of Sale/DefaultCashDenominations.json",
    "app/inventory/Point of Sale/OpeningAmounts.json",
    "app/inventory/Point of Sale/OpeningCash.json",
    "app/inventory/Point of Sale/POSSettings.json",
    "app/POSProfile.json",
    "app/inventory/Point of Sale/POSOpeningShift.json",
    "app/inventory/Point of Sale/POSClosingShift.json",
    "app/inventory/Point of Sale/POSShiftAmounts.json",
    "app/ERPNextSyncSettings.json",
    "app/ERPNextSyncQueue.json",
    "app/FetchFromERPNextQueue.json",
    "app/IntegrationErrorLog.json",
];

const REGIONAL_IN: &[Source] = schemas![
    "regional/in/AccountingSettings.json",
    "regional/in/Address.json",
    "regional/in/Party.json",
];

const REGIONAL_CH: &[Source] = schemas!["regional/ch/AccountingSettings.json"];

/// Regional schemas by country code.
pub(crate) fn regional(country_code: &str) -> &'static [Source] {
    match country_code {
        "in" => REGIONAL_IN,
        "ch" => REGIONAL_CH,
        _ => &[],
    }
}
