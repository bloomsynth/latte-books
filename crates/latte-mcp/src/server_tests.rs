use jiff::civil::date;
use latte_core::setup::{SetupWizardOptions, setup_instance};
use serde_json::{Value, json};

use super::Server;
use crate::print::Printer;

struct TempFile(std::path::PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn company() -> TempFile {
    let path = std::env::temp_dir().join(format!(
        "latte mcp {} {}.books.db",
        std::process::id(),
        latte_db::get_random_string()
    ));
    let mut options = SetupWizardOptions::for_country("India", date(2026, 10, 23));
    options.company_name = "Acme".into();
    options.bank_name = "Acme Bank".into();
    options.fullname = "Ada".into();
    options.email = "ada@example.com".into();
    setup_instance(&path, &options, "test").unwrap();
    TempFile(path)
}

/// Calls a tool and returns its text, and whether it failed.
fn call(server: &mut Server, name: &str, arguments: Value) -> (String, bool) {
    let reply = server
        .handle(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments },
        }))
        .unwrap();
    let result = &reply["result"];
    (
        result["content"][0]["text"].as_str().unwrap().to_owned(),
        result["isError"].as_bool().unwrap(),
    )
}

fn ok(server: &mut Server, name: &str, arguments: Value) -> Value {
    let (text, is_error) = call(server, name, arguments);
    assert!(!is_error, "{name}: {text}");
    serde_json::from_str(&text).unwrap_or(Value::String(text))
}

#[test]
fn an_assistant_invoices_a_customer() {
    let file = company();
    let mut server = Server::new(Some(file.0.clone()));

    let reply = server
        .handle(&json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": { "protocolVersion": "2025-03-26", "capabilities": {} },
        }))
        .unwrap();
    assert_eq!(reply["result"]["protocolVersion"], "2025-03-26");
    assert!(
        reply["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("the books of Acme")
    );
    assert!(
        server
            .handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .is_none()
    );
    let tools = server
        .handle(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }))
        .unwrap();
    assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 19);

    assert_eq!(
        ok(&mut server, "company_info", json!({}))["currency"],
        "INR"
    );
    ok(
        &mut server,
        "create_document",
        json!({
            "type": "Party",
            "values": { "name": "Globex", "role": "Customer" },
        }),
    );
    ok(
        &mut server,
        "create_document",
        json!({
            "type": "Item",
            "values": { "name": "Chair", "rate": 500 },
        }),
    );
    let invoice = ok(
        &mut server,
        "create_document",
        json!({
            "type": "SalesInvoice",
            "values": {
                "party": "Globex",
                "date": "2026-10-23",
                "items": [{ "item": "Chair", "quantity": 10 }],
            },
        }),
    );
    assert_eq!(invoice["grandTotal"], "5000.00");
    assert_eq!(invoice["date"], "2026-10-23T00:00:00.000Z");
    assert_eq!(invoice["items"][0]["amount"], "5000.00");
    let name = invoice["name"].as_str().unwrap().to_owned();

    // A draft posts nothing; submitting does.
    let found = ok(
        &mut server,
        "find_documents",
        json!({
            "type": "SalesInvoice",
            "filters": { "party": "Globex" },
            "fields": ["outstandingAmount", "submitted"],
        }),
    );
    assert_eq!(found[0]["submitted"], false);
    ok(
        &mut server,
        "submit_document",
        json!({ "type": "SalesInvoice", "name": name }),
    );
    let found = ok(
        &mut server,
        "find_documents",
        json!({
            "type": "SalesInvoice",
            "filters": { "outstandingAmount": [">", 0] },
            "fields": ["outstandingAmount"],
        }),
    );
    assert_eq!(found[0]["outstandingAmount"], "5000.00");
    let (ledger, _) = call(
        &mut server,
        "run_report",
        json!({
            "report": "general-ledger",
            "filters": { "referenceName": name },
        }),
    );
    assert!(ledger.contains("Debtors"), "{ledger}");

    // What a person couldn't do, an assistant can't either.
    let (text, is_error) = call(
        &mut server,
        "update_document",
        json!({
            "type": "SalesInvoice",
            "name": name,
            "values": { "terms": "Net 30" },
        }),
    );
    assert!(is_error);
    assert!(
        text.contains("only drafts and submitted journal entries can be changed"),
        "{text}"
    );
    // The edit log says it was submitted, and by whom.
    let history = ok(
        &mut server,
        "get_edit_history",
        json!({ "type": "SalesInvoice", "name": name }),
    );
    assert_eq!(history["createdBy"], "Admin");
    assert_eq!(history["changes"][0]["action"], "Submitted");
    assert_eq!(history["changes"][0]["by"], "Admin");
    let (text, is_error) = call(
        &mut server,
        "create_document",
        json!({
            "type": "SalesInvoice",
            "values": { "colour": "red" },
        }),
    );
    assert!(is_error);
    assert!(text.contains("no field \"colour\""), "{text}");
    let (text, is_error) = call(
        &mut server,
        "delete_document",
        json!({ "type": "SalesInvoice", "name": name }),
    );
    assert!(is_error, "{text}");

    ok(
        &mut server,
        "cancel_document",
        json!({ "type": "SalesInvoice", "name": name }),
    );
    let invoice = ok(
        &mut server,
        "get_document",
        json!({ "type": "SalesInvoice", "name": name }),
    );
    assert_eq!(invoice["cancelled"], true);
}

