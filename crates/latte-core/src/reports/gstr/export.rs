// Port of the JSON export of reports/GoodsAndServiceTax/gstExporter.ts @ v0.36.0
// (`getGstrJsonData` and the `generate*Data` functions), written as the GST portal's
// Returns Offline Tool V3.2.4 reads it.
//
// Books bugs Latte fixes:
// - An item row's `num` is its serial number on the invoice. Books puts the HSN code
//   there.
// - An invoice has one item row per rate, as the portal wants. Books has one per invoice
//   row, so two rows at 18% are two items at 18%.
// - A B2B invoice's place of supply is the one the report shows: the customer address's,
//   else its GSTIN's state. Books always takes the GSTIN's state.
// - B2C-Small has one row per place, rate and type of supply. Books has one per invoice,
//   at the invoice's total rate.
// - A return is a credit note: CDNR for a registered customer, CDNUR for a B2C-Large
//   sale, else taken off B2C-Small. Books lists returns as invoices of negative value.
// - An invoice's value is in rupees, as its taxable values are. Books gives a foreign
//   currency invoice's value in that currency.
// - A row's rate and taxes come from its tax's accounts (CGST, SGST, IGST), so a tax not
//   named like GST-18 is taxed too. Books reads the rate from the tax's name.
//
// Deviations:
// - GSTR-1's export is the period's whole return, whatever transfer type or place the
//   report shows: B2B, B2C-Large, B2C-Small, credit notes (CDNR, CDNUR), nil rated,
//   exempt and non-GST supplies (Table 8), the HSN summary (Table 12, split into B2B and
//   B2C) and the documents issued (Table 13). Tables 12 and 13 cover every supply, so the
//   return is one file. Books exports the chosen transfer type only, with no Tables 8, 9B,
//   12 or 13.
// - The version is GST3.2.4, the offline tool's current one. Books writes GST3.0.4.
// - A row has the taxes that apply only: central and state tax within the state,
//   integrated tax between states. Books writes all three.
// - On GSTR-1, every row needs a GST tax, and an HSN/SAC code (the row's, else its
//   item's); the export stops and says which row hasn't.
// - The HSN summary's description is the items' names; there is no HSN master to take
//   the official one from. A service (SAC 99…) has the unit NA and quantity 0, as the
//   portal asks; a good's unit is its GST unit (UQC), or OTH.

use std::collections::{BTreeMap, HashMap};

use indexmap::IndexMap;
use jiff::civil::Date;
use latte_db::DocValue;
use latte_db::pesa::Money;
use serde_json::{Map, Value, json};

use super::{Gstr, PeriodInvoice, b2cl_limit, party_place, shown_date, state_code};
use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::models::base_models::invoice::{exchange_rate, tax_base};
use crate::models::helpers::{money, number, text};

/// The version of the Returns Offline Tool the JSON is for.
const VERSION: &str = "GST3.2.4";

/// Table 13's document types.
const INVOICES: u8 = 1;
const CREDIT_NOTES: u8 = 5;

/// The GST units of quantity (UQC) the portal takes.
const UQCS: [&str; 45] = [
    "BAG", "BAL", "BDL", "BKL", "BOU", "BOX", "BTL", "BUN", "CAN", "CBM", "CCM", "CMS", "CTN",
    "DOZ", "DRM", "GGK", "GMS", "GRS", "GYD", "KGS", "KLR", "KME", "LTR", "MLT", "MTR", "MTS",
    "NOS", "OTH", "PAC", "PCS", "PRS", "QTL", "ROL", "SET", "SQF", "SQM", "SQY", "TBS", "TGM",
    "THD", "TON", "TUB", "UGS", "UNT", "YDS",
];

/// How a row is taxed, from its tax's accounts.
#[derive(Clone, Copy)]
pub(super) enum RowKind {
    /// GST at a rate, as central and state tax, or as integrated tax. At 0% it is nil
    /// rated.
    Gst {
        cgst: f64,
        sgst: f64,
        igst: f64,
        integrated: bool,
    },
    NilRated,
    Exempt,
    NonGst,
}

impl RowKind {
    pub(super) fn rate(self) -> f64 {
        match self {
            Self::Gst {
                cgst, sgst, igst, ..
            } => cgst + sgst + igst,
            _ => 0.0,
        }
    }
}

