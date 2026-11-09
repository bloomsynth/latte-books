// Not in Books: GSTR-3B, the monthly or quarterly summary return, as the figures to enter
// on the GST portal. GSTR-3B has no file to upload, so the report is the form's tables:
//
// - 3.1(a), (c) and (e): the period's sales, taxed, nil rated or exempt, and non-GST.
// - 3.1(d): the purchases under the reverse charge (gstr/reverse_charge.rs), and the
//   GST owed on them.
// - 4(A): the input tax credit, (2) on services imported under the reverse charge, (3)
//   on other reverse charge purchases, and (5) charged by suppliers on other purchases.
// - 5: the purchases that are exempt, nil rated or non-GST.
// - 6.1: the reverse charge, which is paid in cash; the credit can't pay it.
//
// Rows with no GST tax template are left out of the tables, and shown under them so they
// can be put right. 3.1(b), zero rated exports, isn't told apart from 3.1(a), and Table 4
// takes no credit back (4(B)).

use std::collections::HashMap;

use jiff::Zoned;
use jiff::civil::Date;
use latte_db::pesa::Money;

use super::Gstr;
use super::export::{RowKind, row_kind};
use super::period::{Filing, Period};
use super::reverse_charge;
use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::format::Formatter;
use crate::fyo::model::doc::Doc;
use crate::models::base_models::invoice::{exchange_rate, tax_base};
use crate::models::helpers::text;
use crate::reports::{
    Align, Column, Filter, FilterKind, FilterValue, RawValue, Report, ReportCell, ReportRow,
};

/// A table row's value and taxes, in rupees.
#[derive(Debug, Clone)]
struct Sums {
    value: Money,
    igst: Money,
    cgst: Money,
    sgst: Money,
}

impl Sums {
    fn zero(fyo: &Fyo) -> Self {
        let zero = fyo.pesa().zero();
        Self {
            value: zero.clone(),
            igst: zero.clone(),
            cgst: zero.clone(),
            sgst: zero,
        }
    }

    fn add_value(&mut self, value: &Money) -> Result<(), Error> {
        self.value = self.value.add(value)?;
        Ok(())
    }

    fn add_taxes(&mut self, igst: &Money, cgst: &Money, sgst: &Money) -> Result<(), Error> {
        self.igst = self.igst.add(igst)?;
        self.cgst = self.cgst.add(cgst)?;
        self.sgst = self.sgst.add(sgst)?;
        Ok(())
    }

    /// A row's value, and its taxes at the rates.
    fn add_taxed(&mut self, value: &Money, igst: f64, cgst: f64, sgst: f64) -> Result<(), Error> {
        self.add_value(value)?;
        self.add_taxes(
            &value.percent(igst)?,
            &value.percent(cgst)?,
            &value.percent(sgst)?,
        )
    }

    fn add(&mut self, other: &Sums) -> Result<(), Error> {
        self.add_value(&other.value)?;
        self.add_taxes(&other.igst, &other.cgst, &other.sgst)
    }

    fn is_zero(&self) -> bool {
        [&self.value, &self.igst, &self.cgst, &self.sgst]
            .iter()
            .all(|money| money.round(Some(2)).parse::<f64>().unwrap_or(0.0) == 0.0)
    }
}

/// What a table row shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shows {
    /// The value and the taxes.
    All,
    /// The value: supplies with no tax.
    Value,
    /// The taxes: input tax credit and tax to pay.
    Taxes,
}

/// A row of the return's tables.
#[derive(Debug, Clone)]
struct Line {
    table: &'static str,
    nature: &'static str,
    sums: Sums,
    shows: Shows,
    total: bool,
}

/// GSTR-3B for a period, as GSTR-1's: a Financial Year and a month or quarter, or Custom.
#[derive(Debug, Clone, Default)]
pub struct Gstr3b {
    /// The period, kept as GSTR-1 keeps it.
    pub period: Gstr,
}

