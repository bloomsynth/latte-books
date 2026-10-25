// Port of models/baseModels/InvoiceItem/InvoiceItem.ts @ v0.36.0, the model of
// SalesInvoiceItem and PurchaseInvoiceItem.
//
// Deviations:
// - The `unitConversionFactor` formula doesn't set `quantity` to the quantity in the
//   transfer unit times the factor, as Books' formula does on the side. Formulas here
//   can't change other fields. It only matters when unit conversions are on.
// - A pricing rule's discount amount is set by an `itemDiscountAmount` formula. Books'
//   `setItemDiscountAmount` formula sets it on the side, which formulas here can't. Where
//   a rule stops applying, the invoice clears the discount it gave (invoice.rs), as Books
//   does with the rule it keeps in memory (`prule`).
//
// Books bugs Latte fixes:
// - `itemDiscountedTotal` with a discount percent is the amount less the discount, not the
//   discount itself (pesa's `percent` gives the discount).
// - `itemTaxedTotal` follows changes to the discount and tax too, not only to the rate,
//   quantity and item.
// - When a row's item changes, `transferUnit` keeps the transfer unit if the new item has
//   a conversion for it. Books' check is the wrong way round, so it always puts the unit
//   back.
// - `quantity` is rounded for an item whose unit is a whole number, as Books means to, but
//   it looks for the unit in `uom`, which Item doesn't have.
// - `stockNotTransferred` takes off all that was shipped or received of the item, not only
//   the last row's quantity.
// The two totals are only shown, not stored. The rest store values Books can store too:
// Books reads them as they are, though its formulas put back its own stock not shipped or
// received when a draft is saved in Books.
//
// Not ported yet, as the UI needs them: `filters` and `createFilters`.

use latte_db::pesa::Money;
use latte_db::{DocValue, Filter, GetAllOptions, QueryFilter};
use latte_schema::FieldType;

use crate::Error;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::helpers::{is_truthy, js_string};
use crate::fyo::model::types::{DocContext, Formula, FormulaValue, Model};
use crate::models::base_models::pricing_rule::{map_money, map_number, map_text, rule_for_row};
use crate::models::base_models::tax_summary;
use crate::models::helpers::{linked_value, money, number, query, single_is_truthy, text};

pub struct InvoiceItem;

type FormulaResult = Result<Option<FormulaValue>, Error>;

fn some(value: DocValue) -> FormulaResult {
    Ok(Some(value.into()))
}

fn some_money(value: Money) -> FormulaResult {
    some(DocValue::Money(value))
}

/// `isSales`.
fn is_sales(doc: &Doc) -> bool {
    matches!(doc.schema_name(), "SalesInvoiceItem" | "SalesQuoteItem")
}

/// `discountAfterTax`.
fn discount_after_tax(ctx: &DocContext<'_>) -> bool {
    ctx.parent
        .is_some_and(|parent| is_truthy(parent.get("discountAfterTax")))
}

/// `exchangeRate`: the invoice's, or 1.
fn exchange_rate(ctx: &DocContext<'_>) -> f64 {
    ctx.parent
        .and_then(|parent| number(parent, "exchangeRate"))
        .unwrap_or(1.0)
}

/// `isReturn`.
fn is_return(ctx: &DocContext<'_>) -> bool {
    ctx.parent
        .is_some_and(|parent| is_truthy(parent.get("returnAgainst")))
}

fn enable_discounting(ctx: &DocContext<'_>) -> bool {
    single_is_truthy(ctx.fyo, "AccountingSettings", "enableDiscounting")
}

fn enable_pricing_rule(ctx: &DocContext<'_>) -> bool {
    single_is_truthy(ctx.fyo, "AccountingSettings", "enablePricingRule")
}

/// Whether the invoice has a pricing rule for the row's item.
fn has_pricing_rule_detail(ctx: &DocContext<'_>) -> bool {
    let item = ctx.doc.get("item");
    ctx.parent.is_some_and(|parent| {
        parent
            .table("pricingRuleDetail")
            .iter()
            .any(|rule| rule.get("referenceItem") == item)
    })
}

