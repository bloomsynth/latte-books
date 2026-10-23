//! Create Demo: `dummy/index.ts` and `dummy/helpers.ts` @ v0.36.0. Makes a company,
//! Flo's Clothes, and fills it with a year of made-up sales, purchases, payments and
//! journal entries. `items.json`, `parties.json` and the logo are Books' own.
//!
//! Differences from Books (Books bugs, fixed):
//! - The fiscal year ends on 31 March. Books passes `04-01` for its end too, so its year
//!   ends on 1 April.
//! - A sales invoice doesn't get the same item twice. Books means to skip an item already
//!   on the invoice, but compares the row's item name with the item itself, so never does.
//! - One sale in twenty paid at a discount has 15% written off. Books sets `writeOff`,
//!   which isn't a Payment field (it's `writeoff`), so nothing is ever written off.

use indexmap::IndexMap;
use jiff::ToSpan;
use jiff::civil::Date;
use latte_db::pesa::Money;
use latte_db::{DocValue, DocValueMap};

use crate::setup::{SetupWizardOptions, setup_instance, today};
use crate::{Doc, Error, Fyo};

const ITEMS: &str = include_str!("items.json");
const PARTIES: &str = include_str!("parties.json");
const LOGO: &str = include_str!("logo.txt");

/// `partyPurchaseItemMap`.
const PARTY_PURCHASE_ITEMS: [(&str, &[&str]); 9] = [
    ("Janky Office Spaces", &["Office Rent", "Office Cleaning"]),
    ("Josféña's 611s", &["611 Jeans - PCH", "611 Jeans - SHR"]),
    (
        "Lankness Feet Fomenters",
        &["Bominga Shoes", "Jade Slippers"],
    ),
    (
        "The Overclothes Company",
        &["Jacket - RAW", "Cryo Gloves", "Cool Cloth"],
    ),
    ("Adani Electricity Mumbai Limited", &["Electricity"]),
    ("Only Fulls", &["Full Sleeve - BLK", "Full Sleeve - COL"]),
    ("Just Epaulettes", &["Epaulettes - 4POR"]),
    ("Le Socials", &["Social Ads"]),
    ("Maxwell", &["Marketing - Video"]),
];

/// `flow`: how busy each month is, January to December.
const FLOW: [f64; 12] = [
    0.35, 0.25, 0.15, 0.15, 0.25, 0.05, 0.05, 0.15, 0.25, 0.35, 0.45, 0.55,
];

/// How far along Create Demo is: a stage and a fraction done, or -1 when it can't say.
pub type Notifier<'a> = &'a mut dyn FnMut(&str, f64);

/// `purchaseItemPartyMap`: who an item is bought from.
fn supplier_of(item: &str) -> Option<&'static str> {
    PARTY_PURCHASE_ITEMS
        .iter()
        .find(|(_, items)| items.contains(&item))
        .map(|(party, _)| *party)
}

/// An entry of `items.json` or `parties.json`.
type Record = serde_json::Map<String, serde_json::Value>;

fn records(json: &str) -> Vec<Record> {
    serde_json::from_str(json).expect("Books' demo data is valid JSON")
}

fn str_of<'a>(record: &'a Record, key: &str) -> Option<&'a str> {
    record.get(key).and_then(|v| v.as_str())
}

fn rate_of(record: &Record) -> f64 {
    record.get("rate").and_then(|v| v.as_f64()).unwrap_or(0.0)
}

/// A record as document values; nulls are left out.
fn values_of(record: &Record) -> DocValueMap {
    record
        .iter()
        .filter_map(|(key, value)| {
            let value = match value {
                serde_json::Value::String(s) => DocValue::Text(s.clone()),
                serde_json::Value::Number(n) => DocValue::Number(n.as_f64()?),
                serde_json::Value::Bool(b) => DocValue::Bool(*b),
                _ => return None,
            };
            Some((key.clone(), value))
        })
        .collect()
}

fn text(value: &str) -> DocValue {
    DocValue::Text(value.to_owned())
}

/// The date `months` back from `date`.
fn months_back(date: Date, months: u32) -> Result<Date, Error> {
    date.checked_sub(i64::from(months).months())
        .map_err(|e| Error::Value(e.to_string()))
}

fn dated(date: Date) -> DocValueMap {
    DocValueMap::from([("date".to_owned(), text(&date.to_string()))])
}

