use jiff::civil::date;
use latte_db::DocValue;

use crate::Fyo;
use crate::setup::{SetupWizardOptions, fiscal_year, setup_instance};
use crate::tests::{map, text};

/// A new company file in the temp folder, removed when dropped.
pub(crate) struct TempFile(pub(crate) std::path::PathBuf);

impl TempFile {
    pub(crate) fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "latte {name} {} {}.books.db",
            std::process::id(),
            latte_db::get_random_string()
        ));
        Self(path)
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub(crate) fn indian_company(file: &TempFile) -> Fyo {
    let mut options = SetupWizardOptions::for_country("India", date(2026, 10, 23));
    options.company_name = "Acme".into();
    options.bank_name = "Acme Bank".into();
    options.fullname = "Ada".into();
    options.email = "ada@example.com".into();
    setup_instance(&file.0, &options, "test").unwrap()
}

fn count(fyo: &Fyo, table: &str) -> i64 {
    fyo.db
        .core()
        .connection()
        .query_row(&format!("select count(*) from `{table}`"), [], |r| r.get(0))
        .unwrap()
}

fn setting(fyo: &Fyo, schema_name: &str, fieldname: &str) -> Option<DocValue> {
    fyo.get_value(schema_name, None, fieldname)
}

#[test]
fn the_wizard_fills_in_what_the_country_has() {
    let options = SetupWizardOptions::for_country("India", date(2026, 10, 23));
    assert_eq!(options.currency, "INR");
    assert_eq!(options.chart_of_accounts, "India - Chart of Accounts");
    assert_eq!(options.fiscal_year_start, date(2026, 4, 1));
    assert_eq!(options.fiscal_year_end, date(2027, 3, 31));

    let options = SetupWizardOptions::for_country("Switzerland", date(2026, 2, 1));
    assert_eq!(options.currency, "CHF");
    assert_eq!(options.chart_of_accounts, "Standard Chart of Accounts");

    // In January to March, the year started last year.
    assert_eq!(
        fiscal_year("04-01", true, date(2026, 2, 1)),
        Some(date(2025, 4, 1))
    );
    assert_eq!(
        fiscal_year("03-31", false, date(2026, 2, 1)),
        Some(date(2026, 3, 31))
    );
    assert_eq!(fiscal_year("", true, date(2026, 2, 1)), None);
}

#[test]
fn setting_up_an_indian_company_makes_what_books_makes() {
    let file = TempFile::new("india");
    let fyo = indian_company(&file);

    assert_eq!(count(&fyo, "Account"), 79);
    assert_eq!(
        fyo.db
            .get_doc("Account", "Acme Bank")
            .unwrap()
            .get("parentAccount"),
        Some(&DocValue::Text("Bank Accounts".into()))
    );
    assert_eq!(count(&fyo, "Tax"), 18);
    assert_eq!(count(&fyo, "TaxDetail"), 26);
    assert_eq!(count(&fyo, "Currency"), 147);
    assert_eq!(count(&fyo, "UOM"), 6);
    assert_eq!(count(&fyo, "PaymentMethod"), 3);
    assert_eq!(count(&fyo, "PatchRun"), 9);
    assert_eq!(count(&fyo, "NumberSeries"), 9);
    assert_eq!(count(&fyo, "Location"), 1);

    let text = |value: &str| Some(text(value));
    assert_eq!(setting(&fyo, "SystemSettings", "version"), text("0.36.0"));
    assert_eq!(setting(&fyo, "SystemSettings", "countryCode"), text("in"));
    assert_eq!(setting(&fyo, "SystemSettings", "locale"), text("en-IN"));
    assert_eq!(
        setting(&fyo, "AccountingSettings", "companyName"),
        text("Acme")
    );
    assert_eq!(
        setting(&fyo, "AccountingSettings", "discountAccount"),
        text("Discounts")
    );
    assert_eq!(
        setting(&fyo, "AccountingSettings", "writeOffAccount"),
        text("Write Off")
    );
    assert_eq!(
        setting(&fyo, "AccountingSettings", "roundOffAccount"),
        text("Rounded Off")
    );
    assert_eq!(
        setting(&fyo, "AccountingSettings", "setupComplete"),
        Some(DocValue::Bool(true))
    );
    assert_eq!(
        setting(&fyo, "Defaults", "salesInvoiceNumberSeries"),
        text("SINV-")
    );
    assert_eq!(
        setting(&fyo, "InventorySettings", "stockInHand"),
        text("Stock In Hand")
    );
    assert_eq!(
        setting(&fyo, "InventorySettings", "defaultLocation"),
        text("Stores")
    );
    // Links to what doesn't exist yet are dropped, as Books does.
    assert_eq!(setting(&fyo, "POSSettings", "cashAccount"), None);
    assert_eq!(setting(&fyo, "POSSettings", "posUI"), text("Classic"));

    let fiscal_year_start = fyo.db.core().connection().query_row(
        "select value from SingleValue where fieldname = 'fiscalYearStart'",
        [],
        |r| r.get::<_, String>(0),
    );
    assert_eq!(fiscal_year_start.unwrap(), "2026-04-01");
}

#[test]
fn a_new_company_can_be_opened_and_used() {
    let file = TempFile::new("open");
    drop(indian_company(&file));
    let mut fyo = Fyo::with_models(latte_db::Database::open(&file.0, "test").unwrap());
    let mut party = fyo
        .get_new_doc(
            "Party",
            map([("name", text("Bob")), ("role", text("Customer"))]),
        )
        .unwrap();
    party.sync(&mut fyo).unwrap();
    assert_eq!(
        fyo.get_value("Party", Some("Bob"), "defaultAccount"),
        Some(text("Debtors"))
    );
}

#[test]
fn an_existing_file_is_never_replaced() {
    let file = TempFile::new("exists");
    std::fs::write(&file.0, b"keep").unwrap();
    let options = SetupWizardOptions::for_country("India", date(2026, 10, 23));
    assert!(setup_instance(&file.0, &options, "test").is_err());
    assert_eq!(std::fs::read(&file.0).unwrap(), b"keep");
}

#[test]
fn every_chart_of_accounts_can_be_set_up() {
    for (chart, _) in crate::setup::COA_LIST {
        let file = TempFile::new("chart");
        let mut options = SetupWizardOptions::for_country("France", date(2026, 10, 23));
        options.company_name = "Acme".into();
        options.bank_name = "Acme Bank".into();
        options.fullname = "Ada".into();
        options.email = "ada@example.com".into();
        options.fiscal_year_start = date(2026, 1, 1);
        options.fiscal_year_end = date(2026, 12, 31);
        options.chart_of_accounts = chart.into();
        let fyo =
            setup_instance(&file.0, &options, "test").unwrap_or_else(|e| panic!("{chart}: {e}"));
        assert!(count(&fyo, "Account") > 10, "{chart}");
    }
}
