//! Makes the company the README's screenshot shows: Copperleaf Coffee, a made-up
//! US coffee roaster with a year of growing wholesale sales, seasonal peaks, green coffee
//! bought to match, rent, payroll and ads, and customers who pay 10 to 45 days later.
//!
//!     cargo run -p latte-core --example screenshot_test -- "path/to/screenshot test.books.db"
//!
//! The file must not exist yet, and its name must contain "test". Unlike Create Demo, the
//! numbers are tuned to look like a real small business on the dashboard: no loan dwarfing
//! the cashflow chart, mostly profitable months, and some invoices still unpaid. The same
//! seed gives the same company, ending today.

use std::process::ExitCode;

use jiff::ToSpan;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use latte_core::models::base_models::invoice::get_payment;
use latte_core::setup::{SetupWizardOptions, setup_instance, today};
use latte_core::{Doc, Fyo};
use latte_db::{DocValue, DocValueMap};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const BANK: &str = "First Coast Bank";

/// Sales items: name, unit, rate, income account.
const SALES_ITEMS: [(&str, &str, f64, &str); 7] = [
    ("House Espresso Blend", "Kg", 26.0, "Sales"),
    ("Ethiopia Yirgacheffe", "Kg", 34.0, "Sales"),
    ("Colombia Huila", "Kg", 30.0, "Sales"),
    ("Swiss Water Decaf", "Kg", 29.0, "Sales"),
    ("Cold Brew Concentrate", "Unit", 16.0, "Sales"),
    ("Drip Bags, Box of 10", "Unit", 12.0, "Sales"),
    ("Barista Training", "Hour", 95.0, "Service"),
];

/// Purchase items: name, unit, rate, expense account, supplier.
const PURCHASE_ITEMS: [(&str, &str, f64, &str, &str); 7] = [
    (
        "Green Coffee, Ethiopia",
        "Kg",
        9.5,
        "Cost of Goods Sold",
        "Altura Green Coffee Imports",
    ),
    (
        "Green Coffee, Colombia",
        "Kg",
        8.0,
        "Cost of Goods Sold",
        "Altura Green Coffee Imports",
    ),
    (
        "Green Coffee, Brazil",
        "Kg",
        6.5,
        "Cost of Goods Sold",
        "Altura Green Coffee Imports",
    ),
    (
        "Bags and Labels",
        "Unit",
        0.45,
        "Cost of Goods Sold",
        "Kraft Pack Supply",
    ),
    (
        "Roastery Rent",
        "Unit",
        6500.0,
        "Office Rent",
        "Wharfside Properties",
    ),
    (
        "Electricity and Gas",
        "Unit",
        1100.0,
        "Utility Expenses",
        "Bay City Power",
    ),
    (
        "Online Ads",
        "Unit",
        1.0,
        "Marketing Expenses",
        "Brightline Media",
    ),
];

const CUSTOMERS: [&str; 12] = [
    "Morning Tide Café",
    "Juniper & Rye Bakery",
    "Northside Grocers",
    "The Reading Room Café",
    "Blue Door Bistro",
    "Hillcrest Co-op",
    "Lantern Street Diner",
    "Saltwater Kitchen",
    "Parkview Hotel",
    "Ember & Oak",
    "Sunday Market Deli",
    "Cornerstone Books & Coffee",
];

