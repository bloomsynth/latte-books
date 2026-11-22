// Port of models/baseModels/JournalEntryAccount/JournalEntryAccount.ts @ v0.36.0.
//
// Not ported yet, as the UI needs it: `filters`.

use latte_db::pesa::Money;
use latte_db::{DocValue, Filter, QueryFilter};

use crate::Error;
use crate::fyo::model::types::{DocContext, Formula, FormulaValue, Model};
use crate::models::helpers::query;

pub struct JournalEntryAccount;

impl Model for JournalEntryAccount {
    fn link_filters(&self, fieldname: &str, _ctx: &DocContext<'_>) -> Option<QueryFilter> {
        (fieldname == "account").then(|| query([("isGroup", Filter::from(false))]))
    }

    fn formulas(&self) -> Vec<Formula> {
        vec![
            Formula {
                fieldname: "debit",
                depends_on: None,
                formula: |ctx, _| get_auto_debit_credit(ctx, "debit"),
            },
            Formula {
                fieldname: "credit",
                depends_on: None,
                formula: |ctx, _| get_auto_debit_credit(ctx, "credit"),
            },
        ]
    }
}

fn money(ctx: &DocContext<'_>, fieldname: &str) -> Money {
    match ctx.doc.get(fieldname) {
        Some(DocValue::Money(money)) => money.clone(),
        _ => ctx.fyo.pesa().zero(),
    }
}

/// `getAutoDebitCredit`: a row with neither a debit nor a credit gets what balances the
/// entry.
fn get_auto_debit_credit(ctx: &DocContext<'_>, kind: &str) -> Result<Option<FormulaValue>, Error> {
    if !money(ctx, kind).is_zero() {
        return Ok(None);
    }

    let other_kind = if kind == "debit" { "credit" } else { "debit" };
    if !money(ctx, other_kind).is_zero() {
        return Ok(Some(DocValue::Money(ctx.fyo.pesa().zero()).into()));
    }

    // `this.parentdoc!` throws for a row without a parent, and formula errors are
    // ignored.
    let Some(parent) = ctx.parent else {
        return Ok(None);
    };
    let total = parent.get_sum(ctx.fyo, "accounts", kind)?;
    let total_other = parent.get_sum(ctx.fyo, "accounts", other_kind)?;
    if total.lt(&total_other)? {
        return Ok(Some(DocValue::Money(total_other.sub(&total)?).into()));
    }
    Ok(None)
}
