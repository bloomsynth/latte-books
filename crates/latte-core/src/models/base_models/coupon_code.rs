// Port of models/baseModels/CouponCode/CouponCode.ts and AppliedCouponCodes.ts @ v0.36.0,
// with the coupon helpers of models/helpers.ts (`validateCouponCode`,
// `getApplicableCouponCodesName`, `removeUnusedCoupons`) and Invoice's
// `updateUsedCountOfCoupons` and `reduceUsedCountOfCoupons`.
//
// Books bugs Latte fixes:
// - A coupon used before its Valid From date or after its Valid To date is refused with
//   a message saying so. Books says "Valid From Date should be less than Valid To Date."
//   and "Valid To Date should be greater than Valid From Date.", as if the coupon were
//   set up wrong.
// - Cancelling a return uses its coupons again. Books frees them a second time.
//
// Not ported yet, as the UI needs them: `getListViewSettings`.

use jiff::Timestamp;
use latte_db::pesa::Money;
use latte_db::{DocValue, DocValueMap, Filter, GetAllOptions, QueryFilter};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::helpers::{is_truthy, js_number_string};
use crate::fyo::model::types::{DocContext, Formula, FormulaValue, Model};
use crate::models::base_models::invoice::is_return;
use crate::models::base_models::pricing_rule::{
    applied_coupons, get_pricing_rule, map_money, map_number, map_text,
};
use crate::models::helpers::{money, query, text};

pub struct CouponCode;

pub struct AppliedCouponCodes;

fn validation<T>(message: impl Into<String>) -> Result<T, Error> {
    Err(Error::Validation(message.into()))
}

fn date(value: Option<&DocValue>) -> Option<Timestamp> {
    match value {
        Some(DocValue::Date(date)) => Some(*date),
        _ => None,
    }
}

/// `pricingRuleData`: the coupon's pricing rule's amounts and dates.
fn pricing_rule_data(doc: &Doc, fyo: &Fyo) -> Option<DocValueMap> {
    let rule = text(doc, "pricingRule").filter(|rule| !rule.is_empty())?;
    fyo.db.get_doc("PricingRule", rule).ok()
}

fn is_zero(money: Option<&Money>) -> bool {
    money.is_none_or(Money::is_zero)
}

impl Model for CouponCode {
    fn formulas(&self) -> Vec<Formula> {
        vec![Formula {
            fieldname: "name",
            depends_on: Some(&["couponName"]),
            formula: |ctx, _| {
                let Some(name) = text(ctx.doc, "couponName") else {
                    return Ok(None);
                };
                let name: String = name
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .flat_map(char::to_uppercase)
                    .take(8)
                    .collect();
                Ok(Some(FormulaValue::from(DocValue::Text(name))))
            },
        }]
    }

