// Port of the write half of backend/database/core.ts @ v0.36.0: insert, update, delete,
// deleteAll, rename, the single-value helpers and #createTable, plus getLastInserted from
// bespoke.ts.
//
// Differences from Books:
// - Each write runs in one transaction, so a failed write leaves nothing behind. Books runs
//   each statement on its own.
// - Each row written is recorded in `latte_changes` (see changes.rs).
// - Books doesn't wait for a new single value to be inserted; this does.

use jiff::Timestamp;
use latte_schema::FieldType;
use rusqlite::ToSql;

use crate::Error;
use crate::changes::{self, Change, ChangeLog, Operation};
use crate::converter::to_iso_string;
use crate::core::{
    DatabaseCore, FieldValue, FieldValueMap, Filter, FilterValue, QueryFilter, RawValue,
    apply_filters, quote, table_fields,
};

/// `SYSTEM` in Books: the user of values written by the database itself.
pub const SYSTEM: &str = "__SYSTEM__";

/// `getRandomString` in Books: six random base-36 digits, a dash, and the time in
/// milliseconds in base 36. Names documents and child rows that have no other name.
pub fn get_random_string() -> String {
    let random: String = (0..6)
        .map(|_| char::from_digit(fastrand::u32(0..36), 36).expect("below 36"))
        .collect();
    let now = Timestamp::now().as_millisecond().max(0) as u64;
    format!("{random}-{}", to_base_36(now))
}

fn to_base_36(mut n: u64) -> String {
    let mut digits = Vec::new();
    loop {
        digits.push(char::from_digit((n % 36) as u32, 36).expect("below 36"));
        n /= 36;
        if n == 0 {
            break;
        }
    }
    digits.iter().rev().collect()
}

/// The SQLite column type of a field type, `sqliteTypeMap` in Books. Table and Button
/// fields have no column.
pub fn sqlite_type(fieldtype: FieldType) -> Option<&'static str> {
    match fieldtype {
        FieldType::Int => Some("integer"),
        FieldType::Float => Some("float"),
        FieldType::Check => Some("boolean"),
        FieldType::Date => Some("date"),
        FieldType::Datetime => Some("datetime"),
        FieldType::Table | FieldType::Button => None,
        _ => Some("text"),
    }
}

impl DatabaseCore {
    /// Turns the change log on, creating its table if needed. Writing needs it.
    pub fn enable_change_log(&mut self, device: impl Into<String>) -> Result<(), Error> {
        self.conn.execute(changes::CREATE_TABLE, [])?;
        self.device = Some(device.into());
        Ok(())
    }

    /// Port of `#createTable`, as knex writes it for SQLite. Migrating a file isn't ported
    /// yet; this is for new files.
    pub fn create_table(&self, schema_name: &str) -> Result<(), Error> {
        let schema = self.schema(schema_name)?;
        let mut columns = Vec::new();
        let mut foreign_keys = Vec::new();
        for field in schema.fields.iter().filter(|f| !f.computed) {
            let Some(column_type) = field.fieldtype.and_then(sqlite_type) else {
                continue;
            };
            let mut column = format!("{} {column_type}", quote(&field.fieldname));
            if field.required {
                column.push_str(" not null");
            }
            if let Some(default) = &field.default {
                column.push_str(&format!(" default {}", format_default(default)));
            }
            columns.push(column);

            if field.fieldtype == Some(FieldType::Link)
                && let Some(target) = &field.target
            {
                let target = self.schema(target)?;
                foreign_keys.push(format!(
                    "foreign key({}) references {}(`name`) on delete RESTRICT on update CASCADE",
                    quote(&field.fieldname),
                    quote(&target.name)
                ));
            }
        }

        let mut definitions = columns;
        definitions.extend(foreign_keys);
        if schema.field("name").is_some() {
            definitions.push("primary key (`name`)".into());
        }
        let sql = format!(
            "create table {} ({})",
            quote(schema_name),
            definitions.join(", ")
        );
        self.conn.execute(&sql, [])?;
        Ok(())
    }

    /// Inserts a document and its child rows. Fills in what Books does on the way: a random
    /// name when there is none, and each child's parent fields and `idx`. The meta values
    /// (`created`, `modified` and their users) are the caller's, except for singles.
    pub fn insert(
        &mut self,
        schema_name: &str,
        field_value_map: &mut FieldValueMap,
    ) -> Result<(), Error> {
        self.write(|core, log| {
            if core.schema(schema_name)?.is_single {
                core.update_single_values(schema_name, field_value_map, log)?;
            } else {
                core.insert_one(schema_name, field_value_map, log)?;
            }
            core.insert_or_update_children(schema_name, field_value_map, false, log)
        })
    }

