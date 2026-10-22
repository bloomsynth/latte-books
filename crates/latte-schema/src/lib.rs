//! Loads the Frappe Books JSON schemas vendored in `vendor/books/schemas`.
//!
//! Port of `schemas/` @ v0.36.0. See `vendor/books/schemas/README.md` for how regional,
//! abstract and subclass schemas combine into the final schema.
//!
//! Translating labels, descriptions and placeholders is left to `latte-i18n`, as Books does
//! it outside `schemas/` too.

// Port of schemas/index.ts @ v0.36.0.
//
// Books combines the schemas as plain JSON objects and only then treats them as typed. This
// does the same: the combining works on `serde_json` maps, which keep key order as JavaScript
// objects do, and each finished schema is then read into a `Schema`.

mod sources;
mod types;

use std::collections::{HashMap, HashSet};
use std::fmt;

use indexmap::IndexMap;
use serde_json::{Map, Value, json};

pub use types::{Field, FieldType, Naming, RawValue, Schema, SelectOption};

/// All schemas by name, in Books' order.
pub type SchemaMap = IndexMap<String, Schema>;

/// A schema, or part of one, before combining: `SchemaStub` in Books.
type Stub = Map<String, Value>;
type StubMap = IndexMap<String, Stub>;

/// A custom field as stored in the `CustomField` table. Port of `RawCustomField` in
/// `backend/database/types.ts`.
#[derive(Debug, Clone, Default)]
pub struct RawCustomField {
    /// The schema the field is added to.
    pub parent: String,
    pub label: String,
    pub fieldname: String,
    pub fieldtype: FieldType,
    pub is_required: Option<bool>,
    pub section: Option<String>,
    pub tab: Option<String>,
    /// Select options, one per line.
    pub options: Option<String>,
    pub target: Option<String>,
    pub references: Option<String>,
    pub default: Option<String>,
}

/// A vendored schema file that isn't valid, or a combined schema that doesn't fit `Schema`.
#[derive(Debug)]
pub struct Error {
    pub schema: String,
    pub source: serde_json::Error,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "schema {}: {}", self.schema, self.source)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Builds every schema for a company in `country_code` (e.g. `"in"`), with its custom fields.
/// Books passes `"-"` when there is no country.
pub fn get_schemas(
    country_code: &str,
    raw_custom_fields: &[RawCustomField],
) -> Result<SchemaMap, Error> {
    let built_core_schemas = get_core_schemas()?;
    let built_app_schemas = get_app_schemas(country_code)?;

    let mut stub_map = built_app_schemas;
    stub_map.extend(built_core_schemas);
    add_meta_fields(&mut stub_map)?;
    remove_fields(&mut stub_map);
    set_schema_name_on_fields(&mut stub_map);

    let mut schema_map = SchemaMap::with_capacity(stub_map.len());
    for (name, stub) in stub_map {
        let schema = serde_json::from_value(Value::Object(stub)).map_err(|source| Error {
            schema: name.clone(),
            source,
        })?;
        schema_map.insert(name, schema);
    }

    add_custom_fields(&mut schema_map, raw_custom_fields);
    Ok(schema_map)
}

fn name_field() -> Value {
    json!({
        "fieldname": "name",
        "label": "ID",
        "fieldtype": "Data",
        "required": true,
        "readOnly": true,
    })
}

fn set_schema_name_on_fields(schema_map: &mut StubMap) {
    for (schema_name, schema) in schema_map {
        for field in fields_mut(schema) {
            if let Value::Object(field) = field {
                field.insert("schemaName".into(), schema_name.clone().into());
            }
        }
    }
}

fn remove_fields(schema_map: &mut StubMap) {
    for schema in schema_map.values_mut() {
        let Some(remove) = schema.shift_remove("removeFields") else {
            continue;
        };

        for fieldname in remove.as_array().into_iter().flatten() {
            fields_mut(schema).retain(|f| f.get("fieldname") != Some(fieldname));
            for key in ["tableFields", "quickEditFields", "keywordFields"] {
                if let Some(Value::Array(names)) = schema.get_mut(key) {
                    names.retain(|n| n != fieldname);
                }
            }

            if schema.get("linkDisplayField") == Some(fieldname) {
                schema.shift_remove("linkDisplayField");
            }
        }
    }
}

