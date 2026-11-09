// Latte's own: the return periods of the GST portal. A GSTR is for a month or, for a
// company in the QRMP scheme (Quarterly Return, Monthly Payment), a quarter, of a financial
// year that runs from April to March. Books has only From Date and To Date.

use jiff::ToSpan;
use jiff::civil::{Date, date};

use crate::fyo::Fyo;
use crate::models::helpers::single_text;

/// How often the company files GSTR-1: AccountingSettings' Return Filing, as chosen on the
/// GST portal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filing {
    #[default]
    Monthly,
    /// The QRMP scheme.
    Quarterly,
}

impl Filing {
    pub fn of(fyo: &Fyo) -> Self {
        match single_text(fyo, "AccountingSettings", "gstReturnFiling").as_deref() {
            Some("Quarterly") => Self::Quarterly,
            _ => Self::Monthly,
        }
    }
}

/// A return period of a financial year.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    /// A month, 1 for January.
    Month(i8),
    /// A quarter, 1 for April to June.
    Quarter(i8),
    /// From Date to To Date, as typed.
    Custom,
}

/// The value of Custom in the Period filter.
pub const CUSTOM: &str = "custom";

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// The first financial year of GST, which began on 1 July 2017.
pub const FIRST_FISCAL_YEAR: i16 = 2017;

/// The financial year a date is in, by the year it begins in.
pub fn fiscal_year_of(day: Date) -> i16 {
    if day.month() >= 4 {
        day.year()
    } else {
        day.year() - 1
    }
}

/// A financial year as the portal names it: 2026-27.
pub fn fiscal_year_label(year: i16) -> String {
    format!("{year}-{:02}", (year + 1) % 100)
}

/// A financial year from its name (2026-27) or the year it begins in (2026).
pub fn parse_fiscal_year(text: &str) -> Option<i16> {
    let year: i16 = text.trim().get(..4)?.parse().ok()?;
    (1900..=9998).contains(&year).then_some(year)
}

/// The quarter of the financial year a month is in: 1 for April to June.
fn quarter_of(month: i8) -> i8 {
    (month + 8) % 12 / 3 + 1
}

/// The first month of a quarter: 4 for the first.
fn first_month(quarter: i8) -> i8 {
    (quarter * 3) % 12 + 1
}

/// The year a month of a financial year falls in.
fn year_of(fiscal_year: i16, month: i8) -> i16 {
    if month >= 4 {
        fiscal_year
    } else {
        fiscal_year + 1
    }
}

fn last_day(year: i16, month: i8) -> Date {
    date(year, month, 1).last_of_month()
}

