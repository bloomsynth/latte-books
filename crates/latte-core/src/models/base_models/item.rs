// Port of models/baseModels/Item/Item.ts @ v0.36.0.
//
// Not ported yet, as the UI needs them: `filters`, `getActions` and
// `getListViewSettings`.

use latte_db::{DocValue, Filter, GetAllOptions, QueryFilter};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::helpers::is_truthy;
use crate::fyo::model::types::{DocContext, Formula, Model};
use crate::models::helpers::{number, query, single_is_truthy, single_value, text};

pub struct Item;

impl Model for Item {
    fn link_filters(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<QueryFilter> {
        let root_type = match fieldname {
            "incomeAccount" => "Income",
            "expenseAccount" if is_truthy(ctx.doc.get("trackItem")) => "Liability",
            "expenseAccount" => "Expense",
            _ => return None,
        };
        Some(query([
            ("isGroup", Filter::from(false)),
            ("rootType", Filter::from(root_type)),
        ]))
    }

    fn formulas(&self) -> Vec<Formula> {
        vec![
            Formula {
                fieldname: "incomeAccount",
                depends_on: Some(&["itemType"]),
                formula: |ctx, _| {
                    let account_name = if text(ctx.doc, "itemType") == Some("Product") {
                        "Sales"
                    } else {
                        "Service"
                    };
                    let account_exists = ctx.fyo.db.core().exists("Account", Some(account_name))?;
                    let account = if account_exists { account_name } else { "" };
                    Ok(Some(DocValue::Text(account.to_owned()).into()))
                },
            },
            Formula {
                fieldname: "expenseAccount",
                depends_on: Some(&["itemType", "trackItem"]),
                formula: |ctx, _| {
                    if is_truthy(ctx.doc.get("trackItem")) {
                        return Ok(single_value(
                            ctx.fyo,
                            "InventorySettings",
                            "stockReceivedButNotBilled",
                        )
                        .map(Into::into));
                    }

                    let options = GetAllOptions {
                        filters: QueryFilter::from([(
                            "accountType".to_owned(),
                            Filter::from("Cost of Goods Sold"),
                        )]),
                        ..GetAllOptions::default()
                    };
                    let cogs = ctx.fyo.db.get_all_docs("Account", &options)?;
                    let account = cogs
                        .first()
                        .and_then(|account| account.get("name").cloned())
                        .unwrap_or(DocValue::Text(String::new()));
                    Ok(Some(account.into()))
                },
            },
            Formula {
                fieldname: "hsnCode",
                depends_on: Some(&["itemGroup"]),
                formula: |ctx, _| {
                    let Some(item_group) = text(ctx.doc, "itemGroup").filter(|g| !g.is_empty())
                    else {
                        return Ok(Some(DocValue::Text(String::new()).into()));
                    };
                    let item_group = ctx.fyo.db.get_doc("ItemGroup", item_group)?;
                    Ok(item_group.get("hsnCode").cloned().map(Into::into))
                },
            },
        ]
    }

    /// `beforeSync`: keeps the last conversion for each unit, of those with a factor
    /// above zero.
    fn before_sync(&self, doc: &mut Doc, _fyo: &mut Fyo) -> Result<(), Error> {
        let mut latest_by_uom: indexmap::IndexMap<String, Doc> = indexmap::IndexMap::new();
        for row in doc.table("uomConversions") {
            if number(row, "conversionFactor").unwrap_or(0.0) > 0.0 {
                let uom = crate::fyo::model::helpers::js_string(row.get("uom"));
                latest_by_uom.insert(uom, row.clone());
            }
        }
        doc.set_table_rows("uomConversions", latest_by_uom.into_values().collect());
        Ok(())
    }

    fn validate_field(
        &self,
        fieldname: &str,
        value: &DocValue,
        _ctx: &DocContext<'_>,
    ) -> Result<(), Error> {
        match (fieldname, value) {
            ("barcode", DocValue::Text(barcode))
                if !barcode.is_empty() && !is_digits(barcode, 12, 12) =>
            {
                Err(Error::Validation(
                    "Barcode must be exactly 12 digits.".into(),
                ))
            }
            ("rate", DocValue::Money(rate)) if rate.is_negative() => {
                Err(Error::Validation("Rate can't be negative.".into()))
            }
            ("hsnCode", DocValue::Text(hsn_code))
                if !hsn_code.is_empty() && !is_digits(hsn_code, 4, 8) =>
            {
                Err(Error::Validation("Invalid HSN Code.".into()))
            }
            _ => Ok(()),
        }
    }

    fn hidden(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<bool> {
        let fyo = ctx.fyo;
        let doc = ctx.doc;
        let inventory = |fieldname| single_is_truthy(fyo, "InventorySettings", fieldname);
        Some(match fieldname {
            "trackItem" => {
                !single_is_truthy(fyo, "AccountingSettings", "enableInventory")
                    || text(doc, "itemType") != Some("Product")
                    || (doc.inserted() && !is_truthy(doc.get("trackItem")))
            }
            "barcode" => !inventory("enableBarcodes"),
            "hasBatch" => !inventory("enableBatches"),
            "hasSerialNumber" => {
                !(inventory("enableSerialNumber") && is_truthy(doc.get("trackItem")))
            }
            "uomConversions" => !inventory("enableUomConversions"),
            "itemGroup" => !single_is_truthy(fyo, "AccountingSettings", "enableitemGroup"),
            _ => return None,
        })
    }

    fn read_only(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<bool> {
        match fieldname {
            "unit" | "itemType" | "trackItem" | "hasBatch" | "hasSerialNumber" => {
                Some(ctx.doc.inserted())
            }
            _ => None,
        }
    }
}

/// `/^\d{min,max}$/`.
fn is_digits(value: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_digit())
}
