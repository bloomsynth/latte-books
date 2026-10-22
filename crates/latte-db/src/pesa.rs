// Port of pesa 1.1.13 (https://github.com/frappe/pesa, MIT): src/preciseNumber.ts, src/utils.ts
// and src/money.ts.
//
// pesa keeps an amount as a bigint scaled by 10^precision. Here it is an `i128`, which holds
// amounts up to about 10^27 at Books' precision of 11; parsing a larger one, or arithmetic
// whose result or intermediate product doesn't fit, is an error.
//
// Not ported: the static `PreciseNumber` methods, `Money.options`, `Money.internal`,
// `Money.toJSON`, and the `currency` and `rate` arguments of the arithmetic and comparison
// methods (a `Money` of another currency still converts through the rates set with
// `Money::rate`). Conversion rates are numbers; pesa also takes text.

use std::fmt;

use indexmap::IndexMap;

pub const MIN_PREC: u32 = 0;
pub const MAX_PREC: u32 = 36;
pub const DEF_PREC: u32 = 6;
pub const DEF_DISP: u32 = 3;
pub const USE_BNKR: bool = true;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PesaError(String);

impl PesaError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

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

    /// The amount rounded to `to` decimal places, at the same precision.
    pub fn clip(&self, to: u32) -> Result<Self, PesaError> {
        Self::parse(&self.round(to as i64, None), self.precision)
    }

    /// A copy with pesa's default rounding.
    pub fn copy(&self) -> Self {
        Self::from_scaled(self.value, self.precision)
    }

    pub fn add(&self, value: &Self) -> Result<Self, PesaError> {
        self.operate(Operator::Add, value)
    }

    pub fn sub(&self, value: &Self) -> Result<Self, PesaError> {
        self.operate(Operator::Sub, value)
    }

    /// Rounds the last digit as pesa's `neutralizedMul` does, which looks only at the first
    /// dropped digit and always rounds up, even for a negative product.
    pub fn mul(&self, value: &Self) -> Result<Self, PesaError> {
        self.operate(Operator::Mul, value)
    }

    /// Truncates toward zero, as bigint division does.
    pub fn div(&self, value: &Self) -> Result<Self, PesaError> {
        self.operate(Operator::Div, value)
    }

    /// `executeComparison`: compares the amounts at this number's precision. pesa's `eq`,
    /// `gt`, `lt`, `gte` and `lte`.
    pub fn compare(&self, value: &Self) -> std::cmp::Ordering {
        self.value.cmp(&value.scaled_to(self.precision))
    }

    /// `scaleAndConvert` for a `PreciseNumber`: pesa's `matchPrecision`, which truncates
    /// when lowering the precision. Raising it past i128 saturates, which only comparisons
    /// see; arithmetic checks for it first.
    fn scaled_to(&self, precision: u32) -> i128 {
        self.checked_scaled_to(precision)
            .unwrap_or(if self.value < 0 { i128::MIN } else { i128::MAX })
    }

    fn checked_scaled_to(&self, precision: u32) -> Option<i128> {
        match self.precision.cmp(&precision) {
            std::cmp::Ordering::Greater => Some(
                10i128
                    .checked_pow(self.precision - precision)
                    .map_or(0, |scale| self.value / scale),
            ),
            std::cmp::Ordering::Less => 10i128
                .checked_pow(precision - self.precision)
                .and_then(|scale| self.value.checked_mul(scale)),
            std::cmp::Ordering::Equal => Some(self.value),
        }
    }

    /// `executeOperation` with two values.
    fn operate(&self, operator: Operator, value: &Self) -> Result<Self, PesaError> {
        let overflow = || {
            PesaError(format!(
                "the result of {self} {operator} {value} is too large"
            ))
        };
        let precision = self.precision;
        let a = self.value;
        let b = value.checked_scaled_to(precision).ok_or_else(overflow)?;
        let neutralizer = 10i128.pow(precision);
        let result = match operator {
            Operator::Add => a.checked_add(b),
            Operator::Sub => a.checked_sub(b),
            Operator::Div if b == 0 => return Err(PesaError("division by zero".into())),
            Operator::Div => a.checked_mul(neutralizer).map(|n| n / b),
            Operator::Mul => a
                .checked_mul(b)
                .map(|product| neutralized_mul(product, precision, neutralizer)),
        };
        Ok(Self::from_scaled(result.ok_or_else(overflow)?, precision))
    }
}

