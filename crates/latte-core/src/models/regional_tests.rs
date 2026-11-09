// Tests of India's regional models, the Address model and the GSTR reports.

use jiff::civil::date;
use jiff::tz::TimeZone;
use latte_db::DocValue;
use serde_json::{Value, json};

use crate::fyo::format::Formatter;
use crate::fyo::model::types::DocContext;
use crate::models::base_models::invoice::get_return_doc;
use crate::models::helpers;
use crate::models::invoice_tests::{posting, postings};
use crate::reports::gstr::{Filing, Gstr, Gstr3b, Period, reverse_charge};
use crate::reports::{FilterValue, RawValue, Report};
use crate::setup_tests::{TempFile, indian_company};
use crate::tests::{map, text};
use crate::{Doc, Fyo};

#[test]
fn an_indian_address_shows_its_parts_and_its_place_of_supply() {
    let file = TempFile::new("regional-address");
    let mut fyo = indian_company(&file);
    let values = map([
        ("addressLine1", text("12 MG Road")),
        ("city", text("Bengaluru")),
        ("state", text("Karnataka")),
        ("country", text("India")),
        ("postalCode", text("560001")),
    ]);
    let mut address = fyo.get_new_doc("Address", values).unwrap();
    address.sync(&mut fyo).unwrap();
    assert_eq!(
        address.get("addressDisplay"),
        Some(&text("12 MG Road, Bengaluru, Karnataka, India, 560001"))
    );
    assert_eq!(address.get("pos"), Some(&text("Karnataka")));

    let model = fyo.model("Address");
    let ctx = DocContext {
        doc: &address,
        parent: None,
        fyo: &fyo,
    };
    let states = model.list("state", Some(&ctx)).unwrap();
    assert_eq!(states.len(), 36);
    assert_eq!(states[0].value, "Andaman and Nicobar Islands");
    assert_eq!(model.list("pos", Some(&ctx)).unwrap().len(), 36);
    // A state is one of the country's, so India's are the only ones listed.
    let result = address.set(&mut fyo, "state", text("Bavaria"));
    assert_eq!(
        result.unwrap_err().to_string(),
        "Invalid value Bavaria for State"
    );
    address.set(&mut fyo, "country", text("Germany")).unwrap();
    address.set(&mut fyo, "state", text("Bavaria")).unwrap();
    assert_eq!(address.get("pos"), Some(&text("")));

    assert!(
        model
            .list("country", None)
            .unwrap()
            .iter()
            .any(|country| country.value == "Switzerland")
    );
}

#[test]
fn an_indian_party_keeps_a_gstin_only_when_registered_regular() {
    let file = TempFile::new("regional-party");
    let mut fyo = indian_company(&file);
    let values = map([
        ("name", text("Bharat Traders")),
        ("role", text("Customer")),
        ("gstType", text("Unregistered")),
        ("gstin", text("29ABCDE1234F1Z5")),
    ]);
    let mut party = fyo.get_new_doc("Party", values).unwrap();
    {
        let model = fyo.model("Party");
        let ctx = DocContext {
            doc: &party,
            parent: None,
            fyo: &fyo,
        };
        assert_eq!(model.hidden("gstin", &ctx), Some(true));
        assert_eq!(model.hidden("loyaltyProgram", &ctx), Some(true));
        assert_eq!(model.hidden("loyaltyPoints", &ctx), Some(true));
    }
    party.sync(&mut fyo).unwrap();
    assert_eq!(party.get("gstin"), Some(&text("")));

    party
        .set(&mut fyo, "gstType", text("Registered Regular"))
        .unwrap();
    party
        .set(&mut fyo, "gstin", text("29ABCDE1234F1Z5"))
        .unwrap();
    party.sync(&mut fyo).unwrap();
    assert_eq!(party.get("gstin"), Some(&text("29ABCDE1234F1Z5")));
    // The base Party's formulas still apply.
    assert_eq!(party.get("defaultAccount"), Some(&text("Debtors")));
}

/// A customer, with the address it's at if any.
fn customer(fyo: &mut Fyo, name: &str, gstin: Option<&str>, state: Option<&str>) {
    let mut values = map([("name", text(name)), ("role", text("Customer"))]);
    if let Some(gstin) = gstin {
        values.insert("gstType".into(), text("Registered Regular"));
        values.insert("gstin".into(), text(gstin));
    } else {
        values.insert("gstType".into(), text("Unregistered"));
    }
    if let Some(state) = state {
        let address = map([
            ("addressLine1", text("1 Main Road")),
            ("city", text("Town")),
            ("state", text(state)),
            ("country", text("India")),
        ]);
        let mut address = fyo.get_new_doc("Address", address).unwrap();
        address.sync(fyo).unwrap();
        values.insert("address".into(), address.get("name").cloned().unwrap());
    }
    fyo.get_new_doc("Party", values).unwrap().sync(fyo).unwrap();
}

/// A submitted sales invoice on `date` of `rows`: item, quantity and tax.
fn invoice(fyo: &mut Fyo, party: &str, date: DocValue, rows: &[(&str, f64, &str)]) -> Doc {
    let values = map([("party", text(party)), ("date", date)]);
    let mut doc = fyo.get_new_doc("SalesInvoice", values).unwrap();
    for (idx, (item, quantity, tax)) in rows.iter().enumerate() {
        doc.append(fyo, "items", Default::default()).unwrap();
        doc.set_child(fyo, "items", idx, "item", text(item))
            .unwrap();
        doc.set_child(fyo, "items", idx, "quantity", DocValue::Number(*quantity))
            .unwrap();
        if !tax.is_empty() {
            doc.set_child(fyo, "items", idx, "tax", text(tax)).unwrap();
        }
    }
    doc.sync(fyo).unwrap();
    doc.submit(fyo).unwrap();
    doc
}

/// A submitted sales invoice of `quantity` Widgets at 100 with the tax.
fn sale(fyo: &mut Fyo, party: &str, quantity: f64, tax: &str) -> Doc {
    invoice(fyo, party, text("2026-10-01"), &[("Widget", quantity, tax)])
}

