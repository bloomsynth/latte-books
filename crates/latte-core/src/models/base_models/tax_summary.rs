// Port of models/baseModels/TaxSummary/TaxSummary.ts @ v0.36.0.

use latte_schema::FieldType;

use crate::fyo::model::types::{DocContext, Model};
use crate::models::helpers::{DEFAULT_CURRENCY, company_currency, number, text};

pub struct TaxSummary;

impl Model for TaxSummary {
    fn currency(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<String> {
        let field = ctx.doc.schema().field(fieldname)?;
        (field.fieldtype == Some(FieldType::Currency)).then(|| get_currency(ctx))
    }
}

/// `_getCurrency`, of a row of an invoice: the company's currency if the invoice's
/// exchange rate is 1, otherwise the invoice's.
pub(crate) fn get_currency(ctx: &DocContext<'_>) -> String {
    let exchange_rate = ctx
        .parent
        .and_then(|parent| number(parent, "exchangeRate"))
        .unwrap_or(1.0);
    if exchange_rate == 1.0 {
        return company_currency(ctx.fyo);
    }
    ctx.parent
        .and_then(|parent| text(parent, "currency"))
        .unwrap_or(DEFAULT_CURRENCY)
        .to_owned()
}
