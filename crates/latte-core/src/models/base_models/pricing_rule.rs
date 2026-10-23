// Port of models/baseModels/PricingRule/PricingRule.ts and PricingRuleItem.ts @ v0.36.0,
// with the pricing rule helpers of models/helpers.ts (`getPricingRule`,
// `getPricingRulesOfCoupons`, `filterPricingRules`, `canApplyPricingRule`,
// `canApplyCouponCode`, `getPricingRulesConflicts`) and Invoice's `getPricingRule` and
// `applyProductDiscount`.
//
// Books bugs Latte fixes:
// - A coupon applies to an item whatever the order of the item's pricing rules. Books
//   compares the coupon's rule with only the first rule the item is in.
// - A free item that isn't tracked as stock is added. Books adds a free item only when
//   there is stock of it, so with inventory off, or for a service, it never is.
// - The message for a free item of no quantity is on one line.
// - Whether a rule applies to a row is worked out on the item's own rate times the
//   quantity, as the invoice does it. Books' coupon and row discount checks take the
//   row's amount, which the rule's own rate or discount has lowered, so on saving, a
//   coupon whose rule is at its minimum amount is dropped.
//
// Not ported yet, as the UI needs them: `getListViewSettings`.

use jiff::Timestamp;
use latte_db::pesa::Money;
use latte_db::{DocValue, DocValueMap, Filter, FilterValue, GetAllOptions, Order, QueryFilter};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::helpers::is_truthy;
use crate::fyo::model::types::{DocContext, Formula, Model};
use crate::models::helpers::{linked_value, number, single_is_truthy, single_text, text};

pub struct PricingRule;

pub struct PricingRuleItem;

/// `ApplicablePricingRules`: the rule that applies to an item.
#[derive(Debug, Clone)]
pub struct ApplicablePricingRule {
    pub apply_on_item: String,
    pub pricing_rule: DocValueMap,
}

pub(crate) fn map_text<'a>(row: &'a DocValueMap, fieldname: &str) -> Option<&'a str> {
    match row.get(fieldname) {
        Some(DocValue::Text(text)) if !text.is_empty() => Some(text),
        _ => None,
    }
}

pub(crate) fn map_number(row: &DocValueMap, fieldname: &str) -> f64 {
    match row.get(fieldname) {
        Some(DocValue::Number(n)) => *n,
        _ => 0.0,
    }
}

pub(crate) fn map_money<'a>(row: &'a DocValueMap, fieldname: &str) -> Option<&'a Money> {
    match row.get(fieldname) {
        Some(DocValue::Money(money)) => Some(money),
        _ => None,
    }
}

fn map_date(row: &DocValueMap, fieldname: &str) -> Option<Timestamp> {
    match row.get(fieldname) {
        Some(DocValue::Date(date)) => Some(*date),
        _ => None,
    }
}

fn doc_date(doc: &Doc) -> Option<Timestamp> {
    match doc.get("date") {
        Some(DocValue::Date(date)) => Some(*date),
        _ => None,
    }
}

/// Whether `amount` is outside a min and max where a zero or missing bound is no bound:
/// at or below the min, or at or above the max.
fn outside_amounts(row: &DocValueMap, amount: &Money) -> Result<bool, Error> {
    if let Some(min) = map_money(row, "minAmount")
        && !min.is_zero()
        && amount.lte(min)?
    {
        return Ok(true);
    }
    if let Some(max) = map_money(row, "maxAmount")
        && !max.is_zero()
        && amount.gte(max)?
    {
        return Ok(true);
    }
    Ok(false)
}

/// Whether `date` is outside `validFrom` to `validTo`.
fn outside_dates(row: &DocValueMap, date: Option<Timestamp>) -> bool {
    let Some(date) = date else {
        return false;
    };
    map_date(row, "validFrom").is_some_and(|from| date < from)
        || map_date(row, "validTo").is_some_and(|to| date > to)
}

