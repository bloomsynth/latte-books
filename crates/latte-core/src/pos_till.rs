// Port of what the point of sale does at the till besides drawing it, @ v0.36.0:
// POS.vue (`setShortcuts`, the quick quantity of Q and digits, `setCustomer`,
// `selectedReturnInvoice`), ClassicPOS.vue and ModernPOS.vue (which UI), the row editing
// of components/POS/Classic/SelectedItemRow.vue and Modern/ModernPOSSelectedItemRow.vue,
// KeyboardModal.vue, CouponCodeModal.vue, LoyaltyProgramModal.vue, PriceListModal.vue,
// ItemEnquiryModal.vue and ReturnSalesInvoiceModal.vue, with `validateQty` of
// models/helpers.ts and `validateSerialNumberCount` of src/utils/pos.ts.
//
// Fixed here (docs/DEVIATIONS.md):
// - A quantity refused by `validateQty` is put back as it was. Books keeps the refused
//   quantity on the row (the Classic row and the keypad), and only the quick quantity
//   puts it back.
// - `validateQty` checks the row being changed. Books checks the first row of the item,
//   which for a batched item can be a row of another batch.
// - The keypad refuses a quantity of zero or less on a sale, as the row and the quick
//   quantity do. Books' keypad takes it.
// - Cancelling the loyalty points modal takes back the points it redeemed. Books leaves
//   them redeemed.
// - Changing a row's quantity keeps its discount as it was, and doesn't clear the discount
//   a pricing rule gives for the new quantity, which Books' row does when the row had no
//   discount of its own.

use latte_db::pesa::Money;
use latte_db::{DocValue, DocValueMap, Filter, GetAllOptions, Order, QueryFilter};
use rusqlite::params;

use super::{ItemQtyMap, item_is_tracked};
use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::format::Formatter;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::helpers::{is_truthy, js_number_string};
use crate::models::base_models::coupon_code::validate_coupon_code;
use crate::models::base_models::invoice::{get_return_doc, is_return};
use crate::models::helpers::{money, number, single_is_truthy, single_text, text};

fn validation<T>(message: impl Into<String>) -> Result<T, Error> {
    Err(Error::Validation(message.into()))
}

/// The names of a schema's documents, by name, that match `filters`.
fn names(fyo: &Fyo, schema_name: &str, filters: QueryFilter) -> Result<Vec<String>, Error> {
    let options = GetAllOptions {
        fields: Some(vec!["name".to_owned()]),
        filters,
        order_by: Some(vec!["name".to_owned()]),
        order: Some(Order::Asc),
        ..GetAllOptions::default()
    };
    Ok(fyo
        .db
        .get_all_docs(schema_name, &options)?
        .into_iter()
        .filter_map(|row| match row.get("name") {
            Some(DocValue::Text(name)) => Some(name.clone()),
            _ => None,
        })
        .collect())
}

/// The point of sale's two looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PosUi {
    Classic,
    Modern,
}

/// Which UI POS.vue shows: Classic when the profile says so, or when it says nothing and
/// POS Settings say so (Classic by default); Modern otherwise.
pub fn pos_ui(fyo: &Fyo) -> PosUi {
    let profile = single_text(fyo, "POSSettings", "posProfile").and_then(|profile| {
        match fyo.get_value("POSProfile", Some(&profile), "posUI") {
            Some(DocValue::Text(ui)) if !ui.is_empty() => Some(ui),
            _ => None,
        }
    });
    let ui = profile
        .or_else(|| single_text(fyo, "POSSettings", "posUI"))
        .unwrap_or_else(|| "Classic".to_owned());
    if ui == "Classic" {
        PosUi::Classic
    } else {
        PosUi::Modern
    }
}

/// A flag of the POS profile or POS Settings, as SelectedItemRow reads `canChangeRate`
/// and `canEditDiscount`: either, with a profile; POS Settings' without one.
fn profile_or_settings(fyo: &Fyo, fieldname: &str) -> bool {
    let settings = single_is_truthy(fyo, "POSSettings", fieldname);
    match single_text(fyo, "POSSettings", "posProfile") {
        Some(profile) => {
            is_truthy(
                fyo.get_value("POSProfile", Some(&profile), fieldname)
                    .as_ref(),
            ) || settings
        }
        None => settings,
    }
}