/// `fyo.getValue('Item', this.item, fieldname)`.
fn item_value(ctx: &DocContext<'_>, fieldname: &str) -> Option<DocValue> {
    linked_value(ctx.fyo, "Item", text(ctx.doc, "item"), fieldname)
}

fn rate_or_zero(ctx: &DocContext<'_>) -> Money {
    money(ctx.doc, "rate").unwrap_or_else(|| ctx.fyo.pesa().zero())
}

fn quantity_or_one(doc: &Doc) -> f64 {
    number(doc, "quantity").unwrap_or(1.0)
}

fn item_discount_amount_or_zero(ctx: &DocContext<'_>) -> Money {
    money(ctx.doc, "itemDiscountAmount").unwrap_or_else(|| ctx.fyo.pesa().zero())
}

fn item_discount_percent_or_zero(doc: &Doc) -> f64 {
    number(doc, "itemDiscountPercent").unwrap_or(0.0)
}

/// `getTotalTaxRate`: the sum of the rates of the row's tax template.
fn get_total_tax_rate(ctx: &DocContext<'_>) -> f64 {
    let Some(tax) = text(ctx.doc, "tax").filter(|t| !t.is_empty()) else {
        return 0.0;
    };
    match ctx.fyo.get_value("Tax", Some(tax), "details") {
        Some(DocValue::Table(details)) => details
            .iter()
            .map(|detail| match detail.get("rate") {
                Some(DocValue::Number(rate)) => *rate,
                _ => 0.0,
            })
            .sum(),
        _ => 0.0,
    }
}

