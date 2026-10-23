use std::collections::HashMap;

use latte_db::{DocValue, DocValueMap, Filter, GetAllOptions, Order, QueryFilter};

use crate::models::models;
use crate::models::transactional::ledger_posting::LedgerPosting;
use crate::tests::{company, map, money, text};
use crate::{Doc, Fyo};

const CREATED: &str = "2025-01-01T00:00:00.000Z";

/// The test company with what invoices need: receivable, payable, tax, round-off,
/// discount and cost accounts; a customer, a supplier, an item and an 18% GST template.
pub(super) fn books() -> Fyo {
    let mut fyo = company();
    fyo.register_models(models(), HashMap::new());

    for (idx, (name, root_type, account_type)) in [
        ("Debtors", "Asset", "Receivable"),
        ("Creditors", "Liability", "Payable"),
        ("CGST", "Liability", "Tax"),
        ("SGST", "Liability", "Tax"),
        ("Round Off", "Expense", "Round Off"),
        ("Discounts", "Expense", ""),
        ("Cost of Goods Sold", "Expense", "Cost of Goods Sold"),
    ]
    .into_iter()
    .enumerate()
    {
        let lft = 7.0 + 2.0 * idx as f64;
        let account = map([
            ("name", text(name)),
            ("rootType", text(root_type)),
            ("accountType", text(account_type)),
            ("isGroup", DocValue::Bool(false)),
            ("lft", DocValue::Number(lft)),
            ("rgt", DocValue::Number(lft + 1.0)),
            ("createdBy", text("Admin")),
            ("modifiedBy", text("Admin")),
            ("created", text(CREATED)),
            ("modified", text(CREATED)),
        ]);
        fyo.db.insert("Account", &account).unwrap();
    }
    for (schema_name, name) in [("Currency", "INR"), ("Currency", "USD"), ("UOM", "Unit")] {
        let doc = map([
            ("name", text(name)),
            ("createdBy", text("Admin")),
            ("modifiedBy", text("Admin")),
            ("created", text(CREATED)),
            ("modified", text(CREATED)),
        ]);
        fyo.db.insert(schema_name, &doc).unwrap();
    }
    set_accounting_settings(&mut fyo, map([("roundOffAccount", text("Round Off"))]));

    for (name, role) in [("Acme", "Customer"), ("Parts Co", "Supplier")] {
        let mut party = fyo
            .get_new_doc("Party", map([("name", text(name)), ("role", text(role))]))
            .unwrap();
        party.sync(&mut fyo).unwrap();
    }

    let mut item = fyo
        .get_new_doc(
            "Item",
            map([("name", text("Widget")), ("rate", DocValue::Number(100.0))]),
        )
        .unwrap();
    item.sync(&mut fyo).unwrap();

    let detail = |account: &str| map([("account", text(account)), ("rate", DocValue::Number(9.0))]);
    let mut tax = fyo
        .get_new_doc(
            "Tax",
            map([
                ("name", text("GST-18")),
                (
                    "details",
                    DocValue::Table(vec![detail("CGST"), detail("SGST")]),
                ),
            ]),
        )
        .unwrap();
    tax.sync(&mut fyo).unwrap();
    fyo
}

/// Writes settings as they'd be saved, without the company details the form needs too.
pub(super) fn set_accounting_settings(fyo: &mut Fyo, values: DocValueMap) {
    fyo.db.update("AccountingSettings", &values).unwrap();
}

/// A new invoice for `party`, dated 2025-04-01, with the values set as the form sets
/// them, then a row per `(quantity, values)`, each for a Widget with the 18% GST.
pub(super) fn invoice(
    fyo: &mut Fyo,
    schema_name: &str,
    party: &str,
    values: DocValueMap,
    rows: &[(f64, DocValueMap)],
) -> Doc {
    let mut doc = fyo
        .get_new_doc(schema_name, map([("date", text("2025-04-01"))]))
        .unwrap();
    doc.set(fyo, "party", text(party)).unwrap();
    for (fieldname, value) in values {
        doc.set(fyo, &fieldname, value).unwrap();
    }

    for (idx, (quantity, values)) in rows.iter().enumerate() {
        doc.append(fyo, "items", DocValueMap::new()).unwrap();
        doc.set_child(fyo, "items", idx, "item", text("Widget"))
            .unwrap();
        doc.set_child(fyo, "items", idx, "quantity", DocValue::Number(*quantity))
            .unwrap();
        doc.set_child(fyo, "items", idx, "tax", text("GST-18"))
            .unwrap();
        for (fieldname, value) in values {
            doc.set_child(fyo, "items", idx, fieldname, value.clone())
                .unwrap();
        }
    }
    doc
}