/// How busy each month is, January to December: a holiday peak, a slow January and a
/// cold-brew summer.
const SEASON: [f64; 12] = [
    0.8, 0.85, 0.95, 1.0, 1.05, 1.15, 1.2, 1.1, 1.0, 1.05, 1.1, 1.35,
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [path] = args.as_slice() else {
        eprintln!("usage: screenshot_test <new .books.db with \"test\" in its name>");
        return ExitCode::FAILURE;
    };
    let file_name = std::path::Path::new(path)
        .file_name()
        .map(|f| f.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if !file_name.contains("test") {
        eprintln!("refusing: the file name must contain \"test\"");
        return ExitCode::FAILURE;
    }
    if std::path::Path::new(path).exists() {
        eprintln!("refusing: {path} already exists");
        return ExitCode::FAILURE;
    }
    match run(path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn text(value: &str) -> DocValue {
    DocValue::Text(value.to_owned())
}

fn values<const N: usize>(pairs: [(&str, DocValue); N]) -> DocValueMap {
    pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

fn run(path: &str) -> Result<()> {
    let today = today();
    let mut options = SetupWizardOptions::for_country("United States", today);
    options.company_name = "Copperleaf Coffee".into();
    options.fullname = "Maya Okafor".into();
    options.email = "maya@copperleaf.example".into();
    options.bank_name = BANK.into();
    options.fiscal_year_start = Date::new(today.year(), 1, 1)?;
    options.fiscal_year_end = Date::new(today.year(), 12, 31)?;
    let mut fyo = setup_instance(path, &options, "screenshot_test")?;
    let mut rng = fastrand::Rng::with_seed(7);

    make_masters(&mut fyo)?;

    // Thirteen months, ending with this one: the dashboard shows the last twelve, and the
    // month before them gives the first one payments coming in.
    let first = (today.first_of_month() - 12.months()).first_of_month();
    let mut invoices: Vec<(Date, Doc)> = Vec::new();
    let mut journal_entries: Vec<Doc> = Vec::new();
    for m in 0..13 {
        let start = first + m.months();
        let end = if m == 12 {
            today
        } else {
            start.last_of_month()
        };
        let season = SEASON[start.month() as usize - 1];
        // Steady growth, from about $28,000 a month to about $55,000.
        let target = (28_000.0 + 2_200.0 * f64::from(m)) * season * f64::from(end.day())
            / f64::from(start.last_of_month().day());

        let mut revenue = 0.0;
        while revenue < target {
            let date = start.with().day(rng.i8(1..=end.day())).build()?;
            let (doc, amount) = sales_invoice(&mut fyo, &mut rng, date)?;
            revenue += amount;
            invoices.push((date, doc));
        }

        // Green coffee and packaging, bought on the 1st and the 15th for about 36% of sales.
        for (day, share) in [(1, 0.55), (15, 0.45)] {
            let date = start.with().day(day).build()?;
            if date > today {
                continue;
            }
            let budget = revenue * 0.36 * share;
            let rows = [
                ("Green Coffee, Ethiopia", 0.30),
                ("Green Coffee, Colombia", 0.35),
                ("Green Coffee, Brazil", 0.25),
            ];
            let mut doc = purchase_invoice(&mut fyo, "Altura Green Coffee Imports", date)?;
            for (item, part) in rows {
                let rate = purchase_rate(item);
                let qty = (budget * part / rate / 10.0).round() * 10.0;
                add_row(&mut fyo, &mut doc, item, qty, None)?;
            }
            invoices.push((date, doc));
            let mut doc = purchase_invoice(&mut fyo, "Kraft Pack Supply", date)?;
            let qty = (budget * 0.10 / 0.45 / 100.0).round() * 100.0;
            add_row(&mut fyo, &mut doc, "Bags and Labels", qty, None)?;
            invoices.push((date, doc));
        }

        // Overheads on the 1st: rent, power and gas, and ads, heavier before the holidays
        // and the summer, and heaviest for a new-year campaign that runs at a loss.
        let date = start;
        let mut doc = purchase_invoice(&mut fyo, "Wharfside Properties", date)?;
        add_row(&mut fyo, &mut doc, "Roastery Rent", 1.0, None)?;
        invoices.push((date, doc));
        let mut doc = purchase_invoice(&mut fyo, "Bay City Power", date)?;
        let power = (1100.0 * (rng.f64() * 0.3 + 0.85)).round();
        add_row(&mut fyo, &mut doc, "Electricity and Gas", 1.0, Some(power))?;
        invoices.push((date, doc));
        let mut doc = purchase_invoice(&mut fyo, "Brightline Media", date)?;
        let ads = match start.month() {
            1 => 9000.0,
            2 => 5000.0,
            6 | 11 | 12 => 3200.0,
            _ => 1500.0,
        };
        let ads = (ads * (rng.f64() * 0.3 + 0.85)).round();
        add_row(&mut fyo, &mut doc, "Online Ads", ads, None)?;
        invoices.push((date, doc));

        // Payroll at the end of each finished month.
        if m < 12 {
            journal_entries.push(payroll(&mut fyo, start.last_of_month(), 11_000.0)?);
        }
    }

    invoices.sort_by_key(|(date, _)| *date);
    let total = invoices.len();
    let mut payments: Vec<(Date, Doc)> = Vec::new();
    for (i, (date, mut doc)) in invoices.into_iter().enumerate() {
        if i % 50 == 0 {
            println!("submitting invoices, {i} of {total}");
        }
        doc.sync(&mut fyo)?;
        doc.submit(&mut fyo)?;

        // Customers pay 10 to 45 days later, suppliers are paid 5 to 25 days later, and
        // anything not due by today is still owed.
        let is_sales = doc.schema_name() == "SalesInvoice";
        let days = if is_sales {
            rng.i64(10..=45)
        } else {
            rng.i64(5..=25)
        };
        let paid_on = date + days.days();
        if paid_on > today {
            continue;
        }
        let name = doc.name().unwrap_or_default().to_owned();
        let invoice = fyo.get_doc(doc.schema_name(), Some(&name))?;
        if let Some(mut payment) = get_payment(&invoice, &fyo)? {
            let at = paid_on.at(12, 0, 0, 0).to_zoned(TimeZone::system())?;
            payment.set(&mut fyo, "date", DocValue::Date(at.timestamp()))?;
            payments.push((paid_on, payment));
        }
    }

    for mut doc in journal_entries {
        doc.sync(&mut fyo)?;
        doc.submit(&mut fyo)?;
    }

    payments.sort_by_key(|(date, _)| *date);
    println!("submitting {} payments", payments.len());
    for (_, mut doc) in payments {
        doc.sync(&mut fyo)?;
        doc.submit(&mut fyo)?;
    }

    let mut settings = fyo.get_doc("SystemSettings", None)?;
    settings.set(&mut fyo, "hideGetStarted", DocValue::Bool(true))?;
    settings.sync(&mut fyo)?;

    println!("created {path}");
    Ok(())
}

fn make_masters(fyo: &mut Fyo) -> Result<()> {
    for (name, unit, rate, account) in SALES_ITEMS {
        let item_type = if unit == "Hour" { "Service" } else { "Product" };
        let mut doc = fyo.get_new_doc(
            "Item",
            values([
                ("name", text(name)),
                ("unit", text(unit)),
                ("itemType", text(item_type)),
                ("for", text("Sales")),
                ("incomeAccount", text(account)),
                ("expenseAccount", text("Cost of Goods Sold")),
                ("rate", DocValue::Money(fyo.pesa().from_f64(rate)?)),
            ]),
        )?;
        doc.sync(fyo)?;
    }
    for (name, unit, rate, account, _) in PURCHASE_ITEMS {
        let mut doc = fyo.get_new_doc(
            "Item",
            values([
                ("name", text(name)),
                ("unit", text(unit)),
                ("itemType", text("Product")),
                ("for", text("Purchases")),
                ("incomeAccount", text("Sales")),
                ("expenseAccount", text(account)),
                ("rate", DocValue::Money(fyo.pesa().from_f64(rate)?)),
            ]),
        )?;
        doc.sync(fyo)?;
    }
    let mut suppliers: Vec<&str> = PURCHASE_ITEMS.iter().map(|i| i.4).collect();
    suppliers.dedup();
    for (names, role) in [(&CUSTOMERS[..], "Customer"), (&suppliers[..], "Supplier")] {
        for name in names {
            let mut doc = fyo.get_new_doc(
                "Party",
                values([("name", text(name)), ("role", text(role))]),
            )?;
            doc.sync(fyo)?;
        }
    }
    Ok(())
}

fn purchase_rate(item: &str) -> f64 {
    PURCHASE_ITEMS
        .iter()
        .find(|i| i.0 == item)
        .map(|i| i.2)
        .unwrap_or(1.0)
}

fn add_row(fyo: &mut Fyo, doc: &mut Doc, item: &str, qty: f64, rate: Option<f64>) -> Result<()> {
    let idx = doc.table("items").len();
    doc.append(fyo, "items", DocValueMap::new())?;
    doc.set_child(fyo, "items", idx, "item", text(item))?;
    if let Some(rate) = rate {
        let rate = fyo.pesa().from_f64(rate)?;
        doc.set_child(fyo, "items", idx, "rate", DocValue::Money(rate))?;
    }
    doc.set_child(fyo, "items", idx, "quantity", DocValue::Number(qty))?;
    Ok(())
}

/// A wholesale order: one customer, two to four coffees, and now and then some training.
fn sales_invoice(fyo: &mut Fyo, rng: &mut fastrand::Rng, date: Date) -> Result<(Doc, f64)> {
    let customer = CUSTOMERS[rng.usize(..CUSTOMERS.len())];
    let mut doc = fyo.get_new_doc("SalesInvoice", values([("date", text(&date.to_string()))]))?;
    doc.set(fyo, "party", text(customer))?;
    if matches!(doc.get("account"), None | Some(DocValue::Null)) {
        doc.set(fyo, "account", text("Debtors"))?;
    }
    let mut amount = 0.0;
    let mut picked: Vec<usize> = Vec::new();
    for _ in 0..rng.usize(2..=4) {
        let i = rng.usize(..SALES_ITEMS.len() - 1);
        if picked.contains(&i) {
            continue;
        }
        picked.push(i);
        let (name, unit, rate, _) = SALES_ITEMS[i];
        let qty = if unit == "Kg" {
            f64::from(rng.u32(4..=24)) * 2.5
        } else {
            f64::from(rng.u32(2..=12)) * 6.0
        };
        add_row(fyo, &mut doc, name, qty, None)?;
        amount += qty * rate;
    }
    if rng.f64() < 0.08 {
        let hours = f64::from(rng.u32(2..=6));
        add_row(fyo, &mut doc, "Barista Training", hours, None)?;
        amount += hours * 95.0;
    }
    Ok((doc, amount))
}

fn purchase_invoice(fyo: &mut Fyo, supplier: &str, date: Date) -> Result<Doc> {
    let mut doc = fyo.get_new_doc(
        "PurchaseInvoice",
        values([("date", text(&date.to_string()))]),
    )?;
    doc.set(fyo, "party", text(supplier))?;
    if matches!(doc.get("account"), None | Some(DocValue::Null)) {
        doc.set(fyo, "account", text("Creditors"))?;
    }
    Ok(doc)
}

fn payroll(fyo: &mut Fyo, date: Date, amount: f64) -> Result<Doc> {
    let amount = fyo.pesa().from_f64(amount)?;
    let zero = fyo.pesa().zero();
    let row = |account: &str, debit: &_, credit: &_| {
        values([
            ("account", text(account)),
            ("debit", DocValue::Money(Clone::clone(debit))),
            ("credit", DocValue::Money(Clone::clone(credit))),
        ])
    };
    let rows = vec![row("Salary", &amount, &zero), row("Cash", &zero, &amount)];
    Ok(fyo.get_new_doc(
        "JournalEntry",
        values([
            ("date", text(&date.to_string())),
            ("entryType", text("Cash Entry")),
            ("userRemark", text("Payroll")),
            ("accounts", DocValue::Table(rows)),
        ]),
    )?)
}
