//! Prints every document in a Frappe Books file, with its child rows, read-only.
//!
//!     cargo run -p latte-db --example dump -- path/to/company.books.db [SchemaName]
//!
//! Empty values are left out. Money is shown exactly, with no rounding.

use std::process::ExitCode;

use latte_db::{Database, DocValue, DocValueMap, GetAllOptions, Order};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: dump <path to .books.db> [SchemaName]");
        return ExitCode::FAILURE;
    };
    let only = args.next();

    let db = match Database::open_read_only(&path) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut errors = 0;
    let mut documents = 0;
    for schema in db.schema_map().values() {
        if schema.is_child || only.as_ref().is_some_and(|o| *o != schema.name) {
            continue;
        }

        if schema.is_single {
            match db.core().exists(&schema.name, None) {
                Ok(false) => continue,
                Ok(true) => {}
                Err(e) => {
                    errors += 1;
                    println!("!! {}: {e}", schema.name);
                    continue;
                }
            }
            println!("== {} (single)", schema.name);
            match db.get_doc(&schema.name, "") {
                Ok(doc) => print_doc(&doc, 1),
                Err(e) => {
                    errors += 1;
                    println!("  !! {e}");
                }
            }
            continue;
        }

        let options = GetAllOptions {
            order: Some(Order::Asc),
            ..GetAllOptions::default()
        };
        let rows = match db.get_all_docs(&schema.name, &options) {
            Ok(rows) => rows,
            Err(e) => {
                errors += 1;
                println!("!! {}: {e}", schema.name);
                continue;
            }
        };
        if rows.is_empty() {
            continue;
        }

        println!("== {} ({})", schema.name, rows.len());
        for row in rows {
            let Some(DocValue::Text(name)) = row.get("name") else {
                continue;
            };
            documents += 1;
            println!("{name}");
            match db.get_doc(&schema.name, name) {
                Ok(doc) => print_doc(&doc, 1),
                Err(e) => {
                    errors += 1;
                    println!("  !! {e}");
                }
            }
        }
    }

    eprintln!("{documents} documents, {errors} errors");
    if errors == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn print_doc(doc: &DocValueMap, depth: usize) {
    let indent = "  ".repeat(depth);
    for (fieldname, value) in doc {
        match value {
            DocValue::Null => {}
            DocValue::Text(t) if t.is_empty() => {}
            DocValue::Table(rows) if rows.is_empty() => {}
            DocValue::Table(rows) => {
                println!("{indent}{fieldname}:");
                for (i, row) in rows.iter().enumerate() {
                    println!("{indent}  [{i}]");
                    print_doc(row, depth + 2);
                }
            }
            value => println!("{indent}{fieldname}: {}", show(value)),
        }
    }
}

fn show(value: &DocValue) -> String {
    match value {
        DocValue::Text(t) => t.replace('\n', "\\n"),
        DocValue::Number(n) => n.to_string(),
        DocValue::Bool(b) => b.to_string(),
        // Not `m.currency`: that is the company's currency, and a document in another
        // currency names its own in a field.
        DocValue::Money(m) => m.to_string(),
        DocValue::Date(d) => d.to_string(),
        DocValue::Attachment(a) => format!("attachment {}", a["name"]),
        DocValue::Null | DocValue::Table(_) => String::new(),
    }
}