    /// Updates the values given and replaces the child tables given: rows that exist are
    /// updated, new ones inserted, and the others deleted.
    pub fn update(
        &mut self,
        schema_name: &str,
        field_value_map: &mut FieldValueMap,
    ) -> Result<(), Error> {
        self.write(|core, log| {
            if core.schema(schema_name)?.is_single {
                core.update_single_values(schema_name, field_value_map, log)?;
            } else {
                core.update_one(schema_name, field_value_map, log)?;
            }
            core.insert_or_update_children(schema_name, field_value_map, true, log)
        })
    }

    /// Deletes a document and its child rows. For a single, `name` is the fieldname of the
    /// one value to delete, as in Books.
    pub fn delete(&mut self, schema_name: &str, name: &str) -> Result<(), Error> {
        self.write(|core, log| {
            if core.schema(schema_name)?.is_single {
                let deleted = core.conn.execute(
                    "delete from `SingleValue` where `parent` = ? and `fieldname` = ?",
                    [schema_name, name],
                )?;
                if deleted > 0 {
                    log.push(
                        Change::new(schema_name, schema_name, Operation::Delete)
                            .with_fields(vec![name.to_owned()]),
                    );
                }
                return Ok(());
            }

            let deleted = core.conn.execute(
                &format!("delete from {} where `name` = ?", quote(schema_name)),
                [name],
            )?;
            if deleted > 0 {
                log.push(Change::new(schema_name, name, Operation::Delete));
            }

            for (_, target) in table_fields(core.schema(schema_name)?) {
                let filters: QueryFilter = [("parent".to_owned(), name.into())].into();
                core.delete_where(target, &filters, log)?;
            }
            Ok(())
        })
    }

    /// Deletes the rows that match `filters`, without their child rows, and returns how many
    /// there were.
    pub fn delete_all(&mut self, schema_name: &str, filters: &QueryFilter) -> Result<usize, Error> {
        self.write(|core, log| core.delete_where(schema_name, filters, log))
    }

    /// Renames a document. As in Books, child rows keep the old parent name; links follow
    /// through the foreign keys' `on update CASCADE`, and aren't logged.
    pub fn rename(
        &mut self,
        schema_name: &str,
        old_name: &str,
        new_name: &str,
    ) -> Result<(), Error> {
        self.write(|core, log| {
            let renamed = core.conn.execute(
                &format!(
                    "update {} set `name` = ? where `name` = ?",
                    quote(schema_name)
                ),
                [new_name, old_name],
            )?;
            if renamed > 0 {
                let mut change = Change::new(schema_name, new_name, Operation::Rename);
                change.renamed_from = Some(old_name.to_owned());
                log.push(change);
            }
            Ok(())
        })
    }

    /// `getLastInserted` in Books' bespoke queries: the largest name read as a number, for
    /// schemas named by `autoincrement`.
    pub fn get_last_inserted(&self, schema_name: &str) -> Result<i64, Error> {
        let rows = self.query(
            &format!(
                "select cast(name as int) as num from {} order by num desc limit 1",
                quote(schema_name)
            ),
            &[],
        )?;
        Ok(match rows.first().and_then(|row| row.get("num")) {
            Some(RawValue::Integer(num)) => *num,
            Some(RawValue::Real(num)) => *num as i64,
            _ => 0,
        })
    }

    /// Runs one write in a transaction and logs its changes.
    fn write<T>(
        &mut self,
        f: impl FnOnce(&Self, &mut ChangeLog) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let Some(device) = self.device.clone() else {
            return Err(Error::Value(
                "the change log is off, so this file can't be written".into(),
            ));
        };
        let mut log = ChangeLog {
            device,
            timestamp: to_iso_string(Timestamp::now()),
            changes: Vec::new(),
        };

        let transaction = self.conn.unchecked_transaction()?;
        let result = f(self, &mut log)?;
        log.write(&self.conn)?;
        transaction.commit()?;
        Ok(result)
    }

    /// Port of `#insertOne`. A field left out is written as null, as knex does with
    /// `useNullAsDefault`.
    fn insert_one(
        &self,
        schema_name: &str,
        field_value_map: &mut FieldValueMap,
        log: &mut ChangeLog,
    ) -> Result<(), Error> {
        if name_of(field_value_map).is_none() {
            field_value_map.insert(
                "name".into(),
                FieldValue::Value(RawValue::Text(get_random_string())),
            );
        }

        let schema = self.schema(schema_name)?;
        let fieldnames: Vec<&str> = schema
            .fields
            .iter()
            .filter(|f| f.fieldtype != Some(FieldType::Table) && !f.computed)
            .map(|f| f.fieldname.as_str())
            .collect();
        let mut values = Vec::with_capacity(fieldnames.len());
        for fieldname in &fieldnames {
            values.push(column_value(field_value_map, fieldname)?);
        }

        let sql = format!(
            "insert into {} ({}) values ({})",
            quote(schema_name),
            fieldnames
                .iter()
                .map(|f| quote(f))
                .collect::<Vec<_>>()
                .join(", "),
            vec!["?"; fieldnames.len()].join(", ")
        );
        self.conn
            .execute(&sql, rusqlite::params_from_iter(values.iter()))?;

        let written = fieldnames
            .iter()
            .filter(|f| field_value_map.contains_key(**f))
            .map(|f| (*f).to_owned())
            .collect();
        let name = name_of(field_value_map).unwrap_or_default();
        log.push(Change::new(schema_name, name, Operation::Insert).with_fields(written));
        Ok(())
    }

