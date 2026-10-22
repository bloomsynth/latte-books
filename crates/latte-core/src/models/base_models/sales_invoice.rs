// Port of models/baseModels/SalesInvoice/SalesInvoice.ts @ v0.36.0. The rest of the
// model is Invoice's (invoice.rs).
//
// Deviation: redeeming loyalty points isn't ported, so it's refused where Books would
// post the points or check how many can be redeemed.
//
// Not ported yet, as the UI needs them: `getListViewSettings` and `getActions`.

use latte_db::DocValue;

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::helpers::is_truthy;
use crate::fyo::model::types::DocContext;
use crate::models::base_models::invoice::{self, Invoice};
use crate::models::helpers::{money_or_zero, single_text, text};
use crate::models::transactional::ledger_posting::LedgerPosting;

pub const MODEL: Invoice = Invoice::new(get_posting, validate_field);

fn get_posting(doc: &Doc, fyo: &mut Fyo) -> Result<LedgerPosting, Error> {
    let exchange_rate = invoice::exchange_rate(doc);
    let is_return = invoice::is_return(doc);
    let mut posting = LedgerPosting::new(doc);
    let account = text(doc, "account").unwrap_or_default();
    let base_grand_total = money_or_zero(doc, "baseGrandTotal", fyo);
    if is_return {
        posting.credit(fyo, account, &base_grand_total)?;
    } else {
        posting.debit(fyo, account, &base_grand_total)?;
    }

    for item in doc.table("items") {
        let account = text(item, "account").unwrap_or_default();
        let amount = money_or_zero(item, "amount", fyo).mul(exchange_rate)?;
        if is_return {
            posting.debit(fyo, account, &amount)?;
            continue;
        }
        posting.credit(fyo, account, &amount)?;
    }

    if is_truthy(doc.get("redeemLoyaltyPoints")) {
        return Err(Error::Validation(
            "Redeeming loyalty points isn't supported in Latte yet.".into(),
        ));
    }

    for tax in doc.table("taxes") {
        let account = text(tax, "account").unwrap_or_default();
        let amount = money_or_zero(tax, "amount", fyo).mul(exchange_rate)?;
        if is_return {
            posting.debit(fyo, account, &amount)?;
            continue;
        }
        posting.credit(fyo, account, &amount)?;
    }

    let discount_amount = invoice::get_total_discount(doc, fyo)?;
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
fn validate_field(fieldname: &str, _value: &DocValue, ctx: &DocContext<'_>) -> Result<(), Error> {
    let doc = ctx.doc;
    if fieldname != "loyaltyPoints"
        || !is_truthy(doc.get("redeemLoyaltyPoints"))
        || doc.is_submitted()
        || invoice::is_return(doc)
    {
        return Ok(());
    }
    Err(Error::Validation(
        "Redeeming loyalty points isn't supported in Latte yet.".into(),
    ))
}
