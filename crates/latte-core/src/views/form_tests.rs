use latte_db::{DocValue, DocValueMap};

use super::form::*;
use crate::fyo::format::Formatter;
use crate::models::base_models::invoice::get_payment;
use crate::models::invoice_tests::{books, sales_invoice, set_accounting_settings};
use crate::models::payment_tests::{payments, submitted_invoice};
use crate::tests::{map, text};
use crate::views::field;

#[test]
fn suggestions_are_fuzzy_and_closest_first() {
    let labels = ["Copperleaf Beans", "Coffee", "Cold Brew", "Tea"];
    assert_eq!(suggestions(&labels, ""), vec![0, 1, 2, 3]);
    // "cof" is in order in "Coffee" (distance 0.5 + 3) and "Copperleaf Beans".
    assert_eq!(suggestions(&labels, "cof"), vec![1, 0]);
    assert_eq!(suggestions(&labels, "Tea"), vec![3]);
    assert!(suggestions(&labels, "xyz").is_empty());
}

#[test]
fn the_exchange_rate_follows_books_control() {
    // 1 USD = 82.5 INR; swapped, 1 INR = 1/82.5 USD.
    assert_eq!(exchange_rate_right(82.5, 1.0, false), 82.5);
    assert_eq!(exchange_rate_right(80.0, 2.0, true), 0.025);
    // Typing 165 INR for 2 USD is a rate of 82.5; swapped, 0.5 USD for 40 INR is 80.
    assert_eq!(exchange_rate_from_right(165.0, 2.0, false), 82.5);
    assert_eq!(exchange_rate_from_right(0.5, 40.0, true), 80.0);
}

#[test]
fn only_a_document_in_another_currency_shows_the_rate() {
    let mut fyo = books();
    let mut doc = sales_invoice(&mut fyo, 1.0);
    assert_eq!(exchange_rate(&fyo, &doc), None);
    doc.set_value("currency", text("USD"));
    doc.set_value("exchangeRate", DocValue::Number(82.0));
    assert_eq!(
        exchange_rate(&fyo, &doc),
        Some(("USD".to_owned(), "INR".to_owned(), 82.0))
    );
}

#[test]
fn ctrl_s_warns_of_a_zero_grand_total() {
    let mut fyo = books();
    let formatter = Formatter::new(&fyo).unwrap();
    let doc = fyo.get_new_doc("SalesInvoice", DocValueMap::new()).unwrap();
    assert_eq!(
        sync_message(&formatter, &doc),
        "Create new Sales Invoice entry? Entry has Grand Total 0.00. Please verify amounts."
    );
    let mut doc = sales_invoice(&mut fyo, 1.0);
    assert_eq!(
        sync_message(&formatter, &doc),
        "Create new Sales Invoice entry?"
    );
    doc.sync(&mut fyo).unwrap();
    let name = doc.name().unwrap().to_owned();
    assert_eq!(
        sync_message(&formatter, &doc),
        format!("Save changes made to {name}?")
    );
    assert_eq!(cannot_save_message(&doc), format!("{name} already saved"));
    doc.submit(&mut fyo).unwrap();
    assert_eq!(
        cannot_save_message(&doc),
        format!("{name} already submitted")
    );
}

#[test]
fn the_submit_dialog_says_which_payment_is_made() {
    let mut fyo = payments();
    let formatter = Formatter::new(&fyo).unwrap();
    let mut doc = sales_invoice(&mut fyo, 1.0);
    assert_eq!(
        submit_message(&fyo, &formatter, &doc),
        "Mark Sales Invoice as submitted?"
    );
    fyo.db
        .update("Defaults", &map([("salesPaymentAccount", text("Cash"))]))
        .unwrap();
    doc.set(&mut fyo, "makeAutoPayment", DocValue::Bool(true))
        .unwrap();
    doc.sync(&mut fyo).unwrap();
    assert_eq!(
        submit_message(&fyo, &formatter, &doc),
        "Mark Sales Invoice as submitted? Payment of 118.00 will be made from account \
         \"Debtors\" to account \"Cash\" on Submit."
    );
}