/// A submitted return on `day` of `quantity` of the first row of `name`.
fn return_of(fyo: &mut Fyo, name: &str, quantity: f64, day: jiff::civil::Date) -> Doc {
    let invoice = fyo.get_doc("SalesInvoice", Some(name)).unwrap();
    let mut doc = get_return_doc(&invoice, fyo).unwrap().unwrap();
    let midnight = day.to_zoned(TimeZone::system()).unwrap().timestamp();
    doc.set(fyo, "date", DocValue::Date(midnight)).unwrap();
    doc.set_child(fyo, "items", 0, "quantity", DocValue::Number(-quantity))
        .unwrap();
    doc.sync(fyo).unwrap();
    doc.submit(fyo).unwrap();
    doc
}

/// An item at `rate` with an HSN/SAC code, if any, sold in `unit`.
fn item(fyo: &mut Fyo, name: &str, rate: f64, hsn: Option<&str>, unit: &str) {
    let mut values = map([
        ("name", text(name)),
        ("rate", DocValue::Number(rate)),
        ("unit", text(unit)),
    ]);
    if let Some(hsn) = hsn {
        values.insert("hsnCode".into(), text(hsn));
    }
    fyo.get_new_doc("Item", values).unwrap().sync(fyo).unwrap();
}

/// A company in Karnataka with a GSTIN.
fn registered_company(file: &TempFile) -> Fyo {
    let mut fyo = indian_company(file);
    fyo.db
        .update(
            "AccountingSettings",
            &map([("gstin", text("29AABCA1234A1Z5"))]),
        )
        .unwrap();
    fyo
}

/// Stores an invoice's date as Books does: the instant, in UTC, of that hour of the day
/// in the computer's time zone.
fn store_date_as_books(fyo: &Fyo, name: &str, day: jiff::civil::Date, hour: i8) {
    let zoned = day.at(hour, 0, 0, 0).to_zoned(TimeZone::system()).unwrap();
    let stored = zoned
        .timestamp()
        .strftime("%Y-%m-%dT%H:%M:%S.000Z")
        .to_string();
    fyo.db
        .core()
        .connection()
        .execute(
            "update `SalesInvoice` set `date` = ?1 where `name` = ?2",
            [stored.as_str(), name],
        )
        .unwrap();
}

/// The report's rows for the period, and its GSTR-1 export.
fn gstr_1(
    fyo: &Fyo,
    from: jiff::civil::Date,
    to: jiff::civil::Date,
) -> (Vec<String>, Result<Value, String>) {
    let mut report = Gstr::new();
    report.from_date = Some(from);
    report.to_date = Some(to);
    let formatter = Formatter::new(fyo).unwrap();
    report.run(fyo, &formatter).unwrap();
    let names = report.rows.iter().map(|row| row.inv_no.clone()).collect();
    let json = report.export_json(fyo).unwrap();
    (
        names,
        json.map(|json| serde_json::from_str(&json).unwrap())
            .map_err(|e| e.to_string()),
    )
}

/// A company in Karnataka that has sold to a registered customer there and one in
/// Maharashtra, and to an unregistered one.
fn gst_company() -> (TempFile, Fyo) {
    let file = TempFile::new("regional-gstr");
    let mut fyo = indian_company(&file);
    let values = map([
        ("name", text("Widget")),
        ("rate", DocValue::Number(100.0)),
        ("hsnCode", text("8471")),
    ]);
    fyo.get_new_doc("Item", values)
        .unwrap()
        .sync(&mut fyo)
        .unwrap();
    customer(&mut fyo, "Local Co", Some("29AABCL1234A1Z5"), None);
    customer(&mut fyo, "Mumbai Co", Some("27AABCM1234A1Z5"), None);
    customer(&mut fyo, "Walk In", None, Some("Karnataka"));
    sale(&mut fyo, "Local Co", 2.0, "GST-18");
    sale(&mut fyo, "Mumbai Co", 1.0, "IGST-18");
    sale(&mut fyo, "Walk In", 1.0, "GST-5");
    (file, fyo)
}

fn gstr(transfer_type: &str) -> Gstr {
    let mut report = Gstr::new();
    report.transfer_type = Some(transfer_type.to_owned());
    report.from_date = Some(date(2026, 7, 1));
    report.to_date = Some(date(2026, 10, 31));
    report
}

#[test]
fn gstr_1_lists_each_sale_with_its_gst_once() {
    let (_file, mut fyo) = gst_company();
    fyo.db
        .update(
            "AccountingSettings",
            &map([("gstin", text("29AABCA1234A1Z5"))]),
        )
        .unwrap();

    let mut report = gstr("B2B");
    let formatter = Formatter::new(&fyo).unwrap();
    let rows = report.run(&fyo, &formatter).unwrap();
    assert_eq!(rows.len(), 2);
    let mut gstr_rows = report.rows.clone();
    gstr_rows.sort_by(|a, b| a.party_name.cmp(&b.party_name));
    let [local, mumbai] = gstr_rows.as_slice() else {
        panic!("{gstr_rows:?}");
    };

    assert_eq!(local.place, "Karnataka");
    assert!(local.in_state);
    // Books adds the rates up twice, giving 36.
    assert_eq!(local.rate, 18.0);
    assert_eq!(local.tax_val, 200.0);
    assert_eq!(local.inv_amt, 236.0);
    assert_eq!((local.cgst_amt, local.sgst_amt), (Some(18.0), Some(18.0)));
    assert_eq!(local.igst_amt, None);
    assert_eq!(local.reverse_charge, "N");
    // Books' `switch` falls through, marking every invoice nil rated and exempt.
    assert!(!local.nil_rated && !local.exempt && !local.non_gst);

    assert_eq!(mumbai.place, "Maharashtra");
    assert!(!mumbai.in_state);
    assert_eq!(mumbai.rate, 18.0);
    assert_eq!(mumbai.igst_amt, Some(18.0));
    assert_eq!((mumbai.cgst_amt, mumbai.sgst_amt), (None, None));

    let columns = report.columns(&fyo, &formatter).unwrap();
    assert_eq!(columns[0].label, "GSTIN No.");
    assert_eq!(rows[0].cells.len(), columns.len());

    let json: serde_json::Value =
        serde_json::from_str(&report.export_json(&fyo).unwrap().unwrap()).unwrap();
    assert_eq!(json["version"], "GST3.2.4");
    assert_eq!(json["gstin"], "29AABCA1234A1Z5");
    assert_eq!(json["fp"], "102026");
    let b2b = json["b2b"].as_array().unwrap();
    assert_eq!(b2b.len(), 2);
    let local = b2b.iter().find(|c| c["ctin"] == "29AABCL1234A1Z5").unwrap();
    let inv = &local["inv"][0];
    assert_eq!(inv["idt"], "01-10-2026");
    assert_eq!(inv["pos"], "29");
    assert_eq!(inv["val"], 236);
    // The row's serial number, not its HSN code, and no integrated tax within the state.
    assert_eq!(
        inv["itms"][0],
        json!({
            "num": 1,
            "itm_det": { "rt": 18, "txval": 200, "camt": 18, "samt": 18, "csamt": 0 },
        })
    );
    let mumbai = b2b.iter().find(|c| c["ctin"] == "27AABCM1234A1Z5").unwrap();
    assert_eq!(mumbai["inv"][0]["pos"], "27");
    assert_eq!(
        mumbai["inv"][0]["itms"][0]["itm_det"],
        json!({ "rt": 18, "txval": 100, "iamt": 18, "csamt": 0 })
    );

    // Unregistered and in the state, so a small B2C supply.
    let mut report = gstr("B2CS");
    report.run(&fyo, &formatter).unwrap();
    assert_eq!(report.rows.len(), 1);
    assert_eq!(report.rows[0].party_name, "Walk In");
    // Books says Y for a buyer without a GSTIN, which is no reverse charge.
    assert_eq!(report.rows[0].reverse_charge, "N");
    let json: serde_json::Value =
        serde_json::from_str(&report.export_json(&fyo).unwrap().unwrap()).unwrap();
    assert_eq!(
        json["b2cs"],
        json!([{
            "sply_ty": "INTRA", "pos": "29", "typ": "OE", "rt": 5, "txval": 100,
            "camt": 2.5, "samt": 2.5, "csamt": 0,
        }])
    );
    // The export is the whole return, whichever transfer type the report shows.
    assert_eq!(json["b2b"].as_array().unwrap().len(), 2);

    // A place filters by its state code.
    let mut report = gstr("B2B");
    report.place = Some("27".to_owned());
    report.run(&fyo, &formatter).unwrap();
    assert_eq!(report.rows.len(), 1);
    assert_eq!(report.rows[0].party_name, "Mumbai Co");
}

