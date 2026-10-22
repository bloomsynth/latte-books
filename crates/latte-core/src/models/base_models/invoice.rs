// Port of models/baseModels/Invoice/Invoice.ts @ v0.36.0, the model SalesInvoice and
// PurchaseInvoice share. Each gives its own `getPosting` (sales_invoice.rs and
// purchase_invoice.rs).
//
// Deviations:
// - Exchange rates aren't fetched from api.vatcomply.com. An invoice in a foreign
//   currency gets the rate 1, as Books does where it can't fetch, until the rate is set.
// - What isn't ported yet is refused rather than skipped, so an invoice Latte saves is
//   never one Books would have saved differently:
//   - saving a sales invoice with a row an enabled pricing rule is for (with pricing
//     rules on), with coupons, or redeeming loyalty points. Books would apply a rule only
//     where its quantities, amounts, dates and coupons fit, but Latte refuses on any;
//   - submitting a return, an invoice that earns loyalty points, or one set to make its
//     payment or its shipment or receipt on submit;
//   - cancelling a return, or an invoice with payments, coupons or loyalty points;
//   - deleting an invoice with payments or cancelled shipments or receipts.
//
// Not ported yet: returns (`getReturnDoc` and what submitting one updates), payments
// (`getPayment`, `getLinkedPayments`), stock transfers (`getStockTransfer`,
// `getLinkedStockTransfers`), pricing rules, coupons and loyalty points, `addItem`, and, as
// the UI needs them, `filters` and `createFilters`.

use indexmap::IndexMap;
use jiff::Timestamp;
use latte_db::pesa::Money;
use latte_db::{DocValue, DocValueMap, Filter, FilterValue, GetAllOptions, QueryFilter};
use latte_schema::FieldType;

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::helpers::{is_truthy, js_number_string};
use crate::fyo::model::types::{DocContext, Formula, FormulaValue, Model};
use crate::models::base_models::party;
use crate::models::helpers::{
    DEFAULT_CURRENCY, company_currency, get_number_series, linked_value, money, money_or_zero,
    number, single_is_truthy, single_text, single_value, text,
};
use crate::models::transactional::ledger_posting::LedgerPosting;
use crate::models::transactional::{self, Transactional};

type GetPosting = fn(&Doc, &mut Fyo) -> Result<LedgerPosting, Error>;
type ValidateField = fn(&str, &DocValue, &DocContext<'_>) -> Result<(), Error>;

/// SalesInvoice's or PurchaseInvoice's model, which differ in their posting and
/// `validations`.
pub struct Invoice {
    get_posting: GetPosting,
    validate_field: ValidateField,
}

impl Invoice {
    pub(crate) const fn new(get_posting: GetPosting, validate_field: ValidateField) -> Self {
        Self {
            get_posting,
            validate_field,
        }
    }
}

type FormulaResult = Result<Option<FormulaValue>, Error>;

fn some(value: DocValue) -> FormulaResult {
    Ok(Some(value.into()))
}

fn not_ported(what: &str) -> Error {
    Error::Validation(format!("{what} isn't supported in Latte yet."))
}

/// `isSales`.
pub(crate) fn is_sales(doc: &Doc) -> bool {
    matches!(doc.schema_name(), "SalesInvoice" | "SalesQuote")
}

/// `isQuote`.
fn is_quote(doc: &Doc) -> bool {
    doc.schema_name() == "SalesQuote"
}

/// `isReturn`.
pub(crate) fn is_return(doc: &Doc) -> bool {
    is_truthy(doc.get("returnAgainst"))
}

/// `enableDiscounting`.
fn enable_discounting(fyo: &Fyo) -> bool {
    single_is_truthy(fyo, "AccountingSettings", "enableDiscounting")
}

/// `exchangeRate ?? 1`.
pub(crate) fn exchange_rate(doc: &Doc) -> f64 {
    number(doc, "exchangeRate").unwrap_or(1.0)
}

/// `autoPaymentAccount`: the account set in Defaults to pay invoices into or from.
fn auto_payment_account(doc: &Doc, fyo: &Fyo) -> Option<String> {
    let fieldname = if is_sales(doc) {
        "salesPaymentAccount"
    } else {
        "purchasePaymentAccount"
    };
    single_text(fyo, "Defaults", fieldname)
}

/// `autoStockTransferLocation`.
fn auto_stock_transfer_location(doc: &Doc, fyo: &Fyo) -> Option<String> {
    let fieldname = if is_sales(doc) {
        "shipmentLocation"
    } else {
        "purchaseReceiptLocation"
    };
    single_text(fyo, "Defaults", fieldname)
}

/// `stockTransferSchemaName`.
fn stock_transfer_schema_name(doc: &Doc) -> &'static str {
    if is_sales(doc) {
        "Shipment"
    } else {
        "PurchaseReceipt"
    }
}