/// A rate's or an HSN row's taxable value and taxes, in rupees.
#[derive(Clone)]
struct Taxes {
    txval: Money,
    iamt: Money,
    camt: Money,
    samt: Money,
}

impl Taxes {
    fn new(fyo: &Fyo) -> Self {
        let zero = fyo.pesa().zero();
        Self {
            txval: zero.clone(),
            iamt: zero.clone(),
            camt: zero.clone(),
            samt: zero,
        }
    }

    fn add(&mut self, other: &Taxes) -> Result<(), Error> {
        self.txval = self.txval.add(&other.txval)?;
        self.iamt = self.iamt.add(&other.iamt)?;
        self.camt = self.camt.add(&other.camt)?;
        self.samt = self.samt.add(&other.samt)?;
        Ok(())
    }

    fn is_zero(&self) -> bool {
        [&self.txval, &self.iamt, &self.camt, &self.samt]
            .iter()
            .all(|m| m.round(Some(2)).parse::<f64>().unwrap_or(0.0) == 0.0)
    }
}

/// An invoice row for the HSN summary.
struct Line {
    hsn: String,
    item: String,
    uqc: &'static str,
    qty: f64,
    rate: f64,
    taxes: Taxes,
}

/// A submitted invoice, or a return, as the return reports it.
struct Supply {
    name: String,
    date: Date,
    /// The party's GSTIN; empty for an unregistered one.
    gstin: String,
    /// The place of supply's state code.
    pos: String,
    inter: bool,
    /// The invoice value in rupees; a return's is negative.
    value: Money,
    is_return: bool,
    /// A B2C-Large sale, or a return of one.
    large: bool,
    /// The taxed amounts by rate, in rate order.
    rates: Vec<(f64, Taxes)>,
    nil_rated: Money,
    exempt: Money,
    non_gst: Money,
    lines: Vec<Line>,
}

impl Supply {
    fn registered(&self) -> bool {
        !self.gstin.is_empty()
    }
}

fn cannot_export(message: String) -> Error {
    Error::Validation(format!("Cannot Export: {message}"))
}

/// A JSON number as JavaScript writes it: whole numbers without a fraction.
fn num(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() < 9e15 {
        json!(value as i64)
    } else {
        json!(value)
    }
}

/// An amount in rupees and paise; `negate` gives a return's as its size.
fn amount(money: &Money, negate: bool) -> Result<Value, Error> {
    let money = if negate { money.neg()? } else { money.clone() };
    Ok(num(money.round(Some(2)).parse().unwrap_or(0.0)))
}

/// A rate as a key: rates such as 0.25 don't compare well as floats.
fn rate_key(rate: f64) -> i64 {
    (rate * 100.0).round() as i64
}

fn gst_date(date: Date) -> String {
    date.strftime("%d-%m-%Y").to_string()
}

/// An HSN/SAC code as text. An invoice row stores it as a number, which loses the
/// leading zero of chapters 01 to 09, so a code of odd length gets it back.
fn hsn_code(value: Option<&DocValue>) -> Option<String> {
    let code = match value? {
        DocValue::Number(number) if *number > 0.0 => format!("{}", number.trunc() as i64),
        DocValue::Text(text) if !text.trim().is_empty() => text.trim().to_owned(),
        _ => return None,
    };
    Some(if code.len() % 2 == 1 {
        format!("0{code}")
    } else {
        code
    })
}

/// A unit's GST unit of quantity.
fn uqc(unit: &str) -> &'static str {
    let unit = unit.trim();
    if let Some(code) = UQCS.iter().find(|code| code.eq_ignore_ascii_case(unit)) {
        return code;
    }
    match unit.to_ascii_lowercase().as_str() {
        "unit" | "units" => "UNT",
        "no" | "number" | "numbers" => "NOS",
        "piece" | "pieces" | "pc" => "PCS",
        "kg" | "kilogram" | "kilograms" => "KGS",
        "g" | "gm" | "gram" | "grams" => "GMS",
        "m" | "meter" | "meters" | "metre" | "metres" => "MTR",
        "l" | "litre" | "litres" | "liter" | "liters" => "LTR",
        "ml" | "millilitre" | "millilitres" | "milliliter" | "milliliters" => "MLT",
        "dozen" | "dozens" => "DOZ",
        "boxes" => "BOX",
        "bags" => "BAG",
        "bottle" | "bottles" => "BTL",
        "sets" => "SET",
        "pair" | "pairs" => "PRS",
        "tonne" | "tonnes" => "TON",
        "quintal" | "quintals" => "QTL",
        "pack" | "packs" | "packet" | "packets" => "PAC",
        "roll" | "rolls" => "ROL",
        "carton" | "cartons" => "CTN",
        "cans" => "CAN",
        _ => "OTH",
    }
}

