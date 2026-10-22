use std::collections::HashMap;

use jiff::Timestamp;
use jiff::tz::TimeZone;
use latte_db::{DocValue, DocValueMap, Filter, GetAllOptions, Order, QueryFilter};

use crate::models::models;
use crate::tests::{company, journal_entry, map, money, row, text};
use crate::{Doc, Error, Fyo};

fn books() -> Fyo {
    let mut fyo = company();
    fyo.register_models(models(), HashMap::new());
    fyo
}

/// The document's ledger entries, oldest first.
fn ledger_entries(fyo: &Fyo, name: &str) -> Vec<DocValueMap> {
    let options = GetAllOptions {
        fields: Some(vec!["*".to_owned()]),
        filters: QueryFilter::from([("referenceName".to_owned(), Filter::from(name))]),
        order_by: Some(vec!["name".to_owned()]),
        order: Some(Order::Asc),
        ..GetAllOptions::default()
    };
    fyo.db
        .get_all_docs("AccountingLedgerEntry", &options)
        .unwrap()
}

fn amount(entry: &DocValueMap, fieldname: &str) -> String {
    match entry.get(fieldname) {
        Some(DocValue::Money(money)) => money.round(None),
        value => panic!("{fieldname} is {value:?}"),
    }
}

/// A journal entry of 250 from Sales to Cash, saved and submitted as JV-1001.
fn submitted_journal_entry(fyo: &mut Fyo) -> Doc {
    let mut doc = journal_entry(fyo);
    doc.sync(fyo).unwrap();
    doc.submit(fyo).unwrap();
    doc
}

#[test]
fn submitting_a_journal_entry_posts_it() {
    let mut fyo = books();
    let doc = submitted_journal_entry(&mut fyo);
    assert_eq!(doc.name(), Some("JV-1001"));

    let entries = ledger_entries(&fyo, "JV-1001");
    assert_eq!(entries.len(), 2);
    let cash = &entries[0];
    assert_eq!(cash.get("name"), Some(&text("000000001")));
    assert_eq!(cash.get("account"), Some(&text("Cash")));
    assert_eq!(amount(cash, "debit"), "250.00");
    assert_eq!(amount(cash, "credit"), "0.00");
    assert_eq!(cash.get("referenceType"), Some(&text("JournalEntry")));
    assert_eq!(cash.get("reverted"), Some(&DocValue::Bool(false)));
    // A journal entry has no party; Books' empty party is stored as null.
    assert_eq!(cash.get("party"), Some(&DocValue::Null));

    // Midnight UTC of the entry's date as this computer's time zone sees it.
    let date: Timestamp = "2025-04-01T00:00:00Z".parse().unwrap();
    let day = date.to_zoned(TimeZone::system()).date();
    let expected = day.to_zoned(TimeZone::UTC).unwrap().timestamp();
    assert_eq!(cash.get("date"), Some(&DocValue::Date(expected)));

    let sales = &entries[1];
    assert_eq!(sales.get("account"), Some(&text("Sales")));
    assert_eq!(amount(sales, "credit"), "250.00");
}

#[test]
fn cancelling_a_journal_entry_reverts_its_postings() {
    let mut fyo = books();
    let mut doc = submitted_journal_entry(&mut fyo);
    doc.cancel(&mut fyo).unwrap();
    assert!(doc.is_cancelled());

    let entries = ledger_entries(&fyo, "JV-1001");
    assert_eq!(entries.len(), 4);
    for entry in &entries {
        assert_eq!(entry.get("reverted"), Some(&DocValue::Bool(true)));
    }

    for reversal in &entries[2..] {
        let Some(DocValue::Text(reverts)) = reversal.get("reverts") else {
            panic!("{reversal:?} reverts nothing");
        };
        let original = entries
            .iter()
            .find(|e| e.get("name") == Some(&text(reverts)))
            .unwrap();
        assert_eq!(reversal.get("account"), original.get("account"));
        assert_eq!(amount(reversal, "debit"), amount(original, "credit"));
        assert_eq!(amount(reversal, "credit"), amount(original, "debit"));
    }
}

#[test]
fn deleting_a_cancelled_journal_entry_deletes_its_postings() {
    let mut fyo = books();
    let mut doc = submitted_journal_entry(&mut fyo);
    doc.cancel(&mut fyo).unwrap();
    doc.delete(&mut fyo).unwrap();

    assert!(ledger_entries(&fyo, "JV-1001").is_empty());
    assert!(matches!(
        fyo.get_doc("JournalEntry", Some("JV-1001")),
        Err(Error::NotFound(_))
    ));
}

#[test]
fn an_unbalanced_journal_entry_is_refused() {
    let mut fyo = books();
    let mut doc = fyo
        .get_new_doc(
            "JournalEntry",
            map([
                ("entryType", text("Journal Entry")),
                ("date", text("2025-04-01")),
                (
                    "accounts",
                    DocValue::Table(vec![row("Cash", 250.0, 0.0), row("Sales", 0.0, 200.0)]),
                ),
            ]),
        )
        .unwrap();

    let error = doc.sync(&mut fyo).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Total Debit: 250.00 must be equal to Total Credit: 200.00"
    );
    assert!(doc.not_inserted());
}

#[test]
fn a_new_row_gets_what_balances_the_entry() {
    let mut fyo = books();
    let mut doc = fyo
        .get_new_doc(
            "JournalEntry",
            map([("accounts", DocValue::Table(vec![row("Cash", 250.0, 0.0)]))]),
        )
        .unwrap();
    assert_eq!(doc.get("numberSeries"), Some(&text("JV-")));
    assert!(matches!(doc.get("date"), Some(DocValue::Date(_))));

    doc.append(&mut fyo, "accounts", map([("account", text("Sales"))]))
        .unwrap();
    let rows = doc.table("accounts");
    assert_eq!(money(&rows[1], "credit"), "250.00");
    assert_eq!(money(&rows[1], "debit"), "0.00");
    assert_eq!(money(&rows[0], "credit"), "0.00");
}

#[test]
fn an_account_takes_its_root_and_account_type_from_its_parent() {
    let mut fyo = books();
    let mut account = fyo
        .get_new_doc(
            "Account",
            map([
                ("name", text("HDFC")),
                ("parentAccount", text("Bank Accounts")),
            ]),
        )
        .unwrap();
    assert_eq!(account.get("lft"), Some(&DocValue::Number(0.0)));

    account.sync(&mut fyo).unwrap();
    assert_eq!(account.get("rootType"), Some(&text("Asset")));
    assert_eq!(account.get("accountType"), Some(&text("Bank")));
    assert!(crate::models::base_models::account::is_debit(&account));
}
