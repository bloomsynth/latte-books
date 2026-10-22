use latte_db::{BOOKS_VERSION, Database, DatabaseCore, DocValue, DocValueMap};
use latte_schema::get_schemas;
use rusqlite::{Connection, params};

use super::*;

const CREATED: &str = "2025-01-01T00:00:00.000Z";

/// An in-memory company file with every table, the system settings and two accounts.
fn company() -> Fyo {
    let core = DatabaseCore::new(
        Connection::open_in_memory().unwrap(),
        get_schemas("in", &[]).unwrap(),
    );
    let schemas: Vec<String> = core
        .schema_map()
        .values()
        .filter(|s| !s.is_single)
        .map(|s| s.name.clone())
        .collect();
    for schema_name in &schemas {
        core.create_table(schema_name).unwrap();
    }

    let conn = core.into_connection();
    for (fieldname, value) in [
        ("version", BOOKS_VERSION),
        ("countryCode", "in"),
        ("internalPrecision", "11.0"),
        ("displayPrecision", "2.0"),
        ("currency", "INR"),
    ] {
        conn.execute(
            "insert into SingleValue (name, parent, fieldname, value, createdBy, modifiedBy,
                                      created, modified)
                 values (?, ?, ?, ?, 'Admin', 'Admin', ?5, ?5)",
            params![
                format!("SystemSettings-{fieldname}"),
                "SystemSettings",
                fieldname,
                value,
                CREATED
            ],
        )
        .unwrap();
    }
    conn.execute_batch(
        "insert into Account (name, rootType, lft, rgt, createdBy, modifiedBy, created, modified)
             values ('Cash', 'Asset', 1, 2, 'Admin', 'Admin', '2025-01-01T00:00:00.000Z',
                     '2025-01-01T00:00:00.000Z'),
                    ('Sales', 'Income', 3, 4, 'Admin', 'Admin', '2025-01-01T00:00:00.000Z',
                     '2025-01-01T00:00:00.000Z');",
    )
    .unwrap();

    let mut db = Database::from_connection(conn).unwrap();
    db.core_mut().enable_change_log("test").unwrap();
    Fyo::new(db)
}

fn text(value: &str) -> DocValue {
    DocValue::Text(value.to_owned())
}

fn map<const N: usize>(values: [(&str, DocValue); N]) -> DocValueMap {
    values.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

fn row(account: &str, debit: f64, credit: f64) -> DocValueMap {
    map([
        ("account", text(account)),
        ("debit", DocValue::Number(debit)),
        ("credit", DocValue::Number(credit)),
    ])
}

fn journal_entry(fyo: &Fyo) -> Doc {
    fyo.get_new_doc(
        "JournalEntry",
        map([
            ("entryType", text("Journal Entry")),
            ("date", text("2025-04-01")),
            (
                "accounts",
                DocValue::Table(vec![row("Cash", 250.0, 0.0), row("Sales", 0.0, 250.0)]),
            ),
        ]),
    )
    .unwrap()
}

fn money(doc: &Doc, fieldname: &str) -> String {
    match doc.get(fieldname) {
        Some(DocValue::Money(money)) => money.round(None),
        value => panic!("{fieldname} is {value:?}"),
    }
}

#[test]
fn new_docs_get_defaults_and_temporary_names() {
    let fyo = company();
    let doc = journal_entry(&fyo);

    assert_eq!(doc.name(), Some("New Journal Entry 01"));
    assert_eq!(doc.get("numberSeries"), Some(&text("JV-")));
    assert!(doc.not_inserted() && doc.dirty());

    let rows = doc.table("accounts");
    assert_eq!(rows.len(), 2);
    assert_eq!(money(&rows[0], "debit"), "250.00");
    assert_eq!(money(&rows[1], "debit"), "0.00");
    assert_eq!(rows[1].get("idx"), Some(&DocValue::Number(1.0)));
    assert_eq!(rows[1].get("parent"), Some(&text("New Journal Entry 01")));
    assert_eq!(rows[1].get("parentFieldname"), Some(&text("accounts")));
    assert_eq!(money_of(doc.get_sum(&fyo, "accounts", "credit")), "250.00");

    let second = journal_entry(&fyo);
    assert_eq!(second.name(), Some("New Journal Entry 02"));
}

fn money_of(value: Result<latte_db::pesa::Money, Error>) -> String {
    value.unwrap().round(None)
}

#[test]
fn sync_names_from_the_number_series_and_submit_saves() {
    let mut fyo = company();
    let mut doc = journal_entry(&fyo);
    doc.sync(&mut fyo).unwrap();

    assert_eq!(doc.name(), Some("JV-1001"));
    assert!(doc.inserted() && !doc.dirty());
    assert_eq!(doc.get("createdBy"), Some(&text("Admin")));
    assert!(matches!(doc.get("created"), Some(DocValue::Date(_))));
    assert_eq!(doc.get("submitted"), Some(&DocValue::Bool(false)));
    assert_eq!(
        doc.table("accounts")[0].get("parent"),
        Some(&text("JV-1001"))
    );

    let series = fyo.get_doc("NumberSeries", Some("JV-")).unwrap();
    assert_eq!(series.get("current"), Some(&DocValue::Number(1001.0)));
    assert_eq!(series.get("referenceType"), Some(&text("JournalEntry")));

    assert!(doc.can_submit());
    doc.submit(&mut fyo).unwrap();
    assert!(doc.is_submitted() && !doc.can_edit() && !doc.can_delete());

    let loaded = fyo.get_doc("JournalEntry", Some("JV-1001")).unwrap();
    assert_eq!(loaded.get("submitted"), Some(&DocValue::Bool(true)));
    assert_eq!(money(&loaded.table("accounts")[1], "credit"), "250.00");

    let mut next = journal_entry(&fyo);
    next.sync(&mut fyo).unwrap();
    assert_eq!(next.name(), Some("JV-1002"));
}

#[test]
fn set_checks_values_and_skips_unchanged_ones() {
    let mut fyo = company();
    let mut doc = journal_entry(&fyo);

    assert!(
        !doc.set(&mut fyo, "entryType", text("Journal Entry"))
            .unwrap()
    );
    assert!(!doc.set(&mut fyo, "noSuchField", text("x")).unwrap());
    assert!(doc.set(&mut fyo, "userRemark", text("  paid  ")).unwrap());
    assert_eq!(doc.get("userRemark"), Some(&text("paid")));

    let error = doc.set(&mut fyo, "entryType", text("Gift")).unwrap_err();
    assert_eq!(error.to_string(), "Invalid value Gift for Entry Type");

    // As in Books, `set` keeps the value it is given, so money is passed as money.
    let three_hundred = fyo.pesa().from_f64(300.0).unwrap();
    assert!(
        doc.set_child(
            &mut fyo,
            "accounts",
            0,
            "debit",
            DocValue::Money(three_hundred)
        )
        .unwrap()
    );
    assert_eq!(money(&doc.table("accounts")[0], "debit"), "300.00");
}

#[test]
fn sync_reports_missing_values() {
    let mut fyo = company();
    let mut doc = fyo
        .get_new_doc(
            "JournalEntry",
            map([("accounts", DocValue::Table(vec![map([])]))]),
        )
        .unwrap();

    let error = doc.sync(&mut fyo).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Value missing for Entry Type, Date\nAccount Entries Row 1: Account"
    );
}

