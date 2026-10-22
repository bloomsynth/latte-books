// The change log (docs/PLAN.md §7): every row latte-db writes is recorded in
// `latte_changes`, so that sync can later send what changed. Not in Books, which ignores
// tables its schemas don't name.

use rusqlite::{Connection, params};

use crate::Error;

pub(crate) const CREATE_TABLE: &str = "create table if not exists `latte_changes` (\
    `id` integer primary key autoincrement, \
    `schemaName` text not null, \
    `name` text not null, \
    `operation` text not null, \
    `fields` text, \
    `renamedFrom` text, \
    `timestamp` text not null, \
    `device` text not null)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Operation {
    Insert,
    Update,
    Delete,
    Rename,
}

impl Operation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Insert => "insert",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::Rename => "rename",
        }
    }
}

/// One row written. For a single, `name` is the schema name and `fields` the values set
/// or deleted.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Change {
    pub schema_name: String,
    pub name: String,
    pub operation: Operation,
    /// The columns written, for an insert or update.
    pub fields: Option<Vec<String>>,
    pub renamed_from: Option<String>,
}

impl Change {
    pub fn new(schema_name: &str, name: &str, operation: Operation) -> Self {
        Self {
            schema_name: schema_name.to_owned(),
            name: name.to_owned(),
            operation,
            fields: None,
            renamed_from: None,
        }
    }

    pub fn with_fields(mut self, fields: Vec<String>) -> Self {
        self.fields = Some(fields);
        self
    }
}

/// The changes of one write, which share its timestamp.
pub(crate) struct ChangeLog {
    pub device: String,
    pub timestamp: String,
    pub changes: Vec<Change>,
}

impl ChangeLog {
    pub fn push(&mut self, change: Change) {
        self.changes.push(change);
    }

    pub fn write(&self, conn: &Connection) -> Result<(), Error> {
        let mut statement = conn.prepare_cached(
            "insert into `latte_changes` (`schemaName`, `name`, `operation`, `fields`, \
             `renamedFrom`, `timestamp`, `device`) values (?, ?, ?, ?, ?, ?, ?)",
        )?;
        for change in &self.changes {
            let fields = change
                .fields
                .as_ref()
                .map(|f| serde_json::to_string(f).expect("a list of strings serializes"));
            statement.execute(params![
                change.schema_name,
                change.name,
                change.operation.as_str(),
                fields,
                change.renamed_from,
                self.timestamp,
                self.device,
            ])?;
        }
        Ok(())
    }
}