impl Period {
    /// The Period filter's value: the month as two digits (04), Q1 to Q4, or custom.
    pub fn value(self) -> String {
        match self {
            Self::Month(month) => format!("{month:02}"),
            Self::Quarter(quarter) => format!("Q{quarter}"),
            Self::Custom => CUSTOM.to_owned(),
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.eq_ignore_ascii_case(CUSTOM) {
            return Some(Self::Custom);
        }
        if let Some(quarter) = text.strip_prefix(['Q', 'q']) {
            let quarter: i8 = quarter.parse().ok()?;
            return (1..=4).contains(&quarter).then_some(Self::Quarter(quarter));
        }
        let month: i8 = text.parse().ok()?;
        (1..=12).contains(&month).then_some(Self::Month(month))
    }

    /// The period as the filter shows it: October 2026, or Jul–Sep 2026.
    pub fn label(self, fiscal_year: i16) -> String {
        match self {
            Self::Month(month) => format!(
                "{} {}",
                MONTH_NAMES[month as usize - 1],
                year_of(fiscal_year, month)
            ),
            Self::Quarter(quarter) => {
                let first = first_month(quarter);
                format!(
                    "{}–{} {}",
                    MONTHS[first as usize - 1],
                    MONTHS[first as usize + 1],
                    year_of(fiscal_year, first)
                )
            }
            Self::Custom => "Custom".to_owned(),
        }
    }

    /// The first and last days of the period in a financial year; none for Custom.
    pub fn dates(self, fiscal_year: i16) -> Option<(Date, Date)> {
        let (first, last) = match self {
            Self::Month(month) => (month, month),
            Self::Quarter(quarter) => {
                let first = first_month(quarter);
                (first, first + 2)
            }
            Self::Custom => return None,
        };
        Some((
            date(year_of(fiscal_year, first), first, 1),
            last_day(year_of(fiscal_year, last), last),
        ))
    }

    /// The periods a company files for, April first.
    pub fn all(filing: Filing) -> Vec<Self> {
        match filing {
            Filing::Monthly => [4, 5, 6, 7, 8, 9, 10, 11, 12, 1, 2, 3]
                .map(Self::Month)
                .to_vec(),
            Filing::Quarterly => (1..=4).map(Self::Quarter).collect(),
        }
    }

    /// The last day GSTR-1 can be filed for the period without a late fee: the 11th of the
    /// next month for a month, the 13th of the month after a quarter (QRMP). None for
    /// Custom.
    pub fn gstr1_due_date(self, fiscal_year: i16) -> Option<Date> {
        let (_, last) = self.dates(fiscal_year)?;
        let next = last.checked_add(1.day()).ok()?;
        let day = if matches!(self, Self::Quarter(_)) {
            13
        } else {
            11
        };
        Some(date(next.year(), next.month(), day))
    }
}

/// The latest period that has ended by `today`, and its financial year: last month, or last
/// quarter in QRMP. It is the return due next.
pub fn last_ended(filing: Filing, today: Date) -> (i16, Period) {
    // The first day of this month, or of this quarter, which is in the same calendar year.
    let start = match filing {
        Filing::Monthly => today.first_of_month(),
        Filing::Quarterly => date(today.year(), first_month(quarter_of(today.month())), 1),
    };
    let day = start.yesterday().unwrap_or(start);
    let period = match filing {
        Filing::Monthly => Period::Month(day.month()),
        Filing::Quarterly => Period::Quarter(quarter_of(day.month())),
    };
    (fiscal_year_of(day), period)
}

/// The financial year of the latest `period` that has begun by `today`.
pub fn latest_fiscal_year(period: Period, today: Date) -> i16 {
    let year = fiscal_year_of(today);
    match period.dates(year) {
        Some((first, _)) if first > today => year - 1,
        _ => year,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_financial_year_runs_from_april_to_march() {
        assert_eq!(fiscal_year_of(date(2026, 4, 1)), 2026);
        assert_eq!(fiscal_year_of(date(2026, 3, 31)), 2025);
        assert_eq!(fiscal_year_label(2026), "2026-27");
        assert_eq!(fiscal_year_label(2099), "2099-00");
        assert_eq!(parse_fiscal_year("2026-27"), Some(2026));
        assert_eq!(parse_fiscal_year("2026"), Some(2026));
        assert_eq!(parse_fiscal_year("26"), None);
    }

    #[test]
    fn months_and_quarters_have_their_dates() {
        assert_eq!(
            Period::Month(10).dates(2026),
            Some((date(2026, 10, 1), date(2026, 10, 31)))
        );
        assert_eq!(
            Period::Month(2).dates(2027),
            Some((date(2028, 2, 1), date(2028, 2, 29)))
        );
        assert_eq!(
            Period::Quarter(1).dates(2026),
            Some((date(2026, 4, 1), date(2026, 6, 30)))
        );
        assert_eq!(
            Period::Quarter(2).dates(2026),
            Some((date(2026, 7, 1), date(2026, 9, 30)))
        );
        assert_eq!(
            Period::Quarter(4).dates(2026),
            Some((date(2027, 1, 1), date(2027, 3, 31)))
        );
        assert_eq!(Period::Custom.dates(2026), None);
    }

    #[test]
    fn periods_are_named_as_the_portal_names_them() {
        assert_eq!(Period::Month(10).label(2026), "October 2026");
        assert_eq!(Period::Month(1).label(2026), "January 2027");
        assert_eq!(Period::Quarter(2).label(2026), "Jul–Sep 2026");
        assert_eq!(Period::Quarter(4).label(2026), "Jan–Mar 2027");
        for period in Period::all(Filing::Monthly)
            .into_iter()
            .chain(Period::all(Filing::Quarterly))
            .chain([Period::Custom])
        {
            assert_eq!(Period::parse(&period.value()), Some(period));
        }
        assert_eq!(Period::parse("13"), None);
        assert_eq!(Period::parse("Q5"), None);
    }

    #[test]
    fn gstr1_is_due_on_the_11th_or_after_a_quarter_on_the_13th() {
        assert_eq!(
            Period::Month(10).gstr1_due_date(2026),
            Some(date(2026, 11, 11))
        );
        assert_eq!(
            Period::Month(3).gstr1_due_date(2026),
            Some(date(2027, 4, 11))
        );
        assert_eq!(
            Period::Month(12).gstr1_due_date(2026),
            Some(date(2027, 1, 11))
        );
        assert_eq!(
            Period::Quarter(2).gstr1_due_date(2026),
            Some(date(2026, 10, 13))
        );
        assert_eq!(
            Period::Quarter(4).gstr1_due_date(2026),
            Some(date(2027, 4, 13))
        );
        assert_eq!(Period::Custom.gstr1_due_date(2026), None);
    }

    #[test]
    fn the_default_is_the_latest_period_that_has_ended() {
        let today = date(2026, 11, 9);
        assert_eq!(
            last_ended(Filing::Monthly, today),
            (2026, Period::Month(10))
        );
        assert_eq!(
            last_ended(Filing::Quarterly, today),
            (2026, Period::Quarter(2))
        );
        // Into a new financial year.
        let today = date(2026, 4, 5);
        assert_eq!(last_ended(Filing::Monthly, today), (2025, Period::Month(3)));
        assert_eq!(
            last_ended(Filing::Quarterly, today),
            (2025, Period::Quarter(4))
        );
        // Into a new calendar year.
        let today = date(2027, 1, 1);
        assert_eq!(
            last_ended(Filing::Monthly, today),
            (2026, Period::Month(12))
        );
        assert_eq!(
            last_ended(Filing::Quarterly, today),
            (2026, Period::Quarter(3))
        );
        let today = date(2027, 2, 20);
        assert_eq!(
            last_ended(Filing::Quarterly, today),
            (2026, Period::Quarter(3))
        );
    }

    #[test]
    fn a_period_without_a_year_is_its_latest_that_has_begun() {
        let today = date(2026, 11, 9);
        assert_eq!(latest_fiscal_year(Period::Month(10), today), 2026);
        assert_eq!(latest_fiscal_year(Period::Month(12), today), 2025);
        assert_eq!(latest_fiscal_year(Period::Month(2), today), 2025);
        assert_eq!(latest_fiscal_year(Period::Quarter(3), today), 2026);
        assert_eq!(latest_fiscal_year(Period::Quarter(4), today), 2025);
    }
}
