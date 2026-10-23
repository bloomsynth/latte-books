// Port of models/inventory/StockMovementItem.ts @ v0.36.0. The unit and quantity formulas
// are in transfer_item.rs, with the Books bugs Latte fixes in them.
//
// Not ported yet, as the UI needs it: `createFilters`.

use latte_db::{DocValue, Filter, QueryFilter};

use crate::Error;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::types::{DocContext, Formula, Model};
use crate::models::helpers::{money, number, query, single_text, text};
use crate::models::inventory::transfer_item::{
    self, batch_filter, hidden_by_settings, item_value_formula, some, transfer_unit_filter,
    validate_transfer_unit,
};

pub struct StockMovementItem;

fn movement_type<'a>(ctx: &DocContext<'a>) -> Option<&'a str> {
    ctx.parent.and_then(|parent| text(parent, "movementType"))
}

fn is_issue(ctx: &DocContext<'_>) -> bool {
    movement_type(ctx) == Some("MaterialIssue")
}

fn is_receipt(ctx: &DocContext<'_>) -> bool {
    movement_type(ctx) == Some("MaterialReceipt")
}

fn is_transfer(ctx: &DocContext<'_>) -> bool {
    movement_type(ctx) == Some("MaterialTransfer")
}

fn is_manufacture(ctx: &DocContext<'_>) -> bool {
    movement_type(ctx) == Some("Manufacture")
}

fn is_set(doc: &Doc, fieldname: &str) -> bool {
    text(doc, fieldname).is_some_and(|v| !v.is_empty())
}

/// `validateBatchAndItemConsistency` and the `batch` validation.
fn validate_batch_item(doc: &Doc, ctx: &DocContext<'_>) -> Result<(), Error> {
    let (Some(item), Some(batch)) = (
        text(doc, "item").filter(|i| !i.is_empty()),
        text(doc, "batch").filter(|b| !b.is_empty()),
    ) else {
        return Ok(());
    };
    let Ok(batch_doc) = ctx.fyo.db.get_doc("Batch", batch) else {
        return Ok(());
    };
    if batch_doc.get("item") != Some(&DocValue::Text(item.to_owned())) {
        return Err(Error::Validation(format!(
            "Batch {batch} does not belong to Item {item}"
        )));
    }
    Ok(())
}

impl Model for StockMovementItem {
    fn link_filters(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<QueryFilter> {
        match fieldname {
            "item" => Some(query([("trackItem", Filter::from(true))])),
            "transferUnit" => transfer_unit_filter(ctx),
            "batch" => batch_filter(ctx),
            _ => None,
        }
    }

    fn formulas(&self) -> Vec<Formula> {
        vec![
            Formula {
                fieldname: "rate",
                depends_on: Some(&["item"]),
                formula: |ctx, _| {
                    if !is_set(ctx.doc, "item") {
                        return Ok(ctx.doc.get("rate").cloned().map(Into::into));
                    }
                    item_value_formula(ctx, "rate")
                },
            },
            Formula {
                fieldname: "amount",
                depends_on: Some(&["item", "rate", "quantity"]),
                formula: |ctx, _| {
                    let rate = money(ctx.doc, "rate")
                        .ok_or_else(|| Error::Value("rate is not money".into()))?;
                    let quantity = number(ctx.doc, "quantity").unwrap_or(f64::NAN);
                    some(DocValue::Money(rate.mul(quantity)?))
                },
            },
            Formula {
                fieldname: "fromLocation",
                depends_on: Some(&["movementType"]),
                formula: |ctx, _| {
                    if is_receipt(ctx) || is_transfer(ctx) || is_manufacture(ctx) {
                        return some(DocValue::Null);
                    }
                    let default_location =
                        single_text(ctx.fyo, "InventorySettings", "defaultLocation");
                    if let Some(location) = default_location
                        && !is_set(ctx.doc, "fromLocation")
                        && is_issue(ctx)
                    {
                        return some(DocValue::Text(location));
                    }
                    // As in Books: an issue takes the location the row was moving to.
                    Ok(ctx.doc.get("toLocation").cloned().map(Into::into))
                },
            },
            Formula {
                fieldname: "toLocation",
                depends_on: Some(&["movementType"]),
                formula: |ctx, _| {
                    if is_issue(ctx) || is_transfer(ctx) || is_manufacture(ctx) {
                        return some(DocValue::Null);
                    }
                    let default_location =
                        single_text(ctx.fyo, "InventorySettings", "defaultLocation");
                    if let Some(location) = default_location
                        && !is_set(ctx.doc, "toLocation")
                        && is_receipt(ctx)
                    {
                        return some(DocValue::Text(location));
                    }
                    Ok(ctx.doc.get("toLocation").cloned().map(Into::into))
                },
            },
            Formula {
                fieldname: "unit",
                depends_on: Some(&["item"]),
                formula: |ctx, _| item_value_formula(ctx, "unit"),
            },
            Formula {
                fieldname: "transferUnit",
                depends_on: Some(&["item", "unit"]),
                formula: transfer_item::transfer_unit,
            },
            Formula {
                fieldname: "transferQuantity",
                depends_on: Some(&["item", "quantity"]),
                formula: transfer_item::transfer_quantity,
            },
            Formula {
                fieldname: "quantity",
                depends_on: Some(&[
                    "quantity",
                    "transferQuantity",
                    "transferUnit",
                    "unitConversionFactor",
                ]),
                formula: |ctx, fieldname| transfer_item::quantity(ctx, fieldname, None),
            },
            Formula {
                fieldname: "unitConversionFactor",
                depends_on: Some(&["transferUnit"]),
                formula: transfer_item::unit_conversion_factor,
            },
        ]
    }

    fn validate_field(
        &self,
        fieldname: &str,
        value: &DocValue,
        ctx: &DocContext<'_>,
    ) -> Result<(), Error> {
        let only_one = || {
            Err(Error::Validation(
                "Only From or To can be set for Manufacture".into(),
            ))
        };
        let is_value_set = matches!(value, DocValue::Text(v) if !v.is_empty());
        match fieldname {
            "fromLocation"
                if is_manufacture(ctx) && is_value_set && is_set(ctx.doc, "toLocation") =>
            {
                only_one()
            }
            "toLocation"
                if is_manufacture(ctx) && is_value_set && is_set(ctx.doc, "fromLocation") =>
            {
                only_one()
            }
            "batch" => validate_batch_item(ctx.doc, ctx),
            "transferUnit" => validate_transfer_unit(value, ctx),
            _ => Ok(()),
        }
    }

    fn required(&self, fieldname: &str, ctx: &DocContext<'_>) -> bool {
        match fieldname {
            "fromLocation" => is_issue(ctx) || is_transfer(ctx),
            "toLocation" => is_receipt(ctx) || is_transfer(ctx),
            _ => false,
        }
    }

    fn read_only(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<bool> {
        match fieldname {
            "fromLocation" => Some(is_receipt(ctx)),
            "toLocation" => Some(is_issue(ctx)),
            _ => None,
        }
    }

    fn hidden(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<bool> {
        hidden_by_settings(fieldname, ctx)
    }
}
