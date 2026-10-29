// Port of backend/database/core.ts @ v0.36.0: the read half (get, getAll, getSingleValues,
// exists) and the query builder. The write half is in write.rs.

use indexmap::IndexMap;
use latte_schema::{FieldType, Schema, SchemaMap};
use rusqlite::types::{ToSqlOutput, Value, ValueRef};
use rusqlite::{Connection, ToSql};

use crate::Error;

/// A value as SQLite returns it. `RawValue` in Books, where integers and reals are both
/// JavaScript numbers.
#[derive(Debug, Clone, PartialEq)]
pub enum RawValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
}

impl RawValue {
    fn from_sql(value: ValueRef<'_>) -> Self {
        match value {
            ValueRef::Null => Self::Null,
            ValueRef::Integer(i) => Self::Integer(i),
            ValueRef::Real(r) => Self::Real(r),
            ValueRef::Text(t) => Self::Text(String::from_utf8_lossy(t).into_owned()),
            // Books never stores blobs.
            ValueRef::Blob(b) => Self::Text(String::from_utf8_lossy(b).into_owned()),
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(t) => Some(t),
            _ => None,
        }
    }
}

impl ToSql for RawValue {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(match self {
            Self::Null => ToSqlOutput::Owned(Value::Null),
            Self::Integer(i) => ToSqlOutput::Owned(Value::Integer(*i)),
            Self::Real(r) => ToSqlOutput::Owned(Value::Real(*r)),
            Self::Text(t) => ToSqlOutput::Borrowed(ValueRef::Text(t.as_bytes())),
        })
    }
}

/// A column value, or the rows of a child table.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    Value(RawValue),
    Table(Vec<FieldValueMap>),
}

pub type FieldValueMap = IndexMap<String, FieldValue>;

/// A value to compare with in a filter.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterValue {
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
}

impl From<&str> for FilterValue {
    fn from(value: &str) -> Self {
        Self::Text(value.into())
    }
}

impl From<String> for FilterValue {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<bool> for FilterValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<f64> for FilterValue {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}

impl From<i64> for FilterValue {
    fn from(value: i64) -> Self {
        Self::Number(value as f64)
    }
}

impl ToSql for FilterValue {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Owned(match self {
            Self::Null => Value::Null,
            Self::Bool(b) => Value::Integer(*b as i64),
            Self::Number(n) if n.fract() == 0.0 && n.abs() < 9.0e15 => Value::Integer(*n as i64),
            Self::Number(n) => Value::Real(*n),
            Self::Text(t) => Value::Text(t.clone()),
        }))
    }
}

/// One filter on a field. Books writes `value` for `= value`, `[operator, value]`, or
/// `[operator, value, operator, value]` for two conditions on the same field.
#[derive(Debug, Clone, PartialEq)]
pub enum Filter {
    Equals(FilterValue),
    Op(String, FilterOperand),
    Two(String, FilterOperand, String, FilterOperand),
}

#[derive(Debug, Clone, PartialEq)]
pub enum FilterOperand {
    One(FilterValue),
    List(Vec<FilterValue>),
}

impl Filter {
    pub fn op(operator: &str, value: impl Into<FilterValue>) -> Self {
        Self::Op(operator.into(), FilterOperand::One(value.into()))
    }

    pub fn list(operator: &str, values: Vec<FilterValue>) -> Self {
        Self::Op(operator.into(), FilterOperand::List(values))
    }
}

impl From<FilterValue> for Filter {
    fn from(value: FilterValue) -> Self {
        Self::Equals(value)
    }
}

impl From<&str> for Filter {
    fn from(value: &str) -> Self {
        Self::Equals(value.into())
    }
}

impl From<bool> for Filter {
    fn from(value: bool) -> Self {
        Self::Equals(value.into())
    }
}

/// `QueryFilter` in Books: filters by field, all of which must hold.
pub type QueryFilter = IndexMap<String, Filter>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Default)]
pub struct GetAllOptions {
    /// Defaults to `name`. `*` selects every column.
    pub fields: Option<Vec<String>>,
    pub filters: QueryFilter,
    pub offset: Option<u64>,
    pub limit: Option<u64>,
    pub group_by: Vec<String>,
    /// Defaults to `created` when the schema has it.
    pub order_by: Option<Vec<String>>,
    /// Defaults to descending.
    pub order: Option<Order>,
}

/// A single value to fetch: `fieldname`, optionally only from `parent`.
#[derive(Debug, Clone)]
pub struct SingleValueQuery {
    pub fieldname: String,
    pub parent: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SingleValue {
    pub fieldname: String,
    pub parent: String,
    pub value: RawValue,
}

/// The ORM over one open company file.
pub struct DatabaseCore {
    pub(crate) conn: Connection,
    pub(crate) schema_map: SchemaMap,
    /// Set once the change log is on; writing needs it.
    pub(crate) device: Option<String>,
}

impl DatabaseCore {
    pub fn new(conn: Connection, schema_map: SchemaMap) -> Self {
        Self {
            conn,
            schema_map,
            device: None,
        }
    }

