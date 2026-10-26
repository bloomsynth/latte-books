// How a form shows and reads a field, from src/utils/doc.ts (`evaluateHidden`,
// `evaluateReadOnly`, `evaluateRequired`), src/utils/ui.ts
// (`getFieldsGroupedByTabAndSection`) and the controls in src/components/Controls @ v0.36.0.
//
// Differences from Books:
// - A field the schema marks `hidden: false` (or `readOnly: false`) still asks the model.
//   Books' schemas never write those out, so this changes nothing in practice.
// - Dates are typed as YYYY-MM-DD; Books' browser input takes the computer's format. Both
//   open a calendar (`latte-ui`'s `date_picker.rs`).

use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use latte_db::{DocValue, Filter, FilterValue, GetAllOptions, QueryFilter};
use latte_schema::{Field, FieldType, SelectOption};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::format::{Formatter, local_date};
use crate::fyo::model::doc::Doc;
use crate::fyo::model::helpers::is_truthy;
use crate::fyo::model::types::DocContext;

/// The context a field of `doc` is evaluated in: `parent` is set for a child row.
pub fn context<'a>(fyo: &'a Fyo, doc: &'a Doc, parent: Option<&'a Doc>) -> DocContext<'a> {
    DocContext { doc, parent, fyo }
}

/// `evaluateHidden`.
pub fn is_hidden(field: &Field, ctx: &DocContext<'_>) -> bool {
    field.hidden
        || ctx
            .doc
            .model()
            .hidden(&field.fieldname, ctx)
            .unwrap_or(false)
}

/// `evaluateReadOnly`.
pub fn is_read_only(field: &Field, ctx: &DocContext<'_>) -> bool {
    let doc = ctx.doc;
    if doc.inserted() && field.fieldname == "numberSeries" {
        return true;
    }
    if field.fieldname == "name"
        && (doc.inserted() || doc.schema().naming != Some(latte_schema::Naming::Manual))
    {
        return true;
    }
    let parent_is = |check: fn(&Doc) -> bool| ctx.parent.is_some_and(check);
    if doc.is_submitted() || parent_is(Doc::is_submitted) {
        return true;
    }
    if doc.is_cancelled() || parent_is(Doc::is_cancelled) {
        return true;
    }
    field.read_only
        || doc
            .model()
            .read_only(&field.fieldname, ctx)
            .unwrap_or(false)
}

/// `evaluateRequired`.
pub fn is_required(field: &Field, ctx: &DocContext<'_>) -> bool {
    field.required || ctx.doc.model().required(&field.fieldname, ctx)
}

/// A form's tab: its sections, each with the fields shown in it.
#[derive(Debug, Clone)]
pub struct Tab {
    pub name: String,
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone)]
pub struct Section {
    pub name: String,
    pub fields: Vec<Field>,
}

/// `getFieldsGroupedByTabAndSection`: the fields a form shows, by tab and section, in
/// schema order. Empty tabs and sections are left out.
pub fn form_layout(fyo: &Fyo, doc: &Doc) -> Vec<Tab> {
    let ctx = context(fyo, doc, None);
    let mut tabs: Vec<Tab> = Vec::new();
    for field in &doc.schema().fields {
        let tab_name = field.tab.clone().unwrap_or_else(|| "Main".to_owned());
        let section_name = field
            .section
            .clone()
            .unwrap_or_else(|| "Default".to_owned());
        let tab = match tabs.iter().position(|tab| tab.name == tab_name) {
            Some(i) => &mut tabs[i],
            None => {
                tabs.push(Tab {
                    name: tab_name,
                    sections: Vec::new(),
                });
                tabs.last_mut().expect("just pushed")
            }
        };
        let section = match tab.sections.iter().position(|s| s.name == section_name) {
            Some(i) => &mut tab.sections[i],
            None => {
                tab.sections.push(Section {
                    name: section_name,
                    fields: Vec::new(),
                });
                tab.sections.last_mut().expect("just pushed")
            }
        };
        if field.meta || is_hidden(field, &ctx) {
            continue;
        }
        section.fields.push(field.clone());
    }
    for tab in &mut tabs {
        tab.sections.retain(|section| !section.fields.is_empty());
    }
    tabs.retain(|tab| !tab.sections.is_empty());
    tabs
}

/// The columns of a table field: the child schema's `tableFields`, as `Table.vue` shows.
pub fn table_columns(fyo: &Fyo, field: &Field) -> Vec<Field> {
    let Some(schema) = field.target.as_deref().and_then(|t| fyo.schema(t)) else {
        return Vec::new();
    };
    schema
        .table_fields
        .iter()
        .flatten()
        .filter_map(|fieldname| schema.field(fieldname).cloned())
        .collect()
}