/// Whether a row's rate can be changed at the till.
pub fn can_change_rate(fyo: &Fyo) -> bool {
    profile_or_settings(fyo, "canChangeRate")
}

/// Whether a row's discount can be changed at the till.
pub fn can_edit_discount(fyo: &Fyo) -> bool {
    profile_or_settings(fyo, "canEditDiscount")
}

/// The item groups, for the Filter by Group link.
pub fn item_groups(fyo: &Fyo) -> Result<Vec<String>, Error> {
    names(fyo, "ItemGroup", QueryFilter::new())
}

/// Whether a sale is a return.
pub fn is_return_sale(sale: &Doc) -> bool {
    is_return(sale)
}

/// `disablePayButton`, negated: a sale can be paid, or saved, once it has items and a
/// customer.
pub fn has_items_and_customer(sale: &Doc) -> bool {
    !sale.table("items").is_empty() && text(sale, "party").is_some_and(|p| !p.is_empty())
}

/// `showValidationToast`: what is missing before `action` (saving, payment, applying
/// coupon), or `None` if nothing is.
pub fn missing_before(sale: &Doc, action: &str) -> Option<String> {
    if sale.table("items").is_empty() {
        Some(format!("Please add items before {action}"))
    } else if text(sale, "party").is_none_or(str::is_empty) {
        Some(format!("Please select a customer before {action}"))
    } else {
        None
    }
}

fn row(sale: &Doc, idx: usize) -> Result<&Doc, Error> {
    sale.table("items")
        .get(idx)
        .ok_or_else(|| Error::NotFound(format!("Row {} not found", idx + 1)))
}

/// `validateQty` for a row of the sale: its batch is chosen if it has batches, and for a
/// tracked item, the stock at the POS location covers it.
pub fn validate_qty(fyo: &Fyo, sale: &Doc, idx: usize, qty_map: &ItemQtyMap) -> Result<(), Error> {
    let row = row(sale, idx)?;
    let item = text(row, "item").unwrap_or_default();
    let batch = text(row, "batch").filter(|b| !b.is_empty());
    if is_truthy(fyo.get_value("Item", Some(item), "hasBatch").as_ref()) && batch.is_none() {
        return validation("Please select a batch first");
    }
    if !item_is_tracked(fyo, item) {
        return Ok(());
    }
    let Some(stock) = qty_map.get(item).filter(|stock| stock.available != 0.0) else {
        return validation(format!("Item {item} has Zero Quantity"));
    };
    let quantity = number(row, "quantity").unwrap_or(0.0);
    match batch {
        Some(batch) => {
            let in_batch = stock.batches.get(batch).copied().unwrap_or(0.0);
            if in_batch < quantity {
                return validation(format!(
                    "Item {item} only has {} Quantity in batch {batch}",
                    js_number_string(in_batch)
                ));
            }
        }
        None if stock.available < quantity => {
            return validation(format!(
                "Item {item} only has {} Quantity",
                js_number_string(stock.available)
            ));
        }
        None => {}
    }
    Ok(())
}

/// Sets a row's quantity, checks it, and puts it back if refused.
fn set_checked_quantity(
    fyo: &mut Fyo,
    sale: &mut Doc,
    idx: usize,
    quantity: f64,
    qty_map: &ItemQtyMap,
) -> Result<(), Error> {
    let previous = number(row(sale, idx)?, "quantity").unwrap_or(1.0);
    if !is_return(sale) && quantity <= 0.0 {
        return validation("Quantity must be greater than zero.");
    }
    sale.set_child(fyo, "items", idx, "quantity", DocValue::Number(quantity))?;
    if let Err(e) = validate_qty(fyo, sale, idx, qty_map) {
        sale.set_child(fyo, "items", idx, "quantity", DocValue::Number(previous))?;
        return Err(e);
    }
    Ok(())
}