#[test]
fn gstr_can_be_exported_only_with_the_company_gstin() {
    let (_file, fyo) = gst_company();
    let mut report = gstr("B2B");
    let formatter = Formatter::new(&fyo).unwrap();
    report.run(&fyo, &formatter).unwrap();
    assert_eq!(
        report.can_export(&fyo).unwrap_err().to_string(),
        "Cannot Export: Please set GSTIN in GST Settings."
    );
    assert!(report.export_json(&fyo).unwrap().is_err());
}

#[test]
fn gstr_includes_the_invoices_of_both_ends_of_the_period() {
    let file = TempFile::new("regional-gstr-period");
    let mut fyo = registered_company(&file);
    item(&mut fyo, "Widget", 100.0, Some("8471"), "Unit");
    customer(&mut fyo, "Local Co", Some("29AABCL1234A1Z5"), None);
    let rows = [("Widget", 1.0, "GST-18")];
    // Early on the From Date, which is still the day before in UTC east of it, and late
    // on the To Date, which Books stores after that day's midnight in UTC.
    for (name, day, hour) in [
        ("SINV-1001", date(2026, 7, 1), 1),
        ("SINV-1002", date(2026, 9, 30), 18),
        ("SINV-1003", date(2026, 6, 30), 23),
        ("SINV-1004", date(2026, 10, 1), 1),
    ] {
        invoice(&mut fyo, "Local Co", text("2026-08-01"), &rows);
        store_date_as_books(&fyo, name, day, hour);
    }

    let (mut names, json) = gstr_1(&fyo, date(2026, 7, 1), date(2026, 9, 30));
    names.sort();
    assert_eq!(names, ["SINV-1001", "SINV-1002"]);
    let json = json.unwrap();
    assert_eq!(json["fp"], "092026");
    let invoices = json["b2b"][0]["inv"].as_array().unwrap();
    let dates: Vec<_> = invoices
        .iter()
        .map(|inv| (&inv["inum"], &inv["idt"]))
        .collect();
    assert_eq!(
        dates,
        [
            (&json!("SINV-1001"), &json!("01-07-2026")),
            (&json!("SINV-1002"), &json!("30-09-2026")),
        ]
    );
    assert_eq!(json["doc_issue"]["doc_det"][0]["docs"][0]["totnum"], 2);
}

#[test]
fn gstr_1_export_has_an_item_per_rate_and_the_customers_place_of_supply() {
    let file = TempFile::new("regional-gstr-rates");
    let mut fyo = registered_company(&file);
    item(&mut fyo, "Widget", 100.0, Some("8471"), "Unit");
    item(&mut fyo, "Gadget", 50.0, Some("8517"), "Unit");
    item(&mut fyo, "Consulting", 1000.0, Some("998314"), "Hour");
    customer(&mut fyo, "Local Co", Some("29AABCL1234A1Z5"), None);
    // Registered in Karnataka, supplied in Maharashtra.
    customer(
        &mut fyo,
        "Branch Co",
        Some("29AABCB1234A1Z5"),
        Some("Maharashtra"),
    );
    let day = text("2026-08-10");
    invoice(
        &mut fyo,
        "Local Co",
        day.clone(),
        &[
            ("Widget", 2.0, "GST-18"),
            ("Gadget", 1.0, "GST-5"),
            ("Widget", 1.0, "GST-18"),
        ],
    );
    invoice(
        &mut fyo,
        "Branch Co",
        day,
        &[("Consulting", 2.0, "IGST-18")],
    );

    let json = gstr_1(&fyo, date(2026, 7, 1), date(2026, 9, 30)).1.unwrap();
    assert_eq!(
        json["b2b"],
        json!([
            {
                "ctin": "29AABCB1234A1Z5",
                "inv": [{
                    "inum": "SINV-1002", "idt": "10-08-2026", "val": 2360, "pos": "27",
                    "rchrg": "N", "inv_typ": "R",
                    "itms": [
                        { "num": 1, "itm_det": { "rt": 18, "txval": 2000, "iamt": 360, "csamt": 0 } },
                    ],
                }],
            },
            {
                "ctin": "29AABCL1234A1Z5",
                "inv": [{
                    "inum": "SINV-1001", "idt": "10-08-2026", "val": 406.5, "pos": "29",
                    "rchrg": "N", "inv_typ": "R",
                    "itms": [
                        { "num": 1, "itm_det": { "rt": 5, "txval": 50, "camt": 1.25, "samt": 1.25, "csamt": 0 } },
                        { "num": 2, "itm_det": { "rt": 18, "txval": 300, "camt": 27, "samt": 27, "csamt": 0 } },
                    ],
                }],
            },
        ])
    );
    // A service's unit is NA and its quantity 0.
    assert_eq!(
        json["hsn"],
        json!({ "hsn_b2b": [
            {
                "num": 1, "hsn_sc": "8471", "desc": "Widget", "uqc": "UNT", "qty": 3, "rt": 18,
                "txval": 300, "camt": 27, "samt": 27, "csamt": 0,
            },
            {
                "num": 2, "hsn_sc": "8517", "desc": "Gadget", "uqc": "UNT", "qty": 1, "rt": 5,
                "txval": 50, "camt": 1.25, "samt": 1.25, "csamt": 0,
            },
            {
                "num": 3, "hsn_sc": "998314", "desc": "Consulting", "uqc": "NA", "qty": 0,
                "rt": 18, "txval": 2000, "iamt": 360, "csamt": 0,
            },
        ] })
    );
    assert_eq!(
        json["doc_issue"],
        json!({ "doc_det": [{ "doc_num": 1, "docs": [{
            "num": 1, "from": "SINV-1001", "to": "SINV-1002", "totnum": 2, "cancel": 0,
            "net_issue": 2,
        }] }] })
    );
    assert!(json.get("b2cs").is_none() && json.get("nil").is_none());
}