/// How the tax `name` taxes a row, or `None` if it has no GST account.
pub(super) fn row_kind(fyo: &Fyo, name: &str) -> Result<Option<RowKind>, Error> {
    let tax = fyo.db.get_doc("Tax", name)?;
    let Some(DocValue::Table(details)) = tax.get("details") else {
        return Ok(None);
    };
    let (mut cgst, mut sgst, mut igst) = (0.0, 0.0, 0.0);
    let mut integrated = false;
    let mut kind = None;
    for detail in details {
        let rate = match detail.get("rate") {
            Some(DocValue::Number(rate)) => *rate,
            _ => 0.0,
        };
        match detail.get("account") {
            Some(DocValue::Text(account)) => match account.as_str() {
                "CGST" => cgst += rate,
                "SGST" => sgst += rate,
                "IGST" => {
                    igst += rate;
                    integrated = true;
                }
                "Nil Rated" => kind = Some(RowKind::NilRated),
                "Exempt" => kind = Some(RowKind::Exempt),
                "Non GST" => kind = Some(RowKind::NonGst),
                _ => continue,
            },
            _ => continue,
        }
        if kind.is_none() {
            kind = Some(RowKind::Gst {
                cgst: 0.0,
                sgst: 0.0,
                igst: 0.0,
                integrated: false,
            });
        }
    }
    Ok(kind.map(|kind| match kind {
        RowKind::Gst { .. } => RowKind::Gst {
            cgst,
            sgst,
            igst,
            integrated,
        },
        kind => kind,
    }))
}

