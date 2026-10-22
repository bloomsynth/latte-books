//! SQLite access for Frappe Books company files (`.books.db`).
//!
//! Port of `backend/database/` @ v0.36.0. Every write goes through this crate and is
//! recorded in the `latte_changes` table (see docs/PLAN.md §7).
//!
//! [`Database::open_read_only`] opens a file without ever changing it; [`Database::open`]
//! opens it for writing. Either way the file must have been opened by Frappe Books v0.36.0,
//! as migrating isn't ported yet.

mod changes;
mod converter;
mod core;
pub mod pesa;
mod write;

use std::fmt;
use std::path::Path;

use latte_schema::{FieldType, RawCustomField, SchemaMap, get_schemas};
use rusqlite::{Connection, OpenFlags, OptionalExtension};

pub use crate::converter::{
    DocValue, DocValueMap, convert_doc_value, safe_parse_float, safe_parse_int, to_doc_value,
    to_doc_value_map, to_iso_string, to_raw_value, to_raw_value_map,
};
pub use crate::core::{
    DatabaseCore, FieldValue, FieldValueMap, Filter, FilterOperand, FilterValue, GetAllOptions,
    Order, QueryFilter, RawValue, SingleValue, SingleValueQuery,
};
use crate::pesa::MoneyMaker;
pub use crate::write::{SYSTEM, get_random_string, sqlite_type};

/// The Frappe Books version whose files this reads.
pub const BOOKS_VERSION: &str = "0.36.0";

// fyo/utils/consts.ts
const DEFAULT_INTERNAL_PRECISION: u32 = 11;
const DEFAULT_DISPLAY_PRECISION: u32 = 2;
const DEFAULT_CURRENCY: &str = "INR";

#[derive(Debug)]
pub enum Error {
    Sqlite(rusqlite::Error),
    Schema(latte_schema::Error),
    Pesa(pesa::PesaError),
    /// A schema or document that doesn't exist.
    NotFound(String),
    /// A value that can't be read or converted.
    Value(String),
    /// The file was last opened by another version of Frappe Books, so its tables may not
    /// match the schemas yet.
    Version {
        found: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(e) => write!(f, "{e}"),
            Self::Schema(e) => write!(f, "{e}"),
            Self::Pesa(e) => write!(f, "{e}"),
            Self::NotFound(m) | Self::Value(m) => f.write_str(m),
            Self::Version { found } => write!(
                f,
                "this file is from Frappe Books {found}; open it in Frappe Books {BOOKS_VERSION} \
                 once so it is brought up to date"
            ),
        }
    }
}

impl std::error::Error for Error {}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}

impl From<latte_schema::Error> for Error {
    fn from(e: latte_schema::Error) -> Self {
        Self::Schema(e)
    }
}

impl From<pesa::PesaError> for Error {
    fn from(e: pesa::PesaError) -> Self {
        Self::Pesa(e)
    }
}

/// An open company file: the raw database, its schemas and its money settings.
pub struct Database {
    core: DatabaseCore,
    country_code: String,
    pesa: MoneyMaker,
}

impl Database {
    /// Opens a company file without changing it. The file must have been opened by Frappe
    /// Books v0.36.0, which brings its tables up to date with the schemas.
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self, Error> {
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        Self::from_connection(Connection::open_with_flags(path, flags)?)
    }

    /// Opens a company file for writing. Every write is recorded in its `latte_changes`
    /// table as made on `device`.
    pub fn open(path: impl AsRef<Path>, device: impl Into<String>) -> Result<Self, Error> {
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let mut db = Self::from_connection(Connection::open_with_flags(path, flags)?)?;
        db.core.enable_change_log(device)?;
        Ok(db)
    }

    /// Port of `DatabaseManager._connect` and the version lookup in `#getAppVersion`.
    pub fn from_connection(conn: Connection) -> Result<Self, Error> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let version = system_setting(&conn, "version")?.unwrap_or_else(|| "0.0.0".into());
        if version != BOOKS_VERSION {
            return Err(Error::Version { found: version });
        }

