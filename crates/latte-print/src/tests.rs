use serde_json::{Value, json};

use crate::{PrintError, render, translated_strings};

/// Values like those `print_values` gives, for a document of `entry_type`.
pub(crate) fn values(entry_type: &str) -> Value {
    let item = |name: &str, hsn: &str| {
        json!({
            "item": name, "description": "", "hsnCode": hsn, "quantity": "2",
            "rate": "₹ 150.00", "amount": "₹ 300.00", "location": "Stores",
            "name": "row", "entryType": format!("{entry_type}Item"),
        })
    };
    json!({
        "doc": {
            "name": "SINV-1001", "date": "Oct 23, 2026", "party": "Acme Traders",
            "entryType": entry_type, "entryLabel": "Sales Invoice", "submitted": true,
            "items": [item("Widget", "8471"), item("Gadget", "")],
            "taxes": [{ "account": "CGST", "amount": "₹ 27.00", "name": "tax" }],
            "netTotal": "₹ 600.00", "grandTotal": "₹ 654.00", "totalDiscount": "",
            "discountAfterTax": "", "showHSN": false, "terms": "Pay within\n30 days",
            "grandTotalInWords": "Six Hundred And Fifty Four only",
            "paymentType": "Receive", "paymentMethod": "Cash", "amount": "₹ 654.00",
            "amountPaid": "₹ 654.00", "amountPaidInWords": "Six Hundred And Fifty Four only",
            "subTotal": "₹ 600.00", "isPOS": "True",
            "paymentDetails": [{ "amount": "₹ 654.00", "amountPaid": "₹ 654.00",
                "paymentMethod": "Cash", "outstandingAmount": "₹ 0.00" }],
            "links": { "party": { "address": "A1", "gstin": "33AAAAA0000A1Z5",
                "links": { "address": { "addressDisplay": "1 Main Road\nChennai" } } } },
        },
        "print": {
            "companyName": "Latte Test Co", "email": "a@b.co", "phone": "98765", "address": "",
            "gstin": "", "color": "#112B42", "font": "Arial", "displayLogo": "", "logo": "",
            "amountInWords": "True", "displaytermsandconditions": "True",
            "termsAndConditions": "Goods once sold\nare not returned",
        },
    })
}

/// Bundled templates and the document types they print, as Books names their files.
pub(crate) const BUNDLED: [(&str, &str, &str, f64); 6] = [
    (
        "Basic",
        include_str!("templates/basic.typ"),
        "SalesInvoice",
        21.0,
    ),
    (
        "Business",
        include_str!("templates/business.typ"),
        "SalesInvoice",
        21.0,
    ),
    (
        "Minimal",
        include_str!("templates/minimal.typ"),
        "PurchaseInvoice",
        21.0,
    ),
    (
        "Business.Payment",
        include_str!("templates/payment.typ"),
        "Payment",
        21.0,
    ),
    (
        "Business.Shipment",
        include_str!("templates/shipment.typ"),
        "Shipment",
        21.0,
    ),
    (
        "Business-POS",
        include_str!("templates/pos.typ"),
        "SalesInvoice",
        8.0,
    ),
];

fn t(s: &str) -> String {
    s.to_owned()
}

#[test]
fn every_bundled_template_prints_its_document() {
    for (name, source, entry_type, width) in BUNDLED {
        let height = if width < 10.0 { 22.0 } else { 29.7 };
        let printout = render(source, &values(entry_type), (width, height), &t)
            .unwrap_or_else(|e| panic!("{name}: {}", crate::errors_text(&e)));
        assert_eq!(printout.page_count(), 1, "{name}");
        let (w, h) = printout.page_sizes()[0];
        assert!((w - width / 2.54 * 72.0).abs() < 0.1, "{name}: {w}");
        assert!((h - height / 2.54 * 72.0).abs() < 0.1, "{name}: {h}");
        assert!(printout.pdf().unwrap().starts_with(b"%PDF"));
    }
}

#[test]
fn the_new_template_prints() {
    let printout = render(
        crate::templates::BASE_TEMPLATE,
        &values("SalesInvoice"),
        (21.0, 29.7),
        &t,
    );
    assert!(printout.is_ok());
}

