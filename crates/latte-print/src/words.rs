//! `getGrandTotalInWords` in `src/utils/printTemplates.ts` @ v0.36.0.
//!
//! Books spells the cents as "Paisa" whatever the currency, and so does this.
//! Difference from Books: past the billions Books writes "undefined"; this leaves the
//! scale out.

const ONES: [&str; 10] = [
    "", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine",
];
const TEENS: [&str; 10] = [
    "Ten",
    "Eleven",
    "Twelve",
    "Thirteen",
    "Fourteen",
    "Fifteen",
    "Sixteen",
    "Seventeen",
    "Eighteen",
    "Nineteen",
];
const TENS: [&str; 10] = [
    "", "", "Twenty", "Thirty", "Forty", "Fifty", "Sixty", "Seventy", "Eighty", "Ninety",
];
const SCALES: [&str; 4] = ["", "Thousand", "Million", "Billion"];

fn three_digits(num: u64, t: &dyn Fn(&str) -> String) -> String {
    let mut result = String::new();
    let hundred = num / 100;
    let remainder = num % 100;
    if hundred > 0 {
        result += &format!("{} {}", t(ONES[hundred as usize]), t("Hundred"));
    }
    if remainder > 0 {
        if hundred > 0 {
            result += &format!(" {} ", t("And"));
        }
        if remainder < 10 {
            result += &t(ONES[remainder as usize]);
        } else if remainder < 20 {
            result += &t(TEENS[(remainder - 10) as usize]);
        } else {
            result += &t(TENS[(remainder / 10) as usize]);
            if !remainder.is_multiple_of(10) {
                result += &format!(" {}", t(ONES[(remainder % 10) as usize]));
            }
        }
    }
    result
}

/// The total spelled out: "One Hundred And Twenty and Fifty Paisa only".
pub fn in_words(total: f64, t: &dyn Fn(&str) -> String) -> String {
    // `toFixed(2)`, then the digits only: Books' regex drops a minus sign.
    let fixed = format!("{:.2}", total.abs());
    let (integer, decimal) = fixed.split_once('.').unwrap_or((&fixed, "00"));

    let digits: Vec<char> = integer.chars().collect();
    let mut groups = Vec::new();
    let mut end = digits.len();
    while end > 0 {
        let start = end.saturating_sub(3);
        groups.insert(0, digits[start..end].iter().collect::<String>());
        end = start;
    }

    let mut spelled = String::new();
    let count = groups.len();
    for (index, group) in groups.iter().enumerate() {
        let value: u64 = group.parse().unwrap_or(0);
        if value > 0 {
            spelled += &three_digits(value, t);
            match SCALES.get(count - index - 1) {
                Some(scale) if !scale.is_empty() => spelled += &format!(" {} ", t(scale)),
                _ => spelled.push(' '),
            }
        }
    }
    let mut spelled = spelled.trim().to_owned();
    if spelled.is_empty() {
        spelled = t("Zero");
    }

    let cents: u64 = decimal.parse().unwrap_or(0);
    let decimal = if cents != 0 {
        format!(" {} {} {}", t("and"), three_digits(cents, t), t("Paisa"))
    } else {
        String::new()
    };
    format!("{spelled}{decimal} {}", t("only"))
}

#[cfg(test)]
mod tests {
    use super::in_words;

    fn words(total: f64) -> String {
        in_words(total, &|s| s.to_owned())
    }

    #[test]
    fn spells_totals_as_books_does() {
        assert_eq!(words(0.0), "Zero only");
        assert_eq!(words(300.0), "Three Hundred only");
        assert_eq!(
            words(1180.5),
            "One Thousand One Hundred And Eighty and Fifty Paisa only"
        );
        assert_eq!(words(15.07), "Fifteen and Seven Paisa only");
        assert_eq!(words(2_000_021.0), "Two Million Twenty One only");
        assert_eq!(words(-42.0), "Forty Two only");
    }
}