/// `setupDummyInstance`: creates the demo company at `path`, with `years` of documents;
/// there are always more sales invoices than `base_count`.
pub fn setup_dummy_instance(
    path: impl AsRef<std::path::Path>,
    years: u32,
    base_count: u32,
    device: &str,
    rng: &mut fastrand::Rng,
    notifier: Notifier<'_>,
) -> Result<Fyo, Error> {
    notifier("Setting Up Instance", -1.0);
    let today = today();
    let mut options = SetupWizardOptions::for_country("India", today);
    options.company_name = "Flo's Clothes".into();
    options.fullname = "Lin Florentine".into();
    options.email = "lin@flosclothes.com".into();
    options.bank_name = "Supreme Bank".into();
    let mut fyo = setup_instance(path, &options, device)?;

    notifier("Creating Items and Parties", -1.0);
    generate_static_entries(&mut fyo)?;
    generate_dynamic_entries(&mut fyo, today, years, base_count, rng, notifier)?;
    set_other_settings(&mut fyo)?;
    set_single(
        &mut fyo,
        "SystemSettings",
        "hideGetStarted",
        DocValue::Bool(true),
    )?;
    Ok(fyo)
}

fn set_single(
    fyo: &mut Fyo,
    schema_name: &str,
    fieldname: &str,
    value: DocValue,
) -> Result<(), Error> {
    let mut doc = fyo.get_doc(schema_name, None)?;
    doc.set(fyo, fieldname, value)?;
    doc.sync(fyo)
}

/// `setOtherSettings`: the print settings, with an address and logo, and the GSTIN.
fn set_other_settings(fyo: &mut Fyo) -> Result<(), Error> {
    let address = DocValueMap::from_iter(
        [
            ("addressLine1", "1st Column, Fitzgerald Bridge"),
            ("city", "Pune"),
            ("state", "Maharashtra"),
            ("pos", "Maharashtra"),
            ("postalCode", "411001"),
            ("country", "India"),
        ]
        .map(|(k, v)| (k.to_owned(), text(v))),
    );
    let mut address = fyo.get_new_doc("Address", address)?;
    address.sync(fyo)?;
    let address = address.name().unwrap_or_default().to_owned();

    let mut doc = fyo.get_doc("PrintSettings", None)?;
    doc.set(fyo, "color", text("#F687B3"))?;
    // Books also sets `template: 'Business'`, which PrintSettings no longer has.
    doc.set(fyo, "displayLogo", DocValue::Bool(true))?;
    doc.set(fyo, "phone", text("+91 8983-000418"))?;
    doc.set(fyo, "logo", text(LOGO.trim()))?;
    doc.set(fyo, "address", text(&address))?;
    doc.sync(fyo)?;

    set_single(fyo, "AccountingSettings", "gstin", text("27LIN180000A1Z5"))
}

/// `generateStaticEntries`.
fn generate_static_entries(fyo: &mut Fyo) -> Result<(), Error> {
    for (schema_name, json) in [("Item", ITEMS), ("Party", PARTIES)] {
        for record in records(json) {
            fyo.get_new_doc(schema_name, values_of(&record))?
                .sync(fyo)?;
        }
    }
    Ok(())
}

/// `generateDynamicEntries`.
fn generate_dynamic_entries(
    fyo: &mut Fyo,
    today: Date,
    years: u32,
    base_count: u32,
    rng: &mut fastrand::Rng,
    notifier: Notifier<'_>,
) -> Result<(), Error> {
    let items = records(ITEMS);
    let mut sales = get_sales_invoices(fyo, &items, today, years, base_count, rng, notifier)?;
    sales.sort_by_key(|(date, _)| *date);

    notifier("Creating Purchase Invoices", -1.0);
    let mut invoices = get_sales_purchase_invoices(fyo, &sales)?;
    invoices.extend(get_non_sales_purchase_invoices(
        fyo, &items, today, years, rng,
    )?);

    notifier("Creating Journal Entries", -1.0);
    let mut journal_entries = get_journal_entries(fyo, &sales)?;
    sync_and_submit(fyo, &mut journal_entries, notifier)?;

    invoices.extend(sales);
    invoices.sort_by_key(|(date, _)| *date);
    let mut invoices: Vec<Doc> = invoices.into_iter().map(|(_, doc)| doc).collect();
    sync_and_submit(fyo, &mut invoices, notifier)?;

    let mut payments = get_payments(fyo, &invoices, rng)?;
    sync_and_submit(fyo, &mut payments, notifier)
}