    pub fn schema_map(&self) -> &SchemaMap {
        &self.schema_map
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Gives the connection back, e.g. to open it as a [`crate::Database`] once its tables
    /// are made.
    pub fn into_connection(self) -> Connection {
        self.conn
    }

    pub(crate) fn schema(&self, schema_name: &str) -> Result<&Schema, Error> {
        self.schema_map
            .get(schema_name)
            .ok_or_else(|| Error::NotFound(format!("schema {schema_name} not found")))
    }

    pub fn exists(&self, schema_name: &str, name: Option<&str>) -> Result<bool, Error> {
        if self.schema(schema_name)?.is_single {
            return self.single_exists(schema_name);
        }

        let mut sql = format!("select * from {}", quote(schema_name));
        let mut params = Vec::new();
        if let Some(name) = name {
            sql.push_str(" where `name` = ?");
            params.push(FilterValue::from(name));
        }
        sql.push_str(" limit 1");
        match self.query(&sql, &params) {
            Ok(rows) => Ok(!rows.is_empty()),
            Err(e) if is_no_such_table(&e) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// One document's values, with its child tables. For a single, every stored value;
    /// `fields` is then ignored.
    pub fn get(
        &self,
        schema_name: &str,
        name: &str,
        fields: Option<&[&str]>,
    ) -> Result<FieldValueMap, Error> {
        let schema = self.schema(schema_name)?;
        if !schema.is_single && name.is_empty() {
            return Err(Error::Value("name is mandatory".into()));
        }

        if schema.is_single {
            return self.get_single(schema_name);
        }

        let fields: Vec<&str> = match fields {
            Some(fields) => fields.to_vec(),
            None => schema
                .fields
                .iter()
                .filter(|f| !f.computed)
                .map(|f| f.fieldname.as_str())
                .collect(),
        };

        let all_table_fields = table_fields(schema);
        let table_fields: Vec<(&str, &str)> = all_table_fields
            .iter()
            .copied()
            .filter(|(fieldname, _)| fields.contains(fieldname))
            .collect();
        let non_table_field_names: Vec<&str> = fields
            .iter()
            .copied()
            .filter(|f| !all_table_fields.iter().any(|(t, _)| t == f))
            .collect();

        let mut field_value_map = FieldValueMap::new();
        if !non_table_field_names.is_empty() {
            field_value_map = self
                .get_one(schema_name, name, &non_table_field_names)?
                .unwrap_or_default();
        }

        if !table_fields.is_empty() {
            self.load_children(name, &mut field_value_map, &table_fields)?;
        }
        Ok(field_value_map)
    }

    pub fn get_all(
        &self,
        schema_name: &str,
        options: &GetAllOptions,
    ) -> Result<Vec<FieldValueMap>, Error> {
        let schema = self.schema(schema_name)?;
        let has_created = schema.field("created").is_some();

        let default_fields = ["name".to_owned()];
        let fields = options.fields.as_deref().unwrap_or(&default_fields);
        let default_order_by = ["created".to_owned()];
        let order_by: &[String] = match &options.order_by {
            Some(order_by) => order_by,
            None if has_created => &default_order_by,
            None => &[],
        };
        let order = options.order.unwrap_or(Order::Desc);

        let mut sql = format!(
            "select {} from {}",
            fields
                .iter()
                .map(|f| quote_field(f))
                .collect::<Vec<_>>()
                .join(", "),
            quote(schema_name)
        );
        let mut params = Vec::new();
        apply_filters(&mut sql, &mut params, &options.filters);

        if !options.group_by.is_empty() {
            let columns: Vec<String> = options.group_by.iter().map(|c| quote(c)).collect();
            sql.push_str(&format!(" group by {}", columns.join(", ")));
        }

        if !order_by.is_empty() {
            let direction = match order {
                Order::Asc => "asc",
                Order::Desc => "desc",
            };
            let columns: Vec<String> = order_by
                .iter()
                .map(|c| format!("{} {direction}", quote(c)))
                .collect();
            sql.push_str(&format!(" order by {}", columns.join(", ")));
        }

        // As knex: a zero offset or limit is no offset or limit.
        match (
            options.limit.filter(|l| *l > 0),
            options.offset.filter(|o| *o > 0),
        ) {
            (Some(limit), Some(offset)) => sql.push_str(&format!(" limit {limit} offset {offset}")),
            (Some(limit), None) => sql.push_str(&format!(" limit {limit}")),
            (None, Some(offset)) => sql.push_str(&format!(" limit -1 offset {offset}")),
            (None, None) => {}
        }

        Ok(self
            .query(&sql, &params)?
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|(k, v)| (k, FieldValue::Value(v)))
                    .collect()
            })
            .collect())
    }

