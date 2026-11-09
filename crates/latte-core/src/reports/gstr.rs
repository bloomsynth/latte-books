// Port of reports/GoodsAndServiceTax/ @ v0.36.0: BaseGSTR.ts and GSTR1.ts.
// GSTR-1 lists the sales invoices of a period, with each one's GST. Books' GSTR-2, the
// purchase invoices, is left out: the return hasn't been filed since 2017, and input tax
// credit is claimed from the GSTR-2B the portal makes. The JSON export of gstExporter.ts is in gstr/export.rs.
//
// Books bugs Latte fixes:
// - The rate of an invoice is its taxes' rates added up once. Books adds them twice, so
//   an invoice with 18% GST shows a rate of 36, is never nil rated unless it has no GST,
//   and its B2C-Large and B2C-Small export has twice the integrated tax.
// - Each tax account sets only its own amount. Books' `switch` has no `break`s, so an
//   IGST account also sets the central and state tax, a CGST one also the state tax, and
//   every GST account marks the invoice nil rated, exempt and non-GST.
// - The period is the invoices' dates as the app shows them. Books compares the stored
//   date and time with the period's dates as text, which leaves out the invoices of the
//   To Date, and in India those of the From Date entered before 5:30 in the morning.
// - B2C-Large is an inter-state sale to an unregistered buyer of more than ₹1 lakh, the
//   limit since 1 August 2024 (Notification 12/2024-Central Tax), and of more than
//   ₹2.5 lakh before. Books uses ₹2.5 lakh or more throughout.
//
// Deviation: the exports are what the app calls (`can_export` and `export_json` of
// `Report`), not actions of the report.
//
// Not in Books: the period is a Financial Year and a Period, as the GST portal asks for a
// return: a month, or a quarter for a company that files quarterly (Return Filing, in GST
// Settings), or Custom, which shows From Date and To Date. It starts at the latest period
// that has ended. GSTR-1 says when it is due, and the exports are named by the GSTIN and
// the period (gstr/period.rs).

mod export;
pub mod period;

pub use period::{Filing, Period};

use jiff::civil::{Date, date};
use jiff::{Timestamp, ToSpan, Zoned};
use latte_db::{
    DocValue, DocValueMap, Filter as DbFilter, FilterOperand, FilterValue as DbFilterValue,
    GetAllOptions, QueryFilter,
};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::format::{Formatter, local_date};
use crate::fyo::model::helpers::{is_truthy, js_number_string};
use crate::models::base_models::address::CODE_STATE_MAP;
use crate::models::helpers::single_text;
use crate::reports::stock_ledger::{date_filter, text_filter};
use crate::reports::{
    Align, Column, Filter, FilterKind, FilterValue, RawValue, Report, ReportCell, ReportRow,
};

/// `GSTRRow`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GstrRow {
    pub gstin: String,
    pub party_name: String,
    pub inv_no: String,
    pub inv_date: Option<Timestamp>,
    pub rate: f64,
    pub reverse_charge: &'static str,
    pub in_state: bool,
    pub place: String,
    pub inv_amt: f64,
    pub tax_val: f64,
    pub igst_amt: Option<f64>,
    pub cgst_amt: Option<f64>,
    pub sgst_amt: Option<f64>,
    pub exempt: bool,
    pub non_gst: bool,
    pub nil_rated: bool,
}

/// `BaseGSTR`, as GSTR1.
#[derive(Debug, Clone, Default)]
pub struct Gstr {
    /// A GST state code.
    pub place: Option<String>,
    /// The financial year, by the year it begins in.
    pub fiscal_year: Option<i16>,
    /// The return period of the financial year; Custom is From Date to To Date.
    pub period: Option<Period>,
    pub from_date: Option<Date>,
    pub to_date: Option<Date>,
    /// B2B, B2CL, B2CS or NR.
    pub transfer_type: Option<String>,
    /// `gstrRows`: the rows of the last run.
    pub rows: Vec<GstrRow>,
}

/// A submitted invoice of the period.
#[derive(Debug, Clone)]
struct PeriodInvoice {
    name: String,
    /// The date the invoice shows.
    date: Date,
    cancelled: bool,
    is_return: bool,
}

/// The kind of value a column shows, as its `fieldtype`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Data,
    Currency,
    Date,
    /// No `fieldtype`, as the rate has.
    None,
}