pub(super) fn sales_invoice(fyo: &mut Fyo, quantity: f64) -> Doc {
    invoice(
        fyo,
        "SalesInvoice",
        "Acme",
        DocValueMap::new(),
        &[(quantity, DocValueMap::new())],
    )
}

/// The document's ledger entries that aren't reverted, oldest first, as
/// `(account, debit, credit)`.
pub(super) fn postings(fyo: &Fyo, name: &str) -> Vec<(String, String, String)> {
    let options = GetAllOptions {
        fields: Some(vec!["*".to_owned()]),
        filters: QueryFilter::from([
            ("referenceName".to_owned(), Filter::from(name)),
            ("reverted".to_owned(), Filter::from(false)),
        ]),
        order_by: Some(vec!["name".to_owned()]),
        order: Some(Order::Asc),
        ..GetAllOptions::default()
    };
    let amount = |entry: &DocValueMap, fieldname| match entry.get(fieldname) {
        Some(DocValue::Money(money)) => money.round(None),
        value => panic!("{fieldname} is {value:?}"),
    };
    fyo.db
        .get_all_docs("AccountingLedgerEntry", &options)
        .unwrap()
        .iter()
        .map(|entry| {
            let Some(DocValue::Text(account)) = entry.get("account") else {
                panic!("{entry:?} has no account");
            };
            (
                account.clone(),
                amount(entry, "debit"),
                amount(entry, "credit"),
            )
        })
        .collect()
}

pub(super) fn posting(account: &str, debit: &str, credit: &str) -> (String, String, String) {
    (account.to_owned(), debit.to_owned(), credit.to_owned())
}

pub(super) fn outstanding(fyo: &Fyo, schema_name: &str, name: &str) -> String {
    match fyo.get_value(schema_name, Some(name), "outstandingAmount") {
        Some(DocValue::Money(money)) => money.round(None),
        value => panic!("outstandingAmount is {value:?}"),
    }
}

#[test]
fn parties_and_items_take_their_accounts() {
    let fyo = books();
    let customer = fyo.get_value("Party", Some("Acme"), "defaultAccount");
    assert_eq!(customer, Some(text("Debtors")));
    let supplier = fyo.get_value("Party", Some("Parts Co"), "defaultAccount");
    assert_eq!(supplier, Some(text("Creditors")));
    assert_eq!(
        fyo.get_value("Party", Some("Acme"), "currency"),
        Some(text("INR"))
    );

    assert_eq!(
        fyo.get_value("Item", Some("Widget"), "incomeAccount"),
        Some(text("Sales"))
    );
    assert_eq!(
        fyo.get_value("Item", Some("Widget"), "expenseAccount"),
        Some(text("Cost of Goods Sold"))
    );
}

#[test]
fn an_invoice_totals_its_rows_and_taxes() {
    let mut fyo = books();
    let doc = sales_invoice(&mut fyo, 3.0);

    assert_eq!(doc.get("account"), Some(&text("Debtors")));
    assert_eq!(doc.get("currency"), Some(&text("INR")));
    assert_eq!(doc.get("exchangeRate"), Some(&DocValue::Number(1.0)));
    assert_eq!(doc.get("numberSeries"), Some(&text("SINV-")));

    let row = &doc.table("items")[0];
    assert_eq!(row.get("account"), Some(&text("Sales")));
    assert_eq!(money(row, "rate"), "100.00");
    assert_eq!(money(row, "amount"), "300.00");

    assert_eq!(money(&doc, "netTotal"), "300.00");
    let taxes = doc.table("taxes");
    assert_eq!(taxes.len(), 2);
    assert_eq!(taxes[0].get("account"), Some(&text("CGST")));
    assert_eq!(taxes[0].get("rate"), Some(&DocValue::Number(9.0)));
    assert_eq!(money(&taxes[0], "amount"), "27.00");
    assert_eq!(taxes[1].get("account"), Some(&text("SGST")));
    assert_eq!(money(&taxes[1], "amount"), "27.00");
    assert_eq!(money(&doc, "grandTotal"), "354.00");
    assert_eq!(money(&doc, "baseGrandTotal"), "354.00");
    assert_eq!(money(&doc, "outstandingAmount"), "354.00");
}

