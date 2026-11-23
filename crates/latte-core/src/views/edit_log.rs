// A document's edit history, read from the edit log (latte-db's edit_log.rs), as Tally's
// Edit Log shows a voucher's versions: when it was made and by whom, then each change, with
// the values it changed from and to. Not in Books.

use jiff::Timestamp;
use latte_db::{DocValue, DocValueMap, EditLogEntry, EditOperation};
use latte_schema::{Field, FieldType, Schema};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::format::Formatter;
use crate::fyo::model::helpers::is_truthy;
use crate::views::field;

/// What a change did to the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditKind {
    /// Its values or rows changed.
    Altered,
    Submitted,
    Cancelled,
    Renamed {
        from: String,
    },
    Deleted,
}

impl EditKind {
    pub fn label(&self) -> String {
        match self {
            Self::Altered => "Altered".to_owned(),
            Self::Submitted => "Submitted".to_owned(),
            Self::Cancelled => "Cancelled".to_owned(),
            Self::Renamed { from } => format!("Renamed from {from}"),
            Self::Deleted => "Deleted".to_owned(),
        }
    }
}

/// One value a change changed, as the form shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldChange {
    /// The field's label, after its table's and row number for a row's field.
    pub label: String,
    /// Empty when the value was empty, or for a row that was added.
    pub before: String,
    /// Empty when the value was emptied, or for a row that was removed.
    pub after: String,
}

/// One change to the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditEvent {
    pub kind: EditKind,
    pub user: String,
    /// When, in the computer's time zone.
    pub when: String,
    pub changes: Vec<FieldChange>,
}

/// A document's history, oldest change first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditHistory {
    pub created_by: String,
    pub created: String,
    pub events: Vec<EditEvent>,
}

/// Fields every document has, which a change doesn't show.
const META: [&str; 7] = [
    "name",
    "created",
    "createdBy",
    "modified",
    "modifiedBy",
    "submitted",
    "cancelled",
];

/// Fields every row has, which a change doesn't show.
const ROW_META: [&str; 5] = [
    "name",
    "idx",
    "parent",
    "parentFieldname",
    "parentSchemaName",
];

/// The history of a document, deleted or not: what each version was changed from and to.
pub fn history(
    fyo: &Fyo,
    formatter: &Formatter,
    schema_name: &str,
    name: &str,
) -> Result<EditHistory, Error> {
    let Some(schema) = fyo.schema(schema_name).cloned() else {
        return Err(Error::NotFound(format!("{schema_name} not found")));
    };
    let entries = fyo.db.edit_log(schema_name, name)?;
    let current = match fyo.db.get_doc(schema_name, name) {
        Ok(values) => Some(values),
        Err(latte_db::Error::NotFound(_)) => None,
        Err(e) => return Err(e.into()),
    };

    // The versions in order: each entry's document as it was before the change, then the
    // document as it is now, if it is still there.
    let mut versions: Vec<&DocValueMap> = entries.iter().map(|e| &e.previous).collect();
    versions.extend(current.as_ref());
    let Some(first) = versions.first() else {
        return Err(Error::NotFound(format!(
            "{} {name} not found",
            schema.label
        )));
    };

    let events = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let after = versions.get(i + 1).copied();
            event(fyo, formatter, &schema, entry, after)
        })
        .collect();
    Ok(EditHistory {
        created_by: text(first.get("createdBy")),
        created: when(formatter, first.get("created")),
        events,
    })
}

fn event(
    fyo: &Fyo,
    formatter: &Formatter,
    schema: &Schema,
    entry: &EditLogEntry<DocValueMap>,
    after: Option<&DocValueMap>,
) -> EditEvent {
    let before = &entry.previous;
    let flag = |values: &DocValueMap, fieldname| is_truthy(values.get(fieldname));
    let kind = match (entry.operation, after) {
        (EditOperation::Delete, _) | (_, None) => EditKind::Deleted,
        (EditOperation::Rename, _) => EditKind::Renamed {
            from: entry.renamed_from.clone().unwrap_or_default(),
        },
        (EditOperation::Update, Some(after)) => {
            if !flag(before, "cancelled") && flag(after, "cancelled") {
                EditKind::Cancelled
            } else if !flag(before, "submitted") && flag(after, "submitted") {
                EditKind::Submitted
            } else {
                EditKind::Altered
            }
        }
    };
    let changes = match (&kind, after) {
        (EditKind::Altered | EditKind::Submitted | EditKind::Cancelled, Some(after)) => {
            changes(fyo, formatter, schema, before, after)
        }
        _ => Vec::new(),
    };
    EditEvent {
        kind,
        user: entry.user.clone(),
        when: match entry.timestamp.parse::<Timestamp>() {
            Ok(timestamp) => formatter.datetime(timestamp),
            Err(_) => entry.timestamp.clone(),
        },
        changes,
    }
}