#[derive(Debug, Clone, Copy)]
enum Operator {
    Add,
    Sub,
    Mul,
    Div,
}

impl fmt::Display for Operator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
        })
    }
}

fn neutralized_mul(product: i128, precision: u32, neutralizer: i128) -> i128 {
    let text = product.to_string();
    // `temp.charAt(temp.length - precision)`, where the text keeps its minus sign.
    let rounding = text
        .len()
        .checked_sub(precision as usize)
        .and_then(|i| text.as_bytes().get(i))
        .is_some_and(|d| d.is_ascii_digit() && *d > b'4');
    product / neutralizer + i128::from(rounding)
}

/// The shortest decimal text, with trailing zeros removed. `toString` in pesa.
impl fmt::Display for PreciseNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&to_decimal_string(self.value, self.precision))
    }
}

/// `Money` in pesa: an amount, its currency, how many decimals to display, and rates for
/// converting other currencies into it.
#[derive(Debug, Clone, PartialEq)]
pub struct Money {
    pub number: PreciseNumber,
    pub currency: String,
    pub display: u32,
    /// Keyed `"FROM-TO"`, as in pesa.
    conversion_rates: IndexMap<String, f64>,
}

/// A value to do arithmetic with: pesa's `ArithmeticInput`.
#[derive(Debug, Clone, Copy)]
pub enum Amount<'a> {
    Money(&'a Money),
    Number(f64),
    Text(&'a str),
}

impl<'a> From<&'a Money> for Amount<'a> {
    fn from(money: &'a Money) -> Self {
        Self::Money(money)
    }
}

impl From<f64> for Amount<'_> {
    fn from(number: f64) -> Self {
        Self::Number(number)
    }
}

impl<'a> From<&'a str> for Amount<'a> {
    fn from(text: &'a str) -> Self {
        Self::Text(text)
    }
}

impl Money {
    /// `new Money(amount, { currency, display })`. A currency that isn't three capital
    /// letters is an error, as in pesa; an empty one leaves the money without a currency.
    pub fn new(number: PreciseNumber, currency: &str, display: u32) -> Result<Self, PesaError> {
        if !currency.is_empty() {
            throw_if_invalid_currency_code(currency)?;
        }
        Ok(Self {
            number: number.copy(),
            currency: currency.to_owned(),
            display,
            conversion_rates: IndexMap::new(),
        })
    }

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

    /// Sets the rate for converting `from` into `to`. pesa's `rate` with a `RateSetting`.
    pub fn rate(&mut self, from: &str, to: &str, rate: f64) -> Result<&mut Self, PesaError> {
        let key = get_conversion_rate_key(from, to)?;
        self.conversion_rates.insert(key, rate);
        Ok(self)
    }

    pub fn get_conversion_rate(&self, from: &str, to: &str) -> Result<f64, PesaError> {
        let rate = self
            .conversion_rates
            .get(&get_conversion_rate_key(from, to)?)
            .copied()
            .filter(|rate| *rate != 0.0)
            .or_else(|| {
                let inverse = get_conversion_rate_key(to, from).ok()?;
                let rate = self.conversion_rates.get(&inverse).copied()?;
                (rate != 0.0).then(|| 1.0 / rate)
            });
        rate.ok_or_else(|| PesaError(format!("please set the conversion rate for {from} to {to}")))
    }