#[test]
fn gstr_1_export_sums_small_b2c_sales_and_lists_large_ones() {
    let file = TempFile::new("regional-gstr-b2c");
    let mut fyo = registered_company(&file);
    item(&mut fyo, "Widget", 100.0, Some("8471"), "Unit");
    item(&mut fyo, "Laptop", 60000.0, Some("8471"), "Unit");
    customer(&mut fyo, "Walk In", None, Some("Karnataka"));
    customer(&mut fyo, "Delhi Buyer", None, Some("Delhi"));
    let day = text("2026-08-10");
    invoice(
        &mut fyo,
        "Walk In",
        day.clone(),
        &[("Widget", 1.0, "GST-5")],
    );
    invoice(
        &mut fyo,
        "Walk In",
        day.clone(),
        &[("Widget", 3.0, "GST-5")],
    );
    // ₹1,41,600 between states: over the ₹1 lakh limit.
    invoice(
        &mut fyo,
        "Delhi Buyer",
        day.clone(),
        &[("Laptop", 2.0, "IGST-18")],
    );
    invoice(&mut fyo, "Delhi Buyer", day, &[("Widget", 1.0, "IGST-18")]);

    let (mut names, json) = gstr_1(&fyo, date(2026, 7, 1), date(2026, 9, 30));
    names.sort();
    // The report shows B2B by default, and none of these are.
    assert!(names.is_empty());
    let json = json.unwrap();
    assert_eq!(
        json["b2cl"],
        json!([{ "pos": "07", "inv": [{
            "inum": "SINV-1003", "idt": "10-08-2026", "val": 141600,
            "itms": [{ "num": 1, "itm_det": { "rt": 18, "txval": 120000, "iamt": 21600, "csamt": 0 } }],
        }] }])
    );
    assert_eq!(
        json["b2cs"],
        json!([
            { "sply_ty": "INTER", "pos": "07", "typ": "OE", "rt": 18, "txval": 100, "iamt": 18, "csamt": 0 },
            { "sply_ty": "INTRA", "pos": "29", "typ": "OE", "rt": 5, "txval": 400, "camt": 10, "samt": 10, "csamt": 0 },
        ])
    );
    // The laptops and widgets share a code and a rate.
    assert_eq!(json["hsn"]["hsn_b2b"], json!([]));
    assert_eq!(json["hsn"]["hsn_b2c"].as_array().unwrap().len(), 2);
    assert_eq!(json["hsn"]["hsn_b2c"][1]["desc"], "Laptop, Widget");

    let mut report = gstr("B2CL");
    report.from_date = Some(date(2026, 7, 1));
    report.to_date = Some(date(2026, 9, 30));
    let formatter = Formatter::new(&fyo).unwrap();
    report.run(&fyo, &formatter).unwrap();
    assert_eq!(report.rows.len(), 1);
    assert_eq!(report.rows[0].inv_no, "SINV-1003");

    // Before August 2024 the limit was ₹2.5 lakh.
    invoice(
        &mut fyo,
        "Delhi Buyer",
        text("2024-07-15"),
        &[("Laptop", 2.0, "IGST-18")],
    );
    let json = gstr_1(&fyo, date(2024, 7, 1), date(2024, 7, 31)).1.unwrap();
    assert!(json.get("b2cl").is_none());
    assert_eq!(json["b2cs"][0]["txval"], 120000);
}