/// SelectedItemRow's `setQuantity`: the row's quantity, checked, keeping the discount the
/// row had.
pub fn set_row_quantity(
    fyo: &mut Fyo,
    sale: &mut Doc,
    idx: usize,
    quantity: f64,
    qty_map: &ItemQtyMap,
) -> Result<(), Error> {
    let row = row(sale, idx)?;
    let manual = is_truthy(row.get("setItemDiscountAmount"));
    let percent = number(row, "itemDiscountPercent").unwrap_or(0.0);
    let amount = money(row, "itemDiscountAmount");
    let free = is_truthy(row.get("isFreeItem"));

    set_checked_quantity(fyo, sale, idx, quantity, qty_map)?;
    if free {
        return Ok(());
    }
    if manual {
        sale.set_child(
            fyo,
            "items",
            idx,
            "setItemDiscountAmount",
            DocValue::Bool(true),
        )?;
        if let Some(amount) = amount {
            sale.set_child(
                fyo,
                "items",
                idx,
                "itemDiscountAmount",
                DocValue::Money(amount),
            )?;
        }
    } else if percent != 0.0 {
        sale.set_child(
            fyo,
            "items",
            idx,
            "setItemDiscountAmount",
            DocValue::Bool(false),
        )?;
        sale.set_child(
            fyo,
            "items",
            idx,
            "itemDiscountPercent",
            DocValue::Number(percent),
        )?;
    }
    Ok(())
}

/// `adjustQuantity`, the chevrons by a row's quantity: one more or one less, never zero.
pub fn adjust_row_quantity(
    fyo: &mut Fyo,
    sale: &mut Doc,
    idx: usize,
    by: f64,
    qty_map: &ItemQtyMap,
) -> Result<(), Error> {
    let quantity = number(row(sale, idx)?, "quantity").unwrap_or(1.0) + by;
    if quantity == 0.0 {
        return Ok(());
    }
    set_row_quantity(fyo, sale, idx, quantity, qty_map)
}

/// `onQuickQtyKeyUp`: holding Q and typing digits sets the quantity of the row last
/// clicked, or of the last row that isn't free.
pub fn quick_quantity(
    fyo: &mut Fyo,
    sale: &mut Doc,
    selected: Option<usize>,
    quantity: f64,
    qty_map: &ItemQtyMap,
) -> Result<(), Error> {
    let rows = sale.table("items");
    let idx = selected.filter(|idx| *idx < rows.len()).or_else(|| {
        rows.iter()
            .rposition(|row| !is_truthy(row.get("isFreeItem")))
    });
    let Some(idx) = idx else {
        return Ok(());
    };
    set_checked_quantity(fyo, sale, idx, quantity, qty_map)
}

/// SelectedItemRow's `setRate`.
pub fn set_row_rate(fyo: &mut Fyo, sale: &mut Doc, idx: usize, rate: Money) -> Result<(), Error> {
    row(sale, idx)?;
    sale.set_child(fyo, "items", idx, "rate", DocValue::Money(rate))?;
    sale.run_formulas(fyo)?;
    Ok(())
}

/// A row's discount, as an amount off its rate or a percent.
#[derive(Debug, Clone)]
pub enum Discount {
    Amount(Money),
    Percent(f64),
}

/// SelectedItemRow's `setItemDiscount`.
pub fn set_row_discount(
    fyo: &mut Fyo,
    sale: &mut Doc,
    idx: usize,
    discount: Discount,
) -> Result<(), Error> {
    row(sale, idx)?;
    match discount {
        Discount::Percent(percent) => {
            sale.set_child(
                fyo,
                "items",
                idx,
                "setItemDiscountAmount",
                DocValue::Bool(false),
            )?;
            sale.set_child(
                fyo,
                "items",
                idx,
                "itemDiscountPercent",
                DocValue::Number(percent),
            )?;
        }
        Discount::Amount(amount) => {
            sale.set_child(
                fyo,
                "items",
                idx,
                "setItemDiscountAmount",
                DocValue::Bool(true),
            )?;
            sale.set_child(
                fyo,
                "items",
                idx,
                "itemDiscountAmount",
                DocValue::Money(amount),
            )?;
        }
    }
    Ok(())
}

/// `isDiscountsReadOnly`: a free row's discounts, those the till may not edit, and one
/// kind while the other is set, are read only.
pub fn discount_read_only(fyo: &Fyo, row: &Doc, percent: bool) -> bool {
    let other_set = if percent {
        money(row, "itemDiscountAmount").is_some_and(|m| !m.is_zero())
    } else {
        number(row, "itemDiscountPercent").unwrap_or(0.0) > 0.0
    };
    is_truthy(row.get("isFreeItem")) || !can_edit_discount(fyo) || other_set
}

