use latte_db::{DocValue, DocValueMap};

use crate::models::base_models::invoice::{get_payment, get_return_doc};
use crate::models::invoice_tests::{invoice, outstanding, posting, postings};
use crate::models::payment_tests::{payments, submitted_invoice, validation_message};
use crate::tests::{money, text};
use crate::{Doc, Fyo};

/// A new return of all that is left to return on `name`, as Make Return makes it.
fn return_of(fyo: &mut Fyo, name: &str) -> Doc {
    let invoice = fyo.get_doc("SalesInvoice", Some(name)).unwrap();
    get_return_doc(&invoice, fyo).unwrap().unwrap()
}

fn quantities(doc: &Doc) -> Vec<f64> {
    doc.table("items")
        .iter()
        .map(|row| match row.get("quantity") {
            Some(DocValue::Number(quantity)) => *quantity,
            value => panic!("quantity is {value:?}"),
        })
        .collect()
}

fn flag(fyo: &Fyo, name: &str, fieldname: &str) -> bool {
    fyo.get_value("SalesInvoice", Some(name), fieldname) == Some(DocValue::Bool(true))
}

/// Submits a return of `quantity` (negative) of the first row of `name`.
fn submit_return(fyo: &mut Fyo, name: &str, quantity: Option<f64>) -> Doc {
    let mut doc = return_of(fyo, name);
    if let Some(quantity) = quantity {
        doc.set_child(fyo, "items", 0, "quantity", DocValue::Number(quantity))
            .unwrap();
    }
    doc.sync(fyo).unwrap();
    doc.submit(fyo).unwrap();
    doc
}

fn pay(fyo: &mut Fyo, invoice: &Doc) {
    let mut payment = get_payment(invoice, fyo).unwrap().unwrap();
    payment.sync(fyo).unwrap();
    payment.submit(fyo).unwrap();
}

#[test]
fn returning_an_invoice_reverses_its_posting_and_what_the_party_owes() {
    let mut fyo = payments();
    submitted_invoice(&mut fyo, 3.0);

    let mut doc = return_of(&mut fyo, "SINV-1001");
    assert_eq!(doc.get("returnAgainst"), Some(&text("SINV-1001")));
    assert_eq!(quantities(&doc), vec![-3.0]);
    assert_eq!(money(&doc, "grandTotal"), "-354.00");
    doc.sync(&mut fyo).unwrap();
    assert_eq!(doc.name(), Some("SINV-1002"));
    doc.submit(&mut fyo).unwrap();

    assert_eq!(
        postings(&fyo, "SINV-1002"),
        vec![
            posting("Debtors", "0.00", "354.00"),
            posting("Sales", "300.00", "0.00"),
            posting("CGST", "27.00", "0.00"),
            posting("SGST", "27.00", "0.00"),
        ]
    );
    assert_eq!(outstanding(&fyo, "SalesInvoice", "SINV-1002"), "-354.00");
    assert_eq!(outstanding(&fyo, "Party", "Acme"), "0.00");
    assert!(flag(&fyo, "SINV-1001", "isReturned"));
    assert!(flag(&fyo, "SINV-1001", "isFullyReturned"));
}

#[test]
fn a_partial_return_leaves_the_rest_to_return() {
    let mut fyo = payments();
    submitted_invoice(&mut fyo, 3.0);

    let doc = submit_return(&mut fyo, "SINV-1001", Some(-1.0));
    assert_eq!(money(&doc, "grandTotal"), "-118.00");
    assert!(flag(&fyo, "SINV-1001", "isReturned"));
    assert!(!flag(&fyo, "SINV-1001", "isFullyReturned"));
    assert_eq!(outstanding(&fyo, "Party", "Acme"), "236.00");

    assert_eq!(quantities(&return_of(&mut fyo, "SINV-1001")), vec![-2.0]);
    submit_return(&mut fyo, "SINV-1001", None);
    assert!(flag(&fyo, "SINV-1001", "isFullyReturned"));
    assert_eq!(outstanding(&fyo, "Party", "Acme"), "0.00");

    assert!(return_of(&mut fyo, "SINV-1001").table("items").is_empty());
}

#[test]
fn a_return_cant_take_back_more_than_is_left_to_return() {
    let mut fyo = payments();
    submitted_invoice(&mut fyo, 3.0);
    submit_return(&mut fyo, "SINV-1001", Some(-1.0));

    let mut doc = return_of(&mut fyo, "SINV-1001");
    doc.set_child(&mut fyo, "items", 0, "quantity", DocValue::Number(-3.0))
        .unwrap();
    doc.sync(&mut fyo).unwrap();
    assert_eq!(
        validation_message(doc.submit(&mut fyo)),
        "Cannot return 3 of Widget: only 2 is left to return on SINV-1001."
    );
    assert!(!doc.is_submitted());
}

#[test]
fn cancelling_a_return_lets_its_items_be_returned_again() {
    let mut fyo = payments();
    submitted_invoice(&mut fyo, 3.0);
    let mut doc = submit_return(&mut fyo, "SINV-1001", None);

    doc.cancel(&mut fyo).unwrap();
    assert_eq!(postings(&fyo, "SINV-1002"), vec![]);
    assert_eq!(outstanding(&fyo, "Party", "Acme"), "354.00");
    assert!(!flag(&fyo, "SINV-1001", "isReturned"));
    assert!(!flag(&fyo, "SINV-1001", "isFullyReturned"));
    assert_eq!(quantities(&return_of(&mut fyo, "SINV-1001")), vec![-3.0]);
}