#[test]
fn gstr_1_export_files_returns_as_credit_notes() {
    let file = TempFile::new("regional-gstr-returns");
    let mut fyo = registered_company(&file);
    item(&mut fyo, "Widget", 100.0, Some("8471"), "Unit");
    item(&mut fyo, "Laptop", 60000.0, Some("8471"), "Unit");
    customer(&mut fyo, "Local Co", Some("29AABCL1234A1Z5"), None);
    customer(&mut fyo, "Walk In", None, Some("Karnataka"));
    customer(&mut fyo, "Delhi Buyer", None, Some("Delhi"));
    let sold = text("2026-08-10");
    let returned = date(2026, 8, 20);
    invoice(
        &mut fyo,
        "Local Co",
        sold.clone(),
        &[("Widget", 3.0, "GST-18")],
    );
    return_of(&mut fyo, "SINV-1001", 1.0, returned);
    invoice(
        &mut fyo,
        "Walk In",
        sold.clone(),
        &[("Widget", 2.0, "GST-5")],
    );
    return_of(&mut fyo, "SINV-1003", 1.0, returned);
    invoice(
        &mut fyo,
        "Delhi Buyer",
        sold.clone(),
        &[("Laptop", 2.0, "IGST-18")],
    );
    return_of(&mut fyo, "SINV-1005", 1.0, returned);
    let mut cancelled = invoice(&mut fyo, "Local Co", sold, &[("Widget", 1.0, "GST-18")]);
    cancelled.cancel(&mut fyo).unwrap();

    let json = gstr_1(&fyo, date(2026, 7, 1), date(2026, 9, 30)).1.unwrap();
    // The invoice keeps its value; the return is a credit note of its own.
    assert_eq!(json["b2b"][0]["inv"][0]["val"], 354);
    assert_eq!(json["b2b"][0]["inv"].as_array().unwrap().len(), 1);
    assert_eq!(
        json["cdnr"],
        json!([{ "ctin": "29AABCL1234A1Z5", "nt": [{
            "ntty": "C", "nt_num": "SINV-1002", "nt_dt": "20-08-2026", "val": 118,
            "pos": "29", "rchrg": "N", "inv_typ": "R",
            "itms": [{ "num": 1, "itm_det": { "rt": 18, "txval": 100, "camt": 9, "samt": 9, "csamt": 0 } }],
        }] }])
    );
    // A return of a B2C-Large sale, though for less than the limit itself.
    assert_eq!(
        json["cdnur"],
        json!([{
            "typ": "B2CL", "ntty": "C", "nt_num": "SINV-1006", "nt_dt": "20-08-2026",
            "val": 70800, "pos": "07",
            "itms": [{ "num": 1, "itm_det": { "rt": 18, "txval": 60000, "iamt": 10800, "csamt": 0 } }],
        }])
    );
    // A small sale's return comes off B2C-Small.
    assert_eq!(
        json["b2cs"],
        json!([{
            "sply_ty": "INTRA", "pos": "29", "typ": "OE", "rt": 5, "txval": 100,
            "camt": 2.5, "samt": 2.5, "csamt": 0,
        }])
    );
    assert_eq!(json["hsn"]["hsn_b2b"][0]["qty"], 2);
    assert_eq!(json["hsn"]["hsn_b2b"][0]["txval"], 200);
    assert_eq!(
        json["doc_issue"],
        json!({ "doc_det": [
            { "doc_num": 1, "docs": [{
                "num": 1, "from": "SINV-1001", "to": "SINV-1007", "totnum": 4, "cancel": 1,
                "net_issue": 3,
            }] },
            { "doc_num": 5, "docs": [{
                "num": 1, "from": "SINV-1002", "to": "SINV-1006", "totnum": 3, "cancel": 0,
                "net_issue": 3,
            }] },
        ] })
    );
}

#[test]
fn gstr_1_export_lists_nil_rated_and_exempt_supplies() {
    let file = TempFile::new("regional-gstr-nil");
    let mut fyo = registered_company(&file);
    item(&mut fyo, "Rice", 50.0, Some("1006"), "Kg");
    // Chapter 04, stored as a number without its leading zero.
    item(&mut fyo, "Milk", 50.0, Some("0401"), "Unit");
    customer(&mut fyo, "Local Co", Some("29AABCL1234A1Z5"), None);
    customer(&mut fyo, "Walk In", None, Some("Karnataka"));
    let day = text("2026-08-10");
    invoice(
        &mut fyo,
        "Walk In",
        day.clone(),
        &[("Rice", 2.0, "GST-0"), ("Milk", 1.0, "Exempt-GST-0")],
    );
    invoice(&mut fyo, "Local Co", day, &[("Rice", 1.0, "GST-0")]);

    let json = gstr_1(&fyo, date(2026, 7, 1), date(2026, 9, 30)).1.unwrap();
    assert!(json.get("b2b").is_none() && json.get("b2cs").is_none());
    assert_eq!(
        json["nil"],
        json!({ "inv": [
            { "sply_ty": "INTRAB2B", "nil_amt": 50, "expt_amt": 0, "ngsup_amt": 0 },
            { "sply_ty": "INTRAB2C", "nil_amt": 100, "expt_amt": 50, "ngsup_amt": 0 },
        ] })
    );
    assert_eq!(
        json["hsn"]["hsn_b2c"],
        json!([
            {
                "num": 1, "hsn_sc": "0401", "desc": "Milk", "uqc": "UNT", "qty": 1, "rt": 0,
                "txval": 50, "camt": 0, "samt": 0, "csamt": 0,
            },
            {
                "num": 2, "hsn_sc": "1006", "desc": "Rice", "uqc": "KGS", "qty": 2, "rt": 0,
                "txval": 100, "camt": 0, "samt": 0, "csamt": 0,
            },
        ])
    );
    assert_eq!(json["doc_issue"]["doc_det"][0]["docs"][0]["totnum"], 2);
}

#[test]
fn gstr_1_export_takes_rates_from_the_tax_and_values_in_rupees() {
    let file = TempFile::new("regional-gstr-tax");
    let mut fyo = registered_company(&file);
    item(&mut fyo, "Widget", 100.0, Some("8471"), "Unit");
    let details = DocValue::Table(vec![
        map([("account", text("CGST")), ("rate", DocValue::Number(9.0))]),
        map([("account", text("SGST")), ("rate", DocValue::Number(9.0))]),
    ]);
    let tax = map([("name", text("Standard 18%")), ("details", details)]);
    fyo.get_new_doc("Tax", tax).unwrap().sync(&mut fyo).unwrap();
    customer(&mut fyo, "Local Co", Some("29AABCL1234A1Z5"), None);
    invoice(
        &mut fyo,
        "Local Co",
        text("2026-08-10"),
        &[("Widget", 1.0, "Standard 18%")],
    );

    let values = map([
        ("name", text("US Co")),
        ("role", text("Customer")),
        ("gstType", text("Registered Regular")),
        ("gstin", text("29AABCU1234A1Z5")),
        ("currency", text("USD")),
    ]);
    fyo.get_new_doc("Party", values)
        .unwrap()
        .sync(&mut fyo)
        .unwrap();
    let values = map([("party", text("US Co")), ("date", text("2026-08-11"))]);
    let mut doc = fyo.get_new_doc("SalesInvoice", values).unwrap();
    doc.set(&mut fyo, "exchangeRate", DocValue::Number(80.0))
        .unwrap();
    doc.append(&mut fyo, "items", Default::default()).unwrap();
    doc.set_child(&mut fyo, "items", 0, "item", text("Widget"))
        .unwrap();
    doc.set_child(&mut fyo, "items", 0, "quantity", DocValue::Number(2.0))
        .unwrap();
    doc.set_child(&mut fyo, "items", 0, "tax", text("GST-18"))
        .unwrap();
    doc.sync(&mut fyo).unwrap();
    doc.submit(&mut fyo).unwrap();
    assert_eq!(crate::tests::money(&doc, "grandTotal"), "2.95");

    let json = gstr_1(&fyo, date(2026, 8, 1), date(2026, 8, 31)).1.unwrap();
    assert_eq!(json["b2b"][0]["ctin"], "29AABCL1234A1Z5");
    assert_eq!(
        json["b2b"][0]["inv"][0]["itms"],
        json!([{ "num": 1, "itm_det": { "rt": 18, "txval": 100, "camt": 9, "samt": 9, "csamt": 0 } }])
    );
    // $2.95 at 80 rupees to the dollar.
    let usd = &json["b2b"][1]["inv"][0];
    assert_eq!(usd["val"], 236);
    assert_eq!(
        usd["itms"][0]["itm_det"],
        json!({ "rt": 18, "txval": 200, "camt": 18, "samt": 18, "csamt": 0 })
    );
}

