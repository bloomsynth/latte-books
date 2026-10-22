// Port of fyo/model/helpers.ts @ v0.36.0, with the JavaScript truthiness and string
// conversion that Doc relies on.

use latte_db::DocValue;
use latte_db::pesa::MoneyMaker;
use latte_schema::{Field, FieldType};

use crate::Error;
use crate::fyo::model::doc::{Doc, FieldRef};
use crate::fyo::model::types::{DocContext, Formula};

/// `areDocValuesEqual` for values that aren't tables. Text and numbers are compared with
/// `===`, money with pesa's `eq`, and everything else as lodash's `isEqual` would. Dates
/// are JavaScript objects compared with `===`, so two are never equal.
pub fn are_doc_values_equal(one: &DocValue, two: Option<&DocValue>) -> bool {
    match (one, two) {
        (DocValue::Text(a), Some(DocValue::Text(b))) => a == b,
        (DocValue::Number(a), Some(DocValue::Number(b))) => a == b,
        (DocValue::Text(_) | DocValue::Number(_) | DocValue::Date(_), _) => false,
        (DocValue::Money(money), Some(DocValue::Money(other))) => money.eq(other).unwrap_or(false),
        (DocValue::Money(money), Some(DocValue::Number(n))) => money.eq(*n).unwrap_or(false),
        (DocValue::Money(money), Some(DocValue::Text(t))) => money.eq(t.as_str()).unwrap_or(false),
        (DocValue::Money(_), _) => false,
        (one, Some(two)) => one == two,
        (_, None) => false,
    }
}

/// `getPreDefaultValues`, for fields other than tables (which start empty).
pub fn get_pre_default_values(fieldtype: Option<FieldType>, pesa: &MoneyMaker) -> DocValue {
    match fieldtype {
        Some(FieldType::Currency) => DocValue::Money(pesa.zero()),
        Some(FieldType::Int | FieldType::Float) => DocValue::Number(0.0),
        _ => DocValue::Null,
    }
}

/// `getMissingMandatoryMessage`: the labels of the required fields `ctx.doc` lacks, with
/// the row they are in if it is a child row.
pub fn get_missing_mandatory_message(ctx: &DocContext<'_>) -> String {
    let doc = ctx.doc;
    let message = get_mandatory(ctx)
        .into_iter()
        .filter(|f| match doc.field(&f.fieldname) {
            FieldRef::Table(rows) => rows.is_empty(),
            FieldRef::Undefined | FieldRef::Value(DocValue::Null) => true,
            FieldRef::Value(DocValue::Text(text)) => text.is_empty(),
            FieldRef::Value(_) => false,
        })
        .map(|f| f.label.as_deref().unwrap_or(&f.fieldname))
        .collect::<Vec<_>>()
        .join(", ");

    if !message.is_empty()
        && doc.schema().is_child
        && let Some(parent) = ctx.parent
        && let Some(DocValue::Text(parent_fieldname)) = doc.get("parentFieldname")
    {
        let label = parent
            .schema()
            .field(parent_fieldname)
            .and_then(|f| f.label.as_deref())
            .unwrap_or("undefined");
        let idx = match doc.get("idx") {
            Some(DocValue::Number(idx)) => *idx,
            _ => 0.0,
        };
        return format!("{label} Row {}: {message}", js_number_string(idx + 1.0));
    }
    message
}

/// `getMandatory`: a field that is both required in the schema and by the model is listed
/// twice, as in Books.
fn get_mandatory<'a>(ctx: &DocContext<'a>) -> Vec<&'a Field> {
    let model = ctx.doc.model();
    let mut fields = Vec::new();
    for field in &ctx.doc.schema().fields {
        if field.required {
            fields.push(field);
        }
        if model.required(&field.fieldname, ctx) {
            fields.push(field);
        }
    }
    fields
}

/// `shouldApplyFormula`.
pub fn should_apply_formula(
    field: &Field,
    formula: &Formula,
    doc: &Doc,
    fieldname: Option<&str>,
) -> bool {
    if field.read_only {
        return true;
    }

    let Some(depends_on) = formula.depends_on else {
        return true;
    };
    if depends_on.is_empty() {
        return false;
    }

    if fieldname.is_some_and(|f| depends_on.contains(&f)) {
        return true;
    }

    if doc.is_syncing() {
        return should_apply_formula_pre_sync(&field.fieldname, depends_on, doc);
    }

    matches!(
        doc.field(&field.fieldname),
        FieldRef::Undefined | FieldRef::Value(DocValue::Null)
    )
}