fn state(code: &str) -> Option<&'static str> {
    CODE_STATE_MAP
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, state)| *state)
}

fn state_code(state: &str) -> Option<&'static str> {
    CODE_STATE_MAP
        .iter()
        .find(|(_, s)| *s == state)
        .map(|(code, _)| *code)
}

fn text_value(map: &DocValueMap, fieldname: &str) -> String {
    match map.get(fieldname) {
        Some(DocValue::Text(text)) => text.clone(),
        _ => String::new(),
    }
}

fn money_float(map: &DocValueMap, fieldname: &str) -> f64 {
    match map.get(fieldname) {
        Some(DocValue::Money(money)) => money.float(),
        Some(DocValue::Number(number)) => *number,
        _ => 0.0,
    }
}

fn number_value(map: &DocValueMap, fieldname: &str) -> Option<f64> {
    match map.get(fieldname) {
        Some(DocValue::Number(number)) => Some(*number),
        _ => None,
    }
}

/// The date a Date field's value shows.
fn shown_date(value: Option<&DocValue>) -> Option<Date> {
    match value? {
        DocValue::Date(timestamp) => Some(local_date(*timestamp)),
        DocValue::Text(text) => text.get(..10)?.parse().ok(),
        _ => None,
    }
}

/// Whether the company is registered for GST: it has a GSTIN. Only then are the GST
/// reports shown.
pub fn is_gst_registered(fyo: &Fyo) -> bool {
    single_text(fyo, "AccountingSettings", "gstin").is_some()
}

/// The value over which an inter-state sale to an unregistered buyer is B2C-Large: ₹1
/// lakh from 1 August 2024 (Notification 12/2024-Central Tax), ₹2.5 lakh before.
fn b2cl_limit(invoice_date: Option<Date>) -> f64 {
    match invoice_date {
        Some(invoice_date) if invoice_date < date(2024, 8, 1) => 250_000.0,
        _ => 100_000.0,
    }
}

/// A party's GSTIN, and the state supplied to: its address's place of supply, or else
/// the state of its GSTIN.
fn party_place(fyo: &Fyo, party: &str) -> Result<(String, String), Error> {
    let party = fyo.db.get_doc("Party", party)?;
    let gstin = text_value(&party, "gstin");
    let address = text_value(&party, "address");
    let place = if !address.is_empty() {
        match fyo.get_value("Address", Some(&address), "pos") {
            Some(DocValue::Text(pos)) => pos,
            _ => String::new(),
        }
    } else if !gstin.is_empty() {
        gstin
            .get(..2)
            .and_then(state)
            .unwrap_or_default()
            .to_owned()
    } else {
        String::new()
    };
    Ok((gstin, place))
}

impl Gstr {
    pub fn new() -> Self {
        Self::default()
    }