#[test]
fn update_refuses_a_document_changed_since_it_was_loaded() {
    let mut fyo = company();
    let mut doc = journal_entry(&fyo);
    doc.sync(&mut fyo).unwrap();

    let mut stale = fyo.get_doc("JournalEntry", Some("JV-1001")).unwrap();
    doc.set(&mut fyo, "userRemark", text("first")).unwrap();
    doc.sync(&mut fyo).unwrap();

    stale.set(&mut fyo, "userRemark", text("second")).unwrap();
    assert!(matches!(stale.sync(&mut fyo), Err(Error::Conflict(_))));
}

#[test]
fn number_series_names_are_checked() {
    let mut fyo = company();
    let mut series = fyo
        .get_new_doc(
            "NumberSeries",
            map([
                ("name", text("A/B")),
                ("referenceType", text("JournalEntry")),
            ]),
        )
        .unwrap();

    let error = series.sync(&mut fyo).unwrap_err();
    assert_eq!(
        error.to_string(),
        "The following characters cannot be used /, ?, &, =, % in a Number Series name."
    );
}

#[test]
fn singles_load_by_schema_name() {
    let mut fyo = company();
    let settings = fyo.get_doc("SystemSettings", None).unwrap();
    assert_eq!(settings.name(), Some("SystemSettings"));
    assert_eq!(settings.get("currency"), Some(&text("INR")));

    assert!(matches!(
        fyo.get_doc("JournalEntry", None),
        Err(Error::Value(m)) if m == "JournalEntry is not a Single Schema"
    ));
}

#[test]
fn duplicate_drops_names_and_meta_values() {
    let mut fyo = company();
    let mut doc = journal_entry(&fyo);
    doc.sync(&mut fyo).unwrap();

    let copy = doc.duplicate(&fyo).unwrap();
    assert!(copy.not_inserted());
    assert_eq!(copy.name(), Some("New Journal Entry 02"));
    assert_eq!(copy.get("created"), Some(&DocValue::Null));
    assert_eq!(copy.table("accounts").len(), 2);
    assert_ne!(
        copy.table("accounts")[0].get("name"),
        doc.table("accounts")[0].get("name")
    );
}

#[test]
fn remove_takes_out_a_row_and_renumbers_the_rest() {
    let mut fyo = company();
    let mut doc = journal_entry(&fyo);
    doc.append(&mut fyo, "accounts", row("Cash", 5.0, 0.0))
        .unwrap();

    assert!(doc.remove(&mut fyo, "accounts", 0).unwrap());
    let rows = doc.table("accounts");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get("account"), Some(&text("Sales")));
    assert_eq!(rows[1].get("idx"), Some(&DocValue::Number(1.0)));
}
