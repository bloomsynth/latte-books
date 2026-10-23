// Port of fyo/models/CustomForm.ts and fyo/models/CustomField.ts @ v0.36.0.
//
// A custom field shows up once the file is opened again, as in Books: the schemas are
// built when a file is opened, and opening it adds the field's column. The app opens the
// file again when a custom form is saved or deleted.
//
// Deviation: a custom field's label or fieldname isn't refused for clashing with its own
// row. Books looks for the fieldname among all of the form's rows, the row being edited
// included, so setting a row's label again to the same value is refused.

use latte_db::DocValue;
use latte_schema::{FieldType, Schema, SelectOption};

use crate::Error;
use crate::fyo::model::types::{DocContext, Formula, Model};

/// Schemas that can't have custom fields.
const NOT_CUSTOMIZABLE: [&str; 5] = [
    "PatchRun",
    "SingleValue",
    "CustomField",
    "CustomForm",
    "SetupWizard",
];

fn text<'a>(ctx: &'a DocContext<'_>, fieldname: &str) -> &'a str {
    match ctx.doc.get(fieldname) {
        Some(DocValue::Text(text)) => text,
        _ => "",
    }
}

/// Schemas a custom form, or a custom Link or Table field, may be for.
fn customizable(ctx: Option<&DocContext<'_>>, needs_label: bool) -> Vec<SelectOption> {
    let Some(ctx) = ctx else {
        return Vec::new();
    };
    let mut schemas: Vec<&Schema> = ctx
        .fyo
        .db
        .schema_map()
        .values()
        .filter(|schema| {
            !schema.is_single
                && !(needs_label && schema.label.is_empty())
                && !NOT_CUSTOMIZABLE.contains(&schema.name.as_str())
        })
        .collect();
    schemas.sort_by(|a, b| a.name.cmp(&b.name));
    schemas
        .into_iter()
        .map(|schema| SelectOption {
            value: schema.name.clone(),
            label: schema.label.clone(),
        })
        .collect()
}

pub struct CustomForm;

impl Model for CustomForm {
    fn list(&self, fieldname: &str, ctx: Option<&DocContext<'_>>) -> Option<Vec<SelectOption>> {
        (fieldname == "name").then(|| customizable(ctx, true))
    }

    fn hidden(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<bool> {
        (fieldname == "customFields").then(|| text(ctx, "name").is_empty())
    }

    fn validate(&self, doc: &mut crate::Doc, _: &mut crate::Fyo) -> Result<(), Error> {
        for row in doc.table("customFields") {
            let fieldtype = match row.get("fieldtype") {
                Some(DocValue::Text(fieldtype)) => fieldtype.as_str(),
                _ => "",
            };
            if fieldtype != "Select" && fieldtype != "AutoComplete" {
                continue;
            }
            let options = match row.get("options") {
                Some(DocValue::Text(options)) => options.as_str(),
                _ => "",
            };
            let count = options
                .split('\n')
                .filter(|option| !option.trim().is_empty())
                .count();
            if count <= 1 {
                return Err(Error::Validation(
                    "At least two options need to be set for the selected fieldtype".into(),
                ));
            }
        }
        Ok(())
    }
}

/// lodash's `camelCase`: the words of `value`, the first in lower case and the others
/// capitalized. A word is a run of letters or of digits; a capital starts a new one,
/// except within a run of capitals ("GSTNumber" is "GST" and "Number").
pub fn camel_case(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() || c == '\'' {
            if c != '\'' && !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            continue;
        }
        if let Some(&prev) = word.chars().last().as_ref() {
            let next = chars.get(i + 1).copied();
            let splits = prev.is_ascii_digit() != c.is_ascii_digit()
                || (prev.is_lowercase() && c.is_uppercase())
                || (prev.is_uppercase()
                    && c.is_uppercase()
                    && next.is_some_and(char::is_lowercase));
            if splits {
                words.push(std::mem::take(&mut word));
            }
        }
        word.push(c);
    }
    if !word.is_empty() {
        words.push(word);
    }

