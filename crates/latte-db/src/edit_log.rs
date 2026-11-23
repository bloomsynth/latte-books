// The edit log, as Tally keeps one for India's audit trail rule (Companies (Accounts)
// Rules 2014, rule 3(1), in force from 1 April 2023): whenever a document is altered,
// renamed or deleted, the document as it was, with its rows, is kept in `latte_edit_log`,
// with who changed it and when. Not in Books, which ignores tables its schemas don't name.
//
// The log is written by the same writes that change the document, in their transaction, so
// a change that is undone leaves no entry. Triggers refuse to change or delete its rows, so
// it can only grow. It doesn't see changes made outside Latte, such as by Books.

use rusqlite::{Connection, params};
use serde_json::{Map, Number, Value};

use crate::Error;
use crate::core::{DatabaseCore, FieldValue, FieldValueMap, RawValue};

pub(crate) const CREATE: &str = "\
create table if not exists `latte_edit_log` (
    `id` integer primary key autoincrement,
    `schemaName` text not null,
    `name` text not null,
    `operation` text not null,
    `previous` text not null,
    `renamedFrom` text,
    `user` text not null,
    `timestamp` text not null,
    `device` text not null);
create index if not exists `latte_edit_log_document` on `latte_edit_log` (`schemaName`, `name`);
create trigger if not exists `latte_edit_log_no_update` before update on `latte_edit_log`
begin select raise(abort, 'The edit log can''t be changed.'); end;
create trigger if not exists `latte_edit_log_no_delete` before delete on `latte_edit_log`
begin select raise(abort, 'The edit log can''t be changed.'); end;";

/// Schemas whose rows the app keeps by itself, and which aren't logged: the ledgers are
/// posted from documents that are, settings are stored as single values, and the rest are
/// counters, queues and the record of migrations run.
const NOT_LOGGED: [&str; 8] = [
    "AccountingLedgerEntry",
    "StockLedgerEntry",
    "NumberSeries",
    "SingleValue",
    "PatchRun",
    "ERPNextSyncQueue",
    "FetchFromERPNextQueue",
    "IntegrationErrorLog",
];

/// What was done to a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditOperation {
    /// Its values or rows changed, including being submitted or cancelled.
    Update,
    Delete,
    Rename,
}

impl EditOperation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Update => "update",
            Self::Delete => "delete",
            Self::Rename => "rename",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "update" => Some(Self::Update),
            "delete" => Some(Self::Delete),
            "rename" => Some(Self::Rename),
            _ => None,
        }
    }
}

/// One change to a document: what was done, by whom and when, and the document as it was
/// just before, with its child tables.
#[derive(Debug, Clone, PartialEq)]
pub struct EditLogEntry<M> {
    pub id: i64,
    pub schema_name: String,
    /// The document's name after the change (before it, for a delete).
    pub name: String,
    pub operation: EditOperation,
    pub previous: M,
    /// For a rename, the name it had.
    pub renamed_from: Option<String>,
    pub user: String,
    /// When, as an ISO string, the time the change log gives the same write.
    pub timestamp: String,
    pub device: String,
}

/// An entry not yet written, made by the write that changes the document.
pub(crate) struct PendingEdit {
    pub schema_name: String,
    pub name: String,
    pub operation: EditOperation,
    pub previous: FieldValueMap,
    pub renamed_from: Option<String>,
}

impl DatabaseCore {
    /// Whether changes to a schema's documents are kept in the edit log: every document
    /// people make, but not settings, child rows, or what the app keeps by itself.
    pub fn is_edit_logged(&self, schema_name: &str) -> bool {
        self.schema_map.get(schema_name).is_some_and(|schema| {
            !schema.is_single && !schema.is_child && !NOT_LOGGED.contains(&schema_name)
        })
    }

    /// Sets who the edit log says made the changes from now on.
    pub fn set_user(&mut self, user: impl Into<String>) {
        self.user = user.into();
    }

    /// The document as stored, with its child tables, if it's logged and exists. Read
    /// before a write changes it.
    pub(crate) fn edit_log_snapshot(
        &self,
        schema_name: &str,
        name: &str,
    ) -> Result<Option<FieldValueMap>, Error> {
        if !self.is_edit_logged(schema_name) || name.is_empty() {
            return Ok(None);
        }
        let values = self.get(schema_name, name, None)?;
        Ok(values.contains_key("name").then_some(values))
    }

    pub(crate) fn write_edits(
        &self,
        edits: &[PendingEdit],
        timestamp: &str,
        device: &str,
    ) -> Result<(), Error> {
        write_edits(&self.conn, edits, &self.user, timestamp, device)
    }

