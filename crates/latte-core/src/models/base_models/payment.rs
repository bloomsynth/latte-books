// Port of models/baseModels/Payment/Payment.ts @ v0.36.0.
//
// Deviations:
// - When the amount is more than what is outstanding, Books puts back the amount the form
//   opened with (`initialAmount`) before refusing it. Here the amount is left as it was, as
//   `set` refuses a value before assigning it.
//
// Books bugs Latte fixes:
// - The amount can't be more than what is outstanding on all the invoices the payment is
//   for. Books compares it with the outstanding amount of the last one only, which it
//   keeps from the first time it checks, so a payment for two invoices can't be saved.
// - That check is made while the payment is a draft. Books makes it on every save,
//   cancelling too, and once the payment is made what is outstanding is less: a payment
//   loaded since then can't be cancelled, nor its invoice.
// - With partial payments off, a payment for less than what is outstanding is refused on
//   every save until it is cancelled. Books refuses it only once it has kept the total
//   from that same check, so a new payment whose amount wasn't typed in can be saved for
//   less, and is refused only when submitted.
//
// - Write-offs post as write-offs. The amount is what the payment settles, and comes off
//   the invoices; the amount paid, the amount less the write-off, is what the cash or bank
//   account takes, and the write-off account takes the rest: a loss when receiving, a gain
//   when paying. Books posts the whole amount to the cash or bank account and the
//   write-off against the party's account, the wrong way round, so the cash is overstated,
//   the party still owes the write-off in the ledger, and a loss is posted as a gain.
// - The amount alone must cover what is allocated to the invoices. Books lets the
//   write-off make up the difference, which takes more off the invoices than the payment
//   credits the party with.
// - With partial payments off, a payment with a write-off must still settle all that is
//   outstanding. Books lets any payment with a write-off through.
// - A write-off can't be less than zero or more than the amount. Books doesn't check.
// - Changing the amount of a refund for a return takes it off the return's outstanding
//   amount, which is negative. Books adds it instead, so the return and the party are
//   left owing twice the refund.
//
// Not ported yet, as the UI needs them: `filters`, `getActions` and
// `getListViewSettings`.

use indexmap::IndexMap;
use jiff::Timestamp;
use latte_db::pesa::Money;
use latte_db::{DocValue, DocValueMap, Filter, FilterValue, GetAllOptions, QueryFilter};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::helpers::is_truthy;
use crate::fyo::model::types::{DocContext, Formula, FormulaValue, Model};
use crate::models::base_models::invoice::{TaxItem, get_tax_items};
use crate::models::base_models::party;
use crate::models::helpers::{
    get_number_series, linked_value, money, money_or_zero, one_of, query, single_is_truthy,
    single_text, text,
};
use crate::models::transactional::ledger_posting::LedgerPosting;
use crate::models::transactional::{self, Transactional};

pub struct Payment;

type FormulaResult = Result<Option<FormulaValue>, Error>;

fn some(value: DocValue) -> FormulaResult {
    Ok(Some(value.into()))
}

fn text_value(value: Option<&str>) -> FormulaResult {
    some(value.map_or(DocValue::Null, |v| DocValue::Text(v.to_owned())))
}

/// A row's invoice type and name, if both are set.
fn reference(row: &Doc) -> Option<(&str, &str)> {
    let reference_type = text(row, "referenceType").filter(|t| !t.is_empty())?;
    let reference_name = text(row, "referenceName").filter(|n| !n.is_empty())?;
    Some((reference_type, reference_name))
}

/// The stored values of a row's invoice, as `fyo.doc.getDoc` loads it.
fn reference_values(fyo: &Fyo, row: &Doc) -> Result<DocValueMap, Error> {
    let reference_type = text(row, "referenceType").unwrap_or_default();
    let reference_name = text(row, "referenceName").unwrap_or_default();
    match fyo.db.get_doc(reference_type, reference_name) {
        Ok(values) => Ok(values),
        Err(latte_db::Error::NotFound(_)) => Err(Error::NotFound(format!(
            "Not Found: {reference_type} {reference_name}"
        ))),
        Err(e) => Err(e.into()),
    }
}