fn should_apply_formula_pre_sync(fieldname: &str, depends_on: &[&str], doc: &Doc) -> bool {
    if is_doc_value_truthy(doc.field(fieldname)) {
        return false;
    }

    depends_on.iter().any(|d| is_doc_value_truthy(doc.field(d)))
}

/// `isDocValueTruthy`: money that isn't zero, a table with rows, or a truthy value.
pub fn is_doc_value_truthy(value: FieldRef<'_>) -> bool {
    match value {
        FieldRef::Value(DocValue::Money(money)) => !money.is_zero(),
        FieldRef::Table(rows) => !rows.is_empty(),
        FieldRef::Value(value) => is_truthy(Some(value)),
        FieldRef::Undefined => false,
    }
}

/// JavaScript's `!!value`. Money, dates and attachments are objects, so always truthy.
pub fn is_truthy(value: Option<&DocValue>) -> bool {
    match value {
        None | Some(DocValue::Null) => false,
        Some(DocValue::Bool(b)) => *b,
        Some(DocValue::Number(n)) => *n != 0.0 && !n.is_nan(),
        Some(DocValue::Text(t)) => !t.is_empty(),
        Some(
            DocValue::Money(_) | DocValue::Date(_) | DocValue::Attachment(_) | DocValue::Table(_),
        ) => true,
    }
}

/// `setChildDocIdx`.
pub fn set_child_doc_idx(child_docs: &mut [Doc]) {
    for (idx, child) in child_docs.iter_mut().enumerate() {
        child.set_value("idx", DocValue::Number(idx as f64));
    }
}

/// `getFormulaSequence`: the formulas' fieldnames, with each one after what it depends on.
pub fn get_formula_sequence(formulas: &[Formula]) -> Vec<&'static str> {
    let has_deps = |f: &Formula| f.depends_on.is_some_and(|d| !d.is_empty());
    let mut keyset: Vec<&'static str> = formulas
        .iter()
        .filter(|f| !has_deps(f))
        .map(|f| f.fieldname)
        .collect();
    for formula in formulas.iter().filter(|f| has_deps(f)) {
        let deps = formula.depends_on.unwrap_or_default();
        for d in deps.iter().copied().chain([formula.fieldname]) {
            if !keyset.contains(&d) {
                keyset.push(d);
            }
        }
    }
    keyset.retain(|k| formulas.iter().any(|f| f.fieldname == *k));
    keyset
}

/// JavaScript's `String(value)` for a document value, as Books puts values in messages.
pub fn js_string(value: Option<&DocValue>) -> String {
    match value {
        None => "undefined".into(),
        Some(DocValue::Null) => "null".into(),
        Some(DocValue::Text(t)) => t.clone(),
        Some(DocValue::Number(n)) => js_number_string(*n),
        Some(DocValue::Bool(b)) => b.to_string(),
        Some(DocValue::Money(m)) => m.to_string(),
        Some(DocValue::Date(d)) => latte_db::to_iso_string(*d),
        Some(DocValue::Attachment(_)) => "[object Object]".into(),
        Some(DocValue::Table(_)) => "[object Object]".into(),
    }
}

/// JavaScript's `String(number)` for the numbers documents hold.
pub fn js_number_string(n: f64) -> String {
    if n.is_nan() {
        "NaN".into()
    } else if n.is_infinite() {
        if n > 0.0 { "Infinity" } else { "-Infinity" }.into()
    } else if n == n.trunc() && n.abs() < 1e21 {
        format!("{}", n as i128)
    } else {
        format!("{n}")
    }
}

/// `fyo.pesa(value)` for the values `getSum` and the defaults pass it.
pub(crate) fn to_money(
    value: &DocValue,
    pesa: &MoneyMaker,
) -> Result<latte_db::pesa::Money, Error> {
    Ok(match value {
        DocValue::Money(money) => money.clone(),
        DocValue::Number(n) => pesa.from_f64(*n)?,
        DocValue::Text(t) => pesa.parse(t)?,
        other => {
            return Err(Error::Pesa(latte_db::pesa::PesaError::new(format!(
                "invalid input '{}'",
                js_string(Some(other))
            ))));
        }
    })
}
