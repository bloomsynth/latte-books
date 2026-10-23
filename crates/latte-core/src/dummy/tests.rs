use latte_db::{DocValue, GetAllOptions};

use super::*;
use crate::setup_tests::TempFile;

fn count(fyo: &Fyo, schema_name: &str) -> usize {
    let options = GetAllOptions {
        limit: None,
        ..GetAllOptions::default()
    };
    fyo.db.get_all_docs(schema_name, &options).unwrap().len()
}

fn sum(fyo: &Fyo, column: &str) -> f64 {
    fyo.db
        .core()
        .connection()
        .query_row(
            &format!("select coalesce(sum(cast({column} as real)), 0) from AccountingLedgerEntry"),
            [],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn the_demo_company_has_a_year_of_balanced_books() {
    let file = TempFile::new("demo");
    let mut rng = fastrand::Rng::with_seed(7);
    let mut stages = Vec::new();
    let mut fyo = setup_dummy_instance(&file.0, 1, 10, "test", &mut rng, &mut |stage, _| {
        stages.push(stage.to_owned());
    })
    .unwrap();

    assert_eq!(
        fyo.get_value("AccountingSettings", None, "companyName"),
        Some(DocValue::Text("Flo's Clothes".into()))
    );
    assert_eq!(count(&fyo, "Item"), 16);
    assert_eq!(count(&fyo, "Party"), 210);
    assert!(count(&fyo, "SalesInvoice") > 10);
    assert!(count(&fyo, "PurchaseInvoice") > 12 * 4);
    assert_eq!(count(&fyo, "JournalEntry"), 2);
    assert!(count(&fyo, "Payment") > count(&fyo, "SalesInvoice"));
    assert!(stages.iter().any(|s| s.starts_with("Syncing Payments")));

    // Everything is submitted and the ledger balances.
    for schema_name in ["SalesInvoice", "PurchaseInvoice", "Payment", "JournalEntry"] {
        let drafts: i64 = fyo
            .db
            .core()
            .connection()
            .query_row(
                &format!("select count(*) from {schema_name} where submitted = 0"),
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(drafts, 0, "{schema_name} drafts");
    }
    assert!((sum(&fyo, "debit") - sum(&fyo, "credit")).abs() < 0.01);

    // No item twice on a sales invoice.
    for invoice in fyo
        .db
        .get_all_docs("SalesInvoice", &GetAllOptions::default())
        .unwrap()
    {
        let Some(DocValue::Text(name)) = invoice.get("name") else {
            panic!("no name");
        };
        let doc = fyo.get_doc("SalesInvoice", Some(name)).unwrap();
        let mut items: Vec<String> = doc
            .table("items")
            .iter()
            .map(|r| format!("{:?}", r.get("item")))
            .collect();
        let len = items.len();
        items.sort();
        items.dedup();
        assert_eq!(items.len(), len, "{name}");
    }

    let fiscal_year_end = fyo.get_value("AccountingSettings", None, "fiscalYearEnd");
    let Some(DocValue::Date(end)) = fiscal_year_end else {
        panic!("no fiscal year end");
    };
    let end = crate::fyo::format::local_date(end);
    assert_eq!((end.month(), end.day()), (3, 31));
    assert_eq!(
        fyo.get_value("PrintSettings", None, "color"),
        Some(DocValue::Text("#F687B3".into()))
    );
    assert!(crate::fyo::model::helpers::is_truthy(
        fyo.get_value("SystemSettings", None, "hideGetStarted")
            .as_ref()
    ));
}