impl Gstr3b {
    pub fn new() -> Self {
        Self::default()
    }

    /// The day GSTR-3B for the period is due; none for a Custom period or without a GSTIN.
    pub fn due_date(&self, fyo: &Fyo) -> Option<Date> {
        let gstin = Gstr::company_gstin(fyo).ok()?;
        self.period
            .period?
            .gstr3b_due_date(self.period.fiscal_year?, gstin.get(..2)?)
    }

    /// The return's rows for the period.
    fn lines(&self, fyo: &Fyo) -> Result<Vec<Line>, Error> {
        let mut kinds: HashMap<String, Option<RowKind>> = HashMap::new();
        let mut reverse: HashMap<String, bool> = HashMap::new();
        let mut kind_of = |fyo: &Fyo, tax: &str| -> Result<Option<RowKind>, Error> {
            if let Some(kind) = kinds.get(tax) {
                return Ok(*kind);
            }
            let kind = row_kind(fyo, tax)?;
            kinds.insert(tax.to_owned(), kind);
            Ok(kind)
        };

        let zero = Sums::zero(fyo);
        let mut outward = zero.clone();
        let mut outward_nil = zero.clone();
        let mut outward_non_gst = zero.clone();
        let mut sales_untaxed = zero.clone();
        for invoice in self.period.period_invoices_of(fyo, "SalesInvoice", false)? {
            let doc = Doc::read(fyo, "SalesInvoice", &invoice.name)?;
            let rupees = exchange_rate(&doc);
            for item in doc.table("items") {
                let base = tax_base(&doc, fyo, item)?.mul(rupees)?;
                let kind = match text(item, "tax").filter(|tax| !tax.is_empty()) {
                    Some(tax) => kind_of(fyo, tax)?,
                    None => None,
                };
                match kind {
                    Some(
                        kind @ RowKind::Gst {
                            cgst, sgst, igst, ..
                        },
                    ) if kind.rate() != 0.0 => outward.add_taxed(&base, igst, cgst, sgst)?,
                    Some(RowKind::Gst { .. } | RowKind::NilRated | RowKind::Exempt) => {
                        outward_nil.add_value(&base)?;
                    }
                    Some(RowKind::NonGst) => outward_non_gst.add_value(&base)?,
                    None => sales_untaxed.add_value(&base)?,
                }
            }
        }

        let mut inward_reverse = zero.clone();
        let mut credit_import = zero.clone();
        let mut credit_reverse = zero.clone();
        let mut credit_other = zero.clone();
        let mut inward_exempt = zero.clone();
        let mut inward_non_gst = zero.clone();
        let mut purchases_untaxed = zero.clone();
        for invoice in self
            .period
            .period_invoices_of(fyo, "PurchaseInvoice", false)?
        {
            let doc = Doc::read(fyo, "PurchaseInvoice", &invoice.name)?;
            if let Some(charge) = reverse_charge::of_invoice(fyo, &doc)? {
                inward_reverse.add_value(&charge.taxable)?;
                inward_reverse.add_taxes(&charge.igst, &charge.cgst, &charge.sgst)?;
                let credit = if charge.import {
                    &mut credit_import
                } else {
                    &mut credit_reverse
                };
                credit.add_taxes(&charge.igst, &charge.cgst, &charge.sgst)?;
            }
            let rupees = exchange_rate(&doc);
            for item in doc.table("items") {
                let base = tax_base(&doc, fyo, item)?.mul(rupees)?;
                let Some(tax) = text(item, "tax").filter(|tax| !tax.is_empty()) else {
                    purchases_untaxed.add_value(&base)?;
                    continue;
                };
                let is_reverse = match reverse.get(tax) {
                    Some(is_reverse) => *is_reverse,
                    None => {
                        let is_reverse = reverse_charge::is_reverse_charge_tax(fyo, tax)?;
                        reverse.insert(tax.to_owned(), is_reverse);
                        is_reverse
                    }
                };
                if is_reverse {
                    continue;
                }
                match kind_of(fyo, tax)? {
                    Some(
                        kind @ RowKind::Gst {
                            cgst, sgst, igst, ..
                        },
                    ) if kind.rate() != 0.0 => credit_other.add_taxed(&base, igst, cgst, sgst)?,
                    Some(RowKind::Gst { .. } | RowKind::NilRated | RowKind::Exempt) => {
                        inward_exempt.add_value(&base)?;
                    }
                    Some(RowKind::NonGst) => inward_non_gst.add_value(&base)?,
                    None => purchases_untaxed.add_value(&base)?,
                }
            }
        }

        let mut credit = zero.clone();
        for sums in [&credit_import, &credit_reverse, &credit_other] {
            credit.add_taxes(&sums.igst, &sums.cgst, &sums.sgst)?;
        }
        let mut cash = zero.clone();
        cash.add(&inward_reverse)?;

        let line = |table, nature, sums: &Sums, shows| Line {
            table,
            nature,
            sums: sums.clone(),
            shows,
            total: false,
        };
        let mut lines = vec![
            line(
                "3.1(a)",
                "Outward taxable supplies (other than zero rated, nil rated and exempted)",
                &outward,
                Shows::All,
            ),
            line(
                "3.1(c)",
                "Other outward supplies (nil rated, exempted)",
                &outward_nil,
                Shows::Value,
            ),
            line(
                "3.1(d)",
                "Inward supplies (liable to reverse charge)",
                &inward_reverse,
                Shows::All,
            ),
            line(
                "3.1(e)",
                "Non-GST outward supplies",
                &outward_non_gst,
                Shows::Value,
            ),
            line(
                "4(A)(2)",
                "ITC: Import of services",
                &credit_import,
                Shows::Taxes,
            ),
            line(
                "4(A)(3)",
                "ITC: Inward supplies liable to reverse charge (other than import)",
                &credit_reverse,
                Shows::Taxes,
            ),
            line("4(A)(5)", "ITC: All other ITC", &credit_other, Shows::Taxes),
            Line {
                total: true,
                ..line("4(C)", "Net ITC available", &credit, Shows::Taxes)
            },
            line(
                "5",
                "Inward supplies: composition scheme, exempt and nil rated",
                &inward_exempt,
                Shows::Value,
            ),
            line(
                "5",
                "Inward supplies: non-GST",
                &inward_non_gst,
                Shows::Value,
            ),
            Line {
                total: true,
                ..line(
                    "6.1",
                    "Reverse charge, to be paid in cash",
                    &cash,
                    Shows::Taxes,
                )
            },
        ];
        if !sales_untaxed.is_zero() {
            lines.push(line(
                "",
                "Sales rows with no GST tax template, left out above",
                &sales_untaxed,
                Shows::Value,
            ));
        }
        if !purchases_untaxed.is_zero() {
            lines.push(line(
                "",
                "Purchase rows with no GST tax template, left out above",
                &purchases_untaxed,
                Shows::Value,
            ));
        }
        Ok(lines)
    }
}

