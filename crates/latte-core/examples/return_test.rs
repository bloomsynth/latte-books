//! Makes a return of an invoice in a copy of a company file, so you can compare what Latte
//! saves and posts with what Frappe Books v0.36.0 does for the same return.
//!
//!     cargo run -p latte-core --example return_test -- "path/to/Latte write test.books.db" \
//!         <invoice> [<quantity>]
//!
//! The invoice (a sales or purchase invoice's name, like SINV-1001) gets a return made as
//! its Make Return button makes it, dated now, of all that is left to return, or of
//! `quantity` of its first row if one is given, which is then submitted. It prints what the
//! return saved and posted, and what is owed after it. It refuses invoices with nothing
//! left to return, and files whose name doesn't contain "test", so it can't touch a real
//! company file by mistake.

use std::collections::HashMap;
use std::process::ExitCode;

use latte_core::Fyo;
use latte_core::models::base_models::invoice::get_return_doc;
use latte_db::{Database, DocValue, Filter, GetAllOptions, Order, QueryFilter};

const USAGE: &str = "usage: return_test <path to a copy of a .books.db> <invoice> [<quantity>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (path, invoice, quantity) = match args.as_slice() {
        [path, invoice] => (path.as_str(), invoice.as_str(), None),
        [path, invoice, quantity] => match quantity.parse::<f64>() {
            Ok(quantity) => (path.as_str(), invoice.as_str(), Some(quantity)),
            Err(_) => {
                eprintln!("{USAGE}");
                return ExitCode::FAILURE;
            }
        },
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    let file_name = std::path::Path::new(path)
        .file_name()
        .map(|f| f.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if !file_name.contains("test") {
        eprintln!("refusing: the file name must contain \"test\", so this only runs on copies");
        return ExitCode::FAILURE;
    }

    match run(path, invoice, quantity) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(path: &str, name: &str, quantity: Option<f64>) -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::open(path, "latte-return-test")?;
    let mut fyo = Fyo::new(db);
    fyo.register_models(latte_core::models::models(), HashMap::new());

    let schema_name = if fyo.db.exists("SalesInvoice", Some(name))? {
        "SalesInvoice"
    } else if fyo.db.exists("PurchaseInvoice", Some(name))? {
        "PurchaseInvoice"
    } else {
        return Err(format!("no sales or purchase invoice {name}").into());
    };
    let invoice = fyo.get_doc(schema_name, Some(name))?;
    let Some(mut doc) = get_return_doc(&invoice, &fyo)? else {
        return Err(format!("{name} isn't submitted, or is a return").into());
    };
    if doc.table("items").is_empty() {
        return Err(format!("{name} has nothing left to return").into());
    }
    if let Some(quantity) = quantity {
        let quantity = -quantity.abs();
        doc.set_child(&mut fyo, "items", 0, "quantity", DocValue::Number(quantity))?;
    }
    doc.sync(&mut fyo)?;
    doc.submit(&mut fyo)?;
    let return_name = doc.name().unwrap_or_default().to_owned();
    print_return(&fyo, schema_name, &return_name)?;

    for (label, fieldname) in [
        ("is returned", "isReturned"),
        ("is fully returned", "isFullyReturned"),
    ] {
        let value = fyo.get_value(schema_name, Some(name), fieldname);
        println!("  {name} {label}: {}", show(value.as_ref()));
    }
    for invoice in [name, return_name.as_str()] {
        let outstanding = fyo.get_value(schema_name, Some(invoice), "outstandingAmount");
        println!("  {invoice} outstanding: {}", show(outstanding.as_ref()));
    }
    let party = match invoice.get("party") {
        Some(DocValue::Text(party)) => party.as_str(),
        _ => "",
    };
    let outstanding = fyo.get_value("Party", Some(party), "outstandingAmount");
    println!("  {party} owes {}", show(outstanding.as_ref()));
    Ok(())
}

fn print_return(fyo: &Fyo, schema_name: &str, name: &str) -> Result<(), latte_core::Error> {
    let stored = fyo.db.get_doc(schema_name, name)?;
    println!("{name}");
    for (fieldname, value) in &stored {
        if let DocValue::Table(rows) = value {
            for (idx, row) in rows.iter().enumerate() {
                println!("  {fieldname}[{idx}]");
                for (fieldname, value) in row {
                    println!("    {fieldname}: {}", show(Some(value)));
                }
            }
        } else {
            println!("  {fieldname}: {}", show(Some(value)));
        }
    }

    let options = GetAllOptions {
        fields: Some(vec!["*".to_owned()]),
        filters: QueryFilter::from([("referenceName".to_owned(), Filter::from(name))]),
        order_by: Some(vec!["name".to_owned()]),
        order: Some(Order::Asc),
        ..GetAllOptions::default()
    };
    println!("  ledger entries");
    for entry in fyo.db.get_all_docs("AccountingLedgerEntry", &options)? {
        let field = |fieldname: &str| show(entry.get(fieldname));
        println!(
            "    {} {} | party {} | date {} | debit {} credit {} | reverted {}",
            field("name"),
            field("account"),
            field("party"),
            field("date"),
            field("debit"),
            field("credit"),
            field("reverted"),
        );
    }
    Ok(())
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