    /// Port of `#updateOne`: every value given but the name, the child tables and computed
    /// fields.
    fn update_one(
        &self,
        schema_name: &str,
        field_value_map: &FieldValueMap,
        log: &mut ChangeLog,
    ) -> Result<(), Error> {
        let schema = self.schema(schema_name)?;
        let skip = |fieldname: &str| {
            fieldname == "name"
                || schema
                    .field(fieldname)
                    .is_some_and(|f| f.fieldtype == Some(FieldType::Table) || f.computed)
        };
        let fieldnames: Vec<&str> = field_value_map
            .keys()
            .map(String::as_str)
            .filter(|f| !skip(f))
            .collect();
        if fieldnames.is_empty() {
            return Ok(());
        }

        let Some(name) = name_of(field_value_map) else {
            return Err(Error::Value(format!("{schema_name}: name is mandatory")));
        };
        let mut values: Vec<&dyn ToSql> = Vec::with_capacity(fieldnames.len() + 1);
        for fieldname in &fieldnames {
            match &field_value_map[*fieldname] {
                FieldValue::Value(value) => values.push(value),
                FieldValue::Table(_) => return Err(not_a_column(schema_name, fieldname)),
            }
        }
        values.push(&name);

        let assignments: Vec<String> = fieldnames
            .iter()
            .map(|f| format!("{} = ?", quote(f)))
            .collect();
        let sql = format!(
            "update {} set {} where `name` = ?",
            quote(schema_name),
            assignments.join(", ")
        );
        let updated = self.conn.execute(&sql, values.as_slice())?;
        if updated > 0 {
            let written = fieldnames.iter().map(|f| (*f).to_owned()).collect();
            log.push(Change::new(schema_name, name, Operation::Update).with_fields(written));
        }
        Ok(())
    }

    /// Port of `#updateSingleValues`.
    fn update_single_values(
        &self,
        single_schema_name: &str,
        field_value_map: &FieldValueMap,
        log: &mut ChangeLog,
    ) -> Result<(), Error> {
        let schema = self.schema(single_schema_name)?;
        let mut written = Vec::new();
        for field in schema
            .fields
            .iter()
            .filter(|f| !f.computed && f.fieldtype != Some(FieldType::Table))
        {
            let Some(value) = field_value_map.get(&field.fieldname) else {
                continue;
            };
            let FieldValue::Value(value) = value else {
                return Err(not_a_column(single_schema_name, &field.fieldname));
            };
            self.update_single_value(single_schema_name, &field.fieldname, value, &log.timestamp)?;
            written.push(field.fieldname.clone());
        }

        if !written.is_empty() {
            log.push(
                Change::new(single_schema_name, single_schema_name, Operation::Update)
                    .with_fields(written),
            );
        }
        Ok(())
    }

