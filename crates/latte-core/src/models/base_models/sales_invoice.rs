// Port of models/baseModels/SalesInvoice/SalesInvoice.ts @ v0.36.0. The rest of the
// model is Invoice's (invoice.rs).
//
// Books bug Latte fixes: redeemed points are posted to the loyalty program's expense
// account against the income, as the grand total the customer is debited with is already
// less what they are worth. Books credits the customer with them as well, and the
// round-off account takes the difference.
//
// Not ported yet, as the UI needs them: `getListViewSettings` and `getActions`.

use latte_db::DocValue;

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::helpers::{is_truthy, js_number_string};
use crate::fyo::model::types::DocContext;
use crate::models::base_models::invoice::{self, Invoice};
use crate::models::base_models::loyalty::get_added_lp_with_grand_total;
use crate::models::base_models::pricing_rule::map_text;
use crate::models::helpers::{money_or_zero, number, single_text, text};
use crate::models::transactional::ledger_posting::LedgerPosting;

pub const MODEL: Invoice = Invoice::new(get_posting, validate_field);

fn get_posting(doc: &Doc, fyo: &mut Fyo) -> Result<LedgerPosting, Error> {
    let exchange_rate = invoice::exchange_rate(doc);
    let is_return = invoice::is_return(doc);
    let mut posting = LedgerPosting::new(doc);
    let account = text(doc, "account").unwrap_or_default();
    let base_grand_total =
        invoice::posting_amount(money_or_zero(doc, "baseGrandTotal", fyo), is_return)?;
    if is_return {
        posting.credit(fyo, account, &base_grand_total)?;
    } else {
        posting.debit(fyo, account, &base_grand_total)?;
    }

    for item in doc.table("items") {
        let account = text(item, "account").unwrap_or_default();
        let amount = money_or_zero(item, "amount", fyo).mul(exchange_rate)?;
        let amount = invoice::posting_amount(amount, is_return)?;
        if is_return {
            posting.debit(fyo, account, &amount)?;
            continue;
        }
        posting.credit(fyo, account, &amount)?;
    }

    if is_truthy(doc.get("redeemLoyaltyPoints")) && !is_return {
        let program_name = text(doc, "loyaltyProgram").unwrap_or_default();
        let program = fyo.db.get_doc("LoyaltyProgram", program_name)?;
        let points = number(doc, "loyaltyPoints").unwrap_or(0.0);
        let total_amount = get_added_lp_with_grand_total(fyo, program_name, points)?;
        let expense_account = map_text(&program, "expenseAccount").unwrap_or_default();
        posting.debit(fyo, expense_account, &total_amount)?;
    }

    for tax in doc.table("taxes") {
        let account = text(tax, "account").unwrap_or_default();
        let amount = money_or_zero(tax, "amount", fyo).mul(exchange_rate)?;
        let amount = invoice::posting_amount(amount, is_return)?;
        if is_return {
            posting.debit(fyo, account, &amount)?;
            continue;
        }
        posting.credit(fyo, account, &amount)?;
    }

    let discount_amount =
        invoice::posting_amount(invoice::get_total_discount(doc, fyo)?, is_return)?;
    let discount_account = single_text(fyo, "AccountingSettings", "discountAccount");
    if let Some(discount_account) = discount_account
        && discount_amount.is_positive()
    {
        let amount = discount_amount.mul(exchange_rate)?;
        if is_return {
            posting.credit(fyo, &discount_account, &amount)?;
        } else {
            posting.debit(fyo, &discount_account, &amount)?;
        }
    }

    posting.make_round_off_entry(fyo)?;
    Ok(posting)
}

/// `validations`: only `loyaltyPoints`, which is checked only when redeeming them.
fn validate_field(fieldname: &str, value: &DocValue, ctx: &DocContext<'_>) -> Result<(), Error> {
    let doc = ctx.doc;
    if fieldname != "loyaltyPoints"
        || !is_truthy(doc.get("redeemLoyaltyPoints"))
        || doc.is_submitted()
        || invoice::is_return(doc)
    {
        return Ok(());
    }

    let value = match value {
        DocValue::Number(value) => *value,
        _ => 0.0,
    };
    let party = text(doc, "party").unwrap_or_default();
    let party_points = match ctx.fyo.get_value("Party", Some(party), "loyaltyPoints") {
        Some(DocValue::Number(points)) => points,
        _ => 0.0,
    };
    if value <= 0.0 {
        return Err(Error::Validation("Points must be greather than 0".into()));
    }
    if value > party_points {
        return Err(Error::Validation(format!(
            "{party} only has {} points",
            js_number_string(party_points)
        )));
    }

    if doc
        .get("grandTotal")
        .is_none_or(|total| !is_truthy(Some(total)))
    {
        return Ok(());
    }
    let program = text(doc, "loyaltyProgram").unwrap_or_default();
    let worth = get_added_lp_with_grand_total(ctx.fyo, program, value)?;
    if invoice::base_grand_total_before_points(doc, ctx.fyo)?.lt(&worth)? {
        return Err(Error::Validation(format!(
            "no need {} points to purchase this item",
            js_number_string(value)
        )));
    }
    Ok(())
}
