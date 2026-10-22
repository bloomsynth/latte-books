//! Submits two sales invoices in a copy of a company file, so you can compare what Latte
//! saves and posts with what Frappe Books v0.36.0 does for the same invoices.
//!
//!     cargo run -p latte-core --example invoice_test -- "path/to/Latte write test.books.db" \
//!         <party> <item> <foreign currency party> <foreign currency item>
//!
//! Invoice A is for the party, 2 of the item at a 10% discount (with discounting on).
//! Invoice B is for the foreign currency party, 1 of the item at an exchange rate of 90.
//! Both are dated today and have the terms "Latte invoice test: A" or "… B". It prints
//! what each saved and posted. It refuses files whose name doesn't contain "test", so it
//! can't touch a real company file by mistake.

use std::collections::HashMap;
use std::process::ExitCode;

use latte_core::{Doc, Fyo};
use latte_db::{Database, DocValue, DocValueMap, Filter, GetAllOptions, Order, QueryFilter};

const TERMS_A: &str = "Latte invoice test: A";
const TERMS_B: &str = "Latte invoice test: B";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [path, party, item, foreign_party, foreign_item] = args.as_slice() else {
        eprintln!(
            "usage: invoice_test <path to a copy of a .books.db> <party> <item> \
             <foreign currency party> <foreign currency item>"
        );
        return ExitCode::FAILURE;
    };
    let file_name = std::path::Path::new(path)
        .file_name()
        .map(|f| f.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if !file_name.contains("test") {
        eprintln!("refusing: the file name must contain \"test\", so this only runs on copies");
        return ExitCode::FAILURE;
    }

    match run(path, party, item, foreign_party, foreign_item) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(
    path: &str,
    party: &str,
    item: &str,
    foreign_party: &str,
    foreign_item: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::open(path, "latte-invoice-test")?;
    let mut fyo = Fyo::new(db);
    fyo.register_models(latte_core::models::models(), HashMap::new());

    for terms in [TERMS_A, TERMS_B] {
        let options = GetAllOptions {
            filters: QueryFilter::from([("terms".to_owned(), Filter::from(terms))]),
            ..GetAllOptions::default()
        };
        if !fyo.db.get_all_docs("SalesInvoice", &options)?.is_empty() {
            return Err(format!("\"{terms}\" is already there; use a fresh copy").into());
        }
    }

    let mut a = sales_invoice(&mut fyo, party, TERMS_A, None, item, 2.0, Some(10.0))?;
    a.sync(&mut fyo)?;
    a.submit(&mut fyo)?;

    let mut b = sales_invoice(
        &mut fyo,
        foreign_party,
        TERMS_B,
        Some(90.0),
        foreign_item,
        1.0,
        None,
    )?;
    b.sync(&mut fyo)?;
    b.submit(&mut fyo)?;

    for doc in [&a, &b] {
        print_invoice(&fyo, doc)?;
        println!();
    }
    for party in [party, foreign_party] {
        let outstanding = fyo.get_value("Party", Some(party), "outstandingAmount");
        println!("{party} owes {}", show(outstanding.as_ref()));
    }
    Ok(())
}

/// An invoice made as the form makes it: the party, then the exchange rate, then a row
/// with its item, quantity and discount.
fn sales_invoice(
    fyo: &mut Fyo,
    party: &str,
    terms: &str,
    exchange_rate: Option<f64>,
    item: &str,
    quantity: f64,
    discount_percent: Option<f64>,
) -> Result<Doc, latte_core::Error> {
    let text = |value: &str| DocValue::Text(value.to_owned());
    let mut doc = fyo.get_new_doc("SalesInvoice", DocValueMap::new())?;
    doc.set(fyo, "party", text(party))?;
    if let Some(rate) = exchange_rate {
        doc.set(fyo, "exchangeRate", DocValue::Number(rate))?;
    }
    doc.set(fyo, "terms", text(terms))?;

    doc.append(fyo, "items", DocValueMap::new())?;
    doc.set_child(fyo, "items", 0, "item", text(item))?;
    doc.set_child(fyo, "items", 0, "quantity", DocValue::Number(quantity))?;
    if let Some(percent) = discount_percent {
        doc.set_child(
            fyo,
            "items",
            0,
            "itemDiscountPercent",
            DocValue::Number(percent),
        )?;
    }
    Ok(doc)
}

fn print_invoice(fyo: &Fyo, doc: &Doc) -> Result<(), latte_core::Error> {
    let name = doc.name().unwrap_or_default();
    let stored = fyo.db.get_doc("SalesInvoice", name)?;
    println!("{name} ({})", show(stored.get("terms")));
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
