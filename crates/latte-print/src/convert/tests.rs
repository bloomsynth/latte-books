use super::html_to_typst;
use crate::render;
use crate::tests::values;

/// Books' bundled HTML templates (vendor/books/templates) and the document types they
/// print.
pub(crate) const BOOKS: [(&str, &str, &str, f64); 6] = [
    (
        "Basic",
        include_str!("../../../../vendor/books/templates/Basic.template.html"),
        "SalesInvoice",
        21.0,
    ),
    (
        "Business",
        include_str!("../../../../vendor/books/templates/Business.template.html"),
        "SalesInvoice",
        21.0,
    ),
    (
        "Minimal",
        include_str!("../../../../vendor/books/templates/Minimal.template.html"),
        "PurchaseInvoice",
        21.0,
    ),
    (
        "Business.Payment",
        include_str!("../../../../vendor/books/templates/Business.Payment.template.html"),
        "Payment",
        21.0,
    ),
    (
        "Business.Shipment",
        include_str!("../../../../vendor/books/templates/Business.Shipment.template.html"),
        "Shipment",
        21.0,
    ),
    (
        "Business-POS",
        include_str!("../../../../vendor/books/templates/Business-POS.template.html"),
        "SalesInvoice",
        8.0,
    ),
];

fn t(s: &str) -> String {
    s.to_owned()
}

#[test]
fn books_templates_convert_and_print() {
    for (name, html, entry_type, width) in BOOKS {
        let conversion = html_to_typst(html, name);
        assert!(
            conversion.notes.is_empty(),
            "{name}: {:?}",
            conversion.notes
        );
        let height = if width < 10.0 { 22.0 } else { 29.7 };
        let printout = render(&conversion.source, &values(entry_type), (width, height), &t)
            .unwrap_or_else(|e| {
                panic!(
                    "{name}: {}\n{}",
                    crate::errors_text(&e),
                    numbered(&conversion.source)
                )
            });
        assert!(printout.page_count() >= 1, "{name}");
    }
}

#[test]
fn books_templates_print_without_optional_values() {
    for (name, html, entry_type, width) in BOOKS {
        let conversion = html_to_typst(html, name);
        let mut v = values(entry_type);
        v["doc"]["taxes"] = serde_json::json!("");
        v["doc"]["items"] = serde_json::json!("");
        v["doc"]["terms"] = serde_json::json!("");
        v["doc"]["links"] = serde_json::Value::Null;
        v["print"]["gstin"] = serde_json::Value::Null;
        render(&conversion.source, &v, (width, 29.7), &t)
            .unwrap_or_else(|e| panic!("{name}: {}", crate::errors_text(&e)));
    }
}

/// Writes each Books template, converted, and Latte's port of it as PNGs to
/// `$LATTE_PRINT_OUT`, to compare by eye:
/// `LATTE_PRINT_OUT=out cargo test -p latte-print -- --ignored write_comparisons`.
#[test]
#[ignore]
fn write_comparisons() {
    let Some(out) = std::env::var_os("LATTE_PRINT_OUT") else {
        return;
    };
    let out = std::path::PathBuf::from(out);
    std::fs::create_dir_all(&out).unwrap();
    for ((name, html, entry_type, width), (_, port, _, _)) in
        BOOKS.iter().zip(crate::tests::BUNDLED)
    {
        let height = if *width < 10.0 { 22.0 } else { 29.7 };
        let conversion = html_to_typst(html, name);
        std::fs::write(out.join(format!("{name}.typ")), &conversion.source).unwrap();
        for (kind, source) in [("html", conversion.source.as_str()), ("port", port)] {
            let printout = render(source, &values(entry_type), (*width, height), &t).unwrap();
            let png = printout.png_pages(1.5).unwrap().remove(0);
            std::fs::write(out.join(format!("{name}-{kind}.png")), png).unwrap();
        }
    }
}

fn numbered(source: &str) -> String {
    source
        .lines()
        .enumerate()
        .map(|(i, l)| format!("{:4} {l}", i + 1))
        .collect::<Vec<_>>()
        .join("\n")
}

fn convert(html: &str) -> String {
    html_to_typst(html, "Test").source
}

#[test]
fn interpolation_and_text() {
    let source = convert("<p class=\"text-sm\">GSTIN: {{ print.gstin }}</p>");
    assert!(source.contains("set text(size: text-size.sm)"), "{source}");
    assert!(source.contains("\"GSTIN: \""), "{source}");
    assert!(source.contains("val(get(print, \"gstin\"))"), "{source}");
}

