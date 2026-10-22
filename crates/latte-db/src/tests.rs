use latte_schema::Schema;
use rusqlite::params;

use super::*;

/// SQLite column types by field type. Port of `sqliteTypeMap` in `backend/helpers.ts`.
fn column_type(fieldtype: FieldType) -> Option<&'static str> {
    Some(match fieldtype {
        FieldType::Int => "integer",
        FieldType::Float => "float",
        FieldType::Check => "boolean",
        FieldType::Date => "date",
        FieldType::Datetime => "datetime",
        FieldType::Table | FieldType::Button => return None,
        _ => "text",
    })
}

/// A table as Books creates it, with column defaults but without constraints, so tests can
/// insert partial rows. Check fields without a default get 0, as Books' documents write
/// `false` for them.
fn create_table(conn: &Connection, schema: &Schema) {
    let columns: Vec<String> = schema
        .fields
        .iter()
        .filter(|f| !f.computed)
        .filter_map(|f| {
            let fieldtype = f.fieldtype?;
            let mut column = format!("`{}` {}", f.fieldname, column_type(fieldtype)?);
            match &f.default {
                Some(latte_schema::RawValue::Bool(b)) => {
                    column += &format!(" default '{}'", *b as u8)
                }
                Some(latte_schema::RawValue::Number(n)) => column += &format!(" default '{n}'"),
                Some(latte_schema::RawValue::Text(t)) => column += &format!(" default '{t}'"),
                None if fieldtype == FieldType::Check => column += " default '0'",
                None => {}
            }
            Some(column)
        })
        .collect();
    conn.execute(
        &format!("create table `{}` ({})", schema.name, columns.join(", ")),
        [],
    )
    .unwrap();
}

fn set_single(conn: &Connection, parent: &str, fieldname: &str, value: &str) {
    conn.execute(
        "insert into SingleValue (name, parent, fieldname, value) values (?, ?, ?, ?)",
        params![format!("{parent}-{fieldname}"), parent, fieldname, value],
    )
    .unwrap();
}

/// An in-memory company file with every table, the system settings, and a few documents.
fn company() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    for schema in get_schemas("in", &[]).unwrap().values() {
        if !schema.is_single {
            create_table(&conn, schema);
        }
    }

    for (fieldname, value) in [
        ("version", BOOKS_VERSION),
        ("countryCode", "in"),
        ("internalPrecision", "11.0"),
        ("displayPrecision", "2.0"),
        ("currency", "INR"),
        ("darkMode", "0.0"),
        ("hideGetStarted", "1.0"),
    ] {
        set_single(&conn, "SystemSettings", fieldname, value);
    }
    set_single(&conn, "Defaults", "buyButtonColour", "#fff");

    conn.execute_batch(
        "insert into Party (name, role, created) values
             ('Acme', 'Customer', '2025-01-01T00:00:00.000Z'),
             ('Bolt Ltd', 'Supplier', '2025-02-01T00:00:00.000Z'),
             ('Cogs & Co', 'Both', '2025-03-01T00:00:00.000Z'),
             ('Dyno', NULL, '2025-04-01T00:00:00.000Z');
         insert into SalesInvoice (name, party, date, grandTotal, baseGrandTotal, exchangeRate,
                                   submitted, cancelled, created, attachment)
             values ('SINV-1001', 'Acme', '2025-01-15T05:12:01.000Z', '1740.00000000000',
                     '155817.12345678901', 89.55, 1, 0, '2025-02-12T05:16:01.900Z', NULL),
                    ('SINV-1002', 'Acme', '2025-04-11T12:12:38.258Z', '', NULL, 1, 0, 0,
                     '2025-04-11T12:13:28.183Z',
                     '{\"name\":\"a.pdf\",\"type\":\"application/pdf\",\"data\":\"x\"}');
         insert into SalesInvoiceItem (name, parent, parentSchemaName, parentFieldname, idx,
                                       item, quantity, rate, amount)
             values ('r2', 'SINV-1001', 'SalesInvoice', 'items', 1, 'Shoes', 2.0,
                     '29.00000000000', '58.00000000000'),
                    ('r1', 'SINV-1001', 'SalesInvoice', 'items', 0, 'Jeans', 60.0,
                     '28.00000000000', '1680.00000000000'),
                    ('r3', 'SINV-1002', 'SalesInvoice', 'items', 0, 'Belt', 1.0,
                     '10.00000000000', '10.00000000000');",
    )
    .unwrap();
    conn
}

fn open(conn: Connection) -> Database {
    Database::from_connection(conn).unwrap()
}

fn text(value: &DocValue) -> &str {
    match value {
        DocValue::Text(t) => t,
        other => panic!("not text: {other:?}"),
    }
}