/// What is outstanding on the invoices the payment is for, each counted as positive.
fn references_outstanding(doc: &Doc, fyo: &Fyo) -> Result<Money, Error> {
    let mut outstanding_amount = fyo.pesa().zero();
    for row in doc.table("for") {
        let values = reference_values(fyo, row)?;
        if let Some(DocValue::Money(outstanding)) = values.get("outstandingAmount") {
            outstanding_amount = outstanding_amount.add(&outstanding.abs()?)?;
        }
    }
    Ok(outstanding_amount)
}

/// `_getAccountsMap`: the bank, cash, payable and receivable accounts, as
/// `(name, accountType)`.
///
/// Deviation: Books lists group accounts too, newest first. A group is made just before
/// its accounts, so on a fast computer it can share their `created` time and come first,
/// and a new payment then defaults to Cash In Hand or Bank Accounts, a group its account
/// fields don't offer. Here groups are left out, as those fields' filters leave them out.
fn get_accounts(fyo: &Fyo) -> Result<Vec<(String, String)>, Error> {
    let types = ["Bank", "Cash", "Payable", "Receivable"];
    let options = GetAllOptions {
        fields: Some(vec!["name".to_owned(), "accountType".to_owned()]),
        filters: QueryFilter::from([
            (
                "accountType".to_owned(),
                Filter::list("in", types.into_iter().map(FilterValue::from).collect()),
            ),
            ("isGroup".to_owned(), Filter::from(false)),
        ]),
        ..GetAllOptions::default()
    };
    Ok(fyo
        .db
        .get_all_docs("Account", &options)?
        .into_iter()
        .filter_map(|row| match (row.get("name"), row.get("accountType")) {
            (Some(DocValue::Text(name)), Some(DocValue::Text(account_type))) => {
                Some((name.clone(), account_type.clone()))
            }
            _ => None,
        })
        .collect())
}

/// `accountsMap[accountType]?.[0]`.
fn first_account(fyo: &Fyo, account_type: &str) -> Result<Option<String>, Error> {
    Ok(get_accounts(fyo)?
        .into_iter()
        .find(|(_, t)| t == account_type)
        .map(|(name, _)| name))
}

/// `paymentMethodDoc().type`, or an error where Books reads it from a null document.
fn payment_method_type(doc: &Doc, fyo: &Fyo) -> Result<Option<String>, Error> {
    let method = text(doc, "paymentMethod").filter(|m| !m.is_empty());
    if method.is_none() || !fyo.db.exists("PaymentMethod", method)? {
        return Err(Error::Value(
            "Cannot read properties of null (reading 'type')".into(),
        ));
    }
    Ok(match linked_value(fyo, "PaymentMethod", method, "type") {
        Some(DocValue::Text(method_type)) => Some(method_type),
        _ => None,
    })
}

/// `_getReferenceAccount`: the party's default account, else the account of the first
/// invoice paid.
fn get_reference_account(doc: &Doc, fyo: &Fyo) -> Result<Option<String>, Error> {
    let account = get_account_from_party(doc, fyo);
    if account.is_none() {
        return get_account_from_for(doc, fyo);
    }
    Ok(account)
}

/// `_getAccountFromParty`.
fn get_account_from_party(doc: &Doc, fyo: &Fyo) -> Option<String> {
    let party = text(doc, "party").filter(|p| !p.is_empty())?;
    let values = fyo.db.get_doc("Party", party).ok()?;
    if values.get("role") == Some(&DocValue::Text("Both".into())) {
        return None;
    }
    match values.get("defaultAccount") {
        Some(DocValue::Text(account)) if !account.is_empty() => Some(account.clone()),
        _ => None,
    }
}