/// `canApplyPricingRule`.
pub fn can_apply_pricing_rule(
    rule: &DocValueMap,
    date: Option<Timestamp>,
    quantity: f64,
    amount: &Money,
) -> Result<bool, Error> {
    let min_quantity = map_number(rule, "minQuantity");
    if min_quantity > 0.0 && quantity < min_quantity {
        return Ok(false);
    }
    let max_quantity = map_number(rule, "maxQuantity");
    if max_quantity > 0.0 && quantity > max_quantity {
        return Ok(false);
    }
    if outside_amounts(rule, amount)? {
        return Ok(false);
    }
    Ok(!outside_dates(rule, date))
}

/// `canApplyCouponCode`.
pub fn can_apply_coupon_code(
    coupon: &DocValueMap,
    amount: &Money,
    date: Option<Timestamp>,
) -> Result<bool, Error> {
    if outside_amounts(coupon, amount)? {
        return Ok(false);
    }
    Ok(!outside_dates(coupon, date))
}

/// `filterPricingRules`.
fn filter_pricing_rules(
    doc: &Doc,
    rules: &[DocValueMap],
    quantity: f64,
    amount: &Money,
) -> Result<Vec<DocValueMap>, Error> {
    let mut filtered = Vec::new();
    for rule in rules {
        if can_apply_pricing_rule(rule, doc_date(doc), quantity, amount)? {
            filtered.push(rule.clone());
        }
    }
    Ok(filtered)
}

/// `getPricingRulesConflicts`: whether another rule has the first one's priority.
fn has_conflicts(rules: &[DocValueMap]) -> bool {
    let Some((first, rest)) = rules.split_first() else {
        return false;
    };
    rest.iter()
        .any(|rule| rule.get("priority") == first.get("priority"))
}

/// The names of the pricing rules an invoice row's item and unit are in.
fn rule_names_for(fyo: &Fyo, item: &Doc) -> Result<Vec<String>, Error> {
    let options = GetAllOptions {
        fields: Some(vec!["parent".to_owned()]),
        filters: QueryFilter::from([
            (
                "item".to_owned(),
                Filter::from(text(item, "item").unwrap_or_default()),
            ),
            (
                "unit".to_owned(),
                Filter::from(text(item, "unit").unwrap_or_default()),
            ),
        ]),
        ..GetAllOptions::default()
    };
    Ok(fyo
        .db
        .get_all_docs("PricingRuleItem", &options)?
        .iter()
        .filter_map(|row| map_text(row, "parent").map(str::to_owned))
        .collect())
}

/// The enabled pricing rules of `names`, highest priority first, with more filters.
fn enabled_rules(
    fyo: &Fyo,
    names: &[String],
    more: &[(&str, bool)],
) -> Result<Vec<DocValueMap>, Error> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let names = names
        .iter()
        .map(|name| FilterValue::from(name.as_str()))
        .collect();
    let mut filters = QueryFilter::from([
        ("name".to_owned(), Filter::list("in", names)),
        ("isEnabled".to_owned(), Filter::from(true)),
    ]);
    for (fieldname, value) in more {
        filters.insert((*fieldname).to_owned(), Filter::from(*value));
    }
    let options = GetAllOptions {
        fields: Some(vec!["*".to_owned()]),
        filters,
        order_by: Some(vec!["priority".to_owned()]),
        order: Some(Order::Desc),
        ..GetAllOptions::default()
    };
    Ok(fyo.db.get_all_docs("PricingRule", &options)?)
}

/// The coupon codes of `names`, with all their fields.
fn coupons(fyo: &Fyo, names: &[String], enabled_only: bool) -> Result<Vec<DocValueMap>, Error> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let names = names
        .iter()
        .map(|name| FilterValue::from(name.as_str()))
        .collect();
    let mut filters = QueryFilter::from([("name".to_owned(), Filter::list("in", names))]);
    if enabled_only {
        filters.insert("isEnabled".to_owned(), Filter::from(true));
    }
    let options = GetAllOptions {
        fields: Some(vec!["*".to_owned()]),
        filters,
        ..GetAllOptions::default()
    };
    Ok(fyo.db.get_all_docs("CouponCode", &options)?)
}

