//! Document values as JSON, and back.
//!
//! Amounts go out as text at the currency's precision ("1250.00") so nothing is lost to
//! floats; dates as `2026-10-23`, the date the app stores and shows.

use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use latte_core::Doc;
use latte_db::pesa::MoneyMaker;
use latte_db::{DocValue, DocValueMap, convert_doc_value, to_iso_string};
use latte_schema::{Field, FieldType, Schema};
use serde_json::{Map, Value, json};

/// Meta fields worth showing.
const SHOWN_META: [&str; 2] = ["submitted", "cancelled"];

pub fn value_json(value: &DocValue, fieldtype: Option<FieldType>) -> Value {
    match value {
        DocValue::Null => Value::Null,
        DocValue::Text(text) => json!(text),
        DocValue::Number(n) => json!(n),
        DocValue::Bool(b) => json!(b),
        DocValue::Money(money) => json!(money.round(None)),
        DocValue::Date(timestamp) if fieldtype == Some(FieldType::Date) => {
            json!(date_text(*timestamp, &TimeZone::system()))
        }
        DocValue::Date(timestamp) => json!(to_iso_string(*timestamp)),
        // The file's data is left out: it can be large, and an agent can't use it.
        DocValue::Attachment(attachment) => json!({ "name": attachment.get("name") }),
        DocValue::Table(rows) => Value::Array(
            rows.iter()
                .map(|row| {
                    Value::Object(
                        row.iter()
                            .map(|(k, v)| (k.clone(), value_json(v, None)))
                            .collect(),
                    )
                })
                .collect(),
        ),
    }
}

fn shown(field: &Field) -> bool {
    !field.meta || SHOWN_META.contains(&field.fieldname.as_str())
}

/// A document with its rows, leaving out empty values and bookkeeping fields.
pub fn doc_json(doc: &Doc) -> Value {
    let mut object = Map::new();
    if let Some(name) = doc.name() {
        object.insert("name".to_owned(), json!(name));
    }
    for field in &doc.schema().fields {
        if field.fieldname == "name" || !shown(field) {
            continue;
        }
        if field.fieldtype == Some(FieldType::Table) {
            let rows: Vec<Value> = doc.table(&field.fieldname).iter().map(row_json).collect();
            if !rows.is_empty() {
                object.insert(field.fieldname.clone(), Value::Array(rows));
            }
            continue;
        }
        match doc.get(&field.fieldname) {
            None | Some(DocValue::Null) => {}
            Some(DocValue::Text(text)) if text.is_empty() => {}
            Some(value) => {
                object.insert(field.fieldname.clone(), value_json(value, field.fieldtype));
            }
        }
    }
    Value::Object(object)
}

fn row_json(row: &Doc) -> Value {
    let mut object = Map::new();
    for field in &row.schema().fields {
        if field.meta || field.fieldname == "name" {
            continue;
        }
        match row.get(&field.fieldname) {
            None | Some(DocValue::Null) => {}
            Some(DocValue::Text(text)) if text.is_empty() => {}
            Some(value) => {
                object.insert(field.fieldname.clone(), value_json(value, field.fieldtype));
            }
        }
    }
    Value::Object(object)
}

/// A row read from the file, as JSON.
pub fn map_json(schema: &Schema, row: &DocValueMap) -> Value {
    Value::Object(
        row.iter()
            .map(|(fieldname, value)| {
                let fieldtype = schema
                    .fields
                    .iter()
                    .find(|f| &f.fieldname == fieldname)
                    .and_then(|f| f.fieldtype);
                (fieldname.clone(), value_json(value, fieldtype))
            })
            .collect(),
    )
}

/// A Date field's value as its date, `2026-04-01`.
///
/// A Date field is stored as a date alone, and read as midnight on it in the computer's
/// time zone, as Books reads it (`latte_db`'s converter); so its date is the date in
/// that zone, wherever the computer is. In UTC, a date east of Greenwich would show as
/// the day before.
pub fn date_text(timestamp: Timestamp, tz: &TimeZone) -> String {
    timestamp.to_zoned(tz.clone()).date().to_string()
}

/// A date for a Date field: midnight on it in the computer's time zone, which the file
/// stores as that date. Midnight in UTC would be stored as the day before west of
/// Greenwich.
pub fn date_timestamp(date: Date, tz: &TimeZone) -> Result<Timestamp, String> {
    date.to_zoned(tz.clone())
        .map(|zoned| zoned.timestamp())
        .map_err(|e| e.to_string())
}

