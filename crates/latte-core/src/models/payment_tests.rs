use latte_db::{DocValue, DocValueMap, Filter, GetAllOptions, QueryFilter};

use crate::models::base_models::invoice::get_payment;
use crate::models::invoice_tests::{
    books, insert, invoice, outstanding, posting, postings, sales_invoice, set_accounting_settings,
};
use crate::tests::{map, money, text};
use crate::{Doc, Error, Fyo};

/// The invoice test company, with cash and bank accounts and the Cash and Bank payment
/// methods.
fn payments() -> Fyo {
    let mut fyo = books();
    let cash = map([("name", text("Cash")), ("accountType", text("Cash"))]);
    fyo.db.update("Account", &cash).unwrap();
    // Made after the Bank Accounts group, which has the Bank type too, so it's listed first.
    let bank = map([
        ("name", text("Bank")),
        ("rootType", text("Asset")),
        ("accountType", text("Bank")),
        ("isGroup", DocValue::Bool(false)),
        ("lft", DocValue::Number(21.0)),
        ("rgt", DocValue::Number(22.0)),
        ("createdBy", text("Admin")),
        ("modifiedBy", text("Admin")),
        ("created", text("2025-01-02T00:00:00.000Z")),
        ("modified", text("2025-01-02T00:00:00.000Z")),
    ]);
    fyo.db.insert("Account", &bank).unwrap();
    let write_off = map([
        ("name", text("Write Off")),
        ("rootType", text("Expense")),
        ("isGroup", DocValue::Bool(false)),
        ("lft", DocValue::Number(23.0)),
        ("rgt", DocValue::Number(24.0)),
    ]);
    insert(&mut fyo, "Account", write_off);
    set_accounting_settings(&mut fyo, map([("writeOffAccount", text("Write Off"))]));
    for name in ["Cash", "Bank"] {
        let method = map([("name", text(name)), ("type", text(name))]);
        insert(&mut fyo, "PaymentMethod", method);
    }
    fyo
}

/// A submitted sales invoice for `quantity` Widgets with 18% GST, for Acme.
fn submitted_invoice(fyo: &mut Fyo, quantity: f64) -> Doc {
    let mut doc = sales_invoice(fyo, quantity);
    doc.sync(fyo).unwrap();
    doc.submit(fyo).unwrap();
    doc
}

/// A new payment made by hand, as in the form: Acme, then a row per invoice, then the
/// amount if given.
fn payment_for(fyo: &mut Fyo, invoices: &[&str], amount: Option<&str>) -> Doc {
    let mut doc = fyo
        .get_new_doc("Payment", map([("party", text("Acme"))]))
        .unwrap();
    add_rows(fyo, &mut doc, invoices);
    if let Some(amount) = amount {
        let amount = fyo.pesa().parse(amount).unwrap();
        doc.set(fyo, "amount", DocValue::Money(amount)).unwrap();
    }
    doc
}

fn add_rows(fyo: &mut Fyo, doc: &mut Doc, invoices: &[&str]) {
    for (idx, name) in invoices.iter().enumerate() {
        doc.append(fyo, "for", DocValueMap::new()).unwrap();
        doc.set_child(fyo, "for", idx, "referenceName", text(name))
            .unwrap();
    }
}

fn validation_message(result: Result<impl std::fmt::Debug, Error>) -> String {
    match result {
        Err(Error::Validation(message)) => message,
        result => panic!("expected a validation error, got {result:?}"),
    }
}

