// Port of reports/GoodsAndServiceTax/ @ v0.36.0: BaseGSTR.ts, GSTR1.ts, GSTR2.ts and the
// JSON export of gstExporter.ts. GSTR-1 lists the sales invoices of a period and GSTR-2
// the purchase invoices, with each one's GST.
//
// Books bugs Latte fixes:
// - The rate of an invoice is its taxes' rates added up once. Books adds them twice, so
//   an invoice with 18% GST shows a rate of 36, is never nil rated unless it has no GST,
//   and its B2C-Large and B2C-Small export has twice the integrated tax.
// - Each tax account sets only its own amount. Books' `switch` has no `break`s, so an
//   IGST account also sets the central and state tax, a CGST one also the state tax, and
//   every GST account marks the invoice nil rated, exempt and non-GST.
//
// Deviation: the exports are what the app calls (`can_export` and `export_json` of
// `Report`), not actions of the report.

use jiff::civil::Date;
use jiff::{Timestamp, ToSpan, Zoned};
use latte_db::{
    DocValue, DocValueMap, Filter as DbFilter, FilterOperand, FilterValue as DbFilterValue,
    GetAllOptions, QueryFilter,
};
use serde_json::{Map, Value, json};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::format::{Formatter, local_date};
use crate::fyo::model::helpers::js_number_string;
use crate::models::base_models::address::CODE_STATE_MAP;
use crate::reports::stock_ledger::{date_filter, text_filter};
use crate::reports::{
    Align, Column, Filter, FilterKind, FilterValue, RawValue, Report, ReportCell, ReportRow,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GstrType {
    /// Sales.
    #[default]
    Gstr1,
    /// Purchases.
    Gstr2,
}

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

/// `BaseGSTR`, with GSTR1 and GSTR2 as its two types.
#[derive(Debug, Clone, Default)]
pub struct Gstr {
    pub gstr_type: GstrType,
    /// A GST state code.
    pub place: Option<String>,
    pub from_date: Option<Date>,
    pub to_date: Option<Date>,
    /// B2B, B2CL, B2CS or NR.
    pub transfer_type: Option<String>,
    /// `gstrRows`: the rows of the last run.
    pub rows: Vec<GstrRow>,
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

impl Gstr {
    pub fn new(gstr_type: GstrType) -> Self {
        Self {
            gstr_type,
            ..Self::default()
        }
    }

    /// `transferTypeMap`, as `(value, label)`.
    pub fn transfer_types(&self) -> Vec<(&'static str, &'static str)> {
        if self.gstr_type == GstrType::Gstr2 {
            return vec![("B2B", "B2B")];
        }
        vec![
            ("B2B", "B2B"),
            ("B2CL", "B2C-Large"),
            ("B2CS", "B2C-Small"),
            ("NR", "Nil Rated, Exempted and Non GST supplies"),
        ]
    }

    /// `schemaName`.
    pub fn schema_name(&self) -> &'static str {
        match self.gstr_type {
            GstrType::Gstr1 => "SalesInvoice",
            GstrType::Gstr2 => "PurchaseInvoice",
        }
    }

    fn transfer_type(&self) -> &str {
        self.transfer_type.as_deref().unwrap_or("B2B")
    }

    /// `setDefaultFilters`: the last three months, B2B.
    pub fn set_default_filters(&mut self, today: Date) {
        if self.to_date.is_none() {
            self.to_date = Some(today);
        }
        if self.from_date.is_none() {
            self.from_date = today.checked_sub(3.months()).ok();
        }
        if self.transfer_type.is_none() {
            self.transfer_type = Some("B2B".to_owned());
        }
    }

    /// `filterGstrRows` and `transferFilterFunction`.
    fn allows(&self, row: &GstrRow) -> bool {
        if let Some(place) = self.place.as_deref()
            && state(place) != Some(row.place.as_str())
        {
            return false;
        }
        match self.transfer_type.as_deref() {
            Some("B2B") => !row.gstin.is_empty(),
            Some("B2CL") => row.gstin.is_empty() && !row.in_state && row.inv_amt >= 250000.0,
            Some("B2CS") => row.gstin.is_empty() && (row.in_state || row.inv_amt < 250000.0),
            Some("NR") => row.rate == 0.0,
            _ => true,
        }
    }

    /// `getEntries`: the submitted invoices of the period.
    fn entries(&self, fyo: &Fyo) -> Result<Vec<String>, Error> {
        let to = self.to_date.map(|d| DbFilterValue::Text(d.to_string()));
        let from = self.from_date.map(|d| DbFilterValue::Text(d.to_string()));
        let mut filters = QueryFilter::from([
            ("submitted".to_owned(), DbFilter::from(true)),
            ("cancelled".to_owned(), DbFilter::from(false)),
        ]);
        let date = match (to, from) {
            (Some(to), Some(from)) => Some(DbFilter::Two(
                "<=".to_owned(),
                FilterOperand::One(to),
                ">=".to_owned(),
                FilterOperand::One(from),
            )),
            (Some(to), None) => Some(DbFilter::op("<=", to)),
            (None, Some(from)) => Some(DbFilter::op(">=", from)),
            (None, None) => None,
        };
        if let Some(date) = date {
            filters.insert("date".to_owned(), date);
        }
        let options = GetAllOptions {
            filters,
            ..GetAllOptions::default()
        };
        Ok(fyo
            .db
            .get_all_docs(self.schema_name(), &options)?
            .iter()
            .map(|row| text_value(row, "name"))
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
        let party = fyo.db.get_doc("Party", &party_name)?;
        let party_gstin = text_value(&party, "gstin");

        let address = text_value(&party, "address");
        let place = if !address.is_empty() {
            match fyo.get_value("Address", Some(&address), "pos") {
                Some(DocValue::Text(pos)) => pos,
                _ => String::new(),
            }
        } else if !party_gstin.is_empty() {
            party_gstin
                .get(..2)
                .and_then(state)
                .unwrap_or_default()
                .to_owned()
        } else {
            String::new()
        };

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
            column("Intergrated Tax", "igstAmt", Kind::Currency, 1.0),
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

    /// `getCanExport`: the export needs the company's GSTIN.
    pub fn company_gstin(fyo: &Fyo) -> Result<String, Error> {
        match fyo.get_value("AccountingSettings", None, "gstin") {
            Some(DocValue::Text(gstin)) if !gstin.is_empty() => Ok(gstin),
            _ => Err(Error::Validation(
                "Cannot Export: Please set GSTIN in General Settings.".to_owned(),
            )),
        }
    }

    /// `getGstrJsonData`: the last run's rows as the GST portal's JSON.
    pub fn gstr_json(&self, fyo: &Fyo) -> Result<String, Error> {
        let gstin = Self::company_gstin(fyo)?;
        let fp = self
            .to_date
            .map(|date| date.strftime("%m%Y").to_string())
            .unwrap_or_default();
        let mut data = Map::new();
        data.insert("version".into(), json!("GST3.0.4"));
        data.insert("hash".into(), json!("hash"));
        data.insert("gstin".into(), json!(gstin));
        data.insert("fp".into(), json!(fp));
        match self.transfer_type() {
            "B2B" => {
                data.insert("b2b".into(), self.b2b_data(fyo)?);
            }
            "B2CL" => {
                data.insert("b2cl".into(), self.b2cl_data(fyo)?);
            }
            "B2CS" => {
                data.insert("b2cs".into(), self.b2cs_data());
            }
            _ => {}
        }
        Ok(Value::Object(data).to_string())
    }

    /// The rows' items as `(hsnCode, tax, amount in the company's currency)`.
    fn items(&self, fyo: &Fyo, invoice: &str) -> Result<Vec<(Value, String, f64)>, Error> {
        let item_schema = match self.gstr_type {
            GstrType::Gstr1 => "SalesInvoiceItem",
            GstrType::Gstr2 => "PurchaseInvoiceItem",
        };
        let exchange_rate = match fyo.get_value(self.schema_name(), Some(invoice), "exchangeRate") {
            Some(DocValue::Number(rate)) => rate,
            _ => 1.0,
        };
        let options = GetAllOptions {
            fields: Some(vec!["amount".into(), "tax".into(), "hsnCode".into()]),
            filters: QueryFilter::from([("parent".to_owned(), DbFilter::from(invoice))]),
            ..GetAllOptions::default()
        };
        let mut items = Vec::new();
        for item in fyo.db.get_all_docs(item_schema, &options)? {
            let amount = match item.get("amount") {
                Some(DocValue::Money(amount)) => amount.mul(exchange_rate)?.float(),
                _ => 0.0,
            };
            let hsn_code = number_value(&item, "hsnCode").map_or(Value::Null, num);
            items.push((hsn_code, text_value(&item, "tax"), amount));
        }
        Ok(items)
    }

    /// `generateB2bData`: the invoices by the party's GSTIN.
    fn b2b_data(&self, fyo: &Fyo) -> Result<Value, Error> {
        let mut customers: Vec<(String, Vec<Value>)> = Vec::new();
        for row in &self.rows {
            let mut itms = Vec::new();
            for (hsn_code, tax, amount) in self.items(fyo, &row.inv_no)? {
                let part = |rate: Option<f64>| (rate.unwrap_or(0.0) * amount) / 100.0;
                let mut itm_det = Map::new();
                itm_det.insert("txval".into(), num(amount));
                if let Some(rate) = gst_rate(&tax) {
                    itm_det.insert("rt".into(), num(rate));
                }
                itm_det.insert("csamt".into(), num(0.0));
                itm_det.insert("camt".into(), num(part(csgst_rate(&tax))));
                itm_det.insert("samt".into(), num(part(csgst_rate(&tax))));
                itm_det.insert("iamt".into(), num(part(igst_rate(&tax))));
                itms.push(json!({ "num": hsn_code, "itm_det": itm_det }));
            }
            let inv = json!({
                "inum": row.inv_no,
                "idt": invoice_date(row),
                "val": num(row.inv_amt),
                "pos": row.gstin.get(..2).unwrap_or_default(),
                "rchrg": row.reverse_charge,
                "inv_typ": "R",
                "itms": itms,
            });
            match customers.iter_mut().find(|(ctin, _)| *ctin == row.gstin) {
                Some((_, invs)) => invs.push(inv),
                None => customers.push((row.gstin.clone(), vec![inv])),
            }
        }
        Ok(Value::Array(
            customers
                .into_iter()
                .map(|(ctin, inv)| json!({ "ctin": ctin, "inv": inv }))
                .collect(),
        ))
    }

    /// `generateB2clData`: the invoices by the state supplied to.
    fn b2cl_data(&self, fyo: &Fyo) -> Result<Value, Error> {
        let mut states: Vec<(Option<&str>, Vec<Value>)> = Vec::new();
        for row in &self.rows {
            let mut itms = Vec::new();
            for (hsn_code, tax, amount) in self.items(fyo, &row.inv_no)? {
                itms.push(json!({
                    "num": hsn_code,
                    "itm_det": {
                        "txval": num(amount),
                        "rt": num(gst_rate(&tax).unwrap_or(0.0)),
                        "csamt": 0,
                        "iamt": num(row.rate * amount / 100.0),
                    },
                }));
            }
            let inv = json!({
                "inum": row.inv_no,
                "idt": invoice_date(row),
                "val": num(row.inv_amt),
                "itms": itms,
            });
            let pos = state_code(&row.place);
            match states.iter_mut().find(|(p, _)| *p == pos) {
                Some((_, invs)) => invs.push(inv),
                None => states.push((pos, vec![inv])),
            }
        }
        Ok(Value::Array(
            states
                .into_iter()
                .map(|(pos, inv)| {
                    let mut record = Map::new();
                    if let Some(pos) = pos {
                        record.insert("pos".into(), json!(pos));
                    }
                    record.insert("inv".into(), Value::Array(inv));
                    Value::Object(record)
                })
                .collect(),
        ))
    }

    /// `generateB2csData`: a record per invoice.
    fn b2cs_data(&self) -> Value {
        Value::Array(
            self.rows
                .iter()
                .map(|row| {
                    let mut record = Map::new();
                    let supply = if row.in_state { "INTRA" } else { "INTER" };
                    record.insert("sply_ty".into(), json!(supply));
                    if let Some(pos) = state_code(&row.place) {
                        record.insert("pos".into(), json!(pos));
                    }
                    record.insert("typ".into(), json!("OE"));
                    record.insert("txval".into(), num(row.tax_val));
                    record.insert("rt".into(), num(row.rate));
                    let iamt = if row.in_state {
                        0.0
                    } else {
                        row.tax_val * row.rate / 100.0
                    };
                    let local = |amount: Option<f64>| {
                        if row.in_state {
                            amount.unwrap_or(0.0)
                        } else {
                            0.0
                        }
                    };
                    record.insert("iamt".into(), num(iamt));
                    record.insert("camt".into(), num(local(row.cgst_amt)));
                    record.insert("samt".into(), num(local(row.sgst_amt)));
                    record.insert("csamt".into(), num(0.0));
                    Value::Object(record)
                })
                .collect(),
        )
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

/// The invoice date as dd-MM-yyyy.
fn invoice_date(row: &GstrRow) -> String {
    row.inv_date
        .map(|date| local_date(date).strftime("%d-%m-%Y").to_string())
        .unwrap_or_default()
}

/// A JSON number as JavaScript writes it: whole numbers without a fraction.
fn num(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() < 9e15 {
        json!(value as i64)
    } else {
        json!(value)
    }
}

/// `GST`: a tax's whole rate.
fn gst_rate(tax: &str) -> Option<f64> {
    let rate = tax
        .strip_prefix("IGST-")
        .or_else(|| tax.strip_prefix("GST-"))?;
    is_gst_rate(rate).then(|| rate.parse().ok()).flatten()
}

/// `CSGST`: the central (and the state) part of a GST rate.
fn csgst_rate(tax: &str) -> Option<f64> {
    let rate = tax.strip_prefix("GST-")?;
    is_gst_rate(rate)
        .then(|| rate.parse::<f64>().ok().map(|rate| rate / 2.0))
        .flatten()
}

/// `IGST`: an IGST rate. Books' map has no IGST-0, which comes to the same.
fn igst_rate(tax: &str) -> Option<f64> {
    let rate = tax.strip_prefix("IGST-")?;
    (is_gst_rate(rate) && rate != "0")
        .then(|| rate.parse().ok())
        .flatten()
}

fn is_gst_rate(rate: &str) -> bool {
    matches!(rate, "0" | "0.25" | "3" | "5" | "6" | "12" | "18" | "28")
}

impl Report for Gstr {
    fn title(&self) -> &'static str {
        match self.gstr_type {
            GstrType::Gstr1 => "GSTR1",
            GstrType::Gstr2 => "GSTR2",
        }
    }

    fn report_name(&self) -> &'static str {
        match self.gstr_type {
            GstrType::Gstr1 => "gstr-1",
            GstrType::Gstr2 => "gstr-2",
        }
    }

    fn filters(&self, _fyo: &Fyo) -> Vec<Filter> {
        let transfer_types = self
            .transfer_types()
            .into_iter()
            .map(|(value, label)| (label.to_owned(), value.to_owned()))
            .collect();
        let places = CODE_STATE_MAP
            .iter()
            .map(|(code, state)| ((*state).to_owned(), (*code).to_owned()))
            .collect();
        vec![
            Filter::new(
                "transferType",
                "Transfer Type",
                FilterKind::Select(transfer_types),
            ),
            Filter::new("place", "Place", FilterKind::Select(places)),
            Filter::new("fromDate", "From Date", FilterKind::Date),
            Filter::new("toDate", "To Date", FilterKind::Date),
        ]
    }

    fn get(&self, fieldname: &str) -> FilterValue {
        match fieldname {
            "transferType" => FilterValue::text(self.transfer_type.as_deref()),
            "place" => FilterValue::text(self.place.as_deref()),
            "fromDate" => FilterValue::date(self.from_date),
            "toDate" => FilterValue::date(self.to_date),
            _ => FilterValue::Null,
        }
    }

    fn set(&mut self, fieldname: &str, value: FilterValue) {
        match fieldname {
            "transferType" => self.transfer_type = text_filter(&value),
            "place" => self.place = text_filter(&value),
            "fromDate" => self.from_date = date_filter(&value),
            "toDate" => self.to_date = date_filter(&value),
            _ => {}
        }
    }

    fn columns(&self, _fyo: &Fyo, _formatter: &Formatter) -> Result<Vec<Column>, Error> {
        Ok(self.kinded_columns().into_iter().map(|(c, _)| c).collect())
    }

    fn run(&mut self, fyo: &Fyo, formatter: &Formatter) -> Result<Vec<ReportRow>, Error> {
        self.set_default_filters(Zoned::now().date());
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
}
