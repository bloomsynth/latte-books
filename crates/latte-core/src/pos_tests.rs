// Tests of the point of sale, after models/inventory/Point of Sale/tests/
// testPointOfSale.spec.ts @ v0.36.0 and the work of the POS screen.

use std::collections::HashMap;

use latte_db::{DocValue, DocValueMap};

use super::*;
use crate::setup_tests::{TempFile, indian_company};
use crate::tests::{map, money, text};
use crate::{Doc, Fyo};

const PEN: &str = "Pen";
const CUSTOMER: &str = "Walk-in";
const COUNTER: &str = "Counter Cash";

/// A company with 10 tracked Pens at 700 in Stores, a counter cash account, and POS
/// Settings and Defaults set up for them.
fn shop() -> (TempFile, Fyo) {
    let file = TempFile::new("pos");
    let mut fyo = indian_company(&file);
    let values = map([
        ("name", text(PEN)),
        ("rate", DocValue::Number(700.0)),
        ("trackItem", DocValue::Bool(true)),
    ]);
    fyo.get_new_doc("Item", values)
        .unwrap()
        .sync(&mut fyo)
        .unwrap();
    let values = map([("name", text("Pencil")), ("rate", DocValue::Number(10.0))]);
    fyo.get_new_doc("Item", values)
        .unwrap()
        .sync(&mut fyo)
        .unwrap();
    let values = map([("name", text(CUSTOMER)), ("role", text("Customer"))]);
    fyo.get_new_doc("Party", values)
        .unwrap()
        .sync(&mut fyo)
        .unwrap();
    let values = map([
        ("name", text(COUNTER)),
        ("parentAccount", text("Cash In Hand")),
        ("accountType", text("Cash")),
    ]);
    fyo.get_new_doc("Account", values)
        .unwrap()
        .sync(&mut fyo)
        .unwrap();

    let values = map([
        ("movementType", text("MaterialReceipt")),
        ("date", text("2026-01-01")),
    ]);
    let mut receipt = fyo.get_new_doc("StockMovement", values).unwrap();
    let row = map([
        ("item", text(PEN)),
        ("toLocation", text("Stores")),
        ("rate", DocValue::Number(700.0)),
        ("quantity", DocValue::Number(10.0)),
    ]);
    receipt.append(&mut fyo, "items", row).unwrap();
    receipt.sync(&mut fyo).unwrap();
    receipt.submit(&mut fyo).unwrap();

    let mut defaults = fyo.get_doc("Defaults", None).unwrap();
    defaults
        .set(&mut fyo, "posCustomer", text(CUSTOMER))
        .unwrap();
    defaults.sync(&mut fyo).unwrap();
    (file, fyo)
}

fn set_up_pos(fyo: &mut Fyo) {
    let mut settings = fyo.get_doc("POSSettings", None).unwrap();
    for (fieldname, value) in [
        ("inventory", "Stores"),
        ("cashAccount", COUNTER),
        ("writeOffAccount", "Write Off"),
        ("defaultAccount", "Debtors"),
    ] {
        settings.set(fyo, fieldname, text(value)).unwrap();
    }
    settings.sync(fyo).unwrap();
}

fn pen(fyo: &Fyo) -> PosItem {
    let qty_map = item_qty_map(fyo).unwrap();
    items(fyo, &qty_map, None)
        .unwrap()
        .into_iter()
        .find(|item| item.name == PEN)
        .unwrap()
}

fn sell(fyo: &mut Fyo, quantity: f64, payment: Option<&PosPayment>) -> Transaction {
    let mut sale = new_sale(fyo, None).unwrap();
    let qty_map = item_qty_map(fyo).unwrap();
    let item = pen(fyo);
    for _ in 0..quantity as usize {
        add_item(fyo, &mut sale, &item, 1.0, None, &qty_map).unwrap();
    }
    create_transaction(fyo, &mut sale, &HashMap::new(), payment).unwrap()
}

fn cash(fyo: &Fyo, amount: f64) -> PosPayment {
    PosPayment {
        method: "Cash".to_owned(),
        amount: fyo.pesa().from_f64(amount).unwrap(),
        reference_id: None,
        clearance_date: None,
    }
}