/// The coupon codes on an invoice.
pub(crate) fn applied_coupons(doc: &Doc) -> Vec<String> {
    doc.table("coupons")
        .iter()
        .filter_map(|row| text(row, "coupons").map(str::to_owned))
        .collect()
}

/// `getPricingRulesOfCoupons`: the coupon-based rules of `names` that the invoice's
/// coupons, or `coupon_name`, are for.
fn get_pricing_rules_of_coupons(
    doc: &Doc,
    fyo: &Fyo,
    coupon_name: Option<&str>,
    names: &[String],
) -> Result<Option<Vec<DocValueMap>>, Error> {
    let to_fetch = match coupon_name {
        Some(name) => vec![name.to_owned()],
        None => applied_coupons(doc),
    };
    if to_fetch.is_empty() {
        return Ok(None);
    }

    let applied = coupons(fyo, &to_fetch, false)?;
    let any_for_names = applied.iter().any(|coupon| {
        map_text(coupon, "pricingRule").is_some_and(|rule| names.iter().any(|n| n == rule))
    });
    if !any_for_names {
        return Ok(None);
    }
    Ok(Some(enabled_rules(
        fyo,
        names,
        &[("isCouponCodeBased", true)],
    )?))
}

/// The total quantity of each item on the invoice.
fn item_quantity(doc: &Doc, item: &str) -> f64 {
    doc.table("items")
        .iter()
        .filter(|row| text(row, "item") == Some(item))
        .map(|row| number(row, "quantity").unwrap_or(0.0))
        .sum()
}

/// The item's own rate.
fn item_rate(fyo: &Fyo, item: &str) -> Money {
    match linked_value(fyo, "Item", Some(item), "rate") {
        Some(DocValue::Money(rate)) => rate,
        _ => fyo.pesa().zero(),
    }
}

/// `getPricingRule` in models/helpers.ts: the rule that applies to each row's item, as the
/// rows' discount formulas and the coupons read it. `None` where pricing rules are off.
pub fn get_pricing_rule(
    doc: &Doc,
    fyo: &Fyo,
    coupon_name: Option<&str>,
) -> Result<Option<Vec<ApplicablePricingRule>>, Error> {
    if !single_is_truthy(fyo, "AccountingSettings", "enablePricingRule")
        || doc.schema_name() != "SalesInvoice" && doc.schema_name() != "SalesQuote"
    {
        return Ok(None);
    }

    let mut pricing_rules = Vec::new();
    for item in doc.table("items") {
        if is_truthy(item.get("isFreeItem")) {
            continue;
        }
        let names = rule_names_for(fyo, item)?;
        let rules = enabled_rules(fyo, &names, &[("isCouponCodeBased", false)])?;
        let mut rules_for_item = (!rules.is_empty()).then_some(rules);
        if rules_for_item.is_none() || coupon_name.is_some() {
            rules_for_item = get_pricing_rules_of_coupons(doc, fyo, coupon_name, &names)?;
        }
        let Some(rules) = rules_for_item else {
            continue;
        };

        let item_name = text(item, "item").unwrap_or_default();
        let quantity = item_quantity(doc, item_name);
        // Books takes the row's amount, which a rule's own rate or discount lowers, so a
        // rule at its minimum amount stops applying once it has.
        let amount = item_rate(fyo, item_name).mul(quantity)?;
        let filtered = filter_pricing_rules(doc, &rules, quantity, &amount)?;
        if filtered.is_empty() || has_conflicts(&filtered) {
            continue;
        }
        pricing_rules.push(ApplicablePricingRule {
            apply_on_item: item_name.to_owned(),
            pricing_rule: filtered[0].clone(),
        });
    }
    Ok(Some(pricing_rules))
}

/// `ignorePricingRules`: whether the POS settings or their profile say to.
pub(crate) fn ignore_pricing_rules(fyo: &Fyo) -> bool {
    if let Some(profile) = single_text(fyo, "POSSettings", "posProfile")
        && let Ok(profile) = fyo.db.get_doc("POSProfile", &profile)
    {
        return is_truthy(profile.get("ignorePricingRule"));
    }
    single_is_truthy(fyo, "POSSettings", "ignorePricingRule")
}