fn money(value: &DocValue) -> String {
    match value {
        DocValue::Money(m) => m.store(),
        other => panic!("not money: {other:?}"),
    }
}

fn names(rows: &[DocValueMap]) -> Vec<&str> {
    rows.iter().map(|r| text(&r["name"])).collect()
}

#[test]
fn opens_only_files_from_books_0_36_0() {
    let conn = company();
    conn.execute(
        "update SingleValue set value = '0.35.1' where fieldname = 'version'",
        [],
    )
    .unwrap();
    match Database::from_connection(conn) {
        Err(Error::Version { found }) => assert_eq!(found, "0.35.1"),
        other => panic!("{:?}", other.err()),
    }

    let empty = Connection::open_in_memory().unwrap();
    assert!(matches!(
        Database::from_connection(empty),
        Err(Error::Version { found }) if found == "0.0.0"
    ));
}

#[test]
fn reads_country_and_money_settings() {
    let db = open(company());
    assert_eq!(db.country_code(), "in");
    assert!(db.schema_map()["Party"].field("gstin").is_some());
    let pesa = db.pesa();
    assert_eq!(
        (pesa.precision, pesa.display, pesa.currency.as_str()),
        (11, 2, "INR")
    );
}

#[test]
fn get_doc_reads_values_and_child_rows_in_order() {
    let db = open(company());
    let invoice = db.get_doc("SalesInvoice", "SINV-1001").unwrap();

    assert_eq!(text(&invoice["party"]), "Acme");
    assert_eq!(money(&invoice["grandTotal"]), "1740.00000000000");
    assert_eq!(money(&invoice["baseGrandTotal"]), "155817.12345678901");
    assert_eq!(invoice["exchangeRate"], DocValue::Number(89.55));
    assert_eq!(invoice["submitted"], DocValue::Bool(true));
    assert_eq!(invoice["cancelled"], DocValue::Bool(false));
    assert_eq!(invoice["attachment"], DocValue::Null);
    match &invoice["date"] {
        DocValue::Date(d) => assert_eq!(d.to_string(), "2025-01-15T05:12:01Z"),
        other => panic!("{other:?}"),
    }

    let DocValue::Table(items) = &invoice["items"] else {
        panic!("no items");
    };
    assert_eq!(names(items), ["r1", "r2"]);
    assert_eq!(text(&items[0]["item"]), "Jeans");
    assert_eq!(items[0]["quantity"], DocValue::Number(60.0));
    assert_eq!(money(&items[0]["amount"]), "1680.00000000000");
    assert_eq!(items[0]["idx"], DocValue::Number(0.0));
}

#[test]
fn empty_and_null_money_read_as_zero() {
    let db = open(company());
    let invoice = db.get_doc("SalesInvoice", "SINV-1002").unwrap();
    assert_eq!(money(&invoice["grandTotal"]), "0.00000000000");
    assert_eq!(money(&invoice["baseGrandTotal"]), "0.00000000000");
    let DocValue::Attachment(attachment) = &invoice["attachment"] else {
        panic!("{:?}", invoice["attachment"]);
    };
    assert_eq!(attachment["name"], "a.pdf");
}

#[test]
fn get_doc_of_a_missing_document_is_not_found() {
    let db = open(company());
    assert!(matches!(
        db.get_doc("SalesInvoice", "nope"),
        Err(Error::NotFound(_))
    ));
    assert!(matches!(db.get_doc("Nope", "x"), Err(Error::NotFound(_))));
    assert!(matches!(
        db.core().get("SalesInvoice", "", None),
        Err(Error::Value(_))
    ));
}

#[test]
fn singles_read_their_text_values_by_field_type() {
    let db = open(company());
    let settings = db.get_doc("SystemSettings", "").unwrap();
    assert_eq!(settings["internalPrecision"], DocValue::Number(11.0));
    assert_eq!(settings["darkMode"], DocValue::Bool(false));
    assert_eq!(settings["hideGetStarted"], DocValue::Bool(true));
    assert_eq!(text(&settings["currency"]), "INR");

    // A value whose field Books has since dropped is left out.
    let defaults = db.get_doc("Defaults", "").unwrap();
    assert!(!defaults.contains_key("buyButtonColour"));
}

#[test]
fn get_all_defaults_to_names_newest_first() {
    let db = open(company());
    let rows = db
        .core()
        .get_all("Party", &GetAllOptions::default())
        .unwrap();
    let names: Vec<_> = rows.iter().map(|r| r["name"].clone()).collect();
    let expected = ["Dyno", "Cogs & Co", "Bolt Ltd", "Acme"]
        .map(|n| FieldValue::Value(RawValue::Text(n.into())));
    assert_eq!(names, expected);
    assert_eq!(rows[0].len(), 1);
}

