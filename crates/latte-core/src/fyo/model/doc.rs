// Port of fyo/model/doc.ts @ v0.36.0.
//
// A `Doc` keeps its values in a map, as Books keeps them in the document's properties: a
// missing key is `undefined`. Child rows are `Doc`s in their parent's tables. As they don't
// point back to their parent, what Books does through `parentdoc` is done from the parent:
// a child's value is set with `set_child`, and child formulas, validations and required
// checks get the parent in their `DocContext`.
//
// Deviations:
// - Nothing is async, and the file comes in as `&Fyo` or `&mut Fyo`.
// - Not ported: the document cache and observers, `links` (`loadLinks`, `getLink`,
//   `loadAndGetLink`), the ERPNext sync queue in `sync`, telemetry, `getDbSyncError` (a
//   database error is returned as it is), `quickEditFields` and `shouldDocSyncToERPNext`.
// - `set`'s `retriggerChildDocApplyChange` argument isn't exposed.
// - Formulas, computed values and the child row numbering don't go into a child row's own
//   tables. Books' child schemas have no tables.
// - Comparing a table's rows with lodash's `isEqual` becomes: rows given as maps never
//   equal documents unless both are empty, and documents are equal if their values are.
// - A Number Series name that is set but isn't text is an error when naming.

use std::fmt;
use std::sync::Arc;

use indexmap::IndexMap;
use jiff::Timestamp;
use latte_db::{DocValue, DocValueMap, convert_doc_value, to_doc_value_map, to_raw_value_map};
use latte_schema::{Field, FieldType, Naming, RawValue, Schema};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::helpers::{
    are_doc_values_equal, get_formula_sequence, get_missing_mandatory_message,
    get_pre_default_values, is_truthy, js_string, set_child_doc_idx, should_apply_formula,
    to_money,
};
use crate::fyo::model::naming::set_name;
use crate::fyo::model::types::{DocContext, FormulaValue, Model};

/// A field's value as Books reads `doc[fieldname]`.
#[derive(Debug, Clone, Copy)]
pub enum FieldRef<'a> {
    Undefined,
    Value(&'a DocValue),
    Table(&'a [Doc]),
}

/// Which document a formula or validation is for: the document itself, or one of its
/// child rows.
type Target<'a> = Option<(&'a str, usize)>;

#[derive(Clone)]
pub struct Doc {
    schema: Arc<Schema>,
    model: Arc<dyn Model>,
    values: IndexMap<String, DocValue>,
    tables: IndexMap<String, Vec<Doc>>,
    dirty: bool,
    not_inserted: bool,
    syncing: bool,
}

impl fmt::Debug for Doc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Doc")
            .field("schema", &self.schema.name)
            .field("values", &self.values)
            .field("tables", &self.tables)
            .field("dirty", &self.dirty)
            .field("not_inserted", &self.not_inserted)
            .finish()
    }
}

/// Documents are equal if their schema and values are.
impl PartialEq for Doc {
    fn eq(&self, other: &Self) -> bool {
        self.schema.name == other.schema.name
            && self.values == other.values
            && self.tables == other.tables
    }
}

impl Doc {
    /// The `Doc` constructor. Use [`Fyo::get_new_doc`] or [`Fyo::get_doc`] to make one.
    pub(crate) fn new(
        schema: Arc<Schema>,
        model: Arc<dyn Model>,
        data: DocValueMap,
        fyo: &Fyo,
        convert_to_doc_value: bool,
    ) -> Result<Self, Error> {
        let mut doc = Self {
            schema,
            model,
            values: IndexMap::new(),
            tables: IndexMap::new(),
            dirty: true,
            not_inserted: true,
            syncing: false,
        };
        if doc.schema.is_single {
            doc.set_name(DocValue::Text(doc.schema.name.clone()));
        }

        doc.set_defaults(fyo)?;
        doc.set_values_without_checks(fyo, &data, convert_to_doc_value)?;
        Ok(doc)
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    pub fn schema_name(&self) -> &str {
        &self.schema.name
    }

    pub fn model(&self) -> &Arc<dyn Model> {
        &self.model
    }

    /// The name, if it is set and is text.
    pub fn name(&self) -> Option<&str> {
        match self.values.get("name") {
            Some(DocValue::Text(name)) => Some(name),
            _ => None,
        }
    }

    pub(crate) fn set_name(&mut self, name: DocValue) {
        self.values.insert("name".into(), name);
    }

    /// `doc.get(fieldname)` for a field that isn't a table.
    pub fn get(&self, fieldname: &str) -> Option<&DocValue> {
        self.values.get(fieldname)
    }

    /// The rows of a table field.
    pub fn table(&self, fieldname: &str) -> &[Doc] {
        self.tables.get(fieldname).map_or(&[], Vec::as_slice)
    }

    /// `doc[fieldname]`, whether a table or not.
    pub fn field(&self, fieldname: &str) -> FieldRef<'_> {
        if let Some(rows) = self.tables.get(fieldname) {
            return FieldRef::Table(rows);
        }
        self.values
            .get(fieldname)
            .map_or(FieldRef::Undefined, FieldRef::Value)
    }

