// What StockMovementItem.ts and StockTransferItem.ts @ v0.36.0 share: the unit and
// quantity formulas, the transfer unit check and the fields Inventory Settings hide.
//
// Books bugs Latte fixes (as in invoice_item.rs):
// - `unitConversionFactor` is the factor of the row's transfer unit. Books takes the item's
//   first conversion, whatever its unit.
// - `quantity` is rounded for an item whose unit is a whole number. Books looks for the
//   unit in `uom`, which Item doesn't have, so it never rounds.
// - The transfer unit may be the item's own unit. Books' check wants a conversion for it,
//   so picking the item's unit back is refused.

use latte_db::{DocValue, Filter, GetAllOptions, QueryFilter};

use crate::Error;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::helpers::{is_truthy, js_string};
use crate::fyo::model::types::{DocContext, FormulaValue};
use crate::models::helpers::{number, single_is_truthy, text};

pub(crate) type FormulaResult = Result<Option<FormulaValue>, Error>;

pub(crate) fn some(value: DocValue) -> FormulaResult {
    Ok(Some(value.into()))
}

/// `fyo.getValue('Item', this.item, fieldname)`.
pub(crate) fn item_value(doc: &Doc, ctx: &DocContext<'_>, fieldname: &str) -> Option<DocValue> {
    let item = text(doc, "item").filter(|i| !i.is_empty())?;
    ctx.fyo.get_value("Item", Some(item), fieldname)
}

/// The item's value as the formula's result: `undefined` when there is none.
pub(crate) fn item_value_formula(ctx: &DocContext<'_>, fieldname: &str) -> FormulaResult {
    Ok(item_value(ctx.doc, ctx, fieldname).map(Into::into))
}

/// The `transferUnit` formula.
pub(crate) fn transfer_unit(ctx: &DocContext<'_>, fieldname: Option<&str>) -> FormulaResult {
    if matches!(fieldname, Some("quantity" | "unit")) {
        return Ok(ctx.doc.get("unit").cloned().map(Into::into));
    }
    item_value_formula(ctx, "unit")
}

/// The `transferQuantity` formula.
pub(crate) fn transfer_quantity(ctx: &DocContext<'_>, fieldname: Option<&str>) -> FormulaResult {
    let doc = ctx.doc;
    if fieldname == Some("quantity") || doc.get("unit") == doc.get("transferUnit") {
        return Ok(doc.get("quantity").cloned().map(Into::into));
    }
    Ok(doc.get("transferQuantity").cloned().map(Into::into))
}

/// The `quantity` formula. A return's quantity is negative and any other positive, when
/// `is_return` is given.
pub(crate) fn quantity(
    ctx: &DocContext<'_>,
    fieldname: Option<&str>,
    is_return: Option<bool>,
) -> FormulaResult {
    let doc = ctx.doc;
    let Some(item) = text(doc, "item").filter(|i| !i.is_empty()) else {
        return Ok(doc.get("quantity").cloned().map(Into::into));
    };
    // `getDoc` throws for an item that doesn't exist.
    let item = ctx.fyo.db.get_doc("Item", item)?;
    let is_whole = match item.get("unit") {
        Some(DocValue::Text(unit)) if !unit.is_empty() => {
            is_truthy(ctx.fyo.get_value("UOM", Some(unit), "isWhole").as_ref())
        }
        _ => false,
    };

    let mut quantity = number(doc, "quantity").unwrap_or(1.0);
    if let Some(is_return) = is_return
        && ((is_return && quantity > 0.0) || (!is_return && quantity < 0.0))
    {
        quantity *= -1.0;
    }
    if fieldname == Some("transferQuantity") {
        quantity = number(doc, "transferQuantity").unwrap_or(f64::NAN)
            * number(doc, "unitConversionFactor").unwrap_or(f64::NAN);
    }
    if is_whole {
        // `Math.round`, which rounds halves up.
        return some(DocValue::Number((quantity + 0.5).floor()));
    }
    // `safeParseFloat`.
    let quantity = if quantity.is_nan() { 0.0 } else { quantity };
    some(DocValue::Number(quantity))
}