/// `isRateReadOnly`.
pub fn rate_read_only(fyo: &Fyo, row: &Doc) -> bool {
    is_truthy(row.get("isFreeItem")) || !can_change_rate(fyo)
}

/// SelectedItemRow's `setBatch`.
pub fn set_row_batch(fyo: &mut Fyo, sale: &mut Doc, idx: usize, batch: &str) -> Result<(), Error> {
    row(sale, idx)?;
    let value = if batch.is_empty() {
        DocValue::Null
    } else {
        DocValue::Text(batch.to_owned())
    };
    sale.set_child(fyo, "items", idx, "batch", value)?;
    Ok(())
}

/// `getAvailableQtyInBatch`: the stock of an item's batch, at every location.
pub fn batch_quantity(fyo: &Fyo, item: &str, batch: &str) -> Result<f64, Error> {
    let conn = fyo.db.core().connection();
    Ok(conn.query_row(
        "select coalesce(sum(cast(`quantity` as real)), 0) from `StockLedgerEntry`
         where `item` = ?1 and `batch` = ?2",
        params![item, batch],
        |row| row.get(0),
    )?)
}

/// The batches of an item, for a row's Batch link.
pub fn batches(fyo: &Fyo, item: &str) -> Result<Vec<String>, Error> {
    names(
        fyo,
        "Batch",
        QueryFilter::from([("item".to_owned(), Filter::from(item))]),
    )
}

/// `validateSerialNumberCount`: one serial number a line for each of the row's quantity.
pub fn validate_serial_number_count(
    serial_numbers: &str,
    quantity: f64,
    item: &str,
) -> Result<(), Error> {
    let count = if serial_numbers.is_empty() {
        0
    } else {
        serial_numbers.split('\n').count()
    };
    if quantity.abs() != count as f64 {
        return validation(format!(
            "Need {} Serial Numbers for Item {item}. You have provided {count}",
            js_number_string(quantity)
        ));
    }
    Ok(())
}

/// A field of a row that the Modern UI's keypad (KeyboardModal) edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeypadField {
    Quantity,
    TransferQuantity,
    Rate,
    ItemDiscountAmount,
    ItemDiscountPercent,
}

impl KeypadField {
    pub fn fieldname(self) -> &'static str {
        match self {
            Self::Quantity => "quantity",
            Self::TransferQuantity => "transferQuantity",
            Self::Rate => "rate",
            Self::ItemDiscountAmount => "itemDiscountAmount",
            Self::ItemDiscountPercent => "itemDiscountPercent",
        }
    }

    pub fn is_currency(self) -> bool {
        matches!(self, Self::Rate | Self::ItemDiscountAmount)
    }
}

/// `updateSelectedValue`: the keypad starts empty for an amount, else at the row's value.
pub fn keypad_start(row: &Doc, field: KeypadField) -> String {
    if field.is_currency() {
        return String::new();
    }
    number(row, field.fieldname())
        .map(js_number_string)
        .unwrap_or_default()
}

/// A key of the keypad: a digit or `.` adds itself (in place of a lone 0), `-` and `+`
/// make the value negative or positive, `Del` takes off the last character and `Clear`
/// empties it.
pub fn keypad_press(value: &str, key: &str) -> String {
    match key {
        "-" if value.starts_with('-') => value.to_owned(),
        "-" => format!("-{value}"),
        "+" => value.strip_prefix('-').unwrap_or(value).to_owned(),
        "Del" => {
            let mut value = value.to_owned();
            value.pop();
            value
        }
        "Clear" => String::new(),
        key if value == "0" => key.to_owned(),
        key => format!("{value}{key}"),
    }
}

/// `Number(text)`: empty is 0.
fn js_number(text: &str) -> Result<f64, Error> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(0.0);
    }
    text.parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .map_or_else(|| validation(format!("{text} is not a number")), Ok)
}