/// A Select field's options: the schema's, or the model's `lists`.
pub fn select_options(field: &Field, ctx: &DocContext<'_>) -> Vec<SelectOption> {
    if let Some(options) = &field.options {
        return options.clone();
    }
    ctx.doc
        .model()
        .list(&field.fieldname, Some(ctx))
        .unwrap_or_default()
}

/// The schema a Link or DynamicLink field points to for this document.
pub fn link_target(field: &Field, ctx: &DocContext<'_>) -> Option<String> {
    match field.fieldtype {
        Some(FieldType::DynamicLink) => {
            let references = field.references.as_deref()?;
            match ctx.doc.get(references) {
                Some(DocValue::Text(target)) if !target.is_empty() => Some(target.clone()),
                _ => None,
            }
        }
        _ => field.target.clone(),
    }
}

/// `getFilters`: the filters a Link field's options must pass: the field's own
/// `filters`, else the model's `static filters`, which SystemSettings' `removeFilter`
/// turns off. (`getCreateFilters` is not applied: Books uses those only for creating
/// from the dropdown.)
pub fn link_filters(field: &Field, ctx: &DocContext<'_>) -> QueryFilter {
    let mut filters = QueryFilter::new();
    if let Some(own) = &field.filters {
        for (key, value) in own {
            filters.insert(
                key.clone(),
                Filter::Equals(FilterValue::Text(value.clone())),
            );
        }
        return filters;
    }
    if crate::models::helpers::single_is_truthy(ctx.fyo, "SystemSettings", "removeFilter") {
        return filters;
    }
    if let Some(model_filters) = ctx.doc.model().link_filters(&field.fieldname, ctx) {
        filters.extend(model_filters);
    }
    filters
}

/// A Link field's options as `(name, label)`, label being the target's title when it
/// has one other than its name. `getOptions` in `Link.vue`; ordered by name.
pub fn link_options(
    fyo: &Fyo,
    field: &Field,
    ctx: &DocContext<'_>,
) -> Result<Vec<(String, String)>, Error> {
    link_options_filtered(fyo, field, ctx, true)
}

/// [`link_options`], or with `filtered` false every document of the target, as after
/// "No results found, disable filters".
pub fn link_options_filtered(
    fyo: &Fyo,
    field: &Field,
    ctx: &DocContext<'_>,
    filtered: bool,
) -> Result<Vec<(String, String)>, Error> {
    let Some(target) = link_target(field, ctx) else {
        return Ok(Vec::new());
    };
    let Some(schema) = fyo.schema(&target) else {
        return Ok(Vec::new());
    };
    let title_field = schema
        .title_field
        .clone()
        .filter(|title| title != "name" && schema.field(title).is_some());
    let mut fields = vec!["name".to_owned()];
    fields.extend(title_field.clone());
    let filters = if filtered {
        link_filters(field, ctx)
    } else {
        QueryFilter::new()
    };
    let options = GetAllOptions {
        fields: Some(fields),
        filters: filters
            .into_iter()
            .filter(|(key, _)| schema.field(key).is_some())
            .collect(),
        order_by: Some(vec!["name".to_owned()]),
        order: Some(latte_db::Order::Asc),
        ..GetAllOptions::default()
    };
    let rows = fyo.db.get_all_docs(&target, &options)?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let name = match row.get("name") {
                Some(DocValue::Text(name)) => name.clone(),
                _ => return None,
            };
            let label = match title_field.as_deref().and_then(|t| row.get(t)) {
                Some(DocValue::Text(title)) if !title.is_empty() && *title != name => {
                    format!("{title} ({name})")
                }
                _ => name.clone(),
            };
            Some((name, label))
        })
        .collect())
}

/// `format(value, field)`: a value as a list or a read-only field shows it.
pub fn display(formatter: &Formatter, field: &Field, value: Option<&DocValue>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    match (field.fieldtype, value) {
        (_, DocValue::Null) => String::new(),
        (Some(FieldType::Check), value) => {
            if is_truthy(Some(value)) {
                "Yes".to_owned()
            } else {
                "No".to_owned()
            }
        }
        (Some(FieldType::Select), DocValue::Text(text)) => field
            .options
            .iter()
            .flatten()
            .find(|option| option.value == *text)
            .map_or_else(|| text.clone(), |option| option.label.clone()),
        (_, DocValue::Money(money)) => formatter.currency(money),
        (Some(FieldType::Int), DocValue::Number(n)) => format!("{}", n.trunc() as i64),
        (Some(FieldType::Float), DocValue::Number(n)) => formatter.float(*n),
        (_, DocValue::Number(n)) => crate::fyo::model::helpers::js_number_string(*n),
        (Some(FieldType::Datetime), DocValue::Date(ts)) => formatter.datetime(*ts),
        (_, DocValue::Date(ts)) => formatter.date(local_date(*ts)),
        (_, DocValue::Bool(b)) => if *b { "Yes" } else { "No" }.to_owned(),
        (_, DocValue::Text(text)) => text.clone(),
        (_, DocValue::Attachment(value)) => value
            .get("name")
            .and_then(|name| name.as_str())
            .unwrap_or_default()
            .to_owned(),
        (_, DocValue::Table(rows)) => format!("{} rows", rows.len()),
    }
}