fn add_meta_fields(schema_map: &mut StubMap) -> Result<(), Error> {
    let meta_schema_map = get_map_from_list(sources::META)?;

    let base = &meta_schema_map["base"];
    let tree = get_combined(&meta_schema_map["tree"], base);
    let child = &meta_schema_map["child"];
    let submittable = get_combined(&meta_schema_map["submittable"], base);
    let submittable_tree = get_combined(&tree, &meta_schema_map["submittable"]);

    for schema in schema_map.values_mut() {
        if truthy(schema.get("isSingle")) {
            continue;
        }

        let is_tree = truthy(schema.get("isTree"));
        let is_submittable = truthy(schema.get("isSubmittable"));
        let meta = if is_tree && is_submittable {
            &submittable_tree
        } else if is_tree {
            &tree
        } else if is_submittable {
            &submittable
        } else if truthy(schema.get("isChild")) {
            child
        } else {
            base
        };
        let meta_fields = fields(meta).to_vec();
        fields_mut(schema).extend(meta_fields);
    }

    add_name_field(schema_map);
    add_title_field(schema_map);
    Ok(())
}

fn add_title_field(schema_map: &mut StubMap) {
    for schema in schema_map.values_mut() {
        if schema.get("titleField").is_none_or(Value::is_null) {
            schema.insert("titleField".into(), "name".into());
        }
    }
}

fn add_name_field(schema_map: &mut StubMap) {
    for schema in schema_map.values_mut() {
        if truthy(schema.get("isSingle")) {
            continue;
        }

        let fields = fields_mut(schema);
        if fields
            .iter()
            .any(|f| f.get("fieldname") == Some(&Value::from("name")))
        {
            continue;
        }

        fields.insert(0, name_field());
    }
}

fn get_core_schemas() -> Result<StubMap, Error> {
    let raw_schema_map = get_map_from_list(sources::CORE)?;
    let core_schema_map = get_abstract_combined_schemas(raw_schema_map);
    Ok(clean_schemas(core_schema_map))
}

fn get_app_schemas(country_code: &str) -> Result<StubMap, Error> {
    let app_schema_map = get_map_from_list(sources::APP)?;
    let regional_schema_map = get_map_from_list(sources::regional(country_code))?;
    let combined_schemas = get_regional_combined_schemas(app_schema_map, regional_schema_map);
    let schema_map = get_abstract_combined_schemas(combined_schemas);
    Ok(clean_schemas(schema_map))
}

fn clean_schemas(mut schema_map: StubMap) -> StubMap {
    schema_map
        .retain(|_, schema| !(truthy(schema.get("isAbstract")) && !truthy(schema.get("extends"))));
    for schema in schema_map.values_mut() {
        schema.shift_remove("extends");
        schema.shift_remove("isAbstract");
    }
    schema_map
}

/// `extending` laid over `abstract_schema`: its keys replace the abstract schema's, and its
/// fields replace the abstract schema's fields of the same name, keeping their place.
fn get_combined(extending: &Stub, abstract_schema: &Stub) -> Stub {
    let mut combined_fields = field_map(abstract_schema);
    combined_fields.extend(field_map(extending));

    let mut combined = abstract_schema.clone();
    combined.extend(extending.clone());
    combined.insert(
        "fields".into(),
        Value::Array(combined_fields.into_values().collect()),
    );
    combined
}

fn get_abstract_combined_schemas(schemas: StubMap) -> StubMap {
    let abstract_schema_names: HashSet<String> = schemas
        .iter()
        .filter(|(_, s)| truthy(s.get("isAbstract")))
        .map(|(n, _)| n.clone())
        .collect();

    let extends = |schema: &Stub| -> Option<String> {
        let name = schema.get("extends")?.as_str()?.to_owned();
        abstract_schema_names.contains(&name).then_some(name)
    };

    let mut schema_map: StubMap = schemas
        .iter()
        .filter(|(n, s)| !abstract_schema_names.contains(*n) && extends(s).is_none())
        .map(|(n, s)| (n.clone(), s.clone()))
        .collect();

    for (name, extending_schema) in &schemas {
        if let Some(abstract_name) = extends(extending_schema) {
            let combined = get_combined(extending_schema, &schemas[&abstract_name]);
            schema_map.insert(name.clone(), combined);
        }
    }

    for name in &abstract_schema_names {
        schema_map.shift_remove(name);
    }

    schema_map
}