    pub fn has_conversion_rate(&self, to: &str) -> bool {
        let (Ok(key), Ok(inverse)) = (
            get_conversion_rate_key(&self.currency, to),
            get_conversion_rate_key(to, &self.currency),
        ) else {
            return false;
        };
        self.conversion_rates.contains_key(&key) || self.conversion_rates.contains_key(&inverse)
    }

    /// Converts into the currency `to`, at `rate` (which is then kept) or at a rate set
    /// before.
    pub fn to(&mut self, to: &str, rate: Option<f64>) -> Result<Self, PesaError> {
        self.throw_currency_not_set_if_not_set()?;
        let rate = match rate {
            Some(rate) => {
                let from = self.currency.clone();
                self.rate(&from, to, rate)?;
                rate
            }
            None => self.get_conversion_rate(&self.currency, to)?,
        };
        let rate = PreciseNumber::from_f64(rate, self.number.precision)?;
        self.copy_self(self.number.mul(&rate)?, to)
    }

    pub fn add<'a>(&self, value: impl Into<Amount<'a>>) -> Result<Self, PesaError> {
        let rhs = self.convert_input(value.into())?;
        self.copy_self(self.number.add(&rhs)?, "")
    }

    pub fn sub<'a>(&self, value: impl Into<Amount<'a>>) -> Result<Self, PesaError> {
        let rhs = self.convert_input(value.into())?;
        self.copy_self(self.number.sub(&rhs)?, "")
    }

    pub fn mul<'a>(&self, value: impl Into<Amount<'a>>) -> Result<Self, PesaError> {
        let rhs = self.convert_input(value.into())?;
        self.copy_self(self.number.mul(&rhs)?, "")
    }

    pub fn div<'a>(&self, value: impl Into<Amount<'a>>) -> Result<Self, PesaError> {
        let rhs = self.convert_input(value.into())?;
        self.copy_self(self.number.div(&rhs)?, "")
    }

    /// `value` percent of the amount.
    pub fn percent(&self, value: f64) -> Result<Self, PesaError> {
        let rate = PreciseNumber::from_f64(value / 100.0, self.number.precision)?;
        self.copy_self(self.number.mul(&rate)?, "")
    }

    /// Splits the amount into `values` equal parts, or by the given percentages, each
    /// rounded to `round` decimals (the display precision by default). If the percentages
    /// add up to 100, the last part takes what rounding left over.
    pub fn split(&self, values: Split<'_>, round: Option<u32>) -> Result<Vec<Self>, PesaError> {
        // Where pesa's `reduce` would throw on an empty list.
        let empty = || PesaError("reduce of empty array with no initial value".into());
        let round = round.unwrap_or(self.display);
        let precision = self.number.precision;
        let percents = match values {
            Split::Count(n) => {
                let part = PreciseNumber::from_f64(100.0 / n as f64, precision)?
                    .clip(round)?
                    .float();
                let mut percents = vec![part; n.saturating_sub(1)];
                let sum = percents
                    .iter()
                    .copied()
                    .reduce(|a, b| a + b)
                    .ok_or_else(empty)?;
                percents.push(100.0 - sum);
                percents
            }
            Split::Percents(percents) => percents.to_vec(),
        };

        let mut total: Option<PreciseNumber> = None;
        for percent in &percents {
            let percent = PreciseNumber::from_f64(*percent, DEF_PREC)?;
            total = Some(match total {
                Some(total) => total.add(&percent)?,
                None => percent,
            });
        }
        let is_full = total.ok_or_else(empty)?.float() == 100.0;
        let last = if is_full {
            percents.len() - 1
        } else {
            percents.len()
        };

        let mut splits = Vec::with_capacity(percents.len());
        for percent in &percents[..last] {
            let rate = PreciseNumber::from_f64(percent / 100.0, precision)?;
            let part = self.number.mul(&rate)?.round(round as i64, None);
            splits.push(self.copy_self(PreciseNumber::parse(&part, precision)?, "")?);
        }

        if is_full {
            let (first, rest) = splits.split_first().ok_or_else(empty)?;
            let mut sum = first.clone();
            for split in rest {
                sum = sum.add(split)?;
            }
            let sum = sum.round(Some(round as i64));
            let whole = PreciseNumber::parse(&self.round(Some(round as i64)), precision)?;
            splits.push(self.copy_self(whole, "")?.sub(sum.as_str())?);
        }
        Ok(splits)
    }

    pub fn abs(&self) -> Result<Self, PesaError> {
        if self.lt(0.0)? {
            return self.mul(-1.0);
        }
        Ok(self.copy())
    }

    pub fn neg(&self) -> Result<Self, PesaError> {
        self.mul(-1.0)
    }

    /// Rounds to `to` decimals, or to the display precision, keeping the precision.
    pub fn clip(&self, to: Option<u32>) -> Result<Self, PesaError> {
        let to = to.unwrap_or(self.display);
        self.copy_self(self.number.clip(to)?, "")
    }

    pub fn copy(&self) -> Self {
        let mut copy = self.clone();
        copy.number = self.number.copy();
        copy
    }

    pub fn eq<'a>(&self, value: impl Into<Amount<'a>>) -> Result<bool, PesaError> {
        Ok(self.compare(value.into())?.is_eq())
    }

    pub fn neq<'a>(&self, value: impl Into<Amount<'a>>) -> Result<bool, PesaError> {
        Ok(self.compare(value.into())?.is_ne())
    }

    pub fn gt<'a>(&self, value: impl Into<Amount<'a>>) -> Result<bool, PesaError> {
        Ok(self.compare(value.into())?.is_gt())
    }

    pub fn lt<'a>(&self, value: impl Into<Amount<'a>>) -> Result<bool, PesaError> {
        Ok(self.compare(value.into())?.is_lt())
    }

    pub fn gte<'a>(&self, value: impl Into<Amount<'a>>) -> Result<bool, PesaError> {
        Ok(self.compare(value.into())?.is_ge())
    }

    pub fn lte<'a>(&self, value: impl Into<Amount<'a>>) -> Result<bool, PesaError> {
        Ok(self.compare(value.into())?.is_le())
    }

    pub fn is_positive(&self) -> bool {
        self.number.is_positive()
    }

    pub fn is_negative(&self) -> bool {
        self.number.is_negative()
    }

    pub fn is_zero(&self) -> bool {
        self.number.is_zero()
    }

    fn compare(&self, value: Amount<'_>) -> Result<std::cmp::Ordering, PesaError> {
        Ok(self.number.compare(&self.convert_input(value)?))
    }

    /// `#convertInput`: the value as a number, converted into this currency if it is money
    /// of another one.
    fn convert_input(&self, value: Amount<'_>) -> Result<PreciseNumber, PesaError> {
        let precision = self.number.precision;
        let (rhs, money) = match value {
            Amount::Money(money) => (money.number, Some(money)),
            Amount::Number(number) => (PreciseNumber::from_f64(number, precision)?, None),
            Amount::Text(text) => (PreciseNumber::parse(text, precision)?, None),
        };
        let Some(money) = money.filter(|m| !m.currency.is_empty() && m.currency != self.currency)
        else {
            return Ok(rhs);
        };

        let currency = &money.currency;
        let rate = money
            .get_conversion_rate(currency, &self.currency)
            .or_else(|_| self.get_conversion_rate(currency, &self.currency))
            .map_err(|_| {
                PesaError(format!(
                    "rate not provided for conversion from {currency} to {}",
                    self.currency
                ))
            })?;
        rhs.mul(&PreciseNumber::from_f64(rate, rhs.precision)?)
    }

    /// `#copySelf`: new money at this precision and display, in `currency` or this one,
    /// with this money's conversion rates.
    fn copy_self(&self, number: PreciseNumber, currency: &str) -> Result<Self, PesaError> {
        let currency = if currency.is_empty() {
            &self.currency
        } else {
            currency
        };
        let precision = self.number.precision;
        let scaled = number
            .checked_scaled_to(precision)
            .ok_or_else(|| PesaError(format!("'{number}' is too large")))?;
        let mut money = Self::new(
            PreciseNumber::from_scaled(scaled, precision),
            currency,
            self.display,
        )?;
        money.conversion_rates = self.conversion_rates.clone();
        Ok(money)
    }

    fn throw_currency_not_set_if_not_set(&self) -> Result<(), PesaError> {
        if self.currency.is_empty() {
            return Err(PesaError("currency has not been set for conversion".into()));
        }
        Ok(())
    }
}

