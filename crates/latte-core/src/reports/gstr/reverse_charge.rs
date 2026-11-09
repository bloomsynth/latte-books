// Not in Books: the reverse charge (RCM), for a purchase on which the company, not the
// supplier, pays the GST, such as a service bought from abroad.
//
// A reverse charge purchase takes one of the RCM tax templates, such as IGST-18-RCM. Its
// rows are the GST at the rate, on the GST account it is claimed on as input tax credit,
// and the same GST taken off again, on the RCM account it is owed on:
//
//     IGST-18-RCM: IGST 18%, IGST (RCM) -18%
//
// so the invoice charges what the supplier bills, and its posting debits IGST (the
// credit) and credits IGST (RCM) (the tax to pay in cash, with GSTR-3B). The templates
// and accounts are ordinary rows, so the file stays as Frappe Books reads it; Books
// shows the same totals and balances.

use latte_db::pesa::Money;
use latte_db::{DocValue, DocValueMap};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::models::base_models::address::CODE_STATE_MAP;
use crate::models::base_models::invoice::{exchange_rate, tax_base};
use crate::models::helpers::text;

/// The GST accounts, each with the RCM account its reverse charge is owed on.
pub const ACCOUNTS: [(&str, &str); 3] = [
    ("IGST", "IGST (RCM)"),
    ("CGST", "CGST (RCM)"),
    ("SGST", "SGST (RCM)"),
];

/// The rates the RCM templates are made for: 18% for most services, such as software
/// and legal services, and 5% for goods transport.
const RATES: [f64; 2] = [18.0, 5.0];

/// An RCM template's name: IGST-18-RCM between states or from abroad, GST-18-RCM
/// within the state.
pub fn template_name(rate: f64, integrated: bool) -> String {
    let kind = if integrated { "IGST" } else { "GST" };
    format!("{kind}-{rate}-RCM")
}