    fn validate_field(
        &self,
        fieldname: &str,
        value: &DocValue,
        ctx: &DocContext<'_>,
    ) -> Result<(), Error> {
        let doc = ctx.doc;
        match (fieldname, value) {
            ("minAmount", DocValue::Money(value)) => {
                let Some(max) = money(doc, "maxAmount") else {
                    return Ok(());
                };
                let Some(rule) = pricing_rule_data(doc, ctx.fyo) else {
                    return Ok(());
                };
                if is_zero(map_money(&rule, "minAmount")) && is_zero(map_money(&rule, "maxAmount"))
                {
                    return Ok(());
                }
                if value.is_zero() && max.is_zero() {
                    return Ok(());
                }
                if let Some(min) = map_money(&rule, "minAmount")
                    && value.lt(min)?
                {
                    return validation(
                        "Minimum Amount should be greather than the Pricing Rule's Minimum Amount.",
                    );
                }
                if value.gte(&max)? {
                    return validation("Minimum Amount should be less than the Maximum Amount.");
                }
            }
            ("maxAmount", DocValue::Money(value)) => {
                let Some(min) = money(doc, "minAmount") else {
                    return Ok(());
                };
                let Some(rule) = pricing_rule_data(doc, ctx.fyo) else {
                    return Ok(());
                };
                if is_zero(map_money(&rule, "minAmount")) && is_zero(map_money(&rule, "maxAmount"))
                {
                    return Ok(());
                }
                if min.is_zero() && value.is_zero() {
                    return Ok(());
                }
                if let Some(max) = map_money(&rule, "maxAmount")
                    && value.gt(max)?
                {
                    return validation(
                        "Maximum Amount should be lesser than Pricing Rule's Maximum Amount",
                    );
                }
                if value.lte(&min)? {
                    return validation("Maximum Amount should be greater than the Minimum Amount.");
                }
            }
            ("validFrom", DocValue::Date(value)) => {
                let Some(to) = date(doc.get("validTo")) else {
                    return Ok(());
                };
                let Some(rule) = pricing_rule_data(doc, ctx.fyo) else {
                    return Ok(());
                };
                let from = date(rule.get("validFrom"));
                if from.is_none() && date(rule.get("validTo")).is_none() {
                    return Ok(());
                }
                if from.is_some_and(|from| *value < from) {
                    return validation(
                        "Valid From Date should be greather than Pricing Rule's Valid From Date.",
                    );
                }
                if *value >= to {
                    return validation("Valid From Date should be less than Valid To Date.");
                }
            }
            ("validTo", DocValue::Date(value)) => {
                let Some(from) = date(doc.get("validFrom")) else {
                    return Ok(());
                };
                let Some(rule) = pricing_rule_data(doc, ctx.fyo) else {
                    return Ok(());
                };
                let to = date(rule.get("validTo"));
                if date(rule.get("validFrom")).is_none() && to.is_none() {
                    return Ok(());
                }
                if to.is_some_and(|to| *value > to) {
                    return validation(
                        "Valid To Date should be lesser than Pricing Rule's Valid To Date.",
                    );
                }
                if *value <= from {
                    return validation("Valid To Date should be greater than Valid From Date.");
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn link_filters(&self, fieldname: &str, _ctx: &DocContext<'_>) -> Option<QueryFilter> {
        (fieldname == "pricingRule").then(|| query([("isCouponCodeBased", Filter::from(true))]))
    }
}

impl Model for AppliedCouponCodes {
    fn validate_field(
        &self,
        fieldname: &str,
        value: &DocValue,
        ctx: &DocContext<'_>,
    ) -> Result<(), Error> {
        if fieldname != "coupons" || !is_truthy(Some(value)) {
            return Ok(());
        }
        let (DocValue::Text(value), Some(invoice)) = (value, ctx.parent) else {
            return Ok(());
        };
        validate_coupon_code(value, invoice, ctx.fyo)
    }
}

/// `getApplicableCouponCodesName`: whether `coupon` is enabled and its pricing rule applies
/// to an item on the invoice.
pub(crate) fn coupon_applies(coupon: &str, invoice: &Doc, fyo: &Fyo) -> Result<bool, Error> {
    let options = GetAllOptions {
        fields: Some(vec!["*".to_owned()]),
        filters: QueryFilter::from([
            ("name".to_owned(), Filter::from(coupon)),
            ("isEnabled".to_owned(), Filter::from(true)),
        ]),
        ..GetAllOptions::default()
    };
    let datas = fyo.db.get_all_docs("CouponCode", &options)?;
    let Some(data) = datas.first() else {
        return Ok(false);
    };
    let rules = get_pricing_rule(invoice, fyo, Some(coupon))?.unwrap_or_default();
    Ok(rules
        .iter()
        .any(|rule| map_text(&rule.pricing_rule, "name") == map_text(data, "pricingRule")))
}

/// `validateCouponCode`.
pub(crate) fn validate_coupon_code(value: &str, invoice: &Doc, fyo: &Fyo) -> Result<(), Error> {
    let coupon = fyo.db.get_doc("CouponCode", value).ok();
    let Some(coupon) = coupon.filter(|coupon| is_truthy(coupon.get("isEnabled"))) else {
        return validation("Coupon code cannot be applied as it is not enabled");
    };
    if map_number(&coupon, "maximumUse") <= map_number(&coupon, "used") {
        return validation("Coupon code has been used maximum number of times");
    }
    if !coupon_applies(value, invoice, fyo)? {
        return validation(format!(
            "Coupon {value} is not applicable for applied items."
        ));
    }
    if applied_coupons(invoice)
        .iter()
        .any(|coupon| coupon == value)
    {
        return validation(format!("{value} already applied."));
    }

    let grand_total = money(invoice, "grandTotal").unwrap_or_else(|| fyo.pesa().zero());
    if let Some(min) = map_money(&coupon, "minAmount")
        && !min.is_zero()
        && min.gte(&grand_total)?
    {
        return validation(format!(
            "The Grand Total must exceed {} to apply the coupon {value}.",
            js_number_string(min.float())
        ));
    }
    if let Some(max) = map_money(&coupon, "maxAmount")
        && !max.is_zero()
        && max.lte(&grand_total)?
    {
        return validation(format!(
            "The Grand Total must be less than {} to apply this coupon.",
            js_number_string(max.float())
        ));
    }

    let invoice_date = date(invoice.get("date"));
    if let (Some(from), Some(invoice_date)) = (date(coupon.get("validFrom")), invoice_date)
        && from > invoice_date
    {
        return validation(format!(
            "Coupon {value} isn't valid yet on the invoice date."
        ));
    }
    if let (Some(to), Some(invoice_date)) = (date(coupon.get("validTo")), invoice_date)
        && to < invoice_date
    {
        return validation(format!(
            "Coupon {value} is no longer valid on the invoice date."
        ));
    }
    Ok(())
}

/// `removeUnusedCoupons`: the invoice keeps only the coupons that still apply.
pub(crate) fn remove_unused_coupons(invoice: &mut Doc, fyo: &Fyo) -> Result<(), Error> {
    if invoice.table("coupons").is_empty() {
        return Ok(());
    }
    let mut kept = Vec::new();
    for row in invoice.table("coupons") {
        let coupon = text(row, "coupons").unwrap_or_default();
        if coupon_applies(coupon, invoice, fyo)? {
            kept.push(row.clone());
        }
    }
    invoice.set_table_rows("coupons", kept);
    Ok(())
}

/// `updateUsedCountOfCoupons` and `reduceUsedCountOfCoupons`: adds `by` to the uses of
/// each of the invoice's coupons.
pub(crate) fn change_used_count(invoice: &Doc, fyo: &mut Fyo, by: f64) -> Result<(), Error> {
    for coupon in applied_coupons(invoice) {
        let mut doc = fyo.get_doc("CouponCode", Some(&coupon))?;
        let used = doc
            .get("used")
            .and_then(|used| match used {
                DocValue::Number(n) => Some(*n),
                _ => None,
            })
            .unwrap_or(0.0);
        doc.set_and_sync(fyo, "used", DocValue::Number(used + by))?;
    }
    Ok(())
}

/// How a submit or cancel changes the uses of an invoice's coupons: a sale uses them, a
/// return frees them.
pub(crate) fn used_count_change(invoice: &Doc, submitted: bool) -> f64 {
    let sign = if is_return(invoice) { -1.0 } else { 1.0 };
    if submitted { sign } else { -sign }
}