/// An account's balance, debit less credit.
fn balance(fyo: &Fyo, account: &str) -> f64 {
    fyo.db
        .core()
        .connection()
        .query_row(
            "select coalesce(sum(cast(debit as real) - cast(credit as real)), 0)
             from AccountingLedgerEntry where account = ?1 and reverted = 0",
            [account],
            |row| row.get(0),
        )
        .unwrap()
}

fn closing_row<'a>(shift: &'a Doc, method: &str) -> &'a Doc {
    shift
        .table("closingAmounts")
        .iter()
        .find(|row| crate::models::helpers::text(row, "paymentMethod") == Some(method))
        .unwrap()
}

#[test]
fn the_pos_needs_its_settings() {
    let (_file, mut fyo) = shop();
    assert_eq!(
        validate_settings(&fyo).unwrap_err().to_string(),
        "POS Inventory is not set. Please set it on POS Settings"
    );
    set_up_pos(&mut fyo);
    validate_settings(&fyo).unwrap();
}

#[test]
fn a_shift_opens_sells_and_closes_with_the_counter_cleared() {
    let (_file, mut fyo) = shop();
    set_up_pos(&mut fyo);

    // Two 500s in the drawer.
    let mut opening = opening_shift(&mut fyo).unwrap();
    let methods: Vec<_> = opening
        .table("openingAmounts")
        .iter()
        .map(|row| {
            crate::models::helpers::text(row, "paymentMethod")
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(methods, ["Bank", "Cash", "Transfer"]);
    let row = map([
        ("denomination", DocValue::Number(500.0)),
        ("count", DocValue::Number(2.0)),
    ]);
    opening.append(&mut fyo, "openingCash", row).unwrap();
    open_shift(&mut fyo, &mut opening).unwrap();
    assert!(single_is_truthy(&fyo, "POSSettings", "isShiftOpen"));
    assert_eq!(balance(&fyo, COUNTER), 1000.0);

    // Two Pens for 1500 in cash: 1400 is paid, 100 is change.
    let tendered = cash(&fyo, 1500.0);
    let sale = sell(&mut fyo, 2.0, Some(&tendered));
    assert!(sale.shipment.is_some());
    assert!(sale.payment.is_some());
    let invoice = fyo.get_doc("SalesInvoice", Some(&sale.invoice)).unwrap();
    assert_eq!(invoice.get("party"), Some(&text(CUSTOMER)));
    assert_eq!(money(&invoice, "outstandingAmount"), "0.00");
    // A Pen by transfer.
    let transfer = PosPayment {
        method: "Transfer".to_owned(),
        amount: fyo.pesa().from_f64(700.0).unwrap(),
        reference_id: Some("UTR123".to_owned()),
        clearance_date: Some(jiff::Timestamp::now()),
    };
    sell(&mut fyo, 1.0, Some(&transfer));
    assert_eq!(item_qty_map(&fyo).unwrap()[PEN].available, 7.0);
    assert_eq!(balance(&fyo, COUNTER), 2400.0);

    // 2300 is counted: 100 short.
    let opening = opening_shift(&mut fyo).unwrap();
    assert!(opening.inserted());
    let mut closing = new_closing_shift(&mut fyo, &opening).unwrap();
    assert_eq!(
        money(closing_row(&closing, "Cash"), "expectedAmount"),
        "2400.00"
    );
    assert_eq!(
        money(closing_row(&closing, "Transfer"), "expectedAmount"),
        "700.00"
    );
    assert_eq!(
        money(closing_row(&closing, "Bank"), "expectedAmount"),
        "0.00"
    );
    closing
        .set_child(&mut fyo, "closingCash", 0, "count", DocValue::Number(4.0))
        .unwrap();
    let row = map([
        ("denomination", DocValue::Number(100.0)),
        ("count", DocValue::Number(3.0)),
    ]);
    closing.append(&mut fyo, "closingCash", row).unwrap();
    set_closing_cash_amount(&mut fyo, &mut closing).unwrap();
    assert_eq!(
        money(closing_row(&closing, "Cash"), "differenceAmount"),
        "-100.00"
    );

    let cash_before = balance(&fyo, "Cash");
    close_shift(&mut fyo, &mut closing, &opening).unwrap();
    assert!(!single_is_truthy(&fyo, "POSSettings", "isShiftOpen"));
    // The counter is cleared: 2300 to Cash and the 100 short written off.
    assert_eq!(balance(&fyo, COUNTER), 0.0);
    assert_eq!(balance(&fyo, "Cash") - cash_before, 2300.0);
    assert_eq!(balance(&fyo, "Write Off"), 100.0);
}

#[test]
fn cash_over_is_written_back() {
    let (_file, mut fyo) = shop();
    set_up_pos(&mut fyo);
    let mut opening = opening_shift(&mut fyo).unwrap();
    open_shift(&mut fyo, &mut opening).unwrap();
    let tendered = cash(&fyo, 700.0);
    sell(&mut fyo, 1.0, Some(&tendered));

    let opening = opening_shift(&mut fyo).unwrap();
    let mut closing = new_closing_shift(&mut fyo, &opening).unwrap();
    let row = map([
        ("denomination", DocValue::Number(750.0)),
        ("count", DocValue::Number(1.0)),
    ]);
    closing.append(&mut fyo, "closingCash", row).unwrap();
    set_closing_cash_amount(&mut fyo, &mut closing).unwrap();
    close_shift(&mut fyo, &mut closing, &opening).unwrap();
    assert_eq!(balance(&fyo, COUNTER), 0.0);
    assert_eq!(balance(&fyo, "Write Off"), -50.0);
}

#[test]
fn a_negative_closing_amount_is_refused() {
    let (_file, mut fyo) = shop();
    set_up_pos(&mut fyo);
    let mut opening = opening_shift(&mut fyo).unwrap();
    open_shift(&mut fyo, &mut opening).unwrap();
    let mut closing = new_closing_shift(&mut fyo, &opening).unwrap();
    let idx = closing
        .table("closingAmounts")
        .iter()
        .position(|row| crate::models::helpers::text(row, "paymentMethod") == Some("Bank"))
        .unwrap();
    let amount = DocValue::Money(fyo.pesa().from_f64(-5.0).unwrap());
    closing
        .set_child(&mut fyo, "closingAmounts", idx, "closingAmount", amount)
        .unwrap();
    assert_eq!(
        close_shift(&mut fyo, &mut closing, &opening)
            .unwrap_err()
            .to_string(),
        "Closing Bank Amount can not be negative."
    );
}

#[test]
fn a_payment_of_two_sales_counts_once() {
    let (_file, mut fyo) = shop();
    set_up_pos(&mut fyo);
    let start = jiff::Timestamp::now();
    let one = sell(&mut fyo, 1.0, None).invoice;
    let two = sell(&mut fyo, 2.0, None).invoice;

    let mut payment = fyo
        .get_new_doc("Payment", map([("party", text(CUSTOMER))]))
        .unwrap();
    for (idx, name) in [&one, &two].into_iter().enumerate() {
        payment.append(&mut fyo, "for", DocValueMap::new()).unwrap();
        payment
            .set_child(&mut fyo, "for", idx, "referenceName", text(name))
            .unwrap();
    }
    payment
        .set(&mut fyo, "paymentMethod", text("Cash"))
        .unwrap();
    payment
        .set(&mut fyo, "paymentAccount", text(COUNTER))
        .unwrap();
    payment.sync(&mut fyo).unwrap();
    payment.submit(&mut fyo).unwrap();

    let amounts = transacted_amounts(&fyo, start, jiff::Timestamp::now()).unwrap();
    assert_eq!(amounts["Cash"].round(None), "2100.00");
}

#[test]
fn the_cart_is_checked_as_books_checks_it() {
    let (_file, mut fyo) = shop();
    set_up_pos(&mut fyo);
    let qty_map = item_qty_map(&fyo).unwrap();
    assert_eq!(qty_map[PEN].available, 10.0);
    let item = pen(&fyo);

    let mut sale = new_sale(&fyo, None).unwrap();
    add_item(&mut fyo, &mut sale, &item, 1.0, None, &qty_map).unwrap();
    add_item(&mut fyo, &mut sale, &item, 2.0, None, &qty_map).unwrap();
    assert_eq!(sale.table("items").len(), 1);
    assert_eq!(total_quantity(&sale), 3.0);
    let error = add_item(&mut fyo, &mut sale, &item, 8.0, None, &qty_map).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Cannot add more than the available quantity"
    );

    // An untracked item first doesn't end the stock check.
    let mut sale = new_sale(&fyo, None).unwrap();
    let row = map([
        ("item", text("Pencil")),
        ("quantity", DocValue::Number(1.0)),
    ]);
    sale.append(&mut fyo, "items", row).unwrap();
    let row = map([
        ("item", text(PEN)),
        ("rate", DocValue::Number(700.0)),
        ("quantity", DocValue::Number(11.0)),
    ]);
    sale.append(&mut fyo, "items", row).unwrap();
    assert!(
        validate_sale(&fyo, &sale, &qty_map)
            .unwrap_err()
            .to_string()
            .starts_with("Insufficient Quantity. Item Pen has only 10")
    );

    // A 1% discount is a discount.
    let mut settings = fyo.get_doc("AccountingSettings", None).unwrap();
    settings
        .set(&mut fyo, "enableDiscounting", DocValue::Bool(true))
        .unwrap();
    settings.sync(&mut fyo).unwrap();
    sale.set_child(
        &mut fyo,
        "items",
        1,
        "setItemDiscountAmount",
        DocValue::Bool(false),
    )
    .unwrap();
    sale.set_child(
        &mut fyo,
        "items",
        1,
        "itemDiscountPercent",
        DocValue::Number(1.0),
    )
    .unwrap();
    assert_eq!(item_discounts(&fyo, &sale).unwrap().round(None), "77.00");
}

#[test]
fn a_search_finds_an_item_by_barcode_or_weight() {
    let (_file, mut fyo) = shop();
    set_up_pos(&mut fyo);
    fyo.db
        .update(
            "InventorySettings",
            &map([("enableBarcodes", DocValue::Bool(true))]),
        )
        .unwrap();
    let mut pen = fyo.get_doc("Item", Some(PEN)).unwrap();
    pen.set(&mut fyo, "barcode", text("890100000017")).unwrap();
    pen.set(&mut fyo, "itemCode", text("4711")).unwrap();
    pen.set(&mut fyo, "unit", text("Kg")).unwrap();
    pen.sync(&mut fyo).unwrap();

    assert_eq!(
        find_item(&fyo, "890100000017").unwrap(),
        Some((PEN.to_owned(), 1.0))
    );
    assert_eq!(find_item(&fyo, PEN).unwrap(), Some((PEN.to_owned(), 1.0)));
    assert_eq!(find_item(&fyo, "890100000018").unwrap(), None);

    // A prefix written as 21 is two digits, then four of item code and five of weight in
    // grams.
    let values = map([
        ("weightEnabledBarcode", DocValue::Bool(true)),
        ("checkDigits", DocValue::Number(21.0)),
        ("itemCodeDigits", DocValue::Number(4.0)),
        ("itemWeightDigits", DocValue::Number(5.0)),
    ]);
    fyo.db.update("POSSettings", &values).unwrap();
    assert_eq!(
        find_item(&fyo, "21471101250").unwrap(),
        Some((PEN.to_owned(), 1.25))
    );
}

#[test]
fn a_saved_sale_is_taken_up_and_paid_later() {
    let (_file, mut fyo) = shop();
    set_up_pos(&mut fyo);
    let mut sale = new_sale(&fyo, None).unwrap();
    let qty_map = item_qty_map(&fyo).unwrap();
    let item = pen(&fyo);
    add_item(&mut fyo, &mut sale, &item, 2.0, None, &qty_map).unwrap();
    save_sale(&mut fyo, &mut sale).unwrap();
    let name = sale.name().unwrap().to_owned();
    let saved = held_sales(&fyo, false).unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].get("name"), Some(&text(&name)));

    // Submitted but not paid.
    let mut sale = fyo.get_doc("SalesInvoice", Some(&name)).unwrap();
    create_transaction(&mut fyo, &mut sale, &HashMap::new(), None).unwrap();
    assert!(held_sales(&fyo, false).unwrap().is_empty());
    assert_eq!(held_sales(&fyo, true).unwrap().len(), 1);

    let mut sale = fyo.get_doc("SalesInvoice", Some(&name)).unwrap();
    let payment = cash(&fyo, 1400.0);
    let done = create_transaction(&mut fyo, &mut sale, &HashMap::new(), Some(&payment)).unwrap();
    assert!(done.payment.is_some());
    assert!(done.shipment.is_none());
    assert!(held_sales(&fyo, true).unwrap().is_empty());
}
