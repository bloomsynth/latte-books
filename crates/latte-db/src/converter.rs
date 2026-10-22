// Port of fyo/core/converter.ts @ v0.36.0: database values to document values and back.

use std::str::FromStr;

use indexmap::IndexMap;
use jiff::Timestamp;
use jiff::civil;
use jiff::tz::TimeZone;
use latte_schema::{Field, FieldType, SchemaMap};

use crate::Error;
use crate::core::{FieldValue, FieldValueMap, RawValue};
use crate::pesa::{Money, MoneyMaker};

/// A value as documents use it. `DocValue` in Books.
#[derive(Debug, Clone, PartialEq)]
pub enum DocValue {
    Null,
    Text(String),
    /// Int and Float fields. JavaScript numbers in Books, so an Int stored as `60.0` reads
    /// as 60 and one stored as `"7.9"` as 7.
    Number(f64),
    Bool(bool),
    Money(Money),
    /// Date and Datetime fields: an instant, as JavaScript's `Date`.
    Date(Timestamp),
    /// Attachment fields: `{ name, type, data }` as stored, not checked.
    Attachment(serde_json::Value),
    Table(Vec<DocValueMap>),
}

pub type DocValueMap = IndexMap<String, DocValue>;

/// Converts a row read by `DatabaseCore` into document values. Fields the schema doesn't
/// have are left out.
pub fn to_doc_value_map(
    schema_map: &SchemaMap,
    pesa: &MoneyMaker,
    schema_name: &str,
    raw_value_map: &FieldValueMap,
) -> Result<DocValueMap, Error> {
    let Some(schema) = schema_map.get(schema_name) else {
        return Err(Error::NotFound(format!("schema {schema_name} not found")));
    };

    let mut doc_value_map = DocValueMap::new();
    for (fieldname, raw_value) in raw_value_map {
        let Some(field) = schema.field(fieldname) else {
            continue;
        };

        let value = match raw_value {
            FieldValue::Table(rows) => {
                let target = field.target.as_deref().unwrap_or_default();
                let rows = rows
                    .iter()
                    .map(|row| to_doc_value_map(schema_map, pesa, target, row))
                    .collect::<Result<_, _>>()?;
                DocValue::Table(rows)
            }
            FieldValue::Value(value) => to_doc_value(value, field, pesa)?,
        };
        doc_value_map.insert(fieldname.clone(), value);
    }
    Ok(doc_value_map)
}

pub fn to_doc_value(value: &RawValue, field: &Field, pesa: &MoneyMaker) -> Result<DocValue, Error> {
    match field.fieldtype {
        Some(FieldType::Currency) => to_doc_currency(value, field, pesa),
        Some(FieldType::Date | FieldType::Datetime) => to_doc_date(value, field),
        Some(FieldType::Int) => to_doc_int(value, field),
        Some(FieldType::Float) => to_doc_float(value, field),
        Some(FieldType::Check) => to_doc_check(value, field),
        Some(FieldType::Attachment) => to_doc_attachment(value, field),
        _ => to_doc_string(value, field),
    }
}

/// `Converter.toDocValue` given a value that may already be a document value, as Books
/// does with the data for a new document: money, dates and booleans that fit the field are
/// kept, and text and numbers are read as database values are. A table is an error; rows
/// are made into documents by the caller.
pub fn convert_doc_value(
    value: &DocValue,
    field: &Field,
    pesa: &MoneyMaker,
) -> Result<DocValue, Error> {
    let fieldtype = field.fieldtype;
    match value {
        DocValue::Money(_) if fieldtype == Some(FieldType::Currency) => Ok(value.clone()),
        DocValue::Date(_) if matches!(fieldtype, Some(FieldType::Date | FieldType::Datetime)) => {
            Ok(value.clone())
        }
        DocValue::Attachment(_) if fieldtype == Some(FieldType::Attachment) => Ok(value.clone()),
        DocValue::Bool(_) if fieldtype == Some(FieldType::Check) => Ok(value.clone()),
        DocValue::Bool(b)
            if matches!(
                fieldtype,
                Some(FieldType::Currency | FieldType::Int | FieldType::Float)
            ) =>
        {
            to_doc_value(&RawValue::Real(f64::from(*b)), field, pesa)
        }
        DocValue::Null => to_doc_value(&RawValue::Null, field, pesa),
        DocValue::Text(text) => to_doc_value(&RawValue::Text(text.clone()), field, pesa),
        DocValue::Number(n) => to_doc_value(&RawValue::Real(*n), field, pesa),
        _ => Err(Error::Value(format!(
            "invalid doc conversion {value:?} found, field: {}.{}",
            field.schema_name.as_deref().unwrap_or_default(),
            field.fieldname
        ))),
    }
}

