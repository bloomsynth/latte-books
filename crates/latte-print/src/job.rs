//! A layout job sent by another process: `latte-mcp` gathers a document's values and
//! picks its template without Typst, and runs the app as `latte-books --print-job` to
//! lay it out, so Typst and its fonts are in the app alone.
//!
//! The job is one JSON object on standard input, and the reply one JSON object on
//! standard output. Nothing is opened or saved: the PDF or page images come back in the
//! reply.
//!
//! A job is `{ "job": 1, … }` with one of
//! - `"document": { "template", "values", "width", "height", "translations" }`: a
//!   template's Typst source, its `{ doc, print }` values, the page in centimetres, and
//!   the template's `t("…")` strings in the app's language;
//! - `"report": …`: a report's [`ReportPrint::data`](crate::report::ReportPrint::data);
//!
//! and `"output"` either `"pdf"` or `"png"` (with `"scale"`, pixels to a point, and
//! `"pages"`, how many from the first). The reply is `{ "pages", "pdf" }` or
//! `{ "pages", "png": [...] }`, the files in base64, or `{ "error" }`.

use std::io::{Read, Write};

use base64::Engine as _;
use serde_json::{Value, json};

use crate::{Printout, errors_text};

pub use latte_templates::job::{FLAG, VERSION};

fn number(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64).filter(|n| *n > 0.0)
}

/// Lays out a job, and returns the reply, or what went wrong.
pub fn run(job: &Value) -> Result<Value, String> {
    if job.get("job").and_then(Value::as_u64) != Some(VERSION) {
        return Err(format!(
            "This app lays out print jobs of version {VERSION}; update Latte Books so that the app and latte-mcp are the same version"
        ));
    }
    let printout: Printout = if let Some(document) = job.get("document") {
        let template = document
            .get("template")
            .and_then(Value::as_str)
            .ok_or("The job has no template")?;
        let values = document.get("values").cloned().unwrap_or(Value::Null);
        let size = (
            number(document, "width").unwrap_or(21.0),
            number(document, "height").unwrap_or(29.7),
        );
        let translations = document.get("translations");
        let t = |text: &str| {
            translations
                .and_then(|t| t.get(text))
                .and_then(Value::as_str)
                .unwrap_or(text)
                .to_owned()
        };
        crate::render(template, &values, size, &t).map_err(|e| errors_text(&e))?
    } else if let Some(report) = job.get("report") {
        crate::report::print_report_data(report).map_err(|e| errors_text(&e))?
    } else {
        return Err("The job has no document or report".to_owned());
    };
    let base64 = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
    match job.get("output").and_then(Value::as_str) {
        Some("png") => {
            let scale = number(job, "scale").unwrap_or(1.5);
            let pages = job
                .get("pages")
                .and_then(Value::as_u64)
                .map_or(usize::MAX, |n| n as usize);
            let images = printout.png_pages_up_to(scale, pages)?;
            Ok(json!({
                "pages": printout.page_count(),
                "png": images.iter().map(|png| base64(png)).collect::<Vec<_>>(),
            }))
        }
        Some("pdf") | None => Ok(json!({
            "pages": printout.page_count(),
            "pdf": base64(&printout.pdf()?),
        })),
        Some(other) => Err(format!("A job's output is pdf or png, not {other}")),
    }
}

/// Reads a job from `input` and writes its reply to `output`. The exit code: 0 once a
/// reply is written, whether or not the job laid out.
pub fn serve(mut input: impl Read, mut output: impl Write) -> i32 {
    let mut text = String::new();
    let reply = match input.read_to_string(&mut text) {
        Ok(_) => match serde_json::from_str::<Value>(&text) {
            Ok(job) => run(&job).unwrap_or_else(|e| json!({ "error": e })),
            Err(e) => json!({ "error": format!("The job isn't JSON: {e}") }),
        },
        Err(e) => json!({ "error": format!("Couldn't read the job: {e}") }),
    };
    match writeln!(output, "{reply}").and_then(|()| output.flush()) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_job_lays_out_a_template_as_pdf_or_pages() {
        let template = "#import \"/latte.typ\": *\n#show: setup\n#t(\"Hello\") #doc.name\n";
        let job = |output: &str| {
            json!({
                "job": VERSION,
                "document": {
                    "template": template,
                    "values": { "doc": { "name": "SINV-1" }, "print": {} },
                    "width": 14.8,
                    "height": 21.0,
                    "translations": { "Hello": "Hallo" },
                },
                "output": output,
                "pages": 1,
            })
        };
        let pdf = run(&job("pdf")).unwrap();
        assert_eq!(pdf["pages"], 1);
        assert!(pdf["pdf"].as_str().unwrap().starts_with("JVBER"), "%PDF");
        let png = run(&job("png")).unwrap();
        assert_eq!(png["png"].as_array().unwrap().len(), 1);

        let mut reply = Vec::new();
        let broken = json!({ "job": VERSION, "document": { "template": "#nothing.here" } });
        assert_eq!(serve(broken.to_string().as_bytes(), &mut reply), 0);
        let reply: Value = serde_json::from_slice(&reply).unwrap();
        assert!(
            reply["error"].as_str().unwrap().contains("Line 1"),
            "{reply}"
        );

        let old = json!({ "job": 0, "document": { "template": template } });
        assert!(run(&old).unwrap_err().contains("same version"));
    }
}