    /// The changes made to a document, oldest first, back through its renames, and
    /// starting after the last time a document of the same name was deleted.
    pub fn edit_log(
        &self,
        schema_name: &str,
        name: &str,
    ) -> Result<Vec<EditLogEntry<FieldValueMap>>, Error> {
        // While the document exists, any delete under its name was of an earlier one.
        let exists = self.exists(schema_name, Some(name))?;
        let mut entries = Vec::new();
        let mut name = name.to_owned();
        let mut before = i64::MAX;
        // Each pass reads the entries made under one name, newest first, and goes on to
        // the name before if it was renamed to this one.
        loop {
            let mut statement = self.conn.prepare_cached(
                "select `id`, `schemaName`, `name`, `operation`, `previous`, `renamedFrom`, \
                 `user`, `timestamp`, `device` from `latte_edit_log` \
                 where `schemaName` = ? and `name` = ? and `id` < ? order by `id` desc",
            )?;
            let rows = statement.query_map(params![schema_name, name, before], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                ))
            })?;

            let mut renamed_from = None;
            for row in rows {
                let (id, schema_name, name, operation, previous, from, user, timestamp, device) =
                    row?;
                let Some(operation) = EditOperation::parse(&operation) else {
                    continue;
                };
                // A delete ends an earlier document that had this name.
                if operation == EditOperation::Delete && (exists || !entries.is_empty()) {
                    break;
                }
                let previous: Value = serde_json::from_str(&previous)
                    .map_err(|e| Error::Value(format!("edit log entry {id}: {e}")))?;
                let previous = from_json(&previous)?;
                let is_rename_to_here = operation == EditOperation::Rename;
                entries.push(EditLogEntry {
                    id,
                    schema_name,
                    name,
                    operation,
                    previous,
                    renamed_from: from.clone(),
                    user,
                    timestamp,
                    device,
                });
                if is_rename_to_here {
                    renamed_from = from.map(|from| (from, id));
                    break;
                }
            }
            match renamed_from {
                Some((from, id)) if from != name => {
                    name = from;
                    before = id;
                }
                _ => break,
            }
        }
        entries.reverse();
        Ok(entries)
    }
}

fn write_edits(
    conn: &Connection,
    edits: &[PendingEdit],
    user: &str,
    timestamp: &str,
    device: &str,
) -> Result<(), Error> {
    let mut statement = conn.prepare_cached(
        "insert into `latte_edit_log` (`schemaName`, `name`, `operation`, `previous`, \
         `renamedFrom`, `user`, `timestamp`, `device`) values (?, ?, ?, ?, ?, ?, ?, ?)",
    )?;
    for edit in edits {
        statement.execute(params![
            edit.schema_name,
            edit.name,
            edit.operation.as_str(),
            to_json(&edit.previous).to_string(),
            edit.renamed_from,
            user,
            timestamp,
            device,
        ])?;
    }
    Ok(())
}

/// Whether two stored states of a document differ in more than when and by whom they were
/// last modified.
pub(crate) fn differs(before: &FieldValueMap, after: &FieldValueMap) -> bool {
    let skip = |key: &str| matches!(key, "modified" | "modifiedBy");
    let keys = before.keys().chain(after.keys()).filter(|k| !skip(k));
    keys.into_iter()
        .any(|key| before.get(key) != after.get(key))
}

fn to_json(values: &FieldValueMap) -> Value {
    let mut map = Map::new();
    for (key, value) in values {
        let value = match value {
            FieldValue::Value(RawValue::Null) => Value::Null,
            FieldValue::Value(RawValue::Integer(i)) => Value::from(*i),
            FieldValue::Value(RawValue::Real(r)) => {
                Number::from_f64(*r).map_or(Value::Null, Value::Number)
            }
            FieldValue::Value(RawValue::Text(t)) => Value::String(t.clone()),
            FieldValue::Table(rows) => Value::Array(rows.iter().map(to_json).collect()),
        };
        map.insert(key.clone(), value);
    }
    Value::Object(map)
}

fn from_json(value: &Value) -> Result<FieldValueMap, Error> {
    let Value::Object(map) = value else {
        return Err(Error::Value("an edit log entry isn't an object".into()));
    };
    let mut values = FieldValueMap::new();
    for (key, value) in map {
        let value = match value {
            Value::Null => FieldValue::Value(RawValue::Null),
            Value::Bool(b) => FieldValue::Value(RawValue::Integer(i64::from(*b))),
            Value::Number(n) => match n.as_i64() {
                Some(i) if !n.is_f64() => FieldValue::Value(RawValue::Integer(i)),
                _ => FieldValue::Value(RawValue::Real(n.as_f64().unwrap_or_default())),
            },
            Value::String(s) => FieldValue::Value(RawValue::Text(s.clone())),
            Value::Array(rows) => {
                FieldValue::Table(rows.iter().map(from_json).collect::<Result<_, _>>()?)
            }
            Value::Object(_) => {
                return Err(Error::Value(format!(
                    "{key} in an edit log entry is an object"
                )));
            }
        };
        values.insert(key.clone(), value);
    }
    Ok(values)
}
