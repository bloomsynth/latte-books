// Port of pesa 1.1.13 (https://github.com/frappe/pesa, MIT): src/preciseNumber.ts, src/utils.ts
// and the parts of src/money.ts that read and store amounts. Arithmetic comes with the
// accounting engine.
//
// pesa keeps an amount as a bigint scaled by 10^precision. Here it is an `i128`, which holds
// amounts up to about 10^27 at Books' precision of 11; parsing a larger one is an error.

use std::fmt;

pub const MIN_PREC: u32 = 0;
pub const MAX_PREC: u32 = 36;
pub const DEF_PREC: u32 = 6;
pub const DEF_DISP: u32 = 3;
pub const USE_BNKR: bool = true;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PesaError(String);

impl fmt::Display for PesaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PesaError {}

/// `PreciseNumber` in pesa: a decimal number at a fixed precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PreciseNumber {
    value: i128,
    precision: u32,
    bankers_rounding: bool,
}

impl PreciseNumber {
    /// Parses text such as `"1,740.5"`, `".5"` or `"22."`, as pesa does: commas and
    /// underscores are ignored, and digits past `precision` are rounded half away from zero.
    pub fn parse(value: &str, precision: u32) -> Result<Self, PesaError> {
        let precision = validate_precision(precision)?;
        Ok(Self::from_scaled(scaler(value, precision)?, precision))
    }

    /// Converts a number the way pesa does: through its shortest decimal text.
    pub fn from_f64(value: f64, precision: u32) -> Result<Self, PesaError> {
        if !value.is_finite() {
            return Err(PesaError(format!("invalid value {value} found")));
        }
        let precision = validate_precision(precision)?;
        Ok(Self::from_scaled(
            scale_digits(&format!("{value}"), value < 0.0, precision)?,
            precision,
        ))
    }

    /// An amount already scaled by 10^precision: pesa's bigint input.
    pub fn from_scaled(value: i128, precision: u32) -> Self {
        Self {
            value,
            precision,
            bankers_rounding: USE_BNKR,
        }
    }

    pub fn with_bankers_rounding(mut self, bankers_rounding: bool) -> Self {
        self.bankers_rounding = bankers_rounding;
        self
    }

    /// The amount scaled by 10^precision. `integer` in pesa.
    pub fn integer(&self) -> i128 {
        self.value
    }

    pub fn precision(&self) -> u32 {
        self.precision
    }

    /// The nearest `f64`. `value` and `float` in pesa.
    pub fn float(&self) -> f64 {
        self.value as f64 / 10f64.powi(self.precision as i32)
    }

    pub fn is_zero(&self) -> bool {
        self.value == 0
    }

    pub fn is_positive(&self) -> bool {
        self.value > 0
    }

    pub fn is_negative(&self) -> bool {
        self.value < 0
    }

    /// The text written to the database: every digit of the precision, e.g.
    /// `"1740.00000000000"`.
    pub fn store(&self) -> String {
        self.round(self.precision as i64, None)
    }

    /// Rounds to `to` decimal places, with banker's rounding unless turned off, and pads with
    /// zeros. A port of pesa's string-based `round`.
    pub fn round(&self, to: i64, bankers_rounding: Option<bool>) -> String {
        let bankers_rounding = bankers_rounding.unwrap_or(self.bankers_rounding);
        let to = to.max(0) as usize;
        let precision = self.precision as usize;

        let diff = to as i64 - precision as i64;
        let is_neg = self.value < 0;
        let mut string_rep = self.value.unsigned_abs().to_string();
        if string_rep.len() < precision {
            string_rep = "0".repeat(precision - string_rep.len()) + &string_rep;
        }
        let dpoint = string_rep.len() - precision;
        let whole = if dpoint == 0 {
            "0"
        } else {
            &string_rep[..dpoint]
        };
        let fraction = &string_rep[dpoint..];
        let trailing_zeros = "0".repeat(diff.max(0) as usize);
        let ultimate_digit = digit_at(fraction, to);
        let is_mid = ultimate_digit == Some(5)
            && fraction
                .get(to + 1..)
                .unwrap_or("")
                .bytes()
                .all(|d| d == b'0');
        let mut rounding_digit: u8 = if ultimate_digit.unwrap_or(0) <= 4 || (is_neg && is_mid) {
            0
        } else {
            1
        };

        if bankers_rounding && is_mid && trailing_zeros.is_empty() {
            let penultimate_digit = if to >= 1 {
                digit_at(fraction, to - 1)
            } else {
                dpoint.checked_sub(1).and_then(|i| digit_at(whole, i))
            }
            .unwrap_or(0);
            rounding_digit = if penultimate_digit % 2 == 0 { 0 } else { 1 };
        }

        let kept = fraction.get(..to).unwrap_or(fraction);
        // pesa adds the rounding digit as a bigint; done on the text here so no width limits it.
        let mut low_precision_rep =
            add_digit(&format!("{whole}{kept}{trailing_zeros}"), rounding_digit);
        if low_precision_rep.len() < to {
            low_precision_rep = "0".repeat(to - low_precision_rep.len()) + &low_precision_rep;
        }
        let new_dpoint = low_precision_rep.len() - to;
        let new_whole = if new_dpoint == 0 {
            "0"
        } else {
            &low_precision_rep[..new_dpoint]
        };
        let new_fractional = &low_precision_rep[new_dpoint..];
        let tail = format!(".{new_fractional}{}", "0".repeat(to - new_fractional.len()));
        let no_sign = format!(
            "{new_whole}{}",
            if tail != "." { tail.as_str() } else { "" }
        );
        let is_zero = no_sign.bytes().filter(|d| *d != b'.').all(|d| d == b'0');
        if !is_neg || is_zero {
            no_sign
        } else {
            format!("-{no_sign}")
        }
    }
}

