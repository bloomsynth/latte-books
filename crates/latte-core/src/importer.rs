// Port of src/importer.ts @ v0.36.0, with utils/csvParser.ts and the checks and import of
// src/pages/ImportWizard.vue.
//
// Deviations:
// - A link that doesn't exist is named with the schema it links to. Books names it with
//   the schema being imported: an invoice's missing customer is "(Sales Invoice, Acme)".
// - Columns past the end of the file are for no field. Books leaves them for the fields
//   the template had there, which the file doesn't have: a file with fewer columns than
//   the template is refused for duplicate columns, or counted as having columns it
//   hasn't.
// - Books' CSV parser means to keep a quote doubled inside a quoted value from ending it,
//   but checks the wrong string, so every quote opens or closes a quoted value. For a
//   well-formed file that comes to the same thing, and Latte reads it the same way.

use std::collections::HashMap;

use indexmap::IndexMap;
use latte_db::{DocValue, DocValueMap, RawValue};
use latte_schema::{Field, FieldType, Naming, Schema};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;

/// `TemplateField`: a field that can be imported, from the schema or one of its tables.
#[derive(Debug, Clone)]
pub struct TemplateField {
    pub field: Field,
    pub schema_name: String,
    pub schema_label: String,
    /// `schemaName.fieldname`.
    pub field_key: String,
    /// The table field of the schema being imported that this field's rows are in.
    pub parent_child_field: Option<Field>,
}

impl TemplateField {
    pub fn label(&self) -> &str {
        self.field.label.as_deref().unwrap_or(&self.field.fieldname)
    }

    /// `getColumnLabel`.
    pub fn column_label(&self) -> String {
        match &self.parent_child_field {
            Some(table) => format!(
                "{} ({})",
                self.label(),
                table.label.as_deref().unwrap_or(&table.fieldname)
            ),
            None => self.label().to_owned(),
        }
    }
}

/// `ValueMatrixItem`: a cell, as read and as converted for its column's field. `value`
/// is `None` when the column has no field.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cell {
    pub raw: String,
    pub value: Option<DocValue>,
    pub error: bool,
}

impl Cell {
    fn has_value(&self) -> bool {
        !matches!(self.value, None | Some(DocValue::Null))
    }
}

/// A link that `check_links` didn't find.
#[derive(Debug, Clone, PartialEq)]
pub struct AbsentLink {
    pub schema_label: String,
    pub name: String,
}

/// What `import` did with each document: its name, or why it failed.
#[derive(Debug, Default)]
pub struct ImportResult {
    pub success: Vec<String>,
    /// The names the file gave the documents that were made.
    pub success_old_names: Vec<String>,
    pub failed: Vec<(String, Error)>,
}

/// Field types that can't be imported.
fn is_skipped_type(fieldtype: Option<FieldType>) -> bool {
    matches!(
        fieldtype,
        Some(FieldType::AttachImage | FieldType::Attachment | FieldType::Table)
    )
}

/// `importableSchemaNames` in ImportWizard.vue.
pub fn importable_schema_names(fyo: &Fyo) -> Vec<&'static str> {
    let mut names = vec![
        "SalesInvoice",
        "PurchaseInvoice",
        "Payment",
        "Party",
        "Item",
        "JournalEntry",
        "Tax",
        "Account",
        "Address",
        "NumberSeries",
    ];
    if crate::models::helpers::single_is_truthy(fyo, "AccountingSettings", "enableInventory") {
        names.extend(["StockMovement", "Shipment", "PurchaseReceipt", "Location"]);
    }
    names
}

pub struct Importer {
    pub schema_name: String,
    /// The template field each column is for, if any.
    pub assigned: Vec<Option<String>>,
    pub fields: IndexMap<String, TemplateField>,
    pub picked: IndexMap<String, bool>,
    pub has_child_tables: bool,
    pub rows: Vec<Vec<Cell>>,
    /// Each option field's values, and its values by label.
    options: HashMap<String, (Vec<String>, HashMap<String, String>)>,
}