/// The RCM templates, as their names and rows (account, rate).
fn templates() -> Vec<(String, Vec<(&'static str, f64)>)> {
    let mut templates = Vec::new();
    for rate in RATES {
        templates.push((
            template_name(rate, true),
            vec![("IGST", rate), ("IGST (RCM)", -rate)],
        ));
        let half = rate / 2.0;
        templates.push((
            template_name(rate, false),
            vec![
                ("CGST", half),
                ("SGST", half),
                ("CGST (RCM)", -half),
                ("SGST (RCM)", -half),
            ],
        ));
    }
    templates
}

fn text_value(value: &str) -> DocValue {
    DocValue::Text(value.to_owned())
}

/// Whether the RCM accounts and templates are all in the file.
pub fn is_set_up(fyo: &Fyo) -> Result<bool, Error> {
    for (_, account) in ACCOUNTS {
        if !fyo.db.exists("Account", Some(account))? {
            return Ok(false);
        }
    }
    for (name, _) in templates() {
        if !fyo.db.exists("Tax", Some(&name))? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Adds the RCM accounts, beside IGST under Duties and Taxes, and the RCM templates,
/// those the file doesn't have yet. What was added, by name.
pub fn set_up(fyo: &mut Fyo) -> Result<Vec<String>, Error> {
    let parent = match fyo.get_value("Account", Some("IGST"), "parentAccount") {
        Some(DocValue::Text(parent)) if !parent.is_empty() => parent,
        _ => {
            return Err(Error::Validation(
                "The reverse charge needs the IGST account, which India's chart of accounts \
                 has under Duties and Taxes."
                    .to_owned(),
            ));
        }
    };
    let root_type = match fyo.get_value("Account", Some(&parent), "rootType") {
        Some(DocValue::Text(root_type)) if !root_type.is_empty() => root_type,
        _ => "Liability".to_owned(),
    };

    let mut added = Vec::new();
    for (_, account) in ACCOUNTS {
        if fyo.db.exists("Account", Some(account))? {
            continue;
        }
        let mut doc = fyo.get_new_doc(
            "Account",
            DocValueMap::from([
                ("name".to_owned(), text_value(account)),
                ("parentAccount".to_owned(), text_value(&parent)),
                ("isGroup".to_owned(), DocValue::Bool(false)),
                ("rootType".to_owned(), text_value(&root_type)),
                ("accountType".to_owned(), text_value("Tax")),
            ]),
        )?;
        doc.sync(fyo)?;
        added.push(account.to_owned());
    }
    for (name, rows) in templates() {
        if fyo.db.exists("Tax", Some(&name))? {
            continue;
        }
        let details = rows
            .into_iter()
            .map(|(account, rate)| {
                DocValueMap::from([
                    ("account".to_owned(), text_value(account)),
                    ("rate".to_owned(), DocValue::Number(rate)),
                ])
            })
            .collect();
        let mut doc = fyo.get_new_doc(
            "Tax",
            DocValueMap::from([
                ("name".to_owned(), text_value(&name)),
                ("details".to_owned(), DocValue::Table(details)),
            ]),
        )?;
        doc.sync(fyo)?;
        added.push(name);
    }
    Ok(added)
}

/// A tax template's rows, as (account, rate).
fn tax_rows(fyo: &Fyo, tax: &str) -> Result<Vec<(String, f64)>, Error> {
    let tax = fyo.db.get_doc("Tax", tax)?;
    let Some(DocValue::Table(details)) = tax.get("details") else {
        return Ok(Vec::new());
    };
    Ok(details
        .iter()
        .map(|detail| {
            let account = match detail.get("account") {
                Some(DocValue::Text(account)) => account.clone(),
                _ => String::new(),
            };
            let rate = match detail.get("rate") {
                Some(DocValue::Number(rate)) => *rate,
                _ => 0.0,
            };
            (account, rate)
        })
        .collect())
}

/// Whether a tax template is a reverse charge's: one of its rows is on an RCM account.
pub fn is_reverse_charge_tax(fyo: &Fyo, tax: &str) -> Result<bool, Error> {
    Ok(tax_rows(fyo, tax)?
        .iter()
        .any(|(account, _)| ACCOUNTS.iter().any(|(_, rcm)| rcm == account)))
}

/// A purchase invoice's reverse charge, in rupees: the value of its rows that take an
/// RCM template, and the GST the company owes on them. A return's is negative.
#[derive(Debug, Clone)]
pub struct ReverseCharge {
    pub taxable: Money,
    pub igst: Money,
    pub cgst: Money,
    pub sgst: Money,
    /// A service bought from abroad (an import of services), as GSTR-3B's Table 4(A)(2)
    /// has it: integrated tax from a supplier whose address isn't in India, or who has
    /// none.
    pub import: bool,
    /// The rows under the reverse charge, as the self-invoice lists them.
    pub rows: Vec<ChargeRow>,
}

/// A row under the reverse charge, in rupees.
#[derive(Debug, Clone)]
pub struct ChargeRow {
    pub item: String,
    /// The row's HSN/SAC code, if it has one.
    pub hsn: Option<String>,
    pub value: Money,
    /// The GST rate, all three taxes.
    pub rate: f64,
    pub igst: Money,
    pub cgst: Money,
    pub sgst: Money,
}

impl ReverseCharge {
    /// The GST owed, all three taxes.
    pub fn tax(&self) -> Result<Money, Error> {
        Ok(self.igst.add(&self.cgst)?.add(&self.sgst)?)
    }
}

/// The place of supply of a service bought under the reverse charge: the company's state,
/// by its GSTIN, as "29-Karnataka".
pub fn place_of_supply(fyo: &Fyo) -> Option<String> {
    let gstin = match fyo.get_value("AccountingSettings", None, "gstin") {
        Some(DocValue::Text(gstin)) => gstin,
        _ => return None,
    };
    let code = gstin.get(..2)?;
    let state = CODE_STATE_MAP.iter().find(|(c, _)| *c == code)?.1;
    Some(format!("{code}-{state}"))
}

/// The country of a party's address, if it has one.
fn party_country(fyo: &Fyo, party: &str) -> Option<String> {
    let address = match fyo.get_value("Party", Some(party), "address") {
        Some(DocValue::Text(address)) if !address.is_empty() => address,
        _ => return None,
    };
    match fyo.get_value("Address", Some(&address), "country") {
        Some(DocValue::Text(country)) if !country.is_empty() => Some(country),
        _ => None,
    }
}

/// The reverse charge on a purchase invoice; none for one with no RCM template, or for
/// another schema.
pub fn of_invoice(fyo: &Fyo, doc: &Doc) -> Result<Option<ReverseCharge>, Error> {
    if doc.schema_name() != "PurchaseInvoice" {
        return Ok(None);
    }
    let zero = fyo.pesa().zero();
    let mut charge = ReverseCharge {
        taxable: zero.clone(),
        igst: zero.clone(),
        cgst: zero.clone(),
        sgst: zero.clone(),
        import: false,
        rows: Vec::new(),
    };
    let rupees = exchange_rate(doc);
    let mut found = false;
    for item in doc.table("items") {
        let Some(tax) = text(item, "tax").filter(|tax| !tax.is_empty()) else {
            continue;
        };
        if !is_reverse_charge_tax(fyo, tax)? {
            continue;
        }
        found = true;
        let base = tax_base(doc, fyo, item)?.mul(rupees)?;
        charge.taxable = charge.taxable.add(&base)?;
        let mut row = ChargeRow {
            item: text(item, "item").unwrap_or_default().to_owned(),
            hsn: match item.get("hsnCode") {
                Some(DocValue::Number(code)) if *code > 0.0 => {
                    Some(format!("{}", code.trunc() as i64))
                }
                Some(DocValue::Text(code)) if !code.trim().is_empty() => {
                    Some(code.trim().to_owned())
                }
                _ => None,
            },
            value: base.clone(),
            rate: 0.0,
            igst: zero.clone(),
            cgst: zero.clone(),
            sgst: zero.clone(),
        };
        for (account, rate) in tax_rows(fyo, tax)? {
            // The RCM rows' rates are negative: the tax is owed, not charged.
            let owed = base.percent(-rate)?;
            let total = match account.as_str() {
                "IGST (RCM)" => &mut row.igst,
                "CGST (RCM)" => &mut row.cgst,
                "SGST (RCM)" => &mut row.sgst,
                _ => continue,
            };
            *total = total.add(&owed)?;
            row.rate -= rate;
        }
        charge.igst = charge.igst.add(&row.igst)?;
        charge.cgst = charge.cgst.add(&row.cgst)?;
        charge.sgst = charge.sgst.add(&row.sgst)?;
        charge.rows.push(row);
    }
    if !found {
        return Ok(None);
    }
    let party = text(doc, "party").unwrap_or_default();
    let abroad = party_country(fyo, party).is_none_or(|country| country != "India");
    charge.import = abroad && charge.cgst.is_zero() && charge.sgst.is_zero();
    Ok(Some(charge))
}