/// The shortest decimal text, with trailing zeros removed. `toString` in pesa.
impl fmt::Display for PreciseNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&to_decimal_string(self.value, self.precision))
    }
}

/// `Money` in pesa, without currency conversion yet: an amount, its currency and how many
/// decimals to display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Money {
    pub number: PreciseNumber,
    pub currency: String,
    pub display: u32,
}

impl Money {
    pub fn store(&self) -> String {
        self.number.store()
    }

    /// Rounds to `to` decimal places, or to the display precision.
    pub fn round(&self, to: Option<i64>) -> String {
        self.number.round(to.unwrap_or(self.display as i64), None)
    }

    pub fn float(&self) -> f64 {
        self.number.float()
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.number.fmt(f)
    }
}

/// `getMoneyMaker` in pesa: makes `Money` with the same currency and precisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoneyMaker {
    pub currency: String,
    pub precision: u32,
    pub display: u32,
}

impl MoneyMaker {
    pub fn parse(&self, value: &str) -> Result<Money, PesaError> {
        Ok(self.wrap(PreciseNumber::parse(value, self.precision)?))
    }

    pub fn from_f64(&self, value: f64) -> Result<Money, PesaError> {
        Ok(self.wrap(PreciseNumber::from_f64(value, self.precision)?))
    }

    pub fn zero(&self) -> Money {
        self.wrap(PreciseNumber::from_scaled(0, self.precision))
    }

    fn wrap(&self, number: PreciseNumber) -> Money {
        Money {
            number,
            currency: self.currency.clone(),
            display: self.display,
        }
    }
}

/// `BigInt(digits) + digit`, as text without leading zeros.
fn add_digit(digits: &str, digit: u8) -> String {
    let mut bytes = digits.as_bytes().to_vec();
    let mut carry = digit;
    for b in bytes.iter_mut().rev() {
        if carry == 0 {
            break;
        }
        let sum = *b - b'0' + carry;
        *b = b'0' + sum % 10;
        carry = sum / 10;
    }
    if carry > 0 {
        bytes.insert(0, b'0' + carry);
    }
    let text = String::from_utf8(bytes).expect("digits only");
    match text.trim_start_matches('0') {
        "" => "0".into(),
        trimmed => trimmed.into(),
    }
}

fn digit_at(digits: &str, i: usize) -> Option<u32> {
    digits.as_bytes().get(i).map(|d| (d - b'0') as u32)
}

fn validate_precision(precision: u32) -> Result<u32, PesaError> {
    if !(MIN_PREC..=MAX_PREC).contains(&precision) {
        return Err(PesaError(format!(
            "precision should be between {MIN_PREC} and {MAX_PREC}"
        )));
    }
    Ok(precision)
}

fn scaler(value: &str, precision: u32) -> Result<i128, PesaError> {
    let value = value.replace(['_', ','], "");
    if !is_input_valid(&value) {
        return Err(PesaError(format!(
            "invalid input '{value}' of type 'string'"
        )));
    }
    let value = strip_zeros(&value);
    // pesa takes the sign for rounding from `Number(value)`, so "-0.5" rounds away from zero.
    let negative = value.starts_with('-') && value.bytes().any(|d| d.is_ascii_digit() && d != b'0');
    scale_digits(&value, negative, precision)
}

/// The scaling half of pesa's `scaler`, once `value` is plain decimal text.
fn scale_digits(value: &str, negative: bool, precision: u32) -> Result<i128, PesaError> {
    let (whole, fractional) = value.split_once('.').unwrap_or((value, ""));
    let mut string_big_rep = format!("{whole}{fractional}");
    if string_big_rep == "-0" {
        string_big_rep = "0".into();
    }
    let too_big = || PesaError(format!("'{value}' is too large"));
    let mut big_rep: i128 = match string_big_rep.as_str() {
        "" => 0,
        "-" => return Err(PesaError(format!("invalid input '{value}'"))),
        digits => digits.parse().map_err(|_| too_big())?,
    };

    let fractional_length = fractional.len() as u32;
    if precision > fractional_length {
        big_rep = 10i128
            .checked_pow(precision - fractional_length)
            .and_then(|scale| big_rep.checked_mul(scale))
            .ok_or_else(too_big)?;
    } else {
        // Digits beyond i128's range only shrink the amount to zero.
        big_rep = 10i128
            .checked_pow(fractional_length - precision)
            .map_or(0, |scale| big_rep / scale);
        if digit_at(fractional, precision as usize).is_some_and(|d| d > 4) {
            big_rep += if negative { -1 } else { 1 };
        }
    }
    Ok(big_rep)
}