/// The values that differ between two versions, in the form's order: the document's
/// fields, then each table's rows by position.
fn changes(
    fyo: &Fyo,
    formatter: &Formatter,
    schema: &Schema,
    before: &DocValueMap,
    after: &DocValueMap,
) -> Vec<FieldChange> {
    let mut changes = Vec::new();
    for f in shown_fields(schema, &META) {
        if f.fieldtype == Some(FieldType::Table) {
            let Some(row_schema) = f.target.as_deref().and_then(|t| fyo.schema(t)) else {
                continue;
            };
            let rows = |values: &DocValueMap| match values.get(&f.fieldname) {
                Some(DocValue::Table(rows)) => rows.clone(),
                _ => Vec::new(),
            };
            let (old_rows, new_rows) = (rows(before), rows(after));
            for i in 0..old_rows.len().max(new_rows.len()) {
                let label = format!("{} row {}", label_of(f), i + 1);
                match (old_rows.get(i), new_rows.get(i)) {
                    (Some(old), Some(new)) => {
                        for column in shown_fields(row_schema, &ROW_META) {
                            let (from, to) = (
                                field::display(formatter, column, old.get(&column.fieldname)),
                                field::display(formatter, column, new.get(&column.fieldname)),
                            );
                            if from != to {
                                changes.push(FieldChange {
                                    label: format!("{label}: {}", label_of(column)),
                                    before: from,
                                    after: to,
                                });
                            }
                        }
                    }
                    (None, Some(new)) => changes.push(FieldChange {
                        label: format!("{label} added"),
                        before: String::new(),
                        after: row_summary(formatter, row_schema, new),
                    }),
                    (Some(old), None) => changes.push(FieldChange {
                        label: format!("{label} removed"),
                        before: row_summary(formatter, row_schema, old),
                        after: String::new(),
                    }),
                    (None, None) => {}
                }
            }
            continue;
        }

        let (from, to) = (
            field::display(formatter, f, before.get(&f.fieldname)),
            field::display(formatter, f, after.get(&f.fieldname)),
        );
        if from != to {
            changes.push(FieldChange {
                label: label_of(f).to_owned(),
                before: from,
                after: to,
            });
        }
    }
    changes
}

/// A schema's stored fields, less those every document or row has.
fn shown_fields<'a>(schema: &'a Schema, skip: &'a [&str]) -> impl Iterator<Item = &'a Field> {
    schema.fields.iter().filter(move |f| {
        !f.computed
            && f.fieldtype != Some(FieldType::Button)
            && !skip.contains(&f.fieldname.as_str())
    })
}

/// A row's filled-in values, as "Cash, ₹ 250.00".
fn row_summary(formatter: &Formatter, schema: &Schema, row: &DocValueMap) -> String {
    shown_fields(schema, &ROW_META)
        .map(|f| field::display(formatter, f, row.get(&f.fieldname)))
        .filter(|value| !value.is_empty() && !is_zero(value))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Whether a shown amount is nothing, such as "₹ 0.00", which a row summary leaves out.
fn is_zero(value: &str) -> bool {
    value.chars().any(|c| c.is_ascii_digit())
        && value
            .chars()
            .filter(|c| c.is_ascii_digit())
            .all(|c| c == '0')
}

fn label_of(f: &Field) -> &str {
    f.label.as_deref().unwrap_or(&f.fieldname)
}

fn text(value: Option<&DocValue>) -> String {
    match value {
        Some(DocValue::Text(text)) => text.clone(),
        _ => String::new(),
    }
}

fn when(formatter: &Formatter, value: Option<&DocValue>) -> String {
    match value {
        Some(DocValue::Date(timestamp)) => formatter.datetime(*timestamp),
        _ => String::new(),
    }
}
