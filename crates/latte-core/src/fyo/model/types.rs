// Port of fyo/model/types.ts @ v0.36.0.
//
// A model in Books is a subclass of `Doc` with maps of functions (`formulas`,
// `validations`, `required`, ...) that read the document through `this`. Here a model is a
// `Model`, kept per schema, whose methods are given the document. The maps become methods
// that take a fieldname and return `None` (or do nothing) for a field the map doesn't have.
//
// Not ported yet: `filters`, `createFilters`, `emptyMessages`, `getListViewSettings`,
// `getTreeSettings` and `getActions`, which the UI needs.

use latte_db::{DocValue, DocValueMap, QueryFilter};
use latte_schema::SelectOption;

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;

/// What a model's functions can read: the document, its parent if it is a child row
/// (`parentdoc` in Books), and the file.
#[derive(Clone, Copy)]
pub struct DocContext<'a> {
    pub doc: &'a Doc,
    pub parent: Option<&'a Doc>,
    pub fyo: &'a Fyo,
}

/// `FormulaReturn`, less `undefined` (which is `None`). A `DocValue::Table` is rows for a
/// table field, as `DocValueMap[]` in Books.
#[derive(Debug, Clone)]
pub enum FormulaValue {
    Value(DocValue),
    Docs(Vec<Doc>),
}

impl From<DocValue> for FormulaValue {
    fn from(value: DocValue) -> Self {
        Self::Value(value)
    }
}

impl From<Vec<DocValueMap>> for FormulaValue {
    fn from(rows: Vec<DocValueMap>) -> Self {
        Self::Value(DocValue::Table(rows))
    }
}

/// `Formula`: given the fieldname whose change set it off, if any.
pub type FormulaFn = fn(&DocContext<'_>, Option<&str>) -> Result<Option<FormulaValue>, Error>;

/// `FormulaConfig`, with the fieldname it is keyed by in a `FormulaMap`.
#[derive(Debug, Clone, Copy)]
pub struct Formula {
    pub fieldname: &'static str,
    pub depends_on: Option<&'static [&'static str]>,
    pub formula: FormulaFn,
}

/// A schema's model. Every method has the base `Doc`'s behaviour by default.
pub trait Model: Send + Sync {
    /// `static defaults`: the default for a new document's field.
    fn default(&self, _fieldname: &str, _doc: &Doc, _fyo: &Fyo) -> Option<DocValue> {
        None
    }

    /// `formulas`, in the order Books' `FormulaMap` lists them.
    fn formulas(&self) -> Vec<Formula> {
        Vec::new()
    }

    /// `validations[fieldname](value)`, which is only called with a value that isn't
    /// null.
    fn validate_field(
        &self,
        _fieldname: &str,
        _value: &DocValue,
        _ctx: &DocContext<'_>,
    ) -> Result<(), Error> {
        Ok(())
    }

    /// `required[fieldname]()`.
    fn required(&self, _fieldname: &str, _ctx: &DocContext<'_>) -> bool {
        false
    }

    /// `hidden[fieldname]()`.
    fn hidden(&self, _fieldname: &str, _ctx: &DocContext<'_>) -> Option<bool> {
        None
    }

    /// `readOnly[fieldname]()`.
    fn read_only(&self, _fieldname: &str, _ctx: &DocContext<'_>) -> Option<bool> {
        None
    }

    /// `getCurrencies[fieldname]()`.
    fn currency(&self, _fieldname: &str, _ctx: &DocContext<'_>) -> Option<String> {
        None
    }

    /// `static lists[fieldname](doc)`: options for a field whose schema has none.
    fn list(&self, _fieldname: &str, _ctx: Option<&DocContext<'_>>) -> Option<Vec<SelectOption>> {
        None
    }

    /// `static filters[fieldname](doc)`: which documents a Link field may pick.
    fn link_filters(&self, _fieldname: &str, _ctx: &DocContext<'_>) -> Option<QueryFilter> {
        None
    }

    /// The `change` hook. `changed` is `None` after the values are loaded or saved.
    fn change(&self, _doc: &mut Doc, _fyo: &mut Fyo, _changed: Option<&str>) -> Result<(), Error> {
        Ok(())
    }

    /// `canDelete`, for a model that adds to the base `Doc`'s.
    fn can_delete(&self, _doc: &Doc) -> bool {
        true
    }

    /// `duplicate`: what a model changes on a copy, after the base copy is made.
    fn duplicate(&self, _doc: &mut Doc) {}

    fn validate(&self, _doc: &mut Doc, _fyo: &mut Fyo) -> Result<(), Error> {
        Ok(())
    }

    fn before_sync(&self, _doc: &mut Doc, _fyo: &mut Fyo) -> Result<(), Error> {
        Ok(())
    }

    fn after_sync(&self, _doc: &mut Doc, _fyo: &mut Fyo) -> Result<(), Error> {
        Ok(())
    }

    fn before_submit(&self, _doc: &mut Doc, _fyo: &mut Fyo) -> Result<(), Error> {
        Ok(())
    }

    fn after_submit(&self, _doc: &mut Doc, _fyo: &mut Fyo) -> Result<(), Error> {
        Ok(())
    }

    fn before_cancel(&self, _doc: &mut Doc, _fyo: &mut Fyo) -> Result<(), Error> {
        Ok(())
    }

    fn after_cancel(&self, _doc: &mut Doc, _fyo: &mut Fyo) -> Result<(), Error> {
        Ok(())
    }

    fn before_delete(&self, _doc: &mut Doc, _fyo: &mut Fyo) -> Result<(), Error> {
        Ok(())
    }

    fn after_delete(&self, _doc: &mut Doc, _fyo: &mut Fyo) -> Result<(), Error> {
        Ok(())
    }

    fn before_rename(
        &self,
        _doc: &mut Doc,
        _fyo: &mut Fyo,
        _old_name: &str,
        _new_name: &str,
    ) -> Result<(), Error> {
        Ok(())
    }

    fn after_rename(
        &self,
        _doc: &mut Doc,
        _fyo: &mut Fyo,
        _old_name: &str,
        _new_name: &str,
    ) -> Result<(), Error> {
        Ok(())
    }
}

/// The base `Doc`, for schemas without a model of their own.
pub struct BaseModel;

impl Model for BaseModel {}
