//! Prints a report of a company file, opened read-only, so you can compare it with the
//! same report in Frappe Books v0.36.0.
//!
//!     cargo run -p latte-core --example report_test -- "path/to/company.books.db" \
//!         <report> [<filter>=<value>…] [--csv]
//!
//! `report` is one of general-ledger, profit-and-loss, balance-sheet and trial-balance.
//! Filters are the report's filter fieldnames: dates as 2025-04-01, checks as true or
//! false, numbers as numbers, and anything else as text, for example
//! `fromDate=2025-04-01 toDate=2026-03-31` or `basedOn="Fiscal Year" fromYear=2025
//! toYear=2026`. `--csv` prints the CSV export instead of the table.

use std::process::ExitCode;

use latte_core::Fyo;
use latte_core::fyo::format::Formatter;
use latte_core::reports::{FilterKind, FilterValue, REPORTS, csv, get_report};
use latte_db::Database;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [path, report_name, filters @ ..] = args.as_slice() else {
        let names: Vec<_> = REPORTS.iter().map(|(name, _)| *name).collect();
        eprintln!(
            "usage: report_test <path to a .books.db> <{}> [<filter>=<value>…] [--csv]",
            names.join("|")
        );
        return ExitCode::FAILURE;
    };
    match run(path, report_name, filters) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(path: &str, report_name: &str, args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let fyo = Fyo::new(Database::open_read_only(path)?);
    let formatter = Formatter::new(&fyo)?;
    let mut report = get_report(report_name).ok_or("no such report")?;

    let as_csv = args.iter().any(|arg| arg == "--csv");
    for arg in args.iter().filter(|arg| *arg != "--csv") {
        let (fieldname, value) = arg.split_once('=').ok_or("filters are <filter>=<value>")?;
        let kind = report
            .filters(&fyo)
            .into_iter()
            .find(|filter| filter.fieldname == fieldname)
            .map(|filter| filter.kind)
            .ok_or_else(|| format!("{report_name} has no filter {fieldname}"))?;
        let value = match kind {
            FilterKind::Date => FilterValue::Date(value.parse()?),
            FilterKind::Check => FilterValue::Bool(value == "true"),
            FilterKind::Int { .. } => FilterValue::Int(value.parse()?),
            _ => FilterValue::Text(value.to_owned()),
        };
        report.set(fieldname, value);
    }

    let rows = report.run(&fyo, &formatter)?;
    let columns = report.columns(&fyo, &formatter)?;
    if as_csv {
        println!("{}", csv(&columns, &rows, formatter.display_precision));
        return Ok(());
    }

    println!("{}", report.title());
    for filter in report.filters(&fyo) {
        println!("  {}: {:?}", filter.label, report.get(&filter.fieldname));
    }
    println!();
    let mut table: Vec<Vec<String>> = vec![columns.iter().map(|c| c.label.clone()).collect()];
    for row in &rows {
        table.push(
            row.cells
                .iter()
                .map(|cell| format!("{}{}", "  ".repeat(cell.indent), cell.value))
                .collect(),
        );
    }
    let widths: Vec<usize> = (0..columns.len())
        .map(|i| {
            table
                .iter()
                .map(|row| row.get(i).map_or(0, |cell| cell.chars().count()))
                .max()
                .unwrap_or(0)
        })
        .collect();
    for row in &table {
        let line: Vec<String> = row
            .iter()
            .zip(&widths)
            .zip(&columns)
            .map(|((cell, width), column)| match column.align {
                latte_core::reports::Align::Right => format!("{cell:>width$}"),
                _ => format!("{cell:<width$}"),
            })
            .collect();
        println!("{}", line.join("  ").trim_end());
    }
    Ok(())
}