/// Matches `22.22`, `.22` and `22.`, with an optional leading minus.
fn is_input_valid(value: &str) -> bool {
    let digits = value.strip_prefix('-').unwrap_or(value);
    let dots = digits.bytes().filter(|d| *d == b'.').count();
    let has_digit = digits.bytes().any(|d| d.is_ascii_digit());
    has_digit && dots <= 1 && digits.bytes().all(|d| d.is_ascii_digit() || d == b'.')
}

fn strip_zeros(value: &str) -> String {
    if value.contains('.') && value.ends_with('0') {
        let (whole, fractional) = value.split_once('.').unwrap();
        let fractional = fractional.trim_end_matches('0');
        if fractional.is_empty() {
            whole.to_owned()
        } else {
            format!("{whole}.{fractional}")
        }
    } else {
        value.to_owned()
    }
}

fn to_decimal_string(value: i128, precision: u32) -> String {
    let precision = precision as usize;
    let sign = if value < 0 { "-" } else { "" };
    let string_rep = value.unsigned_abs().to_string();

    if string_rep.len() < precision {
        let zeros = "0".repeat(precision - string_rep.len());
        return format!("{sign}{}", strip_zeros(&format!("0.{zeros}{string_rep}")));
    } else if string_rep.len() == precision {
        return format!("{sign}{}", strip_zeros(&format!("0.{string_rep}")));
    }

    let d = string_rep.len() - precision;
    let (whole, fractional) = string_rep.split_at(d);
    if fractional.is_empty() {
        format!("{sign}{whole}")
    } else {
        format!("{sign}{}", strip_zeros(&format!("{whole}.{fractional}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(value: &str, precision: u32) -> PreciseNumber {
        PreciseNumber::parse(value, precision).unwrap()
    }

    #[test]
    fn stored_text_survives_a_round_trip() {
        for stored in [
            "1740.00000000000",
            "0.00000000000",
            "-26796.12345678901",
            "155817.00000000000",
            "99999999999999.99999999999",
        ] {
            assert_eq!(parse(stored, 11).store(), stored);
        }
    }

    #[test]
    fn parse_scales_and_rounds_half_away_from_zero() {
        assert_eq!(parse("1,740.5", 2).integer(), 174050);
        assert_eq!(parse(".5", 0).integer(), 1);
        assert_eq!(parse("22.", 2).integer(), 2200);
        assert_eq!(parse("1.005", 2).integer(), 101);
        assert_eq!(parse("-1.005", 2).integer(), -101);
        assert_eq!(parse("-0.5", 0).integer(), -1);
        assert_eq!(parse("-0", 3).integer(), 0);
        assert_eq!(parse("1.004", 2).integer(), 100);
    }

    #[test]
    fn parse_rejects_what_pesa_rejects() {
        for bad in ["", "-", ".", "1.2.3", "abc", "1e5", " 1"] {
            assert!(PreciseNumber::parse(bad, 2).is_err(), "{bad:?}");
        }
        assert!(PreciseNumber::parse("1", 37).is_err());
    }

    #[test]
    fn from_f64_goes_through_text() {
        assert_eq!(
            PreciseNumber::from_f64(89.55, 11).unwrap().store(),
            "89.55000000000"
        );
        assert_eq!(PreciseNumber::from_f64(0.1, 2).unwrap().integer(), 10);
        assert_eq!(PreciseNumber::from_f64(-2.5, 0).unwrap().integer(), -3);
    }

    #[test]
    fn round_uses_bankers_rounding_at_the_midpoint() {
        assert_eq!(parse("2.5", 11).round(0, None), "2");
        assert_eq!(parse("3.5", 11).round(0, None), "4");
        assert_eq!(parse("2.345", 11).round(2, None), "2.34");
        assert_eq!(parse("2.355", 11).round(2, None), "2.36");
        assert_eq!(parse("2.3451", 11).round(2, None), "2.35");
        assert_eq!(parse("2.5", 11).round(0, Some(false)), "3");
        assert_eq!(parse("-2.5", 11).round(0, Some(false)), "-2");
        assert_eq!(parse("-0.001", 11).round(2, None), "0.00");
        assert_eq!(parse("1.5", 2).round(4, None), "1.5000");
        assert_eq!(parse("0.05", 11).round(1, None), "0.0");
    }

    #[test]
    fn display_strips_trailing_zeros() {
        assert_eq!(parse("1740.00000000000", 11).to_string(), "1740");
        assert_eq!(parse("0.50", 11).to_string(), "0.5");
        assert_eq!(parse("-0.05", 11).to_string(), "-0.05");
        assert_eq!(parse("0", 11).to_string(), "0");
    }
}