    /// `transferTypeMap`, as `(value, label)`.
    pub fn transfer_types(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("B2B", "B2B"),
            ("B2CL", "B2C-Large"),
            ("B2CS", "B2C-Small"),
            ("NR", "Nil Rated, Exempted and Non GST supplies"),
        ]
    }

    /// `schemaName`.
    pub fn schema_name(&self) -> &'static str {
        "SalesInvoice"
    }

    fn transfer_type(&self) -> &str {
        self.transfer_type.as_deref().unwrap_or("B2B")
    }

    /// `setDefaultFilters`, for a return period: the latest period that has ended, a month
    /// or a quarter as the company files, and B2B. A report given only dates is for those
    /// dates (Custom), and Custom without them is Books' last three months.
    pub fn set_default_filters(&mut self, filing: Filing, today: Date) {
        if self.period.is_none() {
            if self.from_date.is_some() || self.to_date.is_some() {
                self.period = Some(Period::Custom);
            } else {
                let (year, period) = period::last_ended(filing, today);
                let fiscal_year = *self.fiscal_year.get_or_insert(year);
                let all = Period::all(filing);
                self.period = Some(match fiscal_year.cmp(&year) {
                    std::cmp::Ordering::Equal => period,
                    std::cmp::Ordering::Less => all[all.len() - 1],
                    std::cmp::Ordering::Greater => all[0],
                });
            }
        }
        match self.period {
            Some(Period::Custom) | None => {
                if self.to_date.is_none() {
                    self.to_date = Some(today);
                }
                if self.from_date.is_none() {
                    self.from_date = today.checked_sub(3.months()).ok();
                }
                if self.fiscal_year.is_none() {
                    self.fiscal_year = self.from_date.map(period::fiscal_year_of);
                }
            }
            Some(period) => {
                self.fiscal_year
                    .get_or_insert_with(|| period::latest_fiscal_year(period, today));
                self.set_period_dates();
            }
        }
        if self.transfer_type.is_none() {
            self.transfer_type = Some("B2B".to_owned());
        }
    }

    /// From Date and To Date as the period's, unless it is Custom.
    fn set_period_dates(&mut self) {
        if let (Some(period), Some(year)) = (self.period, self.fiscal_year)
            && let Some((from, to)) = period.dates(year)
        {
            self.from_date = Some(from);
            self.to_date = Some(to);
        }
    }

    /// The day GSTR-1 for the period is due; none for a Custom period.
    pub fn due_date(&self) -> Option<Date> {
        self.period?.gstr1_due_date(self.fiscal_year?)
    }

    /// The Financial Year filter's options, latest first: the years since GST began.
    fn fiscal_years(&self, today: Date) -> Vec<(String, String)> {
        let mut years: Vec<i16> =
            (period::FIRST_FISCAL_YEAR..=period::fiscal_year_of(today)).collect();
        if let Some(year) = self.fiscal_year
            && !years.contains(&year)
        {
            years.push(year);
            years.sort_unstable();
        }
        years
            .into_iter()
            .rev()
            .map(|year| {
                let label = period::fiscal_year_label(year);
                (label.clone(), label)
            })
            .collect()
    }

    /// The Period filter's options: the year's months or quarters that have begun, as the
    /// company files, then Custom.
    fn periods(&self, filing: Filing, today: Date) -> Vec<(String, String)> {
        let year = self
            .fiscal_year
            .unwrap_or_else(|| period::fiscal_year_of(today));
        let mut periods: Vec<Period> = Period::all(filing)
            .into_iter()
            .filter(|period| {
                period.dates(year).is_some_and(|(first, last)| {
                    first <= today && last >= date(period::FIRST_FISCAL_YEAR, 7, 1)
                })
            })
            .collect();
        if let Some(period) = self.period
            && period != Period::Custom
            && !periods.contains(&period)
        {
            periods.push(period);
        }
        periods.push(Period::Custom);
        periods
            .into_iter()
            .map(|period| (period.label(year), period.value()))
            .collect()
    }

    /// `filterGstrRows` and `transferFilterFunction`.
    fn allows(&self, row: &GstrRow) -> bool {
        if let Some(place) = self.place.as_deref()
            && state(place) != Some(row.place.as_str())
        {
            return false;
        }
        let large = row.inv_amt > b2cl_limit(row.inv_date.map(local_date));
        match self.transfer_type.as_deref() {
            Some("B2B") => !row.gstin.is_empty(),
            Some("B2CL") => row.gstin.is_empty() && !row.in_state && large,
            Some("B2CS") => row.gstin.is_empty() && (row.in_state || !large),
            Some("NR") => row.rate == 0.0,
            _ => true,
        }
    }

    /// The period's submitted invoices, cancelled ones too if asked, by the dates they
    /// show. The dates are stored as instants, so the query takes a day more on each side
    /// and the dates are then compared as the app shows them.
    fn period_invoices(&self, fyo: &Fyo, cancelled: bool) -> Result<Vec<PeriodInvoice>, Error> {
        let bound = |date: Option<Date>, days: i64| {
            date.and_then(|d| d.checked_add(days.days()).ok())
                .map(|d| DbFilterValue::Text(d.to_string()))
        };
        let mut filters = QueryFilter::from([("submitted".to_owned(), DbFilter::from(true))]);
        if !cancelled {
            filters.insert("cancelled".to_owned(), DbFilter::from(false));
        }
        let date = match (bound(self.to_date, 2), bound(self.from_date, -1)) {
            (Some(to), Some(from)) => Some(DbFilter::Two(
                "<".to_owned(),
                FilterOperand::One(to),
                ">=".to_owned(),
                FilterOperand::One(from),
            )),
            (Some(to), None) => Some(DbFilter::op("<", to)),
            (None, Some(from)) => Some(DbFilter::op(">=", from)),
            (None, None) => None,
        };
        if let Some(date) = date {
            filters.insert("date".to_owned(), date);
        }
        let options = GetAllOptions {
            fields: Some(
                ["name", "date", "cancelled", "returnAgainst"]
                    .map(str::to_owned)
                    .to_vec(),
            ),
            filters,
            ..GetAllOptions::default()
        };
        let mut invoices = Vec::new();
        for row in fyo.db.get_all_docs(self.schema_name(), &options)? {
            let Some(date) = shown_date(row.get("date")) else {
                continue;
            };
            if self.from_date.is_some_and(|from| date < from)
                || self.to_date.is_some_and(|to| date > to)
            {
                continue;
            }
            invoices.push(PeriodInvoice {
                name: text_value(&row, "name"),
                date,
                cancelled: is_truthy(row.get("cancelled")),
                is_return: is_truthy(row.get("returnAgainst")),
            });
        }
        Ok(invoices)
    }

    /// `getEntries`: the submitted invoices of the period.
    fn entries(&self, fyo: &Fyo) -> Result<Vec<String>, Error> {
        Ok(self
            .period_invoices(fyo, false)?
            .into_iter()
            .map(|invoice| invoice.name)
            .collect())
    }

    /// `getGstrRow`.
    fn gstr_row(&self, fyo: &Fyo, name: &str) -> Result<GstrRow, Error> {
        let entry = fyo.db.get_doc(self.schema_name(), name)?;
        let gstin = match fyo.get_value("AccountingSettings", None, "gstin") {
            Some(DocValue::Text(gstin)) => gstin,
            _ => String::new(),
        };
        let party_name = text_value(&entry, "party");
        let (party_gstin, place) = party_place(fyo, &party_name)?;

        let in_state = !gstin.is_empty() && gstin.get(..2).and_then(state) == Some(place.as_str());
        let mut row = GstrRow {
            reverse_charge: if party_gstin.is_empty() { "Y" } else { "N" },
            gstin: party_gstin,
            party_name,
            inv_no: name.to_owned(),
            inv_date: match entry.get("date") {
                Some(DocValue::Date(date)) => Some(*date),
                _ => None,
            },
            in_state,
            place,
            inv_amt: money_float(&entry, "grandTotal"),
            tax_val: money_float(&entry, "netTotal"),
            ..GstrRow::default()
        };
        set_tax_values(fyo, &entry, &mut row)?;
        Ok(row)
    }

    /// `getGstrRows` and `filterGstrRows`.
    pub fn gstr_rows(&self, fyo: &Fyo) -> Result<Vec<GstrRow>, Error> {
        let mut rows = Vec::new();
        for name in self.entries(fyo)? {
            let row = self.gstr_row(fyo, &name)?;
            if self.allows(&row) {
                rows.push(row);
            }
        }
        Ok(rows)
    }

    fn kinded_columns(&self) -> Vec<(Column, Kind)> {
        let column = |label: &str, fieldname: &str, kind: Kind, width: f64| {
            let align = if kind == Kind::Currency {
                Align::Right
            } else {
                Align::Left
            };
            (
                Column {
                    label: label.to_owned(),
                    fieldname: fieldname.to_owned(),
                    align,
                    width,
                },
                kind,
            )
        };
        let mut columns = vec![
            column("Party", "partyName", Kind::Data, 1.5),
            column("Invoice No.", "invNo", Kind::Data, 1.0),
            column("Invoice Value", "invAmt", Kind::Currency, 1.0),
            column("Invoice Date", "invDate", Kind::Date, 1.0),
            column("Place of supply", "place", Kind::Data, 1.0),
            column("Rate", "rate", Kind::None, 0.5),
            column("Taxable Value", "taxVal", Kind::Currency, 1.0),
            column("Reverse Chrg.", "reverseCharge", Kind::Data, 1.0),
            column("Integrated Tax", "igstAmt", Kind::Currency, 1.0),
            column("Central Tax", "cgstAmt", Kind::Currency, 1.0),
            column("State Tax", "sgstAmt", Kind::Currency, 1.0),
        ];
        if self.transfer_type() == "B2B" {
            columns.insert(0, column("GSTIN No.", "gstin", Kind::Data, 1.5));
        }
        columns
    }

    /// `getReportDataFromGSTRRows`.
    fn report_row(
        fyo: &Fyo,
        formatter: &Formatter,
        columns: &[(Column, Kind)],
        row: &GstrRow,
    ) -> Result<ReportRow, Error> {
        let mut cells = Vec::new();
        for (column, kind) in columns {
            let (value, raw_value) = match column.fieldname.as_str() {
                "gstin" => text_cell(&row.gstin),
                "partyName" => text_cell(&row.party_name),
                "invNo" => text_cell(&row.inv_no),
                "place" => text_cell(&row.place),
                "reverseCharge" => text_cell(row.reverse_charge),
                "invDate" => match row.inv_date {
                    Some(date) => (formatter.date(local_date(date)), RawValue::Date(date)),
                    None => (String::new(), RawValue::Empty),
                },
                "rate" => (js_number_string(row.rate), RawValue::Number(row.rate)),
                "invAmt" => currency_cell(fyo, formatter, Some(row.inv_amt))?,
                "taxVal" => currency_cell(fyo, formatter, Some(row.tax_val))?,
                "igstAmt" => currency_cell(fyo, formatter, row.igst_amt)?,
                "cgstAmt" => currency_cell(fyo, formatter, row.cgst_amt)?,
                "sgstAmt" => currency_cell(fyo, formatter, row.sgst_amt)?,
                _ => (String::new(), RawValue::Empty),
            };
            cells.push(ReportCell {
                value,
                raw_value,
                align: if *kind == Kind::Currency {
                    Align::Right
                } else {
                    Align::Left
                },
                width: column.width,
                ..ReportCell::default()
            });
        }
        Ok(ReportRow {
            cells,
            ..ReportRow::default()
        })
    }

    /// `getCanExport`: the export needs the company's GSTIN, which is in GST Settings.
    pub fn company_gstin(fyo: &Fyo) -> Result<String, Error> {
        match fyo.get_value("AccountingSettings", None, "gstin") {
            Some(DocValue::Text(gstin)) if !gstin.is_empty() => Ok(gstin),
            _ => Err(Error::Validation(
                "Cannot Export: Please set GSTIN in GST Settings.".to_owned(),
            )),
        }
    }

    /// `getGstrJsonData`: the period's return as the GST portal's JSON (gstr/export.rs).
    pub fn gstr_json(&self, fyo: &Fyo) -> Result<String, Error> {
        export::gstr_json(self, fyo)
    }
}

