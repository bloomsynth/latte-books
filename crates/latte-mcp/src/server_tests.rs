use jiff::civil::date;
use latte_core::setup::{SetupWizardOptions, setup_instance};
use serde_json::{Value, json};

use super::Server;

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
    assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 12);

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
    assert!(text.contains("only drafts can be changed"), "{text}");
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
