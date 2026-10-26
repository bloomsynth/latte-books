// Tests of exporting a list.

use latte_db::{Filter, FilterValue, QueryFilter};

use super::{csv_export, export_fields, export_table_fields, json_export};
use crate::importer::parse_csv;
use crate::tests::{company, journal_entry};

#[test]
fn attachments_and_computed_fields_are_left_out() {
    let fyo = company();
    let fields = export_fields(&fyo.schema("Item").unwrap().fields, &[]);
    let image = fields.iter().find(|f| f.fieldname == "image").unwrap();
    assert!(!image.export);
    assert!(fields.iter().any(|f| f.fieldname == "name" && f.export));
    let tables = export_table_fields(&fyo, &fyo.schema("JournalEntry").unwrap().fields);
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].target, "JournalEntryAccount");
    assert!(tables[0].fields.iter().all(|f| f.fieldname != "name"));
}

#[test]
fn a_document_has_a_row_for_each_child_row() {
    let mut fyo = company();
    let mut entry = journal_entry(&fyo);
    entry.sync(&mut fyo).unwrap();
    let name = entry.name().unwrap().to_owned();

    let schema = fyo.schema("JournalEntry").unwrap().clone();
    let fields = export_fields(&schema.fields, &[]);
    let tables = export_table_fields(&fyo, &schema.fields);
    let csv = csv_export(
        &fyo,
        "JournalEntry",
        &fields,
        &tables,
        None,
        &QueryFilter::new(),
    )
    .unwrap();
    let rows = parse_csv(&csv);
    // Labels, keys, then the two accounts with the entry's values on each.
    assert_eq!(rows.len(), 4);
    assert!(rows[1].contains(&"JournalEntry.name".to_owned()));
    assert!(rows[1].contains(&"JournalEntryAccount.account".to_owned()));
    let name_col = rows[1]
        .iter()
        .position(|k| k == "JournalEntry.name")
        .unwrap();
    let account_col = rows[1]
        .iter()
        .position(|k| k == "JournalEntryAccount.account")
        .unwrap();
    let debit_col = rows[1]
        .iter()
        .position(|k| k == "JournalEntryAccount.debit")
        .unwrap();
    assert_eq!(rows[2][name_col], name);
    assert_eq!(rows[3][name_col], name);
    // Books orders the rows by `idx`, descending as its queries are by default.
    assert_eq!(rows[2][account_col], "Sales");
    assert_eq!(rows[3][account_col], "Cash");
    // Money as a number.
    assert_eq!(rows[3][debit_col], "250");

    // A filter that matches nothing leaves only the headers.
    let none = QueryFilter::from([("name".to_owned(), Filter::Equals(FilterValue::from("none")))]);
    let csv = csv_export(&fyo, "JournalEntry", &fields, &tables, None, &none).unwrap();
    assert_eq!(parse_csv(&csv).len(), 2);
}

#[test]
fn json_nests_the_rows_in_their_document() {
    let mut fyo = company();
    let mut entry = journal_entry(&fyo);
    entry.sync(&mut fyo).unwrap();

    let schema = fyo.schema("JournalEntry").unwrap().clone();
    let fields = export_fields(&schema.fields, &[]);
    let tables = export_table_fields(&fyo, &schema.fields);
    let json = json_export(
        &fyo,
        "JournalEntry",
        &fields,
        &tables,
        Some(10),
        &QueryFilter::new(),
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let accounts = value[0]["accounts"].as_array().unwrap();
    assert_eq!(accounts.len(), 2);
    assert_eq!(accounts[1]["account"], "Cash");
    assert_eq!(accounts[1]["debit"], 250.0);
    assert!(accounts[0].get("parent").is_none());
}