#[test]
fn an_invoice_with_a_return_cant_be_cancelled() {
    let mut fyo = payments();
    let mut invoice = submitted_invoice(&mut fyo, 3.0);
    let mut doc = return_of(&mut fyo, "SINV-1001");
    doc.sync(&mut fyo).unwrap();

    assert_eq!(
        validation_message(invoice.cancel(&mut fyo)),
        "Cannot cancel SINV-1001 because of the following Sales Invoice: SINV-1002"
    );
}

#[test]
fn an_item_on_two_rows_is_returned_once() {
    let mut fyo = payments();
    let mut doc = invoice(
        &mut fyo,
        "SalesInvoice",
        "Acme",
        DocValueMap::new(),
        &[(2.0, DocValueMap::new()), (1.0, DocValueMap::new())],
    );
    doc.sync(&mut fyo).unwrap();
    doc.submit(&mut fyo).unwrap();

    assert_eq!(
        quantities(&return_of(&mut fyo, "SINV-1001")),
        vec![-2.0, -1.0]
    );
    submit_return(&mut fyo, "SINV-1001", None);
    assert_eq!(outstanding(&fyo, "Party", "Acme"), "0.00");
}

#[test]
fn a_paid_invoice_returned_is_refunded() {
    let mut fyo = payments();
    let invoice = submitted_invoice(&mut fyo, 3.0);
    pay(&mut fyo, &invoice);
    submit_return(&mut fyo, "SINV-1001", None);
    assert_eq!(outstanding(&fyo, "Party", "Acme"), "-354.00");

    // Loaded again, as after a restart.
    let doc = fyo.get_doc("SalesInvoice", Some("SINV-1002")).unwrap();
    let mut refund = get_payment(&doc, &fyo).unwrap().unwrap();
    assert_eq!(refund.get("paymentType"), Some(&text("Pay")));
    assert_eq!(money(&refund, "amount"), "354.00");
    assert_eq!(money(&refund.table("for")[0], "amount"), "-354.00");
    refund.sync(&mut fyo).unwrap();
    assert_eq!(refund.get("paymentAccount"), Some(&text("Debtors")));
    assert_eq!(refund.get("account"), Some(&text("Cash")));
    refund.submit(&mut fyo).unwrap();

    assert_eq!(
        postings(&fyo, "PAY-1002"),
        vec![
            posting("Debtors", "354.00", "0.00"),
            posting("Cash", "0.00", "354.00"),
        ]
    );
    assert_eq!(outstanding(&fyo, "SalesInvoice", "SINV-1002"), "0.00");
    assert_eq!(outstanding(&fyo, "Party", "Acme"), "0.00");
}

#[test]
fn a_refund_whose_amount_is_typed_in_still_clears_the_return() {
    let mut fyo = payments();
    let invoice = submitted_invoice(&mut fyo, 3.0);
    pay(&mut fyo, &invoice);
    submit_return(&mut fyo, "SINV-1001", None);

    let doc = fyo.get_doc("SalesInvoice", Some("SINV-1002")).unwrap();
    let mut refund = get_payment(&doc, &fyo).unwrap().unwrap();
    let amount = fyo.pesa().parse("354").unwrap();
    refund
        .set(&mut fyo, "amount", DocValue::Money(amount))
        .unwrap();
    assert_eq!(money(&refund.table("for")[0], "amount"), "-354.00");
    refund.sync(&mut fyo).unwrap();
    refund.submit(&mut fyo).unwrap();
    assert_eq!(outstanding(&fyo, "SalesInvoice", "SINV-1002"), "0.00");
    assert_eq!(outstanding(&fyo, "Party", "Acme"), "0.00");
}

#[test]
fn a_purchase_return_is_refunded_by_the_supplier() {
    let mut fyo = payments();
    let mut doc = invoice(
        &mut fyo,
        "PurchaseInvoice",
        "Parts Co",
        DocValueMap::new(),
        &[(2.0, DocValueMap::new())],
    );
    doc.sync(&mut fyo).unwrap();
    doc.submit(&mut fyo).unwrap();
    pay(&mut fyo, &doc);

    let mut ret = get_return_doc(&doc, &fyo).unwrap().unwrap();
    assert_eq!(quantities(&ret), vec![-2.0]);
    ret.sync(&mut fyo).unwrap();
    ret.submit(&mut fyo).unwrap();
    let name = ret.name().unwrap().to_owned();
    assert_eq!(
        postings(&fyo, &name)[0],
        posting("Creditors", "236.00", "0.00")
    );

    let ret = fyo.get_doc("PurchaseInvoice", Some(&name)).unwrap();
    let mut refund = get_payment(&ret, &fyo).unwrap().unwrap();
    assert_eq!(refund.get("paymentType"), Some(&text("Receive")));
    assert_eq!(money(&refund, "amount"), "236.00");
    refund.sync(&mut fyo).unwrap();
    refund.submit(&mut fyo).unwrap();
    assert_eq!(outstanding(&fyo, "PurchaseInvoice", &name), "0.00");
    assert_eq!(outstanding(&fyo, "Party", "Parts Co"), "0.00");
}