fn required_money(doc: &Doc, fieldname: &str) -> Result<Money, Error> {
    money(doc, fieldname).ok_or_else(|| Error::Value(format!("{fieldname} is not money")))
}

/// One tax on one row: `InvoiceTaxItem`, less what isn't used.
struct TaxItem {
    account: String,
    rate: f64,
    tax_amount: Money,
}

/// `getTaxItems`: each row's taxes, on its amount less its discount. With the discount
/// applied after tax there are none, as in Books.
fn get_tax_items(doc: &Doc, fyo: &Fyo) -> Result<Vec<TaxItem>, Error> {
    let mut tax_items = Vec::new();
    for item in doc.table("items") {
        let Some(tax) = text(item, "tax").filter(|t| !t.is_empty()) else {
            continue;
        };
        let tax = fyo.db.get_doc("Tax", tax)?;
        let details = match tax.get("details") {
            Some(DocValue::Table(details)) => details.as_slice(),
            _ => &[],
        };

        for details in details {
            let mut amount = required_money(item, "amount")?;
            if is_return(doc) && amount.is_positive() {
                amount = amount.neg()?;
            }

            if is_truthy(doc.get("discountAfterTax")) {
                continue;
            }
            let mut item_discount_amount = get_discount_amount(doc, fyo, item)?;
            if is_return(doc) && item_discount_amount.is_negative() {
                item_discount_amount = item_discount_amount.abs()?;
            }
            amount = if is_return(doc) {
                amount.add(&item_discount_amount)?
            } else {
                amount.sub(&item_discount_amount)?
            };

            let rate = match details.get("rate") {
                Some(DocValue::Number(rate)) => *rate,
                _ => f64::NAN,
            };
            let account = match details.get("account") {
                Some(DocValue::Text(account)) => account.clone(),
                _ => String::new(),
            };
            tax_items.push(TaxItem {
                account,
                rate,
                tax_amount: amount.mul(rate / 100.0)?,
            });
        }
    }
    Ok(tax_items)
}

/// `getTaxSummary`: the taxes by account, as TaxSummary rows, less those that come to
/// zero.
pub(crate) fn get_tax_summary(doc: &Doc, fyo: &Fyo) -> Result<Vec<DocValueMap>, Error> {
    let mut taxes: IndexMap<String, (f64, Money)> = IndexMap::new();
    for TaxItem {
        account,
        rate,
        tax_amount,
    } in get_tax_items(doc, fyo)?
    {
        let entry = taxes
            .entry(account)
            .or_insert_with(|| (rate, fyo.pesa().zero()));
        entry.1 = entry.1.add(&tax_amount)?;
    }

    let mut tax_arr = Vec::new();
    for (account, (rate, amount)) in taxes {
        if amount.is_zero() {
            continue;
        }
        let idx = tax_arr.len() as f64;
        tax_arr.push(DocValueMap::from([
            ("account".to_owned(), DocValue::Text(account)),
            ("rate".to_owned(), DocValue::Number(rate)),
            ("amount".to_owned(), DocValue::Money(amount)),
            ("idx".to_owned(), DocValue::Number(idx)),
        ]));
    }
    Ok(tax_arr)
}

/// `getTotalDiscount`: the rows' discounts and the invoice's.
pub(crate) fn get_total_discount(doc: &Doc, fyo: &Fyo) -> Result<Money, Error> {
    if !enable_discounting(fyo) {
        return Ok(fyo.pesa().zero());
    }

    let item_discount_amount = get_item_discount_amount(doc, fyo)?;
    let invoice_discount_amount = get_invoice_discount_amount(doc, fyo)?;
    let total = item_discount_amount.add(&invoice_discount_amount)?;
    if is_return(doc) && total.is_positive() {
        return Ok(total.neg()?);
    }
    Ok(total)
}

/// `getGrandTotal`: the net total and the taxes, less the discounts.
fn get_grand_total(doc: &Doc, fyo: &Fyo) -> Result<Money, Error> {
    let total_discount = get_total_discount(doc, fyo)?;
    let net_total = required_money(doc, "netTotal")?;
    let taxes = doc.table("taxes");
    if is_truthy(doc.get("redeemLoyaltyPoints")) {
        return Err(not_ported("Redeeming loyalty points"));
    }
    if taxes.is_empty() {
        return Ok(net_total.sub(&total_discount)?);
    }

    let mut grand_total = net_total.abs()?;
    for tax in taxes {
        let amount = required_money(tax, "amount")?;
        grand_total = if is_return(doc) {
            grand_total.abs()?.add(&amount.abs()?)?.neg()?
        } else {
            grand_total.add(&amount.abs()?)?
        };
    }
    Ok(grand_total.sub(&total_discount)?)
}