impl Importer {
    pub fn new(schema_name: &str, fyo: &Fyo) -> Result<Self, Error> {
        if !fyo.db.schema_map().contains_key(schema_name) {
            return Err(Error::Validation(format!(
                "Invalid schemaName {schema_name} found in importer"
            )));
        }
        let (fields, has_child_tables) = template_fields(schema_name, fyo);
        let mut importer = Self {
            schema_name: schema_name.to_owned(),
            assigned: fields.iter().map(|f| Some(f.field_key.clone())).collect(),
            fields: IndexMap::new(),
            picked: IndexMap::new(),
            has_child_tables,
            rows: Vec::new(),
            options: HashMap::new(),
        };
        for field in fields {
            importer.picked.insert(field.field_key.clone(), true);
            importer.fields.insert(field.field_key.clone(), field);
        }
        Ok(importer)
    }

    pub fn is_submittable(&self, fyo: &Fyo) -> bool {
        fyo.db
            .schema_map()
            .get(&self.schema_name)
            .is_some_and(|s| s.is_submittable)
    }

    /// `selectFile`: reads a CSV file's text. False if it can't be read.
    pub fn select_file(&mut self, fyo: &Fyo, text: &str) -> bool {
        let parsed = parse_csv(text);
        self.select_parsed(fyo, &parsed);
        true
    }

    /// `selectParsed`: a row of field keys in the first four rows sets the columns, and
    /// the rows after it are the values. Without one, every row is values.
    fn select_parsed(&mut self, fyo: &Fyo, parsed: &[Vec<String>]) {
        if parsed.is_empty() {
            return;
        }
        let mut start = None;
        for i in (0..=3).rev() {
            let Some(row) = parsed.get(i) else {
                continue;
            };
            if row.is_empty() {
                continue;
            }
            if self.assign_from_row(row) {
                start = Some(i + 1);
                break;
            }
        }
        // Columns past the end of the file are for no field.
        let width = match start {
            Some(start) => parsed[start - 1].len(),
            None => {
                self.assigned.iter_mut().for_each(|key| *key = None);
                parsed[0].len()
            }
        };
        self.assigned.resize(width, None);
        for row in &parsed[start.unwrap_or(0)..] {
            self.push_row(fyo, row);
        }
    }

    /// `assignTemplateFieldsFromParsedRow`.
    fn assign_from_row(&mut self, row: &[String]) -> bool {
        if !row.iter().any(|key| self.fields.contains_key(key)) {
            return false;
        }
        for (i, value) in row.iter().enumerate() {
            let key = (self.fields.contains_key(value) && self.picked.get(value) == Some(&true))
                .then(|| value.clone());
            match self.assigned.get_mut(i) {
                Some(assigned) => *assigned = key,
                None => self.assigned.push(key),
            }
        }
        true
    }

    /// `pushToValueMatrixFromParsedRow`.
    fn push_row(&mut self, fyo: &Fyo, row: &[String]) {
        let mut cells = Vec::new();
        for (i, raw) in row.iter().enumerate() {
            if i >= self.assigned.len() {
                self.assigned.push(None);
            }
            cells.push(self.cell(fyo, i, raw));
        }
        self.rows.push(cells);
    }

    /// `setTemplateField`: sets the field a column is for, and converts its cells again.
    pub fn set_template_field(&mut self, fyo: &Fyo, index: usize, key: Option<String>) {
        match self.assigned.get_mut(index) {
            Some(assigned) => *assigned = key,
            None => self.assigned.push(key),
        }
        for r in 0..self.rows.len() {
            let raw = self.rows[r]
                .get(index)
                .map(|c| c.raw.clone())
                .unwrap_or_default();
            let cell = self.cell(fyo, index, &raw);
            let row = &mut self.rows[r];
            if index >= row.len() {
                row.resize(index + 1, Cell::default());
            }
            row[index] = cell;
        }
    }