    let mut camel = String::new();
    for (i, word) in words.iter().enumerate() {
        let lower = word.to_lowercase();
        if i == 0 {
            camel.push_str(&lower);
            continue;
        }
        let mut chars = lower.chars();
        if let Some(first) = chars.next() {
            camel.extend(first.to_uppercase());
            camel.push_str(chars.as_str());
        }
    }
    camel
}

pub struct CustomField;

impl CustomField {
    fn check_fieldname(fieldname: &str, ctx: &DocContext<'_>) -> Result<(), Error> {
        let Some(parent) = ctx.parent else {
            return Ok(());
        };
        let form = match parent.get("name") {
            Some(DocValue::Text(name)) => name.as_str(),
            _ => "",
        };
        if let Some(schema) = ctx.fyo.db.schema_map().get(form)
            && schema
                .field(fieldname)
                .is_some_and(|field| !field.is_custom)
        {
            return Err(Error::Value(format!(
                "Fieldname {fieldname} already exists for {form}"
            )));
        }

        let rows = parent.table("customFields");
        let used = rows.iter().enumerate().find(|(_, row)| {
            !std::ptr::eq(*row, ctx.doc)
                && row.name() != ctx.doc.name()
                && matches!(row.get("fieldname"), Some(DocValue::Text(f)) if f == fieldname)
        });
        if let Some((idx, _)) = used {
            return Err(Error::Value(format!(
                "Fieldname {fieldname} already used for Custom Field {}",
                idx + 1
            )));
        }
        Ok(())
    }
}

/// Field types a DynamicLink's `references` may name.
const REFERENCE_TYPES: [&str; 4] = ["AutoComplete", "Data", "Text", "Select"];

impl Model for CustomField {
    fn formulas(&self) -> Vec<Formula> {
        vec![Formula {
            fieldname: "fieldname",
            depends_on: Some(&["label"]),
            formula: |ctx, _| {
                let label = text(ctx, "label");
                if label.is_empty() {
                    return Ok(None);
                }
                Ok(Some(DocValue::Text(camel_case(label)).into()))
            },
        }]
    }

    fn validate_field(
        &self,
        fieldname: &str,
        value: &DocValue,
        ctx: &DocContext<'_>,
    ) -> Result<(), Error> {
        let DocValue::Text(value) = value else {
            return Ok(());
        };
        match fieldname {
            "label" => Self::check_fieldname(&camel_case(value), ctx),
            "fieldname" => Self::check_fieldname(value, ctx),
            _ => Ok(()),
        }
    }