/// `getInvoiceDiscountAmount`. The fields for it are always hidden in Books.
fn get_invoice_discount_amount(doc: &Doc, fyo: &Fyo) -> Result<Money, Error> {
    if !enable_discounting(fyo) {
        return Ok(fyo.pesa().zero());
    }
    if is_truthy(doc.get("setDiscountAmount")) {
        return Ok(money_or_zero(doc, "discountAmount", fyo));
    }

    let total_fieldname = if is_truthy(doc.get("discountAfterTax")) {
        "itemTaxedTotal"
    } else {
        "itemDiscountedTotal"
    };
    let mut total_item_amounts = fyo.pesa().zero();
    for item in doc.table("items") {
        total_item_amounts = total_item_amounts.add(&required_money(item, total_fieldname)?)?;
    }
    Ok(total_item_amounts.percent(number(doc, "discountPercent").unwrap_or(0.0))?)
}

/// What a row's discount comes to, per `getDiscountAmount` (`return_negates` true, for
/// one row) or `getItemDiscountAmount` (false, for each row), which differ in the sign
/// they give a return's discount set as a percent before tax.
fn row_discount_amount(
    doc: &Doc,
    fyo: &Fyo,
    item: &Doc,
    return_negates: bool,
) -> Result<Money, Error> {
    let quantity = number(item, "quantity").unwrap_or(0.0);
    let percent = number(item, "itemDiscountPercent").unwrap_or(0.0);
    let return_percent = |negate: bool| {
        let percent = percent.abs() / 100.0;
        if negate { -percent } else { percent }
    };

    if is_truthy(item.get("setItemDiscountAmount")) {
        return Ok(money_or_zero(item, "itemDiscountAmount", fyo).mul(quantity)?);
    }
    let (fieldname, rate) = if !is_truthy(doc.get("discountAfterTax")) {
        let rate = if is_return(doc) {
            return_percent(return_negates)
        } else {
            percent / 100.0
        };
        ("amount", rate)
    } else {
        let rate = if is_return(doc) {
            return_percent(true)
        } else {
            percent / 100.0
        };
        ("itemTaxedTotal", rate)
    };
    Ok(money_or_zero(item, fieldname, fyo).mul(rate)?)
}

/// `getDiscountAmount`: one row's discount.
fn get_discount_amount(doc: &Doc, fyo: &Fyo, item: &Doc) -> Result<Money, Error> {
    if !enable_discounting(fyo) || doc.table("items").is_empty() {
        return Ok(fyo.pesa().zero());
    }

    let discount_amount = row_discount_amount(doc, fyo, item, true)?;
    if is_return(doc) {
        return Ok(discount_amount.neg()?);
    }
    Ok(discount_amount)
}

/// `getItemDiscountAmount`: the rows' discounts.
fn get_item_discount_amount(doc: &Doc, fyo: &Fyo) -> Result<Money, Error> {
    if !enable_discounting(fyo) || doc.table("items").is_empty() {
        return Ok(fyo.pesa().zero());
    }

    let mut discount_amount = fyo.pesa().zero();
    for item in doc.table("items") {
        discount_amount = discount_amount.add(&row_discount_amount(doc, fyo, item, false)?)?;
    }
    if is_return(doc) {
        return Ok(discount_amount.neg()?);
    }
    Ok(discount_amount)
}

/// `getExchangeRate`, without fetching: 1 for the company's currency, and 1 for others
/// too, which Books fetches.
fn get_exchange_rate(doc: &Doc, fyo: &Fyo) -> f64 {
    let _ = (doc, fyo);
    1.0
}

/// `getStockNotTransferred`.
fn get_stock_not_transferred(doc: &Doc) -> f64 {
    doc.table("items")
        .iter()
        .map(|item| number(item, "stockNotTransferred").unwrap_or(0.0))
        .sum()
}

/// `ignorePricingRules`: whether the POS settings or their profile say to.
fn ignore_pricing_rules(fyo: &Fyo) -> bool {
    if let Some(profile) = single_text(fyo, "POSSettings", "posProfile")
        && let Ok(profile) = fyo.db.get_doc("POSProfile", &profile)
    {
        return is_truthy(profile.get("ignorePricingRule"));
    }
    single_is_truthy(fyo, "POSSettings", "ignorePricingRule")
}