#[test]
fn paying_an_invoice_in_full_posts_it_and_clears_what_is_owed() {
    let mut fyo = payments();
    let invoice = submitted_invoice(&mut fyo, 3.0);

    let mut payment = get_payment(&invoice, &fyo).unwrap().unwrap();
    assert_eq!(payment.get("paymentType"), Some(&text("Receive")));
    assert_eq!(payment.get("account"), Some(&text("Debtors")));
    assert_eq!(money(&payment, "amount"), "354.00");
    payment.sync(&mut fyo).unwrap();
    assert_eq!(payment.name(), Some("PAY-1001"));
    assert_eq!(payment.get("paymentAccount"), Some(&text("Cash")));
    assert_eq!(payment.get("paymentMethod"), Some(&text("Cash")));
    payment.submit(&mut fyo).unwrap();

    assert_eq!(
        postings(&fyo, "PAY-1001"),
        vec![
            posting("Cash", "354.00", "0.00"),
            posting("Debtors", "0.00", "354.00"),
        ]
    );
    assert_eq!(outstanding(&fyo, "SalesInvoice", "SINV-1001"), "0.00");
    assert_eq!(outstanding(&fyo, "Party", "Acme"), "0.00");
}

#[test]
fn a_payment_made_by_hand_takes_its_invoice_party_and_amount() {
    let mut fyo = payments();
    submitted_invoice(&mut fyo, 3.0);

    let mut payment = fyo.get_new_doc("Payment", DocValueMap::new()).unwrap();
    add_rows(&mut fyo, &mut payment, &["SINV-1001"]);
    let row = &payment.table("for")[0];
    assert_eq!(row.get("referenceType"), Some(&text("SalesInvoice")));
    assert_eq!(money(row, "amount"), "354.00");
    assert_eq!(money(&payment, "amount"), "354.00");
    assert_eq!(payment.get("party"), Some(&text("Acme")));
    assert_eq!(payment.get("paymentType"), Some(&text("Receive")));
}

#[test]
fn a_purchase_invoice_is_paid_from_cash_to_the_creditors() {
    let mut fyo = payments();
    let mut invoice = invoice(
        &mut fyo,
        "PurchaseInvoice",
        "Parts Co",
        DocValueMap::new(),
        &[(2.0, DocValueMap::new())],
    );
    invoice.sync(&mut fyo).unwrap();
    invoice.submit(&mut fyo).unwrap();

    let mut payment = get_payment(&invoice, &fyo).unwrap().unwrap();
    assert_eq!(payment.get("paymentType"), Some(&text("Pay")));
    assert_eq!(payment.get("paymentAccount"), Some(&text("Creditors")));
    payment.sync(&mut fyo).unwrap();
    assert_eq!(payment.get("account"), Some(&text("Cash")));
    payment.submit(&mut fyo).unwrap();

    assert_eq!(
        postings(&fyo, "PAY-1001"),
        vec![
            posting("Creditors", "236.00", "0.00"),
            posting("Cash", "0.00", "236.00"),
        ]
    );
    assert_eq!(outstanding(&fyo, "PurchaseInvoice", "PINV-1001"), "0.00");
    assert_eq!(outstanding(&fyo, "Party", "Parts Co"), "0.00");
}

#[test]
fn a_partial_payment_needs_partial_payments_on() {
    let mut fyo = payments();
    submitted_invoice(&mut fyo, 3.0);

    let mut payment = payment_for(&mut fyo, &["SINV-1001"], Some("100"));
    assert_eq!(money(&payment.table("for")[0], "amount"), "100.00");
    let message = validation_message(payment.sync(&mut fyo));
    assert_eq!(message, "Enable Partial payment to pay partial amount");
    assert_eq!(money(&payment, "amount"), "354.00");

    set_accounting_settings(
        &mut fyo,
        map([("enablePartialPayment", DocValue::Bool(true))]),
    );
    let mut payment = payment_for(&mut fyo, &["SINV-1001"], Some("100"));
    payment.sync(&mut fyo).unwrap();
    payment.submit(&mut fyo).unwrap();
    assert_eq!(outstanding(&fyo, "SalesInvoice", "SINV-1001"), "254.00");
    assert_eq!(outstanding(&fyo, "Party", "Acme"), "254.00");
}