/// A payment that isn't for a Sales Invoice gets no tax rows, which Books leaves out
/// and its Payment template then fails on.
#[test]
fn a_payment_without_taxes_prints() {
    let mut values = values("Payment");
    values["doc"]["taxes"] = json!("");
    let source = include_str!("templates/payment.typ");
    assert!(render(source, &values, (21.0, 29.7), &t).is_ok());
}

/// Outside India the company has no GSTIN field, which the values give as null.
#[test]
fn a_company_without_gstin_prints() {
    let mut values = values("SalesInvoice");
    values["print"]["gstin"] = Value::Null;
    for (name, source, _, _) in BUNDLED {
        render(source, &values, (21.0, 29.7), &t)
            .unwrap_or_else(|e| panic!("{name}: {}", crate::errors_text(&e)));
    }
}

#[test]
fn errors_say_where_in_the_template() {
    let source = "#import \"/latte.typ\": *\n#show: setup\n\nHello #doc.nam\n";
    let errors = match render(source, &values("SalesInvoice"), (21.0, 29.7), &t) {
        Err(errors) => errors,
        Ok(_) => panic!("a missing key is an error"),
    };
    assert_eq!(errors[0].line, Some(4));
    assert_eq!(errors[0].column, Some(12));
    assert!(errors[0].message.contains("nam"), "{}", errors[0].message);
    let text = errors[0].to_string();
    assert!(text.starts_with("Line 4, column 12: "), "{text}");

    let errors = render("#let x = (", &values("SalesInvoice"), (21.0, 29.7), &t)
        .err()
        .unwrap();
    assert_eq!(errors[0].line, Some(1));
}

#[test]
fn html_templates_are_refused_with_a_message() {
    let errors = render(
        "<main>{{ doc.name }}</main>",
        &values("SalesInvoice"),
        (21.0, 29.7),
        &t,
    )
    .err()
    .unwrap();
    assert_eq!(
        errors,
        vec![PrintError::new(
            "This is a Frappe Books HTML template, which Latte can't print. \
             Convert it to Typst from the template menu."
        )]
    );
}

#[test]
fn the_strings_a_template_translates() {
    let source = r#"#t("Item") #t("Say \"hi\"") #text(t("Rate")) #format("x") #tf("Pay ${0}", 1)"#;
    assert_eq!(
        translated_strings(source),
        vec!["Item", "Pay ${0}", "Rate", "Say \"hi\""]
    );
    let printout = render(
        "#import \"/latte.typ\": *\n#t(\"Item\")",
        &values("SalesInvoice"),
        (21.0, 29.7),
        &|s| format!("[{s}]"),
    );
    assert!(printout.is_ok());
}

#[test]
fn the_logo_is_the_documents_image() {
    // A 1×1 PNG.
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
    let mut values = values("SalesInvoice");
    values["print"]["logo"] = json!(png);
    values["print"]["displayLogo"] = json!("True");
    for (name, source, _, _) in BUNDLED {
        render(source, &values, (21.0, 29.7), &t)
            .unwrap_or_else(|e| panic!("{name}: {}", crate::errors_text(&e)));
    }
}

#[test]
fn only_bundled_fonts_are_used() {
    let families = crate::font_families();
    for family in [
        "Liberation Sans",
        "Liberation Serif",
        "Liberation Mono",
        "Inter",
    ] {
        assert!(
            families.iter().any(|f| f == family),
            "{family}: {families:?}"
        );
    }
    assert!(families.len() < 12, "{families:?}");
}

#[test]
fn a_report_prints() {
    use crate::report::{ReportColumn, ReportPrint, print_report};
    let report = ReportPrint {
        company_name: "Latte Test Co".to_owned(),
        title: "General Ledger".to_owned(),
        columns: vec![
            ReportColumn {
                label: "Account".to_owned(),
                align: "left",
            },
            ReportColumn {
                label: "Debit".to_owned(),
                align: "right",
            },
        ],
        rows: (0..80)
            .map(|i| vec![format!("Cash {i}"), "₹ 10.00".to_owned()])
            .collect(),
        printed: "Oct 25, 2026 10:00".to_owned(),
        width: 29.7,
        height: 21.0,
    };
    let printout = print_report(&report).unwrap();
    assert!(printout.page_count() > 1);
    assert!(printout.page_sizes()[0].0 > printout.page_sizes()[0].1);
}