    /// Sets a cell's text, as editing it in the wizard does.
    pub fn set_cell(&mut self, fyo: &Fyo, row: usize, index: usize, raw: &str) {
        let cell = self.cell(fyo, index, raw);
        if let Some(row) = self.rows.get_mut(row) {
            if index >= row.len() {
                row.resize(index + 1, Cell::default());
            }
            row[index] = cell;
        }
    }

    /// `getValueMatrixItem`.
    fn cell(&mut self, fyo: &Fyo, index: usize, raw: &str) -> Cell {
        let mut cell = Cell {
            raw: raw.to_owned(),
            ..Cell::default()
        };
        let Some(key) = self.assigned.get(index).cloned().flatten() else {
            return cell;
        };
        let Some(tf) = self.fields.get(&key) else {
            return cell;
        };
        if raw.is_empty() {
            cell.value = Some(DocValue::Null);
            return cell;
        }
        if let Some(options) = &tf.field.options {
            return self.option_cell(cell, &key, options.clone());
        }
        match latte_db::to_doc_value(&RawValue::Text(raw.to_owned()), &tf.field, fyo.db.pesa()) {
            Ok(value) => cell.value = Some(value),
            Err(_) => cell.error = true,
        }
        cell
    }

    /// `getOptionFieldVmi`: an option's value, or the value of the option with that label.
    fn option_cell(
        &mut self,
        mut cell: Cell,
        key: &str,
        options: Vec<latte_schema::SelectOption>,
    ) -> Cell {
        if options.is_empty() {
            cell.value = Some(DocValue::Null);
            return cell;
        }
        let (values, by_label) = self.options.entry(key.to_owned()).or_insert_with(|| {
            (
                options.iter().map(|o| o.value.clone()).collect(),
                options
                    .iter()
                    .map(|o| (o.label.clone(), o.value.clone()))
                    .collect(),
            )
        });
        if values.contains(&cell.raw) {
            cell.value = Some(DocValue::Text(cell.raw.clone()));
        } else if let Some(value) = by_label.get(&cell.raw) {
            cell.value = Some(DocValue::Text(value.clone()));
        } else {
            cell.value = Some(DocValue::Null);
            cell.error = true;
        }
        cell
    }

    /// `addRow`: an empty row.
    pub fn add_row(&mut self) {
        let cells = self
            .assigned
            .iter()
            .map(|key| Cell {
                value: key.as_ref().map(|_| DocValue::Null),
                ..Cell::default()
            })
            .collect();
        self.rows.push(cells);
    }

    pub fn remove_row(&mut self, index: usize) {
        if index < self.rows.len() {
            self.rows.remove(index);
        }
    }

    /// The template fields assigned to columns, with their column.
    fn assigned_fields(&self) -> Vec<(usize, &TemplateField)> {
        self.assigned
            .iter()
            .enumerate()
            .filter_map(|(i, key)| Some((i, self.fields.get(key.as_ref()?)?)))
            .collect()
    }

    /// The labels of fields assigned to more than one column.
    pub fn duplicates(&self) -> Vec<String> {
        let mut seen: Vec<&str> = Vec::new();
        let mut duplicates = Vec::new();
        for (_, tf) in self.assigned_fields() {
            if seen.contains(&tf.field_key.as_str()) {
                let label = tf.column_label();
                if !duplicates.contains(&label) {
                    duplicates.push(label);
                }
            } else {
                seen.push(&tf.field_key);
            }
        }
        duplicates
    }

    /// `requiredNotSelected`.
    pub fn required_not_selected(&self) -> Vec<String> {
        self.fields
            .values()
            .filter(|tf| {
                tf.field.required
                    && !self.assigned.contains(&Some(tf.field_key.clone()))
                    && tf.parent_child_field.as_ref().is_none_or(|t| t.required)
            })
            .map(TemplateField::column_label)
            .collect()
    }

    /// `errorMessage`.
    pub fn error_message(&self) -> Option<String> {
        let duplicates = self.duplicates();
        if !duplicates.is_empty() {
            return Some(format!(
                "Duplicate columns found: {}",
                duplicates.join(", ")
            ));
        }
        let required = self.required_not_selected();
        if !required.is_empty() {
            return Some(format!(
                "Required fields not selected: {}",
                required.join(", ")
            ));
        }
        None
    }

