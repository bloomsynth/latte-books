// Port of the parts of fyo/index.ts and fyo/core/docHandler.ts @ v0.36.0 that documents
// need: the open file, the session user, the models, and making and loading documents.
//
// Not ported: the document cache and its observers (each `get_doc` loads the document
// afresh, and `Doc` has no `on` listeners), `fyo.singles`, telemetry, and the translation
// of `t` strings (they are English).

pub mod format;
pub mod model;
pub mod models;

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use latte_db::pesa::MoneyMaker;
use latte_db::{Database, DocValue, DocValueMap};
use latte_schema::{Naming, Schema};

use crate::Error;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::types::{BaseModel, Model};

/// `DEFAULT_USER` in fyo/utils/consts.ts.
pub const DEFAULT_USER: &str = "Admin";
/// `DEFAULT_SERIES_START` in fyo/utils/consts.ts.
pub const DEFAULT_SERIES_START: f64 = 1001.0;

/// `Fyo`: an open company file and what its documents need.
pub struct Fyo {
    pub db: Database,
    schemas: HashMap<String, Arc<Schema>>,
    models: HashMap<String, Arc<dyn Model>>,
    /// `fyo.auth.session.user`.
    user: String,
    temporary_name_counters: RefCell<HashMap<String, u32>>,
}

impl Fyo {
    /// Wraps an open file, with the core models registered and the base `Doc` for every
    /// other schema.
    pub fn new(db: Database) -> Self {
        let schemas = db
            .schema_map()
            .iter()
            .map(|(name, schema)| (name.clone(), Arc::new(schema.clone())))
            .collect();
        let mut fyo = Self {
            db,
            schemas,
            models: HashMap::new(),
            user: String::new(),
            temporary_name_counters: RefCell::new(HashMap::new()),
        };
        fyo.register_models(HashMap::new(), HashMap::new());
        fyo
    }

    /// Wraps an open file with every model Latte has registered, as the app does
    /// (`initializeAndRegister`).
    pub fn with_models(db: Database) -> Self {
        let mut fyo = Self::new(db);
        let regional_models = crate::models::regional_models(fyo.db.country_code());
        fyo.register_models(crate::models::models(), regional_models);
        fyo
    }

    /// `DocHandler.registerModels`: a core model wins over a regional one, which wins over
    /// an app model; schemas with none get the base `Doc`.
    pub fn register_models(
        &mut self,
        models: HashMap<String, Arc<dyn Model>>,
        regional_models: HashMap<String, Arc<dyn Model>>,
    ) {
        let core_models = models::core_models();
        for schema_name in self.schemas.keys() {
            let model = core_models
                .get(schema_name.as_str())
                .or_else(|| regional_models.get(schema_name))
                .or_else(|| models.get(schema_name))
                .cloned()
                .unwrap_or_else(|| Arc::new(BaseModel));
            self.models.insert(schema_name.clone(), model);
        }
    }

    /// The signed-in user, who is written as `createdBy` and `modifiedBy`.
    pub fn set_user(&mut self, user: impl Into<String>) {
        self.user = user.into();
    }

    /// `fyo.auth.session.user || DEFAULT_USER`.
    pub fn session_user(&self) -> &str {
        if self.user.is_empty() {
            DEFAULT_USER
        } else {
            &self.user
        }
    }

    /// `translateSchemaMap`: puts each schema's labels, descriptions, placeholders,
    /// sections, tabs and option labels into the language `translate` reads, leaving the
    /// ones it has no translation for.
    pub fn translate_schemas(&mut self, translate: &dyn Fn(&str) -> Option<String>) {
        let swap = |text: &mut String| {
            if let Some(translation) = translate(text) {
                *text = translation;
            }
        };
        for schema in self.schemas.values_mut() {
            let schema = Arc::make_mut(schema);
            swap(&mut schema.label);
            for field in &mut schema.fields {
                for text in [
                    &mut field.label,
                    &mut field.description,
                    &mut field.placeholder,
                    &mut field.section,
                    &mut field.tab,
                ]
                .into_iter()
                .flatten()
                {
                    swap(text);
                }
                for option in field.options.iter_mut().flatten() {
                    swap(&mut option.label);
                }
            }
        }
    }

    pub fn schema(&self, schema_name: &str) -> Option<&Arc<Schema>> {
        self.schemas.get(schema_name)
    }

    pub fn model(&self, schema_name: &str) -> Arc<dyn Model> {
        self.models
            .get(schema_name)
            .cloned()
            .unwrap_or_else(|| Arc::new(BaseModel))
    }

    /// `fyo.pesa`.
    pub fn pesa(&self) -> &MoneyMaker {
        self.db.pesa()
    }

    /// `DocHandler.getNewDoc`: a new document from `data`, whose values may be database
    /// values or document values. It gets a temporary name until it is saved.
    pub fn get_new_doc(&self, schema_name: &str, data: DocValueMap) -> Result<Doc, Error> {
        self.new_doc(schema_name, data, true)
    }

    pub(crate) fn new_doc(
        &self,
        schema_name: &str,
        data: DocValueMap,
        convert_to_doc_value: bool,
    ) -> Result<Doc, Error> {
        let schema = self
            .schema(schema_name)
            .cloned()
            .ok_or_else(|| Error::NotFound(format!("Schema not found for {schema_name}")))?;
        let model = self.model(schema_name);
        let mut doc = Doc::new(schema, model, data, self, convert_to_doc_value)?;
        if matches!(doc.get("name"), None | Some(DocValue::Null)) {
            let name = self.get_temporary_name(doc.schema());
            doc.set_name(DocValue::Text(name));
        }
        Ok(doc)
    }

    /// `DocHandler.getDoc`: loads a document. Without a name, loads the single
    /// `schema_name`.
    pub fn get_doc(&mut self, schema_name: &str, name: Option<&str>) -> Result<Doc, Error> {
        let name = name.unwrap_or(schema_name);
        let is_single = self.schema(schema_name).is_some_and(|s| s.is_single);
        if name == schema_name && !is_single {
            return Err(Error::Value(format!(
                "{schema_name} is not a Single Schema"
            )));
        }

        let data = DocValueMap::from([("name".to_owned(), DocValue::Text(name.to_owned()))]);
        let mut doc = self.new_doc(schema_name, data, true)?;
        doc.load(self)?;
        Ok(doc)
    }

    /// `fyo.getValue`: a field's stored value, or `None` if the document or the value
    /// doesn't exist. Without a name, reads the single `schema_name`.
    ///
    /// Books loads the document to read it, so a computed field would have its formula
    /// applied; this reads only stored values.
    pub fn get_value(
        &self,
        schema_name: &str,
        name: Option<&str>,
        fieldname: &str,
    ) -> Option<DocValue> {
        let name = name.unwrap_or(schema_name);
        let mut values = self.db.get_doc(schema_name, name).ok()?;
        values.shift_remove(fieldname)
    }

    /// `DocHandler.isTemporaryName`.
    pub fn is_temporary_name(&self, name: &str, schema: &Schema) -> bool {
        name.contains(&format!("New {} ", schema.label))
    }

    /// `DocHandler.getTemporaryName`: a random name, or "New <label> 01", "New <label> 02"
    /// and so on.
    pub fn get_temporary_name(&self, schema: &Schema) -> String {
        if schema.naming == Some(Naming::Random) {
            return latte_db::get_random_string();
        }

        let mut counters = self.temporary_name_counters.borrow_mut();
        let idx = counters.entry(schema.name.clone()).or_insert(1);
        let name = format!("New {} {:02}", schema.label, idx);
        *idx += 1;
        name
    }
}