#[test]
fn a_payment_cant_be_more_than_what_is_outstanding() {
    let mut fyo = payments();
    submitted_invoice(&mut fyo, 3.0);

    let mut payment = payment_for(&mut fyo, &["SINV-1001"], None);
    let amount = fyo.pesa().parse("400").unwrap();
    let message = validation_message(payment.set(&mut fyo, "amount", DocValue::Money(amount)));
    assert_eq!(message, "Payment amount cannot exceed 354.00.");
    assert_eq!(money(&payment, "amount"), "354.00");
}

#[test]
fn one_payment_pays_two_invoices() {
    let mut fyo = payments();
    submitted_invoice(&mut fyo, 3.0);
    submitted_invoice(&mut fyo, 1.0);

    // Books keeps only the last invoice's 118 to check the amount against, so it refuses
    // this payment of 472.
    let mut payment = payment_for(&mut fyo, &["SINV-1001", "SINV-1002"], None);
    assert_eq!(money(&payment, "amount"), "472.00");
    payment.sync(&mut fyo).unwrap();
    payment.submit(&mut fyo).unwrap();

    assert_eq!(outstanding(&fyo, "SalesInvoice", "SINV-1001"), "0.00");
    assert_eq!(outstanding(&fyo, "SalesInvoice", "SINV-1002"), "0.00");
    assert_eq!(outstanding(&fyo, "Party", "Acme"), "0.00");
}

#[test]
fn cancelling_a_payment_puts_back_what_is_owed() {
    let mut fyo = payments();
    let invoice = submitted_invoice(&mut fyo, 3.0);
    let mut payment = get_payment(&invoice, &fyo).unwrap().unwrap();
    payment.sync(&mut fyo).unwrap();
    payment.submit(&mut fyo).unwrap();

    // Loaded again, as after reopening the file. Books checks the amount against what is
    // outstanding now, 0, and refuses to cancel.
    let mut payment = fyo.get_doc("Payment", Some("PAY-1001")).unwrap();
    payment.cancel(&mut fyo).unwrap();

    assert!(postings(&fyo, "PAY-1001").is_empty());
    assert_eq!(outstanding(&fyo, "SalesInvoice", "SINV-1001"), "354.00");
    assert_eq!(outstanding(&fyo, "Party", "Acme"), "354.00");
}

#[test]
fn cancelling_a_paid_invoice_cancels_its_payment() {
    let mut fyo = payments();
    let invoice = submitted_invoice(&mut fyo, 3.0);
    let mut payment = get_payment(&invoice, &fyo).unwrap().unwrap();
    payment.sync(&mut fyo).unwrap();
    payment.submit(&mut fyo).unwrap();

    let mut invoice = fyo.get_doc("SalesInvoice", Some("SINV-1001")).unwrap();
    invoice.cancel(&mut fyo).unwrap();

    let payment = fyo.get_doc("Payment", Some("PAY-1001")).unwrap();
    assert!(payment.is_cancelled());
    assert!(postings(&fyo, "PAY-1001").is_empty());
    assert!(postings(&fyo, "SINV-1001").is_empty());
    assert_eq!(outstanding(&fyo, "SalesInvoice", "SINV-1001"), "354.00");
    assert_eq!(outstanding(&fyo, "Party", "Acme"), "0.00");

    let mut invoice = fyo.get_doc("SalesInvoice", Some("SINV-1001")).unwrap();
    invoice.delete(&mut fyo).unwrap();
    assert!(!fyo.db.exists("Payment", Some("PAY-1001")).unwrap());
    let options = GetAllOptions {
        filters: QueryFilter::from([("referenceName".to_owned(), Filter::from("PAY-1001"))]),
        ..GetAllOptions::default()
    };
    let entries = fyo.db.get_all_docs("AccountingLedgerEntry", &options);
    assert!(entries.unwrap().is_empty());
}

