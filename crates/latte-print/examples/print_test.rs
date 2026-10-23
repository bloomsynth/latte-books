//! Prints a document of a company file, opened read-only, to PDF and PNG, so you can
//! compare it with the same document printed from Frappe Books v0.36.0.
//!
//!     cargo run -p latte-print --example print_test -- "path/to/company.books.db" \
//!         <schema> <name> <output folder> [<template name>]
//!
//! For example `SalesInvoice SINV-1001 out "Minimal - Sales Invoice"`. Without a template
//! name the default template is used. Writes `<name>.pdf`, `<name>-<page>.png`, and
//! `<name>.json` with the values the template read.

use std::path::Path;
use std::process::ExitCode;

use latte_core::Fyo;
use latte_core::fyo::format::Formatter;
use latte_db::Database;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [path, schema, name, out, template @ ..] = args.as_slice() else {
        eprintln!(
            "usage: print_test <path to a .books.db> <schema> <name> <output folder> [<template>]"
        );
        return ExitCode::FAILURE;
    };
    match run(path, schema, name, Path::new(out), template.first()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(
    path: &str,
    schema: &str,
    name: &str,
    out: &Path,
    template: Option<&String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut fyo = Fyo::with_models(Database::open_read_only(path)?);
    let formatter = Formatter::new(&fyo)?;
    let template = match template {
        Some(wanted) => latte_print::templates(schema)
            .into_iter()
            .find(|t| &t.name == wanted)
            .ok_or("no such template")?,
        None => latte_print::default_template(&fyo, schema).ok_or("no template")?,
    };
    let doc = fyo.get_doc(schema, Some(name))?;
    let printout = latte_print::print(&mut fyo, &formatter, &doc, &template, &|s| s.to_owned())?;

    std::fs::create_dir_all(out)?;
    let values = latte_print::print_values(&mut fyo, &formatter, &doc, &|s| s.to_owned())?;
    std::fs::write(
        out.join(format!("{name}.json")),
        serde_json::to_string_pretty(&values)?,
    )?;
    std::fs::write(out.join(format!("{name}.pdf")), printout.pdf()?)?;
    for (i, png) in printout.png_pages(1.5)?.into_iter().enumerate() {
        std::fs::write(out.join(format!("{name}-{}.png", i + 1)), png)?;
    }
    println!("{}: {} page(s)", template.name, printout.page_count());
    Ok(())
}