#[test]
fn an_assistant_has_the_gst_returns_only_with_a_gstin() {
    let file = company();
    let reports = |server: &mut Server| -> Vec<String> {
        ok(server, "list_reports", json!({}))
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["report"].as_str().unwrap().to_owned())
            .collect()
    };

    let mut server = Server::new(Some(file.0.clone()));
    assert!(!reports(&mut server).contains(&"gstr-1".to_owned()));
    let (text, is_error) = call(&mut server, "run_report", json!({ "report": "gstr-1" }));
    assert!(is_error);
    assert!(text.contains("no GSTIN"), "{text}");
    drop(server);

    let mut db = latte_db::Database::open(&file.0, "test").unwrap();
    let gstin = latte_db::DocValue::Text("29AABCA1234A1Z5".into());
    db.update(
        "AccountingSettings",
        &latte_db::DocValueMap::from([("gstin".to_owned(), gstin)]),
    )
    .unwrap();
    drop(db);

    let mut server = Server::new(Some(file.0.clone()));
    let listed = ok(&mut server, "list_reports", json!({}));
    let gstr_1 = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["report"] == "gstr-1")
        .unwrap();
    let filters: Vec<&str> = gstr_1["filters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        filters,
        [
            "fiscalYear",
            "period",
            "transferType",
            "place",
            "fromDate",
            "toDate"
        ]
    );
    // A period, or dates.
    ok(
        &mut server,
        "run_report",
        json!({ "report": "gstr-1", "filters": { "fiscalYear": "2026-27", "period": "Q2" } }),
    );
    ok(
        &mut server,
        "run_report",
        json!({ "report": "gstr-1", "filters": { "fromDate": "2026-07-01", "toDate": "2026-09-30" } }),
    );
}

#[test]
fn a_payment_names_a_purchase_invoice_whatever_order_its_row_is_given_in() {
    let file = company();
    let mut server = Server::new(Some(file.0.clone()));
    ok(
        &mut server,
        "create_document",
        json!({ "type": "Party", "values": { "name": "Initech", "role": "Supplier" } }),
    );
    ok(
        &mut server,
        "create_document",
        json!({ "type": "Item", "values": { "name": "Paper", "rate": 100 } }),
    );
    let invoice = ok(
        &mut server,
        "create_document",
        json!({
            "type": "PurchaseInvoice",
            "values": {
                "party": "Initech",
                "date": "2026-10-23",
                "items": [{ "item": "Paper", "quantity": 5 }],
            },
        }),
    );
    let name = invoice["name"].as_str().unwrap().to_owned();
    ok(
        &mut server,
        "submit_document",
        json!({ "type": "PurchaseInvoice", "name": name }),
    );

    // The invoice's name comes before its type, as a caller may well give them.
    let payment = ok(
        &mut server,
        "create_document",
        json!({
            "type": "Payment",
            "values": {
                "paymentType": "Pay",
                "for": [{
                    "referenceName": name,
                    "referenceType": "PurchaseInvoice",
                }],
                "party": "Initech",
            },
        }),
    );
    assert_eq!(payment["for"][0]["referenceType"], "PurchaseInvoice");
    assert_eq!(payment["for"][0]["referenceName"], name);
}

/// A tool's whole reply content, and whether it failed.
fn call_content(server: &mut Server, name: &str, arguments: Value) -> (Vec<Value>, bool) {
    let reply = server
        .handle(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments },
        }))
        .unwrap();
    let result = &reply["result"];
    (
        result["content"].as_array().unwrap().clone(),
        result["isError"].as_bool().unwrap(),
    )
}

