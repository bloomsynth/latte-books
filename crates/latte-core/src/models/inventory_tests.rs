// The inventory tests of Books (models/inventory/tests/testInventory.spec.ts and
// testStockTransfer.spec.ts @ v0.36.0), on a company set up as the wizard sets one up.

use latte_db::{DocValue, DocValueMap, Filter, GetAllOptions, Order, QueryFilter};

use crate::models::base_models::invoice::get_stock_transfer;
use crate::models::inventory::helpers::get_stock_quantity;
use crate::models::inventory::stock_transfer::get_return_doc;
use crate::models::payment_tests::validation_message;
use crate::setup_tests::{TempFile, indian_company};
use crate::tests::{map, money, text};
use crate::{Doc, Fyo};

const PEN: &str = "Pen";
const INK: &str = "Ink";
const ONE: &str = "LocationOne";
const TWO: &str = "LocationTwo";
const PARTY: &str = "Someone";

/// A company with Pen at 700 and Ink at 50, both tracked, two locations and a party that
/// is both customer and supplier.
fn inventory() -> (TempFile, Fyo) {
    let file = TempFile::new("inventory");
    let mut fyo = indian_company(&file);
    for (item, rate) in [(PEN, 700.0), (INK, 50.0)] {
        let values = map([
            ("name", text(item)),
            ("rate", DocValue::Number(rate)),
            ("trackItem", DocValue::Bool(true)),
        ]);
        fyo.get_new_doc("Item", values)
            .unwrap()
            .sync(&mut fyo)
            .unwrap();
    }
    for location in [ONE, TWO] {
        let values = map([("name", text(location))]);
        fyo.get_new_doc("Location", values)
            .unwrap()
            .sync(&mut fyo)
            .unwrap();
    }
    let values = map([("name", text(PARTY)), ("role", text("Both"))]);
    fyo.get_new_doc("Party", values)
        .unwrap()
        .sync(&mut fyo)
        .unwrap();
    (file, fyo)
}

/// A date as the form sets it.
fn date(date: &str) -> DocValue {
    DocValue::Date(format!("{date}T00:00:00Z").parse().unwrap())
}

fn value(value: Option<&str>) -> DocValue {
    value.map_or(DocValue::Null, text)
}

/// A row of a stock movement: item, from, to, quantity and rate.
type Move<'a> = (&'a str, Option<&'a str>, Option<&'a str>, f64, f64);