#[test]
fn submitting_a_sales_invoice_posts_it_and_raises_what_the_party_owes() {
    let mut fyo = books();
    let mut doc = sales_invoice(&mut fyo, 3.0);
    doc.sync(&mut fyo).unwrap();
    assert_eq!(doc.name(), Some("SINV-1001"));
    doc.submit(&mut fyo).unwrap();

    assert_eq!(
        postings(&fyo, "SINV-1001"),
        vec![
            posting("Debtors", "354.00", "0.00"),
            posting("Sales", "0.00", "300.00"),
            posting("CGST", "0.00", "27.00"),
            posting("SGST", "0.00", "27.00"),
        ]
    );
    assert_eq!(outstanding(&fyo, "SalesInvoice", "SINV-1001"), "354.00");
    assert_eq!(outstanding(&fyo, "Party", "Acme"), "354.00");

    let mut second = sales_invoice(&mut fyo, 1.0);
    second.sync(&mut fyo).unwrap();
    second.submit(&mut fyo).unwrap();
    assert_eq!(outstanding(&fyo, "Party", "Acme"), "472.00");
}

#[test]
fn cancelling_an_invoice_reverts_its_postings_and_what_the_party_owes() {
    let mut fyo = books();
    let mut doc = sales_invoice(&mut fyo, 3.0);
    doc.sync(&mut fyo).unwrap();
    doc.submit(&mut fyo).unwrap();
    doc.cancel(&mut fyo).unwrap();

    assert!(postings(&fyo, "SINV-1001").is_empty());
    assert_eq!(outstanding(&fyo, "Party", "Acme"), "0.00");

    doc.delete(&mut fyo).unwrap();
    let options = GetAllOptions {
        filters: QueryFilter::from([("referenceName".to_owned(), Filter::from("SINV-1001"))]),
        ..GetAllOptions::default()
    };
    let entries = fyo.db.get_all_docs("AccountingLedgerEntry", &options);
    assert!(entries.unwrap().is_empty());
}

#[test]
fn a_purchase_invoice_posts_the_other_way() {
    let mut fyo = books();
    let mut doc = invoice(
        &mut fyo,
        "PurchaseInvoice",
        "Parts Co",
        DocValueMap::new(),
        &[(2.0, DocValueMap::new())],
    );
    assert_eq!(doc.get("account"), Some(&text("Creditors")));
    assert_eq!(doc.get("numberSeries"), Some(&text("PINV-")));
    doc.sync(&mut fyo).unwrap();
    doc.submit(&mut fyo).unwrap();

    assert_eq!(
        postings(&fyo, "PINV-1001"),
        vec![
            posting("Creditors", "0.00", "236.00"),
            posting("Cost of Goods Sold", "200.00", "0.00"),
            posting("CGST", "18.00", "0.00"),
            posting("SGST", "18.00", "0.00"),
        ]
    );
    assert_eq!(outstanding(&fyo, "Party", "Parts Co"), "236.00");
}

#[test]
fn discounts_come_off_before_tax_and_post_to_the_discount_account() {
    let mut fyo = books();
    set_accounting_settings(
        &mut fyo,
        map([
            ("enableDiscounting", DocValue::Bool(true)),
            ("discountAccount", text("Discounts")),
        ]),
    );
    let mut doc = invoice(
        &mut fyo,
        "SalesInvoice",
        "Acme",
        DocValueMap::new(),
        &[(3.0, map([("itemDiscountPercent", DocValue::Number(10.0))]))],
    );

    let row = &doc.table("items")[0];
    assert_eq!(money(row, "amount"), "300.00");
    // Books shows the discount itself (30) and the taxed total from before the discount
    // was set (354).
    assert_eq!(money(row, "itemDiscountedTotal"), "270.00");
    assert_eq!(money(row, "itemTaxedTotal"), "318.60");
    assert_eq!(money(&doc.table("taxes")[0], "amount"), "24.30");
    assert_eq!(money(&doc, "grandTotal"), "318.60");

    doc.sync(&mut fyo).unwrap();
    doc.submit(&mut fyo).unwrap();
    assert_eq!(
        postings(&fyo, "SINV-1001"),
        vec![
            posting("Debtors", "318.60", "0.00"),
            posting("Sales", "0.00", "300.00"),
            posting("CGST", "0.00", "24.30"),
            posting("SGST", "0.00", "24.30"),
            posting("Discounts", "30.00", "0.00"),
        ]
    );
}

/// Inserts a document as it'd be saved, with its meta values.
pub(super) fn insert(fyo: &mut Fyo, schema_name: &str, values: DocValueMap) {
    let mut values = values;
    for fieldname in ["createdBy", "modifiedBy"] {
        values.insert(fieldname.to_owned(), text("Admin"));
    }
    for fieldname in ["created", "modified"] {
        values.insert(fieldname.to_owned(), text(CREATED));
    }
    fyo.db.insert(schema_name, &values).unwrap();
}

