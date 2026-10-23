//! The tools an assistant can use, and what they do.

use jiff::civil::Date;
use latte_core::fyo::format::Formatter;
use latte_core::fyo::model::helpers::is_truthy;
use latte_core::reports::{self, FilterKind, FilterValue};
use latte_core::{Doc, Fyo};
use latte_db::{DocValue, DocValueMap, Filter, GetAllOptions, Order, QueryFilter};
use latte_schema::{FieldType, Schema};
use serde_json::{Map, Value, json};

use crate::values::{doc_json, field_value, find_field, map_json, row_values};

/// What a tool returns: text for the assistant, or what went wrong.
pub type Outcome = Result<String, String>;

fn tool(
    name: &str,
    description: &str,
    read_only: bool,
    properties: Value,
    required: &[&str],
) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
        },
        "annotations": {
            "readOnlyHint": read_only,
            "destructiveHint": !read_only && matches!(name, "cancel_document" | "delete_document"),
        },
    })
}

/// `tools/list`.
pub fn list() -> Value {
    let doc_type = json!({ "type": "string", "description": "The document type, such as SalesInvoice, Party, Item or Payment" });
    let name = json!({ "type": "string", "description": "The document's name, such as SINV-1001" });
    let values = json!({
        "type": "object",
        "description": "Field values by fieldname. A table field takes a list of rows, each an object of the row's fields. Amounts and quantities are numbers or text, dates are like 2026-04-01, and links are the linked document's name.",
    });
    let reports: Vec<&str> = reports::REPORTS.iter().map(|(name, _)| *name).collect();
    json!([
        tool(
            "company_info",
            "The company these books are for: its name, country, currency, fiscal year and file.",
            true,
            json!({}),
            &[]
        ),
        tool(
            "list_document_types",
            "The kinds of documents in the books, such as SalesInvoice, Payment, Party and Item.",
            true,
            json!({}),
            &[]
        ),
        tool(
            "describe_document_type",
            "A document type's fields: fieldname, label, type, whether required, its options or the type it links to, and its tables' fields.",
            true,
            json!({ "type": doc_type }),
            &["type"]
        ),
        tool(
            "find_documents",
            "Lists documents of a type, newest first unless ordered otherwise.",
            true,
            json!({
                "type": doc_type,
                "filters": { "type": "object", "description": "By fieldname: a value to equal, or [operator, value] with operator one of =, !=, <, >, <=, >=, like, not like, in, not in (in takes a list). Example: {\"party\": \"Acme\", \"outstandingAmount\": [\">\", 0]}" },
                "fields": { "type": "array", "items": { "type": "string" }, "description": "Fieldnames to return; the name and a few key fields if left out" },
                "order_by": { "type": "string", "description": "A fieldname to sort by" },
                "ascending": { "type": "boolean" },
                "limit": { "type": "integer", "description": "At most this many (default 50, up to 500)" },
            }),
            &["type"]
        ),
        tool(
            "get_document",
            "One document with its rows. Settings such as AccountingSettings take no name.",
            true,
            json!({ "type": doc_type, "name": name }),
            &["type"]
        ),
        tool(
            "create_document",
            "Saves a new document as a draft, with the same checks as the app, and returns it with its name and totals. A draft changes nothing in the accounts until it is submitted.",
            false,
            json!({ "type": doc_type, "values": values }),
            &["type", "values"]
        ),
        tool(
            "update_document",
            "Changes a draft, or settings such as AccountingSettings (no name). A table given replaces the table's rows. Returns the document.",
            false,
            json!({ "type": doc_type, "name": name, "values": values }),
            &["type", "values"]
        ),
        tool(
            "submit_document",
            "Submits a draft, which posts it to the accounts (and moves stock). Check the draft with the person first.",
            false,
            json!({ "type": doc_type, "name": name }),
            &["type", "name"]
        ),
        tool(
            "cancel_document",
            "Cancels a submitted document, reversing what it posted.",
            false,
            json!({ "type": doc_type, "name": name }),
            &["type", "name"]
        ),
        tool(
            "delete_document",
            "Deletes a draft or cancelled document.",
            false,
            json!({ "type": doc_type, "name": name }),
            &["type", "name"]
        ),
        tool(
            "list_reports",
            "The reports, with the filters each one takes.",
            true,
            json!({}),
            &[]
        ),
        tool(
            "run_report",
            "Runs a report and returns it as CSV.",
            true,
            json!({
                "report": { "type": "string", "enum": reports },
                "filters": { "type": "object", "description": "By filter name, from list_reports. Dates are like 2026-04-01." },
            }),
            &["report"]
        ),
    ])
}

