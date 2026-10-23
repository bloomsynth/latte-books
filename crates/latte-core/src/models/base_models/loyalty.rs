// Port of models/baseModels/LoyaltyProgram/LoyaltyProgram.ts @ v0.36.0, with the loyalty
// point helpers of models/helpers.ts (`createLoyaltyPointEntry`, `removeLoyaltyPoint`,
// `getAddedLPWithGrandTotal`, `getLoyaltyProgramTier`) and Party's `updateLoyaltyPoints`.
//
// Books bug Latte fixes: cancelling an invoice takes back the points it earned. Books'
// `removeLoyaltyPoint` deletes the invoice's entry and makes it again with the same points
// unless points were redeemed, so earned points outlive the cancel.
//
// Deviation: returns of an invoice with loyalty points are refused (invoice.rs), as Books'
// handling of them changes the invoice's entry by the return's points whichever way they
// went.

use jiff::{Timestamp, ToSpan};
use latte_db::pesa::Money;
use latte_db::{DocValue, DocValueMap, Filter, GetAllOptions, QueryFilter};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::helpers::is_truthy;
use crate::fyo::model::types::{DocContext, Model};
use crate::models::base_models::pricing_rule::{map_money, map_number, map_text};
use crate::models::helpers::{money_or_zero, number, query, text};

pub struct LoyaltyProgram;

impl Model for LoyaltyProgram {
    fn link_filters(&self, fieldname: &str, _ctx: &DocContext<'_>) -> Option<QueryFilter> {
        (fieldname == "expenseAccount").then(|| {
            query([
                ("rootType", Filter::from("Expense")),
                ("isGroup", Filter::from(false)),
            ])
        })
    }
}

/// `getLoyaltyProgramTier`: the collection rule with the highest minimum spent that
/// `grand_total` reaches.
pub(crate) fn get_loyalty_program_tier(
    program: &DocValueMap,
    grand_total: &Money,
) -> Result<Option<DocValueMap>, Error> {
    let Some(DocValue::Table(rules)) = program.get("collectionRules") else {
        return Ok(None);
    };
    let mut tier: Option<&DocValueMap> = None;
    for row in rules {
        let Some(minimum) = map_money(row, "minimumTotalSpent") else {
            continue;
        };
        if !minimum.lte(grand_total)? {
            continue;
        }
        let higher = match tier.and_then(|tier| map_money(tier, "minimumTotalSpent")) {
            Some(current) => minimum.gt(current)?,
            None => true,
        };
        if higher {
            tier = Some(row);
        }
    }
    Ok(tier.cloned())
}

/// `getAddedLPWithGrandTotal`: what `points` are worth.
pub(crate) fn get_added_lp_with_grand_total(
    fyo: &Fyo,
    program: &str,
    points: f64,
) -> Result<Money, Error> {
    let program = fyo.db.get_doc("LoyaltyProgram", program)?;
    let factor = map_number(&program, "conversionFactor");
    Ok(fyo.pesa().from_f64(points * factor)?)
}

fn date(value: Option<&DocValue>) -> Option<Timestamp> {
    match value {
        Some(DocValue::Date(date)) => Some(*date),
        _ => None,
    }
}

