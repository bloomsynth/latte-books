// Port of models/baseModels/JournalEntry/JournalEntry.ts @ v0.36.0.
//
// Not ported yet, as the UI needs them: `filters`, `getActions` and
// `getListViewSettings`.

use jiff::Timestamp;
use latte_db::DocValue;

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::helpers::is_truthy;
use crate::fyo::model::types::{DocContext, Model};
use crate::models::helpers::get_number_series;
use crate::models::transactional::ledger_posting::LedgerPosting;
use crate::models::transactional::{self, Transactional};

pub struct JournalEntry;

impl Transactional for JournalEntry {
    fn get_posting(&self, doc: &Doc, fyo: &mut Fyo) -> Result<Option<LedgerPosting>, Error> {
        let mut posting = LedgerPosting::new(doc);
        for row in doc.table("accounts") {
            let debit = money(row, "debit", fyo);
            let credit = money(row, "credit", fyo);
            let account = match row.get("account") {
                Some(DocValue::Text(account)) => account.clone(),
                _ => String::new(),
            };

            if !debit.is_zero() {
                posting.debit(fyo, &account, &debit)?;
            } else if !credit.is_zero() {
                posting.credit(fyo, &account, &credit)?;
            }
        }
        Ok(Some(posting))
    }
}

fn money(row: &Doc, fieldname: &str, fyo: &Fyo) -> latte_db::pesa::Money {
    match row.get(fieldname) {
        Some(DocValue::Money(money)) => money.clone(),
        _ => fyo.pesa().zero(),
    }
}

impl Model for JournalEntry {
    fn default(&self, fieldname: &str, doc: &Doc, fyo: &Fyo) -> Option<DocValue> {
        match fieldname {
            "numberSeries" => get_number_series(doc.schema_name(), fyo).map(DocValue::Text),
            "date" => Some(DocValue::Date(Timestamp::now())),
            _ => None,
        }
    }

    fn hidden(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<bool> {
        match fieldname {
            "referenceNumber" | "referenceDate" | "userRemark" | "attachment" => {
                let doc = ctx.doc;
                let is_done = doc.is_submitted() || doc.is_cancelled();
                Some(!(is_truthy(doc.get(fieldname)) || !is_done))
            }
            _ => None,
        }
    }

    fn validate(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        transactional::validate(self, doc, fyo)
    }

    fn after_submit(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        transactional::after_submit(self, doc, fyo)
    }

    fn after_cancel(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        transactional::after_cancel(self, doc, fyo)
    }

    fn after_delete(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        transactional::after_delete(self, doc, fyo)
    }
}