/// `tools/call`.
pub fn call(fyo: &mut Fyo, name: &str, args: &Value) -> Outcome {
    match name {
        "company_info" => company_info(fyo),
        "list_document_types" => list_document_types(fyo),
        "describe_document_type" => describe(fyo, text_arg(args, "type")?),
        "find_documents" => find(fyo, args),
        "get_document" => {
            let schema = schema(fyo, text_arg(args, "type")?)?;
            let doc = load(fyo, &schema, opt_text(args, "name"))?;
            pretty(&doc_json(&doc))
        }
        "create_document" => create(fyo, text_arg(args, "type")?, args.get("values")),
        "update_document" => update(
            fyo,
            text_arg(args, "type")?,
            opt_text(args, "name"),
            args.get("values"),
        ),
        "submit_document" => {
            let schema = schema(fyo, text_arg(args, "type")?)?;
            let mut doc = load(fyo, &schema, Some(text_arg(args, "name")?))?;
            if !schema.is_submittable {
                return Err(format!("{} documents aren't submitted", schema.label));
            }
            doc.submit(fyo).map_err(|e| e.to_string())?;
            pretty(&doc_json(&doc))
        }
        "cancel_document" => {
            let schema = schema(fyo, text_arg(args, "type")?)?;
            let mut doc = load(fyo, &schema, Some(text_arg(args, "name")?))?;
            if !doc.can_cancel() {
                return Err(format!(
                    "{} can be cancelled only once submitted",
                    doc_label(&doc)
                ));
            }
            doc.cancel(fyo).map_err(|e| e.to_string())?;
            Ok(format!("{} is cancelled.", doc_label(&doc)))
        }
        "delete_document" => {
            let schema = schema(fyo, text_arg(args, "type")?)?;
            let mut doc = load(fyo, &schema, Some(text_arg(args, "name")?))?;
            if !doc.can_delete() {
                return Err(format!("{} is submitted; cancel it first", doc_label(&doc)));
            }
            let label = doc_label(&doc);
            doc.delete(fyo).map_err(|e| e.to_string())?;
            Ok(format!("{label} is deleted."))
        }
        "list_reports" => list_reports(fyo),
        "run_report" => run_report(fyo, text_arg(args, "report")?, args.get("filters")),
        _ => Err(format!("There's no tool \"{name}\"")),
    }
}

fn text_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    opt_text(args, key).ok_or_else(|| format!("\"{key}\" is needed"))
}

fn opt_text<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

fn pretty(value: &Value) -> Outcome {
    serde_json::to_string_pretty(value).map_err(|e| e.to_string())
}

fn doc_label(doc: &Doc) -> String {
    format!("{} {}", doc.schema().label, doc.name().unwrap_or_default())
}

fn setting(fyo: &Fyo, schema: &str, field: &str) -> Value {
    match fyo.get_value(schema, None, field) {
        Some(DocValue::Text(text)) => json!(text),
        Some(DocValue::Date(timestamp)) => json!(crate::values::date_text(
            timestamp,
            &jiff::tz::TimeZone::system()
        )),
        Some(DocValue::Bool(b)) => json!(b),
        Some(DocValue::Number(n)) => json!(n),
        _ => Value::Null,
    }
}

fn company_info(fyo: &Fyo) -> Outcome {
    pretty(&json!({
        "company": setting(fyo, "AccountingSettings", "companyName"),
        "country": setting(fyo, "AccountingSettings", "country"),
        "currency": setting(fyo, "SystemSettings", "currency"),
        "fiscalYearStart": setting(fyo, "AccountingSettings", "fiscalYearStart"),
        "fiscalYearEnd": setting(fyo, "AccountingSettings", "fiscalYearEnd"),
        "inventory": is_truthy(fyo.get_value("AccountingSettings", None, "enableInventory").as_ref()),
        "today": jiff::Zoned::now().date().to_string(),
    }))
}