/// `_getAccountFromFor`: the first invoice's account, or the first cash account for a
/// sales invoice that was returned.
fn get_account_from_for(doc: &Doc, fyo: &Fyo) -> Result<Option<String>, Error> {
    let Some(row) = doc.table("for").first() else {
        return Ok(None);
    };
    let Some((reference_type, reference_name)) = reference(row) else {
        return Ok(None);
    };
    let Ok(values) = fyo.db.get_doc(reference_type, reference_name) else {
        return Ok(None);
    };

    if reference_type == "SalesInvoice" && is_truthy(values.get("isReturned")) {
        return first_account(fyo, "Cash");
    }
    Ok(match values.get("account") {
        Some(DocValue::Text(account)) => Some(account.clone()),
        _ => None,
    })
}

/// `getTaxSummary`: the taxes of the invoices paid that have a payment account, moved
/// from the invoice's tax account to it, by payment account and then tax account.
pub fn get_tax_summary(doc: &Doc, fyo: &Fyo) -> Result<Vec<DocValueMap>, Error> {
    let mut taxes: IndexMap<String, IndexMap<String, (f64, Money)>> = IndexMap::new();
    for row in doc.table("for") {
        let reference_type = text(row, "referenceType").unwrap_or_default();
        let values = reference_values(fyo, row)?;
        let ref_doc = fyo.new_doc(reference_type, values, false)?;

        for TaxItem {
            account,
            payment_account,
            rate,
            tax_amount,
            exchange_rate,
        } in get_tax_items(&ref_doc, fyo)?
        {
            let Some(payment_account) = payment_account.filter(|a| !a.is_empty()) else {
                continue;
            };

            let entry = taxes
                .entry(payment_account)
                .or_default()
                .entry(account)
                .or_insert_with(|| (rate, fyo.pesa().zero()));
            entry.1 = entry.1.add(&tax_amount.mul(exchange_rate)?)?;
        }
    }

    let mut tax_arr = Vec::new();
    for (payment_account, accounts) in taxes {
        for (account, (rate, amount)) in accounts {
            if amount.is_zero() {
                continue;
            }
            let idx = tax_arr.len() as f64;
            tax_arr.push(DocValueMap::from([
                (
                    "account".to_owned(),
                    DocValue::Text(payment_account.clone()),
                ),
                ("from_account".to_owned(), DocValue::Text(account)),
                ("rate".to_owned(), DocValue::Number(rate)),
                ("amount".to_owned(), DocValue::Money(amount)),
                ("idx".to_owned(), DocValue::Number(idx)),
            ]));
        }
    }
    Ok(tax_arr)
}