/// `saveSelectedItem`: puts the keypad's value on the row.
pub fn keypad_save(
    fyo: &mut Fyo,
    sale: &mut Doc,
    idx: usize,
    field: KeypadField,
    value: &str,
    qty_map: &ItemQtyMap,
) -> Result<(), Error> {
    row(sale, idx)?;
    let n = js_number(value)?;
    let set = |fyo: &mut Fyo, sale: &mut Doc, fieldname: &str, value: DocValue| {
        sale.set_child(fyo, "items", idx, fieldname, value)
            .map(|_| ())
    };
    match field {
        KeypadField::Rate => {
            let rate = fyo.pesa().from_f64(n)?;
            set(fyo, sale, "rate", DocValue::Money(rate))?;
        }
        KeypadField::ItemDiscountAmount => {
            let grand_total = money(sale, "grandTotal").unwrap_or_else(|| fyo.pesa().zero());
            if grand_total.lte(n)? {
                let zero = fyo.pesa().zero();
                set(fyo, sale, "itemDiscountAmount", DocValue::Money(zero))?;
                let formatter = Formatter::new(fyo)?;
                return validation(format!(
                    "Discount Amount ({}) cannot be greated than Amount ({}).",
                    formatter.currency(&fyo.pesa().from_f64(n)?),
                    formatter.currency(&grand_total)
                ));
            }
            set(fyo, sale, "setItemDiscountAmount", DocValue::Bool(true))?;
            let amount = fyo.pesa().from_f64(n)?;
            set(fyo, sale, "itemDiscountAmount", DocValue::Money(amount))?;
        }
        KeypadField::ItemDiscountPercent => {
            if n > 100.0 {
                set(fyo, sale, "itemDiscountPercent", DocValue::Number(0.0))?;
                return validation(format!(
                    "Discount Percent ({}) cannot be greater than 100.",
                    value.trim()
                ));
            }
            set(fyo, sale, "setItemDiscountAmount", DocValue::Bool(false))?;
            set(fyo, sale, "itemDiscountPercent", DocValue::Number(n))?;
        }
        KeypadField::Quantity => set_checked_quantity(fyo, sale, idx, n, qty_map)?,
        KeypadField::TransferQuantity => {
            set(fyo, sale, "transferQuantity", DocValue::Number(n))?;
        }
    }
    sale.run_formulas(fyo)?;
    Ok(())
}

/// The coupon codes that can be applied: the enabled ones.
pub fn coupon_codes(fyo: &Fyo) -> Result<Vec<String>, Error> {
    names(
        fyo,
        "CouponCode",
        QueryFilter::from([("isEnabled".to_owned(), Filter::from(true))]),
    )
}

/// CouponCodeModal's `updateCouponCode`: checks the coupon, then adds it to the sale,
/// whose pricing rules then apply.
pub fn apply_coupon(fyo: &mut Fyo, sale: &mut Doc, code: &str) -> Result<(), Error> {
    validate_coupon_code(code, sale, fyo)?;
    let row = DocValueMap::from([("coupons".to_owned(), DocValue::Text(code.to_owned()))]);
    sale.append(fyo, "coupons", row)?;
    Ok(())
}

/// The items a pricing rule applies to on the sale.
fn ruled_items(sale: &Doc) -> Vec<String> {
    sale.table("pricingRuleDetail")
        .iter()
        .filter_map(|row| text(row, "referenceItem").map(str::to_owned))
        .collect()
}

/// CouponCodeModal's `removeAppliedCoupon`: clears the rows' discounts and takes the
/// coupon off; the pricing rules that still apply give theirs back, and a row whose rule
/// no longer applies takes its item's rate again.
pub fn remove_coupon(fyo: &mut Fyo, sale: &mut Doc, idx: usize) -> Result<(), Error> {
    let before = ruled_items(sale);
    let zero = fyo.pesa().zero();
    for row in 0..sale.table("items").len() {
        sale.set_child(
            fyo,
            "items",
            row,
            "itemDiscountAmount",
            DocValue::Money(zero.clone()),
        )?;
        sale.set_child(
            fyo,
            "items",
            row,
            "itemDiscountPercent",
            DocValue::Number(0.0),
        )?;
        sale.set_child(
            fyo,
            "items",
            row,
            "setItemDiscountAmount",
            DocValue::Bool(false),
        )?;
    }
    sale.remove(fyo, "coupons", idx)?;

    let after = ruled_items(sale);
    let dropped: Vec<usize> = sale
        .table("items")
        .iter()
        .enumerate()
        .filter(|(_, row)| {
            !is_truthy(row.get("isFreeItem"))
                && text(row, "item").is_some_and(|item| {
                    before.iter().any(|b| b == item) && !after.iter().any(|a| a == item)
                })
        })
        .map(|(idx, _)| idx)
        .collect();
    for row in dropped {
        let item = text(&sale.table("items")[row], "item")
            .unwrap_or_default()
            .to_owned();
        let listed = match text(sale, "priceList").filter(|p| !p.is_empty()) {
            Some(price_list) => price_list_rate(fyo, price_list, &sale.table("items")[row])?,
            None => None,
        };
        let rate = listed.or_else(|| match fyo.get_value("Item", Some(&item), "rate") {
            Some(DocValue::Money(rate)) => Some(rate),
            _ => None,
        });
        if let Some(rate) = rate {
            sale.set_child(fyo, "items", row, "rate", DocValue::Money(rate))?;
        }
    }
    Ok(())
}