/// The schemas an assistant can work with: not rows.
fn document_types(fyo: &Fyo) -> Vec<&Schema> {
    let mut schemas: Vec<&Schema> = fyo
        .db
        .schema_map()
        .values()
        .filter(|s| !s.is_child)
        .collect();
    schemas.sort_by(|a, b| a.name.cmp(&b.name));
    schemas
}

fn list_document_types(fyo: &Fyo) -> Outcome {
    let types: Vec<Value> = document_types(fyo)
        .iter()
        .map(|s| {
            let mut kind = Vec::new();
            if s.is_submittable {
                kind.push("submittable");
            }
            if s.is_single {
                kind.push("settings");
            }
            json!({ "type": s.name, "label": s.label, "kind": kind.join(", ") })
        })
        .collect();
    pretty(&Value::Array(types))
}

fn schema(fyo: &Fyo, name: &str) -> Result<std::sync::Arc<Schema>, String> {
    fyo.schema(name).cloned().ok_or_else(|| {
        format!("There's no document type \"{name}\"; list_document_types lists them")
    })
}

fn fields_json(fyo: &Fyo, schema: &Schema) -> Vec<Value> {
    schema
        .fields
        .iter()
        .filter(|f| !f.meta && !f.computed)
        .map(|f| {
            let mut field = Map::new();
            field.insert("fieldname".into(), json!(f.fieldname));
            field.insert(
                "label".into(),
                json!(f.label.as_deref().unwrap_or(&f.fieldname)),
            );
            field.insert("type".into(), json!(f.fieldtype.map(|t| format!("{t:?}"))));
            if f.required {
                field.insert("required".into(), json!(true));
            }
            if f.read_only {
                field.insert("readOnly".into(), json!(true));
            }
            if let Some(target) = &f.target {
                if f.fieldtype == Some(FieldType::Table) {
                    if let Some(child) = fyo.schema(target) {
                        field.insert("rows".into(), Value::Array(fields_json(fyo, child)));
                    }
                } else {
                    field.insert("linksTo".into(), json!(target));
                }
            }
            if let Some(options) = f.options.as_ref().filter(|o| !o.is_empty()) {
                let values: Vec<&str> = options.iter().map(|o| o.value.as_str()).collect();
                field.insert("options".into(), json!(values));
            }
            if let Some(description) = &f.description {
                field.insert("description".into(), json!(description));
            }
            Value::Object(field)
        })
        .collect()
}

fn describe(fyo: &Fyo, name: &str) -> Outcome {
    let schema = schema(fyo, name)?;
    pretty(&json!({
        "type": schema.name,
        "label": schema.label,
        "submittable": schema.is_submittable,
        "settings": schema.is_single,
        "fields": fields_json(fyo, &schema),
    }))
}

fn filter_value(value: &Value) -> Result<latte_db::FilterValue, String> {
    Ok(match value {
        Value::Null => latte_db::FilterValue::Null,
        Value::Bool(b) => (*b).into(),
        Value::Number(n) => n.as_f64().unwrap_or_default().into(),
        Value::String(s) => s.as_str().into(),
        _ => return Err(format!("{value} can't be a filter value")),
    })
}

