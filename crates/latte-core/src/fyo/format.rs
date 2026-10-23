// Port of fyo/utils/format.ts @ v0.36.0: how values are shown, by the company's
// settings.
//
// Deviation: a date is shown as the date it is stored for. Books shows it in the
// computer's time zone, so west of UTC a ledger entry stored for midnight UTC shows the
// day before.

use std::collections::HashMap;

use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use latte_db::pesa::Money;
use latte_db::{DocValue, GetAllOptions};
use latte_i18n::format::{format_date, format_datetime, format_decimal};

use crate::Error;
use crate::fyo::Fyo;

/// `DEFAULT_DISPLAY_PRECISION` in fyo/utils/consts.ts.
pub const DEFAULT_DISPLAY_PRECISION: usize = 2;
/// `DEFAULT_DATE_FORMAT`.
pub const DEFAULT_DATE_FORMAT: &str = "MMM d, y";
/// `DEFAULT_LOCALE`.
pub const DEFAULT_LOCALE: &str = "en-IN";
/// `DEFAULT_CURRENCY`.
pub const DEFAULT_CURRENCY: &str = "INR";

/// The settings `format` reads from `fyo.singles.SystemSettings` and
/// `fyo.currencySymbols`, read once.
#[derive(Debug, Clone)]
pub struct Formatter {
    pub locale: String,
    pub display_precision: usize,
    pub date_format: String,
    pub currency: String,
    /// `fyo.currencySymbols`: each Currency's symbol, by its name.
    pub currency_symbols: HashMap<String, String>,
}

impl Formatter {
    pub fn new(fyo: &Fyo) -> Result<Self, Error> {
        let text =
            |fieldname: &str, default: &str| match fyo.get_value("SystemSettings", None, fieldname)
            {
                Some(DocValue::Text(text)) if !text.is_empty() => text,
                _ => default.to_owned(),
            };
        let display_precision = match fyo.get_value("SystemSettings", None, "displayPrecision") {
            Some(DocValue::Number(precision)) if precision >= 0.0 => precision as usize,
            _ => DEFAULT_DISPLAY_PRECISION,
        };

        let options = GetAllOptions {
            fields: Some(vec!["name".to_owned(), "symbol".to_owned()]),
            ..GetAllOptions::default()
        };
        let mut currency_symbols = HashMap::new();
        for row in fyo.db.get_all_docs("Currency", &options)? {
            if let (Some(DocValue::Text(name)), Some(DocValue::Text(symbol))) =
                (row.get("name"), row.get("symbol"))
            {
                currency_symbols.insert(name.clone(), symbol.clone());
            }
        }

        Ok(Self {
            locale: text("locale", DEFAULT_LOCALE),
            display_precision,
            date_format: text("dateFormat", DEFAULT_DATE_FORMAT),
            currency: text("currency", DEFAULT_CURRENCY),
            currency_symbols,
        })
    }

    /// `formatNumber`: rounded to the display precision, in the locale's format.
    pub fn number(&self, money: &Money) -> String {
        let rounded = money.round(Some(self.display_precision as i64));
        format_decimal(&rounded, &self.locale, self.display_precision)
    }

    /// `format(value, 'Currency')` in the company's currency: its symbol, a space and the
    /// number.
    pub fn currency(&self, money: &Money) -> String {
        self.currency_in(money, &self.currency)
    }

    /// `formatCurrency` in `currency`.
    pub fn currency_in(&self, money: &Money, currency: &str) -> String {
        let number = self.number(money);
        match self.currency_symbols.get(currency) {
            Some(symbol) => format!("{symbol} {number}"),
            None => number,
        }
    }

    /// `format(value, 'Float')`.
    pub fn float(&self, value: f64) -> String {
        let precision = self.display_precision;
        format!("{value:.precision$}")
    }

    /// `format(value, 'Date')`.
    pub fn date(&self, date: Date) -> String {
        format_date(date, &self.date_format)
    }

    /// `format(value, 'Date')` for a stored instant: the date it is stored for (UTC).
    pub fn timestamp_date(&self, timestamp: Timestamp) -> String {
        self.date(utc_date(timestamp))
    }

    /// `format(value, 'Datetime')`: the date and time in the computer's time zone.
    pub fn datetime(&self, timestamp: Timestamp) -> String {
        let datetime = timestamp.to_zoned(TimeZone::system()).datetime();
        format_datetime(datetime, &format!("{} HH:mm:ss", self.date_format))
    }
}

/// The UTC date of an instant: the date a ledger entry's date and time is stored for.
pub fn utc_date(timestamp: Timestamp) -> Date {
    timestamp.to_zoned(TimeZone::UTC).date()
}

/// The date of a Date field's value: Date fields are read as midnight in the system time
/// zone, as Books reads them.
pub fn local_date(timestamp: Timestamp) -> Date {
    timestamp.to_zoned(TimeZone::system()).date()
}