impl Model for InvoiceItem {
    /// Only `item` is ported; the batch and transfer unit filters come with inventory.
    fn link_filters(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<QueryFilter> {
        if fieldname != "item" {
            return None;
        }
        let not_for = if ctx.doc.schema_name() == "SalesInvoiceItem" {
            "Purchases"
        } else {
            "Sales"
        };
        Some(query([(
            "for",
            Filter::list("not in", vec![not_for.into()]),
        )]))
    }

    fn formulas(&self) -> Vec<Formula> {
        vec![
            Formula {
                fieldname: "description",
                depends_on: Some(&["item"]),
                formula: |ctx, _| Ok(item_value(ctx, "description").map(Into::into)),
            },
            Formula {
                fieldname: "itemCode",
                depends_on: Some(&["item"]),
                formula: |ctx, _| Ok(item_value(ctx, "itemCode").map(Into::into)),
            },
            Formula {
                fieldname: "rate",
                depends_on: Some(&[
                    "date",
                    "priceList",
                    "batch",
                    "party",
                    "exchangeRate",
                    "item",
                    "quantity",
                    "itemTaxedTotal",
                    "itemDiscountedTotal",
                    "setItemDiscountAmount",
                    "pricingRuleDetail",
                ]),
                formula: rate_formula,
            },
            Formula {
                fieldname: "unit",
                depends_on: Some(&["item"]),
                formula: |ctx, _| Ok(item_value(ctx, "unit").map(Into::into)),
            },
            Formula {
                fieldname: "transferUnit",
                depends_on: Some(&["item", "unit"]),
                formula: |ctx, fieldname| {
                    let Some(item) = text(ctx.doc, "item").filter(|i| !i.is_empty()) else {
                        return Ok(None);
                    };
                    let unit = ctx.doc.get("unit").cloned().unwrap_or(DocValue::Null);
                    if matches!(fieldname, Some("quantity" | "unit")) {
                        return some(unit);
                    }

                    // Books returns the unit when the item has conversions, and so never
                    // keeps a transfer unit when the item changes. Latte keeps it when
                    // it's one of them.
                    let conversions = uom_conversions(ctx, item, None)?;
                    let transfer_unit = ctx.doc.get("transferUnit");
                    if is_truthy(transfer_unit)
                        && conversions
                            .iter()
                            .any(|row| row.get("uom") == transfer_unit)
                    {
                        return Ok(transfer_unit.cloned().map(Into::into));
                    }
                    some(unit)
                },
            },
            Formula {
                fieldname: "transferQuantity",
                depends_on: Some(&["item", "quantity"]),
                formula: |ctx, fieldname| {
                    let doc = ctx.doc;
                    if fieldname == Some("quantity") || doc.get("unit") == doc.get("transferUnit") {
                        return Ok(doc.get("quantity").cloned().map(Into::into));
                    }
                    Ok(doc.get("transferQuantity").cloned().map(Into::into))
                },
            },
            Formula {
                fieldname: "quantity",
                depends_on: Some(&[
                    "quantity",
                    "transferQuantity",
                    "transferUnit",
                    "unitConversionFactor",
                    "item",
                    "isReturn",
                ]),
                formula: |ctx, fieldname| {
                    let doc = ctx.doc;
                    let Some(item) = text(doc, "item").filter(|i| !i.is_empty()) else {
                        return Ok(doc.get("quantity").cloned().map(Into::into));
                    };
                    // `getDoc` throws for an item that doesn't exist. Books looks for the
                    // whole-number unit in the item's `uom`, which Item doesn't have, so it
                    // never rounds. Latte looks in its `unit`.
                    let item = ctx.fyo.db.get_doc("Item", item)?;
                    let is_whole = match item.get("unit") {
                        Some(DocValue::Text(unit)) if !unit.is_empty() => {
                            is_truthy(ctx.fyo.get_value("UOM", Some(unit), "isWhole").as_ref())
                        }
                        _ => false,
                    };

                    let mut quantity = quantity_or_one(doc);
                    let is_return = is_return(ctx);
                    if (is_return && quantity > 0.0) || (!is_return && quantity < 0.0) {
                        quantity *= -1.0;
                    }
                    if fieldname == Some("transferQuantity") {
                        quantity = number(doc, "transferQuantity").unwrap_or(0.0)
                            * number(doc, "unitConversionFactor").unwrap_or(0.0);
                    }
                    if is_whole {
                        // `Math.round`, which rounds halves up.
                        return some(DocValue::Number((quantity + 0.5).floor()));
                    }
                    // `safeParseFloat`.
                    let quantity = if quantity.is_nan() { 0.0 } else { quantity };
                    some(DocValue::Number(quantity))
                },
            },
            Formula {
                fieldname: "unitConversionFactor",
                depends_on: Some(&["transferUnit"]),
                formula: |ctx, _| {
                    let doc = ctx.doc;
                    if doc.get("unit") == doc.get("transferUnit") {
                        return some(DocValue::Number(1.0));
                    }

                    let item = text(doc, "item").unwrap_or_default();
                    let transfer_unit = js_string(doc.get("transferUnit"));
                    let conversions = uom_conversions(ctx, item, Some(&transfer_unit))?;
                    let factor = conversions
                        .first()
                        .and_then(|row| match row.get("conversionFactor") {
                            Some(DocValue::Number(factor)) if !factor.is_nan() => Some(*factor),
                            _ => None,
                        })
                        .unwrap_or(0.0);
                    some(DocValue::Number(factor))
                },
            },
            Formula {
                fieldname: "account",
                depends_on: Some(&["item"]),
                formula: |ctx, _| {
                    let account_type = if is_sales(ctx.doc) {
                        "incomeAccount"
                    } else {
                        "expenseAccount"
                    };
                    Ok(item_value(ctx, account_type).map(Into::into))
                },
            },
            Formula {
                fieldname: "tax",
                depends_on: Some(&["item"]),
                formula: |ctx, _| {
                    let item_tax = item_value(ctx, "tax");
                    if is_truthy(item_tax.as_ref()) {
                        return Ok(item_tax.map(Into::into));
                    }

                    let item_group = match item_value(ctx, "itemGroup") {
                        Some(DocValue::Text(group)) if !group.is_empty() => group,
                        _ => return some(DocValue::Text(String::new())),
                    };
                    let item_group = ctx.fyo.db.get_doc("ItemGroup", &item_group)?;
                    some(item_group.get("tax").cloned().unwrap_or(DocValue::Null))
                },
            },
            Formula {
                fieldname: "amount",
                depends_on: Some(&["item", "rate", "quantity"]),
                formula: |ctx, _| {
                    let rate = money(ctx.doc, "rate")
                        .ok_or_else(|| Error::Value("rate is not money".into()))?;
                    let quantity = number(ctx.doc, "quantity")
                        .ok_or_else(|| Error::Value("quantity is not a number".into()))?;
                    some_money(rate.mul(quantity)?)
                },
            },
            Formula {
                fieldname: "hsnCode",
                depends_on: Some(&["item"]),
                formula: |ctx, _| Ok(item_value(ctx, "hsnCode").map(Into::into)),
            },
            Formula {
                fieldname: "itemDiscountedTotal",
                depends_on: Some(&[
                    "itemDiscountAmount",
                    "itemDiscountPercent",
                    "itemTaxedTotal",
                    "setItemDiscountAmount",
                    "tax",
                    "rate",
                    "quantity",
                    "item",
                ]),
                formula: |ctx, _| {
                    let doc = ctx.doc;
                    let total_tax_rate = get_total_tax_rate(ctx);
                    let rate = rate_or_zero(ctx);
                    let quantity = quantity_or_one(doc);
                    let item_discount_amount = item_discount_amount_or_zero(ctx);
                    let item_discount_percent = item_discount_percent_or_zero(doc);
                    let set_item_discount_amount = is_truthy(doc.get("setItemDiscountAmount"));

                    if set_item_discount_amount
                        && money(doc, "itemDiscountAmount").is_some_and(|m| m.is_zero())
                    {
                        return some_money(rate.mul(quantity)?);
                    }
                    if !set_item_discount_amount
                        && doc.get("itemDiscountPercent") == Some(&DocValue::Number(0.0))
                    {
                        return some_money(rate.mul(quantity)?);
                    }

                    if !discount_after_tax(ctx) {
                        return some_money(get_discounted_total_before_taxation(
                            &rate,
                            quantity,
                            &item_discount_amount,
                            item_discount_percent,
                            set_item_discount_amount,
                        )?);
                    }
                    some_money(get_discounted_total_after_taxation(
                        total_tax_rate,
                        &rate,
                        quantity,
                        &item_discount_amount,
                        item_discount_percent,
                        set_item_discount_amount,
                    )?)
                },
            },
            Formula {
                fieldname: "itemTaxedTotal",
                // Books' list is only rate, quantity and item, so a changed discount or tax
                // leaves the total as it was.
                depends_on: Some(&[
                    "itemDiscountAmount",
                    "itemDiscountPercent",
                    "setItemDiscountAmount",
                    "tax",
                    "rate",
                    "quantity",
                    "item",
                ]),
                formula: |ctx, _| {
                    let doc = ctx.doc;
                    let total_tax_rate = get_total_tax_rate(ctx);
                    let rate = rate_or_zero(ctx);
                    let quantity = quantity_or_one(doc);

                    if !discount_after_tax(ctx) {
                        return some_money(get_taxed_total_after_discounting(
                            total_tax_rate,
                            &rate,
                            quantity,
                            &item_discount_amount_or_zero(ctx),
                            item_discount_percent_or_zero(doc),
                            is_truthy(doc.get("setItemDiscountAmount")),
                        )?);
                    }
                    some_money(get_taxed_total_before_discounting(
                        total_tax_rate,
                        &rate,
                        quantity,
                    )?)
                },
            },
            Formula {
                fieldname: "stockNotTransferred",
                depends_on: Some(&["item", "quantity"]),
                formula: stock_not_transferred_formula,
            },
            Formula {
                fieldname: "itemDiscountAmount",
                depends_on: Some(&["pricingRuleDetail", "quantity", "item"]),
                formula: |ctx, _| {
                    if !enable_pricing_rule(ctx) || is_return(ctx) {
                        return Ok(None);
                    }
                    let Some(rule) = rule_for_row(ctx)? else {
                        return Ok(None);
                    };
                    if map_text(&rule, "priceDiscountType") != Some("amount") {
                        return Ok(None);
                    }
                    let amount = map_money(&rule, "discountAmount")
                        .cloned()
                        .unwrap_or_else(|| ctx.fyo.pesa().zero());
                    some(DocValue::Money(amount))
                },
            },
            Formula {
                fieldname: "setItemDiscountAmount",
                depends_on: Some(&["pricingRuleDetail", "quantity", "item"]),
                formula: |ctx, _| {
                    if !enable_pricing_rule(ctx) {
                        return Ok(ctx
                            .doc
                            .get("setItemDiscountAmount")
                            .cloned()
                            .map(Into::into));
                    }
                    if is_return(ctx) {
                        return Ok(None);
                    }
                    let zero = money(ctx.doc, "itemDiscountAmount").is_none_or(|m| m.is_zero());
                    if !has_pricing_rule_detail(ctx) && zero {
                        return some(DocValue::Bool(false));
                    }
                    match rule_for_row(ctx)? {
                        Some(rule) => some(DocValue::Bool(
                            map_text(&rule, "priceDiscountType") == Some("amount"),
                        )),
                        None => some(DocValue::Bool(!zero)),
                    }
                },
            },
            Formula {
                fieldname: "itemDiscountPercent",
                depends_on: Some(&["pricingRuleDetail", "item"]),
                formula: |ctx, _| {
                    if !enable_pricing_rule(ctx) {
                        return some(DocValue::Number(item_discount_percent_or_zero(ctx.doc)));
                    }
                    if is_return(ctx) {
                        return Ok(None);
                    }
                    let item = ctx.doc.get("item");
                    let detail = ctx.parent.and_then(|parent| {
                        parent
                            .table("pricingRuleDetail")
                            .iter()
                            .find(|rule| rule.get("referenceItem") == item)
                            .and_then(|rule| text(rule, "referenceName"))
                            .map(str::to_owned)
                    });
                    let Some(rule_name) = detail else {
                        return Ok(ctx.doc.get("itemDiscountPercent").cloned().map(Into::into));
                    };
                    let rule = ctx.fyo.db.get_doc("PricingRule", &rule_name)?;
                    if map_text(&rule, "discountType") != Some("Product Discount")
                        && map_text(&rule, "priceDiscountType") == Some("percentage")
                    {
                        return some(DocValue::Number(map_number(&rule, "discountPercentage")));
                    }
                    some(DocValue::Number(item_discount_percent_or_zero(ctx.doc)))
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
            "rate" => {
                let rate = as_money(value)?;
                if rate.gte(0.0)? {
                    return Ok(());
                }
                Err(Error::Validation(format!(
                    "Rate ({}) cannot be less zero.",
                    crate::fyo::format::currency(ctx.fyo, rate)
                )))
            }
            "itemDiscountAmount" => {
                let discount_amount = as_money(value)?;
                let amount = money(ctx.doc, "amount")
                    .ok_or_else(|| Error::Value("amount is not money".into()))?;
                if discount_amount.lte(&amount)? {
                    return Ok(());
                }
                Err(Error::Validation(format!(
                    "Discount Amount ({}) cannot be greated than Amount ({}).",
                    crate::fyo::format::currency(ctx.fyo, discount_amount),
                    crate::fyo::format::currency(ctx.fyo, &amount)
                )))
            }
            "itemDiscountPercent" => {
                let percent = match value {
                    DocValue::Number(percent) => *percent,
                    _ => f64::NAN,
                };
                if percent < 100.0 {
                    return Ok(());
                }
                Err(Error::Validation(format!(
                    "Discount Percent ({}) cannot be greater than 100.",
                    js_string(Some(value))
                )))
            }
            "transferUnit" => {
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
            _ => Ok(()),
        }
    }

    fn hidden(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<bool> {
        let doc = ctx.doc;
        let enable_discounting = enable_discounting(ctx);
        let set_item_discount_amount = is_truthy(doc.get("setItemDiscountAmount"));
        let inventory = |fieldname| single_is_truthy(ctx.fyo, "InventorySettings", fieldname);
        Some(match fieldname {
            "itemDiscountedTotal" => {
                !enable_discounting
                    || (set_item_discount_amount
                        && money(doc, "itemDiscountAmount").is_some_and(|m| m.is_zero()))
                    || (!set_item_discount_amount
                        && doc.get("itemDiscountPercent") == Some(&DocValue::Number(0.0)))
            }
            "setItemDiscountAmount" => !enable_discounting,
            "itemDiscountAmount" => !(enable_discounting && set_item_discount_amount),
            "itemDiscountPercent" => !(enable_discounting && !set_item_discount_amount),
            "batch" => !inventory("enableBatches"),
            "transferUnit" | "transferQuantity" | "unitConversionFactor" => {
                !inventory("enableUomConversions")
            }
            _ => return None,
        })
    }

    fn currency(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<String> {
        let field = ctx.doc.schema().field(fieldname)?;
        (field.fieldtype == Some(FieldType::Currency)).then(|| tax_summary::get_currency(ctx))
    }
}

fn as_money(value: &DocValue) -> Result<&Money, Error> {
    match value {
        DocValue::Money(money) => Ok(money),
        value => Err(Error::Value(format!(
            "{} is not money",
            js_string(Some(value))
        ))),
    }
}

/// The item's unit conversions, of one unit if given.
fn uom_conversions(
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

/// The `rate` formula: the item's rate in the invoice's currency, unless a total was set,
/// when the rate is worked back from it.
fn rate_formula(ctx: &DocContext<'_>, fieldname: Option<&str>) -> FormulaResult {
    let doc = ctx.doc;
    let rate = get_item_rate(ctx)?;
    let current = money(doc, "rate");
    let rate_is_zero = rate.as_ref().is_none_or(Money::is_zero);
    if rate_is_zero && let Some(current) = current.filter(|c| !c.is_zero()) {
        return some_money(current);
    }

    if !matches!(fieldname, Some("itemTaxedTotal" | "itemDiscountedTotal")) {
        return match rate {
            Some(rate) => some_money(rate.div(exchange_rate(ctx))?),
            None => some_money(ctx.fyo.pesa().zero()),
        };
    }

    let rate_from_totals = get_rate(
        quantity_or_zero(doc),
        item_discount_percent_or_zero(doc),
        &item_discount_amount_or_zero(ctx),
        get_total_tax_rate(ctx),
        &money(doc, "itemTaxedTotal").unwrap_or_else(|| ctx.fyo.pesa().zero()),
        &money(doc, "itemDiscountedTotal").unwrap_or_else(|| ctx.fyo.pesa().zero()),
        fieldname == Some("itemTaxedTotal"),
        discount_after_tax(ctx),
        is_truthy(doc.get("setItemDiscountAmount")),
    )?;
    some_money(
        rate_from_totals
            .or(rate)
            .unwrap_or_else(|| ctx.fyo.pesa().zero()),
    )
}

/// `this.quantity ?? 0`, as the `rate` formula reads it.
fn quantity_or_zero(doc: &Doc) -> f64 {
    number(doc, "quantity").unwrap_or(0.0)
}

/// The `stockNotTransferred` formula: how much of the row is still to be shipped or
/// received.
fn stock_not_transferred_formula(ctx: &DocContext<'_>, _: Option<&str>) -> FormulaResult {
    let doc = ctx.doc;
    if ctx.parent.is_some_and(Doc::is_submitted) {
        return Ok(None);
    }

    // `loadAndGetLink('item')` gives null for a row without an item, and reading
    // `trackItem` of it throws.
    let item = text(doc, "item")
        .filter(|i| !i.is_empty())
        .ok_or_else(|| Error::Value("Cannot read properties of null".into()))?;
    let item_doc = ctx.fyo.db.get_doc("Item", item)?;
    if !is_truthy(item_doc.get("trackItem")) {
        return some(DocValue::Number(0.0));
    }

    let back_reference = ctx
        .parent
        .and_then(|parent| text(parent, "backReference"))
        .filter(|b| !b.is_empty());
    let stock_transfer_schema_name = ctx.parent.map(|parent| {
        if matches!(parent.schema_name(), "SalesInvoice" | "SalesQuote") {
            "Shipment"
        } else {
            "PurchaseReceipt"
        }
    });
    let (Some(back_reference), Some(schema_name), Some(quantity)) = (
        back_reference,
        stock_transfer_schema_name,
        number(doc, "quantity"),
    ) else {
        return Ok(doc.get("quantity").cloned().map(Into::into));
    };

    let refdoc = ctx.fyo.db.get_doc(schema_name, back_reference)?;
    let rows = match refdoc.get("items") {
        Some(DocValue::Table(rows)) => rows.as_slice(),
        _ => &[],
    };
    // Books reduces with `i.quantity ?? 0 + acc`, which is the last row's quantity. Latte
    // adds them up.
    let transferred: f64 = rows
        .iter()
        .filter(|row| row.get("item") == doc.get("item"))
        .map(|row| match row.get("quantity") {
            Some(DocValue::Number(quantity)) => *quantity,
            _ => 0.0,
        })
        .sum();
    some(DocValue::Number((quantity - transferred).max(0.0)))
}

/// `getItemRate`: the free item's own rate, else the pricing rule's, the price list's, or
/// the item's.
fn get_item_rate(ctx: &DocContext<'_>) -> Result<Option<Money>, Error> {
    let doc = ctx.doc;
    if is_truthy(doc.get("isFreeItem")) {
        return Ok(money(doc, "rate"));
    }

    let settings = |fieldname| single_is_truthy(ctx.fyo, "AccountingSettings", fieldname);
    if settings("enablePricingRule")
        && let Some(rate) = get_item_rate_from_pricing_rule(ctx)?
    {
        return Ok(Some(rate));
    }

    if settings("enablePriceList") {
        let price_list = ctx.parent.and_then(|parent| text(parent, "priceList"));
        if let Some(rate) = get_item_rate_from_price_list(ctx, price_list)? {
            return Ok(Some(rate));
        }
    }

    if text(doc, "item").is_none_or(str::is_empty) {
        return Ok(None);
    }
    match item_value(ctx, "rate") {
        Some(DocValue::Money(rate)) => Ok(Some(rate)),
        _ => Ok(None),
    }
}

/// `getItemRateFromPricingRule`.
fn get_item_rate_from_pricing_rule(ctx: &DocContext<'_>) -> Result<Option<Money>, Error> {
    let Some(parent) = ctx.parent else {
        return Ok(None);
    };
    let Some(detail) = parent
        .table("pricingRuleDetail")
        .iter()
        .find(|detail| detail.get("referenceItem") == ctx.doc.get("item"))
    else {
        return Ok(None);
    };

    let name = js_string(detail.get("referenceName"));
    let pricing_rule = ctx.fyo.db.get_doc("PricingRule", &name)?;
    let is = |fieldname, value: &str| matches!(pricing_rule.get(fieldname), Some(DocValue::Text(v)) if v == value);
    if !is("discountType", "Price Discount") || !is("priceDiscountType", "rate") {
        return Ok(None);
    }
    match pricing_rule.get("discountRate") {
        Some(DocValue::Money(rate)) => Ok(Some(rate.clone())),
        _ => Ok(None),
    }
}

/// `getItemRateFromPriceList` in models/helpers.ts: the rate of the price list's row for
/// the item in the row's unit.
fn get_item_rate_from_price_list(
    ctx: &DocContext<'_>,
    price_list_name: Option<&str>,
) -> Result<Option<Money>, Error> {
    let doc = ctx.doc;
    let item = doc.get("item");
    let (Some(price_list_name), true) =
        (price_list_name.filter(|p| !p.is_empty()), is_truthy(item))
    else {
        return Ok(None);
    };

    let price_list = ctx.fyo.db.get_doc("PriceList", price_list_name)?;
    let rows = match price_list.get("priceListItem") {
        Some(DocValue::Table(rows)) => rows.as_slice(),
        _ => &[],
    };
    let unit = doc.get("unit");
    let transfer_unit = doc.get("transferUnit");
    let row = rows.iter().find(|row| {
        if row.get("item") != item {
            return false;
        }
        if is_truthy(transfer_unit) && row.get("unit") != transfer_unit {
            return false;
        }
        !is_truthy(unit) || row.get("unit") == unit
    });
    match row.and_then(|row| row.get("rate")) {
        Some(DocValue::Money(rate)) => Ok(Some(rate.clone())),
        _ => Ok(None),
    }
}

/// `getDiscountedTotalBeforeTaxation`. Books' comment says the percent case is the amount
/// less the discount, but pesa's `percent` gives the discount itself, so Books shows the
/// discount. Latte gives the amount less the discount.
fn get_discounted_total_before_taxation(
    rate: &Money,
    quantity: f64,
    item_discount_amount: &Money,
    item_discount_percent: f64,
    set_discount_amount: bool,
) -> Result<Money, Error> {
    if set_discount_amount {
        return Ok(rate.sub(item_discount_amount)?.mul(quantity)?);
    }
    if item_discount_percent > 0.0 {
        let amount = rate.mul(quantity)?;
        let discount = amount.percent(item_discount_percent)?;
        return Ok(amount.sub(&discount)?);
    }
    Ok(rate.mul(quantity)?)
}

/// `getTaxedTotalAfterDiscounting`.
fn get_taxed_total_after_discounting(
    total_tax_rate: f64,
    rate: &Money,
    quantity: f64,
    item_discount_amount: &Money,
    item_discount_percent: f64,
    set_item_discount_amount: bool,
) -> Result<Money, Error> {
    let discounted_total = get_discounted_total_before_taxation(
        rate,
        quantity,
        item_discount_amount,
        item_discount_percent,
        set_item_discount_amount,
    )?;
    Ok(discounted_total.mul(1.0 + total_tax_rate / 100.0)?)
}

/// `getDiscountedTotalAfterTaxation`.
fn get_discounted_total_after_taxation(
    total_tax_rate: f64,
    rate: &Money,
    quantity: f64,
    item_discount_amount: &Money,
    item_discount_percent: f64,
    set_item_discount_amount: bool,
) -> Result<Money, Error> {
    let taxed_total = get_taxed_total_before_discounting(total_tax_rate, rate, quantity)?;
    if set_item_discount_amount {
        return Ok(taxed_total.sub(item_discount_amount)?);
    }
    Ok(taxed_total.mul(1.0 - item_discount_percent / 100.0)?)
}

/// `getTaxedTotalBeforeDiscounting`.
fn get_taxed_total_before_discounting(
    total_tax_rate: f64,
    rate: &Money,
    quantity: f64,
) -> Result<Money, Error> {
    Ok(rate.mul(quantity)?.mul(1.0 + total_tax_rate / 100.0)?)
}

/// `getRate`: the rate that gives the row's taxed or discounted total.
#[allow(clippy::too_many_arguments)]
fn get_rate(
    quantity: f64,
    item_discount_percent: f64,
    item_discount_amount: &Money,
    total_tax_rate: f64,
    item_taxed_total: &Money,
    item_discounted_total: &Money,
    is_item_taxed_total: bool,
    discount_after_tax: bool,
    set_item_discount_amount: bool,
) -> Result<Option<Money>, Error> {
    let is_item_discounted_total = !is_item_taxed_total;
    let discount_before_tax = !discount_after_tax;
    let tax_factor = 1.0 + total_tax_rate / 100.0;

    let rate = if is_item_discounted_total && discount_before_tax && set_item_discount_amount {
        item_discounted_total
            .add(item_discount_amount)?
            .div(quantity)?
    } else if is_item_discounted_total && discount_before_tax && !set_item_discount_amount {
        item_discounted_total.div(quantity * (1.0 - item_discount_percent / 100.0))?
    } else if is_item_discounted_total && discount_after_tax && set_item_discount_amount {
        item_discounted_total
            .add(item_discount_amount)?
            .div(quantity * tax_factor)?
    } else if is_item_discounted_total && discount_after_tax && !set_item_discount_amount {
        item_discounted_total
            .div((quantity * (100.0 - item_discount_percent) * (100.0 + total_tax_rate)) / 100.0)?
    } else if is_item_taxed_total && discount_after_tax {
        item_taxed_total.div(quantity * tax_factor)?
    } else if is_item_taxed_total && discount_before_tax && set_item_discount_amount {
        item_taxed_total
            .div(tax_factor)?
            .add(item_discount_amount)?
            .div(quantity)?
    } else {
        item_taxed_total.div(quantity * (1.0 - item_discount_percent / 100.0) * tax_factor)?
    };
    Ok(Some(rate))
}