    /// Sets a property without any of `set`'s checks, as Books does with `doc[f] = value`.
    pub fn set_value(&mut self, fieldname: &str, value: DocValue) {
        self.values.insert(fieldname.to_owned(), value);
    }

    /// Replaces a table's rows without any of `set`'s checks, as Books does with
    /// `doc[tablefield] = rows`.
    pub fn set_table_rows(&mut self, fieldname: &str, rows: Vec<Doc>) {
        self.insert_table(fieldname, rows);
    }

    pub fn not_inserted(&self) -> bool {
        self.not_inserted
    }

    pub fn inserted(&self) -> bool {
        !self.not_inserted
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn is_syncing(&self) -> bool {
        self.syncing
    }

    pub fn table_fields(&self) -> impl Iterator<Item = &Field> {
        self.schema
            .fields
            .iter()
            .filter(|f| f.fieldtype == Some(FieldType::Table))
    }

    fn truthy(&self, fieldname: &str) -> bool {
        is_truthy(self.values.get(fieldname))
    }

    pub fn is_submitted(&self) -> bool {
        self.truthy("submitted") && !self.truthy("cancelled")
    }

    pub fn is_cancelled(&self) -> bool {
        self.truthy("submitted") && self.truthy("cancelled")
    }

    pub fn can_delete(&self) -> bool {
        if self.not_inserted || self.schema.is_single || self.schema.is_child {
            return false;
        }
        !self.schema.is_submittable || self.is_cancelled() || !self.is_submitted()
    }

    pub fn can_edit(&self) -> bool {
        !self.schema.is_submittable || !(self.truthy("submitted") || self.truthy("cancelled"))
    }

    pub fn can_save(&self) -> bool {
        let submittable = self.schema.is_submittable;
        if submittable && (self.truthy("submitted") || self.truthy("cancelled")) {
            return false;
        }
        self.dirty && !self.schema.is_child
    }

    pub fn can_submit(&self) -> bool {
        self.schema.is_submittable
            && !self.dirty
            && !self.not_inserted
            && !self.truthy("submitted")
            && !self.truthy("cancelled")
    }

    pub fn can_cancel(&self) -> bool {
        self.schema.is_submittable
            && !self.dirty
            && !self.not_inserted
            && !self.truthy("cancelled")
            && self.truthy("submitted")
    }

    fn context<'a>(&'a self, fyo: &'a Fyo) -> DocContext<'a> {
        DocContext {
            doc: self,
            parent: None,
            fyo,
        }
    }

    /// The document a target names, and its parent if it is a child row.
    fn resolve(&self, target: Target<'_>) -> Result<(&Doc, Option<&Doc>), Error> {
        match target {
            None => Ok((self, None)),
            Some((fieldname, idx)) => self
                .tables
                .get(fieldname)
                .and_then(|rows| rows.get(idx))
                .map(|child| (child, Some(self)))
                .ok_or_else(|| no_row(fieldname, idx)),
        }
    }

    fn resolve_mut(&mut self, target: Target<'_>) -> Result<&mut Doc, Error> {
        match target {
            None => Ok(self),
            Some((fieldname, idx)) => self
                .tables
                .get_mut(fieldname)
                .and_then(|rows| rows.get_mut(idx))
                .ok_or_else(|| no_row(fieldname, idx)),
        }
    }

    fn set_values_without_checks(
        &mut self,
        fyo: &Fyo,
        data: &DocValueMap,
        convert_to_doc_value: bool,
    ) -> Result<(), Error> {
        let schema = self.schema.clone();
        for field in &schema.fields {
            let fieldname = &field.fieldname;
            let is_table = field.fieldtype == Some(FieldType::Table);
            match data.get(fieldname) {
                Some(DocValue::Table(rows)) => {
                    for row in rows {
                        self.push(
                            fyo,
                            fieldname,
                            ChildInput::Map(row.clone()),
                            convert_to_doc_value,
                        )?;
                    }
                }
                Some(DocValue::Number(n)) if field.fieldtype == Some(FieldType::Currency) => {
                    self.set_value(fieldname, DocValue::Money(fyo.pesa().from_f64(*n)?));
                }
                // A table set to anything but rows is empty.
                Some(_) if is_table => {
                    self.tables.insert(fieldname.clone(), Vec::new());
                }
                Some(value) if !convert_to_doc_value => self.set_value(fieldname, value.clone()),
                Some(value) => {
                    let value = convert_doc_value(value, field, fyo.pesa())?;
                    self.set_value(fieldname, value);
                }
                None if is_table => {}
                None => {
                    self.values
                        .entry(fieldname.clone())
                        .or_insert(DocValue::Null);
                }
            }

            if is_table {
                self.tables.entry(fieldname.clone()).or_default();
            }
        }
        Ok(())
    }

    fn set_dirty(&mut self, value: bool) {
        self.dirty = value;
    }

    /// `set`: sets a value, checks it, and applies the formulas and the `change` hook.
    /// Rows given for a table are added to it. Returns false if nothing changed.
    pub fn set(&mut self, fyo: &mut Fyo, fieldname: &str, value: DocValue) -> Result<bool, Error> {
        self.set_impl(fyo, fieldname, value, false)
    }

    fn set_impl(
        &mut self,
        fyo: &mut Fyo,
        fieldname: &str,
        value: DocValue,
        retrigger_child_doc_apply_change: bool,
    ) -> Result<bool, Error> {
        if !self.can_set(fieldname, &value) {
            return Ok(false);
        }

        self.set_dirty(true);
        self.assign(fyo, None, fieldname, value)?;
        self.apply_change(fyo, fieldname, retrigger_child_doc_apply_change)?;
        Ok(true)
    }

    /// `setMultiple`.
    pub fn set_multiple(&mut self, fyo: &mut Fyo, values: DocValueMap) -> Result<bool, Error> {
        let mut has_set = false;
        for (fieldname, value) in values {
            has_set |= self.set(fyo, &fieldname, value)?;
        }
        Ok(has_set)
    }

    /// `set` on a child row: the row's formulas and `change` hook run, then this
    /// document's, as Books runs them from the row's `parentdoc`.
    pub fn set_child(
        &mut self,
        fyo: &mut Fyo,
        table: &str,
        idx: usize,
        fieldname: &str,
        value: DocValue,
    ) -> Result<bool, Error> {
        let target = Some((table, idx));
        let child = self.resolve(target)?.0;
        if !child.can_set(fieldname, &value) {
            return Ok(false);
        }
        let parent_fieldname = match child.get("parentFieldname") {
            Some(DocValue::Text(f)) => f.clone(),
            _ => table.to_owned(),
        };

        self.resolve_mut(target)?.set_dirty(true);
        self.set_dirty(true);
        self.assign(fyo, target, fieldname, value)?;

        // `this._applyChange(fieldname)` on the row.
        self.apply_formula_for_fields(fyo, target, Some(fieldname))?;
        let child = self.resolve_mut(target)?;
        child.model.clone().change(child, fyo, Some(fieldname))?;

        self.apply_change(fyo, &parent_fieldname, false)?;
        Ok(true)
    }

    /// The middle of `set`: trims text, adds rows, or checks and assigns the value.
    fn assign(
        &mut self,
        fyo: &Fyo,
        target: Target<'_>,
        fieldname: &str,
        value: DocValue,
    ) -> Result<(), Error> {
        let value = match value {
            DocValue::Text(text) => DocValue::Text(text.trim().to_owned()),
            value => value,
        };

        if let DocValue::Table(rows) = value {
            let doc = self.resolve_mut(target)?;
            for row in rows {
                doc.push(fyo, fieldname, ChildInput::Map(row), false)?;
            }
            return Ok(());
        }

        let (doc, parent) = self.resolve(target)?;
        let field = doc
            .schema
            .field(fieldname)
            .expect("can_set checked the field");
        let ctx = DocContext { doc, parent, fyo };
        validate_field(field, &value, &ctx)?;

        let doc = self.resolve_mut(target)?;
        if doc.tables.contains_key(fieldname) {
            doc.tables.insert(fieldname.to_owned(), Vec::new());
        } else {
            doc.set_value(fieldname, value);
        }
        Ok(())
    }

    /// `_canSet`.
    fn can_set(&self, fieldname: &str, value: &DocValue) -> bool {
        if fieldname == "numberSeries" && !self.not_inserted {
            return false;
        }
        if self.schema.field(fieldname).is_none() {
            return false;
        }

        match (self.field(fieldname), value) {
            (FieldRef::Undefined, _) => true,
            (FieldRef::Table(rows), DocValue::Table(new_rows)) => {
                !(rows.is_empty() && new_rows.is_empty())
            }
            (FieldRef::Table(_), _) => true,
            (FieldRef::Value(current), value) => !are_doc_values_equal(current, Some(value)),
        }
    }

    /// `_applyChange`: the formulas, then the `change` hook.
    fn apply_change(
        &mut self,
        fyo: &mut Fyo,
        changed_fieldname: &str,
        retrigger_child_doc_apply_change: bool,
    ) -> Result<bool, Error> {
        self.apply_formula(
            fyo,
            Some(changed_fieldname),
            retrigger_child_doc_apply_change,
        )?;
        self.model
            .clone()
            .change(self, fyo, Some(changed_fieldname))?;
        Ok(true)
    }

    /// `_setDefaults`.
    fn set_defaults(&mut self, fyo: &Fyo) -> Result<(), Error> {
        let schema = self.schema.clone();
        let model = self.model.clone();
        for field in &schema.fields {
            if field.fieldtype == Some(FieldType::Table) {
                self.tables.insert(field.fieldname.clone(), Vec::new());
                continue;
            }

            let mut default = get_pre_default_values(field.fieldtype, fyo.pesa());
            if let Some(value) = model.default(&field.fieldname, self, fyo) {
                default = value;
            } else if let Some(value) = &field.default {
                default = schema_default(value);
            }

            if field.fieldtype == Some(FieldType::Currency)
                && !matches!(default, DocValue::Money(_))
            {
                default = DocValue::Money(to_money(&default, fyo.pesa())?);
            }
            self.set_value(&field.fieldname, default);
        }
        Ok(())
    }

    /// `remove`: takes out the row numbered `idx`.
    pub fn remove(&mut self, fyo: &mut Fyo, fieldname: &str, idx: usize) -> Result<bool, Error> {
        let mut rows = self.tables.shift_remove(fieldname).unwrap_or_default();
        rows.retain(|row| row.get("idx") != Some(&DocValue::Number(idx as f64)));
        set_child_doc_idx(&mut rows);
        self.insert_table(fieldname, rows);
        self.set_dirty(true);
        self.apply_change(fyo, fieldname, false)
    }

    /// `append`: adds a row and applies the change.
    pub fn append(
        &mut self,
        fyo: &mut Fyo,
        fieldname: &str,
        row: DocValueMap,
    ) -> Result<bool, Error> {
        self.push(fyo, fieldname, ChildInput::Map(row), false)?;
        self.set_dirty(true);
        self.apply_change(fyo, fieldname, false)
    }

    /// `push`: adds a row without applying the change.
    pub fn push_row(&mut self, fyo: &Fyo, fieldname: &str, row: DocValueMap) -> Result<(), Error> {
        self.push(fyo, fieldname, ChildInput::Map(row), false)
    }

    fn push(
        &mut self,
        fyo: &Fyo,
        fieldname: &str,
        row: ChildInput,
        convert_to_doc_value: bool,
    ) -> Result<(), Error> {
        let child = self.get_child_doc(fyo, row, fieldname, convert_to_doc_value)?;
        let mut rows = self.tables.shift_remove(fieldname).unwrap_or_default();
        rows.push(child);
        set_child_doc_idx(&mut rows);
        self.insert_table(fieldname, rows);
        Ok(())
    }

    /// Puts a table back where the schema has it, so the map keeps the schema's order.
    fn insert_table(&mut self, fieldname: &str, rows: Vec<Doc>) {
        let position = self
            .table_fields()
            .position(|f| f.fieldname == fieldname)
            .unwrap_or(usize::MAX)
            .min(self.tables.len());
        self.tables
            .shift_insert(position, fieldname.to_owned(), rows);
    }

    /// `_setChildDocsParent`.
    fn set_child_docs_parent(&mut self) {
        let name = self.values.get("name").cloned();
        for rows in self.tables.values_mut() {
            for child in rows {
                if child.truthy("parent") {
                    continue;
                }
                child.set_value("parent", name.clone().unwrap_or(DocValue::Null));
            }
        }
    }

    /// `_getChildDoc`.
    fn get_child_doc(
        &mut self,
        fyo: &Fyo,
        input: ChildInput,
        fieldname: &str,
        convert_to_doc_value: bool,
    ) -> Result<Doc, Error> {
        if !self.truthy("name") && self.schema.naming != Some(Naming::Manual) {
            let name = fyo.get_temporary_name(&self.schema);
            self.set_name(DocValue::Text(name));
        }

        let name = self.values.get("name").cloned();
        let meta = [
            ("parent", name),
            (
                "parentSchemaName",
                Some(DocValue::Text(self.schema.name.clone())),
            ),
            (
                "parentFieldname",
                Some(DocValue::Text(fieldname.to_owned())),
            ),
        ];
        let fill = |values: &mut IndexMap<String, DocValue>| {
            if matches!(values.get("name"), None | Some(DocValue::Null)) {
                values.insert("name".into(), DocValue::Text(latte_db::get_random_string()));
            }
            for (key, value) in &meta {
                if matches!(values.get(*key), None | Some(DocValue::Null))
                    && let Some(value) = value
                {
                    values.insert((*key).to_owned(), value.clone());
                }
            }
        };

        match input {
            ChildInput::Doc(mut child) => {
                fill(&mut child.values);
                Ok(*child)
            }
            ChildInput::Map(mut row) => {
                fill(&mut row);
                let target = self
                    .schema
                    .field(fieldname)
                    .and_then(|f| f.target.as_deref())
                    .unwrap_or_default();
                fyo.new_doc(target, row, convert_to_doc_value)
            }
        }
    }

    /// `_validateSync`.
    fn validate_sync(&self, fyo: &Fyo) -> Result<(), Error> {
        self.validate_mandatory(fyo)?;
        self.validate_fields(fyo)
    }

    /// `_validateMandatory`, for this document and its rows.
    fn validate_mandatory(&self, fyo: &Fyo) -> Result<(), Error> {
        let mut messages = vec![get_missing_mandatory_message(&self.context(fyo))];
        for field in self.table_fields() {
            for child in self.table(&field.fieldname) {
                messages.push(get_missing_mandatory_message(&DocContext {
                    doc: child,
                    parent: Some(self),
                    fyo,
                }));
            }
        }

        messages.retain(|m| !m.is_empty());
        if !messages.is_empty() {
            let fields = messages.join("\n");
            return Err(Error::Mandatory(format!("Value missing for {fields}")));
        }
        Ok(())
    }

    /// `_validateFields`: every field but tables, of this document only.
    fn validate_fields(&self, fyo: &Fyo) -> Result<(), Error> {
        let ctx = self.context(fyo);
        for field in &self.schema.fields {
            if field.fieldtype == Some(FieldType::Table) {
                continue;
            }
            let value = self.values.get(&field.fieldname).unwrap_or(&DocValue::Null);
            validate_field(field, value, &ctx)?;
        }
        Ok(())
    }

    /// `getValidDict`: the values of the schema's fields, with rows as maps.
    pub fn get_valid_dict(&self, filter_meta: bool, filter_computed: bool) -> DocValueMap {
        let mut data = DocValueMap::new();
        for field in &self.schema.fields {
            if (filter_meta && field.meta) || (filter_computed && field.computed) {
                continue;
            }

            let value = match self.field(&field.fieldname) {
                FieldRef::Table(rows) => DocValue::Table(
                    rows.iter()
                        .map(|row| row.get_valid_dict(filter_meta, filter_computed))
                        .collect(),
                ),
                FieldRef::Value(DocValue::Money(money)) => DocValue::Money(money.copy()),
                FieldRef::Value(value) => value.clone(),
                FieldRef::Undefined => DocValue::Null,
            };
            if value == DocValue::Null && self.schema.is_single {
                continue;
            }
            data.insert(field.fieldname.clone(), value);
        }
        data
    }

    /// `_setBaseMetaValues`.
    fn set_base_meta_values(&mut self, fyo: &Fyo) {
        if self.schema.is_submittable {
            self.set_value("submitted", DocValue::Bool(false));
            self.set_value("cancelled", DocValue::Bool(false));
        }
        if !self.truthy("createdBy") {
            self.set_value("createdBy", DocValue::Text(fyo.session_user().to_owned()));
        }
        if !self.truthy("created") {
            self.set_value("created", DocValue::Date(Timestamp::now()));
        }
        self.update_modified_meta_values(fyo);
    }

    /// `_updateModifiedMetaValues`.
    fn update_modified_meta_values(&mut self, fyo: &Fyo) {
        self.set_value("modifiedBy", DocValue::Text(fyo.session_user().to_owned()));
        self.set_value("modified", DocValue::Date(Timestamp::now()));
    }

    /// `load`: reads the document's values from the file.
    pub fn load(&mut self, fyo: &mut Fyo) -> Result<(), Error> {
        let Some(name) = self.name().map(str::to_owned) else {
            return Ok(());
        };

        let not_found = || Error::NotFound(format!("Not Found: {} {name}", self.schema.name));
        let mut data = match fyo.db.get_doc(&self.schema.name, &name) {
            Ok(data) => data,
            Err(latte_db::Error::NotFound(_)) => return Err(not_found()),
            Err(e) => return Err(e.into()),
        };
        if self.schema.is_single && !is_truthy(data.get("name")) {
            data.insert("name".into(), DocValue::Text(name.clone()));
        }
        if !self.schema.is_single && !is_truthy(data.get("name")) {
            return Err(not_found());
        }

        self.sync_values(fyo, &data)?;
        self.set_dirty(false);
        self.not_inserted = false;
        Ok(())
    }

    /// A saved document with its computed values, loaded without the `change` hook that
    /// `load` runs, so a formula, which can't write to the file, can read one.
    pub(crate) fn read(fyo: &Fyo, schema_name: &str, name: &str) -> Result<Doc, Error> {
        let data = fyo.db.get_doc(schema_name, name)?;
        let mut doc = fyo.new_doc(schema_name, data, false)?;
        doc.set_computed_values_from_formulas(fyo)?;
        doc.dirty = false;
        doc.not_inserted = false;
        Ok(doc)
    }

    /// `_syncValues`.
    fn sync_values(&mut self, fyo: &mut Fyo, data: &DocValueMap) -> Result<(), Error> {
        self.clear_values();
        self.set_values_without_checks(fyo, data, false)?;
        self.set_computed_values_from_formulas(fyo)?;
        self.dirty = false;
        self.model.clone().change(self, fyo, None)
    }

    /// `_setComputedValuesFromFormulas`: computed fields get their formula's value, or
    /// null.
    fn set_computed_values_from_formulas(&mut self, fyo: &Fyo) -> Result<(), Error> {
        let schema = self.schema.clone();
        for field in &schema.fields {
            if field.fieldtype == Some(FieldType::Table) {
                for idx in 0..self.table(&field.fieldname).len() {
                    self.set_computed_values_for(fyo, Some((&field.fieldname, idx)))?;
                }
            }
            if field.computed {
                let value = self.value_from_formula(fyo, None, field, None)?;
                self.assign_formula_value(fyo, None, field, value)?;
            }
        }
        Ok(())
    }

    /// `_setComputedValuesFromFormulas` on a child row.
    fn set_computed_values_for(&mut self, fyo: &Fyo, target: Target<'_>) -> Result<(), Error> {
        let schema = self.resolve(target)?.0.schema.clone();
        for field in schema.fields.iter().filter(|f| f.computed) {
            let value = self.value_from_formula(fyo, target, field, None)?;
            self.assign_formula_value(fyo, target, field, value)?;
        }
        Ok(())
    }

    /// Sets a formula's result, or null if it gave none.
    fn assign_formula_value(
        &mut self,
        fyo: &Fyo,
        target: Target<'_>,
        field: &Field,
        value: Option<FormulaValue>,
    ) -> Result<(), Error> {
        let doc = self.resolve_mut(target)?;
        match value {
            Some(FormulaValue::Docs(rows)) => {
                let mut children = Vec::with_capacity(rows.len());
                for row in rows {
                    children.push(doc.get_child_doc(
                        fyo,
                        ChildInput::Doc(Box::new(row)),
                        &field.fieldname,
                        false,
                    )?);
                }
                doc.insert_table(&field.fieldname, children);
            }
            Some(FormulaValue::Value(DocValue::Table(rows))) => {
                let mut children = Vec::with_capacity(rows.len());
                for row in rows {
                    children.push(doc.get_child_doc(
                        fyo,
                        ChildInput::Map(row),
                        &field.fieldname,
                        false,
                    )?);
                }
                doc.insert_table(&field.fieldname, children);
            }
            Some(FormulaValue::Value(_)) | None if doc.tables.contains_key(&field.fieldname) => {
                doc.insert_table(&field.fieldname, Vec::new());
            }
            Some(FormulaValue::Value(value)) => doc.set_value(&field.fieldname, value),
            None => doc.set_value(&field.fieldname, DocValue::Null),
        }
        Ok(())
    }

    /// `_clearValues`.
    fn clear_values(&mut self) {
        let schema = self.schema.clone();
        for field in &schema.fields {
            if field.fieldtype == Some(FieldType::Table) {
                self.tables.insert(field.fieldname.clone(), Vec::new());
            } else {
                self.set_value(&field.fieldname, DocValue::Null);
            }
        }
        self.dirty = true;
        self.not_inserted = true;
    }

    /// `_setChildDocsIdx`.
    fn set_child_docs_idx(&mut self) {
        for rows in self.tables.values_mut() {
            set_child_doc_idx(rows);
        }
    }

    /// `_validateDbNotModified`: the document mustn't have been saved since it was loaded.
    fn validate_db_not_modified(&self, fyo: &Fyo) -> Result<(), Error> {
        let Some(name) = self.name().filter(|n| !n.is_empty()) else {
            return Ok(());
        };
        if self.not_inserted || self.schema.is_single {
            return Ok(());
        }

        let db_values = match fyo.db.get_doc(&self.schema.name, name) {
            Ok(values) => values,
            Err(latte_db::Error::NotFound(_)) => DocValueMap::new(),
            Err(e) => return Err(e.into()),
        };
        let iso = |value: Option<&DocValue>| match value {
            Some(DocValue::Date(date)) => latte_db::to_iso_string(*date),
            _ => "undefined".to_owned(),
        };
        let doc_modified = iso(self.values.get("modified"));
        let db_modified = iso(db_values.get("modified"));
        if doc_modified != db_modified {
            return Err(Error::Conflict(format!(
                "{} {name} has been modified after loading please reload entry. \
                 {db_modified}, {doc_modified}",
                self.schema.label
            )));
        }
        Ok(())
    }

    /// `runFormulas`.
    pub fn run_formulas(&mut self, fyo: &Fyo) -> Result<bool, Error> {
        self.apply_formula(fyo, None, false)
    }

    /// `_applyFormula`: the rows' formulas, then this document's.
    fn apply_formula(
        &mut self,
        fyo: &Fyo,
        changed_fieldname: Option<&str>,
        retrigger_child_doc_apply_change: bool,
    ) -> Result<bool, Error> {
        let mut changed = self.call_all_table_fields_apply_formula(fyo, changed_fieldname)?;
        changed = self.apply_formula_for_fields(fyo, None, changed_fieldname)? || changed;

        if changed && retrigger_child_doc_apply_change {
            self.call_all_table_fields_apply_formula(fyo, changed_fieldname)?;
            self.apply_formula_for_fields(fyo, None, changed_fieldname)?;
        }
        Ok(changed)
    }

    /// `_callAllTableFieldsApplyFormula`.
    fn call_all_table_fields_apply_formula(
        &mut self,
        fyo: &Fyo,
        changed_fieldname: Option<&str>,
    ) -> Result<bool, Error> {
        let mut changed = false;
        let tables: Vec<String> = self.tables.keys().cloned().collect();
        for fieldname in &tables {
            for idx in 0..self.table(fieldname).len() {
                changed =
                    self.apply_formula_for_fields(fyo, Some((fieldname, idx)), changed_fieldname)?
                        || changed;
            }
        }
        Ok(changed)
    }

    /// `_applyFormulaForFields`, for this document or one of its rows.
    fn apply_formula_for_fields(
        &mut self,
        fyo: &Fyo,
        target: Target<'_>,
        changed_fieldname: Option<&str>,
    ) -> Result<bool, Error> {
        let (doc, _) = self.resolve(target)?;
        let schema = doc.schema.clone();
        let formulas = doc.model.formulas();

        let mut changed = false;
        for fieldname in get_formula_sequence(&formulas) {
            let Some(field) = schema.field(fieldname) else {
                continue;
            };
            let formula = formulas
                .iter()
                .find(|f| f.fieldname == fieldname)
                .expect("the sequence holds only formula fields");
            let (doc, _) = self.resolve(target)?;
            if !should_apply_formula(field, formula, doc, changed_fieldname) {
                continue;
            }

            let Some(new_value) = self.value_from_formula(fyo, target, field, changed_fieldname)?
            else {
                continue;
            };
            let (doc, _) = self.resolve(target)?;
            let is_same = match (&new_value, doc.field(fieldname)) {
                (FormulaValue::Docs(rows), FieldRef::Table(current)) => rows.as_slice() == current,
                (FormulaValue::Value(DocValue::Table(rows)), FieldRef::Table(current)) => {
                    rows.is_empty() && current.is_empty()
                }
                (FormulaValue::Docs(_), _) => false,
                (FormulaValue::Value(_), FieldRef::Table(_)) => false,
                (FormulaValue::Value(value), FieldRef::Value(current)) => {
                    are_doc_values_equal(value, Some(current))
                }
                (FormulaValue::Value(value), FieldRef::Undefined) => {
                    are_doc_values_equal(value, None)
                }
            };
            if is_same {
                continue;
            }

            self.assign_formula_value(fyo, target, field, Some(new_value))?;
            changed = true;
        }
        Ok(changed)
    }

    /// `_getValueFromFormula`: `None` if the field has no formula or it failed.
    fn value_from_formula(
        &self,
        fyo: &Fyo,
        target: Target<'_>,
        field: &Field,
        changed_fieldname: Option<&str>,
    ) -> Result<Option<FormulaValue>, Error> {
        let (doc, parent) = self.resolve(target)?;
        let Some(formula) = doc
            .model
            .formulas()
            .into_iter()
            .find(|f| f.fieldname == field.fieldname)
        else {
            return Ok(None);
        };
        let ctx = DocContext { doc, parent, fyo };
        Ok((formula.formula)(&ctx, changed_fieldname).unwrap_or(None))
    }

    /// `_preSync`.
    fn pre_sync(&mut self, fyo: &mut Fyo) -> Result<(), Error> {
        self.set_child_docs_idx();
        self.set_child_docs_parent();
        self.apply_formula(fyo, None, false)?;
        self.validate_sync(fyo)?;
        self.model.clone().validate(self, fyo)
    }

    /// `_insert`.
    fn insert(&mut self, fyo: &mut Fyo) -> Result<(), Error> {
        self.set_base_meta_values(fyo);
        self.pre_sync(fyo)?;
        set_name(self, fyo)?;

        let valid_dict = self.get_valid_dict(false, true);
        let data = fyo.db.insert(&self.schema.name, &valid_dict)?;
        self.sync_values(fyo, &data)
    }

    /// `_update`.
    fn update(&mut self, fyo: &mut Fyo) -> Result<(), Error> {
        self.validate_db_not_modified(fyo)?;
        self.update_modified_meta_values(fyo);
        self.pre_sync(fyo)?;

        let data = self.get_valid_dict(false, true);
        fyo.db.update(&self.schema.name, &data)?;
        self.sync_values(fyo, &data)
    }

    /// `sync`: saves the document, inserting it if it is new.
    pub fn sync(&mut self, fyo: &mut Fyo) -> Result<(), Error> {
        self.syncing = true;
        let model = self.model.clone();
        model.before_sync(self, fyo)?;
        if self.not_inserted {
            self.insert(fyo)?;
        } else {
            self.update(fyo)?;
        }
        self.not_inserted = false;
        model.after_sync(self, fyo)?;
        self.syncing = false;
        Ok(())
    }

    /// `delete`. Does nothing if the document can't be deleted.
    pub fn delete(&mut self, fyo: &mut Fyo) -> Result<(), Error> {
        if !self.can_delete() {
            return Ok(());
        }

        let model = self.model.clone();
        model.before_delete(self, fyo)?;
        let name = self.name().unwrap_or_default().to_owned();
        fyo.db.delete(&self.schema.name, &name)?;
        model.after_delete(self, fyo)
    }

    /// `submit`. Does nothing if the document isn't submittable or is already submitted
    /// or cancelled.
    pub fn submit(&mut self, fyo: &mut Fyo) -> Result<(), Error> {
        if !self.schema.is_submittable || self.truthy("submitted") || self.truthy("cancelled") {
            return Ok(());
        }

        let model = self.model.clone();
        model.before_submit(self, fyo)?;
        self.set_and_sync_or_restore(fyo, "submitted")?;
        model.after_submit(self, fyo)
    }

    /// `cancel`. Does nothing unless the document is submitted and not cancelled.
    pub fn cancel(&mut self, fyo: &mut Fyo) -> Result<(), Error> {
        if !self.schema.is_submittable || !self.truthy("submitted") || self.truthy("cancelled") {
            return Ok(());
        }

        let model = self.model.clone();
        model.before_cancel(self, fyo)?;
        self.set_and_sync_or_restore(fyo, "cancelled")?;
        model.after_cancel(self, fyo)
    }

    /// Sets the `submitted` or `cancelled` flag and saves. If the save fails, the document
    /// is put back as it was. Books leaves the flag set, so a submitted document whose
    /// cancel failed passes for cancelled and can be deleted without being reverted.
    fn set_and_sync_or_restore(&mut self, fyo: &mut Fyo, fieldname: &str) -> Result<(), Error> {
        let before = self.clone();
        let result = self.set_and_sync(fyo, fieldname, DocValue::Bool(true));
        if result.is_err() {
            *self = before;
        }
        result
    }

    /// `rename`. Does nothing if the document is submitted.
    pub fn rename(&mut self, fyo: &mut Fyo, new_name: &str) -> Result<(), Error> {
        if self.truthy("submitted") {
            return Ok(());
        }

        let old_name = self.name().unwrap_or_default().to_owned();
        let model = self.model.clone();
        model.before_rename(self, fyo, &old_name, new_name)?;
        fyo.db.rename(&self.schema.name, &old_name, new_name)?;
        self.set_name(DocValue::Text(new_name.to_owned()));
        model.after_rename(self, fyo, &old_name, new_name)
    }

    /// `getSum` with `convertToFloat` false: the sum of a column of a table, as money.
    pub fn get_sum(
        &self,
        fyo: &Fyo,
        tablefield: &str,
        childfield: &str,
    ) -> Result<latte_db::pesa::Money, Error> {
        let mut sum = fyo.pesa().zero();
        for child in self.table(tablefield) {
            let value = match child.get(childfield) {
                None | Some(DocValue::Null) => DocValue::Number(0.0),
                Some(value) => value.clone(),
            };
            let money = to_money(&value, fyo.pesa()).map_err(|e| {
                Error::Value(format!(
                    "{e} value: '{}', fieldname: '{tablefield}', childfield: '{childfield}'",
                    js_string(Some(&value))
                ))
            })?;
            sum = sum.add(&money)?;
        }
        Ok(sum)
    }

    /// `setAndSync`.
    pub fn set_and_sync(
        &mut self,
        fyo: &mut Fyo,
        fieldname: &str,
        value: DocValue,
    ) -> Result<(), Error> {
        self.set(fyo, fieldname, value)?;
        self.sync(fyo)
    }

    /// `duplicate`: a new document with this one's values, less its meta values and names.
    pub fn duplicate(&self, fyo: &Fyo) -> Result<Doc, Error> {
        let mut update_map = self.get_valid_dict(true, true);
        for value in update_map.values_mut() {
            if let DocValue::Table(rows) = value {
                for row in rows {
                    row.shift_remove("name");
                }
            }
        }

        if self.truthy("numberSeries") {
            update_map.shift_remove("name");
        } else {
            let name = js_string(update_map.get("name"));
            update_map.insert("name".into(), DocValue::Text(format!("{name} CPY")));
        }

        let schema_map = fyo.db.schema_map();
        let raw = to_raw_value_map(schema_map, fyo.pesa(), &self.schema.name, &update_map)?;
        let data = to_doc_value_map(schema_map, fyo.pesa(), &self.schema.name, &raw)?;
        let mut doc = fyo.new_doc(&self.schema.name, data, false)?;
        let model = Arc::clone(&self.model);
        model.duplicate(&mut doc);
        Ok(doc)
    }
}

/// A row for a table: a map of values, or a document (`Doc | DocValueMap` in Books).
enum ChildInput {
    Map(DocValueMap),
    Doc(Box<Doc>),
}

/// `_validateField`: the options, whether required, then the model's validation.
fn validate_field(field: &Field, value: &DocValue, ctx: &DocContext<'_>) -> Result<(), Error> {
    if matches!(
        field.fieldtype,
        Some(FieldType::Select | FieldType::AutoComplete)
    ) {
        validate_options(field, value, ctx)?;
    }

    validate_required(field, value, ctx)?;
    if *value == DocValue::Null {
        return Ok(());
    }

    ctx.doc.model().validate_field(&field.fieldname, value, ctx)
}

/// `validateOptions` in fyo/model/validationFunction.ts.
fn validate_options(field: &Field, value: &DocValue, ctx: &DocContext<'_>) -> Result<(), Error> {
    let options = match &field.options {
        Some(options) if !options.is_empty() => options.clone(),
        _ => ctx
            .doc
            .model()
            .list(&field.fieldname, Some(ctx))
            .unwrap_or_default(),
    };
    if options.is_empty() {
        return Ok(());
    }

    if !field.required && !is_truthy(Some(value)) {
        return Ok(());
    }

    let is_valid =
        matches!(value, DocValue::Text(text) if options.iter().any(|o| &o.value == text));
    if is_valid || field.allow_custom {
        return Ok(());
    }

    Err(Error::Value(format!(
        "Invalid value {} for {}",
        js_string(Some(value)),
        field.label.as_deref().unwrap_or("undefined")
    )))
}

/// `validateRequired` in fyo/model/validationFunction.ts.
fn validate_required(field: &Field, value: &DocValue, ctx: &DocContext<'_>) -> Result<(), Error> {
    if *value != DocValue::Null {
        return Ok(());
    }

    if field.required || ctx.doc.model().required(&field.fieldname, ctx) {
        return Err(Error::Validation(format!(
            "{} is required",
            field.label.as_deref().unwrap_or("undefined")
        )));
    }
    Ok(())
}

/// A schema's default as Books assigns it: booleans, numbers and text as they are.
fn schema_default(value: &RawValue) -> DocValue {
    match value {
        RawValue::Bool(b) => DocValue::Bool(*b),
        RawValue::Number(n) => DocValue::Number(n.as_f64().unwrap_or(f64::NAN)),
        RawValue::Text(t) => DocValue::Text(t.clone()),
    }
}

fn no_row(fieldname: &str, idx: usize) -> Error {
    Error::NotFound(format!("{fieldname} has no row {idx}"))
}