/// The return's view of a submitted invoice.
fn supply(
    fyo: &Fyo,
    gstr: &Gstr,
    invoice: &PeriodInvoice,
    company_state: &str,
    kinds: &mut HashMap<String, Option<RowKind>>,
) -> Result<Supply, Error> {
    let schema = gstr.schema_name();
    let doc = Doc::read(fyo, schema, &invoice.name)?;
    let (gstin, place) = party_place(fyo, text(&doc, "party").unwrap_or_default())?;
    let rupees = exchange_rate(&doc);
    let zero = fyo.pesa().zero();
    let mut rates: Vec<(f64, Taxes)> = Vec::new();
    let (mut nil_rated, mut exempt, mut non_gst) = (zero.clone(), zero.clone(), zero.clone());
    let mut lines = Vec::new();
    let (mut integrated, mut local) = (false, false);

    for (idx, item) in doc.table("items").iter().enumerate() {
        let item_name = text(item, "item").unwrap_or_default();
        let row = format!("row {} of {} ({item_name})", idx + 1, invoice.name);
        let tax = text(item, "tax").filter(|tax| !tax.is_empty());
        let kind = match tax {
            Some(tax) => match kinds.get(tax) {
                Some(kind) => *kind,
                None => {
                    let kind = row_kind(fyo, tax)?;
                    kinds.insert(tax.to_owned(), kind);
                    kind
                }
            },
            None => None,
        };
        let Some(kind) = kind else {
            return Err(cannot_export(match tax {
                Some(tax) => format!("the tax {tax} on {row} has no GST account."),
                None => format!(
                    "{row} has no tax, so it can't be filed as taxed, nil rated, exempt or \
                     non-GST."
                ),
            }));
        };

        let base = tax_base(&doc, fyo, item)?.mul(rupees)?;
        let mut taxes = Taxes::new(fyo);
        taxes.txval = base.clone();
        match kind {
            RowKind::Gst {
                cgst,
                sgst,
                igst,
                integrated: is_integrated,
            } => {
                if is_integrated {
                    integrated = true;
                } else {
                    local = true;
                }
                let rate = kind.rate();
                if rate == 0.0 {
                    nil_rated = nil_rated.add(&base)?;
                } else {
                    taxes.camt = base.percent(cgst)?;
                    taxes.samt = base.percent(sgst)?;
                    taxes.iamt = base.percent(igst)?;
                    match rates
                        .iter_mut()
                        .find(|(r, _)| rate_key(*r) == rate_key(rate))
                    {
                        Some((_, total)) => total.add(&taxes)?,
                        None => rates.push((rate, taxes.clone())),
                    }
                }
            }
            RowKind::NilRated => nil_rated = nil_rated.add(&base)?,
            RowKind::Exempt => exempt = exempt.add(&base)?,
            RowKind::NonGst => non_gst = non_gst.add(&base)?,
        }

        {
            // The row's code is a number; the item's is text, with its leading zero.
            let item_hsn = hsn_code(fyo.get_value("Item", Some(item_name), "hsnCode").as_ref());
            let hsn = match (hsn_code(item.get("hsnCode")), item_hsn) {
                (Some(row), Some(item))
                    if row.trim_start_matches('0') == item.trim_start_matches('0') =>
                {
                    Some(item)
                }
                (Some(row), _) => Some(row),
                (None, item) => item,
            };
            let hsn = hsn.ok_or_else(|| {
                cannot_export(format!(
                    "{item_name} on {} has no HSN/SAC code. Set it on the item.",
                    invoice.name
                ))
            })?;
            let (uqc, qty) = if hsn.starts_with("99") {
                ("NA", 0.0)
            } else {
                (
                    uqc(text(item, "unit").unwrap_or_default()),
                    number(item, "quantity").unwrap_or(0.0),
                )
            };
            lines.push(Line {
                hsn,
                item: item_name.to_owned(),
                uqc,
                qty,
                rate: kind.rate(),
                taxes,
            });
        }
    }
    rates.sort_by_key(|(rate, _)| rate_key(*rate));

    let pos = state_code(&place).unwrap_or(company_state).to_owned();
    let inter = integrated || (!local && pos != company_state);
    let value = match money(&doc, "baseGrandTotal") {
        Some(value) => value,
        None => money(&doc, "grandTotal").unwrap_or(zero).mul(rupees)?,
    };
    let large = gstin.is_empty() && inter && {
        if invoice.is_return {
            let against = text(&doc, "returnAgainst").unwrap_or_default();
            let value = match fyo.get_value(schema, Some(against), "baseGrandTotal") {
                Some(DocValue::Money(value)) => value.float().abs(),
                _ => 0.0,
            };
            let date = shown_date(fyo.get_value(schema, Some(against), "date").as_ref());
            value > b2cl_limit(date)
        } else {
            value.float() > b2cl_limit(Some(invoice.date))
        }
    };
    Ok(Supply {
        name: invoice.name.clone(),
        date: invoice.date,
        gstin,
        pos,
        inter,
        value,
        is_return: invoice.is_return,
        large,
        rates,
        nil_rated,
        exempt,
        non_gst,
        lines,
    })
}

/// An item row's details: the rate, the taxable value and the taxes that apply.
fn itm_det(rate: f64, taxes: &Taxes, inter: bool, negate: bool) -> Result<Value, Error> {
    let mut det = Map::new();
    det.insert("rt".into(), num(rate));
    det.insert("txval".into(), amount(&taxes.txval, negate)?);
    if inter {
        det.insert("iamt".into(), amount(&taxes.iamt, negate)?);
    } else {
        det.insert("camt".into(), amount(&taxes.camt, negate)?);
        det.insert("samt".into(), amount(&taxes.samt, negate)?);
    }
    det.insert("csamt".into(), num(0.0));
    Ok(Value::Object(det))
}

/// An invoice's or note's item rows, one per rate.
fn itms(supply: &Supply) -> Result<Value, Error> {
    let mut itms = Vec::new();
    for (idx, (rate, taxes)) in supply.rates.iter().enumerate() {
        itms.push(json!({
            "num": idx + 1,
            "itm_det": itm_det(*rate, taxes, supply.inter, supply.is_return)?,
        }));
    }
    Ok(Value::Array(itms))
}

