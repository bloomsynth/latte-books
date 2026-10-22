//! Submits two labelled journal entries in a copy of a company file and cancels the second,
//! so you can compare the ledger entries Latte posts with those Frappe Books v0.36.0 posts.
//!
//!     cargo run -p latte-core --example post_test -- "path/to/Latte write test.books.db"
//!
//! Both entries are between the first two accounts that aren't groups, dated today, and
//! remarked "Latte post test: submitted" (123.45) and "Latte post test: cancelled" (67.89).
//! It prints each entry's ledger entries. It refuses files whose name doesn't contain
//! "test", so it can't touch a real company file by mistake.

use std::collections::HashMap;
use std::process::ExitCode;

use jiff::Zoned;
use latte_core::Fyo;
use latte_db::{Database, DocValue, DocValueMap, Filter, GetAllOptions, Order, QueryFilter};

const SUBMITTED: &str = "Latte post test: submitted";
const CANCELLED: &str = "Latte post test: cancelled";

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: post_test <path to a copy of a .books.db>");
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
    let db = Database::open(path, "latte-post-test")?;
    let mut fyo = Fyo::new(db);
    fyo.register_models(latte_core::models::models(), HashMap::new());

    for remark in [SUBMITTED, CANCELLED] {
        let filters = QueryFilter::from([("userRemark".to_owned(), Filter::from(remark))]);
        let options = GetAllOptions {
            filters,
            ..GetAllOptions::default()
        };
        if !fyo.db.get_all_docs("JournalEntry", &options)?.is_empty() {
            return Err(format!("\"{remark}\" is already there; use a fresh copy").into());
        }
    }

    let accounts = fyo.db.get_all_docs(
        "Account",
        &GetAllOptions {
            filters: [("isGroup".to_owned(), Filter::from(false))].into(),
            order_by: Some(vec!["name".into()]),
            order: Some(Order::Asc),
            limit: Some(2),
            ..GetAllOptions::default()
        },
    )?;
    let [debit_account, credit_account] = accounts.as_slice() else {
        return Err("the file needs two accounts that aren't groups".into());
    };
    let debit_account = text_of(debit_account.get("name"));
    let credit_account = text_of(credit_account.get("name"));
    let today = Zoned::now().date().to_string();
    println!("debit {debit_account}, credit {credit_account}, dated {today}\n");

    let mut submitted = journal_entry(
        &fyo,
        SUBMITTED,
        &today,
        &debit_account,
        &credit_account,
        123.45,
    )?;
    submitted.sync(&mut fyo)?;
    submitted.submit(&mut fyo)?;

    let mut cancelled = journal_entry(
        &fyo,
        CANCELLED,
        &today,
        &debit_account,
        &credit_account,
        67.89,
    )?;
    cancelled.sync(&mut fyo)?;
    cancelled.submit(&mut fyo)?;
    cancelled.cancel(&mut fyo)?;

    for doc in [&submitted, &cancelled] {
        let name = doc.name().unwrap_or_default();
        println!("{name} ({})", text_of(doc.get("userRemark")));
        print_ledger_entries(&fyo, name)?;
        println!();
    }
    Ok(())
}

fn journal_entry(
    fyo: &Fyo,
    remark: &str,
    date: &str,
    debit_account: &str,
    credit_account: &str,
    amount: f64,
) -> Result<latte_core::Doc, latte_core::Error> {
    let text = |value: &str| DocValue::Text(value.to_owned());
    let row = |account: &str, debit: f64, credit: f64| -> DocValueMap {
        DocValueMap::from([
            ("account".to_owned(), text(account)),
            ("debit".to_owned(), DocValue::Number(debit)),
            ("credit".to_owned(), DocValue::Number(credit)),
        ])
    };
    fyo.get_new_doc(
        "JournalEntry",
        DocValueMap::from([
            ("entryType".to_owned(), text("Journal Entry")),
            ("date".to_owned(), text(date)),
            ("userRemark".to_owned(), text(remark)),
            (
                "accounts".to_owned(),
                DocValue::Table(vec![
                    row(debit_account, amount, 0.0),
                    row(credit_account, 0.0, amount),
                ]),
            ),
        ]),
    )
}

fn print_ledger_entries(fyo: &Fyo, name: &str) -> Result<(), latte_core::Error> {
    let options = GetAllOptions {
        fields: Some(vec!["*".to_owned()]),
        filters: QueryFilter::from([("referenceName".to_owned(), Filter::from(name))]),
        order_by: Some(vec!["name".to_owned()]),
        order: Some(Order::Asc),
        ..GetAllOptions::default()
    };
    for entry in fyo.db.get_all_docs("AccountingLedgerEntry", &options)? {
        let field = |fieldname: &str| show(entry.get(fieldname));
        println!(
            "  {} {} | party {} | date {} | debit {} credit {} | reverted {} reverts {}",
            field("name"),
            field("account"),
            field("party"),
            field("date"),
            field("debit"),
            field("credit"),
            field("reverted"),
            field("reverts"),
        );
    }
    Ok(())
}

fn text_of(value: Option<&DocValue>) -> String {
    match value {
        Some(DocValue::Text(text)) => text.clone(),
        _ => String::new(),
    }
}

fn show(value: Option<&DocValue>) -> String {
    match value {
        None | Some(DocValue::Null) => "null".into(),
        Some(DocValue::Text(text)) => text.clone(),
        Some(DocValue::Money(money)) => money.store(),
        Some(DocValue::Date(date)) => latte_db::to_iso_string(*date),
        Some(value) => format!("{value:?}"),
    }
}