fn to_doc_string(value: &RawValue, field: &Field) -> Result<DocValue, Error> {
    match value {
        RawValue::Null => Ok(DocValue::Null),
        RawValue::Text(text) => Ok(DocValue::Text(text.clone())),
        _ => Err(conversion_error(value, field)),
    }
}

fn to_doc_date(value: &RawValue, field: &Field) -> Result<DocValue, Error> {
    let text = match value {
        RawValue::Null => return Ok(DocValue::Null),
        RawValue::Text(text) if text.is_empty() => return Ok(DocValue::Null),
        RawValue::Text(text) => text,
        _ => return Err(conversion_error(value, field)),
    };
    parse_iso(text)
        .map(DocValue::Date)
        .ok_or_else(|| conversion_error(value, field))
}

/// Luxon's `DateTime.fromISO`: an instant when the text has an offset, else a time in the
/// system time zone.
fn parse_iso(text: &str) -> Option<Timestamp> {
    if let Ok(timestamp) = Timestamp::from_str(text) {
        return Some(timestamp);
    }
    let civil = civil::DateTime::from_str(text)
        .or_else(|_| civil::Date::from_str(text).map(|d| d.to_datetime(civil::Time::midnight())))
        .ok()?;
    civil
        .to_zoned(TimeZone::system())
        .ok()
        .map(|z| z.timestamp())
}

fn to_doc_currency(value: &RawValue, _field: &Field, pesa: &MoneyMaker) -> Result<DocValue, Error> {
    let money = match value {
        RawValue::Text(text) if text.is_empty() => pesa.zero(),
        RawValue::Text(text) => pesa.parse(text)?,
        RawValue::Integer(i) => pesa.from_f64(*i as f64)?,
        RawValue::Real(r) => pesa.from_f64(*r)?,
        RawValue::Null => pesa.zero(),
    };
    Ok(DocValue::Money(money))
}

fn to_doc_int(value: &RawValue, field: &Field) -> Result<DocValue, Error> {
    match value {
        RawValue::Text(text) if text.is_empty() => Ok(DocValue::Number(0.0)),
        RawValue::Text(text) => to_doc_float(&RawValue::Real(safe_parse_int(text)), field),
        _ => to_doc_float(value, field),
    }
}

fn to_doc_float(value: &RawValue, field: &Field) -> Result<DocValue, Error> {
    let number = match value {
        RawValue::Text(text) if text.is_empty() => 0.0,
        RawValue::Text(text) => safe_parse_float(text),
        RawValue::Null => 0.0,
        RawValue::Integer(i) => *i as f64,
        RawValue::Real(r) => *r,
    };
    if number.is_nan() {
        return Err(conversion_error(value, field));
    }
    Ok(DocValue::Number(number))
}

fn to_doc_check(value: &RawValue, field: &Field) -> Result<DocValue, Error> {
    match value {
        RawValue::Text(text) => Ok(DocValue::Bool(safe_parse_float(text) != 0.0)),
        RawValue::Integer(i) => Ok(DocValue::Bool(*i != 0)),
        RawValue::Real(r) => Ok(DocValue::Bool(*r != 0.0 && !r.is_nan())),
        RawValue::Null => Err(conversion_error(value, field)),
    }
}

