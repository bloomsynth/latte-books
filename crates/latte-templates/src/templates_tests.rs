use jiff::civil::date;
use latte_core::Fyo;
use latte_core::setup::{SetupWizardOptions, setup_instance};
use latte_db::{DocValue, DocValueMap};

use crate::templates::{
    PrintTemplate, bundled_templates, can_print, default_template_name, load, new_template,
    size_name, template_names, update_print_templates,
};

/// A new company file in the temp folder, removed when dropped.
struct Company {
    path: std::path::PathBuf,
    fyo: Fyo,
}

impl Drop for Company {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn company() -> Company {
    let path = std::env::temp_dir().join(format!(
        "latte print {} {}.books.db",
        std::process::id(),
        latte_db::get_random_string()
    ));
    let mut options = SetupWizardOptions::for_country("India", date(2026, 10, 23));
    options.company_name = "Acme".into();
    options.bank_name = "Acme Bank".into();
    options.fullname = "Ada".into();
    options.email = "ada@example.com".into();
    let fyo = setup_instance(&path, &options, "test").unwrap();
    Company { path, fyo }
}

fn modified(fyo: &mut Fyo, name: &str) -> String {
    match fyo
        .get_doc("PrintTemplate", Some(name))
        .unwrap()
        .get("modified")
    {
        Some(DocValue::Date(ts)) => ts.to_string(),
        other => format!("{other:?}"),
    }
}

#[test]
fn a_company_gets_the_bundled_templates_as_books_names_them() {
    let mut c = company();
    update_print_templates(&mut c.fyo).unwrap();
    let names: Vec<String> = bundled_templates(&c.fyo)
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(names.len(), 14);
    for name in [
        "Basic - Sales Invoice",
        "Business - Quote",
        "Minimal - Purchase Invoice",
        "Business - Payment",
        "Business - Shipment",
        "Business-POS - Sales Invoice",
    ] {
        assert!(names.contains(&name.to_owned()), "{name}");
        let template = load(&mut c.fyo, name).unwrap();
        assert!(!template.is_custom);
        assert!(!template.is_html());
        assert_eq!(modified(&mut c.fyo, name), "2000-01-01T00:00:00Z");
    }
    let pos = load(&mut c.fyo, "Business-POS - Sales Invoice").unwrap();
    assert_eq!((pos.width, pos.height), (8.0, 22.0));
    assert_eq!(size_name(pos.width, pos.height), "POS");
    assert_eq!(
        load(&mut c.fyo, "Basic - Sales Invoice").unwrap().size(),
        (21.0, 29.7)
    );

    assert!(can_print(&c.fyo, "SalesInvoice"));
    assert!(can_print(&c.fyo, "Shipment"));
    assert!(!can_print(&c.fyo, "JournalEntry"));
    assert_eq!(
        template_names(&c.fyo, "Payment"),
        vec!["Business - Payment"]
    );
    assert_eq!(template_names(&c.fyo, "SalesInvoice").len(), 4);
}

#[test]
fn books_html_bundled_rows_are_replaced_and_custom_ones_kept() {
    let mut c = company();
    let fyo = &mut c.fyo;
    // As Books leaves its templates in a file, and a custom one of the user's.
    for (name, custom) in [("Business - Sales Invoice", false), ("Mine", true)] {
        let mut values = DocValueMap::new();
        values.insert("name".into(), DocValue::Text(name.into()));
        values.insert("type".into(), DocValue::Text("SalesInvoice".into()));
        values.insert(
            "template".into(),
            DocValue::Text("<main>{{ doc.name }}</main>".into()),
        );
        values.insert("isCustom".into(), DocValue::Bool(custom));
        let mut doc = fyo.get_new_doc("PrintTemplate", values).unwrap();
        doc.sync(fyo).unwrap();
    }
    update_print_templates(fyo).unwrap();
    assert!(!load(fyo, "Business - Sales Invoice").unwrap().is_html());
    let mine = load(fyo, "Mine").unwrap();
    assert!(mine.is_html() && mine.is_custom);

    // Opening the file again changes nothing.
    let before = modified(fyo, "Business - Sales Invoice");
    update_print_templates(fyo).unwrap();
    assert_eq!(modified(fyo, "Business - Sales Invoice"), before);

    // A bundled template can't be deleted, a custom one can.
    assert!(
        !fyo.get_doc("PrintTemplate", Some("Basic - Sales Invoice"))
            .unwrap()
            .can_delete()
    );
    assert!(
        fyo.get_doc("PrintTemplate", Some("Mine"))
            .unwrap()
            .can_delete()
    );
}

#[test]
fn the_default_template_is_the_one_defaults_names() {
    let mut c = company();
    let fyo = &mut c.fyo;
    update_print_templates(fyo).unwrap();
    let mut values = DocValueMap::new();
    values.insert("party".into(), DocValue::Text("x".into()));
    let invoice = fyo.get_new_doc("SalesInvoice", values).unwrap();
    // A new company's Defaults name no template (`checkSingleLinks` drops them before
    // the templates are made), so the newest of the schema's templates is used.
    let first = template_names(fyo, "SalesInvoice").remove(0);
    assert_eq!(default_template_name(fyo, &invoice), Some(first));

    let mut defaults = fyo.get_doc("Defaults", None).unwrap();
    for (field, name) in [
        ("salesInvoicePrintTemplate", "Business - Sales Invoice"),
        ("posPrintTemplate", "Business-POS - Sales Invoice"),
        ("salesQuotePrintTemplate", "No Such Template"),
    ] {
        defaults
            .set(fyo, field, DocValue::Text(name.into()))
            .unwrap();
    }
    defaults.sync(fyo).unwrap();
    assert_eq!(
        default_template_name(fyo, &invoice).as_deref(),
        Some("Business - Sales Invoice")
    );
    let mut pos = invoice.clone();
    pos.set_value("isPOS", DocValue::Bool(true));
    assert_eq!(
        default_template_name(fyo, &pos).as_deref(),
        Some("Business-POS - Sales Invoice")
    );
    // One that isn't there: the first is used, without Books' error dialog.
    let quote = fyo.get_new_doc("SalesQuote", DocValueMap::new()).unwrap();
    let name = default_template_name(fyo, &quote).unwrap();
    assert!(name.ends_with("- Quote"), "{name}");
}

#[test]
fn new_and_duplicated_templates_are_custom() {
    let mut c = company();
    let fyo = &mut c.fyo;
    update_print_templates(fyo).unwrap();
    let new = new_template(fyo, "Shipment", None).unwrap();
    let new = PrintTemplate::from_doc(&new);
    assert!(new.is_custom);
    assert_eq!(new.kind, "Shipment");
    assert_eq!(new.template, crate::templates::BASE_TEMPLATE);

    let business = fyo
        .get_doc("PrintTemplate", Some("Business - Payment"))
        .unwrap();
    let copy = business.duplicate(fyo).unwrap();
    assert_eq!(copy.get("isCustom"), Some(&DocValue::Bool(true)));
    let ctx = |doc| latte_core::fyo::model::types::DocContext {
        doc,
        parent: None,
        fyo,
    };
    assert_eq!(copy.model().read_only("template", &ctx(&copy)), Some(false));
    assert_eq!(
        business.model().read_only("template", &ctx(&business)),
        Some(true)
    );
}
