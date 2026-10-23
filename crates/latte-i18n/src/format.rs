// Number and date formatting, as fyo/utils/format.ts @ v0.36.0 does it with
// `Intl.NumberFormat` and luxon.
//
// Deviations:
// - Books formats numbers with the locale's own `Intl.NumberFormat`. Latte knows the
//   separators and grouping of the common locales (see `separators`); any other locale
//   is formatted as `en`. Digits are always 0–9, where Intl uses a script's own digits
//   for a few locales (Arabic in some regions, for one).
// - Month and weekday names are English, as luxon gives them in the English locale.

use jiff::civil::{Date, DateTime};

/// How a locale groups the digits before the decimal point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Grouping {
    /// 1,234,567.
    Thousands,
    /// 12,34,567: the last three digits, then twos (India).
    Indian,
}

#[derive(Debug, Clone, Copy)]
struct Separators {
    group: &'static str,
    decimal: &'static str,
    grouping: Grouping,
    /// Intl's minimum grouping digits: 2 leaves four-digit numbers ungrouped (es, pl).
    min_grouping: usize,
}

const NARROW_NBSP: &str = "\u{202f}";
const NBSP: &str = "\u{a0}";

/// The separators Intl gives `locale` (a BCP 47 tag such as `en-IN` or `de-CH`).
fn separators(locale: &str) -> Separators {
    let locale = locale.replace('_', "-").to_lowercase();
    let mut parts = locale.split('-');
    let language = parts.next().unwrap_or("en");
    let region = parts.find(|part| part.len() == 2).unwrap_or("");

    let sep = |group, decimal| Separators {
        group,
        decimal,
        grouping: Grouping::Thousands,
        min_grouping: 1,
    };
    if region == "in"
        && matches!(
            language,
            "en" | "hi"
                | "bn"
                | "gu"
                | "kn"
                | "ml"
                | "mr"
                | "pa"
                | "ta"
                | "te"
                | "ur"
                | "or"
                | "as"
        )
    {
        return Separators {
            grouping: Grouping::Indian,
            ..sep(",", ".")
        };
    }
    if region == "ch" || region == "li" {
        return match language {
            "fr" => sep(NARROW_NBSP, ","),
            _ => sep("’", "."),
        };
    }
    match language {
        "de" | "it" | "nl" | "id" | "tr" | "da" | "el" | "ro" | "hr" | "sl" | "sr" | "vi"
        | "is" => sep(".", ","),
        "es" => Separators {
            min_grouping: 2,
            ..sep(".", ",")
        },
        "pt" if region == "pt" => sep(NBSP, ","),
        "pt" => sep(".", ","),
        "fr" => sep(NARROW_NBSP, ","),
        "pl" => Separators {
            min_grouping: 2,
            ..sep(NBSP, ",")
        },
        "nb" | "no" | "nn" | "sv" | "fi" | "cs" | "sk" | "ru" | "uk" | "hu" | "bg" | "et"
        | "lt" | "lv" => sep(NBSP, ","),
        "fa" => sep("٬", "٫"),
        _ => sep(",", "."),
    }
}

/// `Intl.NumberFormat(locale, { style: 'decimal', minimumFractionDigits: precision })`
/// of a value already rounded to `precision`, given as a plain decimal such as
/// `-1234.5`.
pub fn format_decimal(value: &str, locale: &str, precision: usize) -> String {
    let separators = separators(locale);
    let (negative, digits) = match value.strip_prefix('-') {
        Some(digits) => (true, digits),
        None => (false, value),
    };
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    let whole = whole.trim_start_matches('0');
    let whole = if whole.is_empty() { "0" } else { whole };

    let mut fraction = fraction.to_owned();
    while fraction.len() < precision {
        fraction.push('0');
    }
    let is_zero = whole == "0" && fraction.chars().all(|c| c == '0');

    let mut out = String::new();
    if negative && !is_zero {
        out.push('-');
    }
    out.push_str(&group(whole, separators));
    if !fraction.is_empty() {
        out.push_str(separators.decimal);
        out.push_str(&fraction);
    }
    out
}

/// A float as `format_decimal` takes it: rounded half away from zero to `precision`.
pub fn format_float(value: f64, locale: &str, precision: usize) -> String {
    format_decimal(&format!("{value:.precision$}"), locale, precision)
}