/// Entries grouped under a key, such as invoices under their party's GSTIN, in key order.
fn grouped(mut groups: IndexMap<String, Vec<Value>>, key: &str, list: &str) -> Vec<Value> {
    groups.sort_keys();
    groups
        .into_iter()
        .map(|(value, entries)| {
            let mut group = Map::new();
            group.insert(key.into(), json!(value));
            group.insert(list.into(), Value::Array(entries));
            Value::Object(group)
        })
        .collect()
}

/// Table 4: invoices to registered parties, by GSTIN.
fn b2b(supplies: &[Supply]) -> Result<Vec<Value>, Error> {
    let mut parties: IndexMap<String, Vec<Value>> = IndexMap::new();
    for s in supplies {
        if s.is_return || !s.registered() || s.rates.is_empty() {
            continue;
        }
        parties.entry(s.gstin.clone()).or_default().push(json!({
            "inum": s.name,
            "idt": gst_date(s.date),
            "val": amount(&s.value, false)?,
            "pos": s.pos,
            "rchrg": "N",
            "inv_typ": "R",
            "itms": itms(s)?,
        }));
    }
    Ok(grouped(parties, "ctin", "inv"))
}

/// Table 9B: credit notes to registered parties, by GSTIN.
fn cdnr(supplies: &[Supply]) -> Result<Vec<Value>, Error> {
    let mut parties: IndexMap<String, Vec<Value>> = IndexMap::new();
    for s in supplies {
        if !s.is_return || !s.registered() || s.rates.is_empty() {
            continue;
        }
        parties.entry(s.gstin.clone()).or_default().push(json!({
            "ntty": "C",
            "nt_num": s.name,
            "nt_dt": gst_date(s.date),
            "val": amount(&s.value, true)?,
            "pos": s.pos,
            "rchrg": "N",
            "inv_typ": "R",
            "itms": itms(s)?,
        }));
    }
    Ok(grouped(parties, "ctin", "nt"))
}

/// Table 5: B2C-Large invoices, by place of supply.
fn b2cl(supplies: &[Supply]) -> Result<Vec<Value>, Error> {
    let mut places: IndexMap<String, Vec<Value>> = IndexMap::new();
    for s in supplies {
        if s.is_return || !s.large || s.rates.is_empty() {
            continue;
        }
        places.entry(s.pos.clone()).or_default().push(json!({
            "inum": s.name,
            "idt": gst_date(s.date),
            "val": amount(&s.value, false)?,
            "itms": itms(s)?,
        }));
    }
    Ok(grouped(places, "pos", "inv"))
}

/// Table 9B: credit notes for B2C-Large invoices.
fn cdnur(supplies: &[Supply]) -> Result<Vec<Value>, Error> {
    let mut notes = Vec::new();
    for s in supplies {
        if !s.is_return || !s.large || s.rates.is_empty() {
            continue;
        }
        notes.push(json!({
            "typ": "B2CL",
            "ntty": "C",
            "nt_num": s.name,
            "nt_dt": gst_date(s.date),
            "val": amount(&s.value, true)?,
            "pos": s.pos,
            "itms": itms(s)?,
        }));
    }
    Ok(notes)
}

/// Table 7: the other sales to unregistered parties, net of their returns, by place of
/// supply, rate and type of supply.
fn b2cs(fyo: &Fyo, supplies: &[Supply]) -> Result<Vec<Value>, Error> {
    let mut rows: BTreeMap<(String, i64, bool), (f64, Taxes)> = BTreeMap::new();
    for s in supplies {
        if s.registered() || s.large {
            continue;
        }
        for (rate, taxes) in &s.rates {
            rows.entry((s.pos.clone(), rate_key(*rate), s.inter))
                .or_insert_with(|| (*rate, Taxes::new(fyo)))
                .1
                .add(taxes)?;
        }
    }
    let mut b2cs = Vec::new();
    for ((pos, _, inter), (rate, taxes)) in rows {
        if taxes.is_zero() {
            continue;
        }
        let mut row = Map::new();
        row.insert(
            "sply_ty".into(),
            json!(if inter { "INTER" } else { "INTRA" }),
        );
        row.insert("pos".into(), json!(pos));
        row.insert("typ".into(), json!("OE"));
        if let Value::Object(det) = itm_det(rate, &taxes, inter, false)? {
            row.extend(det);
        }
        b2cs.push(Value::Object(row));
    }
    Ok(b2cs)
}

