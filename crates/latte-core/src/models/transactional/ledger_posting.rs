// Port of models/Transactional/LedgerPosting.ts @ v0.36.0.

use indexmap::IndexMap;
use jiff::Timestamp;
use jiff::tz::TimeZone;
use latte_db::pesa::Money;
use latte_db::{DocValue, DocValueMap, Filter, GetAllOptions, QueryFilter};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::models::base_models::accounting_ledger_entry;

/// The ledger entries of one transactional document, one per account and side. Nothing is
/// written until `post` or `post_reverse` is called.
pub struct LedgerPosting {
    ref_schema_name: String,
    ref_name: DocValue,
    ref_party: DocValue,
    ref_date: DocValue,
    pub entries: Vec<Doc>,
    credit_map: IndexMap<String, usize>,
    debit_map: IndexMap<String, usize>,
    pub reverted: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TransactionType {
    Credit,
    Debit,
}

impl TransactionType {
    fn fieldname(self) -> &'static str {
        match self {
            Self::Credit => "credit",
            Self::Debit => "debit",
        }
    }
}

impl LedgerPosting {
    pub fn new(ref_doc: &Doc) -> Self {
        let value = |fieldname| ref_doc.get(fieldname).cloned().unwrap_or(DocValue::Null);
        Self {
            ref_schema_name: ref_doc.schema_name().to_owned(),
            ref_name: value("name"),
            ref_party: value("party"),
            ref_date: value("date"),
            entries: Vec::new(),
            credit_map: IndexMap::new(),
            debit_map: IndexMap::new(),
            reverted: false,
        }
    }

    pub fn debit(&mut self, fyo: &mut Fyo, account: &str, amount: &Money) -> Result<(), Error> {
        self.add(fyo, account, amount, TransactionType::Debit)
    }

    pub fn credit(&mut self, fyo: &mut Fyo, account: &str, amount: &Money) -> Result<(), Error> {
        self.add(fyo, account, amount, TransactionType::Credit)
    }

    fn add(
        &mut self,
        fyo: &mut Fyo,
        account: &str,
        amount: &Money,
        kind: TransactionType,
    ) -> Result<(), Error> {
        let idx = self.get_ledger_entry(fyo, account, kind)?;
        let entry = &mut self.entries[idx];
        let fieldname = kind.fieldname();
        let value = money(entry, fieldname, fyo)?.add(amount)?;
        entry.set(fyo, fieldname, DocValue::Money(value))?;
        Ok(())
    }

    pub fn post(&mut self, fyo: &mut Fyo) -> Result<(), Error> {
        self.validate_is_equal(fyo)?;
        for entry in &mut self.entries {
            entry.sync(fyo)?;
        }
        Ok(())
    }

    pub fn post_reverse(&mut self, fyo: &mut Fyo) -> Result<(), Error> {
        self.validate_is_equal(fyo)?;
        self.sync_reverse_ledger_entries(fyo)
    }

    pub fn validate(&self, fyo: &Fyo) -> Result<(), Error> {
        self.validate_is_equal(fyo)
    }

    /// `makeRoundOffEntry`: balances the entries on the Round Off Account.
    pub fn make_round_off_entry(&mut self, fyo: &mut Fyo) -> Result<(), Error> {
        let (debit, credit) = self.get_total_debit_and_credit(fyo)?;
        let difference = debit.sub(&credit)?;
        let absolute_value = difference.abs()?;
        if absolute_value.eq(0.0)? {
            return Ok(());
        }

        let round_off_account = get_round_off_account(fyo)?;
        if difference.gt(0.0)? {
            self.credit(fyo, &round_off_account, &absolute_value)
        } else {
            self.debit(fyo, &round_off_account, &absolute_value)
        }
    }

    /// `_getLedgerEntry`: the entry for an account and side, made if there is none yet.
    fn get_ledger_entry(
        &mut self,
        fyo: &Fyo,
        account: &str,
        kind: TransactionType,
    ) -> Result<usize, Error> {
        let map = match kind {
            TransactionType::Debit => &self.debit_map,
            TransactionType::Credit => &self.credit_map,
        };
        if let Some(idx) = map.get(account) {
            return Ok(*idx);
        }

        let party = match &self.ref_party {
            DocValue::Null => DocValue::Text(String::new()),
            party => party.clone(),
        };
        let data = DocValueMap::from([
            ("account".to_owned(), DocValue::Text(account.to_owned())),
            ("party".to_owned(), party),
            (
                "date".to_owned(),
                DocValue::Date(timezone_date_time_adjuster(&self.ref_date)?),
            ),
            (
                "referenceType".to_owned(),
                DocValue::Text(self.ref_schema_name.clone()),
            ),
            ("referenceName".to_owned(), self.ref_name.clone()),
            ("reverted".to_owned(), DocValue::Bool(self.reverted)),
            ("debit".to_owned(), DocValue::Money(fyo.pesa().zero())),
            ("credit".to_owned(), DocValue::Money(fyo.pesa().zero())),
        ]);
        let entry = fyo.new_doc("AccountingLedgerEntry", data, false)?;

        self.entries.push(entry);
        let idx = self.entries.len() - 1;
        match kind {
            TransactionType::Debit => self.debit_map.insert(account.to_owned(), idx),
            TransactionType::Credit => self.credit_map.insert(account.to_owned(), idx),
        };
        Ok(idx)
    }

