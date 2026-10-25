// Port of src/utils/pos.ts @ v0.36.0, `getItemQtyMap` and `getItemVisibility` in
// models/helpers.ts, `getPOSTransactedAmount` in backend/database/bespoke.ts, and what
// src/pages/POS/ (POS.vue, OpenPOSShiftModal.vue, ClosePOSShiftModal.vue and
// PaymentModal.vue) does besides drawing the screen.
//
// Fixed here (docs/BOOKS-BUGS.md):
// - Closing a shift posts the counter's shortage or excess between the counter cash
//   account and the write-off account. Books posts it between Cash and the write-off
//   account, the wrong way round, and leaves it on the counter.
// - The stock check of a sale goes on past an item that isn't tracked; Books stops at
//   the first one and checks none after it.
// - Item discounts count any percent above zero; Books leaves out 1% and less.
// - What a shift took in counts what each payment put against its POS invoices, once,
//   and of submitted payments only. Books counts a payment's whole amount once for each
//   invoice it pays, and counts draft and cancelled payments.
// - A negative closing amount is refused; Books' check swallows its own error.
// - Cash tendered above the total is paid as the total and the rest is change. Books
//   makes a payment of all that was tendered, which the payment refuses as more than
//   what is outstanding.

use std::collections::HashMap;

use indexmap::IndexMap;
use jiff::Timestamp;
use latte_db::pesa::Money;
use latte_db::{DocValue, DocValueMap, Filter, GetAllOptions, Order, QueryFilter};
use rusqlite::params;

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::helpers::is_truthy;
use crate::models::base_models::invoice::{get_payment, get_stock_transfer};
use crate::models::helpers::{money, number, single_is_truthy, single_text, single_value, text};
use crate::models::inventory::helpers::{get_item_visibility, serial_number_status};
use crate::models::inventory::point_of_sale::cash_amount;

/// What there is of an item at the POS location: `{ availableQty, [batch]: qty }`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ItemQty {
    pub available: f64,
    pub batches: IndexMap<String, f64>,
}

pub type ItemQtyMap = HashMap<String, ItemQty>;

/// A POS profile's value if POS Settings name one, else POS Settings' own, as
/// `this.posProfile?.x ?? fyo.singles.POSSettings?.x` reads it.
fn setting(fyo: &Fyo, fieldname: &str) -> Option<DocValue> {
    match single_text(fyo, "POSSettings", "posProfile") {
        Some(profile) => fyo.get_value("POSProfile", Some(&profile), fieldname),
        None => single_value(fyo, "POSSettings", fieldname),
    }
}

fn setting_text(fyo: &Fyo, fieldname: &str) -> Option<String> {
    match setting(fyo, fieldname) {
        Some(DocValue::Text(text)) if !text.is_empty() => Some(text),
        _ => None,
    }
}

/// The POS inventory location: the profile's, or POS Settings'.
pub fn location(fyo: &Fyo) -> Option<String> {
    setting_text(fyo, "inventory")
}

