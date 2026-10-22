//! Compares the schemas with the tables in a Frappe Books file, read-only.
//!
//!     cargo run -p latte-schema --example check_db -- path/to/company.books.db
//!
//! Reads the country and custom fields as Books does when it opens a file, builds the schemas,
//! and lists every table, column and single value that doesn't match.

use std::collections::BTreeSet;
use std::process::ExitCode;

use latte_schema::{FieldType, RawCustomField, get_schemas};
use rusqlite::{Connection, OpenFlags, OptionalExtension};

/// Field types that get a column. Port of `sqliteTypeMap` in `backend/helpers.ts`, less the
/// types no schema uses.
fn has_column(fieldtype: FieldType) -> bool {
    !matches!(fieldtype, FieldType::Table | FieldType::Button)
}

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: check_db <path to .books.db>");
        return ExitCode::FAILURE;
    };
    match check(&path) {
        Ok(0) => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn check(path: &str) -> Result<usize, Box<dyn std::error::Error>> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;

    // Books falls back to "in" when the file has no country yet.
    let country: String = db
        .query_row(
            "select value from SingleValue where fieldname = 'countryCode' and parent = 'SystemSettings'",
            [],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or_else(|| "in".into());

    let custom_fields = read_custom_fields(&db)?;
    let schemas = get_schemas(&country, &custom_fields)?;
    println!(
        "country {country}, {} custom fields, {} schemas",
        custom_fields.len(),
        schemas.len()
    );

    let tables: BTreeSet<String> = db
        .prepare(
            "select name from sqlite_master where type = 'table' and name not like 'sqlite_%'",
        )?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;

    let mut problems = 0;
    let mut report = |message: String| {
        problems += 1;
        println!("  {message}");
    };

    for schema in schemas.values().filter(|s| !s.is_single) {
        if !tables.contains(&schema.name) {
            report(format!("{}: no table", schema.name));
            continue;
        }

        let columns: BTreeSet<String> = db
            .prepare(&format!(
                "select name from pragma_table_info('{}')",
                schema.name
            ))?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        let rows: i64 = db.query_row(
            &format!("select count(*) from \"{}\"", schema.name),
            [],
            |r| r.get(0),
        )?;
        println!("{:<28} {rows:>7} rows", schema.name);

        let stored = schema
            .fields
            .iter()
            .filter(|f| !f.computed && f.fieldtype.is_some_and(has_column));
        for field in stored.clone() {
            if !columns.contains(&field.fieldname) {
                report(format!("{}.{}: no column", schema.name, field.fieldname));
            }
        }
        let fieldnames: BTreeSet<&str> = stored.map(|f| f.fieldname.as_str()).collect();
        for column in &columns {
            if !fieldnames.contains(column.as_str()) {
                report(format!("{}.{column}: column has no field", schema.name));
            }
        }
    }

    for table in &tables {
        if !schemas.contains_key(table) {
            report(format!("{table}: table has no schema"));
        }
    }

    let mut singles = db.prepare("select distinct parent, fieldname from SingleValue")?;
    let singles =
        singles.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    for single in singles {
        let (parent, fieldname) = single?;
        match schemas.get(&parent) {
            None => report(format!("SingleValue {parent}: no schema")),
            Some(s) if !s.is_single => report(format!("SingleValue {parent}: not a single")),
            // Books never deletes the values of settings it has dropped, and ignores them, so
            // these are expected in files from older versions.
            Some(s) if s.field(&fieldname).is_none() => {
                println!("  note: SingleValue {parent}.{fieldname}: left over, no field")
            }
            Some(_) => {}
        }
    }

    println!("{problems} differences");
    Ok(problems)
}

fn read_custom_fields(db: &Connection) -> rusqlite::Result<Vec<RawCustomField>> {
    let mut statement = db.prepare(
        "select parent, label, fieldname, fieldtype, isRequired, section, tab, options, target, \
         \"references\", \"default\" from CustomField",
    )?;
    let rows = statement.query_map([], |r| {
        let fieldtype: String = r.get(3)?;
        let fieldtype = serde_json::from_value(fieldtype.clone().into()).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
        })?;
        Ok(RawCustomField {
            parent: r.get(0)?,
            label: r.get(1)?,
            fieldname: r.get(2)?,
            fieldtype,
            is_required: r.get(4)?,
            section: r.get(5)?,
            tab: r.get(6)?,
            options: r.get(7)?,
            target: r.get(8)?,
            references: r.get(9)?,
            default: r.get(10)?,
        })
    })?;
    rows.collect()
}