/// `setTaxValuesOnGSTRRow`: the rate, and each GST account's amount.
fn set_tax_values(fyo: &Fyo, entry: &DocValueMap, row: &mut GstrRow) -> Result<(), Error> {
    let Some(DocValue::Table(taxes)) = entry.get("taxes") else {
        return Ok(());
    };
    let net_total = match entry.get("netTotal") {
        Some(DocValue::Money(money)) => money.clone(),
        _ => fyo.pesa().zero(),
    };
    for tax in taxes {
        let rate = number_value(tax, "rate").unwrap_or(0.0);
        row.rate += rate;
        let tax_amt = net_total.percent(rate)?.float();
        match text_value(tax, "account").as_str() {
            "IGST" => {
                row.igst_amt = Some(tax_amt);
                row.in_state = false;
            }
            "CGST" => row.cgst_amt = Some(tax_amt),
            "SGST" => row.sgst_amt = Some(tax_amt),
            "Nil Rated" => row.nil_rated = true,
            "Exempt" => row.exempt = true,
            "Non GST" => row.non_gst = true,
            _ => {}
        }
    }
    Ok(())
}

fn text_cell(text: &str) -> (String, RawValue) {
    (text.to_owned(), RawValue::Text(text.to_owned()))
}

/// A currency cell, its raw value the float as in Books; none when the row has no value.
fn currency_cell(
    fyo: &Fyo,
    formatter: &Formatter,
    value: Option<f64>,
) -> Result<(String, RawValue), Error> {
    let Some(value) = value else {
        return Ok((String::new(), RawValue::Empty));
    };
    let money = fyo.pesa().from_f64(value)?;
    Ok((formatter.currency(&money), RawValue::Number(value)))
}

