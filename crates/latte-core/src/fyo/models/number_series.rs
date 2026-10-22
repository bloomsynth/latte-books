// Port of fyo/models/NumberSeries.ts @ v0.36.0.

use latte_db::DocValue;

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::helpers::{js_number_string, js_string};
use crate::fyo::model::types::{DocContext, Model};

pub struct NumberSeries;

impl Model for NumberSeries {
    fn validate_field(
        &self,
        fieldname: &str,
        value: &DocValue,
        _ctx: &DocContext<'_>,
    ) -> Result<(), Error> {
        if fieldname != "name" {
            return Ok(());
        }
        let DocValue::Text(value) = value else {
            return Ok(());
        };

        // `/[/\=\?\&\%]/`, in which `\=` is just `=`.
        if value.contains(['/', '=', '?', '&', '%']) {
            return Err(Error::Validation(
                "The following characters cannot be used /, ?, &, =, % in a Number Series name."
                    .into(),
            ));
        }
        Ok(())
    }

    fn read_only(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<bool> {
        match fieldname {
            "referenceType" | "padZeros" | "start" => Some(ctx.doc.inserted()),
            _ => None,
        }
    }
}

fn number(doc: &Doc, fieldname: &str) -> f64 {
    match doc.get(fieldname) {
        Some(DocValue::Number(n)) => *n,
        _ => 0.0,
    }
}

/// `setCurrent`: the start if nothing has been numbered yet, otherwise one more.
fn set_current(doc: &mut Doc) {
    let current = number(doc, "current");
    let current = if current == 0.0 || current.is_nan() {
        doc.get("start").cloned().unwrap_or(DocValue::Null)
    } else {
        DocValue::Number(current + 1.0)
    };
    doc.set_value("current", current);
}

/// `next`: takes the next number of the series for a `schema_name` document, saves the
/// series and returns the name.
pub fn next(doc: &mut Doc, fyo: &mut Fyo, schema_name: &str) -> Result<String, Error> {
    set_current(doc);
    if check_if_current_exists(doc, fyo, schema_name)? {
        let current = number(doc, "current");
        doc.set_value("current", DocValue::Number(current + 1.0));
    }

    doc.sync(fyo)?;
    Ok(get_padded_name(doc, number(doc, "current")))
}

/// `checkIfCurrentExists`.
fn check_if_current_exists(doc: &Doc, fyo: &Fyo, schema_name: &str) -> Result<bool, Error> {
    if schema_name.is_empty() {
        return Ok(true);
    }

    let name = get_padded_name(doc, number(doc, "current"));
    Ok(fyo.db.core().exists(schema_name, Some(&name))?)
}

/// `getPaddedName`: the prefix, then the number padded with zeros to `padZeros` digits.
pub fn get_padded_name(doc: &Doc, next: f64) -> String {
    let prefix = js_string(doc.get("name"));
    let pad_zeros = match doc.get("padZeros") {
        Some(DocValue::Number(n)) => n.max(0.0) as usize,
        _ => 4,
    };
    format!("{prefix}{:0>pad_zeros$}", js_number_string(next))
}