    fn hidden(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<bool> {
        let fieldtype = text(ctx, "fieldtype");
        match fieldname {
            "options" => Some(!matches!(fieldtype, "Select" | "AutoComplete" | "Color")),
            "target" => Some(!matches!(fieldtype, "Link" | "Table")),
            "references" => Some(fieldtype != "DynamicLink"),
            _ => None,
        }
    }

    fn required(&self, fieldname: &str, ctx: &DocContext<'_>) -> bool {
        let fieldtype = text(ctx, "fieldtype");
        match fieldname {
            "options" => matches!(fieldtype, "Select" | "AutoComplete"),
            "target" => matches!(fieldtype, "Link" | "Table"),
            "references" => fieldtype == "DynamicLink",
            "default" => matches!(ctx.doc.get("isRequired"), Some(DocValue::Bool(true))),
            _ => false,
        }
    }

    fn list(&self, fieldname: &str, ctx: Option<&DocContext<'_>>) -> Option<Vec<SelectOption>> {
        match fieldname {
            "target" => Some(customizable(ctx, false)),
            "references" => {
                let ctx = ctx?;
                let mut options: Vec<SelectOption> = Vec::new();
                let Some(parent) = ctx.parent else {
                    return Some(options);
                };
                for row in parent.table("customFields") {
                    let get = |f: &str| match row.get(f) {
                        Some(DocValue::Text(text)) if !text.is_empty() => Some(text.clone()),
                        _ => None,
                    };
                    if let (Some(value), Some(label), Some(fieldtype)) =
                        (get("fieldname"), get("label"), get("fieldtype"))
                        && REFERENCE_TYPES.contains(&fieldtype.as_str())
                    {
                        options.push(SelectOption { value, label });
                    }
                }
                let form = match parent.get("name") {
                    Some(DocValue::Text(name)) => name.as_str(),
                    _ => "",
                };
                if let Some(schema) = ctx.fyo.db.schema_map().get(form) {
                    for field in &schema.fields {
                        let is_reference = matches!(
                            field.fieldtype,
                            Some(
                                FieldType::AutoComplete
                                    | FieldType::Data
                                    | FieldType::Text
                                    | FieldType::Select
                            )
                        );
                        if let Some(label) = &field.label
                            && is_reference
                            && !field.fieldname.is_empty()
                        {
                            options.push(SelectOption {
                                value: field.fieldname.clone(),
                                label: label.clone(),
                            });
                        }
                    }
                }
                Some(options)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use latte_db::Database;

    use super::camel_case;
    use crate::Fyo;
    use crate::setup_tests::{TempFile, indian_company};
    use crate::tests::{map, text};

    #[test]
    fn a_custom_field_is_added_to_its_form_when_the_file_is_opened_again() {
        let file = TempFile::new("custom-field");
        let mut fyo = indian_company(&file);
        let mut form = fyo
            .get_new_doc("CustomForm", map([("name", text("Item"))]))
            .unwrap();
        form.append(&mut fyo, "customFields", Default::default())
            .unwrap();
        form.set_child(&mut fyo, "customFields", 0, "label", text("Shelf Number"))
            .unwrap();
        form.set_child(&mut fyo, "customFields", 0, "fieldtype", text("Data"))
            .unwrap();
        assert_eq!(
            form.table("customFields")[0].get("fieldname"),
            Some(&text("shelfNumber"))
        );

        // A built-in field's name, or another row's, is refused.
        form.append(&mut fyo, "customFields", Default::default())
            .unwrap();
        let result = form.set_child(&mut fyo, "customFields", 1, "label", text("Rate"));
        assert_eq!(
            result.unwrap_err().to_string(),
            "Fieldname rate already exists for Item"
        );
        let result = form.set_child(&mut fyo, "customFields", 1, "label", text("Shelf number"));
        assert_eq!(
            result.unwrap_err().to_string(),
            "Fieldname shelfNumber already used for Custom Field 1"
        );
        // A row's own label may be set again.
        form.set_child(&mut fyo, "customFields", 0, "label", text("Shelf Number"))
            .unwrap();

        // A Select needs two options.
        form.set_child(&mut fyo, "customFields", 1, "label", text("Grade"))
            .unwrap();
        form.set_child(&mut fyo, "customFields", 1, "fieldtype", text("Select"))
            .unwrap();
        form.set_child(&mut fyo, "customFields", 1, "options", text("A"))
            .unwrap();
        assert_eq!(
            form.sync(&mut fyo).unwrap_err().to_string(),
            "At least two options need to be set for the selected fieldtype"
        );
        form.set_child(
            &mut fyo,
            "customFields",
            1,
            "options",
            text(
                "A
B",
            ),
        )
        .unwrap();
        form.sync(&mut fyo).unwrap();
        drop(fyo);

        let mut fyo = Fyo::with_models(Database::open(&file.0, "test").unwrap());
        let values = map([
            ("name", text("Bolt")),
            ("shelfNumber", text("B-12")),
            ("grade", text("B")),
        ]);
        fyo.get_new_doc("Item", values)
            .unwrap()
            .sync(&mut fyo)
            .unwrap();
        let item = fyo.get_doc("Item", Some("Bolt")).unwrap();
        assert_eq!(item.get("shelfNumber"), Some(&text("B-12")));
        assert_eq!(item.get("grade"), Some(&text("B")));
    }

    #[test]
    fn camel_case_is_lodashs() {
        assert_eq!(camel_case("Delivery Date"), "deliveryDate");
        assert_eq!(camel_case("GST No."), "gstNo");
        assert_eq!(camel_case("Field 1"), "field1");
        assert_eq!(camel_case("fooBar"), "fooBar");
        assert_eq!(camel_case("__FOO_BAR__"), "fooBar");
        assert_eq!(camel_case("GSTNumber"), "gstNumber");
        assert_eq!(camel_case("Driver's Name"), "driversName");
    }
}