/// `getJournalEntries`: a loan into the bank, and some of it drawn as cash, six months
/// before the last sale.
fn get_journal_entries(fyo: &Fyo, sales: &[(Date, Doc)]) -> Result<Vec<Doc>, Error> {
    let mut total = fyo.pesa().zero();
    for (_, invoice) in sales {
        total = total.add(&invoice.get_sum(fyo, "items", "amount")?)?;
    }
    let amount = total.percent(75.0)?.clip(Some(0))?;
    let Some((last, _)) = sales.last() else {
        return Ok(Vec::new());
    };
    let date = months_back(*last, 6)?;

    let row = |account: &str, debit: &Money, credit: &Money| {
        DocValueMap::from_iter([
            ("account".to_owned(), text(account)),
            ("debit".to_owned(), DocValue::Money(debit.clone())),
            ("credit".to_owned(), DocValue::Money(credit.clone())),
        ])
    };
    let zero = fyo.pesa().zero();
    let cash = amount.percent(30.0)?;
    let entries = [
        (
            "Bank Entry",
            [
                row("Supreme Bank", &amount, &zero),
                row("Secured Loans", &zero, &amount),
            ],
        ),
        (
            "Cash Entry",
            [row("Cash", &cash, &zero), row("Supreme Bank", &zero, &cash)],
        ),
    ];
    let mut docs = Vec::new();
    for (entry_type, rows) in entries {
        let mut values = dated(date);
        values.insert("entryType".into(), text(entry_type));
        values.insert("accounts".into(), DocValue::Table(rows.to_vec()));
        docs.push(fyo.get_new_doc("JournalEntry", values)?);
    }
    Ok(docs)
}

fn money_value(doc: &Doc, fieldname: &str, fyo: &Fyo) -> Money {
    match doc.get(fieldname) {
        Some(DocValue::Money(money)) => money.clone(),
        _ => fyo.pesa().zero(),
    }
}

/// `getPayments`: each invoice paid in cash on its day, but for a few customers who
/// don't pay.
fn get_payments(
    fyo: &mut Fyo,
    invoices: &[Doc],
    rng: &mut fastrand::Rng,
) -> Result<Vec<Doc>, Error> {
    let mut payments = Vec::new();
    for invoice in invoices {
        let is_sales = invoice.schema_name() == "SalesInvoice";
        // Defaulters.
        if is_sales && rng.f64() < 0.007 {
            continue;
        }
        let outstanding = money_value(invoice, "outstandingAmount", fyo);
        if outstanding.is_zero() {
            continue;
        }
        let party = match invoice.get("party") {
            Some(DocValue::Text(party)) => party.clone(),
            _ => continue,
        };
        let (payment_type, account, payment_account) = if is_sales {
            ("Receive", "Debtors", "Cash")
        } else {
            ("Pay", "Cash", "Creditors")
        };
        let mut values = DocValueMap::from_iter(
            [
                ("party", party.as_str()),
                ("paymentType", payment_type),
                ("paymentMethod", "Cash"),
                ("account", account),
                ("paymentAccount", payment_account),
            ]
            .map(|(k, v)| (k.to_owned(), text(v))),
        );
        values.insert(
            "date".into(),
            invoice.get("date").cloned().unwrap_or(DocValue::Null),
        );
        values.insert("amount".into(), DocValue::Money(outstanding.clone()));
        let mut doc = fyo.get_new_doc("Payment", values)?;

        // Discount.
        if is_sales && rng.f64() < 0.05 {
            doc.set(fyo, "writeoff", DocValue::Money(outstanding.percent(15.0)?))?;
        }

        let row = DocValueMap::from_iter([
            ("referenceType".to_owned(), text(invoice.schema_name())),
            (
                "referenceName".to_owned(),
                text(invoice.name().unwrap_or_default()),
            ),
            ("amount".to_owned(), DocValue::Money(outstanding)),
        ]);
        doc.push_row(fyo, "for", row)?;
        payments.push(doc);
    }
    Ok(payments)
}

/// `getFlowConstant`: how busy the month `months` back from today is.
fn flow_constant(today: Date, months: u32) -> Result<f64, Error> {
    let date = months_back(today, months)?;
    Ok(FLOW[date.month() as usize - 1])
}

/// `getRandomDates`: `count` days in the month `months` back from today, up to today in
/// this month.
fn random_dates(
    today: Date,
    count: u32,
    months: u32,
    rng: &mut fastrand::Rng,
) -> Result<Vec<Date>, Error> {
    let end = if months == 0 {
        today
    } else {
        months_back(today, months)?.last_of_month()
    };
    (0..count)
        .map(|_| {
            let day = rng.i8(1..=end.day());
            end.with()
                .day(day)
                .build()
                .map_err(|e| Error::Value(e.to_string()))
        })
        .collect()
}