/// Invoice's `getPricingRule`: the rule that applies to each row's item, as the invoice
/// lists them in `pricingRuleDetail`. `None` where pricing rules are ignored, or for a
/// purchase.
pub fn get_invoice_pricing_rules(
    doc: &Doc,
    fyo: &Fyo,
) -> Result<Option<Vec<ApplicablePricingRule>>, Error> {
    if ignore_pricing_rules(fyo) || !matches!(doc.schema_name(), "SalesInvoice" | "SalesQuote") {
        return Ok(None);
    }

    let mut pricing_rules = Vec::new();
    for item in doc.table("items") {
        if is_truthy(item.get("isFreeItem")) {
            continue;
        }
        let item_name = text(item, "item").unwrap_or_default();
        let duplicates = doc
            .table("pricingRuleDetail")
            .iter()
            .filter(|detail| text(detail, "referenceItem") == Some(item_name))
            .count();
        if duplicates >= 2 {
            continue;
        }

        let mut names = rule_names_for(fyo, item)?;
        if names.is_empty() {
            continue;
        }

        let applied = applied_coupons(doc);
        for coupon_name in &applied {
            let datas = coupons(fyo, std::slice::from_ref(coupon_name), true)?;
            let coupon_rules: Vec<String> = datas
                .iter()
                .filter_map(|coupon| map_text(coupon, "pricingRule"))
                .filter(|rule| names.iter().any(|n| n == rule))
                .map(str::to_owned)
                .collect();
            if coupon_rules.is_empty() {
                continue;
            }
            let grand_total = match doc.get("grandTotal") {
                Some(DocValue::Money(money)) => money.clone(),
                _ => fyo.pesa().zero(),
            };
            if can_apply_coupon_code(&datas[0], &grand_total, doc_date(doc))? {
                names.extend(coupon_rules);
            }
        }

        let rules = enabled_rules(fyo, &names, &[])?;
        if let Some(first) = rules.first()
            && is_truthy(first.get("isCouponCodeBased"))
        {
            let first_name = map_text(first, "name");
            let coupon_rules = coupons(fyo, &applied, false)?;
            let has_coupon = coupon_rules.iter().any(|coupon| {
                map_text(coupon, "pricingRule").is_some()
                    && map_text(coupon, "pricingRule") == first_name
            });
            if !has_coupon {
                continue;
            }
        }

        let total_amount =
            item_rate(fyo, item_name).mul(number(item, "quantity").unwrap_or(0.0))?;
        let filtered = filter_pricing_rules(
            doc,
            &rules,
            number(item, "quantity").unwrap_or(0.0),
            &total_amount,
        )?;
        if filtered.is_empty() || has_conflicts(&filtered) {
            continue;
        }
        pricing_rules.push(ApplicablePricingRule {
            apply_on_item: item_name.to_owned(),
            pricing_rule: filtered[0].clone(),
        });
    }
    Ok(Some(pricing_rules))
}

/// The rule `getPricingRule` gives a row's item, for the row's discount formulas.
pub(crate) fn rule_for_row(ctx: &DocContext<'_>) -> Result<Option<DocValueMap>, Error> {
    let Some(parent) = ctx.parent else {
        return Ok(None);
    };
    let item = text(ctx.doc, "item");
    Ok(get_pricing_rule(parent, ctx.fyo, None)?
        .unwrap_or_default()
        .into_iter()
        .find(|rule| Some(rule.apply_on_item.as_str()) == item)
        .map(|rule| rule.pricing_rule))
}

