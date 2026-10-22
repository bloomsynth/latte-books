//! Makes a few labelled test changes to a copy of a company file, so you can check that
//! Frappe Books v0.36.0 still opens it.
//!
//!     cargo run -p latte-db --example write_test -- "path/to/Latte write test.books.db"
//!
//! It adds the party "Latte Test Party" (first named "Latte Test Party (draft)", then
//! renamed) and a draft journal entry "JV-LATTE-TEST" between two existing accounts, edits
//! one of the entry's lines, and changes one setting and sets it back. It refuses files
//! whose name doesn't contain "test", so it can't touch a real company file by mistake.

use std::process::ExitCode;

use jiff::Timestamp;
use latte_db::{Database, DocValue, DocValueMap, Filter, GetAllOptions, Order};

const PARTY_DRAFT: &str = "Latte Test Party (draft)";
const PARTY: &str = "Latte Test Party";
const ENTRY: &str = "JV-LATTE-TEST";
/// `DEFAULT_USER` in Books.
const USER: &str = "Admin";

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: write_test <path to a copy of a .books.db>");
        return ExitCode::FAILURE;
    };
    let file_name = std::path::Path::new(&path)
        .file_name()
        .map(|f| f.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if !file_name.contains("test") {
        eprintln!("refusing: the file name must contain \"test\", so this only runs on copies");
        return ExitCode::FAILURE;
    }

    match run(&path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut db = Database::open(path, "latte-write-test")?;
    for (schema, name) in [("Party", PARTY), ("Party", PARTY_DRAFT), ("JournalEntry", ENTRY)] {
        if db.core().exists(schema, Some(name))? {
            return Err(format!("{schema} {name} is already there; use a fresh copy").into());
        }
    }

    // The party, under a draft name, then renamed.
    db.insert(
        "Party",
        &with_meta(doc(&[("name", text(PARTY_DRAFT)), ("role", text("Both"))])),
    )?;
    db.rename("Party", PARTY_DRAFT, PARTY)?;
    println!("added the party {PARTY}");

    // A draft journal entry between the first two accounts that aren't groups.
    let accounts = db.get_all_docs(
        "Account",
        &GetAllOptions {
            filters: [("isGroup".to_owned(), Filter::from(false))].into(),
            order_by: Some(vec!["name".into()]),
            order: Some(Order::Asc),
            limit: Some(2),
            ..GetAllOptions::default()
        },
    )?;
    let [first, second] = accounts.as_slice() else {
        return Err("the file needs two accounts that aren't groups".into());
    };
    let line = |account: &DocValue, debit: &str, credit: &str| {
        doc(&[
            ("account", account.clone()),
            ("debit", text(debit)),
            ("credit", text(credit)),
        ])
    };
    let today = Timestamp::now();
    db.insert(
        "JournalEntry",
        &with_meta(doc(&[
            ("name", text(ENTRY)),
            ("numberSeries", text("JV-")),
            ("entryType", text("Journal Entry")),
            ("date", DocValue::Date(today)),
            ("userRemark", text("Written by Latte Books as a test")),
            ("submitted", DocValue::Bool(false)),
            ("cancelled", DocValue::Bool(false)),
            (
                "accounts",
                DocValue::Table(vec![
                    line(&first["name"], "100", "0"),
                    line(&second["name"], "0", "100"),
                ]),
            ),
        ])),
    )?;
    println!(
        "added the journal entry {ENTRY}: 100 from {} to {}",
        show(&second["name"]),
        show(&first["name"])
    );

    // Edit both lines to 150, as Books saves a changed document.
    let mut entry = db.get_doc("JournalEntry", ENTRY)?;
    if let Some(DocValue::Table(rows)) = entry.get_mut("accounts") {
        rows[0].insert("debit".into(), text("150"));
        rows[1].insert("credit".into(), text("150"));
    }
    entry.insert("modifiedBy".into(), text(USER));
    entry.insert("modified".into(), DocValue::Date(Timestamp::now()));
    db.update("JournalEntry", &entry)?;
    println!("changed {ENTRY} to 150");

    // A setting, changed and set back.
    let settings = db.get_doc("SystemSettings", "")?;
    let hide = settings
        .get("hideGetStarted")
        .cloned()
        .unwrap_or(DocValue::Bool(false));
    let flipped = DocValue::Bool(!matches!(hide, DocValue::Bool(true)));
    db.update("SystemSettings", &doc(&[("hideGetStarted", flipped)]))?;
    db.update("SystemSettings", &doc(&[("hideGetStarted", hide.clone())]))?;
    println!("changed SystemSettings.hideGetStarted and set it back to {}", show(&hide));

    println!("\nlatte_changes:");
    let conn = db.core().connection();
    let mut statement = conn.prepare(
        "select timestamp, operation, schemaName, name, coalesce(fields, ''), \
         coalesce(renamedFrom, '') from latte_changes order by id",
    )?;
    let rows = statement.query_map([], |r| {
        Ok(format!(
            "  {} {:<6} {} {} {} {}",
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            match r.get::<_, String>(5)? {
                from if from.is_empty() => String::new(),
                from => format!("(was {from})"),
            }
        ))
    })?;
    for row in rows {
        println!("{}", row?);
    }
    Ok(())
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

/// What `Doc._setBaseMetaValues` sets on a new document.
fn with_meta(mut doc: DocValueMap) -> DocValueMap {
    let now = DocValue::Date(Timestamp::now());
    doc.insert("createdBy".into(), text(USER));
    doc.insert("modifiedBy".into(), text(USER));
    doc.insert("created".into(), now.clone());
    doc.insert("modified".into(), now);
    doc
}

fn show(value: &DocValue) -> String {
    match value {
        DocValue::Text(t) => t.clone(),
        other => format!("{other:?}"),
    }
}