fn get_regional_combined_schemas(app_schema_map: StubMap, regional_schema_map: StubMap) -> StubMap {
    let mut combined = app_schema_map;

    for (name, regional_schema) in regional_schema_map {
        let schema = match combined.get(&name) {
            Some(app_schema) => get_combined(&regional_schema, app_schema),
            None => regional_schema,
        };
        combined.insert(name, schema);
    }

    combined
}

fn add_custom_fields(schema_map: &mut SchemaMap, raw_custom_fields: &[RawCustomField]) {
    // Fieldnames each schema had before custom fields were added. As in Books, a custom field
    // is skipped when a built-in field has its name, but not when another custom field does.
    let mut schema_field_map: HashMap<&str, HashSet<&str>> = HashMap::new();
    let mut field_map: IndexMap<&str, Vec<Field>> = IndexMap::new();

    for raw in raw_custom_fields {
        let existing = schema_field_map
            .entry(raw.parent.as_str())
            .or_insert_with(|| {
                schema_map
                    .get(&raw.parent)
                    .map(|s| s.fields.iter().map(|f| f.fieldname.as_str()).collect())
                    .unwrap_or_default()
            });
        if existing.contains(raw.fieldname.as_str()) {
            continue;
        }

        let options: Vec<SelectOption> = raw
            .options
            .as_deref()
            .map(|o| o.split('\n').map(str::trim).filter(|v| !v.is_empty()))
            .into_iter()
            .flatten()
            .map(|value| SelectOption {
                value: value.to_owned(),
                label: value.to_owned(),
            })
            .collect();

        let mut field = Field {
            label: Some(raw.label.clone()),
            fieldname: raw.fieldname.clone(),
            fieldtype: Some(raw.fieldtype),
            section: raw.section.clone(),
            tab: raw.tab.clone(),
            is_custom: true,
            options: (!options.is_empty()).then_some(options),
            required: raw.is_required.unwrap_or_default(),
            target: raw.target.clone(),
            references: raw.references.clone(),
            ..Field::default()
        };

        if field.required {
            match &raw.default {
                Some(default) => field.default = Some(RawValue::Text(default.clone())),
                None => field.required = false,
            }
        }

        field_map
            .entry(raw.parent.as_str())
            .or_default()
            .push(field);
    }

    for (schema_name, fields) in field_map {
        if let Some(schema) = schema_map.get_mut(schema_name) {
            schema.fields.extend(fields);
        }
    }
}

/// Reads schema files into a map by name. A later file of the same name replaces an earlier
/// one in place. Port of `getMapFromList(list, 'name')`.
fn get_map_from_list(sources: &[sources::Source]) -> Result<StubMap, Error> {
    let mut map = StubMap::new();
    for (path, text) in sources {
        let stub: Stub = serde_json::from_str(text).map_err(|source| Error {
            schema: (*path).to_owned(),
            source,
        })?;
        if let Some(name) = stub.get("name").and_then(Value::as_str) {
            map.insert(name.to_owned(), stub);
        }
    }
    Ok(map)
}

/// A schema's fields by fieldname. Port of `getMapFromList(schema.fields ?? [], 'fieldname')`.
fn field_map(schema: &Stub) -> IndexMap<String, Value> {
    fields(schema)
        .iter()
        .filter_map(|f| Some((f.get("fieldname")?.as_str()?.to_owned(), f.clone())))
        .collect()
}

fn fields(schema: &Stub) -> &[Value] {
    match schema.get("fields") {
        Some(Value::Array(fields)) => fields,
        _ => &[],
    }
}

fn fields_mut(schema: &mut Stub) -> &mut Vec<Value> {
    let fields = schema
        .entry("fields")
        .or_insert_with(|| Value::Array(Vec::new()));
    if !fields.is_array() {
        *fields = Value::Array(Vec::new());
    }
    fields.as_array_mut().unwrap()
}

/// JavaScript truthiness, for flags such as `isAbstract`.
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

#[cfg(test)]
mod tests;