fn formulas() -> Vec<Formula> {
    vec![
        Formula {
            fieldname: "account",
            depends_on: Some(&["paymentMethod", "paymentType", "party"]),
            formula: |ctx, _| {
                let (doc, fyo) = (ctx.doc, ctx.fyo);
                if text(doc, "paymentType") == Some("Receive") {
                    let account = match get_reference_account(doc, fyo)? {
                        Some(account) => Some(account),
                        None => first_account(fyo, "Receivable")?,
                    };
                    return text_value(account.as_deref());
                }

                let account_type = match payment_method_type(doc, fyo)?.as_deref() {
                    Some("Cash") => "Cash",
                    _ => "Bank",
                };
                text_value(first_account(fyo, account_type)?.as_deref())
            },
        },
        Formula {
            fieldname: "paymentAccount",
            depends_on: Some(&["paymentMethod", "paymentType", "party"]),
            formula: |ctx, _| {
                let (doc, fyo) = (ctx.doc, ctx.fyo);
                if text(doc, "paymentType") == Some("Pay") {
                    let account = match get_reference_account(doc, fyo)? {
                        Some(account) => Some(account),
                        None => first_account(fyo, "Payable")?,
                    };
                    return text_value(account.as_deref());
                }

                let method_type = payment_method_type(doc, fyo)?;
                let method = text(doc, "paymentMethod");
                if let Some(account @ DocValue::Text(_)) =
                    linked_value(fyo, "PaymentMethod", method, "account")
                {
                    return some(account);
                }

                let account_type = match method_type.as_deref() {
                    Some("Cash") => "Cash",
                    _ => "Bank",
                };
                text_value(first_account(fyo, account_type)?.as_deref())
            },
        },
        Formula {
            fieldname: "paymentType",
            depends_on: None,
            formula: |ctx, _| {
                let (doc, fyo) = (ctx.doc, ctx.fyo);
                let Some(party) = text(doc, "party").filter(|p| !p.is_empty()) else {
                    return Ok(None);
                };

                let ref_doc = doc
                    .table("for")
                    .first()
                    .and_then(reference)
                    .and_then(|(t, n)| Some((t, fyo.db.get_doc(t, n).ok()?)));
                let is_ref_return = ref_doc
                    .as_ref()
                    .is_some_and(|(_, values)| is_truthy(values.get("returnAgainst")));
                let is_ref_sales_return = ref_doc.as_ref().is_some_and(|(t, _)| {
                    matches!(*t, "SalesInvoice" | "SalesQuote") && is_ref_return
                });

                let Ok(party) = fyo.db.get_doc("Party", party) else {
                    return Err(Error::Value(
                        "Cannot read properties of null (reading 'outstandingAmount')".into(),
                    ));
                };
                let payment_type = match party.get("role") {
                    Some(DocValue::Text(role)) if role == "Supplier" => {
                        if is_ref_return {
                            "Receive"
                        } else {
                            "Pay"
                        }
                    }
                    Some(DocValue::Text(role)) if role == "Customer" || role == "Both" => {
                        if is_ref_sales_return {
                            "Pay"
                        } else {
                            "Receive"
                        }
                    }
                    _ => match party.get("outstandingAmount") {
                        Some(DocValue::Money(outstanding)) if !outstanding.is_zero() => {
                            if outstanding.is_positive() {
                                "Receive"
                            } else {
                                "Pay"
                            }
                        }
                        _ => return Ok(doc.get("paymentType").cloned().map(Into::into)),
                    },
                };
                some(DocValue::Text(payment_type.to_owned()))
            },
        },
        Formula {
            fieldname: "amount",
            depends_on: Some(&["for"]),
            formula: |ctx, _| some(DocValue::Money(ctx.doc.get_sum(ctx.fyo, "for", "amount")?)),
        },
        Formula {
            fieldname: "amountPaid",
            depends_on: Some(&["amount", "writeoff", "for"]),
            formula: |ctx, _| {
                let doc = ctx.doc;
                match (money(doc, "amount"), money(doc, "writeoff")) {
                    (Some(amount), Some(writeoff)) => some(DocValue::Money(amount.sub(&writeoff)?)),
                    _ => Err(Error::Value("amount or writeoff is not money".into())),
                }
            },
        },
        Formula {
            fieldname: "referenceType",
            depends_on: Some(&["for"]),
            formula: |ctx, _| {
                Ok(match ctx.doc.get("referenceType") {
                    Some(DocValue::Text(t)) if !t.is_empty() => {
                        Some(DocValue::Text(t.clone()).into())
                    }
                    _ => None,
                })
            },
        },
        Formula {
            fieldname: "taxes",
            depends_on: None,
            formula: |ctx, _| Ok(Some(get_tax_summary(ctx.doc, ctx.fyo)?.into())),
        },
    ]
}

/// `updateAmountOnReferenceUpdate`.
fn update_amount_on_reference_update(doc: &mut Doc, fyo: &Fyo) -> Result<(), Error> {
    let amount = doc.get_sum(fyo, "for", "amount")?;
    doc.set_value("amount", DocValue::Money(amount));
    Ok(())
}