#[test]
fn gstr_1_export_needs_a_gst_tax_and_an_hsn_code_on_every_row() {
    let file = TempFile::new("regional-gstr-missing");
    let mut fyo = registered_company(&file);
    item(&mut fyo, "Widget", 100.0, Some("8471"), "Unit");
    item(&mut fyo, "Gizmo", 100.0, None, "Unit");
    customer(&mut fyo, "Local Co", Some("29AABCL1234A1Z5"), None);
    invoice(
        &mut fyo,
        "Local Co",
        text("2026-08-10"),
        &[("Gizmo", 1.0, "GST-18")],
    );
    invoice(
        &mut fyo,
        "Local Co",
        text("2026-09-10"),
        &[("Widget", 1.0, "")],
    );

    let august = || gstr_1(&fyo, date(2026, 8, 1), date(2026, 8, 31)).1;
    assert_eq!(
        august().unwrap_err(),
        "Cannot Export: Gizmo on SINV-1001 has no HSN/SAC code. Set it on the item."
    );
    let september = gstr_1(&fyo, date(2026, 9, 1), date(2026, 9, 30)).1;
    assert_eq!(
        september.unwrap_err(),
        "Cannot Export: row 1 of SINV-1002 (Widget) has no tax, so it can't be filed as \
         taxed, nil rated, exempt or non-GST."
    );

    // The item's code is used once it is set.
    fyo.db
        .update(
            "Item",
            &map([("name", text("Gizmo")), ("hsnCode", text("8471"))]),
        )
        .unwrap();
    let json = gstr_1(&fyo, date(2026, 8, 1), date(2026, 8, 31)).1.unwrap();
    assert_eq!(json["hsn"]["hsn_b2b"][0]["hsn_sc"], "8471");
}

#[test]
fn gstr_starts_at_the_latest_period_that_has_ended() {
    let today = date(2026, 11, 9);
    let mut report = Gstr::new();
    report.set_default_filters(Filing::Monthly, today);
    assert_eq!(report.fiscal_year, Some(2026));
    assert_eq!(report.period, Some(Period::Month(10)));
    assert_eq!(
        (report.from_date, report.to_date),
        (Some(date(2026, 10, 1)), Some(date(2026, 10, 31)))
    );
    assert_eq!(report.due_date(), Some(date(2026, 11, 11)));

    let mut report = Gstr::new();
    report.set_default_filters(Filing::Quarterly, today);
    assert_eq!(report.period, Some(Period::Quarter(2)));
    assert_eq!(
        (report.from_date, report.to_date),
        (Some(date(2026, 7, 1)), Some(date(2026, 9, 30)))
    );
    assert_eq!(report.due_date(), Some(date(2026, 10, 13)));

    // Dates alone are a Custom period, which has no due date.
    let mut report = Gstr::new();
    report.from_date = Some(date(2026, 8, 5));
    report.set_default_filters(Filing::Monthly, today);
    assert_eq!(report.period, Some(Period::Custom));
    assert_eq!(report.to_date, Some(today));
    assert_eq!(report.due_date(), None);
}

#[test]
fn gstr_filing_follows_the_return_filing_setting() {
    let file = TempFile::new("regional-gstr-filing");
    let mut fyo = registered_company(&file);
    assert_eq!(Filing::of(&fyo), Filing::Monthly);
    let fieldnames = |report: &Gstr, fyo: &Fyo| -> Vec<String> {
        report
            .filters(fyo)
            .into_iter()
            .map(|f| f.fieldname)
            .collect()
    };
    let periods = |report: &Gstr, fyo: &Fyo| -> Vec<(String, String)> {
        match report.filters(fyo).remove(1).kind {
            crate::reports::FilterKind::Select(options) => options,
            kind => panic!("{kind:?}"),
        }
    };

    let mut report = Gstr::new();
    report.set("fiscalYear", FilterValue::Text("2025-26".into()));
    report.set("period", FilterValue::Text("05".into()));
    assert_eq!(
        (report.from_date, report.to_date),
        (Some(date(2025, 5, 1)), Some(date(2025, 5, 31)))
    );
    assert_eq!(
        fieldnames(&report, &fyo),
        ["fiscalYear", "period", "transferType", "place"]
    );
    let options = periods(&report, &fyo);
    assert_eq!(options.len(), 13);
    assert_eq!(options[0], ("April 2025".to_owned(), "04".to_owned()));
    assert_eq!(options[11], ("March 2026".to_owned(), "03".to_owned()));
    assert_eq!(options[12], ("Custom".to_owned(), "custom".to_owned()));
    assert_eq!(
        report.get("fiscalYear"),
        FilterValue::Text("2025-26".into())
    );

    // Another year keeps the month.
    report.set("fiscalYear", FilterValue::Text("2024-25".into()));
    assert_eq!(report.from_date, Some(date(2024, 5, 1)));

    // Typing a date makes the period Custom, which shows the dates.
    report.set("toDate", FilterValue::Date(date(2024, 6, 15)));
    assert_eq!(report.period, Some(Period::Custom));
    assert_eq!(report.from_date, Some(date(2024, 5, 1)));
    assert_eq!(
        fieldnames(&report, &fyo),
        [
            "fiscalYear",
            "period",
            "transferType",
            "place",
            "fromDate",
            "toDate"
        ]
    );

    fyo.db
        .update(
            "AccountingSettings",
            &map([("gstReturnFiling", text("Quarterly"))]),
        )
        .unwrap();
    assert_eq!(Filing::of(&fyo), Filing::Quarterly);
    let mut report = Gstr::new();
    report.set("fiscalYear", FilterValue::Text("2025-26".into()));
    report.set("period", FilterValue::Text("Q3".into()));
    assert_eq!(
        (report.from_date, report.to_date),
        (Some(date(2025, 10, 1)), Some(date(2025, 12, 31)))
    );
    let labels: Vec<String> = periods(&report, &fyo).into_iter().map(|(l, _)| l).collect();
    assert_eq!(
        labels,
        [
            "Apr–Jun 2025",
            "Jul–Sep 2025",
            "Oct–Dec 2025",
            "Jan–Mar 2026",
            "Custom"
        ]
    );
}

