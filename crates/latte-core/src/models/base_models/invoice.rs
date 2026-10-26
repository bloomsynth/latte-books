// Port of models/baseModels/Invoice/Invoice.ts @ v0.36.0, the model SalesInvoice and
// PurchaseInvoice share. Each gives its own `getPosting` (sales_invoice.rs and
// purchase_invoice.rs).
//
// Deviations:
// - Exchange rates aren't fetched from api.vatcomply.com. An invoice in a foreign
//   currency gets the rate 1, as Books does where it can't fetch, until the rate is set.
// - Pricing rules are applied in the `change` hook when the items or coupons change, as
//   formulas can't change other fields here. Books applies them in the
//   `isPricingRuleApplied` formula.
// - A return of an invoice with loyalty points is refused (loyalty.rs).
//
// Books bugs Latte fixes, for returns:
// - What is left to return counts only submitted returns. Books counts drafts and
//   cancelled returns too, so a cancelled return's items can't be returned again.
// - An item on two rows is returned once, not twice over.
// - A return can't take back more than is left to return. Books doesn't check.
// - A return doesn't copy whether its invoice was returned.
// - Cancelling a return clears the invoice's Fully Returned flag. Books only sets it.
// - A refund for a return is for the positive amount the return is owed. Books makes it
//   for the negative outstanding amount it saved, which the payment refuses.
// - A return posts the size of its amounts on the swapped sides, so it reverses its
//   invoice. Books posts its negative amounts there, which posts the invoice again, and
//   skips a return's discount, which is negative, leaving it to the round-off.
//
// - A return's shipment or receipt is made against its invoice's own. Books looks for a
//   shipment of a sales invoice even for a purchase return, and fails where there's none.
//
// Books bugs Latte fixes, for pricing rules and loyalty points:
// - Is Pricing Rule Applied is set only when a rule applies. Books sets it on every sales
//   invoice once pricing rules are on.
// - Redeemed loyalty points come off the grand total once. Books takes them off the grand
//   total and then credits the customer with them again, leaving the round-off account
//   to balance the entry.
//
// Not ported yet: `getLinkedPayments` and `getLinkedStockTransfers` (for the UI),
// `addItem`, and, as the UI needs them, `filters` and `createFilters`.

use indexmap::IndexMap;
use jiff::Timestamp;
use latte_db::pesa::Money;
use latte_db::{DocValue, DocValueMap, Filter, GetAllOptions, QueryFilter};
use latte_schema::FieldType;

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::helpers::{is_truthy, js_number_string};
use crate::fyo::model::types::{DocContext, Formula, FormulaValue, Model};
use crate::models::base_models::pricing_rule::{
    ApplicablePricingRule, apply_product_discount, get_invoice_pricing_rules,
};
use crate::models::base_models::{coupon_code, loyalty, party};
use crate::models::helpers::{
    DEFAULT_CURRENCY, company_currency, get_number_series, linked_value, money, money_or_zero,
    number, one_of, query, single_is_truthy, single_text, single_value, text,
};
use crate::models::inventory::helpers::{get_item_visibility, get_stock_quantity};
use crate::models::transactional::ledger_posting::LedgerPosting;
use crate::models::transactional::{self, Transactional};