/// `getPricingRule` as far as Latte goes: `None` where Books returns nothing, and
/// `Some(())` where it returns the rules that apply, as long as there are none. Where an
/// enabled pricing rule is for one of the rows, it's refused, as applying them isn't
/// ported.
pub(crate) fn get_pricing_rule(doc: &Doc, fyo: &Fyo) -> Result<Option<()>, Error> {
    if ignore_pricing_rules(fyo) || !is_sales(doc) {
        return Ok(None);
    }

    for item in doc.table("items") {
        if is_truthy(item.get("isFreeItem")) {
            continue;
        }
        let options = GetAllOptions {
            fields: Some(vec!["parent".to_owned()]),
            filters: QueryFilter::from([
                (
                    "item".to_owned(),
                    Filter::from(text(item, "item").unwrap_or_default()),
                ),
                (
                    "unit".to_owned(),
                    Filter::from(text(item, "unit").unwrap_or_default()),
                ),
            ]),
            ..GetAllOptions::default()
        };
        let names: Vec<FilterValue> = fyo
            .db
            .get_all_docs("PricingRuleItem", &options)?
            .into_iter()
            .filter_map(|row| match row.get("parent") {
                Some(DocValue::Text(parent)) => Some(FilterValue::from(parent.as_str())),
                _ => None,
            })
            .collect();
        if names.is_empty() {
            continue;
        }

        let options = GetAllOptions {
            filters: QueryFilter::from([
                ("name".to_owned(), Filter::list("in", names)),
                ("isEnabled".to_owned(), Filter::from(true)),
            ]),
            ..GetAllOptions::default()
        };
        if !fyo.db.get_all_docs("PricingRule", &options)?.is_empty() {
            return Err(not_ported("Pricing rules"));
        }
    }
    Ok(Some(()))
}

/// The values of the invoice a return is against.
fn return_against(doc: &Doc, fyo: &Fyo) -> Result<DocValueMap, Error> {
    let name = text(doc, "returnAgainst").unwrap_or_default();
    Ok(fyo.db.get_doc(doc.schema_name(), name)?)
}

fn formulas() -> Vec<Formula> {
    vec![
        Formula {
            fieldname: "account",
            depends_on: Some(&["party"]),
            formula: |ctx, _| {
                let party = text(ctx.doc, "party");
                Ok(linked_value(ctx.fyo, "Party", party, "defaultAccount").map(Into::into))
            },
        },
        Formula {
            fieldname: "loyaltyProgram",
            depends_on: Some(&["party", "name"]),
            formula: |ctx, _| {
                let party = text(ctx.doc, "party").unwrap_or_default();
                let party = ctx.fyo.db.get_doc("Party", party)?;
                some(
                    party
                        .get("loyaltyProgram")
                        .cloned()
                        .unwrap_or(DocValue::Null),
                )
            },
        },
        Formula {
            fieldname: "currency",
            depends_on: Some(&["party"]),
            formula: |ctx, _| {
                let party = text(ctx.doc, "party");
                match linked_value(ctx.fyo, "Party", party, "currency") {
                    Some(DocValue::Null) | None => {
                        Ok(single_value(ctx.fyo, "SystemSettings", "currency").map(Into::into))
                    }
                    Some(currency) => some(currency),
                }
            },
        },
        Formula {
            fieldname: "exchangeRate",
            depends_on: Some(&["party", "currency"]),
            formula: |ctx, _| {
                let doc = ctx.doc;
                if text(doc, "currency") == Some(company_currency(ctx.fyo).as_str()) {
                    return some(DocValue::Number(1.0));
                }
                if let Some(rate) = number(doc, "exchangeRate").filter(|r| *r != 0.0 && *r != 1.0)
                    && !rate.is_nan()
                {
                    return some(DocValue::Number(rate));
                }
                some(DocValue::Number(get_exchange_rate(doc, ctx.fyo)))
            },
        },
        Formula {
            fieldname: "netTotal",
            depends_on: None,
            formula: |ctx, _| {
                some(DocValue::Money(
                    ctx.doc.get_sum(ctx.fyo, "items", "amount")?,
                ))
            },
        },
        Formula {
            fieldname: "taxes",
            depends_on: None,
            formula: |ctx, _| Ok(Some(get_tax_summary(ctx.doc, ctx.fyo)?.into())),
        },
        Formula {
            fieldname: "grandTotal",
            depends_on: Some(&["loyaltyPoints"]),
            formula: |ctx, _| some(DocValue::Money(get_grand_total(ctx.doc, ctx.fyo)?)),
        },
        Formula {
            fieldname: "baseGrandTotal",
            depends_on: Some(&["grandTotal", "exchangeRate"]),
            formula: |ctx, _| {
                let grand_total = required_money(ctx.doc, "grandTotal")?;
                some(DocValue::Money(grand_total.mul(exchange_rate(ctx.doc))?))
            },
        },
        Formula {
            fieldname: "outstandingAmount",
            depends_on: Some(&["discountAmount", "discountPercent"]),
            formula: outstanding_amount_formula,
        },
        Formula {
            fieldname: "stockNotTransferred",
            depends_on: Some(&["items"]),
            formula: |ctx, _| {
                let doc = ctx.doc;
                if is_truthy(doc.get("submitted")) {
                    return Ok(None);
                }
                if is_return(doc) {
                    let returned = return_against(doc, ctx.fyo)?;
                    if returned.get("stockNotTransferred") == Some(&DocValue::Number(0.0)) {
                        return some(DocValue::Number(get_stock_not_transferred(doc)));
                    }
                    return some(DocValue::Number(0.0));
                }
                some(DocValue::Number(get_stock_not_transferred(doc)))
            },
        },
        Formula {
            fieldname: "makeAutoPayment",
            depends_on: Some(&[]),
            formula: |ctx, _| {
                let account = auto_payment_account(ctx.doc, ctx.fyo);
                some(DocValue::Bool(account.is_some()))
            },
        },
        Formula {
            fieldname: "makeAutoStockTransfer",
            depends_on: Some(&[]),
            formula: |ctx, _| {
                let fyo = ctx.fyo;
                some(DocValue::Bool(
                    single_is_truthy(fyo, "AccountingSettings", "enableInventory")
                        && auto_stock_transfer_location(ctx.doc, fyo).is_some(),
                ))
            },
        },
        Formula {
            fieldname: "isPricingRuleApplied",
            depends_on: Some(&["items", "coupons"]),
            formula: |ctx, _| {
                if !single_is_truthy(ctx.fyo, "AccountingSettings", "enablePricingRule") {
                    return some(DocValue::Bool(false));
                }
                // With no rules that apply, Books empties `pricingRuleDetail`, which is
                // empty already.
                let applied = get_pricing_rule(ctx.doc, ctx.fyo)?.is_some();
                some(DocValue::Bool(applied))
            },
        },
    ]
}