#[test]
fn v_if_chains() {
    let source = convert(
        "<div><img v-if=\"print.displayLogo\" :src=\"print.logo\" /><p v-else-if=\"doc.x\">x</p><p v-else>{{ print.companyName }}</p></div>",
    );
    assert!(
        source.contains("if has(get(print, \"displayLogo\")) {"),
        "{source}"
    );
    assert!(
        source.contains("} else if has(get(doc, \"x\")) {"),
        "{source}"
    );
    assert!(source.contains("} else {"), "{source}");
    assert!(source.contains("css-img(get(print, \"logo\"))"), "{source}");
}

#[test]
fn v_for_with_an_index() {
    let source =
        convert("<div v-for=\"(row, index) in doc.items\">{{ index + 1 }} {{ row.item }}</div>");
    assert!(
        source.contains("for (row, index) in js-each(get(doc, \"items\")) {"),
        "{source}"
    );
    assert!(source.contains("js-add(index, 1)"), "{source}");
    let out = render(&source, &values("SalesInvoice"), (21.0, 29.7), &t);
    assert!(out.is_ok(), "{source}");
}

#[test]
fn flex_rows_and_grids() {
    let source = convert(
        "<section class=\"flex justify-between gap-2 items-center\"><div class=\"w-1/3\">a</div><div class=\"flex-1 text-right\">b</div></section>\
         <div class=\"grid grid-cols-3 gap-3\"><div class=\"col-span-2\">x</div><div>y</div></div>",
    );
    assert!(source.contains("css-flex(items-"), "{source}");
    assert!(source.contains("justify: \"between\""), "{source}");
    assert!(source.contains("width: 33.3333%"), "{source}");
    assert!(source.contains("grow: 1"), "{source}");
    assert!(source.contains("css-grid(cells-"), "{source}");
    assert!(source.contains("col-span: 2"), "{source}");
    let out = render(&source, &values("SalesInvoice"), (21.0, 29.7), &t);
    assert!(out.is_ok(), "{source}");
}

#[test]
fn dynamic_classes_and_styles() {
    let source = convert(
        "<p :class=\"{ 'font-bold': doc.submitted, hidden: !doc.name }\" :style=\"{ color: print.color, fontSize: '20px' }\">x</p>",
    );
    assert!(
        source.contains("set text(weight: 700) if has(get(doc, \"submitted\"))"),
        "{source}"
    );
    assert!(
        source.contains("if not (has(not has(get(doc, \"name\"))))"),
        "{source}"
    );
    assert!(
        source.contains("set text(fill: css-color(get(print, \"color\")))"),
        "{source}"
    );
    assert!(source.contains("css-length(\"20px\")"), "{source}");
    let out = render(&source, &values("SalesInvoice"), (21.0, 29.7), &t);
    assert!(out.is_ok(), "{source}");
}

#[test]
fn tables() {
    let source = convert(
        "<table class=\"w-full\"><thead><tr><th>Item</th><th class=\"text-right\">Amount</th></tr></thead>\
         <tbody><tr v-for=\"row in doc.items\"><td>{{ row.item }}</td><td class=\"text-right\">{{ row.amount }}</td></tr></tbody></table>",
    );
    assert!(source.contains("css-table(rows-"), "{source}");
    let out = render(&source, &values("SalesInvoice"), (21.0, 29.7), &t);
    assert!(out.is_ok(), "{source}");
}

#[test]
fn what_cant_be_converted_is_noted_and_left_out() {
    let conversion = html_to_typst(
        "<main><blink class=\"wobble\">{{ fyo.format(doc.x) }}</blink><img src=\"https://example.com/a.png\"></main>",
        "Test",
    );
    assert_eq!(conversion.notes.len(), 5, "{:?}", conversion.notes);
    assert!(
        conversion
            .source
            .contains("// TODO(convert): the tag <blink>")
    );
    assert!(
        conversion
            .source
            .contains("// TODO(convert): class `wobble`")
    );
    let out = render(
        &conversion.source,
        &values("SalesInvoice"),
        (21.0, 29.7),
        &t,
    );
    assert!(out.is_ok(), "{}", conversion.source);
}

#[test]
fn pre_line_keeps_line_breaks() {
    let source = convert("<p class=\"whitespace-pre-line\">\n  {{ doc.terms }}\n</p>");
    assert!(
        source.contains("lines(val(get(doc, \"terms\")))"),
        "{source}"
    );
    assert!(source.contains("linebreak()"), "{source}");
}