struct TempDir(std::path::PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn an_assistant_prints_an_invoice_and_a_report() {
    let file = company();
    let out = TempDir(std::env::temp_dir().join(format!(
        "latte mcp pdfs {} {}",
        std::process::id(),
        latte_db::get_random_string()
    )));
    // The app lays the pages out for an assistant; here the test does, with the same code.
    let mut server = Server::with_printer(
        Some(file.0.clone()),
        Printer::InProcess(latte_print::job::run),
    );
    ok(
        &mut server,
        "create_document",
        json!({ "type": "Party", "values": { "name": "Globex", "role": "Customer" } }),
    );
    ok(
        &mut server,
        "create_document",
        json!({ "type": "Item", "values": { "name": "Chair", "rate": 500 } }),
    );
    let invoice = ok(
        &mut server,
        "create_document",
        json!({
            "type": "SalesInvoice",
            "values": {
                "party": "Globex",
                "date": "2026-10-23",
                "items": [{ "item": "Chair", "quantity": 10 }],
            },
        }),
    );
    let name = invoice["name"].as_str().unwrap().to_owned();

    // The file was never opened in the app, and has Latte's templates all the same.
    let listed = ok(
        &mut server,
        "list_print_templates",
        json!({ "type": "SalesInvoice", "name": name }),
    );
    let templates = listed["templates"].as_array().unwrap();
    assert!(templates.len() >= 3, "{listed}");
    assert_eq!(
        templates.iter().filter(|t| t["default"] == true).count(),
        1,
        "{listed}"
    );
    let by_type = ok(&mut server, "list_print_templates", json!({}));
    assert_eq!(
        by_type["Payment"]["defaultSetting"],
        "Defaults.paymentPrintTemplate"
    );

    // Saved as a PDF; a second save doesn't replace the first.
    let saved = ok(
        &mut server,
        "print_document",
        json!({ "type": "SalesInvoice", "name": name, "path": out.0.display().to_string() }),
    );
    let first = std::path::PathBuf::from(saved["saved"].as_str().unwrap());
    assert_eq!(first, out.0.join(format!("{name}.pdf")));
    assert!(std::fs::read(&first).unwrap().starts_with(b"%PDF"));
    assert_eq!(saved["pages"], 1);
    let again = ok(
        &mut server,
        "print_document",
        json!({
            "type": "SalesInvoice",
            "name": name,
            "template": "Minimal - Sales Invoice",
            "path": out.0.join("copy.pdf").display().to_string(),
        }),
    );
    assert_eq!(again["template"], "Minimal - Sales Invoice");
    let again = ok(
        &mut server,
        "print_document",
        json!({
            "type": "SalesInvoice",
            "name": name,
            "path": out.0.join("copy.pdf").display().to_string(),
        }),
    );
    assert_eq!(
        std::path::PathBuf::from(again["saved"].as_str().unwrap()),
        out.0.join("copy (2).pdf")
    );
    let (text, is_error) = call(
        &mut server,
        "print_document",
        json!({ "type": "SalesInvoice", "name": name, "template": "Fancy" }),
    );
    assert!(is_error);
    assert!(text.contains("Basic - Sales Invoice"), "{text}");

    // A preview is the pages as images.
    let (content, is_error) = call_content(
        &mut server,
        "preview_print",
        json!({ "type": "SalesInvoice", "name": name }),
    );
    assert!(!is_error, "{content:?}");
    assert_eq!(content.len(), 2, "{content:?}");
    assert_eq!(content[1]["type"], "image");
    assert_eq!(content[1]["mimeType"], "image/png");
    assert!(
        content[1]["data"]
            .as_str()
            .unwrap()
            .starts_with("iVBORw0KGgo")
    );

    // A template is written from the guide, tried out, then saved and printed with.
    let guide = ok(
        &mut server,
        "print_template_guide",
        json!({ "type": "SalesInvoice", "name": name }),
    );
    assert!(guide["values"]["doc"]["items"].is_array(), "{guide}");
    assert_eq!(guide["documentValues"]["doc"]["name"], name);
    let source = format!(
        "{}\n#doc.links.party.name\n",
        guide["starter"].as_str().unwrap()
    );
    let (text, is_error) = call(
        &mut server,
        "preview_print",
        json!({
            "type": "SalesInvoice",
            "name": name,
            "template_source": "#import \"/latte.typ\": *\n#show: setup\n#doc.nothing.here\n",
        }),
    );
    assert!(is_error);
    assert!(text.contains("Line 3"), "{text}");
    let (content, is_error) = call_content(
        &mut server,
        "preview_print",
        json!({ "type": "SalesInvoice", "name": name, "template_source": source, "page_size": "A5" }),
    );
    assert!(!is_error, "{content:?}");
    assert!(content[0]["text"].as_str().unwrap().contains("A5"));
    ok(
        &mut server,
        "create_document",
        json!({
            "type": "PrintTemplate",
            "values": { "name": "Plain", "type": "SalesInvoice", "template": source },
        }),
    );
    let saved = ok(
        &mut server,
        "print_document",
        json!({ "type": "SalesInvoice", "name": name, "template": "Plain", "path": out.0.display().to_string(), "overwrite": true }),
    );
    assert_eq!(saved["saved"], first.display().to_string());

    // A report, its columns picked, on a landscape page.
    ok(
        &mut server,
        "submit_document",
        json!({ "type": "SalesInvoice", "name": name }),
    );
    let report = ok(
        &mut server,
        "print_report",
        json!({
            "report": "general-ledger",
            "columns": ["Account", "debit"],
            "landscape": true,
            "path": out.0.join("ledger.pdf").display().to_string(),
        }),
    );
    assert!(report["rows"].as_u64().unwrap() > 0, "{report}");
    assert!(
        std::fs::read(out.0.join("ledger.pdf"))
            .unwrap()
            .starts_with(b"%PDF")
    );
    let (text, is_error) = call(
        &mut server,
        "print_report",
        json!({ "report": "general-ledger", "columns": ["Colour"] }),
    );
    assert!(is_error);
    assert!(text.contains("no column \"Colour\""), "{text}");
}