type GetPosting = fn(&Doc, &mut Fyo) -> Result<LedgerPosting, Error>;
type ValidateField = fn(&str, &DocValue, &DocContext<'_>) -> Result<(), Error>;

/// SalesInvoice's, PurchaseInvoice's or SalesQuote's model, which differ in their posting
/// and `validations`. A quote posts nothing.
pub struct Invoice {
    get_posting: Option<GetPosting>,
    validate_field: ValidateField,
}

impl Invoice {
    pub(crate) const fn new(get_posting: GetPosting, validate_field: ValidateField) -> Self {
        Self {
            get_posting: Some(get_posting),
            validate_field,
        }
    }

    pub(crate) const fn without_posting(validate_field: ValidateField) -> Self {
        Self {
            get_posting: None,
            validate_field,
        }
    }
}

type FormulaResult = Result<Option<FormulaValue>, Error>;

fn some(value: DocValue) -> FormulaResult {
    Ok(Some(value.into()))
}

pub(crate) fn not_ported(what: &str) -> Error {
    Error::Validation(format!("{what} isn't supported in Latte yet."))
}

/// `isSales`.
pub(crate) fn is_sales(doc: &Doc) -> bool {
    matches!(doc.schema_name(), "SalesInvoice" | "SalesQuote")
}

/// `isQuote`.
pub(crate) fn is_quote(doc: &Doc) -> bool {
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
pub(crate) fn auto_stock_transfer_location(doc: &Doc, fyo: &Fyo) -> Option<String> {
    let fieldname = if is_sales(doc) {
        "shipmentLocation"
    } else {
        "purchaseReceiptLocation"
    };
    single_text(fyo, "Defaults", fieldname)
}

/// `stockTransferSchemaName`.
pub(crate) fn stock_transfer_schema_name(doc: &Doc) -> &'static str {
    if is_sales(doc) {
        "Shipment"
    } else {
        "PurchaseReceipt"
    }
}

fn required_money(doc: &Doc, fieldname: &str) -> Result<Money, Error> {
    money(doc, fieldname).ok_or_else(|| Error::Value(format!("{fieldname} is not money")))
}

/// One tax on one row: `InvoiceTaxItem`, less `fullAmount`, which isn't used.
pub(crate) struct TaxItem {
    pub(crate) account: String,
    pub(crate) payment_account: Option<String>,
    pub(crate) rate: f64,
    pub(crate) tax_amount: Money,
    pub(crate) exchange_rate: f64,
}

/// `getTaxItems`: each row's taxes, on its amount less its discount, or on its whole
/// amount when the discount is applied after tax.
///
/// Books bug Latte fixes: with the discount after tax, Books leaves out every tax, so the
/// invoice charges and posts none.
pub(crate) fn get_tax_items(doc: &Doc, fyo: &Fyo) -> Result<Vec<TaxItem>, Error> {
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

            if !is_truthy(doc.get("discountAfterTax")) {
                let mut item_discount_amount = get_discount_amount(doc, fyo, item)?;
                if is_return(doc) && item_discount_amount.is_negative() {
                    item_discount_amount = item_discount_amount.abs()?;
                }
                amount = if is_return(doc) {
                    amount.add(&item_discount_amount)?
                } else {
                    amount.sub(&item_discount_amount)?
                };
            }

            let rate = match details.get("rate") {
                Some(DocValue::Number(rate)) => *rate,
                _ => f64::NAN,
            };
            let account = match details.get("account") {
                Some(DocValue::Text(account)) => account.clone(),
                _ => String::new(),
            };
            let payment_account = match details.get("payment_account") {
                Some(DocValue::Text(account)) => Some(account.clone()),
                _ => None,
            };
            tax_items.push(TaxItem {
                account,
                payment_account,
                rate,
                tax_amount: amount.mul(rate / 100.0)?,
                exchange_rate: exchange_rate(doc),
            });
        }
    }
    Ok(tax_items)
}

