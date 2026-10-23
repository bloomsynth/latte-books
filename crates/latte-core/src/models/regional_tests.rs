// Tests of India's regional models, the Address model and the GSTR reports.

use jiff::civil::date;
use latte_db::DocValue;

use crate::fyo::format::Formatter;
use crate::fyo::model::types::DocContext;
use crate::reports::Report;
use crate::reports::gstr::{Gstr, GstrType};
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

/// A submitted sales invoice of `quantity` Widgets at 100 with the tax.
fn sale(fyo: &mut Fyo, party: &str, quantity: f64, tax: &str) -> Doc {
    let values = map([("party", text(party)), ("date", text("2026-10-01"))]);
    let mut doc = fyo.get_new_doc("SalesInvoice", values).unwrap();
    doc.append(fyo, "items", Default::default()).unwrap();
    doc.set_child(fyo, "items", 0, "item", text("Widget"))
        .unwrap();
    doc.set_child(fyo, "items", 0, "quantity", DocValue::Number(quantity))
        .unwrap();
    doc.set_child(fyo, "items", 0, "tax", text(tax)).unwrap();
    doc.sync(fyo).unwrap();
    doc.submit(fyo).unwrap();
    doc
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
    let mut report = Gstr::new(GstrType::Gstr1);
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
    assert_eq!(json["gstin"], "29AABCA1234A1Z5");
    assert_eq!(json["fp"], "102026");
    let b2b = json["b2b"].as_array().unwrap();
    assert_eq!(b2b.len(), 2);
    let local = b2b.iter().find(|c| c["ctin"] == "29AABCL1234A1Z5").unwrap();
    let inv = &local["inv"][0];
    assert_eq!(inv["idt"], "01-10-2026");
    assert_eq!(inv["pos"], "29");
    assert_eq!(inv["val"], 236);
    assert_eq!(
        inv["itms"][0],
        serde_json::json!({
            "num": 8471,
            "itm_det": { "txval": 200, "rt": 18, "csamt": 0, "camt": 18, "samt": 18, "iamt": 0 },
        })
    );

    // Unregistered and in the state, so a small B2C supply.
    let mut report = gstr("B2CS");
    report.run(&fyo, &formatter).unwrap();
    assert_eq!(report.rows.len(), 1);
    assert_eq!(report.rows[0].party_name, "Walk In");
    assert_eq!(report.rows[0].reverse_charge, "Y");
    let json: serde_json::Value =
        serde_json::from_str(&report.export_json(&fyo).unwrap().unwrap()).unwrap();
    assert_eq!(
        json["b2cs"][0],
        serde_json::json!({
            "sply_ty": "INTRA", "pos": "29", "typ": "OE", "txval": 100, "rt": 5,
            "iamt": 0, "camt": 2.5, "samt": 2.5, "csamt": 0,
        })
    );

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
        "Cannot Export: Please set GSTIN in General Settings."
    );
    assert!(report.export_json(&fyo).unwrap().is_err());
}