#[test]
fn a_payment_is_for_its_party_invoices_only() {
    let mut fyo = payments();
    submitted_invoice(&mut fyo, 3.0);
    let mut other = fyo
        .get_new_doc(
            "Party",
            map([("name", text("Other")), ("role", text("Customer"))]),
        )
        .unwrap();
    other.sync(&mut fyo).unwrap();

    let mut payment = payment_for(&mut fyo, &["SINV-1001"], None);
    payment.set(&mut fyo, "party", text("Other")).unwrap();
    let message = validation_message(payment.sync(&mut fyo));
    assert_eq!(message, "SINV-1001 party Acme is different from Other");
}

#[test]
fn a_bank_payment_needs_its_clearance_date_and_reference() {
    let mut fyo = payments();
    submitted_invoice(&mut fyo, 3.0);

    let mut payment = payment_for(&mut fyo, &["SINV-1001"], None);
    payment
        .set(&mut fyo, "paymentMethod", text("Bank"))
        .unwrap();
    assert_eq!(payment.get("paymentAccount"), Some(&text("Bank")));
    let message = validation_message(payment.sync(&mut fyo));
    assert_eq!(message, "Clearance Date not set.");

    payment
        .set(&mut fyo, "clearanceDate", text("2025-04-02"))
        .unwrap();
    payment
        .set(&mut fyo, "referenceId", text("UTR123"))
        .unwrap();
    payment.sync(&mut fyo).unwrap();
}

fn set_money(fyo: &mut Fyo, doc: &mut Doc, fieldname: &str, amount: &str) -> Result<bool, Error> {
    let amount = fyo.pesa().parse(amount).unwrap();
    doc.set(fyo, fieldname, DocValue::Money(amount))
}

#[test]
fn a_write_off_on_a_receipt_is_a_loss_and_clears_the_invoice() {
    let mut fyo = payments();
    submitted_invoice(&mut fyo, 3.0);

    let mut payment = payment_for(&mut fyo, &["SINV-1001"], None);
    set_money(&mut fyo, &mut payment, "writeoff", "4").unwrap();
    assert_eq!(money(&payment, "amountPaid"), "350.00");
    payment.sync(&mut fyo).unwrap();
    payment.submit(&mut fyo).unwrap();

    // Books posts Cash 354 and Debtors 354 less 4, with the 4 credited to Write Off.
    assert_eq!(
        postings(&fyo, "PAY-1001"),
        vec![
            posting("Cash", "350.00", "0.00"),
            posting("Debtors", "0.00", "354.00"),
            posting("Write Off", "4.00", "0.00"),
        ]
    );
    assert_eq!(outstanding(&fyo, "SalesInvoice", "SINV-1001"), "0.00");
    assert_eq!(outstanding(&fyo, "Party", "Acme"), "0.00");

    payment.cancel(&mut fyo).unwrap();
    assert!(postings(&fyo, "PAY-1001").is_empty());
    assert_eq!(outstanding(&fyo, "SalesInvoice", "SINV-1001"), "354.00");
}

#[test]
fn a_write_off_on_a_payment_to_a_supplier_is_a_gain() {
    let mut fyo = payments();
    let mut invoice = invoice(
        &mut fyo,
        "PurchaseInvoice",
        "Parts Co",
        DocValueMap::new(),
        &[(2.0, DocValueMap::new())],
    );
    invoice.sync(&mut fyo).unwrap();
    invoice.submit(&mut fyo).unwrap();

    let mut payment = get_payment(&invoice, &fyo).unwrap().unwrap();
    set_money(&mut fyo, &mut payment, "writeoff", "6").unwrap();
    payment.sync(&mut fyo).unwrap();
    payment.submit(&mut fyo).unwrap();

    assert_eq!(
        postings(&fyo, "PAY-1001"),
        vec![
            posting("Creditors", "236.00", "0.00"),
            posting("Cash", "0.00", "230.00"),
            posting("Write Off", "0.00", "6.00"),
        ]
    );
    assert_eq!(outstanding(&fyo, "PurchaseInvoice", "PINV-1001"), "0.00");
    assert_eq!(outstanding(&fyo, "Party", "Parts Co"), "0.00");
}