/// `updateDetailsOnReferenceUpdate`: the party and payment type of the one invoice paid,
/// if they aren't set yet.
fn update_details_on_reference_update(doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
    let rows = doc.table("for");
    let Some((reference_type, reference_name)) = rows.first().and_then(reference) else {
        return Ok(());
    };
    if rows.len() != 1 || is_truthy(doc.get("party")) || is_truthy(doc.get("paymentType")) {
        return Ok(());
    }

    let (reference_type, reference_name) = (reference_type.to_owned(), reference_name.to_owned());
    let invoice = fyo.get_doc(&reference_type, Some(&reference_name))?;
    let payment_type = if invoice.schema_name() == "SalesInvoice" {
        "Receive"
    } else {
        "Pay"
    };
    let party = invoice.get("party").cloned().unwrap_or(DocValue::Null);
    doc.set_value("party", party);
    doc.set_value("paymentType", DocValue::Text(payment_type.to_owned()));
    Ok(())
}

/// `updateReferenceOnAmountUpdate`: the one invoice paid gets the payment's amount, taken
/// off a return's (negative) outstanding amount as a negative amount. Books gives a return
/// the positive amount, which takes its refund off twice over.
fn update_reference_on_amount_update(doc: &mut Doc, fyo: &Fyo) -> Result<(), Error> {
    if doc.table("for").len() != 1 {
        return Ok(());
    }
    let mut amount = doc.get("amount").cloned().unwrap_or(DocValue::Null);
    let mut rows = doc.table("for").to_vec();
    if let (DocValue::Money(money), Some((reference_type, reference_name))) =
        (&amount, reference(&rows[0]))
        && is_truthy(
            fyo.get_value(reference_type, Some(reference_name), "returnAgainst")
                .as_ref(),
        )
    {
        amount = DocValue::Money(money.abs()?.neg()?);
    }
    rows[0].set_value("amount", amount);
    doc.set_table_rows("for", rows);
    Ok(())
}

/// `validateFor`: the invoices paid are the party's.
fn validate_for(doc: &Doc, fyo: &Fyo) -> Result<(), Error> {
    let party = text(doc, "party").unwrap_or_default();
    for row in doc.table("for") {
        let values = reference_values(fyo, row)?;
        let ref_party = match values.get("party") {
            Some(DocValue::Text(p)) => p.as_str(),
            _ => "",
        };
        if ref_party != party {
            return Err(Error::Validation(format!(
                "{} party {ref_party} is different from {party}",
                text(row, "referenceName").unwrap_or_default()
            )));
        }
    }
    Ok(())
}

/// `validateAccounts`.
fn validate_accounts(doc: &Doc) -> Result<(), Error> {
    let account = text(doc, "account").filter(|a| !a.is_empty());
    if account.is_none() || text(doc, "paymentAccount") != account {
        return Ok(());
    }
    Err(Error::Validation(format!(
        "To Account and From Account can't be the same: {}",
        account.unwrap_or_default()
    )))
}

/// `validateTotalReferenceAmount`: the amount covers what is allocated to the invoices. The
/// write-off is part of the amount, so unlike Books it doesn't count again.
fn validate_total_reference_amount(doc: &Doc, fyo: &Fyo) -> Result<(), Error> {
    if doc.table("for").is_empty() {
        return Ok(());
    }

    let reference_amount_total = doc.get_sum(fyo, "for", "amount")?;
    let amount = money_or_zero(doc, "amount", fyo);
    if amount.gte(&reference_amount_total)? {
        return Ok(());
    }

    let payment = crate::fyo::format::currency(fyo, &amount);
    let ref_amount = crate::fyo::format::currency(fyo, &reference_amount_total);
    Err(Error::Validation(format!(
        "Amount: {payment} is less than the total amount allocated to references: \
         {ref_amount}."
    )))
}