#[test]
fn a_sales_invoice_for_more_than_the_stock_is_short() {
    let mut fyo = books();
    let doc = sales_invoice(&mut fyo, 3.0);
    // Inventory off: nothing is checked.
    assert!(insufficient_quantity(&fyo, &doc).unwrap().is_empty());
    set_accounting_settings(&mut fyo, map([("enableInventory", DocValue::Bool(true))]));
    // Widget isn't tracked.
    assert!(insufficient_quantity(&fyo, &doc).unwrap().is_empty());
    fyo.db
        .update(
            "Item",
            &map([
                ("name", text("Widget")),
                ("trackItem", DocValue::Bool(true)),
            ]),
        )
        .unwrap();
    let short = insufficient_quantity(&fyo, &doc).unwrap();
    assert_eq!(short, vec![("Widget".to_owned(), 3.0)]);
    assert_eq!(
        insufficient_quantity_detail(&short),
        [
            "The following items have insufficient quantity for Shipment: Widget (3)".to_owned(),
            "Continue submitting Sales Invoice?".to_owned()
        ]
    );
}

#[test]
fn linked_entries_list_payments_and_ledger_entries() {
    let mut fyo = payments();
    let formatter = Formatter::new(&fyo).unwrap();
    let invoice = submitted_invoice(&mut fyo, 1.0);
    let mut payment = get_payment(&invoice, &fyo).unwrap().unwrap();
    payment.sync(&mut fyo).unwrap();
    payment.submit(&mut fyo).unwrap();

    let names = linked_entry_names(&fyo, &invoice).unwrap();
    let schemas: Vec<&str> = names.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(schemas, ["Payment", "AccountingLedgerEntry"]);

    let groups = linked_entries(&fyo, &formatter, &invoice).unwrap();
    let payment_entry = &groups[0].entries[0];
    assert_eq!(payment_entry.name, payment.name().unwrap());
    assert_eq!(
        payment_entry.pills,
        vec![
            Pill {
                text: "Acme".to_owned(),
                color: "gray"
            },
            Pill {
                text: "118.00".to_owned(),
                color: "green"
            },
        ]
    );
    let ledger = &groups[1];
    assert_eq!(ledger.label, "Accounting Ledger Entry");
    assert!(
        ledger
            .entries
            .iter()
            .any(|e| e.pills.first().is_some_and(|p| p.text == "Dr. 118.00"))
    );

    // The party is linked to by the invoice and the payment.
    let party = fyo.get_doc("Party", Some("Acme")).unwrap();
    let names = linked_entry_names(&fyo, &party).unwrap();
    let schemas: Vec<&str> = names.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(&schemas[..2], ["SalesInvoice", "Payment"]);

    // A cancelled payment is still listed, as in Books: it links to the invoice from its
    // row, and rows aren't cancelled.
    payment.cancel(&mut fyo).unwrap();
    let names = linked_entry_names(&fyo, &invoice).unwrap();
    assert!(names.iter().any(|(s, _)| s == "Payment"));
}

#[test]
fn deleting_a_linked_document_is_a_link_validation_error() {
    let mut fyo = books();
    let mut doc = sales_invoice(&mut fyo, 1.0);
    doc.sync(&mut fyo).unwrap();
    let mut party = fyo.get_doc("Party", Some("Acme")).unwrap();
    let error = party.delete(&mut fyo).unwrap_err();
    assert!(is_link_validation_error(&error), "{error:?}");
    assert_eq!(
        delete_failed_message(&party),
        "Cannot delete Party \"Acme\" because of linked entries."
    );
    assert_eq!(cannot_cancel_message(&party), "Acme cannot be deleted");
}

#[test]
fn cancelling_an_invoice_names_its_payments() {
    let mut fyo = payments();
    let invoice = submitted_invoice(&mut fyo, 1.0);
    assert_eq!(
        cancel_message(&fyo, &invoice).unwrap(),
        "This action is permanent"
    );
    let mut payment = get_payment(&invoice, &fyo).unwrap().unwrap();
    payment.sync(&mut fyo).unwrap();
    assert_eq!(
        cancel_message(&fyo, &invoice).unwrap(),
        format!(
            "This action is permanent and will cancel the following payment: {}",
            payment.name().unwrap()
        )
    );
}