/// Table 8: nil rated, exempt and non-GST supplies, net of returns.
fn nil(fyo: &Fyo, supplies: &[Supply]) -> Result<Vec<Value>, Error> {
    let types = ["INTRB2B", "INTRAB2B", "INTRB2C", "INTRAB2C"];
    let zero = fyo.pesa().zero();
    let mut totals = vec![(zero.clone(), zero.clone(), zero); types.len()];
    for s in supplies {
        let idx = match (s.registered(), s.inter) {
            (true, true) => 0,
            (true, false) => 1,
            (false, true) => 2,
            (false, false) => 3,
        };
        let total = &mut totals[idx];
        total.0 = total.0.add(&s.nil_rated)?;
        total.1 = total.1.add(&s.exempt)?;
        total.2 = total.2.add(&s.non_gst)?;
    }
    let mut rows = Vec::new();
    for (sply_ty, (nil_rated, exempt, non_gst)) in types.into_iter().zip(totals) {
        let row = json!({
            "sply_ty": sply_ty,
            "nil_amt": amount(&nil_rated, false)?,
            "expt_amt": amount(&exempt, false)?,
            "ngsup_amt": amount(&non_gst, false)?,
        });
        if ["nil_amt", "expt_amt", "ngsup_amt"]
            .iter()
            .any(|key| row[key] != json!(0))
        {
            rows.push(row);
        }
    }
    Ok(rows)
}

/// Table 12: the HSN summary, net of returns, for B2B and for B2C supplies.
fn hsn(fyo: &Fyo, supplies: &[Supply]) -> Result<Option<Value>, Error> {
    struct Row {
        rate: f64,
        qty: f64,
        taxes: Taxes,
        items: Vec<String>,
        inter: bool,
        intra: bool,
    }
    let mut rows: BTreeMap<(bool, String, i64, &str), Row> = BTreeMap::new();
    for s in supplies {
        for line in &s.lines {
            let row = rows
                .entry((
                    !s.registered(),
                    line.hsn.clone(),
                    rate_key(line.rate),
                    line.uqc,
                ))
                .or_insert_with(|| Row {
                    rate: line.rate,
                    qty: 0.0,
                    taxes: Taxes::new(fyo),
                    items: Vec::new(),
                    inter: false,
                    intra: false,
                });
            row.qty += line.qty;
            row.taxes.add(&line.taxes)?;
            if !row.items.contains(&line.item) {
                row.items.push(line.item.clone());
            }
            if s.inter {
                row.inter = true;
            } else {
                row.intra = true;
            }
        }
    }

    let (mut b2b, mut b2c) = (Vec::new(), Vec::new());
    for ((is_b2c, hsn, _, uqc), row) in rows {
        let qty = (row.qty * 100.0).round() / 100.0;
        if row.taxes.is_zero() && qty == 0.0 {
            continue;
        }
        let list = if is_b2c { &mut b2c } else { &mut b2b };
        let mut entry = Map::new();
        entry.insert("num".into(), json!(list.len() + 1));
        entry.insert("hsn_sc".into(), json!(hsn));
        entry.insert("desc".into(), json!(row.items.join(", ")));
        entry.insert("uqc".into(), json!(uqc));
        entry.insert("qty".into(), num(qty));
        entry.insert("rt".into(), num(row.rate));
        entry.insert("txval".into(), amount(&row.taxes.txval, false)?);
        if row.inter {
            entry.insert("iamt".into(), amount(&row.taxes.iamt, false)?);
        }
        if row.intra {
            entry.insert("camt".into(), amount(&row.taxes.camt, false)?);
            entry.insert("samt".into(), amount(&row.taxes.samt, false)?);
        }
        entry.insert("csamt".into(), num(0.0));
        list.push(Value::Object(entry));
    }
    if b2b.is_empty() && b2c.is_empty() {
        return Ok(None);
    }
    let mut hsn = Map::new();
    hsn.insert("hsn_b2b".into(), Value::Array(b2b));
    if !b2c.is_empty() {
        hsn.insert("hsn_b2c".into(), Value::Array(b2c));
    }
    Ok(Some(Value::Object(hsn)))
}