    fn validate_is_equal(&self, fyo: &Fyo) -> Result<(), Error> {
        let (debit, credit) = self.get_total_debit_and_credit(fyo)?;
        if debit.eq(&credit)? {
            return Ok(());
        }

        Err(Error::Validation(format!(
            "Total Debit: {} must be equal to Total Credit: {}",
            crate::fyo::format::currency(fyo, &debit),
            crate::fyo::format::currency(fyo, &credit)
        )))
    }

    fn get_total_debit_and_credit(&self, fyo: &Fyo) -> Result<(Money, Money), Error> {
        let mut debit = fyo.pesa().zero();
        let mut credit = fyo.pesa().zero();
        for entry in &self.entries {
            debit = debit.add(&money(entry, "debit", fyo)?)?;
            credit = credit.add(&money(entry, "credit", fyo)?)?;
        }
        Ok((debit, credit))
    }

    /// `_syncReverseLedgerEntries`: reverts the document's entries that aren't reverted.
    fn sync_reverse_ledger_entries(&self, fyo: &mut Fyo) -> Result<(), Error> {
        let names =
            get_ledger_entry_names(fyo, &self.ref_schema_name, &self.ref_name, Some(false))?;
        for name in names {
            let mut doc = fyo.get_doc("AccountingLedgerEntry", Some(&name))?;
            accounting_ledger_entry::revert(&mut doc, fyo)?;
        }
        Ok(())
    }
}

/// The names of a document's ledger entries, newest first.
pub(crate) fn get_ledger_entry_names(
    fyo: &Fyo,
    reference_type: &str,
    reference_name: &DocValue,
    reverted: Option<bool>,
) -> Result<Vec<String>, Error> {
    let reference_name = match reference_name {
        DocValue::Text(name) => name.clone(),
        _ => String::new(),
    };
    let mut filters = QueryFilter::from([
        ("referenceType".to_owned(), Filter::from(reference_type)),
        (
            "referenceName".to_owned(),
            Filter::from(reference_name.as_str()),
        ),
    ]);
    if let Some(reverted) = reverted {
        filters.insert("reverted".to_owned(), Filter::from(reverted));
    }

    let options = GetAllOptions {
        fields: Some(vec!["name".to_owned()]),
        filters,
        ..GetAllOptions::default()
    };
    Ok(fyo
        .db
        .get_all_docs("AccountingLedgerEntry", &options)?
        .into_iter()
        .filter_map(|row| match row.get("name") {
            Some(DocValue::Text(name)) => Some(name.clone()),
            _ => None,
        })
        .collect())
}

/// An entry's debit or credit. Entries are made with both set to money.
fn money(entry: &Doc, fieldname: &str, fyo: &Fyo) -> Result<Money, Error> {
    match entry.get(fieldname) {
        Some(DocValue::Money(money)) => Ok(money.clone()),
        _ => Ok(fyo.pesa().zero()),
    }
}

/// `timezoneDateTimeAdjuster`: midnight UTC of the day the date falls on in the computer's
/// time zone. Books gets there by setting the local time to minus the UTC offset.
fn timezone_date_time_adjuster(date: &DocValue) -> Result<Timestamp, Error> {
    let timestamp = match date {
        DocValue::Date(timestamp) => *timestamp,
        // `new Date(null)` is the epoch.
        DocValue::Null => Timestamp::UNIX_EPOCH,
        other => {
            return Err(Error::Value(format!(
                "Invalid date {}",
                crate::fyo::model::helpers::js_string(Some(other))
            )));
        }
    };

    let day = timestamp.to_zoned(TimeZone::system()).date();
    day.to_zoned(TimeZone::UTC)
        .map(|zoned| zoned.timestamp())
        .map_err(|e| Error::Value(e.to_string()))
}

/// `_getRoundOffAccount`.
fn get_round_off_account(fyo: &Fyo) -> Result<String, Error> {
    let settings = fyo.db.get_doc("AccountingSettings", "AccountingSettings")?;
    match settings.get("roundOffAccount") {
        Some(DocValue::Text(account)) if !account.is_empty() => Ok(account.clone()),
        _ => Err(Error::NotFound(
            "Please set Round Off Account in the Settings.".into(),
        )),
    }
}