#[test]
fn a_write_off_needs_the_write_off_account() {
    let mut fyo = payments();
    submitted_invoice(&mut fyo, 3.0);
    set_accounting_settings(&mut fyo, map([("writeOffAccount", text("Gone"))]));

    let mut payment = payment_for(&mut fyo, &["SINV-1001"], None);
    set_money(&mut fyo, &mut payment, "writeoff", "4").unwrap();
    let error = payment.sync(&mut fyo).unwrap_err();
    assert!(matches!(error, Error::NotFound(_)), "{error:?}");
    assert_eq!(
        error.to_string(),
        "Write Off Account Gone does not exist. Please set Write Off Account in General Settings"
    );
}

#[test]
fn a_write_off_is_no_less_than_zero_and_no_more_than_the_amount() {
    let mut fyo = payments();
    submitted_invoice(&mut fyo, 3.0);

    let mut payment = payment_for(&mut fyo, &["SINV-1001"], None);
    set_money(&mut fyo, &mut payment, "writeoff", "-1").unwrap();
    let message = validation_message(payment.sync(&mut fyo));
    assert_eq!(message, "Write Off cannot be less than zero.");

    set_money(&mut fyo, &mut payment, "writeoff", "400").unwrap();
    let message = validation_message(payment.sync(&mut fyo));
    assert_eq!(
        message,
        "Write Off cannot exceed the payment amount 354.00."
    );
}

#[test]
fn a_write_off_doesnt_make_a_partial_payment_whole() {
    let mut fyo = payments();
    submitted_invoice(&mut fyo, 3.0);

    // Books saves this, and the invoice is left owing 4.
    let mut payment = payment_for(&mut fyo, &["SINV-1001"], Some("350"));
    set_money(&mut fyo, &mut payment, "writeoff", "4").unwrap();
    let message = validation_message(payment.sync(&mut fyo));
    assert_eq!(message, "Enable Partial payment to pay partial amount");
}

#[test]
fn the_amount_alone_covers_what_is_allocated_to_the_invoices() {
    let mut fyo = payments();
    set_accounting_settings(
        &mut fyo,
        map([("enablePartialPayment", DocValue::Bool(true))]),
    );
    submitted_invoice(&mut fyo, 3.0);
    submitted_invoice(&mut fyo, 1.0);

    // Books lets the 72 written off make up the difference, and takes 472 off the
    // invoices while crediting the party with 400.
    let mut payment = payment_for(&mut fyo, &["SINV-1001", "SINV-1002"], None);
    set_money(&mut fyo, &mut payment, "amount", "400").unwrap();
    set_money(&mut fyo, &mut payment, "writeoff", "72").unwrap();
    let message = validation_message(payment.sync(&mut fyo));
    assert_eq!(
        message,
        "Amount: 400.00 is less than the total amount allocated to references: 472.00."
    );
}

#[test]
fn an_invoice_set_to_make_its_payment_pays_itself_on_submit() {
    let mut fyo = payments();
    let defaults = map([("salesPaymentAccount", text("Bank"))]);
    fyo.db.update("Defaults", &defaults).unwrap();

    let mut invoice = sales_invoice(&mut fyo, 3.0);
    assert_eq!(invoice.get("makeAutoPayment"), Some(&DocValue::Bool(true)));
    invoice.sync(&mut fyo).unwrap();
    invoice.submit(&mut fyo).unwrap();

    assert_eq!(money(&invoice, "outstandingAmount"), "0.00");
    assert_eq!(
        postings(&fyo, "PAY-1001"),
        vec![
            posting("Bank", "354.00", "0.00"),
            posting("Debtors", "0.00", "354.00"),
        ]
    );
}