    /// `checkCellErrors`.
    pub fn cell_errors(&self) -> Vec<String> {
        let assigned = self.assigned_fields();
        let mut errors = Vec::new();
        for (i, row) in self.rows.iter().enumerate() {
            for (index, tf) in &assigned {
                if row.get(*index).is_some_and(|c| c.error) {
                    errors.push(format!("(Row {}, {})", i + 1, tf.column_label()));
                }
            }
        }
        errors
    }

    /// `checkLinks`: the linked documents that don't exist.
    pub fn check_links(&self, fyo: &Fyo) -> Result<Vec<AbsentLink>, Error> {
        let mut names: IndexMap<String, Vec<String>> = IndexMap::new();
        for row in &self.rows {
            for (index, tf) in self.assigned_fields() {
                if tf.field.fieldtype != Some(FieldType::Link) {
                    continue;
                }
                let Some(target) = &tf.field.target else {
                    continue;
                };
                let Some(Cell {
                    value: Some(DocValue::Text(value)),
                    ..
                }) = row.get(index)
                else {
                    continue;
                };
                if value.is_empty() {
                    continue;
                }
                let values = names.entry(target.clone()).or_default();
                if !values.contains(value) {
                    values.push(value.clone());
                }
            }
        }

        let mut absent = Vec::new();
        for (target, values) in names {
            for name in values {
                if fyo.db.exists(&target, Some(&name))? {
                    continue;
                }
                let schema_label = fyo
                    .db
                    .schema_map()
                    .get(&target)
                    .map_or_else(|| target.clone(), |s| s.label.clone());
                absent.push(AbsentLink { schema_label, name });
            }
        }
        Ok(absent)
    }

    /// `preImportValidations`: why the values can't be imported, if they can't.
    pub fn check(&self, fyo: &Fyo) -> Result<Option<String>, Error> {
        if let Some(message) = self.error_message() {
            return Ok(Some(message));
        }
        let errors = self.cell_errors();
        if !errors.is_empty() {
            return Ok(Some(format!(
                "Following cells have errors: {}.",
                errors.join(", ")
            )));
        }
        let absent = self.check_links(fyo)?;
        if !absent.is_empty() {
            let links: Vec<String> = absent
                .iter()
                .map(|l| format!("({}, {})", l.schema_label, l.name))
                .collect();
            return Ok(Some(format!(
                "Following links do not exist: {}.",
                links.join(", ")
            )));
        }
        Ok(None)
    }