fn to_doc_attachment(value: &RawValue, field: &Field) -> Result<DocValue, Error> {
    let text = match value {
        RawValue::Null => return Ok(DocValue::Null),
        RawValue::Text(text) if text.is_empty() => return Ok(DocValue::Null),
        RawValue::Integer(0) => return Ok(DocValue::Null),
        RawValue::Real(r) if *r == 0.0 || r.is_nan() => return Ok(DocValue::Null),
        RawValue::Text(text) => text,
        _ => return Err(conversion_error(value, field)),
    };
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(json) if is_json_truthy(&json) => Ok(DocValue::Attachment(json)),
        Ok(_) => Ok(DocValue::Null),
        Err(_) => Err(conversion_error(value, field)),
    }
}

fn is_json_truthy(json: &serde_json::Value) -> bool {
    match json {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0),
        serde_json::Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

fn conversion_error(value: &RawValue, field: &Field) -> Error {
    Error::Value(format!(
        "invalid doc conversion {value:?} found, field: {}.{}",
        field.schema_name.as_deref().unwrap_or_default(),
        field.fieldname
    ))
}

/// `safeParseFloat` in Books: JavaScript's `Number(text)`, with NaN read as 0.
pub fn safe_parse_float(text: &str) -> f64 {
    let parsed = js_number(text);
    if parsed.is_nan() { 0.0 } else { parsed }
}

/// `safeParseInt` in Books: `Math.trunc(Number(text))`, with NaN read as 0.
pub fn safe_parse_int(text: &str) -> f64 {
    safe_parse_float(text).trunc()
}

/// JavaScript's `Number(text)` for strings.
fn js_number(text: &str) -> f64 {
    let text = text.trim();
    if text.is_empty() {
        return 0.0;
    }
    let radix = |prefix: [&str; 2], radix: u32| {
        let digits = text
            .strip_prefix(prefix[0])
            .or_else(|| text.strip_prefix(prefix[1]))?;
        Some(u64::from_str_radix(digits, radix).map_or(f64::NAN, |n| n as f64))
    };
    if let Some(n) = radix(["0x", "0X"], 16)
        .or_else(|| radix(["0o", "0O"], 8))
        .or_else(|| radix(["0b", "0B"], 2))
    {
        return n;
    }
    match text.strip_prefix(['+', '-']).unwrap_or(text) {
        "Infinity" if text.starts_with('-') => f64::NEG_INFINITY,
        "Infinity" => f64::INFINITY,
        // Rust also reads "inf" and "nan", which JavaScript doesn't.
        rest if rest
            .bytes()
            .all(|b| b.is_ascii_digit() || b"eE.+-".contains(&b)) =>
        {
            text.parse().unwrap_or(f64::NAN)
        }
        _ => f64::NAN,
    }
}

/// Converts document values into values for `DatabaseCore` to write. A field the schema
/// doesn't have is an error, as it is in Books.
pub fn to_raw_value_map(
    schema_map: &SchemaMap,
    pesa: &MoneyMaker,
    schema_name: &str,
    doc_value_map: &DocValueMap,
) -> Result<FieldValueMap, Error> {
    let Some(schema) = schema_map.get(schema_name) else {
        return Err(Error::NotFound(format!("schema {schema_name} not found")));
    };

    let mut raw_value_map = FieldValueMap::new();
    for (fieldname, doc_value) in doc_value_map {
        let Some(field) = schema.field(fieldname) else {
            return Err(Error::Value(format!(
                "{schema_name} has no field {fieldname}"
            )));
        };

        let value = match doc_value {
            DocValue::Table(rows) => {
                let target = field.target.as_deref().unwrap_or_default();
                let rows = rows
                    .iter()
                    .map(|row| to_raw_value_map(schema_map, pesa, target, row))
                    .collect::<Result<_, _>>()?;
                FieldValue::Table(rows)
            }
            value => FieldValue::Value(to_raw_value(value, field, pesa)?),
        };
        raw_value_map.insert(fieldname.clone(), value);
    }
    Ok(raw_value_map)
}

/// Numbers come out as `RawValue::Real`: better-sqlite3 binds every JavaScript number as a
/// double, so an Int column stores an integer, a Float column a real, and a text column
/// (such as `SingleValue.value`) text like `"11.0"`.
pub fn to_raw_value(value: &DocValue, field: &Field, pesa: &MoneyMaker) -> Result<RawValue, Error> {
    match field.fieldtype {
        Some(FieldType::Currency) => to_raw_currency(value, field, pesa),
        Some(FieldType::Date) => to_raw_date(value, field),
        Some(FieldType::Datetime) => to_raw_date_time(value, field),
        Some(FieldType::Int) => to_raw_int(value, field),
        Some(FieldType::Float) => to_raw_float(value, field),
        Some(FieldType::Check) => to_raw_check(value, field),
        Some(FieldType::Link) => Ok(to_raw_link(value)),
        Some(FieldType::Attachment) => to_raw_attachment(value, field),
        _ => to_raw_string(value, field),
    }
}

fn to_raw_currency(value: &DocValue, field: &Field, pesa: &MoneyMaker) -> Result<RawValue, Error> {
    let money = match value {
        DocValue::Money(money) => money.clone(),
        DocValue::Null => pesa.zero(),
        DocValue::Number(n) => pesa.from_f64(*n)?,
        DocValue::Text(text) => pesa.parse(text)?,
        _ => return Err(raw_conversion_error(value, field)),
    };
    Ok(RawValue::Text(money.store()))
}

fn to_raw_int(value: &DocValue, field: &Field) -> Result<RawValue, Error> {
    match value {
        DocValue::Text(text) => Ok(RawValue::Real(safe_parse_int(text))),
        DocValue::Null => Ok(RawValue::Real(0.0)),
        DocValue::Number(n) => Ok(RawValue::Real(n.floor())),
        _ => Err(raw_conversion_error(value, field)),
    }
}

fn to_raw_float(value: &DocValue, field: &Field) -> Result<RawValue, Error> {
    match value {
        DocValue::Text(text) => Ok(RawValue::Real(safe_parse_float(text))),
        DocValue::Null => Ok(RawValue::Real(0.0)),
        DocValue::Number(n) => Ok(RawValue::Real(*n)),
        _ => Err(raw_conversion_error(value, field)),
    }
}

/// The date in the system time zone, as Luxon's `DateTime.fromJSDate(date).toISODate()`.
/// Text and numbers go through JavaScript's `new Date(value)` first; one that isn't a
/// valid date is stored as null.
fn to_raw_date(value: &DocValue, field: &Field) -> Result<RawValue, Error> {
    let timestamp = match value {
        DocValue::Null => return Ok(RawValue::Null),
        DocValue::Date(timestamp) => Some(*timestamp),
        DocValue::Text(text) => js_date_parse(text),
        DocValue::Number(ms) if ms.is_finite() => Timestamp::from_millisecond(*ms as i64).ok(),
        DocValue::Number(_) => None,
        _ => return Err(raw_conversion_error(value, field)),
    };
    Ok(match timestamp {
        Some(timestamp) => {
            RawValue::Text(timestamp.to_zoned(TimeZone::system()).date().to_string())
        }
        None => RawValue::Null,
    })
}

fn to_raw_date_time(value: &DocValue, field: &Field) -> Result<RawValue, Error> {
    match value {
        DocValue::Null => Ok(RawValue::Null),
        DocValue::Text(text) => Ok(RawValue::Text(text.clone())),
        DocValue::Date(timestamp) => Ok(RawValue::Text(to_iso_string(*timestamp))),
        _ => Err(raw_conversion_error(value, field)),
    }
}

fn to_raw_check(value: &DocValue, field: &Field) -> Result<RawValue, Error> {
    match value {
        DocValue::Number(n) => Ok(RawValue::Real(f64::from(*n != 0.0 && !n.is_nan()))),
        DocValue::Bool(b) => Ok(RawValue::Real(f64::from(*b))),
        _ => Err(raw_conversion_error(value, field)),
    }
}

fn to_raw_string(value: &DocValue, field: &Field) -> Result<RawValue, Error> {
    match value {
        DocValue::Null => Ok(RawValue::Null),
        DocValue::Text(text) => Ok(RawValue::Text(text.clone())),
        _ => Err(raw_conversion_error(value, field)),
    }
}

/// Anything but non-empty text is stored as null: Books checks `value?.length`, which
/// numbers, booleans and objects don't have.
fn to_raw_link(value: &DocValue) -> RawValue {
    match value {
        DocValue::Text(text) if !text.is_empty() => RawValue::Text(text.clone()),
        _ => RawValue::Null,
    }
}

fn to_raw_attachment(value: &DocValue, field: &Field) -> Result<RawValue, Error> {
    match value {
        DocValue::Null | DocValue::Bool(false) => Ok(RawValue::Null),
        DocValue::Text(text) if text.is_empty() => Ok(RawValue::Null),
        DocValue::Number(n) if *n == 0.0 || n.is_nan() => Ok(RawValue::Null),
        DocValue::Attachment(json)
            if ["name", "data", "type"]
                .iter()
                .all(|key| json.get(key).is_some_and(is_json_truthy)) =>
        {
            Ok(RawValue::Text(json.to_string()))
        }
        _ => Err(raw_conversion_error(value, field)),
    }
}

fn raw_conversion_error(value: &DocValue, field: &Field) -> Error {
    Error::Value(format!(
        "invalid raw conversion {value:?} found, field: {}.{}",
        field.schema_name.as_deref().unwrap_or_default(),
        field.fieldname
    ))
}

/// JavaScript's `Date.prototype.toISOString`: UTC with milliseconds.
pub fn to_iso_string(timestamp: Timestamp) -> String {
    timestamp.strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// JavaScript's `new Date(text)` for ISO text: a date alone is UTC midnight, a date and
/// time without an offset is local time.
fn js_date_parse(text: &str) -> Option<Timestamp> {
    if text.len() == 10
        && let Ok(date) = civil::Date::from_str(text)
    {
        return date.to_zoned(TimeZone::UTC).ok().map(|z| z.timestamp());
    }
    parse_iso(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_number_matches_javascript() {
        for (text, expected) in [
            ("2.0", 2.0),
            ("11.0", 11.0),
            (" 5 ", 5.0),
            ("", 0.0),
            ("1e3", 1000.0),
            (".5", 0.5),
            ("5.", 5.0),
            ("+5", 5.0),
            ("-5", -5.0),
            ("0x10", 16.0),
        ] {
            assert_eq!(js_number(text), expected, "{text:?}");
        }
        for text in ["abc", "inf", "nan", "1,000", "12px"] {
            assert!(js_number(text).is_nan(), "{text:?}");
        }
        assert_eq!(js_number("-Infinity"), f64::NEG_INFINITY);
    }

    #[test]
    fn safe_parse_reads_nan_as_zero() {
        assert_eq!(safe_parse_float("abc"), 0.0);
        assert_eq!(safe_parse_int("7.9"), 7.0);
        assert_eq!(safe_parse_int("-7.9"), -7.0);
    }

    #[test]
    fn iso_dates_parse_like_luxon() {
        assert_eq!(
            parse_iso("2025-01-15T05:12:01.000Z").unwrap().to_string(),
            "2025-01-15T05:12:01Z"
        );
        assert!(parse_iso("2025-01-15").is_some());
        assert!(parse_iso("2025-01-15T05:12:01").is_some());
        assert!(parse_iso("15/01/2025").is_none());
    }
}
