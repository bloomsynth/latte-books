// Port of fyo/core/converter.ts @ v0.36.0: database values to document values. The other
// direction comes with writing.

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
