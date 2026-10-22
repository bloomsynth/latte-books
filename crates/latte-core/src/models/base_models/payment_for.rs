// Port of models/baseModels/PaymentFor/PaymentFor.ts @ v0.36.0, a payment's row for an
// invoice it pays.
//
// Not ported yet, as the UI needs them: `filters`.

use latte_db::DocValue;

use crate::Error;
use crate::fyo::model::types::{DocContext, Formula, FormulaValue, Model};
use crate::models::helpers::{linked_value, text};

pub struct PaymentFor;

type FormulaResult = Result<Option<FormulaValue>, Error>;

fn some(value: DocValue) -> FormulaResult {
    Ok(Some(value.into()))
}

/// The row's invoice type and name, if both are set.
fn reference<'a>(ctx: &DocContext<'a>) -> Option<(&'a str, &'a str)> {
    let reference_type = text(ctx.doc, "referenceType").filter(|t| !t.is_empty())?;
    let reference_name = text(ctx.doc, "referenceName").filter(|n| !n.is_empty())?;
    Some((reference_type, reference_name))
}

impl Model for PaymentFor {
    fn formulas(&self) -> Vec<Formula> {
        vec![
            Formula {
                fieldname: "referenceType",
                depends_on: None,
                formula: |ctx, _| {
                    if text(ctx.doc, "referenceType").is_some_and(|t| !t.is_empty()) {
                        return Ok(None);
                    }

                    let party = ctx.parent.and_then(|p| text(p, "party"));
                    let role = linked_value(ctx.fyo, "Party", party, "role");
                    let reference_type = match role {
                        Some(DocValue::Text(role)) if role == "Supplier" => "PurchaseInvoice",
                        _ => "SalesInvoice",
                    };
                    some(DocValue::Text(reference_type.to_owned()))
                },
            },
            Formula {
                fieldname: "referenceName",
                depends_on: Some(&["referenceType"]),
                formula: |ctx, _| {
                    let Some((reference_type, reference_name)) = reference(ctx) else {
                        return Ok(ctx.doc.get("referenceName").cloned().map(Into::into));
                    };
                    if !ctx.fyo.db.exists(reference_type, Some(reference_name))? {
                        return some(DocValue::Null);
                    }
                    some(DocValue::Text(reference_name.to_owned()))
                },
            },
            Formula {
                fieldname: "amount",
                depends_on: Some(&["referenceName"]),
                formula: |ctx, _| {
                    let zero = || some(DocValue::Money(ctx.fyo.pesa().zero()));
                    let Some(reference_name) =
                        text(ctx.doc, "referenceName").filter(|n| !n.is_empty())
                    else {
                        return zero();
                    };

                    let reference_type = text(ctx.doc, "referenceType").unwrap_or_default();
                    match ctx.fyo.get_value(
                        reference_type,
                        Some(reference_name),
                        "outstandingAmount",
                    ) {
                        Some(DocValue::Money(outstanding)) => some(DocValue::Money(outstanding)),
                        _ => zero(),
                    }
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
        if fieldname != "referenceName" {
            return Ok(());
        }

        let name = match value {
            DocValue::Text(name) => name.as_str(),
            _ => "",
        };
        let reference_type = text(ctx.doc, "referenceType").filter(|t| !t.is_empty());
        if let Some(reference_type) = reference_type
            && ctx.fyo.db.exists(reference_type, Some(name))?
        {
            return Ok(());
        }

        let reference_type = reference_type.unwrap_or("SalesInvoice");
        let label = ctx
            .fyo
            .schema(reference_type)
            .map_or(reference_type.to_owned(), |s| s.label.clone());
        Err(Error::NotFound(format!("{label} {name} does not exist")))
    }
}
