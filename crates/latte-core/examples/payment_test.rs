//! Pays invoices in full in a copy of a company file, so you can compare what Latte saves
//! and posts with what Frappe Books v0.36.0 does for the same payments.
//!
//!     cargo run -p latte-core --example payment_test -- "path/to/Latte write test.books.db" \
//!         <invoice> [<invoice>…]
//!
//! Each invoice (a sales or purchase invoice's name, like SINV-1001) gets a payment made as
//! the invoice's Make Payment button makes it, dated now, which is then submitted. It
//! prints what each payment saved and posted, and what is still owed. It refuses invoices
//! with nothing outstanding, so it can't pay one twice, and files whose name doesn't
//! contain "test", so it can't touch a real company file by mistake.

use std::collections::HashMap;
use std::process::ExitCode;

use latte_core::Fyo;
use latte_core::models::base_models::invoice::get_payment;
use latte_db::{Database, DocValue, Filter, GetAllOptions, Order, QueryFilter};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [path, invoices @ ..] = args.as_slice() else {
        eprintln!("usage: payment_test <path to a copy of a .books.db> <invoice> [<invoice>…]");
        return ExitCode::FAILURE;
    };
    if invoices.is_empty() {
        eprintln!("usage: payment_test <path to a copy of a .books.db> <invoice> [<invoice>…]");
        return ExitCode::FAILURE;
    }
    let file_name = std::path::Path::new(path)
        .file_name()
        .map(|f| f.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if !file_name.contains("test") {
        eprintln!("refusing: the file name must contain \"test\", so this only runs on copies");
        return ExitCode::FAILURE;
    }

    match run(path, invoices) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(path: &str, invoices: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::open(path, "latte-payment-test")?;
    let mut fyo = Fyo::new(db);
    fyo.register_models(latte_core::models::models(), HashMap::new());

    let mut docs = Vec::new();
    for name in invoices {
        let schema_name = if fyo.db.exists("SalesInvoice", Some(name))? {
            "SalesInvoice"
        } else if fyo.db.exists("PurchaseInvoice", Some(name))? {
            "PurchaseInvoice"
        } else {
            return Err(format!("no sales or purchase invoice {name}").into());
        };
        let doc = fyo.get_doc(schema_name, Some(name))?;
        if get_payment(&doc, &fyo)?.is_none() {
            return Err(format!("{name} isn't submitted or has nothing outstanding").into());
        }
        docs.push(doc);
    }

    for invoice in docs {
        let mut payment = get_payment(&invoice, &fyo)?.expect("checked above");
        payment.sync(&mut fyo)?;
        payment.submit(&mut fyo)?;
        print_payment(&fyo, payment.name().unwrap_or_default())?;

        let schema_name = invoice.schema_name();
        let name = invoice.name().unwrap_or_default();
        let outstanding = fyo.get_value(schema_name, Some(name), "outstandingAmount");
        println!("  {name} still owes {}", show(outstanding.as_ref()));
        let party = match invoice.get("party") {
            Some(DocValue::Text(party)) => party.as_str(),
            _ => "",
        };
        let outstanding = fyo.get_value("Party", Some(party), "outstandingAmount");
        println!("  {party} owes {}", show(outstanding.as_ref()));
        println!();
    }
    Ok(())
}

fn print_payment(fyo: &Fyo, name: &str) -> Result<(), latte_core::Error> {
    let stored = fyo.db.get_doc("Payment", name)?;
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