/// The `outstandingAmount` formula: the base grand total until the invoice is submitted;
/// for a return, what can be paid back of what was paid.
fn outstanding_amount_formula(ctx: &DocContext<'_>, _: Option<&str>) -> FormulaResult {
    let doc = ctx.doc;
    if is_truthy(doc.get("submitted")) {
        return Ok(None);
    }

    if is_return(doc) {
        let returned = return_against(doc, ctx.fyo)?;
        let returned_money = |fieldname: &str| match returned.get(fieldname) {
            Some(DocValue::Money(money)) => Ok(money.clone()),
            _ => Err(Error::Value(format!("{fieldname} is not money"))),
        };
        let grand_total = required_money(doc, "grandTotal")?;
        if returned_money("outstandingAmount")?.is_zero() {
            return some(DocValue::Money(grand_total.abs()?));
        }

        let total_paid = returned_money("grandTotal")?
            .abs()?
            .sub(&returned_money("outstandingAmount")?)?;
        let outstanding_amount = grand_total.abs()?;
        if outstanding_amount.lte(&total_paid)? {
            return some(DocValue::Money(outstanding_amount));
        }
        return some(DocValue::Money(total_paid));
    }

    Ok(doc.get("baseGrandTotal").cloned().map(Into::into))
}

/// `validateBatch` in models/inventory/helpers.ts: rows of batched items need a batch,
/// and others mustn't have one.
fn validate_batch(doc: &Doc, fyo: &Fyo) -> Result<(), Error> {
    if is_quote(doc) {
        return Ok(());
    }

    for row in doc.table("items") {
        let row_number = js_number_string(number(row, "idx").unwrap_or(0.0) + 1.0);
        let Some(item) = text(row, "item").filter(|i| !i.is_empty()) else {
            continue;
        };
        let has_batch = is_truthy(fyo.get_value("Item", Some(item), "hasBatch").as_ref());
        let batch = is_truthy(row.get("batch"));
        if !has_batch && batch {
            return Err(Error::Validation(format!(
                "Batch set for row {row_number}. Item {item} is not a batched item"
            )));
        }
        if has_batch && !batch {
            return Err(Error::Validation(format!(
                "Batch not set for row {row_number}. Item {item} is a batched item"
            )));
        }
    }
    Ok(())
}