/// A document name's series and number: SINV-1019 is SINV- and 1019.
fn series(name: &str) -> (&str, Option<u64>) {
    let prefix = name.trim_end_matches(|c: char| c.is_ascii_digit());
    (prefix, name[prefix.len()..].parse().ok())
}

/// Table 13: the invoices and credit notes issued, cancelled ones included, by series.
fn doc_issue(invoices: &[PeriodInvoice]) -> Option<Value> {
    let mut series_docs: BTreeMap<(u8, &str), Vec<&PeriodInvoice>> = BTreeMap::new();
    for invoice in invoices {
        let doc_num = if invoice.is_return {
            CREDIT_NOTES
        } else {
            INVOICES
        };
        let (prefix, _) = series(&invoice.name);
        series_docs
            .entry((doc_num, prefix))
            .or_default()
            .push(invoice);
    }
    let mut doc_det: Vec<(u8, Vec<Value>)> = Vec::new();
    for ((doc_num, _), mut docs) in series_docs {
        docs.sort_by_key(|doc| (series(&doc.name).1, doc.name.as_str()));
        let (Some(first), Some(last)) = (docs.first(), docs.last()) else {
            continue;
        };
        let (first, last) = (&first.name, &last.name);
        let cancel = docs.iter().filter(|doc| doc.cancelled).count();
        if doc_det.last().is_none_or(|(n, _)| *n != doc_num) {
            doc_det.push((doc_num, Vec::new()));
        }
        let list = &mut doc_det.last_mut()?.1;
        let num = list.len() + 1;
        list.push(json!({
            "num": num,
            "from": first,
            "to": last,
            "totnum": docs.len(),
            "cancel": cancel,
            "net_issue": docs.len() - cancel,
        }));
    }
    if doc_det.is_empty() {
        return None;
    }
    let doc_det: Vec<Value> = doc_det
        .into_iter()
        .map(|(doc_num, docs)| json!({ "doc_num": doc_num, "docs": docs }))
        .collect();
    Some(json!({ "doc_det": doc_det }))
}

/// `getGstrJsonData`: the period's return as the GST portal's JSON.
pub(super) fn gstr_json(gstr: &Gstr, fyo: &Fyo) -> Result<String, Error> {
    let gstin = Gstr::company_gstin(fyo)?;
    let company_state = gstin.get(..2).unwrap_or_default();
    let fp = gstr
        .to_date
        .map(|date| date.strftime("%m%Y").to_string())
        .unwrap_or_default();

    let invoices = gstr.period_invoices(fyo, true)?;
    let mut kinds = HashMap::new();
    let mut supplies = Vec::new();
    for invoice in invoices.iter().filter(|invoice| !invoice.cancelled) {
        supplies.push(supply(fyo, gstr, invoice, company_state, &mut kinds)?);
    }
    supplies.sort_by(|a, b| (a.date, &a.name).cmp(&(b.date, &b.name)));

    let mut data = Map::new();
    data.insert("version".into(), json!(VERSION));
    data.insert("hash".into(), json!("hash"));
    data.insert("gstin".into(), json!(gstin));
    data.insert("fp".into(), json!(fp));
    section(&mut data, "b2b", b2b(&supplies)?);
    section(&mut data, "b2cl", b2cl(&supplies)?);
    section(&mut data, "b2cs", b2cs(fyo, &supplies)?);
    section(&mut data, "cdnr", cdnr(&supplies)?);
    section(&mut data, "cdnur", cdnur(&supplies)?);
    let nil = nil(fyo, &supplies)?;
    if !nil.is_empty() {
        data.insert("nil".into(), json!({ "inv": nil }));
    }
    if let Some(hsn) = hsn(fyo, &supplies)? {
        data.insert("hsn".into(), hsn);
    }
    if let Some(doc_issue) = doc_issue(&invoices) {
        data.insert("doc_issue".into(), doc_issue);
    }
    Ok(Value::Object(data).to_string())
}

/// Adds a table to the return if it has rows.
fn section(data: &mut Map<String, Value>, name: &str, rows: Vec<Value>) {
    if !rows.is_empty() {
        data.insert(name.into(), Value::Array(rows));
    }
}