/// The price lists a sale can be on: enabled, for sales.
pub fn price_lists(fyo: &Fyo) -> Result<Vec<String>, Error> {
    names(
        fyo,
        "PriceList",
        QueryFilter::from([
            ("isEnabled".to_owned(), Filter::from(true)),
            ("isSales".to_owned(), Filter::from(true)),
        ]),
    )
}

/// PriceListModal's `applyPriceList` and `removePriceList`: the rows take the list's
/// rates, or the items' own.
pub fn set_price_list(
    fyo: &mut Fyo,
    sale: &mut Doc,
    price_list: Option<&str>,
) -> Result<(), Error> {
    let value = match price_list.filter(|p| !p.is_empty()) {
        Some(price_list) => DocValue::Text(price_list.to_owned()),
        None => DocValue::Null,
    };
    sale.set(fyo, "priceList", value)?;
    Ok(())
}

/// `getItemRateFromPriceList`: the price list's rate for a row's item, in the row's
/// transfer unit if it has one, else in its unit.
pub(crate) fn price_list_rate(
    fyo: &Fyo,
    price_list: &str,
    row: &Doc,
) -> Result<Option<Money>, Error> {
    let Some(item) = text(row, "item").filter(|i| !i.is_empty()) else {
        return Ok(None);
    };
    let price_list = fyo.db.get_doc("PriceList", price_list)?;
    let rows = match price_list.get("priceListItem") {
        Some(DocValue::Table(rows)) => rows.as_slice(),
        _ => &[],
    };
    let unit_of = |row: &DocValueMap| match row.get("unit") {
        Some(DocValue::Text(unit)) => Some(unit.clone()),
        _ => None,
    };
    let wanted = text(row, "transferUnit")
        .filter(|u| !u.is_empty())
        .or_else(|| text(row, "unit").filter(|u| !u.is_empty()));
    let found = rows.iter().find(|pli| {
        matches!(pli.get("item"), Some(DocValue::Text(i)) if i == item)
            && wanted.is_none_or(|unit| unit_of(pli).as_deref() == Some(unit))
    });
    Ok(match found.and_then(|row| row.get("rate")) {
        Some(DocValue::Money(rate)) => Some(rate.clone()),
        _ => None,
    })
}

/// A customer's loyalty program and points, as `setCustomer` reads them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Loyalty {
    pub program: Option<String>,
    pub points: f64,
}

pub fn customer_loyalty(fyo: &Fyo, party: &str) -> Loyalty {
    if party.is_empty() {
        return Loyalty::default();
    }
    Loyalty {
        program: match fyo.get_value("Party", Some(party), "loyaltyProgram") {
            Some(DocValue::Text(program)) if !program.is_empty() => Some(program),
            _ => None,
        },
        points: match fyo.get_value("Party", Some(party), "loyaltyPoints") {
            Some(DocValue::Number(points)) => points,
            _ => 0.0,
        },
    }
}

