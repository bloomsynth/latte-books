//! Exporting a list's documents: `src/utils/export.ts` @ v0.36.0, used by the Export
//! Wizard (`src/components/ExportWizard.vue`).
//!
//! The CSV has a row of labels, a row of `Schema.fieldname` keys, then a row for each
//! document, or for each of its child rows with the document's values repeated. The JSON
//! is the documents with their child rows. Money is exported as a number.

use latte_db::{
    FieldValue, FieldValueMap, Filter, FilterValue, GetAllOptions, QueryFilter, RawValue,
};
use latte_schema::{Field, FieldType};
use serde_json::{Map, Value};

use crate::fyo::model::helpers::js_number_string;
use crate::importer::generate_csv;
use crate::{Error, Fyo};

/// `ExportField`: a field and whether it is exported.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportField {
    pub fieldname: String,
    pub label: String,
    pub fieldtype: Option<FieldType>,
    pub export: bool,
}

/// `ExportTableField`: a table field and the fields of its rows.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportTableField {
    pub fieldname: String,
    pub label: String,
    pub target: String,
    pub fields: Vec<ExportField>,
}

/// `getExportFields`: the labelled, stored fields, all exported but attachments.
pub fn export_fields(fields: &[Field], exclude: &[&str]) -> Vec<ExportField> {
    fields
        .iter()
        .filter(|f| !f.computed && f.label.is_some() && !exclude.contains(&f.fieldname.as_str()))
        .map(|f| ExportField {
            fieldname: f.fieldname.clone(),
            label: f.label.clone().unwrap_or_default(),
            fieldtype: f.fieldtype,
            export: !matches!(
                f.fieldtype,
                Some(FieldType::AttachImage | FieldType::Attachment)
            ),
        })
        .collect()
}

/// `getExportTableFields`: each table field with its rows' fields, but `name`.
pub fn export_table_fields(fyo: &Fyo, fields: &[Field]) -> Vec<ExportTableField> {
    fields
        .iter()
        .filter(|f| f.fieldtype == Some(FieldType::Table))
        .filter_map(|f| {
            let target = f.target.clone()?;
            let row_fields = fyo
                .schema(&target)
                .map(|s| export_fields(&s.fields, &["name"]))
                .unwrap_or_default();
            (!row_fields.is_empty()).then(|| ExportTableField {
                fieldname: f.fieldname.clone(),
                label: f.label.clone().unwrap_or_default(),
                target,
                fields: row_fields,
            })
        })
        .collect()
}

/// A value as exported: money as a number (`convertRawPesaToFloat`), others as stored.
fn export_value(value: Option<&FieldValue>, field: Option<&ExportField>) -> Value {
    let raw = match value {
        Some(FieldValue::Value(raw)) => raw,
        _ => return Value::Null,
    };
    let currency = field.is_some_and(|f| f.fieldtype == Some(FieldType::Currency));
    match raw {
        RawValue::Null if currency => Value::from(0),
        RawValue::Null => Value::Null,
        RawValue::Integer(i) => Value::from(*i),
        RawValue::Real(r) => Value::from(*r),
        RawValue::Text(text) if currency => text
            .trim()
            .parse::<f64>()
            .map_or(Value::from(0), Value::from),
        RawValue::Text(text) => Value::from(text.clone()),
    }
}

/// A value as CSV text: `row[fieldname] ?? ''`, numbers as JavaScript prints them.
fn csv_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Number(n) => n.as_f64().map(js_number_string).unwrap_or_default(),
        Value::String(text) => text.clone(),
        Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    }
}

/// `getExportData`: the documents, and the rows of each exported table by its field.
struct ExportData {
    parents: Vec<Map<String, Value>>,
    children: Vec<(String, Vec<Map<String, Value>>)>,
}

fn export_data(
    fyo: &Fyo,
    schema_name: &str,
    fields: &[ExportField],
    table_fields: &[ExportTableField],
    limit: Option<u64>,
    filters: &QueryFilter,
) -> Result<ExportData, Error> {
    // `getParentData`.
    let mut order_by = vec!["created".to_owned()];
    if fyo
        .schema(schema_name)
        .is_some_and(|s| s.field("date").is_some())
    {
        order_by.insert(0, "date".to_owned());
    }
    let mut columns: Vec<String> = fields
        .iter()
        .filter(|f| f.export && f.fieldtype != Some(FieldType::Table))
        .map(|f| f.fieldname.clone())
        .collect();
    if !columns.iter().any(|c| c == "name") {
        columns.insert(0, "name".to_owned());
    }
    let options = GetAllOptions {
        fields: Some(columns.clone()),
        filters: filters.clone(),
        order_by: Some(order_by),
        limit,
        ..GetAllOptions::default()
    };
    let parents: Vec<Map<String, Value>> = fyo
        .db
        .core()
        .get_all(schema_name, &options)?
        .iter()
        .map(|row| to_json(row, &columns, fields))
        .collect();
    let names: Vec<FilterValue> = parents
        .iter()
        .filter_map(|p| p.get("name")?.as_str().map(FilterValue::from))
        .collect();

    // `getAllChildTableData`.
    let mut children = Vec::new();
    for table in table_fields {
        let exported = fields
            .iter()
            .any(|f| f.fieldname == table.fieldname && f.export);
        if !exported {
            continue;
        }
        let mut columns: Vec<String> = table
            .fields
            .iter()
            .filter(|f| f.export && f.fieldtype != Some(FieldType::Table))
            .map(|f| f.fieldname.clone())
            .collect();
        if !columns.iter().any(|c| c == "parent") {
            columns.insert(0, "parent".to_owned());
        }
        let options = GetAllOptions {
            fields: Some(columns.clone()),
            filters: QueryFilter::from([("parent".to_owned(), Filter::list("in", names.clone()))]),
            order_by: Some(vec!["idx".to_owned()]),
            ..GetAllOptions::default()
        };
        let rows = fyo
            .db
            .core()
            .get_all(&table.target, &options)?
            .iter()
            .map(|row| to_json(row, &columns, &table.fields))
            .collect();
        children.push((table.fieldname.clone(), rows));
    }
    Ok(ExportData { parents, children })
}