/// A JSON value for a field, as the document value the field takes.
pub fn field_value(field: &Field, value: &Value, pesa: &MoneyMaker) -> Result<DocValue, String> {
    let label = field.label.as_deref().unwrap_or(&field.fieldname);
    let value = match (field.fieldtype, value) {
        (_, Value::Null) => DocValue::Null,
        (Some(FieldType::Date), Value::String(text)) if !text.is_empty() => {
            let date: Date = text
                .get(..10)
                .and_then(|d| d.parse().ok())
                .ok_or_else(|| format!("{label}: \"{text}\" is not a date like 2026-04-01"))?;
            DocValue::Date(date_timestamp(date, &TimeZone::system())?)
        }
        (Some(FieldType::Datetime), Value::String(text)) if !text.is_empty() => {
            let timestamp: Timestamp = text
                .parse()
                .or_else(|_| format!("{text}T00:00:00Z").parse())
                .map_err(|_| {
                    format!("{label}: \"{text}\" is not a time like 2026-04-01T10:30:00Z")
                })?;
            DocValue::Date(timestamp)
        }
        (_, Value::String(text)) => DocValue::Text(text.clone()),
        (_, Value::Number(n)) => DocValue::Number(n.as_f64().unwrap_or_default()),
        (_, Value::Bool(b)) => DocValue::Bool(*b),
        _ => return Err(format!("{label}: give a text, number or true/false")),
    };
    if let (Some(options), DocValue::Text(text)) = (&field.options, &value)
        && !options.is_empty()
        && !options.iter().any(|o| &o.value == text)
    {
        // An option may be given by its label.
        if let Some(option) = options.iter().find(|o| &o.label == text) {
            return Ok(DocValue::Text(option.value.clone()));
        }
        let values: Vec<&str> = options.iter().map(|o| o.value.as_str()).collect();
        return Err(format!(
            "{label}: \"{text}\" isn't one of {}",
            values.join(", ")
        ));
    }
    convert_doc_value(&value, field, pesa).map_err(|e| format!("{label}: {e}"))
}

/// A row given for a table, as values of the row's schema.
pub fn row_values(schema: &Schema, row: &Value, pesa: &MoneyMaker) -> Result<DocValueMap, String> {
    let Value::Object(row) = row else {
        return Err(format!(
            "Each {} row is an object of its fields",
            schema.label
        ));
    };
    let mut values = DocValueMap::new();
    for (fieldname, value) in row {
        let field = find_field(schema, fieldname)?;
        values.insert(fieldname.clone(), field_value(field, value, pesa)?);
    }
    Ok(values)
}

pub fn find_field<'a>(schema: &'a Schema, fieldname: &str) -> Result<&'a Field, String> {
    schema
        .fields
        .iter()
        .find(|f| f.fieldname == fieldname)
        .ok_or_else(|| {
            format!(
                "{} has no field \"{fieldname}\"; describe_document_type lists its fields",
                schema.name
            )
        })
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;
    use jiff::tz::{Offset, TimeZone};
    use latte_db::pesa::MoneyMaker;
    use latte_db::{RawValue, to_doc_value, to_raw_value};
    use latte_schema::{FieldType, get_schemas};
    use serde_json::json;

    use super::{date_text, date_timestamp, field_value, value_json};

    fn zone(hours: i32, minutes: i32) -> TimeZone {
        TimeZone::fixed(Offset::from_seconds(hours * 3600 + minutes * 60).unwrap())
    }

    /// A date given in India (UTC+5:30) or the US (UTC-5) comes back as the same date:
    /// in UTC the first read back as the day before, and the second was stored as it.
    #[test]
    fn a_date_stays_the_same_date_east_and_west_of_greenwich() {
        let april_first = date(2025, 4, 1);
        for tz in [
            zone(5, 30),
            zone(-5, 0),
            zone(14, 0),
            zone(-11, 0),
            TimeZone::UTC,
        ] {
            let timestamp = date_timestamp(april_first, &tz).unwrap();
            assert_eq!(date_text(timestamp, &tz), "2025-04-01", "{tz:?}");
        }
        let india = date_timestamp(april_first, &zone(5, 30)).unwrap();
        assert_eq!(date_text(india, &TimeZone::UTC), "2025-03-31");
    }

    /// Through the file's converter, which uses this computer's time zone: what an
    /// assistant gives is stored as that date and read back as it.
    #[test]
    fn a_date_field_is_stored_and_read_as_the_date_given() {
        let schemas = get_schemas("in", &[]).unwrap();
        let field = schemas["AccountingSettings"]
            .field("fiscalYearStart")
            .unwrap()
            .clone();
        assert_eq!(field.fieldtype, Some(FieldType::Date));
        let pesa = MoneyMaker {
            currency: "INR".into(),
            precision: 11,
            display: 2,
        };

        let value = field_value(&field, &json!("2025-04-01"), &pesa).unwrap();
        let raw = to_raw_value(&value, &field, &pesa).unwrap();
        assert_eq!(raw, RawValue::Text("2025-04-01".into()));

        let read = to_doc_value(&raw, &field, &pesa).unwrap();
        assert_eq!(value_json(&read, field.fieldtype), json!("2025-04-01"));
    }
}
