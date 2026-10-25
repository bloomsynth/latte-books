//! Prints a document of a company file, opened read-only, to PDF and PNG, so you can
//! compare it with the same document printed from Frappe Books v0.36.0.
//!
//!     cargo run -p latte-print --example print_test -- "path/to/company.books.db" \
//!         <schema> <name> <output folder> [<template name or .typ/.html file>]
//!
//! For example `SalesInvoice SINV-1001 out "Minimal - Sales Invoice"`. Without a template
//! the default one is used; a template Latte bundles is used even when the file doesn't
//! have it yet. A `.html` file is a Books template, converted to Typst first (the Typst
//! is written as `<name>.typ`). Writes `<name>.pdf`, `<name>-<page>.png`, and
//! `<name>.json` with the values the template read.

use std::path::Path;
use std::process::ExitCode;

use latte_core::Fyo;
use latte_core::fyo::format::Formatter;
use latte_db::Database;
use latte_print::templates::{self, PrintTemplate};

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
    let doc = fyo.get_doc(schema, Some(name))?;
    std::fs::create_dir_all(out)?;
    let bundled = |fyo: &Fyo, wanted: &str| {
        templates::bundled_templates(fyo)
            .into_iter()
            .find(|t| t.name == wanted)
    };
    let template = match template {
        Some(file) if file.ends_with(".html") || file.ends_with(".typ") => {
            let source = std::fs::read_to_string(file)?;
            let source = if file.ends_with(".html") {
                let conversion = latte_print::convert::html_to_typst(&source, file);
                for note in &conversion.notes {
                    eprintln!("note: {note}");
                }
                std::fs::write(out.join(format!("{name}.typ")), &conversion.source)?;
                conversion.source
            } else {
                source
            };
            let pos = file.contains("POS");
            PrintTemplate {
                name: file.clone(),
                kind: schema.to_owned(),
                template: source,
                width: if pos { 8.0 } else { 0.0 },
                height: if pos { 22.0 } else { 0.0 },
                is_custom: true,
            }
        }
        Some(wanted) => match bundled(&fyo, wanted) {
            Some(t) => t,
            None => templates::load(&mut fyo, wanted)?,
        },
        None => {
            let wanted = templates::default_template_name(&mut fyo, &doc)
                .unwrap_or_else(|| format!("Business - {}", doc.schema().label));
            match templates::load(&mut fyo, &wanted) {
                Ok(t) if !t.is_html() => t,
                _ => bundled(&fyo, &wanted).ok_or("no template")?,
            }
        }
    };
    let printout = latte_print::print(&mut fyo, &formatter, &doc, &template, &|s| s.to_owned())
        .map_err(|e| latte_print::errors_text(&e))?;

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