impl Report for Gstr {
    fn title(&self) -> &'static str {
        "GSTR1"
    }

    fn report_name(&self) -> &'static str {
        "gstr-1"
    }

    fn filters(&self, fyo: &Fyo) -> Vec<Filter> {
        let today = Zoned::now().date();
        let transfer_types = self
            .transfer_types()
            .into_iter()
            .map(|(value, label)| (label.to_owned(), value.to_owned()))
            .collect();
        let places = CODE_STATE_MAP
            .iter()
            .map(|(code, state)| ((*state).to_owned(), (*code).to_owned()))
            .collect();
        let mut filters = vec![
            Filter::new(
                "fiscalYear",
                "Financial Year",
                FilterKind::Select(self.fiscal_years(today)),
            ),
            Filter::new(
                "period",
                "Period",
                FilterKind::Select(self.periods(Filing::of(fyo), today)),
            ),
            Filter::new(
                "transferType",
                "Transfer Type",
                FilterKind::Select(transfer_types),
            ),
            Filter::new("place", "Place", FilterKind::Select(places)),
        ];
        // Before the first run there is no period yet, and the dates can be given instead,
        // as an AI assistant may.
        if !matches!(self.period, Some(Period::Month(_) | Period::Quarter(_))) {
            filters.extend([
                Filter::new("fromDate", "From Date", FilterKind::Date),
                Filter::new("toDate", "To Date", FilterKind::Date),
            ]);
        }
        filters
    }

    fn get(&self, fieldname: &str) -> FilterValue {
        match fieldname {
            "fiscalYear" => {
                FilterValue::text(self.fiscal_year.map(period::fiscal_year_label).as_deref())
            }
            "period" => FilterValue::text(self.period.map(Period::value).as_deref()),
            "transferType" => FilterValue::text(self.transfer_type.as_deref()),
            "place" => FilterValue::text(self.place.as_deref()),
            "fromDate" => FilterValue::date(self.from_date),
            "toDate" => FilterValue::date(self.to_date),
            _ => FilterValue::Null,
        }
    }

    /// A Financial Year or Period sets the dates to the period's; a date makes the period
    /// Custom. A cleared period is the latest that has ended again.
    fn set(&mut self, fieldname: &str, value: FilterValue) {
        match fieldname {
            "fiscalYear" => {
                self.fiscal_year = text_filter(&value)
                    .as_deref()
                    .and_then(period::parse_fiscal_year);
                self.set_period_dates();
            }
            "period" => {
                self.period = text_filter(&value).as_deref().and_then(Period::parse);
                if self.period.is_none() {
                    (self.from_date, self.to_date) = (None, None);
                }
                self.set_period_dates();
            }
            "transferType" => self.transfer_type = text_filter(&value),
            "place" => self.place = text_filter(&value),
            "fromDate" => {
                self.from_date = date_filter(&value);
                self.period = Some(Period::Custom);
            }
            "toDate" => {
                self.to_date = date_filter(&value);
                self.period = Some(Period::Custom);
            }
            _ => {}
        }
    }

    fn columns(&self, _fyo: &Fyo, _formatter: &Formatter) -> Result<Vec<Column>, Error> {
        Ok(self.kinded_columns().into_iter().map(|(c, _)| c).collect())
    }

    fn run(&mut self, fyo: &Fyo, formatter: &Formatter) -> Result<Vec<ReportRow>, Error> {
        self.set_default_filters(Filing::of(fyo), Zoned::now().date());
        self.rows = self.gstr_rows(fyo)?;
        let columns = self.kinded_columns();
        self.rows
            .iter()
            .map(|row| Self::report_row(fyo, formatter, &columns, row))
            .collect()
    }

    fn has_json_export(&self) -> bool {
        true
    }

    fn export_json(&self, fyo: &Fyo) -> Option<Result<String, Error>> {
        Some(self.gstr_json(fyo))
    }

    fn can_export(&self, fyo: &Fyo) -> Result<(), Error> {
        Self::company_gstin(fyo).map(|_| ())
    }

    /// When GSTR-1 for the period is due, or was.
    fn note(&self, _fyo: &Fyo, formatter: &Formatter) -> Option<String> {
        let due = self.due_date()?;
        let shown = formatter.date(due);
        Some(if Zoned::now().date() > due {
            format!("GSTR-1 was due on {shown}")
        } else {
            format!("GSTR-1 is due on {shown}")
        })
    }

    /// GSTR1_<GSTIN>_<MMYYYY>: the return's GSTIN and its `fp`, the period's last month;
    /// a Custom period's dates.
    fn file_name(&self, fyo: &Fyo) -> Option<String> {
        let gstin = Self::company_gstin(fyo).ok()?;
        let period = match self.period? {
            Period::Custom => format!("{}_{}", self.from_date?, self.to_date?),
            _ => self.to_date?.strftime("%m%Y").to_string(),
        };
        Some(format!("{}_{gstin}_{period}", self.title()))
    }
}
