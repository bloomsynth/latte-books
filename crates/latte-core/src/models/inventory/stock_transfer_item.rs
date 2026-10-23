// Port of models/inventory/StockTransferItem.ts @ v0.36.0, the model of ShipmentItem and
// PurchaseReceiptItem. The unit and quantity formulas are in transfer_item.rs, with the
// Books bugs Latte fixes in them.
//
// The `account` formula isn't ported: the schema has no `account` field, so Books never
// applies it.

use latte_db::{DocValue, DocValueMap, Filter, QueryFilter};

use crate::Error;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::helpers::is_truthy;
use crate::fyo::model::types::{DocContext, Formula, Model};
use crate::models::helpers::{money, number, query, single_text, text};
use crate::models::inventory::transfer_item::{
    self, FormulaResult, batch_filter, hidden_by_settings, item_value_formula, some,
    transfer_unit_filter, validate_transfer_unit,
};

pub struct StockTransferItem;

/// `isSales`.
fn is_sales(doc: &Doc) -> bool {
    doc.schema_name() == "ShipmentItem"
}

/// `isReturn`: whether the transfer is a return.
fn is_return(ctx: &DocContext<'_>) -> bool {
    ctx.parent
        .is_some_and(|parent| is_truthy(parent.get("returnAgainst")))
}

/// The row of the back reference's invoice with this row's item, which
/// `getItemDiscountAmount` and `getItemDiscountPercent` read. Books' `getDoc` throws
/// without a back reference.
fn invoice_row(ctx: &DocContext<'_>) -> Result<Option<DocValueMap>, Error> {
    let schema_name = if is_sales(ctx.doc) {
        "SalesInvoice"
    } else {
        "PurchaseInvoice"
    };
    let back_reference = ctx
        .parent
        .and_then(|parent| text(parent, "backReference"))
        .filter(|b| !b.is_empty())
        .ok_or_else(|| Error::Value(format!("{schema_name} is not a Single Schema")))?;
    let invoice = ctx.fyo.db.get_doc(schema_name, back_reference)?;
    let Some(DocValue::Table(rows)) = invoice.get("items") else {
        return Ok(None);
    };
    Ok(rows
        .iter()
        .find(|row| row.get("item") == ctx.doc.get("item"))
        .cloned())
}

fn invoice_row_value(ctx: &DocContext<'_>, fieldname: &str) -> FormulaResult {
    Ok(invoice_row(ctx)?
        .and_then(|mut row| row.shift_remove(fieldname))
        .map(Into::into))
}

impl Model for StockTransferItem {
    fn link_filters(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<QueryFilter> {
        match fieldname {
            "item" => {
                let not_for = if is_sales(ctx.doc) {
                    "Purchases"
                } else {
                    "Sales"
                };
                Some(query([
                    ("for", Filter::list("not in", vec![not_for.into()])),
                    ("trackItem", Filter::from(true)),
                ]))
            }
            "transferUnit" => transfer_unit_filter(ctx),
            "batch" => batch_filter(ctx),
            _ => None,
        }
    }

    fn formulas(&self) -> Vec<Formula> {
        vec![
            Formula {
                fieldname: "description",
                depends_on: Some(&["item"]),
                formula: |ctx, _| item_value_formula(ctx, "description"),
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
                    "isReturn",
                ]),
                formula: |ctx, fieldname| {
                    transfer_item::quantity(ctx, fieldname, Some(is_return(ctx)))
                },
            },
            Formula {
                fieldname: "unitConversionFactor",
                depends_on: Some(&["transferUnit"]),
                formula: transfer_item::unit_conversion_factor,
            },
            Formula {
                fieldname: "hsnCode",
                depends_on: Some(&["item"]),
                formula: |ctx, _| item_value_formula(ctx, "hsnCode"),
            },
            Formula {
                fieldname: "amount",
                depends_on: Some(&["rate", "quantity"]),
                formula: |ctx, _| {
                    let quantity = number(ctx.doc, "quantity").unwrap_or(0.0);
                    let amount = match money(ctx.doc, "rate") {
                        Some(rate) => rate.mul(quantity)?,
                        None => ctx.fyo.pesa().zero(),
                    };
                    some(DocValue::Money(amount))
                },
            },
            Formula {
                fieldname: "itemDiscountAmount",
                depends_on: Some(&["items"]),
                formula: |ctx, _| invoice_row_value(ctx, "itemDiscountAmount"),
            },
            Formula {
                fieldname: "itemDiscountPercent",
                depends_on: Some(&["items"]),
                formula: |ctx, _| invoice_row_value(ctx, "itemDiscountPercent"),
            },
            Formula {
                fieldname: "rate",
                depends_on: Some(&["item"]),
                formula: |ctx, _| {
                    let rate = match transfer_item::item_value(ctx.doc, ctx, "rate") {
                        Some(DocValue::Money(rate)) => Some(rate),
                        _ => None,
                    };
                    let current = money(ctx.doc, "rate");
                    let rate_is_zero = rate.as_ref().is_none_or(|r| r.float() == 0.0);
                    if rate_is_zero && let Some(current) = current.filter(|c| c.float() != 0.0) {
                        return some(DocValue::Money(current));
                    }
                    some(DocValue::Money(
                        rate.unwrap_or_else(|| ctx.fyo.pesa().zero()),
                    ))
                },
            },
            Formula {
                fieldname: "location",
                depends_on: None,
                formula: |ctx, _| {
                    if text(ctx.doc, "location").is_some_and(|l| !l.is_empty()) {
                        return Ok(None);
                    }
                    Ok(single_text(ctx.fyo, "InventorySettings", "defaultLocation")
                        .map(|location| DocValue::Text(location).into()))
                },
            },
        ]
    }

    fn validate_field(
        &self,
        fieldname: &str,
        value: &DocValue,
        ctx: &DocContext<'_>,
    ) -> Result<(), Error> {
        match fieldname {
            "transferUnit" => validate_transfer_unit(value, ctx),
            _ => Ok(()),
        }
    }

    fn hidden(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<bool> {
        match fieldname {
            "itemDiscountAmount" => {
                Some(money(ctx.doc, "itemDiscountAmount").is_none_or(|amount| amount.is_zero()))
            }
            "itemDiscountPercent" => Some(!is_truthy(ctx.doc.get("itemDiscountPercent"))),
            _ => hidden_by_settings(fieldname, ctx),
        }
    }
}