fn find(fyo: &Fyo, args: &Value) -> Outcome {
    let schema = schema(fyo, text_arg(args, "type")?)?;
    if schema.is_single {
        return Err(format!(
            "{} is settings; use get_document without a name",
            schema.name
        ));
    }
    let mut filters = QueryFilter::new();
    if let Some(Value::Object(given)) = args.get("filters") {
        for (fieldname, value) in given {
            find_field(&schema, fieldname)?;
            let filter = match value {
                Value::Array(parts) => match parts.as_slice() {
                    [Value::String(op), Value::Array(list)] => {
                        Filter::list(op, list.iter().map(filter_value).collect::<Result<_, _>>()?)
                    }
                    [Value::String(op), value] => Filter::op(op, filter_value(value)?),
                    _ => {
                        return Err(format!(
                            "The filter on {fieldname} should be [operator, value]"
                        ));
                    }
                },
                value => Filter::Equals(filter_value(value)?),
            };
            filters.insert(fieldname.clone(), filter);
        }
    }
    let fields: Vec<String> = match args.get("fields").and_then(Value::as_array) {
        Some(fields) => {
            let mut names = vec!["name".to_owned()];
            for field in fields.iter().filter_map(Value::as_str) {
                if find_field(&schema, field)?.fieldtype == Some(FieldType::Table) {
                    return Err(format!("{field} is a table; get_document shows its rows"));
                }
                if field != "name" {
                    names.push(field.to_owned());
                }
            }
            names
        }
        None => {
            let mut names = vec!["name".to_owned()];
            for field in schema.keyword_fields.iter().flatten() {
                if schema
                    .fields
                    .iter()
                    .any(|f| &f.fieldname == field && !f.computed)
                {
                    names.push(field.clone());
                }
            }
            if schema.is_submittable {
                names.extend(["submitted".to_owned(), "cancelled".to_owned()]);
            }
            names
        }
    };
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(50)
        .clamp(1, 500);
    let options = GetAllOptions {
        fields: Some(fields),
        filters,
        limit: Some(limit),
        order_by: opt_text(args, "order_by").map(|f| vec![f.to_owned()]),
        order: Some(
            if args.get("ascending").and_then(Value::as_bool) == Some(true) {
                Order::Asc
            } else {
                Order::Desc
            },
        ),
        ..GetAllOptions::default()
    };
    let rows = fyo
        .db
        .get_all_docs(&schema.name, &options)
        .map_err(|e| e.to_string())?;
    pretty(&Value::Array(
        rows.iter().map(|row| map_json(&schema, row)).collect(),
    ))
}

fn load(fyo: &mut Fyo, schema: &Schema, name: Option<&str>) -> Result<Doc, String> {
    match (schema.is_single, name) {
        (true, _) => fyo.get_doc(&schema.name, None),
        (false, Some(name)) => fyo.get_doc(&schema.name, Some(name)),
        (false, None) => return Err(format!("Which {}? Give its name", schema.label)),
    }
    .map_err(|e| e.to_string())
}