/// A value as typed into its input: numbers plain, dates as YYYY-MM-DD.
pub fn edit_text(field: &Field, value: Option<&DocValue>) -> String {
    match value {
        None | Some(DocValue::Null) => String::new(),
        Some(DocValue::Money(money)) => {
            let text = money.round(None);
            trim_zeros(&text)
        }
        Some(DocValue::Number(n)) if field.fieldtype == Some(FieldType::Int) => {
            format!("{}", n.trunc() as i64)
        }
        Some(DocValue::Number(n)) => crate::fyo::model::helpers::js_number_string(*n),
        Some(DocValue::Date(ts)) if field.fieldtype == Some(FieldType::Datetime) => {
            let zoned = ts.to_zoned(TimeZone::system());
            format!("{} {}", zoned.date(), zoned.time().strftime("%H:%M"))
        }
        Some(DocValue::Date(ts)) => local_date(*ts).to_string(),
        Some(DocValue::Text(text)) => text.clone(),
        Some(DocValue::Bool(b)) => b.to_string(),
        Some(DocValue::Attachment(value)) => value.to_string(),
        Some(DocValue::Table(_)) => String::new(),
    }
}

fn trim_zeros(number: &str) -> String {
    match number.split_once('.') {
        Some((whole, fraction)) => {
            let fraction = fraction.trim_end_matches('0');
            if fraction.is_empty() {
                whole.to_owned()
            } else {
                format!("{whole}.{fraction}")
            }
        }
        None => number.to_owned(),
    }
}

/// Reads what was typed into a field's input. Empty text is no value.
pub fn parse(fyo: &Fyo, field: &Field, text: &str) -> Result<DocValue, Error> {
    let text = text.trim();
    let label = field.label.as_deref().unwrap_or(&field.fieldname);
    if text.is_empty() {
        return Ok(match field.fieldtype {
            Some(FieldType::Int | FieldType::Float) => DocValue::Number(0.0),
            Some(FieldType::Currency) => DocValue::Money(fyo.pesa().zero()),
            Some(FieldType::Check) => DocValue::Bool(false),
            _ => DocValue::Null,
        });
    }
    let number = |text: &str| -> Result<f64, Error> {
        text.replace(',', "")
            .parse::<f64>()
            .map_err(|_| Error::Value(format!("{label}: {text} is not a number")))
    };
    Ok(match field.fieldtype {
        Some(FieldType::Int) => DocValue::Number(number(text)?.trunc()),
        Some(FieldType::Float) => DocValue::Number(number(text)?),
        Some(FieldType::Currency) => {
            number(text)?;
            DocValue::Money(fyo.pesa().parse(&text.replace(',', ""))?)
        }
        Some(FieldType::Check) => DocValue::Bool(matches!(text, "true" | "1" | "Yes" | "yes")),
        Some(FieldType::Date) => DocValue::Date(date_timestamp(parse_date(text, label)?)?),
        Some(FieldType::Datetime) => {
            let (date, time) = text.split_once(' ').unwrap_or((text, "00:00"));
            let date = parse_date(date, label)?;
            let time: jiff::civil::Time = format!("{time}:00")
                .get(..8)
                .unwrap_or("00:00:00")
                .parse()
                .map_err(|_| Error::Value(format!("{label}: {time} is not a time")))?;
            let zoned = date
                .to_datetime(time)
                .to_zoned(TimeZone::system())
                .map_err(|e| Error::Value(e.to_string()))?;
            DocValue::Date(zoned.timestamp())
        }
        _ => DocValue::Text(text.to_owned()),
    })
}

fn parse_date(text: &str, label: &str) -> Result<Date, Error> {
    text.parse()
        .map_err(|_| Error::Value(format!("{label}: {text} is not a date (YYYY-MM-DD)")))
}

/// A Date field's value for a date: its local midnight, as Books stores one.
pub fn date_timestamp(date: Date) -> Result<Timestamp, Error> {
    date.to_zoned(TimeZone::system())
        .map(|zoned| zoned.timestamp())
        .map_err(|e| Error::Value(e.to_string()))
}