#[test]
fn create_starts_with_the_typed_name_and_the_create_filters() {
    let mut fyo = books();
    let doc = sales_invoice(&mut fyo, 1.0);
    let party = doc.schema().field("party").unwrap().clone();
    let ctx = field::context(&fyo, &doc, None);
    let values = create_values(&fyo, &party, &ctx, "Beans Inc");
    assert_eq!(values.get("name"), Some(&text("Beans Inc")));
    assert_eq!(values.get("role"), Some(&text("Customer")));

    let row = &doc.table("items")[0];
    let item = row.schema().field("item").unwrap().clone();
    let ctx = field::context(&fyo, row, Some(&doc));
    let values = create_values(&fyo, &item, &ctx, "");
    assert_eq!(values.get("for"), Some(&text("Sales")));
    assert!(matches!(values.get("name"), Some(DocValue::Text(n)) if n.starts_with("New Item")));
}

#[test]
fn remove_filter_drops_the_models_filters() {
    let mut fyo = books();
    let doc = sales_invoice(&mut fyo, 1.0);
    let party = doc.schema().field("party").unwrap().clone();
    {
        let ctx = field::context(&fyo, &doc, None);
        assert!(link_is_filtered(&fyo, &party, &ctx));
        let options = field::link_options(&fyo, &party, &ctx).unwrap();
        assert_eq!(options.len(), 1);
    }
    fyo.db
        .update(
            "SystemSettings",
            &map([("removeFilter", DocValue::Bool(true))]),
        )
        .unwrap();
    let ctx = field::context(&fyo, &doc, None);
    assert!(!link_is_filtered(&fyo, &party, &ctx));
    assert_eq!(field::link_options(&fyo, &party, &ctx).unwrap().len(), 2);
}

#[test]
fn a_scanned_barcode_adds_its_item_or_one_more_of_it() {
    let mut fyo = books();
    fyo.db
        .update(
            "Item",
            &map([("name", text("Widget")), ("barcode", text("123456789012"))]),
        )
        .unwrap();
    assert_eq!(barcode_item(&fyo, "123456789012"), Ok("Widget".to_owned()));
    assert_eq!(
        barcode_item(&fyo, "12345"),
        Err("Invalid barcode value 12345.".to_owned())
    );
    assert_eq!(
        barcode_item(&fyo, "999999999999"),
        Err("Item with barcode 999999999999 not found.".to_owned())
    );

    let mut doc = fyo.get_new_doc("SalesInvoice", DocValueMap::new()).unwrap();
    add_item(&mut fyo, &mut doc, "Widget").unwrap();
    add_item(&mut fyo, &mut doc, "Widget").unwrap();
    assert_eq!(doc.table("items").len(), 1);
    assert_eq!(
        doc.table("items")[0].get("quantity"),
        Some(&DocValue::Number(2.0))
    );
}

#[test]
fn quick_edit_shows_the_quick_edit_fields_less_those_hidden() {
    let fyo = payments();
    let doc = fyo.get_new_doc("Account", DocValueMap::new()).unwrap();
    let names: Vec<String> = quick_edit_fields(&fyo, &doc, &[])
        .into_iter()
        .map(|f| f.fieldname)
        .collect();
    assert!(names.contains(&"rootType".to_owned()));
    let payment = fyo.get_new_doc("Payment", DocValueMap::new()).unwrap();
    let hide = ["party".to_owned(), "for".to_owned()];
    let names: Vec<String> = quick_edit_fields(&fyo, &payment, &hide)
        .into_iter()
        .map(|f| f.fieldname)
        .collect();
    assert!(!names.contains(&"party".to_owned()) && !names.contains(&"for".to_owned()));
    assert!(names.contains(&"amount".to_owned()));
}

#[test]
fn quick_view_shows_the_linked_documents_values() {
    let fyo = books();
    let formatter = Formatter::new(&fyo).unwrap();
    let values = quick_view(&fyo, &formatter, "Party", "Acme");
    assert!(
        values.contains(&("Role".to_owned(), "Customer".to_owned())),
        "{values:?}"
    );
}