/// How much of `item` there is to give away, as `getItemQtyMap` counts it: the stock at the
/// POS location, or everywhere.
fn available_quantity(fyo: &Fyo, item: &str) -> Result<f64, Error> {
    use crate::reports::stock::{StockBalanceFilters, get_stock_balance_entries};
    use crate::reports::stock_ledger::computed_entries;

    let location = match single_text(fyo, "POSSettings", "posProfile") {
        Some(profile) => {
            linked_value(fyo, "POSProfile", Some(&profile), "inventory").and_then(|value| {
                match value {
                    DocValue::Text(text) if !text.is_empty() => Some(text),
                    _ => None,
                }
            })
        }
        None => single_text(fyo, "POSSettings", "inventory"),
    };
    let never = |_| false;
    let filters = StockBalanceFilters {
        item: None,
        location: location.as_deref(),
        batch: None,
        before: &never,
        after: &never,
    };
    Ok(get_stock_balance_entries(&computed_entries(fyo)?, &filters)
        .iter()
        .filter(|row| row.item == item)
        .map(|row| row.balance_quantity)
        .sum())
}

/// `applyProductDiscount`: a row for each free item a product discount gives.
pub(crate) fn apply_product_discount(doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
    if doc.table("items").is_empty() || ignore_pricing_rules(fyo) {
        return Ok(());
    }

    let is_return = is_truthy(doc.get("returnAgainst"));
    if !is_return {
        let items: Vec<Doc> = doc
            .table("items")
            .iter()
            .filter(|item| !is_truthy(item.get("isFreeItem")))
            .cloned()
            .collect();
        doc.set_table_rows("items", items);
    }

    let mut free_rows = Vec::new();
    for item in doc.table("items") {
        let item_name = text(item, "item").unwrap_or_default();
        let Some(detail) = doc
            .table("pricingRuleDetail")
            .iter()
            .find(|detail| text(detail, "referenceItem") == Some(item_name))
        else {
            continue;
        };
        let rule_name = text(detail, "referenceName").unwrap_or_default();
        let rule = fyo.db.get_doc("PricingRule", rule_name)?;
        if map_text(&rule, "discountType") == Some("Price Discount") {
            continue;
        }
        let applied_items = match rule.get("appliedItems") {
            Some(DocValue::Table(rows)) => rows.clone(),
            _ => Vec::new(),
        };
        if !applied_items
            .iter()
            .any(|row| map_text(row, "item") == Some(item_name))
        {
            continue;
        }

        let quantity = number(item, "quantity").unwrap_or(0.0);
        let amount = match item.get("amount") {
            Some(DocValue::Money(money)) => money.clone(),
            _ => fyo.pesa().zero(),
        };
        if !can_apply_pricing_rule(&rule, doc_date(doc), quantity, &amount)? {
            continue;
        }

        let mut free_quantity = map_number(&rule, "freeItemQuantity");
        if is_truthy(rule.get("isRecursive")) {
            free_quantity = quantity / map_number(&rule, "recurseEvery");
        }
        if is_truthy(rule.get("roundFreeItemQty")) {
            free_quantity = free_quantity.floor();
        }

        let free_item = map_text(&rule, "freeItem").unwrap_or_default().to_owned();
        if free_quantity <= 0.0 || free_quantity.is_nan() {
            return Err(Error::Validation(format!(
                "Free item \"{free_item}\" was not added due to zero quantity"
            )));
        }

        let tracked = single_is_truthy(fyo, "AccountingSettings", "enableInventory")
            && matches!(
                linked_value(fyo, "Item", Some(&free_item), "trackItem"),
                Some(DocValue::Bool(true))
            );
        if tracked && available_quantity(fyo, &free_item)? < free_quantity {
            continue;
        }

        free_rows.push(DocValueMap::from([
            ("item".to_owned(), DocValue::Text(free_item)),
            ("quantity".to_owned(), DocValue::Number(free_quantity)),
            ("isFreeItem".to_owned(), DocValue::Bool(true)),
            (
                "pricingRule".to_owned(),
                rule.get("title").cloned().unwrap_or(DocValue::Null),
            ),
            (
                "unit".to_owned(),
                rule.get("freeItemUnit").cloned().unwrap_or(DocValue::Null),
            ),
        ]));
    }

    for row in free_rows {
        doc.append(fyo, "items", row)?;
    }
    Ok(())
}

fn validation(message: &str) -> Result<(), Error> {
    Err(Error::Validation(message.to_owned()))
}