    /// `getDataAndChildTableMapFromValueMatrix`: each document's values by the name in
    /// the file, and its rows by child schema and row name.
    #[allow(clippy::type_complexity)]
    fn data_and_children(
        &self,
    ) -> (
        IndexMap<String, DocValueMap>,
        HashMap<String, IndexMap<String, IndexMap<String, DocValueMap>>>,
    ) {
        let mut data: IndexMap<String, DocValueMap> = IndexMap::new();
        let mut children: HashMap<String, IndexMap<String, IndexMap<String, DocValueMap>>> =
            HashMap::new();

        let mut name_indices: HashMap<&str, usize> = HashMap::new();
        for (i, key) in self.assigned.iter().enumerate() {
            if let Some(key) = key
                && let Some(schema_name) = key.strip_suffix(".name")
            {
                name_indices.insert(schema_name, i);
            }
        }
        let Some(&name_index) = name_indices.get(self.schema_name.as_str()) else {
            return (data, children);
        };

        for (i, row) in self.rows.iter().enumerate() {
            let Some(Cell {
                value: Some(DocValue::Text(name)),
                ..
            }) = row.get(name_index)
            else {
                continue;
            };
            for (j, cell) in row.iter().enumerate() {
                let Some(tf) = self
                    .assigned
                    .get(j)
                    .cloned()
                    .flatten()
                    .and_then(|k| self.fields.get(&k))
                else {
                    continue;
                };
                if !cell.has_value() {
                    continue;
                }
                let value = cell.value.clone().unwrap_or(DocValue::Null);
                if tf.parent_child_field.is_none() {
                    data.entry(name.clone())
                        .or_default()
                        .insert(tf.field.fieldname.clone(), value);
                    continue;
                }

                let child_name = name_indices
                    .get(tf.schema_name.as_str())
                    .and_then(|&index| match row.get(index) {
                        Some(Cell {
                            value: Some(DocValue::Text(name)),
                            ..
                        }) => Some(name.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| format!("{}-{i}", tf.schema_name));
                children
                    .entry(name.clone())
                    .or_default()
                    .entry(tf.schema_name.clone())
                    .or_default()
                    .entry(child_name)
                    .or_default()
                    .insert(tf.field.fieldname.clone(), value);
            }
        }
        (data, children)
    }

    /// `populateDocs`: the documents the rows make.
    pub fn docs(&self, fyo: &mut Fyo) -> Result<Vec<Doc>, Error> {
        let (data, mut children) = self.data_and_children();
        let tables: Vec<(String, String)> = fyo
            .db
            .schema_map()
            .get(&self.schema_name)
            .map(|schema| {
                schema
                    .fields
                    .iter()
                    .filter(|f| f.fieldtype == Some(FieldType::Table))
                    .filter_map(|f| Some((f.target.clone()?, f.fieldname.clone())))
                    .collect()
            })
            .unwrap_or_default();

        let mut docs = Vec::new();
        for (name, values) in data {
            let mut doc = fyo.get_new_doc(&self.schema_name, values)?;
            let mut rows = children.remove(&name).unwrap_or_default();
            for (target, fieldname) in &tables {
                let Some(rows) = rows.shift_remove(target) else {
                    continue;
                };
                for row in rows.into_values() {
                    doc.push_row(fyo, fieldname, row)?;
                }
            }
            docs.push(doc);
        }
        Ok(docs)
    }

    /// `importData`: saves each document the rows make, and submits it if asked.
    pub fn import(&self, fyo: &mut Fyo, submit: bool) -> Result<ImportResult, Error> {
        let mut result = ImportResult::default();
        for doc in self.docs(fyo)? {
            result.import_doc(fyo, doc, submit);
        }
        Ok(result)
    }
}

impl ImportResult {
    /// One document of `importData`'s loop: saved, and submitted if asked, with what came
    /// of it noted.
    pub fn import_doc(&mut self, fyo: &mut Fyo, mut doc: Doc, submit: bool) {
        let old_name = doc.name().unwrap_or_default().to_owned();
        let mut saved = doc.sync(fyo);
        if saved.is_ok() && submit {
            saved = doc.submit(fyo);
        }
        let name = doc.name().unwrap_or_default().to_owned();
        match saved {
            Ok(()) => {
                self.success.push(name);
                self.success_old_names.push(old_name);
            }
            Err(error) => self.failed.push((name, error)),
        }
    }
}

impl Importer {
    /// `clearSuccessfullyImportedEntries`: keeps only the rows whose document failed.
    pub fn keep_failed(&mut self, result: &ImportResult) {
        let name_key = format!("{}.name", self.schema_name);
        let Some(index) = self
            .assigned
            .iter()
            .position(|k| k.as_deref() == Some(&name_key))
        else {
            self.rows.clear();
            return;
        };
        self.rows.retain(|row| match row.get(index) {
            Some(Cell {
                value: Some(DocValue::Text(name)),
                ..
            }) => !result.success_old_names.contains(name),
            _ => false,
        });
    }

    /// `getCSVTemplate`: the picked fields' schema labels, labels and keys.
    pub fn csv_template(&self) -> String {
        let mut schema_labels = Vec::new();
        let mut labels = Vec::new();
        let mut keys = Vec::new();
        for (key, picked) in &self.picked {
            if !picked {
                continue;
            }
            let Some(tf) = self.fields.get(key) else {
                continue;
            };
            schema_labels.push(tf.schema_label.clone());
            labels.push(tf.label().to_owned());
            keys.push(tf.field_key.clone());
        }
        generate_csv(&[schema_labels, labels, keys])
    }
}

/// `getTemplateFields`: the fields of the schema and of its tables that can be imported.
fn template_fields(schema_name: &str, fyo: &Fyo) -> (Vec<TemplateField>, bool) {
    let schema_map = fyo.db.schema_map();
    let mut has_child_tables = false;
    let mut fields = Vec::new();
    let mut stack: Vec<(&Schema, Option<Field>)> = match schema_map.get(schema_name) {
        Some(schema) => vec![(schema, None)],
        None => Vec::new(),
    };
    while let Some((schema, parent_child_field)) = stack.pop() {
        for field in &schema.fields {
            if should_skip_field(field, schema) {
                continue;
            }
            if field.fieldtype == Some(FieldType::Table) {
                has_child_tables = true;
                if let Some(target) = field.target.as_ref().and_then(|t| schema_map.get(t)) {
                    stack.push((target, Some(field.clone())));
                }
            }
            if is_skipped_type(field.fieldtype) {
                continue;
            }

            let mut tf = field.clone();
            tf.read_only = false;
            if schema.is_child && tf.fieldname == "name" {
                tf.required = false;
            }
            // Books means to keep a child field required when its table is, but looks the
            // table up by the wrong key, so a child field never is. Required child fields
            // mostly have formulas, so Latte keeps that.
            if schema.is_child {
                tf.required = false;
            }
            fields.push(TemplateField {
                field: tf,
                schema_name: schema.name.clone(),
                schema_label: schema.label.clone(),
                field_key: format!("{}.{}", schema.name, field.fieldname),
                parent_child_field: parent_child_field.clone(),
            });
        }
    }
    (fields, has_child_tables)
}

/// `shouldSkipField`.
fn should_skip_field(field: &Field, schema: &Schema) -> bool {
    if field.computed || field.meta {
        return true;
    }
    if schema.naming == Some(Naming::NumberSeries) && field.fieldname == "name" {
        return false;
    }
    field.hidden || (field.read_only && !field.required)
}

/// `parseCSV`: rows split on CRLF, or on LF if that gives one row.
pub fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = split_csv_block(text, "\r\n");
    if rows.len() == 1 {
        rows = split_csv_block(text, "\n");
    }
    rows.iter().map(|line| split_csv_line(line)).collect()
}

/// `splitCsvBlock`: the lines, keeping line breaks inside quotes.
fn split_csv_block(text: &str, splitter: &str) -> Vec<String> {
    let mut text = text.to_owned();
    if !text.ends_with(splitter) {
        text.push_str(splitter);
    }
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut in_quotes = false;
    let mut rest = text.as_str();
    while let Some(c) = rest.chars().next() {
        if c == '"' {
            in_quotes = !in_quotes;
        }
        if !in_quotes && rest.starts_with(splitter) {
            lines.push(std::mem::take(&mut line));
            rest = &rest[splitter.len()..];
            continue;
        }
        line.push(c);
        rest = &rest[c.len_utf8()..];
    }
    lines
}

/// `splitCsvLine`.
fn split_csv_line(line: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut item = String::new();
    let mut in_quotes = false;
    for c in line.chars().chain([',']) {
        if c == '"' {
            in_quotes = !in_quotes;
        }
        if !in_quotes && c == ',' {
            let unwrapped = if item.len() >= 2 && item.starts_with('"') && item.ends_with('"') {
                &item[1..item.len() - 1]
            } else {
                item.as_str()
            };
            items.push(unwrapped.replace("\"\"", "\""));
            item.clear();
            continue;
        }
        item.push(c);
    }
    items
}

/// `generateCSV`.
pub fn generate_csv(matrix: &[Vec<String>]) -> String {
    matrix
        .iter()
        .map(|row| {
            row.iter()
                .map(|item| crate::reports::csv_item(item))
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>()
        .join("\r\n")
}

#[cfg(test)]
#[path = "importer_tests.rs"]
mod tests;