/// LoyaltyProgramModal's `updateLoyaltyPoints` and POS.vue's `setLoyaltyPoints`: redeems
/// `points` of the `available` points of the sale's customer, if the customer is in a
/// program.
pub fn redeem_loyalty_points(
    fyo: &mut Fyo,
    sale: &mut Doc,
    points: f64,
    available: f64,
) -> Result<(), Error> {
    let party = text(sale, "party").unwrap_or_default().to_owned();
    let Some(program) = customer_loyalty(fyo, &party).program else {
        return Ok(());
    };
    if available < points {
        return validation(format!(
            "{party} only has {} points",
            js_number_string(available)
        ));
    }
    let factor = match fyo.get_value("LoyaltyProgram", Some(&program), "conversionFactor") {
        Some(DocValue::Number(factor)) => factor,
        _ => 0.0,
    };
    let base_grand_total = money(sale, "baseGrandTotal").unwrap_or_else(|| fyo.pesa().zero());
    if base_grand_total.lt(points * factor)? {
        return validation(format!(
            "no need {} points to purchase this item",
            js_number_string(points)
        ));
    }
    if points < 0.0 {
        return validation("Points must be greater than 0");
    }
    // As Books sets them: the points first, unchecked, then redeemed.
    sale.set_value("loyaltyPoints", DocValue::Number(points));
    sale.set(fyo, "redeemLoyaltyPoints", DocValue::Bool(true))?;
    sale.run_formulas(fyo)?;
    Ok(())
}

/// The loyalty modal's Cancel: no points redeemed.
pub fn cancel_loyalty_points(fyo: &mut Fyo, sale: &mut Doc) -> Result<(), Error> {
    sale.set(fyo, "redeemLoyaltyPoints", DocValue::Bool(false))?;
    sale.set(fyo, "loyaltyPoints", DocValue::Number(0.0))?;
    sale.run_formulas(fyo)?;
    Ok(())
}

/// What the Item Enquiry modal takes down.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ItemEnquiry {
    pub item: String,
    pub description: String,
    pub customer: String,
    pub contact: String,
    pub similar_product: String,
}

/// `updateCustomerContact`: the customer's phone.
pub fn party_phone(fyo: &Fyo, party: &str) -> String {
    match fyo.get_value("Party", Some(party), "phone") {
        Some(DocValue::Text(phone)) => phone,
        _ => String::new(),
    }
}

/// ItemEnquiryModal's `submitForm`: saves the enquiry, and gives its name.
pub fn submit_item_enquiry(fyo: &mut Fyo, enquiry: &ItemEnquiry) -> Result<String, Error> {
    let values: DocValueMap = [
        ("item", &enquiry.item),
        ("description", &enquiry.description),
        ("customer", &enquiry.customer),
        ("contact", &enquiry.contact),
        ("similarProduct", &enquiry.similar_product),
    ]
    .into_iter()
    .filter(|(_, value)| !value.trim().is_empty())
    .map(|(fieldname, value)| {
        (
            fieldname.to_owned(),
            DocValue::Text(value.trim().to_owned()),
        )
    })
    .collect();
    let mut doc = fyo.get_new_doc("ItemEnquiry", values)?;
    doc.sync(fyo)?;
    Ok(doc.name().unwrap_or_default().to_owned())
}

/// `setReturnedInvoices`: the submitted POS sales that aren't returns and aren't fully
/// returned.
pub fn returnable_sales(fyo: &Fyo) -> Result<Vec<DocValueMap>, Error> {
    let options = GetAllOptions {
        fields: Some(
            [
                "name",
                "party",
                "date",
                "grandTotal",
                "returnAgainst",
                "isFullyReturned",
            ]
            .map(str::to_owned)
            .to_vec(),
        ),
        filters: QueryFilter::from([
            ("isPOS".to_owned(), Filter::from(true)),
            ("submitted".to_owned(), Filter::from(true)),
            ("cancelled".to_owned(), Filter::from(false)),
        ]),
        ..GetAllOptions::default()
    };
    let mut sales = fyo.db.get_all_docs("SalesInvoice", &options)?;
    sales.retain(|row| {
        !is_truthy(row.get("isFullyReturned"))
            && !matches!(row.get("returnAgainst"), Some(DocValue::Text(r)) if !r.is_empty())
    });
    Ok(sales)
}

/// `selectedReturnInvoice`: a new return of what is left of a sale, for the till to pay
/// back.
pub fn return_sale(fyo: &mut Fyo, name: &str) -> Result<Option<Doc>, Error> {
    let sale = fyo.get_doc("SalesInvoice", Some(name))?;
    get_return_doc(&sale, fyo)
}