fn parties(filters: QueryFilter) -> Vec<String> {
    let db = open(company());
    let options = GetAllOptions {
        filters,
        order_by: Some(vec!["name".into()]),
        order: Some(Order::Asc),
        ..GetAllOptions::default()
    };
    db.get_all_docs("Party", &options)
        .unwrap()
        .iter()
        .map(|r| text(&r["name"]).to_owned())
        .collect()
}

#[test]
fn filters_work_as_in_books() {
    let filter = |field: &str, filter: Filter| QueryFilter::from([(field.to_owned(), filter)]);

    assert_eq!(parties(filter("role", "Customer".into())), ["Acme"]);
    assert_eq!(parties(filter("role", FilterValue::Null.into())), ["Dyno"]);
    assert_eq!(
        parties(filter("name", Filter::op("includes", "o"))),
        ["Bolt Ltd", "Cogs & Co", "Dyno"]
    );
    assert_eq!(
        parties(filter("name", Filter::op("like", "B%"))),
        ["Bolt Ltd"]
    );
    assert_eq!(
        parties(filter("name", Filter::op("!=", "Acme"))),
        ["Bolt Ltd", "Cogs & Co", "Dyno"]
    );
    assert_eq!(
        parties(filter(
            "role",
            Filter::list("in", vec!["Customer".into(), FilterValue::Null])
        )),
        ["Acme", "Dyno"]
    );
    assert_eq!(
        parties(filter(
            "role",
            Filter::list("not in", vec!["Customer".into(), "Both".into()])
        )),
        ["Bolt Ltd"]
    );
    assert_eq!(
        parties(filter(
            "created",
            Filter::Two(
                ">=".into(),
                FilterOperand::One("2025-02-01".into()),
                "<=".into(),
                FilterOperand::One("2025-03-31".into()),
            )
        )),
        ["Bolt Ltd", "Cogs & Co"]
    );
    assert_eq!(
        parties(filter(
            "created",
            Filter::list("between", vec!["2025-02-01".into(), "2025-03-31".into()])
        )),
        ["Bolt Ltd", "Cogs & Co"]
    );
}

#[test]
fn get_all_limits_and_offsets() {
    let db = open(company());
    let options = GetAllOptions {
        order_by: Some(vec!["name".into()]),
        order: Some(Order::Asc),
        limit: Some(2),
        offset: Some(1),
        ..GetAllOptions::default()
    };
    let rows = db.get_all_docs("Party", &options).unwrap();
    assert_eq!(names(&rows), ["Bolt Ltd", "Cogs & Co"]);

    let options = GetAllOptions {
        offset: Some(3),
        ..options
    };
    let options = GetAllOptions {
        limit: None,
        ..options
    };
    assert_eq!(
        names(&db.get_all_docs("Party", &options).unwrap()),
        ["Dyno"]
    );
}

#[test]
fn get_single_values_matches_any_query() {
    let db = open(company());
    let values = db
        .core()
        .get_single_values(&[
            SingleValueQuery {
                fieldname: "currency".into(),
                parent: Some("SystemSettings".into()),
            },
            SingleValueQuery {
                fieldname: "buyButtonColour".into(),
                parent: None,
            },
        ])
        .unwrap();
    let found: Vec<_> = values
        .iter()
        .map(|v| (v.parent.as_str(), v.fieldname.as_str()))
        .collect();
    assert_eq!(found.len(), 2);
    assert!(found.contains(&("SystemSettings", "currency")));
    assert!(found.contains(&("Defaults", "buyButtonColour")));
}

#[test]
fn exists_checks_documents_and_singles() {
    let db = open(company());
    let core = db.core();
    assert!(core.exists("Party", Some("Acme")).unwrap());
    assert!(!core.exists("Party", Some("Zed")).unwrap());
    assert!(core.exists("Party", None).unwrap());
    assert!(!core.exists("Lead", None).unwrap());
    assert!(core.exists("SystemSettings", None).unwrap());
    assert!(!core.exists("AccountingSettings", None).unwrap());
}

#[test]
fn custom_fields_come_from_the_file() {
    let conn = company();
    conn.execute(
        "insert into CustomField (name, parent, label, fieldname, fieldtype, isRequired, options)
             values ('cf1', 'Item', 'Colour', 'colour', 'Select', 0, 'Red\nBlue')",
        [],
    )
    .unwrap();
    let db = open(conn);
    let colour = db.schema_map()["Item"].field("colour").unwrap();
    assert!(colour.is_custom);
    assert_eq!(colour.options.as_ref().map(Vec::len), Some(2));
}