/// `validateReferences`: the rows are for invoices, and the amount is more than zero and
/// no more than what is outstanding on them.
fn validate_references(doc: &Doc, fyo: &Fyo) -> Result<(), Error> {
    if doc.table("for").is_empty() {
        return Ok(());
    }

    for row in doc.table("for") {
        if !matches!(
            text(row, "referenceType"),
            Some("SalesInvoice" | "PurchaseInvoice")
        ) {
            return Err(Error::Validation(
                "Please select a valid reference type.".into(),
            ));
        }
    }

    // `validateReferenceOutstanding`.
    let outstanding_amount = references_outstanding(doc, fyo)?;
    let amount = money_or_zero(doc, "amount", fyo);
    if amount.is_positive() && amount.lte(&outstanding_amount)? {
        return Ok(());
    }

    if !amount.is_positive() {
        return Err(Error::Validation(format!(
            "Payment amount: {} should be greater than 0.",
            crate::fyo::format::currency(fyo, &amount)
        )));
    }
    Err(Error::Validation(format!(
        "Payment amount: {} should be less than Outstanding amount: {}.",
        crate::fyo::format::currency(fyo, &amount),
        crate::fyo::format::currency(fyo, &outstanding_amount)
    )))
}

/// The write-off is no less than zero and no more than the amount.
fn validate_write_off(doc: &Doc, fyo: &Fyo) -> Result<(), Error> {
    let writeoff = money_or_zero(doc, "writeoff", fyo);
    if writeoff.is_negative() {
        return Err(Error::Validation(
            "Write Off cannot be less than zero.".into(),
        ));
    }
    let amount = money_or_zero(doc, "amount", fyo);
    if writeoff.gt(&amount)? {
        return Err(Error::Validation(format!(
            "Write Off cannot exceed the payment amount {}.",
            crate::fyo::format::currency(fyo, &amount)
        )));
    }
    Ok(())
}

/// `validateWriteOffAccount`: a payment with a write-off needs the write-off account,
/// which it returns.
fn validate_write_off_account(fyo: &Fyo) -> Result<String, Error> {
    let account = match single_text(fyo, "AccountingSettings", "writeOffAccount") {
        Some(account) if !account.is_empty() => account,
        _ => {
            return Err(Error::NotFound(
                "Write Off Account not set. Please set Write Off Account in General Settings"
                    .into(),
            ));
        }
    };
    if fyo.db.exists("Account", Some(&account))? {
        return Ok(account);
    }
    Err(Error::NotFound(format!(
        "Write Off Account {account} does not exist. Please set Write Off Account in General \
         Settings"
    )))
}

/// `validateReferencesAreSet`: a bank payment needs its clearance date and reference.
fn validate_references_are_set(doc: &Doc, fyo: &Fyo) -> Result<(), Error> {
    if payment_method_type(doc, fyo)?.as_deref() != Some("Bank") {
        return Ok(());
    }
    if !is_truthy(doc.get("clearanceDate")) {
        return Err(Error::Validation("Clearance Date not set.".into()));
    }
    if !is_truthy(doc.get("referenceId")) {
        return Err(Error::Validation("Reference Id not set.".into()));
    }
    Ok(())
}

/// `updateReferenceDocOutstanding`, and `_revertReferenceOutstanding` with `revert`: takes
/// the rows' amounts off what is outstanding on their invoices, or puts them back.
fn update_reference_outstanding(doc: &Doc, fyo: &mut Fyo, revert: bool) -> Result<(), Error> {
    for row in doc.table("for") {
        let reference_type = text(row, "referenceType").unwrap_or_default();
        let mut ref_doc = fyo.get_doc(reference_type, text(row, "referenceName"))?;
        let previous = money_or_zero(&ref_doc, "outstandingAmount", fyo);
        let amount = money_or_zero(row, "amount", fyo);
        let outstanding_amount = if revert {
            previous.add(&amount)?
        } else {
            previous.sub(&amount)?
        };
        ref_doc.set_and_sync(
            fyo,
            "outstandingAmount",
            DocValue::Money(outstanding_amount),
        )?;
    }
    Ok(())
}

/// `updatePartyOutstanding`.
fn update_party_outstanding(doc: &Doc, fyo: &mut Fyo) -> Result<(), Error> {
    let mut party = fyo.get_doc("Party", text(doc, "party"))?;
    party::update_outstanding_amount(&mut party, fyo)
}