/// `getPaymentIds`: the payments made against the invoice.
fn get_payment_ids(doc: &Doc, fyo: &Fyo) -> Result<Vec<String>, Error> {
    let options = GetAllOptions {
        fields: Some(vec!["parent".to_owned()]),
        filters: QueryFilter::from([
            ("referenceType".to_owned(), Filter::from(doc.schema_name())),
            (
                "referenceName".to_owned(),
                Filter::from(doc.name().unwrap_or_default()),
            ),
        ]),
        order_by: Some(vec!["name".to_owned()]),
        ..GetAllOptions::default()
    };
    let mut ids: Vec<String> = Vec::new();
    for row in fyo.db.get_all_docs("PaymentFor", &options)? {
        if let Some(DocValue::Text(parent)) = row.get("parent")
            && !ids.contains(parent)
        {
            ids.push(parent.clone());
        }
    }
    Ok(ids)
}

/// `_getLinkedStockTransferNames`: the invoice's shipments or receipts, cancelled or not.
fn get_linked_stock_transfer_names(
    doc: &Doc,
    fyo: &Fyo,
    cancelled: bool,
) -> Result<Vec<String>, Error> {
    let Some(name) = doc.name() else {
        return Err(Error::Validation(format!(
            "Name not found for {}",
            doc.schema().label
        )));
    };
    let options = GetAllOptions {
        fields: Some(vec!["name".to_owned()]),
        filters: QueryFilter::from([
            ("backReference".to_owned(), Filter::from(name)),
            ("cancelled".to_owned(), Filter::from(cancelled)),
        ]),
        ..GetAllOptions::default()
    };
    Ok(fyo
        .db
        .get_all_docs(stock_transfer_schema_name(doc), &options)?
        .into_iter()
        .filter_map(|row| match row.get("name") {
            Some(DocValue::Text(name)) => Some(name.clone()),
            _ => None,
        })
        .collect())
}

/// `_validateStockTransferCancelled`.
fn validate_stock_transfer_cancelled(doc: &Doc, fyo: &Fyo) -> Result<(), Error> {
    let transfers = get_linked_stock_transfer_names(doc, fyo, false)?;
    if transfers.is_empty() {
        return Ok(());
    }

    let schema_name = stock_transfer_schema_name(doc);
    let label = fyo
        .schema(schema_name)
        .map_or(schema_name.to_owned(), |s| s.label.clone());
    Err(Error::Validation(format!(
        "Cannot cancel {} {} because of the following {label}: {}",
        doc.schema().label,
        doc.name().unwrap_or_default(),
        transfers.join(", ")
    )))
}

/// `_validateHasLinkedReturnInvoices`.
fn validate_has_linked_return_invoices(doc: &Doc, fyo: &Fyo) -> Result<(), Error> {
    let Some(name) = doc.name() else {
        return Ok(());
    };
    if is_return(doc) || is_quote(doc) {
        return Ok(());
    }

    let options = GetAllOptions {
        filters: QueryFilter::from([("returnAgainst".to_owned(), Filter::from(name))]),
        ..GetAllOptions::default()
    };
    let names: Vec<String> = fyo
        .db
        .get_all_docs(doc.schema_name(), &options)?
        .iter()
        .map(|row| crate::fyo::model::helpers::js_string(row.get("name")))
        .collect();
    if names.is_empty() {
        return Ok(());
    }
    Err(Error::Validation(format!(
        "Cannot cancel {name} because of the following {}: {}",
        doc.schema().label,
        names.join(", ")
    )))
}

/// Whether `_createLoyaltyPointEntry` would make an entry: the invoice has a loyalty
/// program that runs on its date.
fn earns_loyalty_points(doc: &Doc, fyo: &Fyo) -> Result<bool, Error> {
    let Some(program) = text(doc, "loyaltyProgram").filter(|p| !p.is_empty()) else {
        return Ok(false);
    };
    let program = fyo.db.get_doc("LoyaltyProgram", program)?;
    let date = |value: Option<&DocValue>| match value {
        Some(DocValue::Date(date)) => Some(*date),
        _ => None,
    };
    let (Some(expiry_date), Some(from_date), Some(to_date)) = (
        date(doc.get("date")),
        date(program.get("fromDate")),
        date(program.get("toDate")),
    ) else {
        return Ok(false);
    };
    Ok(from_date <= expiry_date && to_date >= expiry_date)
}