#[test]
fn gstr_1_for_a_quarter_is_filed_for_its_last_month() {
    let (_file, mut fyo) = gst_company();
    fyo.db
        .update(
            "AccountingSettings",
            &map([("gstin", text("29AABCA1234A1Z5"))]),
        )
        .unwrap();
    let mut report = Gstr::new();
    report.set("fiscalYear", FilterValue::Text("2026-27".into()));
    report.set("period", FilterValue::Text("Q3".into()));
    let formatter = Formatter::new(&fyo).unwrap();
    report.run(&fyo, &formatter).unwrap();
    // The sales of 1 October 2026.
    assert_eq!(report.rows.len(), 2);
    let json: Value = serde_json::from_str(&report.export_json(&fyo).unwrap().unwrap()).unwrap();
    assert_eq!(json["fp"], "122026");
    assert_eq!(
        report.file_name(&fyo).as_deref(),
        Some("GSTR1_29AABCA1234A1Z5_122026")
    );

    report.set("fromDate", FilterValue::Date(date(2026, 10, 1)));
    report.set("toDate", FilterValue::Date(date(2026, 10, 15)));
    assert_eq!(
        report.file_name(&fyo).as_deref(),
        Some("GSTR1_29AABCA1234A1Z5_2026-10-01_2026-10-15")
    );
}

/// A supplier, with an address in the country if any.
fn supplier(fyo: &mut Fyo, name: &str, gstin: Option<&str>, country: Option<&str>) {
    let mut values = map([("name", text(name)), ("role", text("Supplier"))]);
    if let Some(gstin) = gstin {
        values.insert("gstType".into(), text("Registered Regular"));
        values.insert("gstin".into(), text(gstin));
    } else {
        values.insert("gstType".into(), text("Unregistered"));
    }
    if let Some(country) = country {
        let address = map([
            ("addressLine1", text("1 Market Street")),
            ("city", text("San Francisco")),
            ("country", text(country)),
        ]);
        let mut address = fyo.get_new_doc("Address", address).unwrap();
        address.sync(fyo).unwrap();
        values.insert("address".into(), address.get("name").cloned().unwrap());
    }
    fyo.get_new_doc("Party", values).unwrap().sync(fyo).unwrap();
}

/// A submitted purchase invoice on `day` of `rows`: item, quantity and tax, if any.
fn purchase(fyo: &mut Fyo, party: &str, day: &str, rows: &[(&str, f64, &str)]) -> Doc {
    let values = map([("party", text(party)), ("date", text(day))]);
    let mut doc = fyo.get_new_doc("PurchaseInvoice", values).unwrap();
    for (idx, (item, quantity, tax)) in rows.iter().enumerate() {
        doc.append(fyo, "items", Default::default()).unwrap();
        doc.set_child(fyo, "items", idx, "item", text(item))
            .unwrap();
        doc.set_child(fyo, "items", idx, "quantity", DocValue::Number(*quantity))
            .unwrap();
        if !tax.is_empty() {
            doc.set_child(fyo, "items", idx, "tax", text(tax)).unwrap();
        }
    }
    doc.sync(fyo).unwrap();
    doc.submit(fyo).unwrap();
    doc
}

fn money_of(doc: &Doc, fieldname: &str) -> f64 {
    match doc.get(fieldname) {
        Some(DocValue::Money(money)) => money.float(),
        other => panic!("{fieldname}: {other:?}"),
    }
}