#[test]
fn a_whole_number_unit_rounds_the_quantity() {
    let mut fyo = books();
    let unit = map([("name", text("Unit")), ("isWhole", DocValue::Bool(true))]);
    fyo.db.update("UOM", &unit).unwrap();

    let doc = sales_invoice(&mut fyo, 2.5);
    let row = &doc.table("items")[0];
    // Books looks for the unit in the item's `uom`, which Item doesn't have.
    assert_eq!(row.get("quantity"), Some(&DocValue::Number(3.0)));
    assert_eq!(money(row, "amount"), "300.00");
}

#[test]
fn a_row_keeps_a_transfer_unit_the_new_item_converts_to_too() {
    let mut fyo = books();
    insert(&mut fyo, "UOM", map([("name", text("Box"))]));
    for (name, per_box) in [("Bolts", 12.0), ("Nuts", 50.0)] {
        let mut item = fyo
            .get_new_doc(
                "Item",
                map([
                    ("name", text(name)),
                    ("rate", DocValue::Number(10.0)),
                    (
                        "uomConversions",
                        DocValue::Table(vec![map([
                            ("uom", text("Box")),
                            ("conversionFactor", DocValue::Number(per_box)),
                        ])]),
                    ),
                ]),
            )
            .unwrap();
        item.sync(&mut fyo).unwrap();
    }

    let mut doc = fyo.get_new_doc("SalesInvoice", DocValueMap::new()).unwrap();
    doc.set(&mut fyo, "party", text("Acme")).unwrap();
    doc.append(&mut fyo, "items", DocValueMap::new()).unwrap();
    doc.set_child(&mut fyo, "items", 0, "item", text("Bolts"))
        .unwrap();
    doc.set_child(&mut fyo, "items", 0, "transferUnit", text("Box"))
        .unwrap();
    let row = &doc.table("items")[0];
    assert_eq!(row.get("transferUnit"), Some(&text("Box")));
    assert_eq!(
        row.get("unitConversionFactor"),
        Some(&DocValue::Number(12.0))
    );

    doc.set_child(&mut fyo, "items", 0, "item", text("Nuts"))
        .unwrap();
    let row = &doc.table("items")[0];
    // Books' check is the wrong way round, so a new item always puts the unit back.
    assert_eq!(row.get("transferUnit"), Some(&text("Box")));
}

#[test]
fn what_isnt_shipped_takes_off_every_shipped_row() {
    let mut fyo = books();
    insert(&mut fyo, "Location", map([("name", text("Stores"))]));
    let mut item = fyo
        .get_new_doc(
            "Item",
            map([
                ("name", text("Gadget")),
                ("rate", DocValue::Number(50.0)),
                ("trackItem", DocValue::Bool(true)),
                ("expenseAccount", text("Cost of Goods Sold")),
            ]),
        )
        .unwrap();
    item.sync(&mut fyo).unwrap();

    insert(
        &mut fyo,
        "NumberSeries",
        map([
            ("name", text("SHPM-")),
            ("start", DocValue::Number(1001.0)),
            ("padZeros", DocValue::Number(4.0)),
            ("current", DocValue::Number(1001.0)),
            ("referenceType", text("Shipment")),
        ]),
    );
    let shipped = |quantity| {
        map([
            ("item", text("Gadget")),
            ("location", text("Stores")),
            ("transferQuantity", DocValue::Number(quantity)),
            ("quantity", DocValue::Number(quantity)),
            ("unitConversionFactor", DocValue::Number(1.0)),
            ("rate", DocValue::Number(50.0)),
        ])
    };
    insert(
        &mut fyo,
        "Shipment",
        map([
            ("name", text("SHPM-1001")),
            ("numberSeries", text("SHPM-")),
            ("party", text("Acme")),
            ("date", text("2025-04-01")),
            ("submitted", DocValue::Bool(true)),
            ("cancelled", DocValue::Bool(false)),
            ("isReturned", DocValue::Bool(false)),
            ("items", DocValue::Table(vec![shipped(2.0), shipped(3.0)])),
        ]),
    );

    let mut doc = fyo.get_new_doc("SalesInvoice", DocValueMap::new()).unwrap();
    doc.set(&mut fyo, "party", text("Acme")).unwrap();
    doc.set(&mut fyo, "backReference", text("SHPM-1001"))
        .unwrap();
    doc.append(&mut fyo, "items", DocValueMap::new()).unwrap();
    doc.set_child(&mut fyo, "items", 0, "item", text("Gadget"))
        .unwrap();
    doc.set_child(&mut fyo, "items", 0, "quantity", DocValue::Number(8.0))
        .unwrap();
    let row = &doc.table("items")[0];
    // Books takes off only the last row's 3.
    assert_eq!(row.get("stockNotTransferred"), Some(&DocValue::Number(3.0)));
}

