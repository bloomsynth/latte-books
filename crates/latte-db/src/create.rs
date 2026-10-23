// Port of what Books does to a new file @ v0.36.0: `DatabaseManager.createNewDatabase`,
// the first `migrate` with `#initializeSingles` (backend/database/core.ts), and the
// patches in backend/patches/, which all run on a new file.
//
// On a new file the patches do nothing but add the units (addUOMs) and the Cash, Bank
// and Transfer payment methods (createPaymentMethods), and record that they have run in
// `PatchRun`, so Books doesn't run them again.
//
// Differences from Books:
// - Books deletes a file already at the path; this refuses to.
// - Books sets the version later, when the app starts (`setVersion`); this sets it here,
//   so the file can be opened.

use std::path::Path;

use jiff::Timestamp;
use latte_schema::{RawValue as SchemaValue, get_schemas};
use rusqlite::Connection;

use crate::core::{DatabaseCore, FieldValue, FieldValueMap, RawValue};
use crate::write::SYSTEM;
use crate::{BOOKS_VERSION, Database, Error, to_iso_string};

/// The patches of backend/patches/index.ts, in the order Books runs them on a new file:
/// the ones run before migrating, the first by priority, then the rest.
const PATCHES: [&str; 9] = [
    "updateSchemas",
    "testPatch",
    "fixRoundOffAccount",
    "createInventoryNumberSeries",
    "setPaymentReferenceType",
    "fixLedgerDateTime",
    "fixItemHSNField",
    "addUOMs",
    "createPaymentMethods",
];

/// `defaultUOMs` in backend/patches/addUOMs.ts.
const UOMS: [(&str, bool); 6] = [
    ("Unit", true),
    ("Kg", false),
    ("Gram", false),
    ("Meter", false),
    ("Hour", false),
    ("Day", false),
];

impl Database {
    /// Creates a company file for a country (`in`, `ch` and so on): every table, the
    /// settings' default values, the units and the payment methods. Every write is recorded
    /// as made on `device`. The company itself is set up by `latte_core::setup`.
    pub fn create(
        path: impl AsRef<Path>,
        country_code: &str,
        device: impl Into<String>,
    ) -> Result<Self, Error> {
        let path = path.as_ref();
        if path.exists() {
            return Err(Error::Value(format!("{} already exists", path.display())));
        }

        let device = device.into();
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let mut core = DatabaseCore::new(conn, get_schemas(country_code, &[])?);
        let tables: Vec<String> = core
            .schema_map()
            .values()
            .filter(|schema| !schema.is_single)
            .map(|schema| schema.name.clone())
            .collect();
        for table in &tables {
            core.create_table(table)?;
        }
        core.enable_change_log(device.clone())?;
        initialize_singles(&mut core)?;
        run_patches(&mut core)?;

        let mut settings = FieldValueMap::new();
        settings.insert("version".into(), text(BOOKS_VERSION));
        settings.insert("countryCode".into(), text(country_code));
        core.update("SystemSettings", &mut settings)?;

        let mut db = Self::from_connection(core.into_connection())?;
        db.core.enable_change_log(device)?;
        Ok(db)
    }
}

/// Port of `#initializeSingles` for a new file: each single's default values. Books writes
/// a check as the number 0 or 1, so it is stored as "0.0" or "1.0".
fn initialize_singles(core: &mut DatabaseCore) -> Result<(), Error> {
    let singles: Vec<(String, FieldValueMap)> = core
        .schema_map()
        .values()
        .filter(|schema| schema.is_single)
        .map(|schema| {
            let defaults = schema
                .fields
                .iter()
                .filter_map(|field| {
                    let value = match field.default.as_ref()? {
                        SchemaValue::Bool(b) => RawValue::Real(f64::from(u8::from(*b))),
                        SchemaValue::Number(n) => RawValue::Real(n.as_f64()?),
                        SchemaValue::Text(t) => RawValue::Text(t.clone()),
                    };
                    Some((field.fieldname.clone(), FieldValue::Value(value)))
                })
                .collect();
            (schema.name.clone(), defaults)
        })
        .collect();

    for (schema_name, mut defaults) in singles {
        if !defaults.is_empty() {
            core.insert(&schema_name, &mut defaults)?;
        }
    }
    Ok(())
}

/// What the patches do to a new file, and their `PatchRun` entries (`makeEntry`).
fn run_patches(core: &mut DatabaseCore) -> Result<(), Error> {
    for patch in PATCHES {
        match patch {
            "addUOMs" => {
                for (name, is_whole) in UOMS {
                    let mut uom = meta();
                    uom.insert("name".into(), text(name));
                    uom.insert(
                        "isWhole".into(),
                        FieldValue::Value(RawValue::Integer(is_whole.into())),
                    );
                    core.insert("UOM", &mut uom)?;
                }
            }
            // A new file has no accounts yet, so the methods have none.
            "createPaymentMethods" => {
                for (name, method_type) in
                    [("Cash", "Cash"), ("Bank", "Bank"), ("Transfer", "Bank")]
                {
                    let mut method = meta();
                    method.insert("name".into(), text(name));
                    method.insert("type".into(), text(method_type));
                    method.insert("account".into(), FieldValue::Value(RawValue::Null));
                    core.insert("PaymentMethod", &mut method)?;
                }
            }
            _ => {}
        }

        let mut entry = meta();
        entry.insert("name".into(), text(patch));
        entry.insert("failed".into(), FieldValue::Value(RawValue::Integer(0)));
        // The version the file had: a new file's default.
        entry.insert("version".into(), text("0.0.0"));
        core.insert("PatchRun", &mut entry)?;
    }
    Ok(())
}

/// `getDefaultMetaFieldValueMap`.
fn meta() -> FieldValueMap {
    let now = to_iso_string(Timestamp::now());
    let mut map = FieldValueMap::new();
    map.insert("createdBy".into(), text(SYSTEM));
    map.insert("modifiedBy".into(), text(SYSTEM));
    map.insert("created".into(), text(&now));
    map.insert("modified".into(), text(&now));
    map
}

fn text(value: &str) -> FieldValue {
    FieldValue::Value(RawValue::Text(value.to_owned()))
}