    /// Port of `#updateSingleValue` and `#insertSingleValue`.
    fn update_single_value(
        &self,
        single_schema_name: &str,
        fieldname: &str,
        value: &RawValue,
        now: &str,
    ) -> Result<(), Error> {
        let exists = self.conn.query_row(
            "select count(*) from `SingleValue` where `parent` = ? and `fieldname` = ?",
            [single_schema_name, fieldname],
            |r| r.get::<_, i64>(0),
        )? > 0;

        if exists {
            self.conn.execute(
                "update `SingleValue` set `value` = ?, `modifiedBy` = ?, `modified` = ? \
                 where `parent` = ? and `fieldname` = ?",
                rusqlite::params![value, SYSTEM, now, single_schema_name, fieldname],
            )?;
        } else {
            self.conn.execute(
                "insert into `SingleValue` (`createdBy`, `modifiedBy`, `created`, `modified`, \
                 `parent`, `fieldname`, `value`, `name`) values (?, ?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    SYSTEM,
                    SYSTEM,
                    now,
                    now,
                    single_schema_name,
                    fieldname,
                    value,
                    get_random_string()
                ],
            )?;
        }
        Ok(())
    }

    /// Port of `#insertOrUpdateChildren`. A child table left out, or null, is left alone.
    fn insert_or_update_children(
        &self,
        schema_name: &str,
        field_value_map: &mut FieldValueMap,
        is_update: bool,
        log: &mut ChangeLog,
    ) -> Result<(), Error> {
        let schema = self.schema(schema_name)?;
        let parent_name = if schema.is_single {
            schema_name.to_owned()
        } else {
            name_of(field_value_map).unwrap_or_default().to_owned()
        };

        for (fieldname, target) in table_fields(schema) {
            let rows = match field_value_map.get_mut(fieldname) {
                None | Some(FieldValue::Value(RawValue::Null)) => continue,
                Some(FieldValue::Table(rows)) => rows,
                Some(FieldValue::Value(_)) => {
                    return Err(Error::Value(format!(
                        "{schema_name}.{fieldname} must be a list of rows"
                    )));
                }
            };

            let mut added: Vec<String> = Vec::new();
            for child in rows.iter_mut() {
                prepare_child(schema_name, &parent_name, child, fieldname, added.len());
                let child_name = name_of(child).unwrap_or_default().to_owned();
                if is_update && self.exists(target, Some(&child_name))? {
                    self.update_one(target, child, log)?;
                } else {
                    self.insert_one(target, child, log)?;
                }
                added.push(child_name);
            }

            if is_update {
                let filters: QueryFilter = [
                    ("parent".to_owned(), parent_name.as_str().into()),
                    (
                        "name".to_owned(),
                        Filter::list("not in", added.into_iter().map(FilterValue::Text).collect()),
                    ),
                ]
                .into();
                self.delete_where(target, &filters, log)?;
            }
        }
        Ok(())
    }

    /// Deletes the rows matching `filters`, logging each by name.
    fn delete_where(
        &self,
        schema_name: &str,
        filters: &QueryFilter,
        log: &mut ChangeLog,
    ) -> Result<usize, Error> {
        let mut condition = String::new();
        let mut params = Vec::new();
        apply_filters(&mut condition, &mut params, filters);

        let names = self.query(
            &format!("select `name` from {}{condition}", quote(schema_name)),
            &params,
        )?;
        let deleted = self.conn.execute(
            &format!("delete from {}{condition}", quote(schema_name)),
            rusqlite::params_from_iter(params.iter()),
        )?;
        for row in names {
            if let Some(RawValue::Text(name)) = row.get("name") {
                log.push(Change::new(schema_name, name, Operation::Delete));
            }
        }
        Ok(deleted)
    }
}

/// Port of `#prepareChild`.
fn prepare_child(
    parent_schema_name: &str,
    parent_name: &str,
    child: &mut FieldValueMap,
    parent_fieldname: &str,
    idx: usize,
) {
    if name_of(child).is_none() {
        child.insert(
            "name".into(),
            FieldValue::Value(RawValue::Text(get_random_string())),
        );
    }
    let text = |t: &str| FieldValue::Value(RawValue::Text(t.to_owned()));
    child.insert("parent".into(), text(parent_name));
    child.insert("parentSchemaName".into(), text(parent_schema_name));
    child.insert("parentFieldname".into(), text(parent_fieldname));
    if matches!(
        child.get("idx"),
        None | Some(FieldValue::Value(RawValue::Null))
    ) {
        child.insert(
            "idx".into(),
            FieldValue::Value(RawValue::Integer(idx as i64)),
        );
    }
}

/// The document's name, unless it is missing or empty (`!fieldValueMap.name` in Books).
fn name_of(field_value_map: &FieldValueMap) -> Option<&str> {
    match field_value_map.get("name") {
        Some(FieldValue::Value(RawValue::Text(name))) if !name.is_empty() => Some(name),
        _ => None,
    }
}

fn column_value<'a>(
    field_value_map: &'a FieldValueMap,
    fieldname: &str,
) -> Result<&'a RawValue, Error> {
    match field_value_map.get(fieldname) {
        None => Ok(&RawValue::Null),
        Some(FieldValue::Value(value)) => Ok(value),
        Some(FieldValue::Table(_)) => Err(Error::Value(format!("{fieldname} is not a column"))),
    }
}

fn not_a_column(schema_name: &str, fieldname: &str) -> Error {
    Error::Value(format!("{schema_name}.{fieldname} is not a column"))
}

/// A default value as knex writes it into a `create table`.
fn format_default(value: &latte_schema::RawValue) -> String {
    match value {
        latte_schema::RawValue::Bool(b) => format!("'{}'", u8::from(*b)),
        latte_schema::RawValue::Number(n) => match n.as_f64() {
            // JavaScript prints 1.0 as 1.
            Some(f) if f.fract() == 0.0 && f.abs() < 1e21 => format!("'{}'", f as i64),
            _ => format!("'{n}'"),
        },
        latte_schema::RawValue::Text(t) => format!("'{}'", t.replace('\'', "''")),
    }
}

#[cfg(test)]
mod tests;