    pub fn get_single_values(
        &self,
        fieldnames: &[SingleValueQuery],
    ) -> Result<Vec<SingleValue>, Error> {
        let mut conditions = Vec::new();
        let mut params = Vec::new();
        for query in fieldnames {
            params.push(FilterValue::from(query.fieldname.as_str()));
            match &query.parent {
                Some(parent) => {
                    conditions.push("(`fieldname` = ? and `parent` = ?)");
                    params.push(FilterValue::from(parent.as_str()));
                }
                None => conditions.push("`fieldname` = ?"),
            }
        }

        let mut sql = "select `fieldname`, `value`, `parent` from `SingleValue`".to_owned();
        if !conditions.is_empty() {
            sql.push_str(" where ");
            sql.push_str(&conditions.join(" or "));
        }

        let rows = match self.query(&sql, &params) {
            Ok(rows) => rows,
            Err(e) if is_no_such_table(&e) => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        Ok(rows
            .into_iter()
            .map(|mut row| SingleValue {
                fieldname: take_text(&mut row, "fieldname"),
                parent: take_text(&mut row, "parent"),
                value: row.shift_remove("value").unwrap_or(RawValue::Null),
            })
            .collect())
    }

    fn single_exists(&self, single_schema_name: &str) -> Result<bool, Error> {
        let count: i64 = self.conn.query_row(
            "select count(`parent`) from `SingleValue` where `parent` = ?",
            [single_schema_name],
            |r| r.get(0),
        )?;
        Ok(count > 0)
    }

    fn load_children(
        &self,
        parent_name: &str,
        field_value_map: &mut FieldValueMap,
        table_fields: &[(&str, &str)],
    ) -> Result<(), Error> {
        for (fieldname, target) in table_fields {
            let options = GetAllOptions {
                fields: Some(vec!["*".into()]),
                filters: [("parent".to_owned(), Filter::from(parent_name))].into(),
                order_by: Some(vec!["idx".into()]),
                order: Some(Order::Asc),
                ..GetAllOptions::default()
            };
            let rows = self.get_all(target, &options)?;
            field_value_map.insert((*fieldname).to_owned(), FieldValue::Table(rows));
        }
        Ok(())
    }

    fn get_one(
        &self,
        schema_name: &str,
        name: &str,
        fields: &[&str],
    ) -> Result<Option<FieldValueMap>, Error> {
        let sql = format!(
            "select {} from {} where `name` = ? limit 1",
            fields
                .iter()
                .map(|f| quote_field(f))
                .collect::<Vec<_>>()
                .join(", "),
            quote(schema_name)
        );
        let row = self
            .query(&sql, &[FilterValue::from(name)])?
            .into_iter()
            .next();
        Ok(row.map(|row| {
            row.into_iter()
                .map(|(k, v)| (k, FieldValue::Value(v)))
                .collect()
        }))
    }

    fn get_single(&self, schema_name: &str) -> Result<FieldValueMap, Error> {
        let options = GetAllOptions {
            fields: Some(vec!["fieldname".into(), "value".into()]),
            filters: [("parent".to_owned(), Filter::from(schema_name))].into(),
            order_by: Some(vec!["fieldname".into()]),
            order: Some(Order::Asc),
            ..GetAllOptions::default()
        };
        let values = self.get_all("SingleValue", &options)?;

        let mut field_value_map = FieldValueMap::new();
        for mut row in values {
            let (Some(FieldValue::Value(RawValue::Text(fieldname))), Some(value)) =
                (row.shift_remove("fieldname"), row.shift_remove("value"))
            else {
                continue;
            };
            field_value_map.insert(fieldname, value);
        }

        let table_fields = table_fields(self.schema(schema_name)?);
        if !table_fields.is_empty() {
            self.load_children(schema_name, &mut field_value_map, &table_fields)?;
        }
        Ok(field_value_map)
    }

    pub(crate) fn query(
        &self,
        sql: &str,
        params: &[FilterValue],
    ) -> Result<Vec<IndexMap<String, RawValue>>, Error> {
        let mut statement = self.conn.prepare(sql)?;
        let columns: Vec<String> = statement.column_names().map(|c| c.to_string()).collect();
        let mut rows = statement.query(rusqlite::params_from_iter(params))?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let mut map = IndexMap::with_capacity(columns.len());
            for (i, column) in columns.iter().enumerate() {
                map.insert(column.clone(), RawValue::from_sql(row.get_ref(i)?));
            }
            out.push(map);
        }
        Ok(out)
    }
}

/// The schema's child table fields: (fieldname, target).
pub(crate) fn table_fields(schema: &Schema) -> Vec<(&str, &str)> {
    schema
        .fields
        .iter()
        .filter(|f| f.fieldtype == Some(FieldType::Table))
        .filter_map(|f| Some((f.fieldname.as_str(), f.target.as_deref()?)))
        .collect()
}

fn take_text(row: &mut IndexMap<String, RawValue>, key: &str) -> String {
    match row.shift_remove(key) {
        Some(RawValue::Text(t)) => t,
        _ => String::new(),
    }
}

pub(crate) fn is_no_such_table(e: &Error) -> bool {
    matches!(e, Error::Sqlite(e) if e.to_string().contains("no such table"))
}

/// Quotes an identifier as knex does for SQLite.
pub(crate) fn quote(identifier: &str) -> String {
    format!("`{}`", identifier.replace('`', "``"))
}

fn quote_field(field: &str) -> String {
    if field == "*" {
        field.to_owned()
    } else {
        quote(field)
    }
}

/// Port of `#applyFiltersToBuilder` and `#getFiltersArray`. Conditions are joined with
/// `and`, except that an `in` list holding `null` adds `or field is null` right after it,
/// as knex does, without brackets.
pub(crate) fn apply_filters(
    sql: &mut String,
    params: &mut Vec<FilterValue>,
    filters: &QueryFilter,
) {
    let mut filters_array: Vec<(&str, String, FilterOperand)> = Vec::new();
    for (field, value) in filters {
        let mut push = |operator: &str, operand: &FilterOperand| {
            let mut operator = operator.to_lowercase();
            let mut operand = operand.clone();
            if operator == "includes" {
                operator = "like".into();
            }
            if operator == "like"
                && let FilterOperand::One(FilterValue::Text(text)) = &operand
                && !text.contains('%')
            {
                operand = FilterOperand::One(FilterValue::Text(format!("%{text}%")));
            }
            filters_array.push((field.as_str(), operator, operand));
        };
        match value {
            Filter::Equals(v) => {
                filters_array.push((field.as_str(), "=".into(), FilterOperand::One(v.clone())))
            }
            Filter::Op(operator, operand) => push(operator, operand),
            Filter::Two(op1, operand1, op2, operand2) => {
                push(op1, operand1);
                // Books doesn't lower-case or rewrite the second operator.
                filters_array.push((field.as_str(), op2.clone(), operand2.clone()));
            }
        }
    }

    for (i, (field, operator, operand)) in filters_array.into_iter().enumerate() {
        sql.push_str(if i == 0 { " where " } else { " and " });
        let column = quote(field);
        match (operator.as_str(), operand) {
            ("in", FilterOperand::List(values)) if values.contains(&FilterValue::Null) => {
                // Books keeps only truthy values here, as `.filter(Boolean)`.
                let non_nulls: Vec<FilterValue> = values.into_iter().filter(is_truthy).collect();
                push_list(sql, params, &column, "in", non_nulls);
                sql.push_str(&format!(" or {column} is null"));
            }
            (op, FilterOperand::List(values)) => push_list(sql, params, &column, op, values),
            ("=" | "is", FilterOperand::One(FilterValue::Null)) => {
                sql.push_str(&format!("{column} is null"))
            }
            ("!=" | "<>" | "is not", FilterOperand::One(FilterValue::Null)) => {
                sql.push_str(&format!("{column} is not null"))
            }
            (op, FilterOperand::One(value)) => {
                sql.push_str(&format!("{column} {op} ?"));
                params.push(value);
            }
        }
    }
}

fn push_list(
    sql: &mut String,
    params: &mut Vec<FilterValue>,
    column: &str,
    operator: &str,
    values: Vec<FilterValue>,
) {
    if operator.ends_with("between") {
        sql.push_str(&format!("{column} {operator} ? and ?"));
        params.extend(values.into_iter().take(2));
        return;
    }
    if values.is_empty() {
        // knex writes an always-false (or, for `not in`, always-true) condition.
        sql.push_str(if operator == "not in" {
            "1 = 1"
        } else {
            "1 = 0"
        });
        return;
    }
    let marks = vec!["?"; values.len()].join(", ");
    sql.push_str(&format!("{column} {operator} ({marks})"));
    params.extend(values);
}

fn is_truthy(value: &FilterValue) -> bool {
    match value {
        FilterValue::Null => false,
        FilterValue::Bool(b) => *b,
        FilterValue::Number(n) => *n != 0.0 && !n.is_nan(),
        FilterValue::Text(t) => !t.is_empty(),
    }
}