/// `getSalesInvoiceDates`.
fn sales_invoice_dates(
    today: Date,
    years: u32,
    base_count: u32,
    rng: &mut fastrand::Rng,
) -> Result<Vec<Date>, Error> {
    let mut dates = Vec::new();
    for months in 0..years * 12 {
        let flow = flow_constant(today, months)?;
        let count = (flow * f64::from(base_count) * (rng.f64() * 0.25 + 0.75)).ceil() as u32;
        dates.extend(random_dates(today, count, months, rng)?);
    }
    Ok(dates)
}

/// `Math.ceil(Math.random() * n)`, as a quantity.
fn up_to(n: u32, rng: &mut fastrand::Rng) -> f64 {
    f64::from(rng.u32(1..=n))
}

/// `getSalesInvoices`: each made as the form makes it, a customer and up to five items.
fn get_sales_invoices(
    fyo: &mut Fyo,
    items: &[Record],
    today: Date,
    years: u32,
    base_count: u32,
    rng: &mut fastrand::Rng,
    notifier: Notifier<'_>,
) -> Result<Vec<(Date, Doc)>, Error> {
    let sales_items: Vec<&Record> = items
        .iter()
        .filter(|i| str_of(i, "for") != Some("Purchases"))
        .collect();
    let parties = records(PARTIES);
    let customers: Vec<&str> = parties
        .iter()
        .filter(|p| str_of(p, "role") != Some("Supplier"))
        .filter_map(|p| str_of(p, "name"))
        .collect();

    let dates = sales_invoice_dates(today, years, base_count, rng)?;
    let mut invoices = Vec::new();
    for (d, date) in dates.iter().enumerate() {
        notifier(
            &format!("Creating Sales Invoices, {d} out of {}", dates.len()),
            d as f64 / dates.len() as f64,
        );
        let customer = customers[rng.usize(..customers.len())];
        let mut doc = fyo.get_new_doc("SalesInvoice", dated(*date))?;
        doc.set(fyo, "party", text(customer))?;
        if matches!(doc.get("account"), None | Some(DocValue::Null)) {
            doc.set(fyo, "account", text("Debtors"))?;
        }

        let mut added: Vec<&str> = Vec::new();
        for _ in 0..rng.u32(1..=5) {
            let item = sales_items[rng.usize(..sales_items.len())];
            let name = str_of(item, "name").unwrap_or_default();
            if added.contains(&name) {
                continue;
            }
            added.push(name);

            let rate = rate_of(item);
            let quantity = if rate < 100.0 && rng.f64() < 0.4 {
                up_to(10, rng)
            } else if rate < 1000.0 && rng.f64() < 0.2 {
                up_to(4, rng)
            } else if rng.f64() < 0.01 {
                up_to(3, rng)
            } else {
                1.0
            };

            let mut fc = FLOW[date.month() as usize - 1];
            if base_count < 500 {
                fc += 1.0;
            }
            let rate = fyo.pesa().from_f64(rate * (fc + 1.0))?.clip(Some(0))?;

            let idx = doc.table("items").len();
            doc.append(fyo, "items", DocValueMap::new())?;
            doc.set_child(fyo, "items", idx, "item", text(name))?;
            doc.set_child(fyo, "items", idx, "rate", DocValue::Money(rate))?;
            doc.set_child(fyo, "items", idx, "quantity", DocValue::Number(quantity))?;
            for field in ["incomeAccount", "tax", "description", "hsnCode"] {
                if let Some(value) = str_of(item, field) {
                    let fieldname = if field == "incomeAccount" {
                        "account"
                    } else {
                        field
                    };
                    doc.set_child(fyo, "items", idx, fieldname, text(value))?;
                }
            }
        }
        invoices.push((*date, doc));
    }
    Ok(invoices)
}

/// A purchase invoice from `supplier` on `date`, with no rows yet.
fn purchase_invoice(fyo: &mut Fyo, supplier: &str, date: Date) -> Result<Doc, Error> {
    let mut doc = fyo.get_new_doc("PurchaseInvoice", dated(date))?;
    doc.set(fyo, "party", text(supplier))?;
    if matches!(doc.get("account"), None | Some(DocValue::Null)) {
        doc.set(fyo, "account", text("Creditors"))?;
    }
    Ok(doc)
}