/// `getItemQtyMap`: the stock of each item and batch at the POS location.
pub fn item_qty_map(fyo: &Fyo) -> Result<ItemQtyMap, Error> {
    let location = location(fyo);
    let conn = fyo.db.core().connection();
    let mut statement = conn.prepare(
        "select `item`, `batch`, sum(cast(`quantity` as real)) from `StockLedgerEntry`
         where ?1 is null or `location` = ?1
         group by `item`, `batch` order by `item`, `batch`",
    )?;
    let rows = statement
        .query_map(params![location], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<f64>>(2)?.unwrap_or_default(),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut map = ItemQtyMap::new();
    for (item, batch, quantity) in rows {
        let entry = map.entry(item).or_default();
        if let Some(batch) = batch.filter(|b| !b.is_empty()) {
            entry.batches.insert(batch, quantity);
        }
        entry.available += quantity;
    }
    Ok(map)
}

/// `validateIsPosSettingsSet`.
pub fn validate_settings(fyo: &Fyo) -> Result<(), Error> {
    let checks = [
        (
            "inventory",
            "POS Inventory is not set. Please set it on POS Settings",
        ),
        (
            "cashAccount",
            "POS Counter Cash Account is not set. Please set it on POS Settings",
        ),
        (
            "writeOffAccount",
            "POS Write Off Account is not set. Please set it on POS Settings",
        ),
    ];
    for (fieldname, message) in checks {
        if single_text(fyo, "POSSettings", fieldname).is_none() {
            return Err(Error::Validation(message.to_owned()));
        }
    }
    Ok(())
}

/// `setDefaultCustomer`: the profile's customer, or the one in Defaults.
pub fn default_customer(fyo: &Fyo) -> Option<String> {
    if let Some(profile) = single_text(fyo, "POSSettings", "posProfile")
        && let Some(DocValue::Text(customer)) =
            fyo.get_value("POSProfile", Some(&profile), "posCustomer")
        && !customer.is_empty()
    {
        return Some(customer);
    }
    single_text(fyo, "Defaults", "posCustomer")
}

/// Whether the point of sale sells only tracked items.
pub fn sells_inventory_items(fyo: &Fyo) -> bool {
    get_item_visibility(fyo).as_deref() == Some("Inventory Items")
}

/// An item on the POS screen.
#[derive(Debug, Clone, PartialEq)]
pub struct PosItem {
    pub name: String,
    pub rate: Option<Money>,
    pub unit: Option<String>,
    pub available: f64,
    pub has_batch: bool,
    pub has_serial_number: bool,
    /// The item's picture, as Books keeps it: a data URL.
    pub image: Option<String>,
    pub item_group: Option<String>,
}

/// `setItems`: the items the point of sale sells, tracked ones or the others as its item
/// visibility says, of one item group if given.
pub fn items(
    fyo: &Fyo,
    qty_map: &ItemQtyMap,
    item_group: Option<&str>,
) -> Result<Vec<PosItem>, Error> {
    let inventory_items = sells_inventory_items(fyo);
    let hide_unavailable = is_truthy(setting(fyo, "hideUnavailableItems").as_ref());
    let mut filters = QueryFilter::from([("trackItem".to_owned(), Filter::from(inventory_items))]);
    if let Some(group) = item_group.filter(|g| !g.is_empty()) {
        filters.insert("itemGroup".to_owned(), Filter::from(group));
    }
    let options = GetAllOptions {
        fields: Some(
            [
                "name",
                "rate",
                "unit",
                "hasBatch",
                "hasSerialNumber",
                "image",
                "itemGroup",
            ]
            .map(str::to_owned)
            .to_vec(),
        ),
        filters,
        order_by: Some(vec!["name".to_owned()]),
        order: Some(Order::Asc),
        ..GetAllOptions::default()
    };

    let mut items = Vec::new();
    for row in fyo.db.get_all_docs("Item", &options)? {
        let Some(DocValue::Text(name)) = row.get("name") else {
            continue;
        };
        let available = qty_map.get(name).map_or(0.0, |qty| qty.available);
        if hide_unavailable && inventory_items && available <= 0.0 {
            continue;
        }
        items.push(PosItem {
            name: name.clone(),
            rate: match row.get("rate") {
                Some(DocValue::Money(rate)) => Some(rate.clone()),
                _ => None,
            },
            unit: match row.get("unit") {
                Some(DocValue::Text(unit)) => Some(unit.clone()),
                _ => None,
            },
            available,
            has_batch: is_truthy(row.get("hasBatch")),
            has_serial_number: is_truthy(row.get("hasSerialNumber")),
            image: match row.get("image") {
                Some(DocValue::Text(image)) if !image.is_empty() => Some(image.clone()),
                _ => None,
            },
            item_group: match row.get("itemGroup") {
                Some(DocValue::Text(group)) if !group.is_empty() => Some(group.clone()),
                _ => None,
            },
        });
    }
    Ok(items)
}

/// `setSinvDoc`: a new POS sale, to the default customer if there is one.
pub fn new_sale(fyo: &Fyo, party: Option<&str>) -> Result<Doc, Error> {
    let mut values = DocValueMap::from([("isPOS".to_owned(), DocValue::Bool(true))]);
    if let Some(account) = single_text(fyo, "POSSettings", "defaultAccount") {
        values.insert("account".to_owned(), DocValue::Text(account));
    }
    if let Some(party) = party.map(str::to_owned).or_else(|| default_customer(fyo)) {
        values.insert("party".to_owned(), DocValue::Text(party));
    }
    fyo.get_new_doc("SalesInvoice", values)
}

/// A whole number of POS Settings, 0 if unset.
fn settings_int(fyo: &Fyo, fieldname: &str) -> usize {
    match single_value(fyo, "POSSettings", fieldname) {
        Some(DocValue::Number(n)) if n > 0.0 => n as usize,
        _ => 0,
    }
}

/// `parseInt(text, 10)`: the leading digits.
fn parse_int(text: &str) -> Option<f64> {
    let digits: String = text
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// `handleItemSearch` on Enter: the item a search names, and how many of it to add.
///
/// With POS Settings' Weight Enabled Barcode, a search as long as its check digits (as
/// written), item code digits and weight digits is a weighed item's barcode: the item
/// code (or barcode) is followed by its weight, in grams for an item sold by the kg.
/// Otherwise a 12-digit search is a barcode, and anything else an item's name.
pub fn find_item(fyo: &Fyo, search: &str) -> Result<Option<(String, f64)>, Error> {
    let check_digits = match settings_int(fyo, "checkDigits") {
        0 => 0,
        n => n.to_string().len(),
    };
    let item_code_digits = settings_int(fyo, "itemCodeDigits");
    let weight_digits = settings_int(fyo, "itemWeightDigits");
    let chars: Vec<char> = search.chars().collect();

    let mut weighed = None;
    if single_is_truthy(fyo, "POSSettings", "weightEnabledBarcode")
        && chars.len() == check_digits + item_code_digits + weight_digits
    {
        let code: String = chars[check_digits..check_digits + item_code_digits]
            .iter()
            .collect();
        let weight: String = chars[check_digits + item_code_digits..].iter().collect();
        // `Number('')` is 0, so an empty weight is a number too.
        if weight.trim().is_empty() || weight.trim().parse::<f64>().is_ok() {
            weighed = Some((code, weight));
        }
    }

    let options = GetAllOptions {
        fields: Some(
            ["name", "barcode", "itemCode", "unit"]
                .map(str::to_owned)
                .to_vec(),
        ),
        ..GetAllOptions::default()
    };
    let items = fyo.db.get_all_docs("Item", &options)?;
    let is = |row: &DocValueMap, fieldname: &str, value: &str| matches!(row.get(fieldname), Some(DocValue::Text(v)) if v == value);
    let mut found = match &weighed {
        Some((code, _)) => items
            .iter()
            .find(|row| is(row, "itemCode", code) || is(row, "barcode", code)),
        None if chars.len() == 12 => items.iter().find(|row| is(row, "barcode", search)),
        None => None,
    };
    if found.is_none() {
        found = items.iter().find(|row| is(row, "name", search));
    }
    let Some(row) = found else {
        return Ok(None);
    };
    let Some(DocValue::Text(name)) = row.get("name") else {
        return Ok(None);
    };

    let mut quantity = 1.0;
    if let Some(weight) = weighed
        .as_ref()
        .map(|(_, weight)| weight)
        .filter(|weight| !weight.is_empty())
        .and_then(|weight| parse_int(weight))
    {
        let by_kg =
            matches!(row.get("unit"), Some(DocValue::Text(unit)) if unit.to_lowercase() == "kg");
        quantity = if by_kg { weight / 1000.0 } else { weight };
    }
    Ok(Some((name.clone(), quantity)))
}

/// `setSavedInvoices` and `setSubmittedInvoices`: the POS sales saved but not submitted,
/// or submitted, not returns and not fully paid.
pub fn held_sales(fyo: &Fyo, submitted: bool) -> Result<Vec<DocValueMap>, Error> {
    let mut filters = QueryFilter::from([
        ("isPOS".to_owned(), Filter::from(true)),
        ("submitted".to_owned(), Filter::from(submitted)),
    ]);
    if submitted {
        filters.insert(
            "returnAgainst".to_owned(),
            Filter::from(latte_db::FilterValue::Null),
        );
    }
    let options = GetAllOptions {
        fields: Some(
            ["name", "party", "date", "grandTotal", "outstandingAmount"]
                .map(str::to_owned)
                .to_vec(),
        ),
        filters,
        ..GetAllOptions::default()
    };
    let mut sales = fyo.db.get_all_docs("SalesInvoice", &options)?;
    if submitted {
        sales.retain(
            |row| !matches!(row.get("outstandingAmount"), Some(DocValue::Money(m)) if m.is_zero()),
        );
    }
    Ok(sales)
}

/// `saveOrder`: saves the sale, unsubmitted, to take up again later.
pub fn save_sale(fyo: &mut Fyo, sale: &mut Doc) -> Result<(), Error> {
    let qty_map = item_qty_map(fyo)?;
    validate_sale(fyo, sale, &qty_map)?;
    sale.run_formulas(fyo)?;
    sale.sync(fyo)
}

fn item_is_tracked(fyo: &Fyo, item: &str) -> bool {
    is_truthy(fyo.get_value("Item", Some(item), "trackItem").as_ref())
}

/// `addItem`: adds one or more of an item to a sale, or to its row of the item if it
/// has one (of the same batch, for a batched item).
pub fn add_item(
    fyo: &mut Fyo,
    sale: &mut Doc,
    item: &PosItem,
    quantity: f64,
    batch: Option<&str>,
    qty_map: &ItemQtyMap,
) -> Result<(), Error> {
    if sale.is_submitted() {
        return Err(Error::Validation(
            "Cannot add an item to a submitted invoice.".to_owned(),
        ));
    }
    if sale
        .get("returnAgainst")
        .is_some_and(|v| is_truthy(Some(v)))
    {
        return Err(Error::Validation(
            "Unable to add an item to the return invoice.".to_owned(),
        ));
    }
    if item.has_batch && batch.is_none() {
        return Err(Error::Validation("Please select a batch first".to_owned()));
    }

    let tracked = item_is_tracked(fyo, &item.name);
    let stock = qty_map.get(&item.name);
    let available = match batch {
        Some(batch) => stock
            .and_then(|s| s.batches.get(batch))
            .copied()
            .unwrap_or(0.0),
        None => stock.map_or(0.0, |s| s.available),
    };
    if tracked && available <= 0.0 {
        return Err(Error::Validation(format!(
            "Item {} is out of stock (quantity is zero)",
            item.name
        )));
    }

    let existing = sale.table("items").iter().position(|row| {
        text(row, "item") == Some(&item.name)
            && !is_truthy(row.get("isFreeItem"))
            && text(row, "batch") == batch
    });
    if let Some(idx) = existing {
        let current = number(&sale.table("items")[idx], "quantity").unwrap_or(0.0);
        if tracked && current + quantity > available {
            return Err(Error::Validation(
                "Cannot add more than the available quantity".to_owned(),
            ));
        }
        if sale.get("priceList").is_none_or(|v| !is_truthy(Some(v)))
            && let Some(rate) = &item.rate
        {
            sale.set_child(fyo, "items", idx, "rate", DocValue::Money(rate.clone()))?;
        }
        sale.set_child(
            fyo,
            "items",
            idx,
            "quantity",
            DocValue::Number(current + quantity),
        )?;
        return Ok(());
    }

    let mut row = DocValueMap::from([
        ("item".to_owned(), DocValue::Text(item.name.clone())),
        ("quantity".to_owned(), DocValue::Number(quantity)),
    ]);
    if let Some(rate) = &item.rate {
        row.insert("rate".to_owned(), DocValue::Money(rate.clone()));
    }
    if let Some(batch) = batch {
        row.insert("batch".to_owned(), DocValue::Text(batch.to_owned()));
    }
    sale.append(fyo, "items", row)?;

    // A sale on a price list takes the list's rate for the item.
    if let Some(price_list) = text(sale, "priceList").filter(|p| !p.is_empty()) {
        let price_list = price_list.to_owned();
        let idx = sale.table("items").len() - 1;
        if let Some(rate) = till::price_list_rate(fyo, &price_list, &sale.table("items")[idx])? {
            sale.set_child(fyo, "items", idx, "rate", DocValue::Money(rate))?;
        }
    }
    Ok(())
}

/// `validateSinv`: every tracked item is in stock, in the quantity sold.
pub fn validate_sale(fyo: &Fyo, sale: &Doc, qty_map: &ItemQtyMap) -> Result<(), Error> {
    let is_return = sale
        .get("returnAgainst")
        .is_some_and(|v| is_truthy(Some(v)));
    for row in sale.table("items") {
        let Some(item) = text(row, "item") else {
            continue;
        };
        if !item_is_tracked(fyo, item) {
            continue;
        }
        let quantity = number(row, "quantity").unwrap_or(0.0);
        if quantity == 0.0 || (quantity < 1.0 && !is_return) {
            return Err(Error::Validation(format!(
                "Invalid Quantity for Item {item}"
            )));
        }
        let Some(stock) = qty_map.get(item) else {
            return Err(Error::Validation(format!("Item {item} not in Stock")));
        };
        if quantity > stock.available {
            return Err(Error::Validation(format!(
                "Insufficient Quantity. Item {item} has only {} quantities available. you \
                 selected {quantity}",
                stock.available
            )));
        }
    }
    Ok(())
}

/// `validateShipment`: the serial numbers given are all active.
pub fn validate_serial_numbers(
    fyo: &Fyo,
    serial_numbers: &HashMap<String, String>,
) -> Result<(), Error> {
    for numbers in serial_numbers.values() {
        for serial_number in numbers.split('\n').map(str::trim).filter(|s| !s.is_empty()) {
            if serial_number_status(fyo, serial_number) != "Active" {
                return Err(Error::Validation(format!(
                    "Serial Number {serial_number} status is not Active."
                )));
            }
        }
    }
    Ok(())
}

/// `getTotalQuantity`.
pub fn total_quantity(sale: &Doc) -> f64 {
    sale.table("items")
        .iter()
        .map(|row| number(row, "quantity").unwrap_or(0.0))
        .sum()
}

/// `getItemDiscounts`: the discounts given on the rows.
pub fn item_discounts(fyo: &Fyo, sale: &Doc) -> Result<Money, Error> {
    let mut discounts = fyo.pesa().zero();
    for row in sale.table("items") {
        if is_truthy(row.get("setItemDiscountAmount")) {
            if let Some(amount) = money(row, "itemDiscountAmount").filter(|a| !a.is_zero()) {
                let quantity = number(row, "quantity").unwrap_or(0.0);
                discounts = discounts.add(&amount.mul(quantity)?)?;
            }
        } else if let Some(amount) = money(row, "amount") {
            let percent = number(row, "itemDiscountPercent").unwrap_or(0.0);
            if percent > 0.0 {
                discounts = discounts.add(&amount.percent(percent)?)?;
            }
        }
    }
    Ok(discounts)
}

/// `getTotalTaxedAmount`: the sum of the sale's taxes.
pub fn total_taxed_amount(fyo: &Fyo, sale: &Doc) -> Result<Money, Error> {
    let mut total = fyo.pesa().zero();
    if sale.table("items").is_empty() {
        return Ok(total);
    }
    for row in sale.table("taxes") {
        if let Some(amount) = money(row, "amount") {
            total = total.add(&amount)?;
        }
    }
    Ok(total)
}

/// How a sale is paid.
#[derive(Debug, Clone)]
pub struct PosPayment {
    pub method: String,
    /// What was tendered. Above the total, the rest is change.
    pub amount: Money,
    pub reference_id: Option<String>,
    pub clearance_date: Option<Timestamp>,
}

/// What `createTransaction` made: the invoice, and its shipment and payment if made.
#[derive(Debug, Clone, PartialEq)]
pub struct Transaction {
    pub invoice: String,
    pub shipment: Option<String>,
    pub payment: Option<String>,
}

/// `createTransaction`: submits the sale, ships what it sold from the POS location, and
/// takes its payment if given. Serial numbers are keyed by item, one to a line.
pub fn create_transaction(
    fyo: &mut Fyo,
    sale: &mut Doc,
    serial_numbers: &HashMap<String, String>,
    payment: Option<&PosPayment>,
) -> Result<Transaction, Error> {
    // A sale submitted earlier, taken up from the saved sales, is only paid. Books tries
    // to save and submit it again, shows the error, and pays it anyway.
    if !sale.is_submitted() {
        let qty_map = item_qty_map(fyo)?;
        sale.set(fyo, "date", DocValue::Date(Timestamp::now()))?;
        validate_sale(fyo, sale, &qty_map)?;
        let is_return = sale
            .get("returnAgainst")
            .is_some_and(|v| is_truthy(Some(v)));
        if !is_return {
            validate_serial_numbers(fyo, serial_numbers)?;
        }
        sale.run_formulas(fyo)?;
        sale.sync(fyo)?;
        sale.submit(fyo)?;
    }
    let invoice = sale.name().unwrap_or_default().to_owned();

    let mut shipment = None;
    if number(sale, "stockNotTransferred").is_some_and(|n| n != 0.0) && sells_inventory_items(fyo) {
        shipment = make_stock_transfer(fyo, sale, serial_numbers)?;
    }

    let payment = match payment {
        Some(payment) => make_payment(fyo, sale, payment)?,
        None => None,
    };
    Ok(Transaction {
        invoice,
        shipment,
        payment,
    })
}

/// `makeStockTransfer`: the sale's shipment, of the tracked items, from the POS
/// location.
fn make_stock_transfer(
    fyo: &mut Fyo,
    sale: &Doc,
    serial_numbers: &HashMap<String, String>,
) -> Result<Option<String>, Error> {
    let Some(mut shipment) = get_stock_transfer(sale, fyo, false)? else {
        return Ok(None);
    };
    let location = location(fyo);
    for idx in 0..shipment.table("items").len() {
        let item = text(&shipment.table("items")[idx], "item")
            .unwrap_or_default()
            .to_owned();
        if !item_is_tracked(fyo, &item) {
            continue;
        }
        let location = location.clone().map_or(DocValue::Null, DocValue::Text);
        shipment.set_child(fyo, "items", idx, "location", location)?;
        if let Some(numbers) = serial_numbers.get(&item) {
            shipment.set_child(
                fyo,
                "items",
                idx,
                "serialNumber",
                DocValue::Text(numbers.clone()),
            )?;
        }
    }
    shipment.sync(fyo)?;
    shipment.submit(fyo)?;
    Ok(shipment.name().map(str::to_owned))
}

/// `makePayment`: pays the sale by the method given, into the POS cash account for
/// cash.
fn make_payment(fyo: &mut Fyo, sale: &Doc, payment: &PosPayment) -> Result<Option<String>, Error> {
    let Some(mut doc) = get_payment(sale, fyo)? else {
        return Ok(None);
    };
    let outstanding = money(&doc, "amount").unwrap_or_else(|| fyo.pesa().zero());
    // A return's refund is for what the return is owed, which Books' payment modal shows
    // as a negative amount.
    let tendered = if crate::models::base_models::invoice::is_return(sale) {
        payment.amount.abs()?
    } else {
        payment.amount.clone()
    };
    let amount = if tendered.gt(&outstanding)? {
        outstanding
    } else {
        tendered
    };
    doc.set(fyo, "paymentMethod", DocValue::Text(payment.method.clone()))?;
    doc.set(fyo, "amount", DocValue::Money(amount))?;
    doc.set(
        fyo,
        "referenceType",
        DocValue::Text("SalesInvoice".to_owned()),
    )?;

    let method_type = fyo.get_value("PaymentMethod", Some(&payment.method), "type");
    if method_type == Some(DocValue::Text("Cash".to_owned())) {
        // A refund is paid from the counter, which is the payment's From Account. Books
        // sets the counter as the To Account, as for a sale, which the payment refuses as
        // the same account twice.
        let fieldname = if crate::models::base_models::invoice::is_return(sale) {
            "account"
        } else {
            "paymentAccount"
        };
        if let Some(account) = single_text(fyo, "POSSettings", "cashAccount") {
            doc.set(fyo, fieldname, DocValue::Text(account))?;
        }
    } else {
        let reference = payment
            .reference_id
            .clone()
            .map_or(DocValue::Null, DocValue::Text);
        let clearance = payment
            .clearance_date
            .map_or(DocValue::Null, DocValue::Date);
        doc.set(fyo, "referenceId", reference)?;
        doc.set(fyo, "clearanceDate", clearance)?;
    }
    doc.sync(fyo)?;
    doc.submit(fyo)?;
    Ok(doc.name().map(str::to_owned))
}

/// The payment methods, by name.
pub fn payment_methods(fyo: &Fyo) -> Result<Vec<String>, Error> {
    let options = GetAllOptions {
        fields: Some(vec!["name".to_owned()]),
        order_by: Some(vec!["name".to_owned()]),
        order: Some(Order::Asc),
        ..GetAllOptions::default()
    };
    Ok(fyo
        .db
        .get_all_docs("PaymentMethod", &options)?
        .iter()
        .filter_map(|row| match row.get("name") {
            Some(DocValue::Text(name)) => Some(name.clone()),
            _ => None,
        })
        .collect())
}

/// `getPOSOpeningShiftDoc`: the last opening shift while a shift is open, else a new one
/// with the default cash denominations, none counted, and a row for each payment method.
pub fn opening_shift(fyo: &mut Fyo) -> Result<Doc, Error> {
    if single_is_truthy(fyo, "POSSettings", "isShiftOpen")
        && let Some(name) = last_opening_shift(fyo)?
    {
        return fyo.get_doc("POSOpeningShift", Some(&name));
    }

    let mut doc = fyo.get_new_doc("POSOpeningShift", DocValueMap::new())?;
    let defaults = fyo.get_doc("Defaults", None)?;
    for row in defaults.table("posCashDenominations") {
        let denomination = row.get("denomination").cloned().unwrap_or(DocValue::Null);
        let row = DocValueMap::from([
            ("denomination".to_owned(), denomination),
            ("count".to_owned(), DocValue::Number(0.0)),
        ]);
        doc.push_row(fyo, "openingCash", row)?;
    }
    for method in payment_methods(fyo)? {
        let row = DocValueMap::from([
            ("paymentMethod".to_owned(), DocValue::Text(method)),
            ("amount".to_owned(), DocValue::Money(fyo.pesa().zero())),
        ]);
        doc.push_row(fyo, "openingAmounts", row)?;
    }
    Ok(doc)
}

fn last_opening_shift(fyo: &Fyo) -> Result<Option<String>, Error> {
    let options = GetAllOptions {
        fields: Some(vec!["name".to_owned()]),
        order_by: Some(vec!["created".to_owned()]),
        order: Some(Order::Desc),
        limit: Some(1),
        ..GetAllOptions::default()
    };
    Ok(fyo
        .db
        .get_all_docs("POSOpeningShift", &options)?
        .first()
        .and_then(|row| match row.get("name") {
            Some(DocValue::Text(name)) => Some(name.clone()),
            _ => None,
        }))
}

/// `setOpeningCashAmount`: the Cash row's amount is what the counted cash adds up to.
pub fn set_opening_cash_amount(fyo: &mut Fyo, shift: &mut Doc) -> Result<(), Error> {
    let amount = cash_amount(shift, "openingCash", fyo)?;
    for idx in 0..shift.table("openingAmounts").len() {
        if text(&shift.table("openingAmounts")[idx], "paymentMethod") == Some("Cash") {
            shift.set_child(
                fyo,
                "openingAmounts",
                idx,
                "amount",
                DocValue::Money(amount.clone()),
            )?;
        }
    }
    Ok(())
}

/// A submitted journal entry of the rows given, as `(account, amount, is_debit)`.
fn transfer_entry(fyo: &mut Fyo, rows: &[(&str, &Money, bool)]) -> Result<(), Error> {
    let values = DocValueMap::from([(
        "entryType".to_owned(),
        DocValue::Text("Journal Entry".to_owned()),
    )]);
    let mut entry = fyo.get_new_doc("JournalEntry", values)?;
    for (account, amount, is_debit) in rows {
        let zero = DocValue::Money(fyo.pesa().zero());
        let amount = DocValue::Money((*amount).clone());
        let (debit, credit) = if *is_debit {
            (amount, zero)
        } else {
            (zero, amount)
        };
        let row = DocValueMap::from([
            ("account".to_owned(), DocValue::Text((*account).to_owned())),
            ("debit".to_owned(), debit),
            ("credit".to_owned(), credit),
        ]);
        entry.append(fyo, "accounts", row)?;
    }
    entry.sync(fyo)?;
    entry.submit(fyo)?;
    Ok(())
}

/// `OpenPOSShiftModal.handleSubmit`: saves the shift, marks it open, and moves the
/// opening cash from Cash to the counter.
pub fn open_shift(fyo: &mut Fyo, shift: &mut Doc) -> Result<(), Error> {
    let amount = cash_amount(shift, "openingCash", fyo)?;
    if amount.is_negative() {
        return Err(Error::Validation(
            "Opening Cash Amount can not be negative.".to_owned(),
        ));
    }
    validate_settings(fyo)?;
    set_opening_cash_amount(fyo, shift)?;
    shift.set(fyo, "openingDate", DocValue::Date(Timestamp::now()))?;
    shift.sync(fyo)?;
    set_shift_open(fyo, true)?;

    if !amount.is_zero() {
        let counter = single_text(fyo, "POSSettings", "cashAccount").unwrap_or_default();
        transfer_entry(fyo, &[(&counter, &amount, true), ("Cash", &amount, false)])?;
    }
    Ok(())
}

fn set_shift_open(fyo: &mut Fyo, open: bool) -> Result<(), Error> {
    let mut settings = fyo.get_doc("POSSettings", None)?;
    settings.set(fyo, "isShiftOpen", DocValue::Bool(open))?;
    settings.sync(fyo)
}

/// `getPOSTransactedAmount`: what submitted payments put against the POS invoices dated
/// from `from` to `to`, by payment method. Returns take away.
pub fn transacted_amounts(
    fyo: &Fyo,
    from: Timestamp,
    to: Timestamp,
) -> Result<IndexMap<String, Money>, Error> {
    let iso = |t: Timestamp| t.strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    let conn = fyo.db.core().connection();
    let mut statement = conn.prepare(
        "select p.`paymentMethod`, pf.`amount`, si.`returnAgainst`
         from `PaymentFor` pf
         join `Payment` p on p.`name` = pf.`parent`
         join `SalesInvoice` si on si.`name` = pf.`referenceName`
         where si.`isPOS` = 1 and si.`date` between ?1 and ?2
           and p.`submitted` = 1 and p.`cancelled` = 0
         order by p.`paymentMethod`",
    )?;
    let rows = statement
        .query_map(params![iso(from), iso(to)], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, Option<String>>(2)?
                    .is_some_and(|r| !r.is_empty()),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut amounts = IndexMap::<String, Money>::new();
    for (method, amount, is_return) in rows {
        let amount = fyo
            .pesa()
            .parse(if amount.is_empty() { "0" } else { &amount })?;
        let amount = if is_return {
            amount.abs()?.neg()?
        } else {
            amount
        };
        let total = amounts.entry(method).or_insert_with(|| fyo.pesa().zero());
        *total = total.add(&amount)?;
    }
    Ok(amounts)
}

/// `ClosePOSShiftModal.seedValues`: a closing shift with the opening shift's cash counts
/// and, for each payment method, its opening amount, what is expected from what the
/// shift took in, and nothing counted yet.
pub fn new_closing_shift(fyo: &mut Fyo, opening: &Doc) -> Result<Doc, Error> {
    let transacted = match opening.get("openingDate") {
        Some(DocValue::Date(from)) => transacted_amounts(fyo, *from, Timestamp::now())?,
        _ => IndexMap::new(),
    };
    let mut doc = fyo.get_new_doc("POSClosingShift", DocValueMap::new())?;
    for row in opening.table("openingCash") {
        let values = DocValueMap::from([
            (
                "denomination".to_owned(),
                row.get("denomination").cloned().unwrap_or(DocValue::Null),
            ),
            (
                "count".to_owned(),
                row.get("count").cloned().unwrap_or(DocValue::Number(0.0)),
            ),
        ]);
        doc.push_row(fyo, "closingCash", values)?;
    }
    for row in opening.table("openingAmounts") {
        let Some(method) = text(row, "paymentMethod") else {
            continue;
        };
        let opening_amount = money(row, "amount").unwrap_or_else(|| fyo.pesa().zero());
        let expected = match transacted.get(method) {
            Some(amount) => opening_amount.add(amount)?,
            None => opening_amount.clone(),
        };
        let values = DocValueMap::from([
            (
                "paymentMethod".to_owned(),
                DocValue::Text(method.to_owned()),
            ),
            ("openingAmount".to_owned(), DocValue::Money(opening_amount)),
            (
                "closingAmount".to_owned(),
                DocValue::Money(fyo.pesa().zero()),
            ),
            ("expectedAmount".to_owned(), DocValue::Money(expected)),
        ]);
        doc.push_row(fyo, "closingAmounts", values)?;
    }
    doc.run_formulas(fyo)?;
    Ok(doc)
}

/// `setClosingCashAmount`: the Cash row's closing amount is what the counted cash adds
/// up to.
pub fn set_closing_cash_amount(fyo: &mut Fyo, shift: &mut Doc) -> Result<(), Error> {
    let amount = cash_amount(shift, "closingCash", fyo)?;
    for idx in 0..shift.table("closingAmounts").len() {
        if text(&shift.table("closingAmounts")[idx], "paymentMethod") == Some("Cash") {
            shift.set_child(
                fyo,
                "closingAmounts",
                idx,
                "closingAmount",
                DocValue::Money(amount.clone()),
            )?;
        }
    }
    Ok(())
}

/// `ClosePOSShiftModal.handleSubmit`: saves the closing shift, moves the counted cash
/// from the counter to Cash, writes off what is short or over, and marks the shift
/// closed.
pub fn close_shift(fyo: &mut Fyo, shift: &mut Doc, opening: &Doc) -> Result<(), Error> {
    for row in shift.table("closingAmounts") {
        if money(row, "closingAmount").is_some_and(|a| a.is_negative()) {
            return Err(Error::Validation(format!(
                "Closing {} Amount can not be negative.",
                text(row, "paymentMethod").unwrap_or_default()
            )));
        }
    }
    validate_settings(fyo)?;
    shift.set(fyo, "closingDate", DocValue::Date(Timestamp::now()))?;
    let opening_name = opening
        .name()
        .map_or(DocValue::Null, |n| DocValue::Text(n.to_owned()));
    shift.set(fyo, "openingShift", opening_name)?;
    shift.sync(fyo)?;
    transfer_cash_and_write_off(fyo, shift)?;
    set_shift_open(fyo, false)
}

/// `transferPOSCashAndWriteOff`, fixed: the counted cash goes from the counter to Cash,
/// and the counter's shortage (or excess) is written off against it, which leaves the
/// counter with nothing.
fn transfer_cash_and_write_off(fyo: &mut Fyo, shift: &Doc) -> Result<(), Error> {
    let Some(cash) = shift
        .table("closingAmounts")
        .iter()
        .find(|row| text(row, "paymentMethod") == Some("Cash"))
    else {
        return Ok(());
    };
    let zero = fyo.pesa().zero();
    let expected = money(cash, "expectedAmount").unwrap_or_else(|| zero.clone());
    if expected.is_zero() {
        return Ok(());
    }
    let closing = money(cash, "closingAmount").unwrap_or_else(|| zero.clone());
    let difference = money(cash, "differenceAmount").unwrap_or_else(|| zero.clone());
    let counter = single_text(fyo, "POSSettings", "cashAccount").unwrap_or_default();
    let write_off = single_text(fyo, "POSSettings", "writeOffAccount").unwrap_or_default();

    let shortage = difference.abs()?;
    let mut rows = Vec::new();
    if !closing.is_zero() {
        rows.push(("Cash", &closing, true));
        rows.push((counter.as_str(), &closing, false));
    }
    if difference.is_negative() {
        rows.push((write_off.as_str(), &shortage, true));
        rows.push((counter.as_str(), &shortage, false));
    } else if !difference.is_zero() {
        rows.push((counter.as_str(), &difference, true));
        rows.push((write_off.as_str(), &difference, false));
    }
    if rows.is_empty() {
        return Ok(());
    }
    transfer_entry(fyo, &rows)
}

#[path = "pos_till.rs"]
mod till;
pub use till::*;

#[cfg(test)]
#[path = "pos_tests.rs"]
mod tests;
