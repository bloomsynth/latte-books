// Port of models/baseModels/Party/Party.ts @ v0.36.0.
//
// Not ported yet: `updateLoyaltyPoints` (loyalty programs), and, as the UI needs them,
// `filters`, `getListViewSettings` and `getActions`. The Lead a party was made from is
// saved with the base `Doc`, as the Lead model isn't ported.

use latte_db::{DocValue, Filter, GetAllOptions, QueryFilter};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::helpers::is_truthy;
use crate::fyo::model::types::{DocContext, Formula, Model};
use crate::models::helpers::{single_value, text};

pub struct Party;

/// `updateOutstandingAmount`: what the party owes on submitted invoices, less what is owed
/// to it if it is both a customer and a supplier.
pub fn update_outstanding_amount(doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
    let role = text(doc, "role").unwrap_or_default().to_owned();
    let mut outstanding_amount = fyo.pesa().zero();
    if role == "Customer" || role == "Both" {
        let outstanding_receive = get_total_outstanding_amount(doc, fyo, "SalesInvoice")?;
        outstanding_amount = outstanding_amount.add(&outstanding_receive)?;
    }

    if role == "Supplier" {
        let outstanding_pay = get_total_outstanding_amount(doc, fyo, "PurchaseInvoice")?;
        outstanding_amount = outstanding_amount.add(&outstanding_pay)?;
    }

    if role == "Both" {
        let outstanding_pay = get_total_outstanding_amount(doc, fyo, "PurchaseInvoice")?;
        outstanding_amount = outstanding_amount.sub(&outstanding_pay)?;
    }

    doc.set(
        fyo,
        "outstandingAmount",
        DocValue::Money(outstanding_amount),
    )?;
    doc.sync(fyo)
}

/// `_getTotalOutstandingAmount`.
fn get_total_outstanding_amount(
    doc: &Doc,
    fyo: &Fyo,
    schema_name: &str,
) -> Result<latte_db::pesa::Money, Error> {
    let options = GetAllOptions {
        fields: Some(vec!["outstandingAmount".to_owned()]),
        filters: QueryFilter::from([
            ("submitted".to_owned(), Filter::from(true)),
            ("cancelled".to_owned(), Filter::from(false)),
            (
                "party".to_owned(),
                Filter::from(doc.name().unwrap_or_default()),
            ),
        ]),
        ..GetAllOptions::default()
    };
    let mut total = fyo.pesa().zero();
    for row in fyo.db.get_all_docs(schema_name, &options)? {
        if let Some(DocValue::Money(amount)) = row.get("outstandingAmount") {
            total = total.add(amount)?;
        }
    }
    Ok(total)
}

impl Model for Party {
    fn formulas(&self) -> Vec<Formula> {
        vec![
            Formula {
                fieldname: "defaultAccount",
                depends_on: Some(&["role"]),
                formula: |ctx, _| {
                    let role = text(ctx.doc, "role").unwrap_or_default();
                    if role == "Both" {
                        return Ok(Some(DocValue::Text(String::new()).into()));
                    }

                    let account_name = if role == "Supplier" {
                        "Creditors"
                    } else {
                        "Debtors"
                    };
                    let account_exists = ctx.fyo.db.core().exists("Account", Some(account_name))?;
                    let account = if account_exists { account_name } else { "" };
                    Ok(Some(DocValue::Text(account.to_owned()).into()))
                },
            },
            Formula {
                fieldname: "currency",
                depends_on: None,
                formula: |ctx, _| {
                    if is_truthy(ctx.doc.get("currency")) {
                        return Ok(None);
                    }
                    Ok(single_value(ctx.fyo, "SystemSettings", "currency").map(Into::into))
                },
            },
        ]
    }

    fn validate_field(
        &self,
        fieldname: &str,
        value: &DocValue,
        _ctx: &DocContext<'_>,
    ) -> Result<(), Error> {
        match fieldname {
            "email" => validate_email(value),
            "phone" => validate_phone_number(value),
            _ => Ok(()),
        }
    }

    fn after_delete(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        set_lead_status(doc, fyo, "Interested")
    }

    fn after_sync(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        set_lead_status(doc, fyo, "Converted")
    }
}

/// The status of the Lead the party was made from, if any.
fn set_lead_status(doc: &Doc, fyo: &mut Fyo, status: &str) -> Result<(), Error> {
    if !is_truthy(doc.get("fromLead")) {
        return Ok(());
    }

    let mut lead = fyo.get_doc("Lead", doc.name())?;
    lead.set_and_sync(fyo, "status", DocValue::Text(status.to_owned()))
}

/// `validateEmail` in fyo/model/validationFunction.ts, which tests
/// `/(.+)@(.+){2,}\.(.+){2,}/`: something, an `@`, at least two characters, a dot, and at
/// least two characters more.
fn validate_email(value: &DocValue) -> Result<(), Error> {
    let DocValue::Text(email) = value else {
        return Err(Error::Value(format!("Invalid email {value:?}")));
    };

    let chars: Vec<char> = email.chars().collect();
    let is_valid = chars.iter().enumerate().any(|(at, c)| {
        *c == '@'
            && at >= 1
            && chars
                .iter()
                .enumerate()
                .any(|(dot, c)| *c == '.' && dot >= at + 3 && dot + 2 < chars.len())
    });
    if !is_valid {
        return Err(Error::Validation(format!("Invalid email: {email}")));
    }
    Ok(())
}

/// `validatePhoneNumber`, which tests `/[+]{0,1}[\d ]+/`: a digit or a space anywhere.
fn validate_phone_number(value: &DocValue) -> Result<(), Error> {
    let DocValue::Text(phone) = value else {
        return Err(Error::Value(format!("Invalid phone {value:?}")));
    };

    if !phone.chars().any(|c| c.is_ascii_digit() || c == ' ') {
        return Err(Error::Validation(format!("Invalid phone: {phone}")));
    }
    Ok(())
}
