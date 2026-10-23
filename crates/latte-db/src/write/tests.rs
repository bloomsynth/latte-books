// Writing, on in-memory files whose tables are made as Books makes them.

use jiff::Timestamp;
use latte_schema::get_schemas;
use rusqlite::Connection;

use crate::{
    BOOKS_VERSION, Database, DatabaseCore, DocValue, DocValueMap, Error, Filter, QueryFilter,
};

const DEVICE: &str = "device-a";
const NOW: &str = "2026-10-22T09:30:00.000Z";

/// A new company file: every table, the system settings, a number series and two accounts.
fn company() -> Database {
    let mut core = DatabaseCore::new(
        Connection::open_in_memory().unwrap(),
        get_schemas("in", &[]).unwrap(),
    );
    let schemas: Vec<String> = core
        .schema_map()
        .values()
        .filter(|s| !s.is_single)
        .map(|s| s.name.clone())
        .collect();
    for schema in &schemas {
        core.create_table(schema).unwrap();
    }
    core.enable_change_log(DEVICE).unwrap();
    let mut settings = raw(&[
        ("version", BOOKS_VERSION),
        ("countryCode", "in"),
        ("currency", "INR"),
    ]);
    core.insert("SystemSettings", &mut settings).unwrap();

    let mut db = Database::from_connection(core.conn).unwrap();
    db.core_mut().enable_change_log(DEVICE).unwrap();
    db.insert(
        "NumberSeries",
        &doc(&[
            ("name", text("JV-")),
            ("start", DocValue::Number(1001.0)),
            ("padZeros", DocValue::Number(4.0)),
            ("referenceType", text("JournalEntry")),
            ("current", DocValue::Number(0.0)),
        ])
        .with_meta(),
    )
    .unwrap();
    for (i, account) in ["Cash", "Sales"].into_iter().enumerate() {
        db.insert(
            "Account",
            &doc(&[
                ("name", text(account)),
                ("rootType", text("Asset")),
                ("lft", DocValue::Number(i as f64 * 2.0)),
                ("rgt", DocValue::Number(i as f64 * 2.0 + 1.0)),
            ])
            .with_meta(),
        )
        .unwrap();
    }
    db.core()
        .connection()
        .execute("delete from latte_changes", [])
        .unwrap();
    db
}

fn text(t: &str) -> DocValue {
    DocValue::Text(t.into())
}

fn doc(values: &[(&str, DocValue)]) -> DocValueMap {
    values
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect()
}

fn raw(values: &[(&str, &str)]) -> crate::FieldValueMap {
    values
        .iter()
        .map(|(k, v)| {
            (
                (*k).to_owned(),
                crate::FieldValue::Value(crate::RawValue::Text((*v).into())),
            )
        })
        .collect()
}

trait WithMeta {
    fn with_meta(self) -> Self;
}

impl WithMeta for DocValueMap {
    fn with_meta(mut self) -> Self {
        let now: Timestamp = NOW.parse().unwrap();
        self.insert("createdBy".into(), text("Admin"));
        self.insert("modifiedBy".into(), text("Admin"));
        self.insert("created".into(), DocValue::Date(now));
        self.insert("modified".into(), DocValue::Date(now));
        self
    }
}

fn journal_entry(name: &str, lines: &[(&str, &str, &str)]) -> DocValueMap {
    let accounts = lines
        .iter()
        .map(|(account, debit, credit)| {
            let mut line = doc(&[
                ("account", text(account)),
                ("debit", text(debit)),
                ("credit", text(credit)),
            ]);
            if let Some((row, _)) = account.split_once(':') {
                line.insert("name".into(), text(row));
                line.insert("account".into(), text(&account[row.len() + 1..]));
            }
            line
        })
        .collect();
    doc(&[
        ("name", text(name)),
        ("numberSeries", text("JV-")),
        ("entryType", text("Journal Entry")),
        ("date", text("2026-10-22")),
        ("submitted", DocValue::Bool(false)),
        ("cancelled", DocValue::Bool(false)),
        ("accounts", DocValue::Table(accounts)),
    ])
    .with_meta()
}

/// A change log row: (schemaName, name, operation, fields, renamedFrom).
type ChangeRow = (String, String, String, Option<String>, Option<String>);