/// `getStockMovement`.
fn movement(fyo: &mut Fyo, movement_type: &str, date: &str, rows: &[Move<'_>]) -> Doc {
    let values = map([("movementType", text(movement_type)), ("date", text(date))]);
    let mut doc = fyo.get_new_doc("StockMovement", values).unwrap();
    for (item, from, to, quantity, rate) in rows {
        let row = map([
            ("item", text(item)),
            ("fromLocation", value(*from)),
            ("toLocation", value(*to)),
            ("rate", DocValue::Number(*rate)),
            ("quantity", DocValue::Number(*quantity)),
        ]);
        doc.append(fyo, "items", row).unwrap();
    }
    doc
}

/// `getStockTransfer`: Pen at `location`, at its rate of 700.
fn transfer(fyo: &mut Fyo, schema_name: &str, date: &str, quantity: f64) -> Doc {
    let values = map([("party", text(PARTY)), ("date", text(date))]);
    let mut doc = fyo.get_new_doc(schema_name, values).unwrap();
    let row = map([
        ("item", text(PEN)),
        ("location", text(ONE)),
        ("quantity", DocValue::Number(quantity)),
        ("rate", DocValue::Number(700.0)),
    ]);
    doc.append(fyo, "items", row).unwrap();
    doc
}

fn submit(fyo: &mut Fyo, doc: &mut Doc) {
    doc.sync(fyo).unwrap();
    doc.submit(fyo).unwrap();
}

fn stock(fyo: &Fyo, item: &str, location: Option<&str>) -> Option<f64> {
    get_stock_quantity(fyo, item, location, None, None, None, &[]).unwrap()
}

fn rows(fyo: &Fyo, schema_name: &str, name: &str, fields: &[&str]) -> Vec<DocValueMap> {
    let options = GetAllOptions {
        fields: Some(fields.iter().map(|f| (*f).to_owned()).collect()),
        filters: QueryFilter::from([("referenceName".to_owned(), Filter::from(name))]),
        order_by: Some(vec!["name".to_owned()]),
        order: Some(Order::Asc),
        ..GetAllOptions::default()
    };
    fyo.db.get_all_docs(schema_name, &options).unwrap()
}

/// The stock ledger entries of a document, as `(location, quantity)`.
fn sles(fyo: &Fyo, name: &str) -> Vec<(String, f64)> {
    rows(fyo, "StockLedgerEntry", name, &["location", "quantity"])
        .iter()
        .map(|row| {
            let Some(DocValue::Text(location)) = row.get("location") else {
                panic!("location is {:?}", row.get("location"));
            };
            let Some(DocValue::Number(quantity)) = row.get("quantity") else {
                panic!("quantity is {:?}", row.get("quantity"));
            };
            (location.clone(), *quantity)
        })
        .collect()
}

/// The ledger entries of a document, as `(account, debit, credit, reverted)`.
fn ales(fyo: &Fyo, name: &str) -> Vec<(String, String, String, bool)> {
    let fields = ["account", "debit", "credit", "reverted"];
    let amount = |row: &DocValueMap, fieldname| match row.get(fieldname) {
        Some(DocValue::Money(money)) => money.round(None),
        value => panic!("{fieldname} is {value:?}"),
    };
    rows(fyo, "AccountingLedgerEntry", name, &fields)
        .iter()
        .map(|row| {
            let Some(DocValue::Text(account)) = row.get("account") else {
                panic!("account is {:?}", row.get("account"));
            };
            let reverted = row.get("reverted") == Some(&DocValue::Bool(true));
            (
                account.clone(),
                amount(row, "debit"),
                amount(row, "credit"),
                reverted,
            )
        })
        .collect()
}

fn ale(account: &str, debit: &str, credit: &str) -> (String, String, String, bool) {
    (
        account.to_owned(),
        debit.to_owned(),
        credit.to_owned(),
        false,
    )
}

fn not_transferred(fyo: &Fyo, schema_name: &str, name: &str) -> Option<DocValue> {
    fyo.get_value(schema_name, Some(name), "stockNotTransferred")
}

#[test]
fn stock_movements_move_stock_and_cancelling_them_moves_it_back() {
    let (_file, mut fyo) = inventory();

    let receipt = (INK, None, Some(ONE), 2.0, 50.0);
    let mut doc = movement(
        &mut fyo,
        "MaterialReceipt",
        "2026-11-03T09:57:04.528Z",
        &[receipt],
    );
    submit(&mut fyo, &mut doc);
    let receipt = doc.name().unwrap().to_owned();
    assert!(receipt.starts_with("SMOV-"), "{receipt}");
    assert_eq!(money(&doc, "amount"), "100.00");
    assert_eq!(money(&doc.table("items")[0], "amount"), "100.00");
    assert_eq!(sles(&fyo, &receipt), vec![(ONE.to_owned(), 2.0)]);
    assert_eq!(stock(&fyo, INK, None), Some(2.0));

    let transfer = (INK, Some(ONE), Some(TWO), 2.0, 50.0);
    let mut doc = movement(
        &mut fyo,
        "MaterialTransfer",
        "2026-11-03T09:58:04.528Z",
        &[transfer],
    );
    submit(&mut fyo, &mut doc);
    let transfer = doc.name().unwrap().to_owned();
    assert_eq!(
        sles(&fyo, &transfer),
        vec![(ONE.to_owned(), -2.0), (TWO.to_owned(), 2.0)]
    );
    assert_eq!(stock(&fyo, INK, Some(ONE)), Some(0.0));
    assert_eq!(stock(&fyo, INK, None), Some(2.0));

    let issue = (INK, Some(TWO), None, 2.0, 50.0);
    let mut doc = movement(
        &mut fyo,
        "MaterialIssue",
        "2026-11-03T09:59:04.528Z",
        &[issue],
    );
    submit(&mut fyo, &mut doc);
    let issue = doc.name().unwrap().to_owned();
    assert_eq!(sles(&fyo, &issue), vec![(TWO.to_owned(), -2.0)]);
    assert_eq!(stock(&fyo, INK, None), Some(0.0));

    for name in [issue, transfer, receipt] {
        let mut doc = fyo.get_doc("StockMovement", Some(&name)).unwrap();
        doc.cancel(&mut fyo).unwrap();
        assert!(sles(&fyo, &name).is_empty(), "{name}");
    }
    assert_eq!(stock(&fyo, INK, None), None);
}

/// `runEntries`: submits each movement, which must go through or not, then checks the
/// stock.
fn run_entries(fyo: &mut Fyo, item: &str, entries: &[(&str, &str, bool, f64, Move<'_>)]) {
    for (movement_type, date, valid, post_quantity, row) in entries {
        let mut doc = movement(fyo, movement_type, date, &[*row]);
        doc.sync(fyo).unwrap();
        let result = doc.submit(fyo);
        assert_eq!(
            result.is_ok(),
            *valid,
            "{movement_type} on {date}: {result:?}"
        );
        assert_eq!(stock(fyo, item, None), Some(*post_quantity), "{date}");
    }
}

#[test]
fn a_movement_can_take_no_more_than_the_stock() {
    let (_file, mut fyo) = inventory();
    let entries = [
        (
            "MaterialReceipt",
            "2026-11-03T09:58:04.528Z",
            true,
            10.0,
            (PEN, None, Some(ONE), 10.0, 700.0),
        ),
        (
            "MaterialTransfer",
            "2026-11-03T09:58:05.528Z",
            false,
            10.0,
            (PEN, Some(ONE), Some(TWO), 11.0, 700.0),
        ),
        (
            "MaterialIssue",
            "2026-11-03T09:58:06.528Z",
            false,
            10.0,
            (PEN, Some(ONE), None, 11.0, 700.0),
        ),
        (
            "MaterialTransfer",
            "2026-11-03T09:58:07.528Z",
            true,
            10.0,
            (PEN, Some(ONE), Some(TWO), 10.0, 700.0),
        ),
        (
            "MaterialIssue",
            "2026-11-03T09:58:08.528Z",
            true,
            0.0,
            (PEN, Some(TWO), None, 10.0, 700.0),
        ),
    ];
    run_entries(&mut fyo, PEN, &entries);
}

#[test]
fn a_movement_dated_earlier_can_take_no_more_than_the_stock_then() {
    let (_file, mut fyo) = inventory();
    let entries = [
        (
            "MaterialReceipt",
            "2026-11-15T00:00:00.000Z",
            true,
            10.0,
            (INK, None, Some(ONE), 10.0, 50.0),
        ),
        (
            "MaterialIssue",
            "2026-11-17T00:00:00.000Z",
            true,
            5.0,
            (INK, Some(ONE), None, 5.0, 50.0),
        ),
        (
            "MaterialTransfer",
            "2026-11-16T00:00:00.000Z",
            false,
            5.0,
            (INK, Some(ONE), Some(TWO), 10.0, 50.0),
        ),
    ];
    run_entries(&mut fyo, INK, &entries);
}

#[test]
fn receipts_and_shipments_post_to_the_stock_accounts() {
    let (_file, mut fyo) = inventory();
    let settings = fyo.get_doc("InventorySettings", None).unwrap();
    assert!(
        settings
            .get("stockInHand")
            .is_some_and(|v| v != &DocValue::Null)
    );
    assert!(
        settings
            .get("stockReceivedButNotBilled")
            .is_some_and(|v| v != &DocValue::Null)
    );

    let mut receipt = transfer(&mut fyo, "PurchaseReceipt", "2026-01-01", 10.0);
    receipt.sync(&mut fyo).unwrap();
    assert_eq!(money(&receipt, "grandTotal"), "7000.00");
    receipt.submit(&mut fyo).unwrap();
    let receipt = receipt.name().unwrap().to_owned();
    assert!(receipt.starts_with("PREC-"), "{receipt}");
    assert_eq!(sles(&fyo, &receipt).len(), 1);
    assert_eq!(stock(&fyo, PEN, Some(ONE)), Some(10.0));
    assert_eq!(
        ales(&fyo, &receipt),
        vec![
            ale("Stock In Hand", "7000.00", "0.00"),
            ale("Stock Received But Not Billed", "0.00", "7000.00"),
        ]
    );

    let mut shipment = transfer(&mut fyo, "Shipment", "2026-01-02", 5.0);
    shipment.sync(&mut fyo).unwrap();
    assert_eq!(money(&shipment, "grandTotal"), "3500.00");
    shipment.submit(&mut fyo).unwrap();
    let shipment = shipment.name().unwrap().to_owned();
    assert!(shipment.starts_with("SHPM-"), "{shipment}");
    assert_eq!(stock(&fyo, PEN, Some(ONE)), Some(5.0));
    assert_eq!(
        ales(&fyo, &shipment),
        vec![
            ale("Cost of Goods Sold", "3500.00", "0.00"),
            ale("Stock In Hand", "0.00", "3500.00"),
        ]
    );

    // More than is in stock.
    let mut invalid = transfer(&mut fyo, "Shipment", "2026-01-03", 10.0);
    invalid.sync(&mut fyo).unwrap();
    assert!(invalid.submit(&mut fyo).is_err());
    let invalid = invalid.name().unwrap().to_owned();
    assert!(sles(&fyo, &invalid).is_empty());
    assert!(ales(&fyo, &invalid).is_empty());

    // Cancelling the receipt would take out stock that was shipped.
    let mut doc = fyo.get_doc("PurchaseReceipt", Some(&receipt)).unwrap();
    assert!(doc.cancel(&mut fyo).is_err());
    assert_eq!(stock(&fyo, PEN, Some(ONE)), Some(5.0));
    assert_eq!(sles(&fyo, &receipt).len(), 1);
    assert!(ales(&fyo, &receipt).iter().all(|(.., reverted)| !reverted));

    let mut doc = fyo.get_doc("Shipment", Some(&shipment)).unwrap();
    doc.cancel(&mut fyo).unwrap();
    assert_eq!(stock(&fyo, PEN, Some(ONE)), Some(10.0));
    assert!(sles(&fyo, &shipment).is_empty());
    let entries = ales(&fyo, &shipment);
    assert!(entries.len() == 4 && entries.iter().all(|(.., reverted)| *reverted));
    doc.delete(&mut fyo).unwrap();
    assert!(ales(&fyo, &shipment).is_empty());
    assert!(!fyo.db.exists("Shipment", Some(&shipment)).unwrap());

    let mut doc = fyo.get_doc("PurchaseReceipt", Some(&receipt)).unwrap();
    doc.cancel(&mut fyo).unwrap();
    assert_eq!(stock(&fyo, PEN, Some(ONE)), None);
    doc.delete(&mut fyo).unwrap();
    assert!(ales(&fyo, &receipt).is_empty());
}

/// A submitted invoice for Pens.
fn invoice(fyo: &mut Fyo, schema_name: &str, date: &str, quantity: f64) -> Doc {
    let account = if schema_name == "SalesInvoice" {
        "Debtors"
    } else {
        "Creditors"
    };
    let values = map([
        ("party", text(PARTY)),
        ("date", text(date)),
        ("account", text(account)),
    ]);
    let mut doc = fyo.get_new_doc(schema_name, values).unwrap();
    let row = map([
        ("item", text(PEN)),
        ("quantity", DocValue::Number(quantity)),
        ("rate", DocValue::Number(700.0)),
    ]);
    doc.append(fyo, "items", row).unwrap();
    submit(fyo, &mut doc);
    doc
}

#[test]
fn a_receipt_made_from_a_purchase_invoice_receives_what_it_has_left() {
    let (_file, mut fyo) = inventory();
    let pinv = invoice(&mut fyo, "PurchaseInvoice", "2026-01-04", 3.0);
    let pinv_name = pinv.name().unwrap().to_owned();
    assert_eq!(pinv_name, "PINV-1001");
    assert_eq!(
        pinv.get("stockNotTransferred"),
        Some(&DocValue::Number(3.0))
    );

    let mut receipt = get_stock_transfer(&pinv, &fyo, false).unwrap().unwrap();
    assert_eq!(receipt.schema_name(), "PurchaseReceipt");
    assert_eq!(receipt.get("backReference"), Some(&text(&pinv_name)));
    assert_eq!(
        receipt.table("items")[0].get("quantity"),
        Some(&DocValue::Number(3.0))
    );
    submit(&mut fyo, &mut receipt);
    assert_eq!(
        not_transferred(&fyo, "PurchaseInvoice", &pinv_name),
        Some(DocValue::Number(0.0))
    );

    // Cancelling the receipt puts it back on the invoice.
    receipt.cancel(&mut fyo).unwrap();
    assert_eq!(
        not_transferred(&fyo, "PurchaseInvoice", &pinv_name),
        Some(DocValue::Number(3.0))
    );

    let pinv = fyo.get_doc("PurchaseInvoice", Some(&pinv_name)).unwrap();
    let mut receipt = get_stock_transfer(&pinv, &fyo, false).unwrap().unwrap();
    submit(&mut fyo, &mut receipt);
    let receipt_name = receipt.name().unwrap().to_owned();

    let mut pinv = fyo.get_doc("PurchaseInvoice", Some(&pinv_name)).unwrap();
    let message = validation_message(pinv.cancel(&mut fyo));
    assert_eq!(
        message,
        format!(
            "Cannot cancel Purchase Invoice {pinv_name} because of the following Purchase Receipt: {receipt_name}"
        )
    );
    assert!(
        ales(&fyo, &pinv_name)
            .iter()
            .all(|(.., reverted)| !reverted)
    );
}

#[test]
fn shipments_made_from_a_sales_invoice_ship_what_it_has_left() {
    let (_file, mut fyo) = inventory();
    let mut receipt = transfer(&mut fyo, "PurchaseReceipt", "2026-01-05", 10.0);
    submit(&mut fyo, &mut receipt);

    let sinv = invoice(&mut fyo, "SalesInvoice", "2026-01-06", 3.0);
    let sinv_name = sinv.name().unwrap().to_owned();
    assert_eq!(
        sinv.get("stockNotTransferred"),
        Some(&DocValue::Number(3.0))
    );

    let mut first = get_stock_transfer(&sinv, &fyo, false).unwrap().unwrap();
    first.set(&mut fyo, "date", date("2026-01-07")).unwrap();
    first
        .set_child(&mut fyo, "items", 0, "location", text(ONE))
        .unwrap();
    first
        .set_child(&mut fyo, "items", 0, "quantity", DocValue::Number(1.0))
        .unwrap();
    submit(&mut fyo, &mut first);
    assert_eq!(first.get("backReference"), Some(&text(&sinv_name)));
    assert_eq!(
        not_transferred(&fyo, "SalesInvoice", &sinv_name),
        Some(DocValue::Number(2.0))
    );

    let sinv = fyo.get_doc("SalesInvoice", Some(&sinv_name)).unwrap();
    assert_eq!(
        sinv.table("items")[0].get("stockNotTransferred"),
        Some(&DocValue::Number(2.0))
    );
    let mut second = get_stock_transfer(&sinv, &fyo, false).unwrap().unwrap();
    second
        .set_child(&mut fyo, "items", 0, "location", text(ONE))
        .unwrap();
    assert_eq!(
        second.table("items")[0].get("quantity"),
        Some(&DocValue::Number(2.0))
    );
    submit(&mut fyo, &mut second);
    let sinv = fyo.get_doc("SalesInvoice", Some(&sinv_name)).unwrap();
    assert_eq!(
        sinv.get("stockNotTransferred"),
        Some(&DocValue::Number(0.0))
    );
    assert!(get_stock_transfer(&sinv, &fyo, false).unwrap().is_none());

    let mut doc = fyo.get_doc("SalesInvoice", Some(&sinv_name)).unwrap();
    assert!(doc.cancel(&mut fyo).is_err());

    // A copy isn't made from the invoice.
    let copy = first.duplicate(&fyo).unwrap();
    assert_eq!(copy.get("backReference"), Some(&DocValue::Null));

    first.cancel(&mut fyo).unwrap();
    assert_eq!(
        not_transferred(&fyo, "SalesInvoice", &sinv_name),
        Some(DocValue::Number(1.0))
    );
    second.cancel(&mut fyo).unwrap();
    assert_eq!(
        not_transferred(&fyo, "SalesInvoice", &sinv_name),
        Some(DocValue::Number(3.0))
    );

    // Deleting the invoice deletes its cancelled shipments.
    let mut doc = fyo.get_doc("SalesInvoice", Some(&sinv_name)).unwrap();
    doc.cancel(&mut fyo).unwrap();
    doc.delete(&mut fyo).unwrap();
    assert!(!fyo.db.exists("SalesInvoice", Some(&sinv_name)).unwrap());
    for name in [first.name().unwrap(), second.name().unwrap()] {
        assert!(!fyo.db.exists("Shipment", Some(name)).unwrap(), "{name}");
    }
}

#[test]
fn choosing_the_invoice_of_a_shipment_fills_it_in() {
    let (_file, mut fyo) = inventory();
    let mut receipt = transfer(&mut fyo, "PurchaseReceipt", "2026-01-08", 10.0);
    submit(&mut fyo, &mut receipt);
    let sinv = invoice(&mut fyo, "SalesInvoice", "2026-01-09", 5.0);

    let mut shipment = fyo.get_new_doc("Shipment", DocValueMap::new()).unwrap();
    shipment
        .set(&mut fyo, "backReference", text(sinv.name().unwrap()))
        .unwrap();
    shipment.set(&mut fyo, "date", date("2026-01-10")).unwrap();
    shipment
        .set_child(&mut fyo, "items", 0, "location", text(ONE))
        .unwrap();
    assert_eq!(shipment.get("party"), Some(&text(PARTY)));
    submit(&mut fyo, &mut shipment);

    assert_eq!(stock(&fyo, PEN, Some(ONE)), Some(5.0));
    let not_shipped = not_transferred(&fyo, "SalesInvoice", sinv.name().unwrap());
    assert_eq!(not_shipped, Some(DocValue::Number(0.0)));
}

#[test]
fn a_return_reverses_what_its_shipment_or_receipt_posted() {
    let (_file, mut fyo) = inventory();
    let mut receipt = transfer(&mut fyo, "PurchaseReceipt", "2026-05-17", 6.0);
    submit(&mut fyo, &mut receipt);
    let mut shipment = transfer(&mut fyo, "Shipment", "2026-05-18", 3.0);
    submit(&mut fyo, &mut shipment);

    let mut shipment_return = get_return_doc(&shipment, &fyo).unwrap().unwrap();
    assert_eq!(
        shipment_return.get("returnAgainst"),
        Some(&text(shipment.name().unwrap()))
    );
    assert_eq!(money(&shipment_return, "grandTotal"), "-2100.00");
    submit(&mut fyo, &mut shipment_return);
    assert_eq!(stock(&fyo, PEN, Some(ONE)), Some(6.0));
    // Books posts -2100 on these sides, which ships the stock again.
    assert_eq!(
        ales(&fyo, shipment_return.name().unwrap()),
        vec![
            ale("Stock In Hand", "2100.00", "0.00"),
            ale("Cost of Goods Sold", "0.00", "2100.00"),
        ]
    );
    let returned = fyo.get_value("Shipment", shipment.name(), "isReturned");
    assert_eq!(returned, Some(DocValue::Bool(true)));
    // Nothing is left to return.
    let shipment = fyo.get_doc("Shipment", shipment.name()).unwrap();
    let again = get_return_doc(&shipment, &fyo).unwrap().unwrap();
    assert!(again.table("items").is_empty());

    let mut receipt_return = get_return_doc(&receipt, &fyo).unwrap().unwrap();
    assert_eq!(money(&receipt_return, "grandTotal"), "-4200.00");
    submit(&mut fyo, &mut receipt_return);
    assert_eq!(stock(&fyo, PEN, Some(ONE)), Some(0.0));
    assert_eq!(
        ales(&fyo, receipt_return.name().unwrap()),
        vec![
            ale("Stock Received But Not Billed", "4200.00", "0.00"),
            ale("Stock In Hand", "0.00", "4200.00"),
        ]
    );
}

#[test]
fn a_return_takes_back_no_more_than_was_shipped() {
    let (_file, mut fyo) = inventory();
    let mut receipt = transfer(&mut fyo, "PurchaseReceipt", "2026-05-17", 6.0);
    submit(&mut fyo, &mut receipt);
    let mut shipment = transfer(&mut fyo, "Shipment", "2026-05-18", 3.0);
    submit(&mut fyo, &mut shipment);

    let mut doc = get_return_doc(&shipment, &fyo).unwrap().unwrap();
    doc.set_child(&mut fyo, "items", 0, "quantity", DocValue::Number(-4.0))
        .unwrap();
    doc.sync(&mut fyo).unwrap();
    let message = validation_message(doc.submit(&mut fyo));
    assert_eq!(
        message,
        format!(
            "Cannot return 4 of Pen: only 3 is left to return on {}.",
            shipment.name().unwrap()
        )
    );
}

#[test]
fn a_return_invoice_ships_against_its_invoices_shipment() {
    let (_file, mut fyo) = inventory();
    let mut receipt = transfer(&mut fyo, "PurchaseReceipt", "2026-05-17", 6.0);
    submit(&mut fyo, &mut receipt);
    let pinv = invoice(&mut fyo, "PurchaseInvoice", "2026-05-19", 2.0);
    let mut pinv_receipt = get_stock_transfer(&pinv, &fyo, false).unwrap().unwrap();
    pinv_receipt
        .set_child(&mut fyo, "items", 0, "location", text(ONE))
        .unwrap();
    submit(&mut fyo, &mut pinv_receipt);

    let mut pinv_return = crate::models::base_models::invoice::get_return_doc(&pinv, &fyo)
        .unwrap()
        .unwrap();
    submit(&mut fyo, &mut pinv_return);
    // Books looks for a shipment of a sales invoice here, and fails.
    let return_receipt = get_stock_transfer(&pinv_return, &fyo, false)
        .unwrap()
        .unwrap();
    assert_eq!(return_receipt.schema_name(), "PurchaseReceipt");
    assert_eq!(
        return_receipt.get("returnAgainst"),
        Some(&text(pinv_receipt.name().unwrap()))
    );
}

/// The cells of the named columns, by row.
fn report_values(
    fyo: &Fyo,
    report: &mut dyn crate::reports::Report,
    fieldnames: &[&str],
) -> Vec<Vec<String>> {
    let formatter = crate::fyo::format::Formatter::new(fyo).unwrap();
    let columns = report.columns(fyo, &formatter).unwrap();
    let indexes: Vec<_> = fieldnames
        .iter()
        .map(|f| columns.iter().position(|c| c.fieldname == *f).unwrap())
        .collect();
    report
        .run(fyo, &formatter)
        .unwrap()
        .iter()
        .map(|row| {
            indexes
                .iter()
                .map(|i| row.cells[*i].value.clone())
                .collect()
        })
        .collect()
}

#[test]
fn the_stock_reports_value_stock_first_in_first_out() {
    use crate::reports::stock_balance::StockBalance;
    use crate::reports::stock_ledger::StockLedger;
    use jiff::civil::date as day;

    let (_file, mut fyo) = inventory();
    for (date, quantity, rate) in [
        ("2026-11-03T10:00:00.000Z", 5.0, 50.0),
        ("2026-11-04T10:00:00.000Z", 5.0, 60.0),
    ] {
        let mut doc = movement(
            &mut fyo,
            "MaterialReceipt",
            date,
            &[(INK, None, Some(ONE), quantity, rate)],
        );
        submit(&mut fyo, &mut doc);
    }
    let issue = (INK, Some(ONE), None, 6.0, 50.0);
    let mut doc = movement(
        &mut fyo,
        "MaterialIssue",
        "2026-11-05T10:00:00.000Z",
        &[issue],
    );
    submit(&mut fyo, &mut doc);

    let mut ledger = StockLedger {
        from_date: Some(day(2026, 11, 1)),
        to_date: Some(day(2026, 11, 30)),
        ascending: true,
        ..StockLedger::default()
    };
    let fields = [
        "name",
        "quantity",
        "balanceQuantity",
        "valuationRate",
        "balanceValue",
    ];
    let values = report_values(&fyo, &mut ledger, &fields);
    let row = |cells: [&str; 5]| cells.map(str::to_owned).to_vec();
    assert_eq!(
        values,
        vec![
            row(["1", "5.00", "5.00", "₹ 50.00", "₹ 250.00"]),
            row(["2", "5.00", "10.00", "₹ 55.00", "₹ 550.00"]),
            row(["3", "-6.00", "4.00", "₹ 60.00", "₹ 240.00"]),
        ]
    );

    // The To Date is included.
    ledger.to_date = Some(day(2026, 11, 4));
    assert_eq!(report_values(&fyo, &mut ledger, &["name"]).len(), 2);

    let mut balance = StockBalance {
        from_date: Some(day(2026, 11, 4)),
        to_date: Some(day(2026, 11, 30)),
        ..StockBalance::default()
    };
    let fields = [
        "item",
        "openingQuantity",
        "incomingQuantity",
        "outgoingQuantity",
        "balanceQuantity",
    ];
    assert_eq!(
        report_values(&fyo, &mut balance, &fields),
        vec![
            vec![INK, "5.00", "5.00", "6.00", "4.00"]
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        ]
    );
}

#[test]
fn a_receipt_takes_off_its_invoices_discount() {
    let (_file, mut fyo) = inventory();
    let values = map([("enableDiscounting", DocValue::Bool(true))]);
    fyo.db.update("AccountingSettings", &values).unwrap();

    let values = map([
        ("party", text(PARTY)),
        ("date", text("2026-01-04")),
        ("account", text("Creditors")),
        ("setDiscountAmount", DocValue::Bool(true)),
    ]);
    let mut pinv = fyo.get_new_doc("PurchaseInvoice", values).unwrap();
    let row = map([
        ("item", text(PEN)),
        ("quantity", DocValue::Number(2.0)),
        ("rate", DocValue::Number(700.0)),
    ]);
    pinv.append(&mut fyo, "items", row).unwrap();
    let discount = DocValue::Money(fyo.pesa().from_f64(100.0).unwrap());
    pinv.set(&mut fyo, "discountAmount", discount).unwrap();
    submit(&mut fyo, &mut pinv);
    assert_eq!(money(&pinv, "grandTotal"), "1300.00");

    // Books takes the discount percent of the rows, none, and receives 1400.
    let mut receipt = get_stock_transfer(&pinv, &fyo, false).unwrap().unwrap();
    submit(&mut fyo, &mut receipt);
    assert_eq!(money(&receipt, "grandTotal"), "1300.00");
    let name = receipt.name().unwrap().to_owned();
    assert_eq!(
        ales(&fyo, &name),
        vec![
            ale("Stock In Hand", "1300.00", "0.00"),
            ale("Stock Received But Not Billed", "0.00", "1300.00"),
        ]
    );
}

#[test]
fn a_discount_after_tax_keeps_the_tax_and_carries_to_the_shipment() {
    let (_file, mut fyo) = inventory();
    let values = map([("enableDiscounting", DocValue::Bool(true))]);
    fyo.db.update("AccountingSettings", &values).unwrap();
    let mut receipt = transfer(&mut fyo, "PurchaseReceipt", "2026-01-01", 10.0);
    submit(&mut fyo, &mut receipt);

    let values = map([
        ("party", text(PARTY)),
        ("date", text("2026-01-02")),
        ("account", text("Debtors")),
        ("discountAfterTax", DocValue::Bool(true)),
    ]);
    let mut sinv = fyo.get_new_doc("SalesInvoice", values).unwrap();
    sinv.append(&mut fyo, "items", DocValueMap::new()).unwrap();
    for (fieldname, value) in [
        ("item", text(PEN)),
        ("quantity", DocValue::Number(2.0)),
        ("tax", text("GST-18")),
        ("itemDiscountPercent", DocValue::Number(10.0)),
    ] {
        sinv.set_child(&mut fyo, "items", 0, fieldname, value)
            .unwrap();
    }
    submit(&mut fyo, &mut sinv);
    // 1400 with 18% tax is 1652, less 10%. Books leaves the tax out: 1234.80.
    assert_eq!(money(&sinv.table("taxes")[0], "amount"), "126.00");
    assert_eq!(money(&sinv, "grandTotal"), "1486.80");
    assert_eq!(
        ales(&fyo, sinv.name().unwrap()),
        vec![
            ale("Debtors", "1486.80", "0.00"),
            ale("Sales", "0.00", "1400.00"),
            ale("CGST", "0.00", "126.00"),
            ale("SGST", "0.00", "126.00"),
            ale("Discounts", "165.20", "0.00"),
        ]
    );

    // Books takes 10% of the shipment row's taxed total, which it doesn't have, and
    // ships 1652.
    let mut shipment = get_stock_transfer(&sinv, &fyo, false).unwrap().unwrap();
    shipment
        .set_child(&mut fyo, "items", 0, "location", text(ONE))
        .unwrap();
    submit(&mut fyo, &mut shipment);
    assert_eq!(money(&shipment, "grandTotal"), "1486.80");
}

#[test]
fn a_shipment_costs_an_item_with_no_stock_entries_at_its_amount() {
    let (_file, mut fyo) = inventory();
    let values = map([("name", text("Box")), ("rate", DocValue::Number(20.0))]);
    fyo.get_new_doc("Item", values)
        .unwrap()
        .sync(&mut fyo)
        .unwrap();
    let mut receipt = transfer(&mut fyo, "PurchaseReceipt", "2026-01-01", 10.0);
    submit(&mut fyo, &mut receipt);

    // Books fails on Box: Pen has stock entries and Box has none.
    let mut shipment = transfer(&mut fyo, "Shipment", "2026-01-02", 2.0);
    let row = map([
        ("item", text("Box")),
        ("location", text(ONE)),
        ("quantity", DocValue::Number(3.0)),
        ("rate", DocValue::Number(20.0)),
    ]);
    shipment.append(&mut fyo, "items", row).unwrap();
    submit(&mut fyo, &mut shipment);
    assert_eq!(
        ales(&fyo, shipment.name().unwrap()),
        vec![
            ale("Cost of Goods Sold", "1460.00", "0.00"),
            ale("Stock In Hand", "0.00", "1460.00"),
        ]
    );
}

#[test]
fn a_serial_number_is_returned_once() {
    let (_file, mut fyo) = inventory();
    let values = map([("enableSerialNumber", DocValue::Bool(true))]);
    fyo.db.update("InventorySettings", &values).unwrap();
    let values = map([
        ("name", text("Phone")),
        ("rate", DocValue::Number(500.0)),
        ("trackItem", DocValue::Bool(true)),
        ("hasSerialNumber", DocValue::Bool(true)),
    ]);
    fyo.get_new_doc("Item", values)
        .unwrap()
        .sync(&mut fyo)
        .unwrap();
    let phones = |fyo: &mut Fyo, schema_name: &str, date: &str| {
        let values = map([("party", text(PARTY)), ("date", text(date))]);
        let mut doc = fyo.get_new_doc(schema_name, values).unwrap();
        let row = map([
            ("item", text("Phone")),
            ("location", text(ONE)),
            ("quantity", DocValue::Number(3.0)),
            ("rate", DocValue::Number(500.0)),
            ("serialNumber", text("SN1\nSN2\nSN3")),
        ]);
        doc.append(fyo, "items", row).unwrap();
        submit(fyo, &mut doc);
        doc
    };
    phones(&mut fyo, "PurchaseReceipt", "2026-01-01");
    let shipment = phones(&mut fyo, "Shipment", "2026-01-02");

    let mut first = get_return_doc(&shipment, &fyo).unwrap().unwrap();
    first
        .set_child(&mut fyo, "items", 0, "quantity", DocValue::Number(-1.0))
        .unwrap();
    first
        .set_child(&mut fyo, "items", 0, "serialNumber", text("SN1"))
        .unwrap();
    submit(&mut fyo, &mut first);

    // Books takes all three again.
    let second = get_return_doc(&shipment, &fyo).unwrap().unwrap();
    let row = &second.table("items")[0];
    assert_eq!(row.get("quantity"), Some(&DocValue::Number(-2.0)));
    assert_eq!(row.get("serialNumber"), Some(&text("SN2\nSN3")));
}