/// `_createLoyaltyPointEntry` and `createLoyaltyPointEntry`: the points an invoice earns,
/// or the points it redeems taken off, when its program runs on its date.
pub(crate) fn create_loyalty_point_entry(doc: &Doc, fyo: &mut Fyo) -> Result<(), Error> {
    let Some(program_name) = text(doc, "loyaltyProgram").filter(|p| !p.is_empty()) else {
        return Ok(());
    };
    let program = fyo.db.get_doc("LoyaltyProgram", program_name)?;
    let (Some(invoice_date), Some(from_date), Some(to_date)) = (
        date(doc.get("date")),
        date(program.get("fromDate")),
        date(program.get("toDate")),
    ) else {
        return Ok(());
    };
    if !(from_date <= invoice_date && to_date >= invoice_date) {
        return Ok(());
    }
    if !is_truthy(program.get("isEnabled")) {
        return Ok(());
    }

    let expiry_days = map_number(&program, "expiryDuration") as i64;
    let expiry_date = Timestamp::now()
        .checked_add((expiry_days * 24).hours())
        .map_err(|e| Error::Value(e.to_string()))?;
    let grand_total = money_or_zero(doc, "grandTotal", fyo);

    let mut tier_name = DocValue::Null;
    let points = if is_truthy(doc.get("redeemLoyaltyPoints")) {
        -number(doc, "loyaltyPoints").unwrap_or(0.0)
    } else {
        let Some(tier) = get_loyalty_program_tier(&program, &grand_total)? else {
            return Ok(());
        };
        tier_name = tier.get("tierName").cloned().unwrap_or(DocValue::Null);
        grand_total.float().round() * map_number(&tier, "collectionFactor")
    };

    let data = DocValueMap::from([
        (
            "loyaltyProgram".to_owned(),
            DocValue::Text(program_name.to_owned()),
        ),
        (
            "customer".to_owned(),
            doc.get("party").cloned().unwrap_or(DocValue::Null),
        ),
        (
            "invoice".to_owned(),
            DocValue::Text(doc.name().unwrap_or_default().to_owned()),
        ),
        ("postingDate".to_owned(), DocValue::Date(invoice_date)),
        ("purchaseAmount".to_owned(), DocValue::Money(grand_total)),
        ("expiryDate".to_owned(), DocValue::Date(expiry_date)),
        ("loyaltyProgramTier".to_owned(), tier_name),
        ("loyaltyPoints".to_owned(), DocValue::Number(points)),
    ]);
    let mut entry = fyo.get_new_doc("LoyaltyPointEntry", data)?;
    entry.sync(fyo)?;
    update_loyalty_points(fyo, text(doc, "party").unwrap_or_default())
}

/// The loyalty point entries made for an invoice.
fn entries_of(fyo: &Fyo, program: &str, invoice: &str) -> Result<Vec<DocValueMap>, Error> {
    let options = GetAllOptions {
        filters: QueryFilter::from([
            ("loyaltyProgram".to_owned(), Filter::from(program)),
            ("invoice".to_owned(), Filter::from(invoice)),
        ]),
        ..GetAllOptions::default()
    };
    Ok(fyo.db.get_all_docs("LoyaltyPointEntry", &options)?)
}

/// Whether an entry was made for the invoice `invoice`.
pub(crate) fn loyalty_point_entry_exists(
    fyo: &Fyo,
    program: Option<&str>,
    invoice: &str,
) -> Result<bool, Error> {
    let Some(program) = program.filter(|p| !p.is_empty()) else {
        return Ok(false);
    };
    Ok(!entries_of(fyo, program, invoice)?.is_empty())
}

/// `removeLoyaltyPoint` for a cancelled invoice: its entries go, and so do the points it
/// earned or the points it redeemed come back.
pub(crate) fn remove_loyalty_points(doc: &Doc, fyo: &mut Fyo) -> Result<(), Error> {
    let Some(program) = text(doc, "loyaltyProgram").filter(|p| !p.is_empty()) else {
        return Ok(());
    };
    let entries = entries_of(fyo, program, doc.name().unwrap_or_default())?;
    if entries.is_empty() {
        return Ok(());
    }
    for entry in entries {
        let name = map_text(&entry, "name").unwrap_or_default().to_owned();
        let mut entry = fyo.get_doc("LoyaltyPointEntry", Some(&name))?;
        entry.delete(fyo)?;
    }
    update_loyalty_points(fyo, text(doc, "party").unwrap_or_default())
}

/// Party's `updateLoyaltyPoints`: the points of the entries that expire after they were
/// made, for a customer.
pub(crate) fn update_loyalty_points(fyo: &mut Fyo, party: &str) -> Result<(), Error> {
    let mut party_doc = fyo.get_doc("Party", Some(party))?;
    let mut points = 0.0;
    if matches!(text(&party_doc, "role"), Some("Customer" | "Both")) {
        let options = GetAllOptions {
            fields: Some(
                ["name", "loyaltyPoints", "expiryDate", "postingDate"]
                    .map(str::to_owned)
                    .to_vec(),
            ),
            filters: QueryFilter::from([("customer".to_owned(), Filter::from(party))]),
            ..GetAllOptions::default()
        };
        for entry in fyo.db.get_all_docs("LoyaltyPointEntry", &options)? {
            if date(entry.get("expiryDate")) > date(entry.get("postingDate")) {
                points += map_number(&entry, "loyaltyPoints");
            }
        }
    }
    party_doc.set_and_sync(fyo, "loyaltyPoints", DocValue::Number(points))
}