fn to_json(row: &FieldValueMap, columns: &[String], fields: &[ExportField]) -> Map<String, Value> {
    columns
        .iter()
        .map(|column| {
            let field = fields.iter().find(|f| &f.fieldname == column);
            (column.clone(), export_value(row.get(column), field))
        })
        .collect()
}

/// `getCsvExportData`.
pub fn csv_export(
    fyo: &Fyo,
    schema_name: &str,
    fields: &[ExportField],
    table_fields: &[ExportTableField],
    limit: Option<u64>,
    filters: &QueryFilter,
) -> Result<String, Error> {
    let data = export_data(fyo, schema_name, fields, table_fields, limit, filters)?;

    // `getCsvHeaders`: (schema, label, fieldname, table field).
    let parent_headers: Vec<(&str, &str, &str)> = fields
        .iter()
        .filter(|f| f.export && f.fieldtype != Some(FieldType::Table))
        .map(|f| (schema_name, f.label.as_str(), f.fieldname.as_str()))
        .collect();
    let mut child_headers: Vec<(&str, &str, &str, &str)> = Vec::new();
    for table in table_fields {
        let exported = fields
            .iter()
            .any(|f| f.fieldname == table.fieldname && f.export);
        if !exported {
            continue;
        }
        for f in table.fields.iter().filter(|f| f.export) {
            child_headers.push((
                table.target.as_str(),
                f.label.as_str(),
                f.fieldname.as_str(),
                table.fieldname.as_str(),
            ));
        }
    }

    let mut rows: Vec<Vec<String>> = Vec::new();
    rows.push(
        parent_headers
            .iter()
            .map(|(_, label, _)| (*label).to_owned())
            .chain(
                child_headers
                    .iter()
                    .map(|(_, label, ..)| (*label).to_owned()),
            )
            .collect(),
    );
    rows.push(
        parent_headers
            .iter()
            .map(|(schema, _, fieldname)| format!("{schema}.{fieldname}"))
            .chain(
                child_headers
                    .iter()
                    .map(|(schema, _, fieldname, _)| format!("{schema}.{fieldname}")),
            )
            .collect(),
    );
    for parent in &data.parents {
        let Some(name) = parent.get("name").and_then(Value::as_str) else {
            continue;
        };
        let base: Vec<String> = parent_headers
            .iter()
            .map(|(_, _, fieldname)| parent.get(*fieldname).map(csv_text).unwrap_or_default())
            .collect();
        // `getParentNameMap`: this document's rows, by table field.
        let tables: Vec<(&str, Vec<&Map<String, Value>>)> = data
            .children
            .iter()
            .map(|(fieldname, rows)| {
                let rows = rows
                    .iter()
                    .filter(|row| row.get("parent").and_then(Value::as_str) == Some(name))
                    .collect::<Vec<_>>();
                (fieldname.as_str(), rows)
            })
            .filter(|(_, rows)| !rows.is_empty())
            .collect();
        if tables.is_empty() {
            let mut row = base.clone();
            row.extend(child_headers.iter().map(|_| String::new()));
            rows.push(row);
            continue;
        }
        for (table, table_rows) in tables {
            for table_row in table_rows {
                let mut row = base.clone();
                row.extend(child_headers.iter().map(|(_, _, fieldname, parent_field)| {
                    if *parent_field != table {
                        return String::new();
                    }
                    table_row.get(*fieldname).map(csv_text).unwrap_or_default()
                }));
                rows.push(row);
            }
        }
    }
    Ok(generate_csv(&rows))
}

/// `getJsonExportData`: the documents, each with its exported tables' rows.
pub fn json_export(
    fyo: &Fyo,
    schema_name: &str,
    fields: &[ExportField],
    table_fields: &[ExportTableField],
    limit: Option<u64>,
    filters: &QueryFilter,
) -> Result<String, Error> {
    let mut data = export_data(fyo, schema_name, fields, table_fields, limit, filters)?;
    // `convertParentDataToJsonExport`.
    for (fieldname, rows) in std::mem::take(&mut data.children) {
        for mut row in rows {
            let Some(parent) = row.get("parent").and_then(Value::as_str).map(str::to_owned) else {
                continue;
            };
            let Some(doc) = data
                .parents
                .iter_mut()
                .find(|p| p.get("name").and_then(Value::as_str) == Some(parent.as_str()))
            else {
                continue;
            };
            row.remove("parent");
            row.remove("name");
            if let Value::Array(list) = doc
                .entry(fieldname.clone())
                .or_insert_with(|| Value::Array(Vec::new()))
            {
                list.push(Value::Object(row));
            }
        }
    }
    let parents: Vec<Value> = data.parents.into_iter().map(Value::Object).collect();
    Ok(serde_json::to_string(&parents).unwrap_or_default())
}

#[cfg(test)]
#[path = "export_tests.rs"]
mod tests;