/// The report's columns: (label, fieldname, align, width).
const COLUMNS: [(&str, &str, Align, f64); 6] = [
    ("Table", "table", Align::Left, 0.6),
    ("Nature of Supplies", "nature", Align::Left, 3.0),
    ("Taxable Value", "value", Align::Right, 1.0),
    ("Integrated Tax", "igst", Align::Right, 1.0),
    ("Central Tax", "cgst", Align::Right, 1.0),
    ("State/UT Tax", "sgst", Align::Right, 1.0),
];

impl Report for Gstr3b {
    fn title(&self) -> &'static str {
        "GSTR3B"
    }

    fn report_name(&self) -> &'static str {
        "gstr-3b"
    }

    /// GSTR-1's period filters.
    fn filters(&self, fyo: &Fyo) -> Vec<Filter> {
        let today = Zoned::now().date();
        let mut filters = vec![
            Filter::new(
                "fiscalYear",
                "Financial Year",
                FilterKind::Select(self.period.fiscal_years(today)),
            ),
            Filter::new(
                "period",
                "Period",
                FilterKind::Select(self.period.periods(Filing::of(fyo), today)),
            ),
        ];
        if !matches!(
            self.period.period,
            Some(Period::Month(_) | Period::Quarter(_))
        ) {
            filters.extend([
                Filter::new("fromDate", "From Date", FilterKind::Date),
                Filter::new("toDate", "To Date", FilterKind::Date),
            ]);
        }
        filters
    }

    fn get(&self, fieldname: &str) -> FilterValue {
        match fieldname {
            "fiscalYear" | "period" | "fromDate" | "toDate" => self.period.get(fieldname),
            _ => FilterValue::Null,
        }
    }

    fn set(&mut self, fieldname: &str, value: FilterValue) {
        if matches!(fieldname, "fiscalYear" | "period" | "fromDate" | "toDate") {
            self.period.set(fieldname, value);
        }
    }

    fn columns(&self, _fyo: &Fyo, _formatter: &Formatter) -> Result<Vec<Column>, Error> {
        Ok(COLUMNS
            .iter()
            .map(|(label, fieldname, align, width)| Column {
                label: (*label).to_owned(),
                fieldname: (*fieldname).to_owned(),
                align: *align,
                width: *width,
            })
            .collect())
    }

    fn run(&mut self, fyo: &Fyo, formatter: &Formatter) -> Result<Vec<ReportRow>, Error> {
        self.period
            .set_default_filters(Filing::of(fyo), Zoned::now().date());
        let lines = self.lines(fyo)?;
        Ok(lines
            .iter()
            .map(|line| {
                let amount = |money: &Money, shown: bool| {
                    if shown {
                        (formatter.currency(money), RawValue::Number(money.float()))
                    } else {
                        (String::new(), RawValue::Empty)
                    }
                };
                let value = line.shows != Shows::Taxes;
                let taxes = line.shows != Shows::Value;
                let cells = [
                    (line.table.to_owned(), RawValue::Text(line.table.to_owned())),
                    (
                        line.nature.to_owned(),
                        RawValue::Text(line.nature.to_owned()),
                    ),
                    amount(&line.sums.value, value),
                    amount(&line.sums.igst, taxes),
                    amount(&line.sums.cgst, taxes),
                    amount(&line.sums.sgst, taxes),
                ];
                ReportRow {
                    cells: cells
                        .into_iter()
                        .zip(COLUMNS)
                        .map(|((value, raw_value), (_, _, align, width))| ReportCell {
                            value,
                            raw_value,
                            bold: line.total,
                            align,
                            width,
                            ..ReportCell::default()
                        })
                        .collect(),
                    ..ReportRow::default()
                }
            })
            .collect())
    }

    /// When GSTR-3B for the period is due, or was.
    fn note(&self, fyo: &Fyo, formatter: &Formatter) -> Option<String> {
        let due = self.due_date(fyo)?;
        let shown = formatter.date(due);
        Some(if Zoned::now().date() > due {
            format!("GSTR-3B was due on {shown}")
        } else {
            format!("GSTR-3B is due on {shown}")
        })
    }

    /// GSTR3B_<GSTIN>_<MMYYYY>, for the CSV, as GSTR-1's export is named.
    fn file_name(&self, fyo: &Fyo) -> Option<String> {
        let gstin = Gstr::company_gstin(fyo).ok()?;
        let period = match self.period.period? {
            Period::Custom => format!("{}_{}", self.period.from_date?, self.period.to_date?),
            _ => self.period.to_date?.strftime("%m%Y").to_string(),
        };
        Some(format!("GSTR3B_{gstin}_{period}"))
    }
}