#[test]
fn discounting_needs_a_discount_account() {
    let mut fyo = books();
    set_accounting_settings(&mut fyo, map([("enableDiscounting", DocValue::Bool(true))]));
    let mut doc = sales_invoice(&mut fyo, 1.0);
    let error = doc.sync(&mut fyo).unwrap_err();
    assert_eq!(error.to_string(), "Discount Account is not set.");
}

#[test]
fn a_foreign_currency_invoice_posts_in_the_company_currency() {
    let mut fyo = books();
    let mut party = fyo
        .get_new_doc(
            "Party",
            map([
                ("name", text("Globex")),
                ("role", text("Customer")),
                ("currency", text("USD")),
            ]),
        )
        .unwrap();
    party.sync(&mut fyo).unwrap();

    let mut doc = invoice(
        &mut fyo,
        "SalesInvoice",
        "Globex",
        map([("exchangeRate", DocValue::Number(80.0))]),
        &[(3.0, DocValueMap::new())],
    );
    assert_eq!(doc.get("currency"), Some(&text("USD")));
    assert_eq!(doc.get("exchangeRate"), Some(&DocValue::Number(80.0)));
    // The item's rate is in the company's currency.
    assert_eq!(money(&doc.table("items")[0], "rate"), "1.25");
    // 4.425, rounded half to even as pesa does.
    assert_eq!(money(&doc, "grandTotal"), "4.42");
    assert_eq!(money(&doc, "baseGrandTotal"), "354.00");

    doc.sync(&mut fyo).unwrap();
    doc.submit(&mut fyo).unwrap();
    assert_eq!(
        postings(&fyo, "SINV-1001"),
        vec![
            posting("Debtors", "354.00", "0.00"),
            posting("Sales", "0.00", "300.00"),
            posting("CGST", "0.00", "27.00"),
            posting("SGST", "0.00", "27.00"),
        ]
    );
    assert_eq!(outstanding(&fyo, "SalesInvoice", "SINV-1001"), "354.00");
}

#[test]
fn what_doesnt_balance_goes_to_the_round_off_account() {
    let mut fyo = books();
    let doc = sales_invoice(&mut fyo, 1.0);
    let pesa = |value: &str| fyo.pesa().parse(value).unwrap();
    let (debit, credit, more) = (pesa("100.01"), pesa("100"), pesa("0.02"));

    let mut posting = LedgerPosting::new(&doc);
    posting.debit(&mut fyo, "Debtors", &debit).unwrap();
    posting.credit(&mut fyo, "Sales", &credit).unwrap();
    posting.make_round_off_entry(&mut fyo).unwrap();
    let round_off = &posting.entries[2];
    assert_eq!(round_off.get("account"), Some(&text("Round Off")));
    assert_eq!(money(round_off, "credit"), "0.01");
    posting.validate(&fyo).unwrap();

    posting.credit(&mut fyo, "Sales", &more).unwrap();
    posting.make_round_off_entry(&mut fyo).unwrap();
    assert_eq!(money(&posting.entries[3], "debit"), "0.02");
    posting.validate(&fyo).unwrap();
}

#[test]
fn with_pricing_rules_on_but_none_for_its_items_an_invoice_saves_as_usual() {
    let mut fyo = books();
    set_accounting_settings(&mut fyo, map([("enablePricingRule", DocValue::Bool(true))]));
    let mut doc = sales_invoice(&mut fyo, 3.0);
    // Books' `getPricingRule` gives an empty list, which it counts as applied; Latte
    // doesn't.
    assert_eq!(
        doc.get("isPricingRuleApplied"),
        Some(&DocValue::Bool(false))
    );
    assert_eq!(money(&doc, "grandTotal"), "354.00");
    doc.sync(&mut fyo).unwrap();

    let mut purchase = invoice(
        &mut fyo,
        "PurchaseInvoice",
        "Parts Co",
        DocValueMap::new(),
        &[(1.0, DocValueMap::new())],
    );
    assert_eq!(
        purchase.get("isPricingRuleApplied"),
        None,
        "PurchaseInvoice has no such field"
    );
    purchase.sync(&mut fyo).unwrap();
}