impl Transactional for Payment {
    /// `getPosting`. `account` is the From Account and `paymentAccount` the To Account:
    /// Debtors and Cash when receiving, Cash and Creditors when paying. The party's
    /// account takes the whole amount and the cash or bank account the amount paid; the
    /// write-off account takes the difference (`applyWriteOffPosting`, fixed).
    fn get_posting(&self, doc: &Doc, fyo: &mut Fyo) -> Result<Option<LedgerPosting>, Error> {
        let writeoff = money_or_zero(doc, "writeoff", fyo);
        let write_off_account = if writeoff.is_zero() {
            None
        } else {
            Some(validate_write_off_account(fyo)?)
        };
        let mut posting = LedgerPosting::new(doc);

        let payment_account = text(doc, "paymentAccount").unwrap_or_default().to_owned();
        let account = text(doc, "account").unwrap_or_default().to_owned();
        let amount = money_or_zero(doc, "amount", fyo);
        let amount_paid = amount.sub(&writeoff)?;
        let is_pay = text(doc, "paymentType") == Some("Pay");
        if is_pay {
            posting.debit(fyo, &payment_account, &amount)?;
            posting.credit(fyo, &account, &amount_paid)?;
        } else {
            posting.debit(fyo, &payment_account, &amount_paid)?;
            posting.credit(fyo, &account, &amount)?;
        }
        if let Some(write_off_account) = write_off_account {
            if is_pay {
                posting.credit(fyo, &write_off_account, &writeoff)?;
            } else {
                posting.debit(fyo, &write_off_account, &writeoff)?;
            }
        }

        let payment_type = text(doc, "paymentType");
        for tax in doc.table("taxes") {
            let from_account = text(tax, "from_account").unwrap_or_default();
            let tax_account = text(tax, "account").unwrap_or_default();
            let tax_amount = money_or_zero(tax, "amount", fyo);
            match payment_type {
                Some("Receive") => {
                    posting.debit(fyo, from_account, &tax_amount)?;
                    posting.credit(fyo, tax_account, &tax_amount)?;
                }
                Some("Pay") => {
                    posting.credit(fyo, from_account, &tax_amount)?;
                    posting.debit(fyo, tax_account, &tax_amount)?;
                }
                _ => {}
            }
        }

        Ok(Some(posting))
    }
}