/// The `unitConversionFactor` formula.
pub(crate) fn unit_conversion_factor(ctx: &DocContext<'_>, _: Option<&str>) -> FormulaResult {
    let doc = ctx.doc;
    if doc.get("unit") == doc.get("transferUnit") {
        return some(DocValue::Number(1.0));
    }

    let item = text(doc, "item").unwrap_or_default();
    let transfer_unit = js_string(doc.get("transferUnit"));
    let factor = uom_conversions(ctx, item, Some(&transfer_unit))?
        .first()
        .and_then(|row| match row.get("conversionFactor") {
            Some(DocValue::Number(factor)) => Some(*factor),
            _ => None,
        })
        .unwrap_or(1.0);
    some(DocValue::Number(if factor.is_nan() { 0.0 } else { factor }))
}

/// The item's unit conversions, of one unit if given.
pub(crate) fn uom_conversions(
    ctx: &DocContext<'_>,
    item: &str,
    uom: Option<&str>,
) -> Result<Vec<latte_db::DocValueMap>, Error> {
    let mut filters = QueryFilter::from([("parent".to_owned(), Filter::from(item))]);
    if let Some(uom) = uom {
        filters.insert("uom".to_owned(), Filter::from(uom));
    }
    let options = GetAllOptions {
        fields: Some(vec!["conversionFactor".to_owned(), "uom".to_owned()]),
        filters,
        ..GetAllOptions::default()
    };
    Ok(ctx.fyo.db.get_all_docs("UOMConversionItem", &options)?)
}

/// The `transferUnit` validation.
pub(crate) fn validate_transfer_unit(value: &DocValue, ctx: &DocContext<'_>) -> Result<(), Error> {
    let Some(item) = text(ctx.doc, "item").filter(|i| !i.is_empty()) else {
        return Ok(());
    };
    if Some(value) == ctx.doc.get("unit") {
        return Ok(());
    }

    let unit = js_string(Some(value));
    if uom_conversions(ctx, item, Some(&unit))?.is_empty() {
        return Err(Error::Validation(format!(
            "Transfer Unit {unit} is not applicable for Item {item}"
        )));
    }
    Ok(())
}

/// The fields Inventory Settings hide when batches, serial numbers or unit conversions are
/// off.
pub(crate) fn hidden_by_settings(fieldname: &str, ctx: &DocContext<'_>) -> Option<bool> {
    let setting = match fieldname {
        "batch" => "enableBatches",
        "serialNumber" => "enableSerialNumber",
        "transferUnit" | "transferQuantity" | "unitConversionFactor" => "enableUomConversions",
        _ => return None,
    };
    Some(!single_is_truthy(ctx.fyo, "InventorySettings", setting))
}

/// The `batch` link filter: the item's batches.
pub(crate) fn batch_filter(ctx: &DocContext<'_>) -> Option<QueryFilter> {
    let item = text(ctx.doc, "item").unwrap_or_default();
    let options = GetAllOptions {
        filters: QueryFilter::from([("item".to_owned(), Filter::from(item))]),
        ..GetAllOptions::default()
    };
    let names = ctx
        .fyo
        .db
        .get_all_docs("Batch", &options)
        .ok()?
        .into_iter()
        .filter_map(|row| match row.get("name") {
            Some(DocValue::Text(name)) => Some(name.as_str().into()),
            _ => None,
        })
        .collect();
    Some(QueryFilter::from([(
        "name".to_owned(),
        Filter::list("in", names),
    )]))
}

/// The `transferUnit` link filter: the item's unit and the units it converts to.
pub(crate) fn transfer_unit_filter(ctx: &DocContext<'_>) -> Option<QueryFilter> {
    let item = text(ctx.doc, "item").unwrap_or_default();
    let mut units: Vec<latte_db::FilterValue> = uom_conversions(ctx, item, None)
        .ok()?
        .into_iter()
        .filter_map(|row| match row.get("uom") {
            Some(DocValue::Text(uom)) if !uom.is_empty() => Some(uom.as_str().into()),
            _ => None,
        })
        .collect();
    if let Some(DocValue::Text(unit)) = item_value(ctx.doc, ctx, "unit")
        && !unit.is_empty()
    {
        units.push(unit.into());
    }
    Some(QueryFilter::from([(
        "name".to_owned(),
        Filter::list("in", units),
    )]))
}
