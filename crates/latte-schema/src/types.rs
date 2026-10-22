// Port of schemas/types.ts @ v0.36.0.
//
// Books models a field as a union of TypeScript interfaces, one per group of field types. Here
// a field is one struct: the attributes that belong to only some field types (`options`,
// `target`, `references`, `minvalue`, ...) are `Option`s. `getOptions` is a function in Books
// and is left out; it belongs to the models.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FieldType {
    #[default]
    Data,
    Select,
    Link,
    Date,
    Datetime,
    Table,
    AutoComplete,
    Check,
    AttachImage,
    DynamicLink,
    Int,
    Float,
    Currency,
    Text,
    Color,
    Button,
    Attachment,
    /// Not in Books' `FieldType`, but `ERPNextSyncSettings.authToken` uses it.
    Secret,
}

impl FieldType {
    /// `OptionFieldType` in Books: fields that carry `options`.
    pub fn has_options(self) -> bool {
        matches!(self, Self::Select | Self::AutoComplete | Self::Color)
    }

    /// `TargetFieldType` in Books: fields that carry a `target` schema.
    pub fn has_target(self) -> bool {
        matches!(self, Self::Table | Self::Link)
    }
}

/// A default value as written in a schema. It should match the field's database type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RawValue {
    Bool(bool),
    /// Kept as written, so a decimal default loses nothing.
    Number(serde_json::Number),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectOption {
    pub value: String,
    /// `PricingRule.priority` writes its labels as numbers. They are shown as text.
    #[serde(deserialize_with = "string_or_number")]
    pub label: String,
}

fn string_or_number<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Label {
        Text(String),
        Number(serde_json::Number),
    }

    Ok(match Label::deserialize(deserializer)? {
        Label::Text(text) => text,
        Label::Number(number) => number.to_string(),
    })
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Field {
    /// Column name in the db.
    pub fieldname: String,
    /// UI descriptive field type that maps to a column type. `None` only for a placeholder
    /// field in an abstract schema that no subclass filled in.
    pub fieldtype: Option<FieldType>,
    /// Translatable UI facing name. Books' types require it, but meta fields such as `idx`
    /// have none.
    pub label: Option<String>,
    /// Name of the schema the field belongs to. Books doesn't set it on custom fields.
    pub schema_name: Option<String>,
    /// Implies not null.
    #[serde(default)]
    pub required: bool,
    /// Whether the field is shown in a form.
    #[serde(default)]
    pub hidden: bool,
    /// Whether the field is editable.
    #[serde(default)]
    pub read_only: bool,
    /// Translatable inline documentation.
    pub description: Option<String>,
    pub default: Option<RawValue>,
    /// Translatable form field placeholder.
    pub placeholder: Option<String>,
    /// Used in dropdown fields.
    pub group_by: Option<String>,
    /// Only for the db, not the UI.
    #[serde(default)]
    pub meta: bool,
    /// Whether the field is used to filter the list.
    #[serde(default)]
    pub filter: bool,
    /// Computed values are not stored in the database.
    #[serde(default)]
    pub computed: bool,
    /// Groups fields into sections.
    pub section: Option<String>,
    /// Groups fields into tabs.
    pub tab: Option<String>,
    /// Marks the place of a field in an abstract schema; a subclass supplies the field.
    #[serde(default, rename = "abstract")]
    pub is_abstract: bool,
    #[serde(default)]
    pub is_custom: bool,
    pub filters: Option<BTreeMap<String, String>>,

    /// Select, AutoComplete and Color fields.
    pub options: Option<Vec<SelectOption>>,
    #[serde(default)]
    pub allow_custom: bool,

    /// Table and Link fields: the schema to fetch values from.
    pub target: Option<String>,
    /// Whether to show Create in the dropdown.
    #[serde(default)]
    pub create: bool,
    /// Whether the table has quick editable columns.
    #[serde(default)]
    pub edit: bool,

    /// DynamicLink fields: the option field that names the target schema.
    pub references: Option<String>,

    /// Int and Float fields: UI bounds.
    pub minvalue: Option<f64>,
    pub maxvalue: Option<f64>,

    /// Not in Books' types. Set on `POSProfile.name`.
    #[serde(default)]
    pub unique: bool,
    /// A misspelling of `readOnly` in `IntegrationErrorLog`. Books reads only `readOnly`, so
    /// this has no effect there either.
    #[serde(default, rename = "readonly")]
    pub readonly_misspelt: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Naming {
    Autoincrement,
    Random,
    NumberSeries,
    Manual,
}

/// A schema after loading: regional, abstract and meta schemas are already combined in, so
/// `extends`, `isAbstract` and `removeFields` are gone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Schema {
    /// Table name.
    pub name: String,
    /// Translatable UI facing name.
    pub label: String,
    /// Maps to database columns.
    pub fields: Vec<Field>,
    /// Nested set, e.g. for the chart of accounts.
    #[serde(default)]
    pub is_tree: bool,
    /// A child table, i.e. a table with a "parent" foreign key column.
    #[serde(default)]
    pub is_child: bool,
    /// Fields are stored as values in `SingleValue`.
    #[serde(default)]
    pub is_single: bool,
    /// Fields shown as columns of a child table.
    pub table_fields: Option<Vec<String>>,
    /// Transactional types: values count only after submit.
    #[serde(default)]
    pub is_submittable: bool,
    /// Fields used for search.
    pub keyword_fields: Option<Vec<String>>,
    /// Fields for the quick edit form.
    pub quick_edit_fields: Option<Vec<String>>,
    /// Display field if inline editable.
    pub link_display_field: Option<String>,
    /// Whether the user can create an entry from the list view.
    pub create: Option<bool>,
    /// How the name is assigned. Books defaults to `random`, or `numberSeries` if present.
    pub naming: Option<Naming>,
    /// Main display field. Always set after loading.
    pub title_field: Option<String>,
    /// Not in Books' types. Set on the invoice, receipt and shipment schemas.
    #[serde(default)]
    pub show_title: bool,
}

impl Schema {
    pub fn field(&self, fieldname: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.fieldname == fieldname)
    }
}