fn group(whole: &str, separators: Separators) -> String {
    let len = whole.len();
    if len < 4 || len < 3 + separators.min_grouping {
        return whole.to_owned();
    }
    let (head, last) = whole.split_at(len - 3);
    let size = match separators.grouping {
        Grouping::Thousands => 3,
        Grouping::Indian => 2,
    };
    let mut groups = Vec::new();
    let mut end = head.len();
    while end > 0 {
        let start = end.saturating_sub(size);
        groups.push(&head[start..end]);
        end = start;
    }
    groups.reverse();
    groups.push(last);
    groups.join(separators.group)
}

const MONTHS: [&str; 12] = [
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
const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

/// luxon's `toFormat` for a date, with the tokens Books' date formats use: `d`, `dd`,
/// `M`, `MM`, `MMM`, `MMMM` (and `L` for `M`), `y`, `yy`, `yyyy`, `EEE`, `EEEE`, and
/// text in single quotes.
pub fn format_date(date: Date, format: &str) -> String {
    format_datetime(date.to_datetime(jiff::civil::Time::midnight()), format)
}

/// `format_date`, with the time tokens `H`, `HH`, `m`, `mm`, `s` and `ss` too.
pub fn format_datetime(datetime: DateTime, format: &str) -> String {
    let chars: Vec<char> = format.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '\'' {
                out.push(chars[j]);
                j += 1;
            }
            i = j + 1;
            continue;
        }
        let mut run = 1;
        while i + run < chars.len() && chars[i + run] == c {
            run += 1;
        }
        let date = datetime.date();
        let token = match (c, run) {
            ('d', 1) => Some(date.day().to_string()),
            ('d', _) => Some(format!("{:02}", date.day())),
            ('M' | 'L', 1) => Some(date.month().to_string()),
            ('M' | 'L', 2) => Some(format!("{:02}", date.month())),
            ('M' | 'L', 3) => Some(MONTHS[month_index(date)][..3].to_owned()),
            ('M' | 'L', _) => Some(MONTHS[month_index(date)].to_owned()),
            ('y', 2) => Some(format!("{:02}", date.year().rem_euclid(100))),
            ('y', 1) => Some(date.year().to_string()),
            ('y', _) => Some(format!("{:0run$}", date.year())),
            ('E', 1..=3) => Some(WEEKDAYS[weekday_index(date)][..3].to_owned()),
            ('E', _) => Some(WEEKDAYS[weekday_index(date)].to_owned()),
            ('H', 1) => Some(datetime.hour().to_string()),
            ('H', _) => Some(format!("{:02}", datetime.hour())),
            ('m', 1) => Some(datetime.minute().to_string()),
            ('m', _) => Some(format!("{:02}", datetime.minute())),
            ('s', 1) => Some(datetime.second().to_string()),
            ('s', _) => Some(format!("{:02}", datetime.second())),
            _ => None,
        };
        match token {
            Some(token) => out.push_str(&token),
            None => (0..run).for_each(|_| out.push(c)),
        }
        i += run;
    }
    out
}

fn month_index(date: Date) -> usize {
    usize::try_from(date.month() - 1).unwrap_or(0)
}

fn weekday_index(date: Date) -> usize {
    usize::try_from(date.weekday().to_monday_zero_offset()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_take_the_locales_grouping() {
        assert_eq!(format_decimal("1234567.5", "en-IN", 2), "12,34,567.50");
        assert_eq!(format_decimal("1234567.50", "en-US", 2), "1,234,567.50");
        assert_eq!(format_decimal("-1234.00", "de-DE", 2), "-1.234,00");
        assert_eq!(format_decimal("1234.00", "es-ES", 2), "1234,00");
        assert_eq!(format_decimal("12345.00", "es-ES", 2), "12.345,00");
        assert_eq!(format_decimal("1234.00", "de-CH", 2), "1’234.00");
        assert_eq!(format_decimal("-0.00", "en-IN", 2), "0.00");
        assert_eq!(format_decimal("999", "en-IN", 0), "999");
        assert_eq!(format_float(-2.5, "en-IN", 2), "-2.50");
    }

    #[test]
    fn dates_take_books_formats() {
        let date = jiff::civil::date(2022, 3, 23);
        assert_eq!(format_date(date, "MMM d, y"), "Mar 23, 2022");
        assert_eq!(format_date(date, "d MMM, y"), "23 Mar, 2022");
        assert_eq!(format_date(date, "dd/MM/yyyy"), "23/03/2022");
        assert_eq!(format_date(date, "dd.MM.yyyy"), "23.03.2022");
        assert_eq!(format_date(date, "yyyy-MM-dd"), "2022-03-23");
        assert_eq!(
            format_date(date, "EEE, MMMM d 'of' yy"),
            "Wed, March 23 of 22"
        );
    }
}