/// Whether `removeLoyaltyPoint` would change an entry: one was made for the invoice.
fn has_loyalty_point_entry(doc: &Doc, fyo: &Fyo) -> Result<bool, Error> {
    let Some(program) = text(doc, "loyaltyProgram").filter(|p| !p.is_empty()) else {
        return Ok(false);
    };
    let options = GetAllOptions {
        filters: QueryFilter::from([
            ("loyaltyProgram".to_owned(), Filter::from(program)),
            (
                "invoice".to_owned(),
                Filter::from(doc.name().unwrap_or_default()),
            ),
        ]),
        ..GetAllOptions::default()
    };
    Ok(!fyo
        .db
        .get_all_docs("LoyaltyPointEntry", &options)?
        .is_empty())
}

/// `_updatePartyOutStanding`.
fn update_party_outstanding(doc: &Doc, fyo: &mut Fyo) -> Result<(), Error> {
    let mut party = fyo.get_doc("Party", text(doc, "party"))?;
    party::update_outstanding_amount(&mut party, fyo)
}

impl Transactional for Invoice {
    fn get_posting(&self, doc: &Doc, fyo: &mut Fyo) -> Result<Option<LedgerPosting>, Error> {
        Ok(Some((self.get_posting)(doc, fyo)?))
    }
}

impl Model for Invoice {
    fn default(&self, fieldname: &str, doc: &Doc, fyo: &Fyo) -> Option<DocValue> {
        match fieldname {
            "makeAutoPayment" => Some(DocValue::Bool(auto_payment_account(doc, fyo).is_some())),
            "makeAutoStockTransfer" => Some(DocValue::Bool(
                single_is_truthy(fyo, "AccountingSettings", "enableInventory")
                    && auto_stock_transfer_location(doc, fyo).is_some(),
            )),
            "numberSeries" => get_number_series(doc.schema_name(), fyo).map(DocValue::Text),
            "terms" => {
                let fieldname = if doc.schema_name() == "SalesInvoice" {
                    "salesInvoiceTerms"
                } else {
                    "purchaseInvoiceTerms"
                };
                match single_value(fyo, "Defaults", fieldname) {
                    None | Some(DocValue::Null) => Some(DocValue::Text(String::new())),
                    terms => terms,
                }
            }
            "date" => Some(DocValue::Date(Timestamp::now())),
            _ => None,
        }
    }

    fn formulas(&self) -> Vec<Formula> {
        formulas()
    }

    fn validate_field(
        &self,
        fieldname: &str,
        value: &DocValue,
        ctx: &DocContext<'_>,
    ) -> Result<(), Error> {
        (self.validate_field)(fieldname, value, ctx)
    }