        let country_code = system_setting(&conn, "countryCode")?.unwrap_or_else(|| "in".into());
        let raw_custom_fields = read_custom_fields(&conn)?;
        let schema_map = get_schemas(&country_code, &raw_custom_fields)?;
        let core = DatabaseCore::new(conn, schema_map);
        let pesa = money_maker(&core)?;
        Ok(Self {
            core,
            country_code,
            pesa,
        })
    }

    pub fn core(&self) -> &DatabaseCore {
        &self.core
    }

    pub fn core_mut(&mut self) -> &mut DatabaseCore {
        &mut self.core
    }

    pub fn schema_map(&self) -> &SchemaMap {
        self.core.schema_map()
    }

    pub fn country_code(&self) -> &str {
        &self.country_code
    }

    /// Makes money with the file's currency and precisions. `fyo.pesa` in Books.
    pub fn pesa(&self) -> &MoneyMaker {
        &self.pesa
    }

    /// A document with its child tables, as document values.
    pub fn get_doc(&self, schema_name: &str, name: &str) -> Result<DocValueMap, Error> {
        let raw = self.core.get(schema_name, name, None)?;
        // Books returns only the (empty) child tables for a missing document.
        if !raw.contains_key("name") && !self.core.schema_map()[schema_name].is_single {
            return Err(Error::NotFound(format!("{schema_name} {name} not found")));
        }
        to_doc_value_map(self.schema_map(), &self.pesa, schema_name, &raw)
    }

    /// Rows of a schema, as document values. See [`DatabaseCore::get_all`] for the options.
    pub fn get_all_docs(
        &self,
        schema_name: &str,
        options: &GetAllOptions,
    ) -> Result<Vec<DocValueMap>, Error> {
        self.core
            .get_all(schema_name, options)?
            .iter()
            .map(|row| to_doc_value_map(self.schema_map(), &self.pesa, schema_name, row))
            .collect()
    }

    /// Inserts a document and returns it as written, with its name and its child rows'
    /// parent fields filled in. See [`DatabaseCore::insert`].
    pub fn insert(&mut self, schema_name: &str, doc: &DocValueMap) -> Result<DocValueMap, Error> {
        let mut raw = to_raw_value_map(self.schema_map(), &self.pesa, schema_name, doc)?;
        self.core.insert(schema_name, &mut raw)?;
        to_doc_value_map(self.schema_map(), &self.pesa, schema_name, &raw)
    }

    /// See [`DatabaseCore::update`].
    pub fn update(&mut self, schema_name: &str, doc: &DocValueMap) -> Result<(), Error> {
        let mut raw = to_raw_value_map(self.schema_map(), &self.pesa, schema_name, doc)?;
        self.core.update(schema_name, &mut raw)
    }

    /// See [`DatabaseCore::exists`].
    pub fn exists(&self, schema_name: &str, name: Option<&str>) -> Result<bool, Error> {
        self.core.exists(schema_name, name)
    }

    /// See [`DatabaseCore::delete`].
    pub fn delete(&mut self, schema_name: &str, name: &str) -> Result<(), Error> {
        self.core.delete(schema_name, name)
    }

    /// See [`DatabaseCore::delete_all`].
    pub fn delete_all(&mut self, schema_name: &str, filters: &QueryFilter) -> Result<usize, Error> {
        self.core.delete_all(schema_name, filters)
    }

    /// See [`DatabaseCore::rename`].
    pub fn rename(
        &mut self,
        schema_name: &str,
        old_name: &str,
        new_name: &str,
    ) -> Result<(), Error> {
        self.core.rename(schema_name, old_name, new_name)
    }
}

fn system_setting(conn: &Connection, fieldname: &str) -> Result<Option<String>, Error> {
    let result = conn
        .query_row(
            "select `value` from `SingleValue` where `fieldname` = ? and `parent` = 'SystemSettings'",
            [fieldname],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional();
    match result {
        Ok(value) => Ok(value.flatten().filter(|v| !v.is_empty())),
        // Books treats a file without the table as not set up yet.
        Err(e) if e.to_string().contains("no such table") => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Port of `DatabaseManager.setRawCustomFields`.
fn read_custom_fields(conn: &Connection) -> Result<Vec<RawCustomField>, Error> {
    let mut statement = match conn.prepare(
        "select `parent`, `label`, `fieldname`, `fieldtype`, `isRequired`, `section`, `tab`, \
         `options`, `target`, `references`, `default` from `CustomField`",
    ) {
        Ok(statement) => statement,
        Err(e) if e.to_string().contains("no such table") => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let rows = statement.query_map([], |r| {
        let fieldtype: String = r.get(3)?;
        Ok((
            fieldtype,
            RawCustomField {
                parent: r.get(0)?,
                label: r.get(1)?,
                fieldname: r.get(2)?,
                is_required: r.get::<_, Option<f64>>(4)?.map(|n| n != 0.0),
                section: r.get(5)?,
                tab: r.get(6)?,
                options: r.get(7)?,
                target: r.get(8)?,
                references: r.get(9)?,
                default: r.get(10)?,
                ..RawCustomField::default()
            },
        ))
    })?;

    let mut custom_fields = Vec::new();
    for row in rows {
        let (fieldtype, mut field) = row?;
        field.fieldtype =
            serde_json::from_value::<FieldType>(fieldtype.clone().into()).map_err(|_| {
                Error::Value(format!(
                    "custom field {}: unknown fieldtype {fieldtype}",
                    field.fieldname
                ))
            })?;
        custom_fields.push(field);
    }
    Ok(custom_fields)
}

/// Port of the money settings in `Fyo.#initializeMoneyMaker`.
fn money_maker(core: &DatabaseCore) -> Result<MoneyMaker, Error> {
    let settings =
        ["internalPrecision", "displayPrecision", "currency"].map(|fieldname| SingleValueQuery {
            fieldname: fieldname.into(),
            parent: Some("SystemSettings".into()),
        });
    let values = core.get_single_values(&settings)?;
    let value = |fieldname: &str| {
        values
            .iter()
            .find(|v| v.fieldname == fieldname)
            .map(|v| &v.value)
    };
    // pesa rounds the precision, which Books stores as text such as "11.0".
    let precision = |fieldname: &str, default: u32| match value(fieldname) {
        Some(RawValue::Text(t)) => safe_parse_float(t).round().max(0.0) as u32,
        Some(RawValue::Integer(i)) => (*i).max(0) as u32,
        Some(RawValue::Real(r)) => r.round().max(0.0) as u32,
        _ => default,
    };

    Ok(MoneyMaker {
        precision: precision("internalPrecision", DEFAULT_INTERNAL_PRECISION),
        display: precision("displayPrecision", DEFAULT_DISPLAY_PRECISION),
        currency: match value("currency") {
            Some(RawValue::Text(t)) => t.clone(),
            _ => DEFAULT_CURRENCY.into(),
        },
    })
}

#[cfg(test)]
mod tests;