impl Model for Payment {
    /// Deviation: Books compares `paymentMethod.name` with "Cash", but the field holds the
    /// method's name, which has no `name`, so a Cash payment was offered bank accounts too.
    /// Here the method named Cash offers only cash accounts.
    fn link_filters(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<QueryFilter> {
        let payment_type = text(ctx.doc, "paymentType");
        let cash = text(ctx.doc, "paymentMethod") == Some("Cash");
        let money_account = || {
            if cash {
                query([
                    ("accountType", Filter::from("Cash")),
                    ("isGroup", Filter::from(false)),
                ])
            } else {
                query([
                    ("accountType", one_of(&["Bank", "Cash"])),
                    ("isGroup", Filter::from(false)),
                ])
            }
        };
        Some(match fieldname {
            "party" => match payment_type {
                Some("Pay") => query([("role", one_of(&["Supplier", "Both"]))]),
                Some("Receive") => query([("role", one_of(&["Customer", "Both"]))]),
                _ => QueryFilter::new(),
            },
            "numberSeries" => query([("referenceType", Filter::from("Payment"))]),
            "account" if payment_type == Some("Receive") => query([
                ("accountType", Filter::from("Receivable")),
                ("isGroup", Filter::from(false)),
            ]),
            "account" => money_account(),
            "paymentAccount" if payment_type == Some("Pay") => query([
                ("accountType", Filter::from("Payable")),
                ("isGroup", Filter::from(false)),
            ]),
            "paymentAccount" => money_account(),
            _ => return None,
        })
    }

    fn default(&self, fieldname: &str, doc: &Doc, fyo: &Fyo) -> Option<DocValue> {
        match fieldname {
            "numberSeries" => get_number_series(doc.schema_name(), fyo).map(DocValue::Text),
            "date" => Some(DocValue::Date(Timestamp::now())),
            _ => None,
        }
    }

    fn formulas(&self) -> Vec<Formula> {
        formulas()
    }

    /// `validations.amount`.
    fn validate_field(
        &self,
        fieldname: &str,
        value: &DocValue,
        ctx: &DocContext<'_>,
    ) -> Result<(), Error> {
        let (doc, fyo) = (ctx.doc, ctx.fyo);
        if fieldname != "amount" {
            return Ok(());
        }
        let DocValue::Money(value) = value else {
            return Ok(());
        };

        if value.is_negative() {
            return Err(Error::Validation(
                "Payment amount cannot be less than zero.".into(),
            ));
        }
        if doc.table("for").is_empty() || is_truthy(doc.get("submitted")) {
            return Ok(());
        }

        let total_amount = references_outstanding(doc, fyo)?;
        if value.gt(&total_amount)? {
            return Err(Error::Validation(format!(
                "Payment amount cannot exceed {}.",
                crate::fyo::format::currency(fyo, &total_amount)
            )));
        }
        if value.is_zero() {
            return Err(Error::Validation(format!(
                "Payment amount cannot be {}.",
                crate::fyo::format::currency(fyo, value)
            )));
        }
        Ok(())
    }

    fn hidden(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<bool> {
        let doc = ctx.doc;
        let is_done = doc.is_submitted() || doc.is_cancelled();
        Some(match fieldname {
            "amountPaid" => money(doc, "writeoff").is_none_or(|w| w.is_zero()),
            "attachment" => !(is_truthy(doc.get("attachment")) || !is_done),
            "for" => is_done && doc.table("for").is_empty(),
            "taxes" => doc.table("taxes").is_empty(),
            _ => return None,
        })
    }

    fn change(&self, doc: &mut Doc, fyo: &mut Fyo, changed: Option<&str>) -> Result<(), Error> {
        match changed {
            Some("for") => {
                update_amount_on_reference_update(doc, fyo)?;
                update_details_on_reference_update(doc, fyo)
            }
            Some("amount") => update_reference_on_amount_update(doc, fyo),
            _ => Ok(()),
        }
    }

    fn validate(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        transactional::validate(self, doc, fyo)?;
        if is_truthy(doc.get("submitted")) {
            return Ok(());
        }

        validate_for(doc, fyo)?;
        validate_accounts(doc)?;
        validate_write_off(doc, fyo)?;
        validate_total_reference_amount(doc, fyo)?;
        validate_references(doc, fyo)?;
        validate_references_are_set(doc, fyo)
    }

    /// `beforeSync`: with partial payments off, a payment must settle all that is
    /// outstanding, write-off or not.
    fn before_sync(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        if is_truthy(doc.get("cancelled"))
            || doc.table("for").is_empty()
            || single_is_truthy(fyo, "AccountingSettings", "enablePartialPayment")
        {
            return Ok(());
        }

        let total_amount = references_outstanding(doc, fyo)?;
        if money_or_zero(doc, "amount", fyo).lt(&total_amount)? {
            doc.set_value("amount", DocValue::Money(total_amount));
            return Err(Error::Validation(
                "Enable Partial payment to pay partial amount".into(),
            ));
        }
        Ok(())
    }

    fn after_submit(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        transactional::after_submit(self, doc, fyo)?;
        update_reference_outstanding(doc, fyo, false)?;
        update_party_outstanding(doc, fyo)
    }

    fn after_cancel(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        transactional::after_cancel(self, doc, fyo)?;
        update_reference_outstanding(doc, fyo, true)?;
        update_party_outstanding(doc, fyo)
    }

    fn after_delete(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        transactional::after_delete(self, doc, fyo)
    }
}
