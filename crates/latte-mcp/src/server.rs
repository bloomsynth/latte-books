//! The Model Context Protocol over standard input and output: JSON-RPC 2.0 messages, one
//! per line.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use latte_core::Fyo;
use latte_core::config;
use latte_db::{Database, DocValue};
use serde_json::{Value, json};

use crate::print::Printer;
use crate::tools;

/// The protocol versions this server speaks, newest first.
const VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// The open company, opened again when the company chosen for assistants changes.
struct Company {
    path: PathBuf,
    fyo: Fyo,
}

pub struct Server {
    /// The file given with `--file`, which stays the same.
    file: Option<PathBuf>,
    company: Option<Company>,
    printer: Printer,
}

impl Server {
    /// A server that prints with the app beside it.
    pub fn new(file: Option<PathBuf>) -> Self {
        Self::with_printer(file, Printer::App)
    }

    pub fn with_printer(file: Option<PathBuf>, printer: Printer) -> Self {
        Self {
            file,
            company: None,
            printer,
        }
    }

    fn company(&mut self) -> Result<&mut Company, String> {
        open(self.file.as_deref(), &mut self.company)
    }

    fn instructions(&mut self) -> String {
        let company = match self.company() {
            Ok(company) => {
                let name = match company
                    .fyo
                    .get_value("AccountingSettings", None, "companyName")
                {
                    Some(DocValue::Text(name)) => name,
                    _ => String::new(),
                };
                format!(
                    "You are working in the books of {name}, in Latte Books ({}).",
                    company.path.display()
                )
            }
            Err(e) => e,
        };
        format!(
            "{company} Latte Books is an accounting app (a port of Frappe Books). Look things \
             up with find_documents, get_document and run_report; describe_document_type \
             gives a type's fields. New documents are saved as drafts; show the person a \
             draft and ask before submitting it, since submitting posts it to the accounts. \
             print_document and print_report save PDFs; preview_print shows a printout's \
             pages, and print_template_guide tells how to write a print template."
        )
    }

    /// Handles one message, and returns the reply to send, if any.
    pub fn handle(&mut self, message: &Value) -> Option<Value> {
        let id = message.get("id").cloned();
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        // A notification gets no reply.
        let id = id?;
        let result = match method {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str);
                let version = asked
                    .filter(|v| VERSIONS.contains(v))
                    .unwrap_or(VERSIONS[0]);
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": {} },
                    "serverInfo": {
                        "name": "latte-books",
                        "title": "Latte Books",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                    "instructions": self.instructions(),
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools::list() })),
            "tools/call" => Ok(self.call(&params)),
            _ => Err(json!({ "code": -32601, "message": format!("Method not found: {method}") })),
        };
        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": error }),
        })
    }

    fn call(&mut self, params: &Value) -> Value {
        let name = params.get("name").and_then(Value::as_str).unwrap_or("");
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let outcome = open(self.file.as_deref(), &mut self.company)
            .and_then(|company| tools::call(&mut company.fyo, &self.printer, name, &args));
        let (content, is_error) = match outcome {
            Ok(content) => (content, false),
            Err(text) => (vec![json!({ "type": "text", "text": text })], true),
        };
        json!({ "content": content, "isError": is_error })
    }

    /// The company file in use, once one is open.
    pub fn path(&self) -> Option<&Path> {
        self.company.as_ref().map(|c| c.path.as_path())
    }
}

/// The company in `file`, or else the one chosen for assistants, opened again when that
/// changes.
fn open<'a>(
    file: Option<&Path>,
    company: &'a mut Option<Company>,
) -> Result<&'a mut Company, String> {
    let path = file
        .map(Path::to_path_buf)
        .or_else(config::assistant_file)
        .ok_or("No company is open in Latte Books yet. Open one in the app, then try again.")?;
    if company.as_ref().is_none_or(|c| c.path != path) {
        *company = None;
        let db = Database::open(&path, config::assistant_device_id())
            .map_err(|e| format!("Couldn't open {}: {e}", path.display()))?;
        *company = Some(Company {
            path,
            fyo: Fyo::with_models(db),
        });
    }
    Ok(company.as_mut().expect("opened above"))
}

/// Reads messages from `input` and writes replies to `output` until the input ends.
pub fn serve(
    file: Option<PathBuf>,
    input: impl BufRead,
    mut output: impl Write,
) -> std::io::Result<()> {
    let mut server = Server::new(file);
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(batch)) => {
                let replies: Vec<Value> = batch.iter().filter_map(|m| server.handle(m)).collect();
                (!replies.is_empty()).then_some(Value::Array(replies))
            }
            Ok(message) => server.handle(&message),
            Err(e) => Some(json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": { "code": -32700, "message": format!("Parse error: {e}") },
            })),
        };
        if let Some(reply) = reply {
            writeln!(output, "{reply}")?;
            output.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