/// Sets the values given on a document, a table's rows replacing what it had.
fn set_values(fyo: &mut Fyo, doc: &mut Doc, values: Option<&Value>) -> Result<(), String> {
    let Some(Value::Object(values)) = values else {
        return Err("\"values\" should be an object of field values".to_owned());
    };
    let schema = doc.schema().clone();
    for (fieldname, value) in values {
        let field = find_field(&schema, fieldname)?;
        if field.fieldtype == Some(FieldType::Table) {
            let Value::Array(rows) = value else {
                return Err(format!("{fieldname} is a table; give a list of rows"));
            };
            let child = field
                .target
                .as_deref()
                .and_then(|t| fyo.schema(t))
                .cloned()
                .ok_or_else(|| format!("{fieldname} has no row type"))?;
            while !doc.table(fieldname).is_empty() {
                doc.remove(fyo, fieldname, 0).map_err(|e| e.to_string())?;
            }
            // Each value is set on its own, as in the form, so that picking an item fills
            // in its rate and the rest.
            for row in rows {
                let row = row_values(&child, row, fyo.pesa())?;
                doc.append(fyo, fieldname, DocValueMap::new())
                    .map_err(|e| e.to_string())?;
                let idx = doc.table(fieldname).len() - 1;
                for (field, value) in row {
                    doc.set_child(fyo, fieldname, idx, &field, value)
                        .map_err(|e| e.to_string())?;
                }
            }
            continue;
        }
        if field.read_only || field.computed || field.meta {
            return Err(format!(
                "{fieldname} is worked out by the books and can't be set"
            ));
        }
        let value = field_value(field, value, fyo.pesa())?;
        doc.set(fyo, fieldname, value).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn create(fyo: &mut Fyo, type_name: &str, values: Option<&Value>) -> Outcome {
    let schema = schema(fyo, type_name)?;
    if schema.is_single {
        return Err(format!(
            "{} is settings; use update_document without a name",
            schema.name
        ));
    }
    let mut doc = fyo
        .get_new_doc(&schema.name, DocValueMap::new())
        .map_err(|e| e.to_string())?;
    set_values(fyo, &mut doc, values)?;
    doc.sync(fyo).map_err(|e| e.to_string())?;
    pretty(&doc_json(&doc))
}

fn update(fyo: &mut Fyo, type_name: &str, name: Option<&str>, values: Option<&Value>) -> Outcome {
    let schema = schema(fyo, type_name)?;
    let mut doc = load(fyo, &schema, name)?;
    if doc.is_submitted() || doc.is_cancelled() {
        return Err(format!(
            "{} is {}; only drafts can be changed",
            doc_label(&doc),
            if doc.is_cancelled() {
                "cancelled"
            } else {
                "submitted"
            }
        ));
    }
    set_values(fyo, &mut doc, values)?;
    doc.sync(fyo).map_err(|e| e.to_string())?;
    pretty(&doc_json(&doc))
}

fn filter_kind(kind: &FilterKind) -> Value {
    match kind {
        FilterKind::Select(options) => {
            json!({ "options": options.iter().map(|(_, v)| v.as_str()).collect::<Vec<_>>() })
        }
        FilterKind::Link(target) => json!({ "linksTo": target }),
        FilterKind::DynamicLink { references } => json!({ "linksToTypeIn": references }),
        FilterKind::Date => json!("date"),
        FilterKind::Int { min } => json!({ "integer": { "min": min } }),
        FilterKind::Check => json!("true/false"),
    }
}

fn list_reports(fyo: &Fyo) -> Outcome {
    let list: Vec<Value> = reports::REPORTS
        .iter()
        .filter_map(|(name, title)| {
            let report = reports::get_report(name)?;
            let filters: Vec<Value> = report
                .filters(fyo)
                .iter()
                .map(|f| {
                    json!({
                        "name": f.fieldname,
                        "label": f.label,
                        "kind": filter_kind(&f.kind),
                        "required": f.required,
                    })
                })
                .collect();
            Some(json!({ "report": name, "title": title, "filters": filters }))
        })
        .collect();
    pretty(&Value::Array(list))
}

fn run_report(fyo: &Fyo, name: &str, filters: Option<&Value>) -> Outcome {
    let mut report =
        reports::get_report(name).ok_or_else(|| format!("There's no report \"{name}\""))?;
    let known = report.filters(fyo);
    if let Some(Value::Object(given)) = filters {
        for (fieldname, value) in given {
            let filter = known
                .iter()
                .find(|f| &f.fieldname == fieldname)
                .ok_or_else(|| {
                    format!("{name} has no filter \"{fieldname}\"; list_reports lists them")
                })?;
            let value = match (&filter.kind, value) {
                (_, Value::Null) => FilterValue::Null,
                (FilterKind::Date, Value::String(text)) => {
                    FilterValue::Date(text.parse::<Date>().map_err(|_| {
                        format!("{fieldname}: \"{text}\" is not a date like 2026-04-01")
                    })?)
                }
                (FilterKind::Int { .. }, value) => FilterValue::Int(
                    value
                        .as_i64()
                        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
                        .ok_or_else(|| format!("{fieldname} takes a whole number"))?,
                ),
                (FilterKind::Check, Value::Bool(b)) => FilterValue::Bool(*b),
                (_, Value::String(text)) => FilterValue::Text(text.clone()),
                _ => return Err(format!("{fieldname}: {value} isn't a value it takes")),
            };
            report.set(fieldname, value);
        }
    }
    let formatter = Formatter::new(fyo).map_err(|e| e.to_string())?;
    let columns = report.columns(fyo, &formatter).map_err(|e| e.to_string())?;
    let rows = report.run(fyo, &formatter).map_err(|e| e.to_string())?;
    Ok(reports::csv(&columns, &rows, formatter.display_precision))
}