/// How to split money: into equal parts, or by percentages.
#[derive(Debug, Clone, Copy)]
pub enum Split<'a> {
    Count(usize),
    Percents(&'a [f64]),
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
            conversion_rates: IndexMap::new(),
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

fn throw_if_invalid_currency_code(code: &str) -> Result<(), PesaError> {
    if code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase()) {
        return Ok(());
    }
    Err(PesaError(format!("invalid currency code '{code}'")))
}

fn get_conversion_rate_key(from: &str, to: &str) -> Result<String, PesaError> {
    throw_if_invalid_currency_code(from)?;
    throw_if_invalid_currency_code(to)?;
    Ok(format!("{from}-{to}"))
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

    #[test]
    fn add_and_sub_match_the_precision_by_truncating() {
        let a = parse("1.10", 2);
        assert_eq!(a.add(&parse("2.25", 2)).unwrap().integer(), 335);
        assert_eq!(a.add(&parse("0.0199", 4)).unwrap().integer(), 111);
        assert_eq!(a.sub(&parse("2.25", 2)).unwrap().integer(), -115);
        assert_eq!(parse("0.0199", 4).add(&a).unwrap().integer(), 11199);
    }

    #[test]
    fn mul_rounds_on_the_first_dropped_digit() {
        let mul = |a: &str, b: &str, precision| {
            parse(a, precision)
                .mul(&parse(b, precision))
                .unwrap()
                .to_string()
        };
        assert_eq!(mul("1.25", "1.25", 2), "1.56");
        assert_eq!(mul("1.5", "1.7", 1), "2.6");
        // pesa adds the rounding digit without minding the sign.
        assert_eq!(mul("-1.5", "1.7", 1), "-2.4");
        assert_eq!(mul("-1.4", "1.2", 1), "-1.5");
        assert_eq!(mul("3", "4", 0), "12");
        assert_eq!(mul("0.05", "0.1", 2), "0.01");
        assert_eq!(mul("-0.05", "0.1", 2), "0.01");
    }

    #[test]
    fn div_truncates() {
        let div = |a: &str, b: &str| parse(a, 2).div(&parse(b, 2)).map(|n| n.to_string());
        assert_eq!(div("1", "3").unwrap(), "0.33");
        assert_eq!(div("2", "3").unwrap(), "0.66");
        assert_eq!(div("-2", "3").unwrap(), "-0.66");
        assert!(div("1", "0").is_err());
    }

    #[test]
    fn arithmetic_that_overflows_is_an_error() {
        let big = parse("99999999999999999999999999", 11);
        assert!(big.mul(&big).is_err());
        assert!(big.add(&big).is_ok());
        assert!(big.div(&parse("0.5", 11)).is_err());
    }

    #[test]
    fn compare_matches_the_precision() {
        use std::cmp::Ordering;
        assert_eq!(
            parse("1.00", 2).compare(&parse("1.005", 3)),
            Ordering::Equal
        );
        assert_eq!(
            parse("1.01", 2).compare(&parse("1.005", 3)),
            Ordering::Greater
        );
        assert_eq!(parse("-1", 2).compare(&parse("0", 2)), Ordering::Less);
    }

    #[test]
    fn clip_rounds_and_keeps_the_precision() {
        let clipped = parse("2.345", 11).clip(2).unwrap();
        assert_eq!(clipped.precision(), 11);
        assert_eq!(clipped.store(), "2.34000000000");
    }

    fn inr() -> MoneyMaker {
        MoneyMaker {
            currency: "INR".into(),
            precision: 11,
            display: 2,
        }
    }

    #[test]
    fn money_arithmetic() {
        let pesa = inr();
        let m = pesa.from_f64(0.2).unwrap();
        assert_eq!(m.add(0.1).unwrap().store(), "0.30000000000");
        assert_eq!(m.sub("1,000").unwrap().to_string(), "-999.8");
        assert_eq!(m.mul(3.0).unwrap().to_string(), "0.6");
        assert_eq!(
            m.div(&pesa.parse("3").unwrap()).unwrap().round(None),
            "0.07"
        );
        assert_eq!(
            pesa.parse("200")
                .unwrap()
                .percent(18.0)
                .unwrap()
                .to_string(),
            "36"
        );
        let negative = pesa.parse("-5.5").unwrap();
        assert_eq!(negative.abs().unwrap().to_string(), "5.5");
        assert_eq!(negative.neg().unwrap().to_string(), "5.5");
        assert_eq!(m.abs().unwrap(), m);
        assert_eq!(
            pesa.parse("2.345").unwrap().clip(None).unwrap().store(),
            "2.34000000000"
        );
        let sum = m.add(&m).unwrap();
        assert_eq!(sum.currency, "INR");
        assert_eq!(sum.display, 2);
    }

    #[test]
    fn money_comparisons() {
        let ten = inr().parse("10").unwrap();
        assert!(ten.eq("10").unwrap());
        assert!(ten.eq(10.0).unwrap());
        assert!(ten.neq(&inr().zero()).unwrap());
        assert!(ten.gt(9.99).unwrap());
        assert!(ten.lt(10.01).unwrap());
        assert!(ten.gte(10.0).unwrap() && ten.lte(10.0).unwrap());
        assert!(ten.eq("ten").is_err());
    }

    #[test]
    fn money_of_another_currency_needs_a_rate() {
        let usd = MoneyMaker {
            currency: "USD".into(),
            ..inr()
        };
        let dollar = usd.parse("1").unwrap();
        let rupees = inr().parse("160").unwrap();
        assert_eq!(
            dollar.add(&rupees).unwrap_err().to_string(),
            "rate not provided for conversion from INR to USD"
        );

        let mut dollar = dollar;
        dollar.rate("USD", "INR", 80.0).unwrap();
        assert_eq!(dollar.add(&rupees).unwrap().to_string(), "3");
        assert!(dollar.has_conversion_rate("INR"));
        assert_eq!(dollar.get_conversion_rate("INR", "USD").unwrap(), 0.0125);

        let mut two = usd.parse("2").unwrap();
        let converted = two.to("INR", Some(80.0)).unwrap();
        assert_eq!(converted.currency, "INR");
        assert_eq!(converted.to_string(), "160");
        assert!(two.rate("usd", "INR", 1.0).is_err());
    }

    #[test]
    fn money_splits() {
        let hundred = inr().parse("100").unwrap();
        let rounded = |splits: Vec<Money>| splits.iter().map(|m| m.round(None)).collect::<Vec<_>>();
        assert_eq!(
            rounded(hundred.split(Split::Count(3), None).unwrap()),
            ["33.33", "33.33", "33.34"]
        );
        assert_eq!(
            rounded(hundred.split(Split::Percents(&[50.0, 25.0]), None).unwrap()),
            ["50.00", "25.00"]
        );
        assert_eq!(
            rounded(hundred.split(Split::Percents(&[70.0, 30.0]), None).unwrap()),
            ["70.00", "30.00"]
        );
        assert!(hundred.split(Split::Count(1), None).is_err());
        assert!(hundred.split(Split::Percents(&[]), None).is_err());
    }
}
