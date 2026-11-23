use std::collections::HashMap;

use jiff::Timestamp;
use jiff::tz::TimeZone;
use latte_db::{DocValue, DocValueMap, Filter, GetAllOptions, Order, QueryFilter};

use crate::fyo::model::types::DocContext;
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
    let day = date.to_zoned_datetime(TimeZone::system()).date();
    let expected = day.to_zoned_datetime(TimeZone::UTC).unwrap().timestamp();
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

/// Sets a journal entry row's debit and credit, as the form does: the side with the
/// amount first, so the row's formulas don't fill in the other.
fn set_amounts(fyo: &mut Fyo, doc: &mut Doc, idx: usize, debit: f64, credit: f64) {
    let mut sides = [("debit", debit), ("credit", credit)];
    if debit == 0.0 {
        sides.reverse();
    }
    for (fieldname, value) in sides {
        let value = DocValue::Money(fyo.pesa().from_f64(value).unwrap());
        doc.set_child(fyo, "accounts", idx, fieldname, value)
            .unwrap();
    }
}

#[test]
fn a_submitted_journal_entry_can_be_altered_and_is_reposted() {
    let mut fyo = books();
    let mut doc = submitted_journal_entry(&mut fyo);
    assert!(doc.can_edit());
    assert!(!doc.can_save(), "nothing has changed yet");

    // Its fields are open in the form, as a draft's are.
    let loaded = fyo.get_doc("JournalEntry", Some("JV-1001")).unwrap();
    let field = loaded.schema().field("date").unwrap().clone();
    let ctx = DocContext {
        doc: &loaded,
        parent: None,
        fyo: &fyo,
    };
    assert!(!crate::views::field::is_read_only(&field, &ctx));

    set_amounts(&mut fyo, &mut doc, 0, 0.0, 300.0);
    set_amounts(&mut fyo, &mut doc, 1, 300.0, 0.0);
    let posted: Timestamp = "2025-04-05T00:00:00Z".parse().unwrap();
    doc.set(&mut fyo, "date", DocValue::Date(posted)).unwrap();
    assert!(doc.can_save());
    doc.sync(&mut fyo).unwrap();
    assert!(doc.is_submitted() && !doc.dirty());

    // The old entries are gone and the new ones posted, with no reversals.
    let entries = ledger_entries(&fyo, "JV-1001");
    assert_eq!(entries.len(), 2);
    let side = |account: &str| {
        let entry = entries
            .iter()
            .find(|e| e.get("account") == Some(&text(account)))
            .unwrap();
        assert_eq!(entry.get("reverted"), Some(&DocValue::Bool(false)));
        (amount(entry, "debit"), amount(entry, "credit"))
    };
    assert_eq!(side("Cash"), ("0.00".into(), "300.00".into()));
    assert_eq!(side("Sales"), ("300.00".into(), "0.00".into()));
    let day = posted.to_zoned_datetime(TimeZone::system()).date();
    let expected = day.to_zoned_datetime(TimeZone::UTC).unwrap().timestamp();
    assert_eq!(entries[0].get("date"), Some(&DocValue::Date(expected)));

    // It saves again, and cancelling reverts what it posts now.
    set_amounts(&mut fyo, &mut doc, 0, 0.0, 120.0);
    set_amounts(&mut fyo, &mut doc, 1, 120.0, 0.0);
    doc.sync(&mut fyo).unwrap();
    doc.cancel(&mut fyo).unwrap();
    let entries = ledger_entries(&fyo, "JV-1001");
    assert_eq!(entries.len(), 4);
    assert!(
        entries
            .iter()
            .all(|e| amount(e, "debit") == "120.00" || amount(e, "credit") == "120.00")
    );
    assert!(!doc.can_edit() && !doc.can_save());
}

#[test]
fn an_unbalanced_alteration_changes_nothing() {
    let mut fyo = books();
    let mut doc = submitted_journal_entry(&mut fyo);
    let before = ledger_entries(&fyo, "JV-1001");

    set_amounts(&mut fyo, &mut doc, 0, 300.0, 0.0);
    let error = doc.sync(&mut fyo).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Total Debit: 300.00 must be equal to Total Credit: 250.00"
    );
    assert_eq!(ledger_entries(&fyo, "JV-1001"), before);
    let saved = fyo.get_doc("JournalEntry", Some("JV-1001")).unwrap();
    assert_eq!(money(&saved.table("accounts")[0], "debit"), "250.00");

    // The change is kept, and once balanced it saves without a conflict.
    set_amounts(&mut fyo, &mut doc, 1, 0.0, 300.0);
    doc.sync(&mut fyo).unwrap();
    let entries = ledger_entries(&fyo, "JV-1001");
    assert_eq!(entries.len(), 2);
    assert!(
        entries
            .iter()
            .all(|e| amount(e, "debit") == "300.00" || amount(e, "credit") == "300.00")
    );
}

#[test]
fn atomic_undoes_every_write_when_it_fails() {
    let mut fyo = books();
    let result: Result<(), Error> = fyo.atomic(|fyo| {
        let mut doc = journal_entry(fyo);
        doc.sync(fyo)?;
        Err(Error::Validation("stop".into()))
    });
    assert!(result.is_err());
    assert!(!fyo.db.exists("JournalEntry", Some("JV-1001")).unwrap());

    fyo.atomic(|fyo| journal_entry(fyo).sync(fyo)).unwrap();
    assert!(fyo.db.exists("JournalEntry", Some("JV-1001")).unwrap());
}

#[test]
fn the_edit_history_shows_each_change_with_its_values() {
    use crate::fyo::format::Formatter;
    use crate::views::edit_log::{EditKind, FieldChange, history};

    let mut fyo = books();
    let mut doc = submitted_journal_entry(&mut fyo);
    fyo.set_user("Priya");
    set_amounts(&mut fyo, &mut doc, 0, 300.0, 0.0);
    set_amounts(&mut fyo, &mut doc, 1, 0.0, 300.0);
    doc.set(&mut fyo, "userRemark", text("Corrected amount"))
        .unwrap();
    doc.sync(&mut fyo).unwrap();
    doc.cancel(&mut fyo).unwrap();

    let formatter = Formatter::new(&fyo).unwrap();
    let history = history(&fyo, &formatter, "JournalEntry", "JV-1001").unwrap();
    assert_eq!(history.created_by, "Admin");
    let kinds: Vec<_> = history
        .events
        .iter()
        .map(|e| (&e.kind, e.user.as_str()))
        .collect();
    assert_eq!(
        kinds,
        [
            (&EditKind::Submitted, "Admin"),
            (&EditKind::Altered, "Priya"),
            (&EditKind::Cancelled, "Priya"),
        ]
    );
    assert!(history.events[0].changes.is_empty());
    assert!(history.events[2].changes.is_empty());

    let changes = &history.events[1].changes;
    let change = |label: &str| changes.iter().find(|c| c.label == label).cloned();
    let debit = change("Account Entries row 1: Debit").unwrap();
    assert!(
        debit.before.contains("250.00") && debit.after.contains("300.00"),
        "{debit:?}"
    );
    assert!(
        change("Account Entries row 2: Credit").is_some(),
        "{changes:?}"
    );
    assert_eq!(
        change("User Remark"),
        Some(FieldChange {
            label: "User Remark".into(),
            before: String::new(),
            after: "Corrected amount".into(),
        })
    );
    assert_eq!(changes.len(), 3, "{changes:?}");
}
