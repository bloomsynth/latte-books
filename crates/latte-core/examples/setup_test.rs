//! Creates a company file as Frappe Books v0.36.0's setup wizard does, so you can compare
//! it with one Books made.
//!
//!     cargo run -p latte-core --example setup_test -- "path/to/new test.books.db" \
//!         <country> <company name> <bank name> [<chart of accounts>]
//!
//! The file must not exist yet, and its name must contain "test". The fiscal year,
//! currency and chart of accounts are the wizard's defaults for the country; a fiscal
//! year the country has none for is April to March.

use std::process::ExitCode;

use jiff::civil::Date;
use latte_core::setup::{SetupWizardOptions, setup_instance, today};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [path, country, company, bank, rest @ ..] = args.as_slice() else {
        eprintln!(
            "usage: setup_test <new .books.db with \"test\" in its name> <country> <company> \
             <bank> [<chart of accounts>]"
        );
        return ExitCode::FAILURE;
    };
    if !path.to_lowercase().contains("test") {
        eprintln!("error: the file name must contain \"test\"");
        return ExitCode::FAILURE;
    }

    let today = today();
    let mut options = SetupWizardOptions::for_country(country, today);
    options.company_name = company.clone();
    options.bank_name = bank.clone();
    options.fullname = "Test".into();
    options.email = "test@example.com".into();
    if options.fiscal_year_start == Date::constant(1, 1, 1) {
        let year = if today.month() < 4 {
            today.year() - 1
        } else {
            today.year()
        };
        options.fiscal_year_start = Date::constant(year, 4, 1);
        options.fiscal_year_end = Date::constant(year + 1, 3, 31);
    }
    if let Some(chart) = rest.first() {
        options.chart_of_accounts = chart.clone();
    }

    match setup_instance(path, &options, "setup_test") {
        Ok(_) => {
            println!("created {path}: {options:#?}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