impl Model for PricingRule {
    fn validate_field(
        &self,
        fieldname: &str,
        value: &DocValue,
        ctx: &DocContext<'_>,
    ) -> Result<(), Error> {
        let doc = ctx.doc;
        let positive = |fieldname| number(doc, fieldname).filter(|n| *n != 0.0);
        let amount = |fieldname| match doc.get(fieldname) {
            Some(DocValue::Money(money)) => Some(money.clone()),
            _ => None,
        };
        let date = |fieldname| match doc.get(fieldname) {
            Some(DocValue::Date(date)) => Some(*date),
            _ => None,
        };
        match (fieldname, value) {
            ("minQuantity", DocValue::Number(value)) if *value != 0.0 => {
                if positive("maxQuantity").is_some_and(|max| *value > max) {
                    return validation(
                        "Minimum Quantity should be less than the Maximum Quantity.",
                    );
                }
            }
            ("maxQuantity", DocValue::Number(value)) if *value != 0.0 => {
                if positive("minQuantity").is_some_and(|min| *value < min) {
                    return validation(
                        "Maximum Quantity should be greater than the Minimum Quantity.",
                    );
                }
            }
            ("minAmount", DocValue::Money(value)) => {
                if let Some(max) = amount("maxAmount")
                    && !value.is_zero()
                    && !max.is_zero()
                    && value.gte(&max)?
                {
                    return validation("Minimum Amount should be less than the Maximum Amount.");
                }
            }
            ("maxAmount", DocValue::Money(value)) => {
                if let Some(min) = amount("minAmount")
                    && !min.is_zero()
                    && !value.is_zero()
                    && value.lte(&min)?
                {
                    return validation("Maximum Amount should be greater than the Minimum Amount.");
                }
            }
            ("validFrom", DocValue::Date(value))
                if date("validTo").is_some_and(|to| *value > to) =>
            {
                return validation("Valid From Date should be less than Valid To Date.");
            }
            ("validTo", DocValue::Date(value))
                if date("validFrom").is_some_and(|from| *value < from) =>
            {
                return validation("Valid To Date should be greater than Valid From Date.");
            }
            _ => {}
        }
        Ok(())
    }

    fn required(&self, fieldname: &str, ctx: &DocContext<'_>) -> bool {
        fieldname == "priceDiscountType" && is_price_discount(ctx.doc)
    }

    fn hidden(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<bool> {
        let doc = ctx.doc;
        let price = is_price_discount(doc);
        let price_type = text(doc, "priceDiscountType");
        Some(match fieldname {
            "location" => !single_is_truthy(ctx.fyo, "AccountingSettings", "enableInventory"),
            "isCouponCodeBased" => {
                !single_is_truthy(ctx.fyo, "AccountingSettings", "enableCouponCode")
            }
            "priceDiscountType" => !price,
            "discountRate" => !price || price_type != Some("rate"),
            "discountPercentage" => !price || price_type != Some("percentage"),
            "discountAmount" => !price || price_type != Some("amount"),
            "forPriceList" => !price || price_type == Some("rate"),
            "freeItem" | "freeItemQuantity" | "freeItemUnit" | "roundFreeItemQty"
            | "isRecursive" => price,
            "roundingMethod" => price || !is_truthy(doc.get("roundFreeItemQty")),
            "recurseEvery" | "recurseOver" => price || !is_truthy(doc.get("isRecursive")),
            _ => return None,
        })
    }
}

/// `isDiscountTypeIsPriceDiscount`.
fn is_price_discount(doc: &Doc) -> bool {
    text(doc, "discountType") == Some("Price Discount")
}

/// PriceListItem.ts has the same formula, so PriceListItem has this model too.
impl Model for PricingRuleItem {
    fn formulas(&self) -> Vec<Formula> {
        vec![Formula {
            fieldname: "unit",
            depends_on: Some(&["item"]),
            formula: |ctx, _| {
                let item = text(ctx.doc, "item");
                if item.is_none() {
                    return Ok(None);
                }
                Ok(linked_value(ctx.fyo, "Item", item, "unit").map(Into::into))
            },
        }]
    }
}