/// `getTaxSummary`: the taxes by account, as TaxSummary rows, less those that come to
/// zero.
pub fn get_tax_summary(doc: &Doc, fyo: &Fyo) -> Result<Vec<DocValueMap>, Error> {
    let mut taxes: IndexMap<String, (f64, Money)> = IndexMap::new();
    for TaxItem {
        account,
        rate,
        tax_amount,
        ..
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
/// An amount as a posting takes it. A return's totals are negative, and Books posts them
/// as they are, on the swapped sides, which posts the invoice again instead of reversing
/// it. Latte posts their size, as Books' own tests expect.
pub(crate) fn posting_amount(amount: Money, is_return: bool) -> Result<Money, Error> {
    if is_return {
        return Ok(amount.abs()?);
    }
    Ok(amount)
}

pub fn get_total_discount(doc: &Doc, fyo: &Fyo) -> Result<Money, Error> {
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
        return get_lp_added_base_grand_total(doc, fyo);
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

/// The grand total before redeemed points come off it, as `getLPAddedBaseGrandTotal` and
/// the `loyaltyPoints` validation work it out.
pub(crate) fn base_grand_total_before_points(doc: &Doc, fyo: &Fyo) -> Result<Money, Error> {
    let total_discount = get_total_discount(doc, fyo)?;
    let net_total = required_money(doc, "netTotal")?;
    let base_total = if doc.table("taxes").is_empty() {
        net_total
    } else {
        let mut total = net_total.abs()?;
        for tax in doc.table("taxes") {
            total = total.add(&required_money(tax, "amount")?.abs()?)?;
        }
        total
    };
    Ok(base_total.sub(&total_discount)?)
}

/// `getLPAddedBaseGrandTotal` for an invoice that isn't a return: the grand total less
/// what the redeemed points are worth.
fn get_lp_added_base_grand_total(doc: &Doc, fyo: &Fyo) -> Result<Money, Error> {
    let base_total = base_grand_total_before_points(doc, fyo)?;
    let program = text(doc, "loyaltyProgram").unwrap_or_default();
    let points = number(doc, "loyaltyPoints").unwrap_or(0.0);
    Ok(base_total.sub(&loyalty::get_added_lp_with_grand_total(
        fyo, program, points,
    )?)?)
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
pub(crate) fn get_stock_not_transferred(doc: &Doc) -> f64 {
    doc.table("items")
        .iter()
        .map(|item| number(item, "stockNotTransferred").unwrap_or(0.0))
        .sum()
}

/// `appendPricingRuleDetail`, as the `isPricingRuleApplied` formula does it: lists the
/// rules that apply in `pricingRuleDetail`, if they changed.
fn update_pricing_rule_detail(doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
    if !single_is_truthy(fyo, "AccountingSettings", "enablePricingRule") {
        return Ok(());
    }
    let rules = get_invoice_pricing_rules(doc, fyo)?.unwrap_or_default();
    let rows: Vec<DocValueMap> = rules
        .iter()
        .map(
            |ApplicablePricingRule {
                 apply_on_item,
                 pricing_rule,
             }| {
                DocValueMap::from([
                    (
                        "referenceName".to_owned(),
                        pricing_rule.get("name").cloned().unwrap_or(DocValue::Null),
                    ),
                    (
                        "referenceItem".to_owned(),
                        DocValue::Text(apply_on_item.clone()),
                    ),
                ])
            },
        )
        .collect();
    let current: Vec<(Option<&str>, Option<&str>)> = doc
        .table("pricingRuleDetail")
        .iter()
        .map(|row| (text(row, "referenceName"), text(row, "referenceItem")))
        .collect();
    let wanted: Vec<(Option<&str>, Option<&str>)> = rows
        .iter()
        .map(|row| {
            let get = |fieldname| match row.get(fieldname) {
                Some(DocValue::Text(text)) => Some(text.as_str()),
                _ => None,
            };
            (get("referenceName"), get("referenceItem"))
        })
        .collect();
    if current == wanted {
        return Ok(());
    }

    // Books' rows keep the rule they last had (`prule`) and clear its discount when it
    // stops applying.
    let dropped: Vec<String> = current
        .iter()
        .filter_map(|(_, item)| *item)
        .filter(|item| !wanted.iter().any(|(_, wanted)| wanted == &Some(*item)))
        .map(str::to_owned)
        .collect();
    if !dropped.is_empty() {
        let zero = fyo.pesa().zero();
        let items = doc
            .table("items")
            .iter()
            .cloned()
            .map(|mut row| {
                if text(&row, "item").is_some_and(|item| dropped.iter().any(|d| d == item)) {
                    row.set_value("itemDiscountAmount", DocValue::Money(zero.clone()));
                    row.set_value("itemDiscountPercent", DocValue::Number(0.0));
                    row.set_value("setItemDiscountAmount", DocValue::Bool(false));
                }
                row
            })
            .collect();
        doc.set_table_rows("items", items);
    }
    // `set('pricingRuleDetail', null)`, which runs the rows' rate formula even when no
    // rule is left, then the rows: a table value adds rows.
    doc.set(fyo, "pricingRuleDetail", DocValue::Null)?;
    if !rows.is_empty() {
        doc.set(fyo, "pricingRuleDetail", DocValue::Table(rows))?;
    }
    Ok(())
}

/// The values of the invoice a return is against.
fn return_against(doc: &Doc, fyo: &Fyo) -> Result<DocValueMap, Error> {
    let name = text(doc, "returnAgainst").unwrap_or_default();
    Ok(fyo.db.get_doc(doc.schema_name(), name)?)
}

/// An item and its batch, as returns count them.
pub(crate) type ItemKey = (String, Option<String>);

pub(crate) fn map_text(row: &DocValueMap, fieldname: &str) -> Option<String> {
    match row.get(fieldname) {
        Some(DocValue::Text(text)) if !text.is_empty() => Some(text.clone()),
        _ => None,
    }
}

pub(crate) fn map_number(row: &DocValueMap, fieldname: &str) -> f64 {
    match row.get(fieldname) {
        Some(DocValue::Number(number)) => *number,
        _ => 0.0,
    }
}

/// The quantity of each item and batch on an invoice's rows, free items left out.
pub(crate) fn item_quantities(rows: &[DocValueMap]) -> IndexMap<ItemKey, f64> {
    let mut quantities = IndexMap::new();
    for row in rows {
        let Some(item) = map_text(row, "item") else {
            continue;
        };
        if is_truthy(row.get("isFreeItem")) {
            continue;
        }
        *quantities
            .entry((item, map_text(row, "batch")))
            .or_insert(0.0) += map_number(row, "quantity");
    }
    quantities
}

/// The submitted returns against an invoice, other than `except`.
pub(crate) fn submitted_returns(
    fyo: &Fyo,
    schema_name: &str,
    name: &str,
    except: Option<&str>,
) -> Result<Vec<DocValueMap>, Error> {
    let options = GetAllOptions {
        fields: Some(vec!["name".to_owned()]),
        filters: QueryFilter::from([
            ("returnAgainst".to_owned(), Filter::from(name)),
            ("submitted".to_owned(), Filter::from(true)),
            ("cancelled".to_owned(), Filter::from(false)),
        ]),
        ..GetAllOptions::default()
    };
    let mut returns = Vec::new();
    for row in fyo.db.get_all_docs(schema_name, &options)? {
        let Some(DocValue::Text(return_name)) = row.get("name") else {
            continue;
        };
        if Some(return_name.as_str()) == except {
            continue;
        }
        returns.push(fyo.db.get_doc(schema_name, return_name)?);
    }
    Ok(returns)
}

pub(crate) fn table_rows(values: &DocValueMap, fieldname: &str) -> Vec<DocValueMap> {
    match values.get(fieldname) {
        Some(DocValue::Table(rows)) => rows.clone(),
        _ => Vec::new(),
    }
}

/// `getReturnQtyTotal`: what is left to return of each item and batch on an invoice, after
/// its submitted returns other than `except`. Books counts draft and cancelled returns
/// too, so cancelling a return doesn't let the items be returned again.
pub(crate) fn get_return_qty_total(
    fyo: &Fyo,
    schema_name: &str,
    name: &str,
    except: Option<&str>,
) -> Result<IndexMap<ItemKey, f64>, Error> {
    let invoice = fyo.db.get_doc(schema_name, name)?;
    let mut quantities = item_quantities(&table_rows(&invoice, "items"));
    for returned in submitted_returns(fyo, schema_name, name, except)? {
        for (key, quantity) in item_quantities(&table_rows(&returned, "items")) {
            if let Some(left) = quantities.get_mut(&key) {
                *left -= quantity.abs();
            }
        }
    }
    Ok(quantities)
}

/// `getReturnDoc`: a new return of what is left to return on a submitted invoice, dated
/// now, or `None` if the invoice isn't saved.
///
/// Each row takes back what is left of its item and batch, up to its own quantity, so an
/// item on two rows is returned once. Books gives each row all that is left of its item,
/// so such an item is returned twice over. The return doesn't copy whether the invoice
/// was returned, which Books does, marking a second return as returned itself.
pub fn get_return_doc(doc: &Doc, fyo: &Fyo) -> Result<Option<Doc>, Error> {
    let Some(name) = doc.name() else {
        return Ok(None);
    };

    let mut data = doc.get_valid_dict(true, true);
    data.insert("pricingRuleDetail".to_owned(), DocValue::Table(Vec::new()));
    let Some(DocValue::Table(rows)) = data.get("items") else {
        return Ok(None);
    };

    let mut left = get_return_qty_total(fyo, doc.schema_name(), name, None)?;
    let mut items = Vec::new();
    for row in rows {
        let quantity = map_number(row, "quantity");
        let mut row = row.clone();
        row.shift_remove("name");
        let unit_conversion_factor = map_number(&row, "unitConversionFactor");

        let returned = if is_truthy(row.get("isFreeItem")) {
            quantity
        } else {
            let Some(item) = map_text(&row, "item") else {
                continue;
            };
            let Some(left) = left.get_mut(&(item, map_text(&row, "batch"))) else {
                continue;
            };
            let returned = quantity.min(*left);
            *left -= returned.max(0.0);
            returned
        };
        if returned <= 0.0 {
            continue;
        }

        row.insert("quantity".to_owned(), DocValue::Number(-returned));
        row.insert(
            "transferQuantity".to_owned(),
            DocValue::Number(-returned / unit_conversion_factor),
        );
        items.push(row);
    }

    data.shift_remove("name");
    data.insert("date".to_owned(), DocValue::Date(Timestamp::now()));
    data.insert("items".to_owned(), DocValue::Table(items));
    data.insert("returnAgainst".to_owned(), DocValue::Text(name.to_owned()));
    data.insert("isReturned".to_owned(), DocValue::Bool(false));
    data.insert("isFullyReturned".to_owned(), DocValue::Bool(false));

    let mut return_doc = fyo.get_new_doc(doc.schema_name(), data)?;
    return_doc.run_formulas(fyo)?;
    Ok(Some(return_doc))
}

/// A return takes back no more of each item and batch than the invoice has left to
/// return. Books doesn't check, so a return can take back more than was sold.
pub(crate) fn validate_return_quantities(doc: &Doc, fyo: &Fyo) -> Result<(), Error> {
    let against = text(doc, "returnAgainst").unwrap_or_default();
    let left = get_return_qty_total(fyo, doc.schema_name(), against, doc.name())?;
    let rows: Vec<DocValueMap> = doc
        .table("items")
        .iter()
        .map(|row| row.get_valid_dict(false, false))
        .collect();
    for ((item, batch), quantity) in item_quantities(&rows) {
        let returning = quantity.abs();
        let what = match &batch {
            Some(batch) => format!("{item} (batch {batch})"),
            None => item.clone(),
        };
        let Some(&left) = left.get(&(item, batch)) else {
            return Err(Error::Validation(format!("{what} is not on {against}.")));
        };
        if returning > left {
            return Err(Error::Validation(format!(
                "Cannot return {} of {what}: only {} is left to return on {against}.",
                js_number_string(returning),
                js_number_string(left.max(0.0))
            )));
        }
    }
    Ok(())
}

/// `_updateIsItemsReturned`: marks the invoice a return is against as returned while it
/// has a submitted return.
fn update_is_items_returned(doc: &Doc, fyo: &mut Fyo) -> Result<(), Error> {
    if !is_return(doc) || is_quote(doc) {
        return Ok(());
    }
    let against = text(doc, "returnAgainst").unwrap_or_default().to_owned();
    let is_returned = !submitted_returns(fyo, doc.schema_name(), &against, None)?.is_empty();
    let mut invoice = fyo.get_doc(doc.schema_name(), Some(&against))?;
    invoice.set_and_sync(fyo, "isReturned", DocValue::Bool(is_returned))
}

/// `updateIsItemsFullyReturned`: marks a sales invoice fully returned once nothing is left
/// to return. Books only ever sets it, so an invoice stays fully returned after its return
/// is cancelled; here cancelling clears it.
fn update_is_items_fully_returned(doc: &Doc, fyo: &mut Fyo) -> Result<(), Error> {
    if !is_return(doc) || doc.schema_name() != "SalesInvoice" {
        return Ok(());
    }
    let against = text(doc, "returnAgainst").unwrap_or_default().to_owned();
    let left = get_return_qty_total(fyo, doc.schema_name(), &against, None)?;
    let is_fully_returned = left.values().all(|left| *left <= 0.0);
    let mut invoice = fyo.get_doc(doc.schema_name(), Some(&against))?;
    if is_truthy(invoice.get("isFullyReturned")) == is_fully_returned {
        return Ok(());
    }
    invoice.set_and_sync(fyo, "isFullyReturned", DocValue::Bool(is_fully_returned))
}

/// What a return can't do yet: take back loyalty points.
fn validate_return_supported(doc: &Doc, fyo: &Fyo) -> Result<(), Error> {
    let against = text(doc, "returnAgainst").unwrap_or_default();
    let program = fyo.get_value(doc.schema_name(), Some(against), "loyaltyProgram");
    let program = match &program {
        Some(DocValue::Text(program)) => Some(program.as_str()),
        _ => None,
    };
    if loyalty::loyalty_point_entry_exists(fyo, program, against)? {
        return Err(not_ported("Returning an invoice with loyalty points"));
    }
    Ok(())
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
                // `pricingRuleDetail` is set in the `change` hook.
                let applied = !get_invoice_pricing_rules(ctx.doc, ctx.fyo)?
                    .unwrap_or_default()
                    .is_empty();
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
pub fn get_payment_ids(doc: &Doc, fyo: &Fyo) -> Result<Vec<String>, Error> {
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

/// The shipment or receipt a return's invoice was shipped or received with: the newest
/// one not cancelled. Books looks for a shipment of a sales invoice even for a purchase
/// return, and fails where there's none.
fn returned_stock_transfer(doc: &Doc, fyo: &Fyo) -> Result<Option<String>, Error> {
    let against = text(doc, "returnAgainst").unwrap_or_default();
    let options = GetAllOptions {
        fields: Some(vec!["name".to_owned()]),
        filters: QueryFilter::from([
            ("backReference".to_owned(), Filter::from(against)),
            ("cancelled".to_owned(), Filter::from(false)),
        ]),
        order_by: Some(vec!["created".to_owned()]),
        order: Some(latte_db::Order::Desc),
        ..GetAllOptions::default()
    };
    Ok(fyo
        .db
        .get_all_docs(stock_transfer_schema_name(doc), &options)?
        .first()
        .and_then(|row| map_text(row, "name")))
}

/// `getStockTransfer`: a new shipment or receipt of what a submitted invoice has left to
/// ship or receive, or `None`. One made on submit (`is_auto`) leaves out items with
/// batches or serial numbers, and items without the stock.
pub fn get_stock_transfer(doc: &Doc, fyo: &Fyo, is_auto: bool) -> Result<Option<Doc>, Error> {
    if !doc.is_submitted() {
        return Ok(None);
    }

    let return_against = if is_return(doc) {
        returned_stock_transfer(doc, fyo)?
    } else {
        None
    };

    let item_visibility = get_item_visibility(fyo);
    let inventory_items = item_visibility.as_deref() == Some("Inventory Items");
    let not_transferred = number(doc, "stockNotTransferred").unwrap_or(0.0);
    if (not_transferred == 0.0 || not_transferred.is_nan()) && inventory_items {
        return Ok(None);
    }

    let (terms, number_series) = if is_sales(doc) {
        ("shipmentTerms", "shipmentNumberSeries")
    } else {
        ("purchaseReceiptTerms", "purchaseReceiptNumberSeries")
    };
    let date = Timestamp::now();
    let mut data = DocValueMap::from([
        (
            "party".to_owned(),
            doc.get("party").cloned().unwrap_or(DocValue::Null),
        ),
        ("date".to_owned(), DocValue::Date(date)),
        (
            "terms".to_owned(),
            DocValue::Text(single_text(fyo, "Defaults", terms).unwrap_or_default()),
        ),
        (
            "backReference".to_owned(),
            DocValue::Text(doc.name().unwrap_or_default().to_owned()),
        ),
        (
            "returnAgainst".to_owned(),
            return_against.map_or(DocValue::Null, DocValue::Text),
        ),
    ]);
    if let Some(series) = single_text(fyo, "Defaults", number_series) {
        data.insert("numberSeries".to_owned(), DocValue::Text(series));
    }

    let location = auto_stock_transfer_location(doc, fyo)
        .or_else(|| single_text(fyo, "InventorySettings", "defaultLocation"));
    if is_auto && location.is_none() {
        return Ok(None);
    }
    let location_value = location.clone().map_or(DocValue::Null, DocValue::Text);

    let mut transfer = fyo.get_new_doc(stock_transfer_schema_name(doc), data)?;
    for row in doc.table("items") {
        let Some(item) = text(row, "item").filter(|i| !i.is_empty()) else {
            continue;
        };
        let item_doc = fyo.db.get_doc("Item", item)?;
        if is_auto
            && (is_truthy(item_doc.get("hasBatch")) || is_truthy(item_doc.get("hasSerialNumber")))
        {
            continue;
        }

        let batch = text(row, "batch")
            .filter(|b| !b.is_empty())
            .map_or(DocValue::Null, |b| DocValue::Text(b.to_owned()));
        let mut values = DocValueMap::from([
            ("item".to_owned(), DocValue::Text(item.to_owned())),
            ("location".to_owned(), location_value.clone()),
            ("batch".to_owned(), batch),
            (
                "description".to_owned(),
                row.get("description").cloned().unwrap_or(DocValue::Null),
            ),
            (
                "hsnCode".to_owned(),
                row.get("hsnCode").cloned().unwrap_or(DocValue::Null),
            ),
        ]);

        if is_truthy(row.get("isFreeItem")) {
            values.insert(
                "quantity".to_owned(),
                row.get("quantity").cloned().unwrap_or(DocValue::Null),
            );
            values.insert("rate".to_owned(), DocValue::Money(fyo.pesa().zero()));
            transfer.push_row(fyo, "items", values)?;
            continue;
        }

        let quantity = if is_truthy(item_doc.get("trackItem")) {
            number(row, "stockNotTransferred")
        } else {
            number(row, "quantity")
        }
        .unwrap_or(0.0);
        let mut rate = money_or_zero(row, "rate", fyo);
        let exchange_rate = exchange_rate(doc);
        if exchange_rate > 1.0 {
            rate = rate.mul(exchange_rate)?;
        }

        if (quantity == 0.0 || quantity.is_nan()) && inventory_items {
            continue;
        }

        if is_auto {
            let stock =
                get_stock_quantity(fyo, item, location.as_deref(), None, Some(date), None, &[])?
                    .unwrap_or(0.0);
            if stock < quantity {
                continue;
            }
        }

        values.insert("quantity".to_owned(), DocValue::Number(quantity));
        values.insert("rate".to_owned(), DocValue::Money(rate));
        transfer.push_row(fyo, "items", values)?;
    }

    if transfer.table("items").is_empty() {
        return Ok(None);
    }
    Ok(Some(transfer))
}

/// `getPayment`: a new payment of what is outstanding on a submitted invoice, or `None`
/// if nothing is.
pub fn get_payment(doc: &Doc, fyo: &Fyo) -> Result<Option<Doc>, Error> {
    if !doc.is_submitted() {
        return Ok(None);
    }
    let Some(outstanding_amount) = money(doc, "outstandingAmount") else {
        return Ok(None);
    };
    if outstanding_amount.is_zero() {
        return Ok(None);
    }

    let (mut account_field, mut payment_type) = ("account", "Receive");
    let reference_type = if is_sales(doc) {
        if is_return(doc) {
            (account_field, payment_type) = ("paymentAccount", "Pay");
        }
        "SalesInvoice"
    } else {
        (account_field, payment_type) = ("paymentAccount", "Pay");
        if is_return(doc) {
            (account_field, payment_type) = ("account", "Receive");
        }
        "PurchaseInvoice"
    };
    // Books keeps a return's outstanding amount as is: the positive amount the form
    // works out, but the negative one it saves, which a payment refuses.
    let payment_amount = outstanding_amount.abs()?;
    let row_amount = if is_return(doc) {
        doc.get("grandTotal").cloned().unwrap_or(DocValue::Null)
    } else {
        DocValue::Money(outstanding_amount)
    };

    let value = |fieldname: &str| doc.get(fieldname).cloned().unwrap_or(DocValue::Null);
    let text = |value: &str| DocValue::Text(value.to_owned());
    let mut data = DocValueMap::from([
        ("party".to_owned(), value("party")),
        ("date".to_owned(), DocValue::Date(Timestamp::now())),
        ("paymentType".to_owned(), text(payment_type)),
        ("amount".to_owned(), DocValue::Money(payment_amount)),
        (account_field.to_owned(), value("account")),
        ("referenceType".to_owned(), text(reference_type)),
        (
            "for".to_owned(),
            DocValue::Table(vec![DocValueMap::from([
                ("referenceType".to_owned(), text(doc.schema_name())),
                ("referenceName".to_owned(), value("name")),
                ("amount".to_owned(), row_amount),
            ])]),
        ),
    ]);

    if is_truthy(doc.get("makeAutoPayment"))
        && let Some(auto_payment_account) = auto_payment_account(doc, fyo)
    {
        let fieldname = if is_sales(doc) {
            "paymentAccount"
        } else {
            "account"
        };
        data.insert(fieldname.to_owned(), DocValue::Text(auto_payment_account));
    }

    Ok(Some(fyo.get_new_doc("Payment", data)?))
}

/// `_cancelPayments`.
fn cancel_payments(doc: &Doc, fyo: &mut Fyo) -> Result<(), Error> {
    for payment_id in get_payment_ids(doc, fyo)? {
        let mut payment = fyo.get_doc("Payment", Some(&payment_id))?;
        payment.cancel(fyo)?;
    }
    Ok(())
}

/// `_updatePartyOutStanding`.
fn update_party_outstanding(doc: &Doc, fyo: &mut Fyo) -> Result<(), Error> {
    let mut party = fyo.get_doc("Party", text(doc, "party"))?;
    party::update_outstanding_amount(&mut party, fyo)
}

impl Transactional for Invoice {
    fn get_posting(&self, doc: &Doc, fyo: &mut Fyo) -> Result<Option<LedgerPosting>, Error> {
        match self.get_posting {
            Some(get_posting) => Ok(Some(get_posting(doc, fyo)?)),
            None => Ok(None),
        }
    }
}

impl Model for Invoice {
    fn link_filters(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<QueryFilter> {
        let sales = is_sales(ctx.doc);
        Some(match fieldname {
            "party" => query([(
                "role",
                one_of(&[if sales { "Customer" } else { "Supplier" }, "Both"]),
            )]),
            "account" => query([
                ("isGroup", Filter::from(false)),
                (
                    "accountType",
                    Filter::from(if sales { "Receivable" } else { "Payable" }),
                ),
            ]),
            "numberSeries" => query([("referenceType", Filter::from(ctx.doc.schema_name()))]),
            "priceList" => query([
                ("isEnabled", Filter::from(true)),
                (
                    if sales { "isSales" } else { "isPurchase" },
                    Filter::from(true),
                ),
            ]),
            _ => return None,
        })
    }

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

    /// `isPricingRuleApplied`'s `appendPricingRuleDetail`.
    fn change(&self, doc: &mut Doc, fyo: &mut Fyo, changed: Option<&str>) -> Result<(), Error> {
        if matches!(changed, Some("items" | "coupons")) && !doc.is_submitted() && !is_return(doc) {
            update_pricing_rule_detail(doc, fyo)?;
        }
        Ok(())
    }

    /// `beforeSync`: free items for the product discounts that apply, and only the coupons
    /// that still apply.
    fn before_sync(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        if !doc.table("pricingRuleDetail").is_empty() {
            apply_product_discount(doc, fyo)?;
        } else if !doc.table("items").is_empty() && !is_return(doc) {
            let items: Vec<Doc> = doc
                .table("items")
                .iter()
                .filter(|item| !is_truthy(item.get("isFreeItem")))
                .cloned()
                .collect();
            doc.set_table_rows("items", items);
        }
        if !is_return(doc) {
            coupon_code::remove_unused_coupons(doc, fyo)?;
        }
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
            get_invoice_pricing_rules(doc, fyo)?;
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
            validate_return_supported(doc, fyo)?;
            validate_return_quantities(doc, fyo)?;
        }
        Ok(())
    }

    fn after_submit(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        transactional::after_submit(self, doc, fyo)?;
        if is_return(doc) {
            update_is_items_returned(doc, fyo)?;
            coupon_code::change_used_count(doc, fyo, coupon_code::used_count_change(doc, true))?;
            update_is_items_fully_returned(doc, fyo)?;
        }
        if is_quote(doc) {
            // SalesQuote's `afterSubmit`.
            if text(doc, "referenceType") == Some("Lead") {
                let mut lead = fyo.get_doc("Lead", text(doc, "party"))?;
                lead.set_and_sync(fyo, "status", DocValue::Text("Quotation".to_owned()))?;
            }
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
        update_party_outstanding(doc, fyo)?;

        if is_truthy(doc.get("makeAutoPayment")) && auto_payment_account(doc, fyo).is_some() {
            if let Some(mut payment) = get_payment(doc, fyo)? {
                payment.sync(fyo)?;
                payment.submit(fyo)?;
            }
            doc.load(fyo)?;
        }

        if is_truthy(doc.get("makeAutoStockTransfer"))
            && auto_stock_transfer_location(doc, fyo).is_some()
        {
            if let Some(mut transfer) = get_stock_transfer(doc, fyo, true)? {
                transfer.sync(fyo)?;
                transfer.submit(fyo)?;
            }
            doc.load(fyo)?;
        }

        if !is_return(doc) {
            loyalty::create_loyalty_point_entry(doc, fyo)?;
            if doc.schema_name() == "SalesInvoice" {
                coupon_code::change_used_count(doc, fyo, 1.0)?;
            }
        }
        Ok(())
    }

    fn before_cancel(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        validate_stock_transfer_cancelled(doc, fyo)?;
        validate_has_linked_return_invoices(doc, fyo)?;

        if is_return(doc) {
            validate_return_supported(doc, fyo)?;
        }
        Ok(())
    }

    fn after_cancel(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        transactional::after_cancel(self, doc, fyo)?;
        cancel_payments(doc, fyo)?;
        update_party_outstanding(doc, fyo)?;
        update_is_items_returned(doc, fyo)?;
        if !is_return(doc) {
            loyalty::remove_loyalty_points(doc, fyo)?;
        }
        coupon_code::change_used_count(doc, fyo, coupon_code::used_count_change(doc, false))?;
        update_is_items_fully_returned(doc, fyo)
    }

    fn before_delete(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        validate_stock_transfer_cancelled(doc, fyo)?;

        // `_deleteCancelledStockTransfers`.
        let schema_name = stock_transfer_schema_name(doc);
        for name in get_linked_stock_transfer_names(doc, fyo, true)? {
            let mut transfer = fyo.get_doc(schema_name, Some(&name))?;
            transfer.delete(fyo)?;
        }
        Ok(())
    }

    fn after_delete(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        transactional::after_delete(self, doc, fyo)?;
        for name in get_payment_ids(doc, fyo)? {
            let mut payment = fyo.get_doc("Payment", Some(&name))?;
            payment.delete(fyo)?;
        }
        Ok(())
    }
}
