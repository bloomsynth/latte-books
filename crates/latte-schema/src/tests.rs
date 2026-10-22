use std::fs;
use std::path::{Path, PathBuf};

use super::*;

fn vendored_schema_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/books/schemas")
}

fn json_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            json_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "json") {
            out.push(path);
        }
    }
}

fn fieldnames(schema: &Schema) -> Vec<&str> {
    schema.fields.iter().map(|f| f.fieldname.as_str()).collect()
}

const COUNTRIES: [&str; 3] = ["-", "in", "ch"];

#[test]
fn every_vendored_schema_is_valid_json() {
    let mut files = Vec::new();
    json_files(&vendored_schema_dir(), &mut files);
    assert!(files.len() >= 70, "found only {} schema files", files.len());
    for path in files {
        let text = fs::read_to_string(&path).unwrap();
        serde_json::from_str::<serde_json::Value>(&text)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    }
}

#[test]
fn every_vendored_schema_is_loaded() {
    let dir = vendored_schema_dir();
    let mut files = Vec::new();
    json_files(&dir, &mut files);

    let mut listed: Vec<&str> = [sources::CORE, sources::META, sources::APP]
        .into_iter()
        .chain(["in", "ch"].map(sources::regional))
        .flatten()
        .map(|(path, _)| *path)
        .collect();
    listed.sort();

    let mut on_disk: Vec<String> = files
        .iter()
        .map(|p| {
            p.strip_prefix(&dir)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .filter(|p| !p.starts_with("tests/"))
        .collect();
    on_disk.sort();

    assert_eq!(listed, on_disk);
}

#[test]
fn schemas_load_for_every_country() {
    for country in COUNTRIES {
        let schemas = get_schemas(country, &[]).unwrap_or_else(|e| panic!("{country}: {e}"));
        // 76 app and 3 core schemas, less the 6 abstract ones.
        assert_eq!(schemas.len(), 73, "{country}");
    }
}

#[test]
fn abstract_schemas_are_left_out() {
    let schemas = get_schemas("-", &[]).unwrap();
    for name in [
        "Invoice",
        "InvoiceItem",
        "StockTransfer",
        "StockTransferItem",
        "CashDenominations",
        "POSShiftAmounts",
    ] {
        assert!(!schemas.contains_key(name), "{name}");
    }
    assert!(schemas.contains_key("SalesInvoice"));
    assert!(schemas.contains_key("DefaultCashDenominations"));
}

#[test]
fn order_is_app_then_core() {
    let schemas = get_schemas("-", &[]).unwrap();
    let names: Vec<&str> = schemas.keys().map(String::as_str).collect();
    assert_eq!(names[..3], ["Misc", "SetupWizard", "GetStarted"]);
    assert_eq!(
        names[names.len() - 3..],
        ["PatchRun", "SingleValue", "SystemSettings"]
    );
}

#[test]
fn every_field_is_complete() {
    for country in COUNTRIES {
        for schema in get_schemas(country, &[]).unwrap().values() {
            for field in &schema.fields {
                let at = format!("{country} {}.{}", schema.name, field.fieldname);
                assert!(field.fieldtype.is_some(), "{at}: no fieldtype");
                assert!(!field.is_abstract, "{at}: abstract placeholder left");
                assert_eq!(
                    field.schema_name.as_deref(),
                    Some(schema.name.as_str()),
                    "{at}"
                );
            }
        }
    }
}

#[test]
fn fieldnames_are_unique() {
    for country in COUNTRIES {
        for schema in get_schemas(country, &[]).unwrap().values() {
            let mut names = fieldnames(schema);
            let count = names.len();
            names.sort();
            names.dedup();
            assert_eq!(names.len(), count, "{country} {}", schema.name);
        }
    }
}

#[test]
fn links_point_at_schemas_and_fields() {
    for country in COUNTRIES {
        let schemas = get_schemas(country, &[]).unwrap();
        for schema in schemas.values() {
            for field in &schema.fields {
                let at = format!("{country} {}.{}", schema.name, field.fieldname);
                let fieldtype = field.fieldtype.unwrap();
                if fieldtype.has_target() {
                    let target = field.target.as_deref().unwrap_or_else(|| panic!("{at}"));
                    assert!(schemas.contains_key(target), "{at}: no schema {target}");
                }
                if fieldtype == FieldType::Table {
                    let target = &schemas[field.target.as_deref().unwrap()];
                    assert!(target.is_child, "{at}: {} isn't a child table", target.name);
                }
                if fieldtype == FieldType::DynamicLink {
                    let references = field.references.as_deref().unwrap();
                    assert!(schema.field(references).is_some(), "{at}: no {references}");
                }
            }
        }
    }
}

#[test]
fn meta_fields_are_added() {
    let schemas = get_schemas("-", &[]).unwrap();
    let base = ["createdBy", "modifiedBy", "created", "modified"];
    let child = ["idx", "parent", "parentSchemaName", "parentFieldname"];

    let rule = fieldnames(&schemas["PricingRule"]);
    assert_eq!(rule[0], "name");
    assert_eq!(rule[rule.len() - 4..], base);

    let item = fieldnames(&schemas["SalesInvoiceItem"]);
    assert_eq!(item[item.len() - 4..], child);
    assert_eq!(item[0], "name", "child rows have a primary key too");

    let invoice = fieldnames(&schemas["SalesInvoice"]);
    for name in base.iter().chain(&["submitted", "cancelled"]) {
        assert!(invoice.contains(name), "{name}");
    }

    let account = fieldnames(&schemas["Account"]);
    for name in ["lft", "rgt"] {
        assert!(account.contains(&name), "{name}");
    }

    let settings = &schemas["AccountingSettings"];
    assert!(settings.is_single);
    assert!(settings.field("name").is_none());
    assert!(settings.field("created").is_none());
}

#[test]
fn name_field_is_kept_when_present() {
    let schemas = get_schemas("-", &[]).unwrap();
    let name = schemas["POSProfile"].field("name").unwrap();
    assert_eq!(name.label.as_deref(), Some("Profile"));
    assert!(name.unique);

    let name = schemas["PricingRule"].field("name").unwrap();
    assert_eq!(name.label.as_deref(), Some("ID"));
    assert!(name.required && name.read_only);
}

#[test]
fn title_field_defaults_to_name() {
    for schema in get_schemas("-", &[]).unwrap().values() {
        assert!(schema.title_field.is_some(), "{}", schema.name);
    }
}

#[test]
fn subclass_fields_take_the_abstract_fields_place() {
    let schemas = get_schemas("-", &[]).unwrap();
    let invoice = &schemas["SalesInvoice"];
    let names = fieldnames(invoice);
    let party = names.iter().position(|n| *n == "party").unwrap();
    let date = names.iter().position(|n| *n == "date").unwrap();
    assert!(party < date, "party should stay where Invoice puts it");

    let party = invoice.field("party").unwrap();
    assert_eq!(party.target.as_deref(), Some("Party"));
    assert_eq!(party.section.as_deref(), Some("Default"));

    // A subclass with no fields of its own gets the abstract schema's.
    let purchase = fieldnames(&schemas["PurchaseInvoiceItem"]);
    let sales = fieldnames(&schemas["SalesInvoiceItem"]);
    for name in &purchase {
        assert!(sales.contains(name), "{name}");
    }
    assert!(sales.contains(&"isFreeItem"));
    assert!(!purchase.contains(&"isFreeItem"));
}

#[test]
fn remove_fields_removes_them() {
    let schemas = get_schemas("-", &[]).unwrap();
    let quote = &schemas["SalesQuote"];
    for name in ["account", "stockNotTransferred", "backReference"] {
        assert!(quote.field(name).is_none(), "{name}");
    }
    assert!(schemas["SalesInvoice"].field("account").is_some());
}

#[test]
fn regional_schemas_override_app_schemas() {
    let plain = get_schemas("-", &[]).unwrap();
    let india = get_schemas("in", &[]).unwrap();
    let swiss = get_schemas("ch", &[]).unwrap();

    assert!(plain["Party"].field("taxId").is_some());
    assert!(plain["Party"].field("gstin").is_none());
    assert!(india["Party"].field("taxId").is_none());
    assert!(india["Party"].field("gstin").is_some());
    assert!(
        india["Party"]
            .quick_edit_fields
            .as_ref()
            .unwrap()
            .contains(&"gstin".to_owned())
    );

    assert!(india["Address"].field("pos").is_some());
    assert!(india["AccountingSettings"].field("gstin").is_some());
    assert!(swiss["AccountingSettings"].field("taxId").is_some());
    assert!(swiss["Party"].field("gstin").is_none());
}

fn custom(parent: &str, fieldname: &str) -> RawCustomField {
    RawCustomField {
        parent: parent.into(),
        label: fieldname.into(),
        fieldname: fieldname.into(),
        ..RawCustomField::default()
    }
}

#[test]
fn custom_fields_are_appended() {
    let colour = RawCustomField {
        fieldtype: FieldType::Select,
        options: Some("Red\n  Blue \n\n".into()),
        section: Some("Extra".into()),
        ..custom("Item", "colour")
    };
    let schemas = get_schemas("-", &[colour, custom("Item", "size")]).unwrap();
    let item = &schemas["Item"];
    let names = fieldnames(item);
    assert_eq!(names[names.len() - 2..], ["colour", "size"]);

    let colour = item.field("colour").unwrap();
    assert!(colour.is_custom);
    assert_eq!(colour.schema_name, None);
    assert_eq!(colour.section.as_deref(), Some("Extra"));
    let options: Vec<&str> = colour
        .options
        .iter()
        .flatten()
        .map(|o| o.value.as_str())
        .collect();
    assert_eq!(options, ["Red", "Blue"]);
    assert_eq!(item.field("size").unwrap().options, None);
}

#[test]
fn custom_fields_never_replace_built_in_fields_or_reach_unknown_schemas() {
    let schemas = get_schemas("-", &[custom("Item", "name"), custom("Nope", "x")]).unwrap();
    let item = &schemas["Item"];
    assert_eq!(
        item.fields.iter().filter(|f| f.fieldname == "name").count(),
        1
    );
    assert!(!item.field("name").unwrap().is_custom);
    assert!(!schemas.contains_key("Nope"));
}

#[test]
fn a_required_custom_field_needs_a_default() {
    let with_default = RawCustomField {
        is_required: Some(true),
        default: Some("M".into()),
        ..custom("Item", "size")
    };
    let without = RawCustomField {
        is_required: Some(true),
        ..custom("Item", "colour")
    };
    let schemas = get_schemas("-", &[with_default, without]).unwrap();
    let item = &schemas["Item"];

    let size = item.field("size").unwrap();
    assert!(size.required);
    assert_eq!(size.default, Some(RawValue::Text("M".into())));

    let colour = item.field("colour").unwrap();
    assert!(!colour.required);
    assert_eq!(colour.default, None);
}