fn changes(db: &Database) -> Vec<ChangeRow> {
    let conn = db.core().connection();
    let mut statement = conn
        .prepare(
            "select schemaName, name, operation, fields, renamedFrom, device from latte_changes \
             order by id",
        )
        .unwrap();
    statement
        .query_map([], |r| {
            assert_eq!(r.get::<_, String>(5)?, DEVICE);
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn change(schema: &str, name: &str, operation: &str, fields: Option<&str>) -> ChangeRow {
    (
        schema.into(),
        name.into(),
        operation.into(),
        fields.map(Into::into),
        None,
    )
}

fn clear_changes(db: &Database) {
    db.core()
        .connection()
        .execute("delete from latte_changes", [])
        .unwrap();
}

fn column(db: &Database, sql: &str) -> Vec<String> {
    let conn = db.core().connection();
    let mut statement = conn.prepare(sql).unwrap();
    statement
        .query_map([], |r| {
            Ok(match r.get_ref(0)? {
                rusqlite::types::ValueRef::Null => "NULL".to_owned(),
                rusqlite::types::ValueRef::Integer(i) => format!("{i} (integer)"),
                rusqlite::types::ValueRef::Real(f) => format!("{f:?} (real)"),
                rusqlite::types::ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
                rusqlite::types::ValueRef::Blob(_) => "blob".to_owned(),
            })
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn table_sql(db: &Database, table: &str) -> String {
    column(
        db,
        &format!("select sql from sqlite_master where name = '{table}'"),
    )
    .remove(0)
}

#[test]
fn creates_tables_as_books_does() {
    let db = company();
    // As in a file made by Frappe Books v0.36.0.
    assert_eq!(
        table_sql(&db, "NumberSeries"),
        "CREATE TABLE `NumberSeries` (`name` text not null, `start` integer not null default \
         '1001', `padZeros` integer not null default '4', `referenceType` text not null default \
         '-', `current` integer not null, `createdBy` text not null, `modifiedBy` text not null, \
         `created` datetime not null, `modified` datetime not null, primary key (`name`))"
    );
    assert_eq!(
        table_sql(&db, "SingleValue"),
        "CREATE TABLE `SingleValue` (`name` text not null, `parent` text not null, `fieldname` \
         text not null, `value` text not null, `createdBy` text not null, `modifiedBy` text not \
         null, `created` datetime not null, `modified` datetime not null, primary key (`name`))"
    );
    assert!(table_sql(&db, "SalesInvoiceItem").contains(
        "`setItemDiscountAmount` boolean default '0', `itemDiscountAmount` text, \
         `itemDiscountPercent` float"
    ));
    assert!(table_sql(&db, "SalesInvoiceItem").contains(
        "foreign key(`item`) references `Item`(`name`) on delete RESTRICT on update CASCADE"
    ));
}

#[test]
fn inserts_a_document_with_its_child_rows() {
    let mut db = company();
    let entry = journal_entry("JV-1001", &[("Cash", "100", "0"), ("Sales", "0", "100")]);
    let written = db.insert("JournalEntry", &entry).unwrap();

    let DocValue::Table(rows) = &written["accounts"] else {
        panic!("no accounts");
    };
    for (i, row) in rows.iter().enumerate() {
        let DocValue::Text(name) = &row["name"] else {
            panic!("no name");
        };
        assert_eq!(name.len(), 6 + 1 + 8, "{name}");
        assert_eq!(row["parent"], text("JV-1001"));
        assert_eq!(row["parentSchemaName"], text("JournalEntry"));
        assert_eq!(row["parentFieldname"], text("accounts"));
        assert_eq!(row["idx"], DocValue::Number(i as f64));
    }

    let read = db.get_doc("JournalEntry", "JV-1001").unwrap();
    assert_eq!(read["date"], DocValue::Date(read_date("2026-10-22")));
    let DocValue::Table(read_rows) = &read["accounts"] else {
        panic!("no accounts");
    };
    for (read_row, row) in read_rows.iter().zip(rows) {
        for fieldname in ["name", "account", "debit", "credit", "idx", "parent"] {
            assert_eq!(read_row[fieldname], row[fieldname], "{fieldname}");
        }
    }
    let DocValue::Money(debit) = &read_rows[0]["debit"] else {
        panic!("no debit");
    };
    assert_eq!(debit.store(), "100.00000000000");

    // As Books stores them.
    assert_eq!(
        column(&db, "select date || ' ' || created from JournalEntry"),
        ["2026-10-22 2026-10-22T09:30:00.000Z"]
    );
    assert_eq!(
        column(&db, "select submitted from JournalEntry"),
        ["0 (integer)"]
    );
    assert_eq!(
        column(&db, "select debit from JournalEntryAccount order by idx"),
        ["100.00000000000", "0.00000000000"]
    );

    let log = changes(&db);
    assert_eq!(log.len(), 3);
    assert_eq!(log[0].0, "JournalEntry");
    assert_eq!(log[0].1, "JV-1001");
    assert_eq!(log[0].2, "insert");
    assert!(log[0].3.as_deref().unwrap().contains("\"entryType\""));
    assert_eq!(log[1].0, "JournalEntryAccount");
    assert_eq!(log[1].2, "insert");
}

/// The date as `to_doc_value` reads `2026-10-22`: midnight in the system time zone.
fn read_date(date: &str) -> Timestamp {
    date.parse::<jiff::civil::Date>()
        .unwrap()
        .to_zoned(jiff::tz::TimeZone::system())
        .unwrap()
        .timestamp()
}

#[test]
fn update_replaces_child_rows() {
    let mut db = company();
    db.insert(
        "JournalEntry",
        &journal_entry(
            "JV-1001",
            &[("r1:Cash", "100", "0"), ("r2:Sales", "0", "100")],
        ),
    )
    .unwrap();
    clear_changes(&db);

    // r1 changes, r2 goes, r3 is new.
    let mut entry = journal_entry(
        "JV-1001",
        &[("r1:Cash", "150", "0"), ("r3:Sales", "0", "150")],
    );
    entry.shift_remove("createdBy");
    entry.shift_remove("created");
    db.update("JournalEntry", &entry).unwrap();

    assert_eq!(
        column(
            &db,
            "select name || ' ' || idx || ' ' || debit from JournalEntryAccount order by idx"
        ),
        ["r1 0 150.00000000000", "r3 1 0.00000000000"]
    );
    let log = changes(&db);
    assert_eq!(log[0].0, "JournalEntry");
    assert_eq!(log[0].2, "update");
    assert!(!log[0].3.as_deref().unwrap().contains("\"name\""));
    assert!(!log[0].3.as_deref().unwrap().contains("\"accounts\""));
    assert_eq!(log[1].1, "r1");
    assert_eq!(log[1].2, "update");
    assert_eq!(
        log[2..],
        [
            change(
                "JournalEntryAccount",
                "r3",
                "insert",
                Some(
                    "[\"name\",\"account\",\"debit\",\"credit\",\"idx\",\"parent\",\
                     \"parentSchemaName\",\"parentFieldname\"]"
                )
            ),
            change("JournalEntryAccount", "r2", "delete", None),
        ]
    );
}

#[test]
fn update_leaves_child_rows_alone_when_not_given() {
    let mut db = company();
    db.insert(
        "JournalEntry",
        &journal_entry("JV-1001", &[("r1:Cash", "100", "0")]),
    )
    .unwrap();
    db.update(
        "JournalEntry",
        &doc(&[("name", text("JV-1001")), ("entryType", text("Bank Entry"))]),
    )
    .unwrap();
    assert_eq!(column(&db, "select name from JournalEntryAccount"), ["r1"]);
    assert_eq!(
        column(&db, "select entryType from JournalEntry"),
        ["Bank Entry"]
    );
}

#[test]
fn delete_takes_the_child_rows() {
    let mut db = company();
    db.insert(
        "JournalEntry",
        &journal_entry("JV-1001", &[("r1:Cash", "100", "0")]),
    )
    .unwrap();
    clear_changes(&db);

    db.delete("JournalEntry", "JV-1001").unwrap();
    assert!(column(&db, "select name from JournalEntryAccount").is_empty());
    assert_eq!(
        changes(&db),
        [
            change("JournalEntry", "JV-1001", "delete", None),
            change("JournalEntryAccount", "r1", "delete", None),
        ]
    );
}

#[test]
fn a_failed_write_leaves_nothing_behind() {
    let mut db = company();
    db.insert(
        "JournalEntry",
        &journal_entry("JV-1001", &[("r1:Cash", "100", "0")]),
    )
    .unwrap();
    clear_changes(&db);

    // An account in use can't go: its foreign keys say RESTRICT.
    assert!(db.delete("Account", "Cash").is_err());
    // The second row links to an account that doesn't exist, so the first goes back out.
    let bad = journal_entry(
        "JV-1002",
        &[("r2:Cash", "1", "0"), ("r3:Nowhere", "0", "1")],
    );
    assert!(db.insert("JournalEntry", &bad).is_err());

    assert_eq!(column(&db, "select count(*) from Account"), ["2 (integer)"]);
    assert_eq!(column(&db, "select name from JournalEntry"), ["JV-1001"]);
    assert_eq!(column(&db, "select name from JournalEntryAccount"), ["r1"]);
    assert!(changes(&db).is_empty());
}

#[test]
fn rename_carries_links_but_not_child_parents() {
    let mut db = company();
    db.insert(
        "JournalEntry",
        &journal_entry("JV-1001", &[("r1:Cash", "100", "0")]),
    )
    .unwrap();
    clear_changes(&db);

    db.rename("Account", "Cash", "Petty Cash").unwrap();
    assert_eq!(
        column(&db, "select account from JournalEntryAccount"),
        ["Petty Cash"]
    );
    db.rename("JournalEntry", "JV-1001", "JV-2001").unwrap();
    assert_eq!(
        column(&db, "select parent from JournalEntryAccount"),
        ["JV-1001"]
    );

    let log = changes(&db);
    assert_eq!(log.len(), 2);
    assert_eq!(
        log[0],
        (
            "Account".into(),
            "Petty Cash".into(),
            "rename".into(),
            None,
            Some("Cash".into())
        )
    );
}

#[test]
fn singles_are_written_as_single_values() {
    let mut db = company();
    db.update(
        "SystemSettings",
        &doc(&[
            ("internalPrecision", DocValue::Number(11.0)),
            ("hideGetStarted", DocValue::Bool(true)),
            ("currency", text("USD")),
        ]),
    )
    .unwrap();

    // Numbers as better-sqlite3 binds them: doubles, which a text column keeps as "11.0".
    assert_eq!(
        column(
            &db,
            "select fieldname || '=' || value || ' ' || createdBy || ' ' || modifiedBy from \
             SingleValue where parent = 'SystemSettings' order by fieldname"
        ),
        [
            "countryCode=in __SYSTEM__ __SYSTEM__",
            "currency=USD __SYSTEM__ __SYSTEM__",
            "hideGetStarted=1.0 __SYSTEM__ __SYSTEM__",
            "internalPrecision=11.0 __SYSTEM__ __SYSTEM__",
            "version=0.36.0 __SYSTEM__ __SYSTEM__",
        ]
    );
    assert_eq!(
        changes(&db),
        [change(
            "SystemSettings",
            "SystemSettings",
            "update",
            Some("[\"hideGetStarted\",\"currency\",\"internalPrecision\"]")
        )]
    );

    clear_changes(&db);
    db.delete("SystemSettings", "hideGetStarted").unwrap();
    assert_eq!(
        column(
            &db,
            "select count(*) from SingleValue where fieldname = 'hideGetStarted'"
        ),
        ["0 (integer)"]
    );
    assert_eq!(
        changes(&db),
        [change(
            "SystemSettings",
            "SystemSettings",
            "delete",
            Some("[\"hideGetStarted\"]")
        )]
    );
}

#[test]
fn delete_all_deletes_what_matches() {
    let mut db = company();
    for party in ["Acme", "Bolt", "Cogs"] {
        db.insert(
            "Party",
            &doc(&[("name", text(party)), ("role", text("Both"))]).with_meta(),
        )
        .unwrap();
    }
    clear_changes(&db);

    let filters: QueryFilter = [("name".to_owned(), Filter::op("like", "o"))].into();
    assert_eq!(db.delete_all("Party", &filters).unwrap(), 2);
    assert_eq!(column(&db, "select name from Party"), ["Acme"]);
    assert_eq!(
        changes(&db),
        [
            change("Party", "Bolt", "delete", None),
            change("Party", "Cogs", "delete", None),
        ]
    );
}

#[test]
fn names_documents_that_have_none() {
    let mut db = company();
    let written = db
        .insert("Party", &doc(&[("role", text("Both"))]).with_meta())
        .unwrap();
    let DocValue::Text(name) = &written["name"] else {
        panic!("no name");
    };
    let (random, time) = name.split_once('-').unwrap();
    assert_eq!(random.len(), 6);
    assert!(
        random
            .chars()
            .all(|c| c.is_ascii_digit() || c.is_ascii_lowercase())
    );
    let ms = u64::from_str_radix(time, 36).unwrap() as i64;
    assert!((Timestamp::now().as_millisecond() - ms).abs() < 60_000);
}

#[test]
fn last_inserted_reads_names_as_numbers() {
    let mut db = company();
    assert_eq!(db.core().get_last_inserted("Party").unwrap(), 0);
    for name in ["000000009", "000000010", "Walk-in"] {
        db.insert(
            "Party",
            &doc(&[("name", text(name)), ("role", text("Both"))]).with_meta(),
        )
        .unwrap();
    }
    assert_eq!(db.core().get_last_inserted("Party").unwrap(), 10);
}

#[test]
fn writing_needs_the_change_log() {
    let db = company();
    let mut read_only = Database::from_connection(db.core.conn).unwrap();
    let result = read_only.update(
        "JournalEntry",
        &doc(&[("name", text("JV-1001")), ("entryType", text("x"))]),
    );
    assert!(matches!(result, Err(Error::Value(_))), "{result:?}");
}

#[test]
fn converts_document_values_as_books_does() {
    let db = company();
    let map = db.schema_map();
    let pesa = db.pesa();
    let field = |schema: &str, fieldname: &str| map[schema].field(fieldname).unwrap().clone();
    let to_raw = |schema: &str, fieldname: &str, value: DocValue| {
        crate::to_raw_value(&value, &field(schema, fieldname), pesa)
    };
    use crate::RawValue::{Null, Real, Text};

    assert_eq!(
        to_raw("JournalEntryAccount", "debit", DocValue::Null).unwrap(),
        Text("0.00000000000".into())
    );
    assert_eq!(
        to_raw("JournalEntryAccount", "debit", DocValue::Number(0.1)).unwrap(),
        Text("0.10000000000".into())
    );
    assert_eq!(
        to_raw("NumberSeries", "start", text("7.9")).unwrap(),
        Real(7.0)
    );
    assert_eq!(
        to_raw("NumberSeries", "start", DocValue::Number(-7.5)).unwrap(),
        Real(-8.0)
    );
    assert_eq!(
        to_raw("JournalEntryAccount", "account", text("")).unwrap(),
        Null
    );
    assert_eq!(
        to_raw("JournalEntryAccount", "account", DocValue::Number(3.0)).unwrap(),
        Null
    );
    assert!(to_raw("JournalEntry", "submitted", DocValue::Null).is_err());
    assert!(to_raw("JournalEntry", "entryType", DocValue::Number(1.0)).is_err());
    assert_eq!(
        to_raw("JournalEntry", "date", text("not a date")).unwrap(),
        Null
    );
    // A date alone is UTC midnight to JavaScript, then the local date.
    let utc_midnight: Timestamp = "2026-10-22T00:00:00Z".parse().unwrap();
    assert_eq!(
        to_raw("JournalEntry", "date", text("2026-10-22")).unwrap(),
        Text(
            utc_midnight
                .to_zoned(jiff::tz::TimeZone::system())
                .date()
                .to_string()
        )
    );
    let instant: Timestamp = "2026-10-22T09:30:00.5Z".parse().unwrap();
    assert_eq!(
        to_raw("JournalEntry", "created", DocValue::Date(instant)).unwrap(),
        Text("2026-10-22T09:30:00.500Z".into())
    );
    let attachment = serde_json::json!({"name": "a.pdf", "type": "application/pdf", "data": "x"});
    assert_eq!(
        to_raw(
            "SalesInvoice",
            "attachment",
            DocValue::Attachment(attachment)
        )
        .unwrap(),
        Text("{\"name\":\"a.pdf\",\"type\":\"application/pdf\",\"data\":\"x\"}".into())
    );
    assert!(
        to_raw(
            "SalesInvoice",
            "attachment",
            DocValue::Attachment(serde_json::json!({"name": "a.pdf"}))
        )
        .is_err()
    );
}

#[test]
fn changes_from_another_device_are_found() {
    let mut db = company();
    let account = |name: &str| {
        doc(&[
            ("name", text(name)),
            ("rootType", text("Asset")),
            ("lft", DocValue::Number(4.0)),
            ("rgt", DocValue::Number(5.0)),
        ])
        .with_meta()
    };
    let start = db.core().last_change_id().unwrap();
    db.insert("Account", &account("Bank")).unwrap();
    // Our own change isn't another device's.
    assert_eq!(db.core().changes_from_others(start).unwrap().1, vec![]);

    db.core_mut().enable_change_log("device-b").unwrap();
    db.insert("Account", &account("Petty Cash")).unwrap();
    db.core_mut().enable_change_log(DEVICE).unwrap();
    let (last, changed) = db.core().changes_from_others(start).unwrap();
    assert_eq!(last, db.core().last_change_id().unwrap());
    assert_eq!(
        changed,
        vec![("Account".to_owned(), "Petty Cash".to_owned())]
    );
    assert_eq!(db.core().changes_from_others(last).unwrap(), (last, vec![]));
}