/// `getSalesPurchaseInvoices`: on the first of each month, the stock sold that month is
/// bought again from its supplier, in tens.
fn get_sales_purchase_invoices(
    fyo: &mut Fyo,
    sales: &[(Date, Doc)],
) -> Result<Vec<(Date, Doc)>, Error> {
    // The sales invoices by month, in order.
    let mut by_month: IndexMap<Date, Vec<&Doc>> = IndexMap::new();
    for (date, invoice) in sales {
        by_month
            .entry(date.first_of_month())
            .or_default()
            .push(invoice);
    }
    by_month.sort_keys();

    let mut purchase_qty: IndexMap<String, f64> = IndexMap::new();
    let mut invoices = Vec::new();
    for (date, month) in by_month {
        // The quantity of each item sold that month.
        let mut sold: IndexMap<String, f64> = IndexMap::new();
        for invoice in month {
            for row in invoice.table("items") {
                let Some(DocValue::Text(item)) = row.get("item") else {
                    continue;
                };
                if item == "Dry-Cleaning" {
                    continue;
                }
                let quantity = match row.get("quantity") {
                    Some(DocValue::Number(q)) => *q,
                    _ => 0.0,
                };
                *sold.entry(item.clone()).or_default() += quantity;
            }
        }

        // The order quantity for the first of the month.
        for (name, quantity) in &sold {
            let previous = purchase_qty.entry(name.clone()).or_default();
            if *previous <= *quantity {
                *previous = quantity - *previous;
            }
            *previous = (*previous / 10.0).ceil() * 10.0;
        }

        let mut by_supplier: IndexMap<&str, Vec<&str>> = IndexMap::new();
        for name in sold.keys() {
            if let Some(supplier) = supplier_of(name) {
                by_supplier.entry(supplier).or_default().push(name);
            }
        }

        for (supplier, items) in by_supplier {
            let mut doc = purchase_invoice(fyo, supplier, date)?;
            for (idx, item) in items.into_iter().enumerate() {
                doc.append(fyo, "items", DocValueMap::new())?;
                doc.set_child(fyo, "items", idx, "item", text(item))?;
                let quantity = purchase_qty.get(item).copied().unwrap_or(0.0);
                doc.set_child(fyo, "items", idx, "quantity", DocValue::Number(quantity))?;
            }
            invoices.push((date, doc));
        }
    }
    Ok(invoices)
}

/// `getNonSalesPurchaseInvoices`: rent, power, cleaning and ads, on the first of each
/// month.
fn get_non_sales_purchase_invoices(
    fyo: &mut Fyo,
    items: &[Record],
    today: Date,
    years: u32,
    rng: &mut fastrand::Rng,
) -> Result<Vec<(Date, Doc)>, Error> {
    let periodic = [
        ("Marketing - Video", 2),
        ("Social Ads", 1),
        ("Electricity", 1),
        ("Office Cleaning", 1),
        ("Office Rent", 1),
    ];
    let mut invoices = Vec::new();
    for months in 0..years * 12 {
        let date = months_back(today, months)?.first_of_month();
        for (name, every) in periodic {
            if months % every != 0 {
                continue;
            }
            let Some(item) = items.iter().find(|i| str_of(i, "name") == Some(name)) else {
                continue;
            };
            let supplier = supplier_of(name).unwrap_or_default();
            let mut doc = purchase_invoice(fyo, supplier, date)?;

            let mut quantity = 1.0;
            let mut rate = rate_of(item);
            if name == "Social Ads" {
                quantity = up_to(200, rng);
            } else if name != "Office Rent" {
                rate *= rng.f64() * 0.4 + 0.8;
            }
            let rate = fyo.pesa().from_f64(rate)?.clip(Some(0))?;

            doc.append(fyo, "items", DocValueMap::new())?;
            doc.set_child(fyo, "items", 0, "item", text(name))?;
            doc.set_child(fyo, "items", 0, "quantity", DocValue::Number(quantity))?;
            doc.set_child(fyo, "items", 0, "rate", DocValue::Money(rate))?;
            invoices.push((date, doc));
        }
    }
    Ok(invoices)
}

/// `syncAndSubmit`.
fn sync_and_submit(fyo: &mut Fyo, docs: &mut [Doc], notifier: Notifier<'_>) -> Result<(), Error> {
    let total = docs.len();
    for (i, doc) in docs.iter_mut().enumerate() {
        let kind = match doc.schema_name() {
            "SalesInvoice" | "PurchaseInvoice" => "Invoices",
            "Payment" => "Payments",
            _ => "Journal Entries",
        };
        notifier(
            &format!("Syncing {kind}, {i} out of {total}"),
            i as f64 / total as f64,
        );
        doc.sync(fyo)?;
        doc.submit(fyo)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