#[test]
fn indias_setup_makes_the_reverse_charge_templates() {
    let file = TempFile::new("regional-rcm-setup");
    let mut fyo = indian_company(&file);
    assert!(reverse_charge::is_set_up(&fyo).unwrap());
    let tax = fyo.get_doc("Tax", Some("IGST-18-RCM")).unwrap();
    let rows: Vec<(String, f64)> = tax
        .table("details")
        .iter()
        .map(|row| {
            (
                helpers::text(row, "account").unwrap().to_owned(),
                helpers::number(row, "rate").unwrap(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [("IGST".to_owned(), 18.0), ("IGST (RCM)".to_owned(), -18.0)]
    );
    assert_eq!(
        fyo.get_value("Account", Some("IGST (RCM)"), "parentAccount"),
        Some(text("Duties and Taxes"))
    );
    // Setting it up again adds nothing.
    assert!(reverse_charge::set_up(&mut fyo).unwrap().is_empty());
}

#[test]
fn a_reverse_charge_purchase_is_for_the_bill_and_posts_the_tax_owed() {
    let file = TempFile::new("regional-rcm-purchase");
    let mut fyo = registered_company(&file);
    item(&mut fyo, "Hosting", 1000.0, Some("998315"), "Unit");
    supplier(&mut fyo, "Cloud Inc", None, Some("United States"));
    let doc = purchase(
        &mut fyo,
        "Cloud Inc",
        "2026-10-05",
        &[("Hosting", 1.0, "IGST-18-RCM")],
    );

    // The supplier is owed the bill; the GST is owed by the company.
    assert_eq!(money_of(&doc, "grandTotal"), 1000.0);
    assert_eq!(money_of(&doc, "outstandingAmount"), 1000.0);
    let mut entries = postings(&fyo, doc.name().unwrap());
    entries.sort();
    assert_eq!(
        entries,
        [
            posting("Cost of Goods Sold", "1000.00", "0.00"),
            posting("Creditors", "0.00", "1000.00"),
            posting("IGST", "180.00", "0.00"),
            posting("IGST (RCM)", "0.00", "180.00"),
        ]
    );

    let charge = reverse_charge::of_invoice(&fyo, &doc).unwrap().unwrap();
    assert_eq!(charge.taxable.float(), 1000.0);
    assert_eq!(charge.igst.float(), 180.0);
    assert!(charge.cgst.is_zero() && charge.sgst.is_zero());
    assert!(charge.import);
    assert_eq!(charge.rows.len(), 1);
    assert_eq!(charge.rows[0].rate, 18.0);
    assert_eq!(charge.rows[0].hsn.as_deref(), Some("998315"));
    assert_eq!(
        reverse_charge::place_of_supply(&fyo).as_deref(),
        Some("29-Karnataka")
    );

    // From a supplier in India, within the state: not an import.
    supplier(&mut fyo, "Transport Co", None, Some("India"));
    let doc = purchase(
        &mut fyo,
        "Transport Co",
        "2026-10-06",
        &[("Hosting", 1.0, "GST-5-RCM")],
    );
    assert_eq!(money_of(&doc, "grandTotal"), 1000.0);
    let charge = reverse_charge::of_invoice(&fyo, &doc).unwrap().unwrap();
    assert_eq!((charge.cgst.float(), charge.sgst.float()), (25.0, 25.0));
    assert!(!charge.import);

    // An ordinary purchase has none.
    let doc = purchase(
        &mut fyo,
        "Cloud Inc",
        "2026-10-07",
        &[("Hosting", 1.0, "IGST-18")],
    );
    assert_eq!(money_of(&doc, "grandTotal"), 1180.0);
    assert!(reverse_charge::of_invoice(&fyo, &doc).unwrap().is_none());
}

/// A GSTR-3B row: its table, nature of supplies, figures (taxable value, integrated,
/// central and state tax, each none where the row shows none) and whether it is bold.
type Gstr3bRow = (String, String, [Option<f64>; 4], bool);

fn gstr_3b_rows(fyo: &Fyo, report: &mut Gstr3b) -> Vec<Gstr3bRow> {
    let formatter = Formatter::new(fyo).unwrap();
    report
        .run(fyo, &formatter)
        .unwrap()
        .into_iter()
        .map(|row| {
            let number = |c: usize| match &row.cells[c].raw_value {
                RawValue::Number(n) => Some((n * 100.0).round() / 100.0),
                _ => None,
            };
            (
                row.cells[0].value.clone(),
                row.cells[1].value.clone(),
                [number(2), number(3), number(4), number(5)],
                row.cells[0].bold,
            )
        })
        .collect()
}

#[test]
fn gstr_3b_sums_up_sales_the_reverse_charge_and_the_credit() {
    let (_file, mut fyo) = gst_company();
    fyo.db
        .update(
            "AccountingSettings",
            &map([("gstin", text("29AABCA1234A1Z5"))]),
        )
        .unwrap();
    item(&mut fyo, "Hosting", 1000.0, Some("998315"), "Unit");
    item(&mut fyo, "Support", 500.0, Some("998316"), "Unit");
    item(&mut fyo, "Tea", 200.0, Some("0902"), "Unit");
    supplier(&mut fyo, "Cloud Inc", None, Some("United States"));
    supplier(&mut fyo, "Local Supplier", Some("29AABCS1234A1Z5"), None);
    let day = "2026-10-05";
    purchase(
        &mut fyo,
        "Cloud Inc",
        day,
        &[("Hosting", 1.0, "IGST-18-RCM")],
    );
    purchase(
        &mut fyo,
        "Local Supplier",
        day,
        &[("Support", 1.0, "GST-18")],
    );
    // A purchase with no GST, left out of the tables and listed under them.
    purchase(&mut fyo, "Local Supplier", day, &[("Tea", 1.0, "")]);

    let mut report = Gstr3b::new();
    report.set("fiscalYear", FilterValue::Text("2026-27".into()));
    report.set("period", FilterValue::Text("10".into()));
    let rows = gstr_3b_rows(&fyo, &mut report);
    let find = |table: &str, nature: &str| {
        rows.iter()
            .find(|row| row.0 == table && row.1.starts_with(nature))
            .unwrap_or_else(|| panic!("{table} {nature}: {rows:?}"))
            .clone()
    };
    // The sales: 200 and 100 at 18%, within the state and to Maharashtra, and 100 at 5%.
    assert_eq!(
        find("3.1(a)", "Outward").2,
        [Some(400.0), Some(18.0), Some(20.5), Some(20.5)]
    );
    assert_eq!(
        find("3.1(d)", "Inward").2,
        [Some(1000.0), Some(180.0), Some(0.0), Some(0.0)]
    );
    assert_eq!(
        find("4(A)(2)", "ITC: Import").2,
        [None, Some(180.0), Some(0.0), Some(0.0)]
    );
    assert_eq!(
        find("4(A)(3)", "ITC: Inward").2,
        [None, Some(0.0), Some(0.0), Some(0.0)]
    );
    assert_eq!(
        find("4(A)(5)", "ITC: All other").2,
        [None, Some(0.0), Some(45.0), Some(45.0)]
    );
    let net = find("4(C)", "Net ITC");
    assert_eq!(net.2, [None, Some(180.0), Some(45.0), Some(45.0)]);
    assert!(net.3);
    let cash = find("6.1", "Reverse charge");
    assert_eq!(cash.2, [None, Some(180.0), Some(0.0), Some(0.0)]);
    assert!(cash.3);
    assert_eq!(
        find("", "Purchase rows with no GST").2,
        [Some(200.0), None, None, None]
    );
    assert!(!rows.iter().any(|row| row.1.starts_with("Sales rows")));

    // Due on the 20th of the next month; for a quarter, the 22nd in Karnataka and the
    // 24th in Delhi.
    assert_eq!(report.due_date(&fyo), Some(date(2026, 11, 20)));
    report.set("period", FilterValue::Text("Q3".into()));
    assert_eq!(report.due_date(&fyo), Some(date(2027, 1, 22)));
    assert_eq!(
        Period::Quarter(3).gstr3b_due_date(2026, "07"),
        Some(date(2027, 1, 24))
    );
    assert_eq!(
        report.file_name(&fyo).as_deref(),
        Some("GSTR3B_29AABCA1234A1Z5_122026")
    );
}