    fn hidden(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<bool> {
        let doc = ctx.doc;
        let fyo = ctx.fyo;
        let truthy = |fieldname| is_truthy(doc.get(fieldname));
        let is_done = doc.is_submitted() || doc.is_cancelled();
        let accounting = |fieldname| single_is_truthy(fyo, "AccountingSettings", fieldname);
        Some(match fieldname {
            "makeAutoPayment" => truthy("submitted") || auto_payment_account(doc, fyo).is_none(),
            "makeAutoStockTransfer" => {
                truthy("submitted")
                    || !accounting("enableInventory")
                    || auto_stock_transfer_location(doc, fyo).is_none()
            }
            "setDiscountAmount" | "discountAmount" | "discountPercent" => true,
            "discountAfterTax" => !enable_discounting(fyo),
            "taxes" => doc.table("taxes").is_empty(),
            "baseGrandTotal" => {
                doc.get("exchangeRate") == Some(&DocValue::Number(1.0))
                    || money(doc, "baseGrandTotal").is_some_and(|m| m.is_zero())
            }
            "terms" | "attachment" => !(truthy(fieldname) || !is_done),
            "backReference" | "quote" | "loyaltyProgram" => !truthy(fieldname),
            "loyaltyPoints" => !truthy("redeemLoyaltyPoints") || is_return(doc),
            "redeemLoyaltyPoints" => !truthy("loyaltyProgram") || is_return(doc),
            "coupons" => doc.is_submitted() && doc.table("coupons").is_empty(),
            "priceList" => {
                !accounting("enablePriceList") || (!doc.can_edit() && !truthy("priceList"))
            }
            "returnAgainst" => is_done && !truthy("returnAgainst"),
            "pricingRuleDetail" => {
                !accounting("enablePricingRule") || doc.table("pricingRuleDetail").is_empty()
            }
            _ => return None,
        })
    }

    fn currency(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<String> {
        let doc = ctx.doc;
        match fieldname {
            "baseGrandTotal" | "outstandingAmount" => Some(company_currency(ctx.fyo)),
            _ if doc.schema().field(fieldname)?.fieldtype == Some(FieldType::Currency) => {
                if doc.get("exchangeRate") == Some(&DocValue::Number(1.0)) {
                    return Some(company_currency(ctx.fyo));
                }
                Some(text(doc, "currency").unwrap_or(DEFAULT_CURRENCY).to_owned())
            }
            _ => None,
        }
    }

    /// `beforeSync`: free items go when no pricing rule applies.
    fn before_sync(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        if !doc.table("pricingRuleDetail").is_empty() {
            return Err(not_ported("Pricing rules"));
        }
        if !doc.table("items").is_empty() && !is_return(doc) {
            let items: Vec<Doc> = doc
                .table("items")
                .iter()
                .filter(|item| !is_truthy(item.get("isFreeItem")))
                .cloned()
                .collect();
            doc.set_table_rows("items", items);
        }
        if !doc.table("coupons").is_empty() {
            return Err(not_ported("Coupons"));
        }
        let _ = fyo;
        Ok(())
    }

    fn validate(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        transactional::validate(self, doc, fyo)?;
        if is_quote(doc) {
            return Ok(());
        }

        if enable_discounting(fyo)
            && single_text(fyo, "AccountingSettings", "discountAccount").is_none()
        {
            return Err(Error::Validation("Discount Account is not set.".into()));
        }
        validate_batch(doc, fyo)?;

        // `_validatePricingRule`.
        if single_is_truthy(fyo, "AccountingSettings", "enablePricingRule") {
            get_pricing_rule(doc, fyo)?;
        }
        if is_truthy(doc.get("redeemLoyaltyPoints")) {
            return Err(not_ported("Redeeming loyalty points"));
        }
        Ok(())
    }

    fn before_submit(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        let party = text(doc, "party").unwrap_or_default();
        let party_points = match fyo.get_value("Party", Some(party), "loyaltyPoints") {
            Some(DocValue::Number(points)) => points,
            _ => 0.0,
        };
        let loyalty_points = number(doc, "loyaltyPoints").unwrap_or(f64::NAN);
        if loyalty_points > party_points {
            return Err(Error::Validation(format!(
                "{party} only has {} points",
                js_number_string(party_points)
            )));
        }

        if is_return(doc) {
            return Err(not_ported("Submitting a return"));
        }
        if earns_loyalty_points(doc, fyo)? {
            return Err(not_ported("Earning loyalty points"));
        }
        if is_truthy(doc.get("makeAutoPayment")) && auto_payment_account(doc, fyo).is_some() {
            return Err(not_ported("Making the payment on submit"));
        }
        if is_truthy(doc.get("makeAutoStockTransfer"))
            && auto_stock_transfer_location(doc, fyo).is_some()
        {
            return Err(not_ported("Making the shipment or receipt on submit"));
        }
        Ok(())
    }

    fn after_submit(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        transactional::after_submit(self, doc, fyo)?;
        if is_quote(doc) {
            return Ok(());
        }

        let data = DocValueMap::from([
            (
                "name".to_owned(),
                DocValue::Text(doc.name().unwrap_or_default().to_owned()),
            ),
            (
                "outstandingAmount".to_owned(),
                doc.get("baseGrandTotal").cloned().unwrap_or(DocValue::Null),
            ),
        ]);
        fyo.db.update(doc.schema_name(), &data)?;
        update_party_outstanding(doc, fyo)
    }

    fn before_cancel(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        validate_stock_transfer_cancelled(doc, fyo)?;
        validate_has_linked_return_invoices(doc, fyo)?;

        if !get_payment_ids(doc, fyo)?.is_empty() {
            return Err(not_ported("Cancelling an invoice with payments"));
        }
        if is_return(doc) {
            return Err(not_ported("Cancelling a return"));
        }
        if has_loyalty_point_entry(doc, fyo)? {
            return Err(not_ported("Cancelling an invoice with loyalty points"));
        }
        if !doc.table("coupons").is_empty() {
            return Err(not_ported("Cancelling an invoice with coupons"));
        }
        Ok(())
    }

    fn after_cancel(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        transactional::after_cancel(self, doc, fyo)?;
        update_party_outstanding(doc, fyo)
    }

    fn before_delete(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        validate_stock_transfer_cancelled(doc, fyo)?;
        if !get_linked_stock_transfer_names(doc, fyo, true)?.is_empty() {
            return Err(not_ported(
                "Deleting an invoice with cancelled shipments or receipts",
            ));
        }
        if !get_payment_ids(doc, fyo)?.is_empty() {
            return Err(not_ported("Deleting an invoice with payments"));
        }
        Ok(())
    }

    fn after_delete(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        transactional::after_delete(self, doc, fyo)
    }
}
