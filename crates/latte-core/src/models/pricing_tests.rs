// The pricing rule, coupon code, loyalty program, lead and quote tests of Books
// (models/baseModels/tests/testPricingRule.spec.ts, testCouponCodes.spec.ts,
// testLoyaltyProgram.spec.ts and testLead.spec.ts @ v0.36.0), on a company set up as the
// wizard sets one up.

use latte_db::{DocValue, DocValueMap, Filter, GetAllOptions, QueryFilter};

use crate::models::base_models::lead::{create_customer, create_sales_quote};
use crate::models::base_models::sales_quote::get_invoice;
use crate::models::payment_tests::validation_message;
use crate::setup_tests::{TempFile, indian_company};
use crate::tests::{map, money, text};
use crate::{Doc, Fyo};

const JACKET: &str = "Jacket";
const CAP: &str = "Cap";
const PEN: &str = "Pen";
const PARTY: &str = "Daisy";

/// A date as the form sets it.
fn date(date: &str) -> DocValue {
    text(date)
}

fn number(n: f64) -> DocValue {
    DocValue::Number(n)
}

fn settings(fyo: &mut Fyo, values: DocValueMap) {
    fyo.db.update("AccountingSettings", &values).unwrap();
}

/// Jacket at 1000, Cap at 100 and Pen at 700, none tracked, and Daisy, a customer.
fn shop() -> (TempFile, Fyo) {
    let file = TempFile::new("pricing");
    let mut fyo = indian_company(&file);
    for (item, rate) in [(JACKET, 1000.0), (CAP, 100.0), (PEN, 700.0)] {
        let values = map([("name", text(item)), ("rate", number(rate))]);
        fyo.get_new_doc("Item", values)
            .unwrap()
            .sync(&mut fyo)
            .unwrap();
    }
    let values = map([
        ("name", text(PARTY)),
        ("email", text("daisy@alien.com")),
        ("role", text("Customer")),
    ]);
    fyo.get_new_doc("Party", values)
        .unwrap()
        .sync(&mut fyo)
        .unwrap();
    settings(&mut fyo, map([("enablePricingRule", DocValue::Bool(true))]));
    (file, fyo)
}

fn applied_items(item: &str) -> DocValue {
    DocValue::Table(vec![map([("item", text(item))])])
}

/// PRLE-1001: Jacket at 800 for 4 to 6 of them and 4000 to 6000.
fn jacket_rule(fyo: &mut Fyo, more: &[(&str, DocValue)]) -> Doc {
    let mut values = map([
        ("title", text("JKT PDR Offer")),
        ("appliedItems", applied_items(JACKET)),
        ("discountType", text("Price Discount")),
        ("priceDiscountType", text("rate")),
        ("discountRate", number(800.0)),
        ("minQuantity", number(4.0)),
        ("maxQuantity", number(6.0)),
        ("minAmount", number(4000.0)),
        ("maxAmount", number(6000.0)),
        ("priority", text("1")),
    ]);
    for (fieldname, value) in more {
        values.insert((*fieldname).to_owned(), value.clone());
    }
    let mut rule = fyo.get_new_doc("PricingRule", values).unwrap();
    rule.sync(fyo).unwrap();
    rule
}

/// PRLE-1002: a free Pen for 4 to 6 Caps and 200 to 1000, in February 2024.
fn cap_rule(fyo: &mut Fyo, priority: &str) -> Doc {
    let values = map([
        ("title", text("CAP PDR Offer")),
        ("appliedItems", applied_items(CAP)),
        ("discountType", text("Product Discount")),
        ("freeItem", text(PEN)),
        ("freeItemQuantity", number(1.0)),
        ("freeItemUnit", text("Unit")),
        ("minQuantity", number(4.0)),
        ("maxQuantity", number(6.0)),
        ("minAmount", number(200.0)),
        ("maxAmount", number(1000.0)),
        ("validFrom", date("2024-02-01")),
        ("validTo", date("2024-02-29")),
        ("priority", text(priority)),
    ]);
    let mut rule = fyo.get_new_doc("PricingRule", values).unwrap();
    rule.sync(fyo).unwrap();
    rule
}

fn invoice(fyo: &mut Fyo, on: &str, item: &str, quantity: f64, rate: f64) -> Doc {
    let values = map([("party", text(PARTY)), ("date", date(on))]);
    let mut sinv = fyo.get_new_doc("SalesInvoice", values).unwrap();
    let row = map([
        ("item", text(item)),
        ("quantity", number(quantity)),
        ("rate", number(rate)),
    ]);
    sinv.append(fyo, "items", row).unwrap();
    sinv
}

fn rules_applied(sinv: &Doc) -> Vec<String> {
    sinv.table("pricingRuleDetail")
        .iter()
        .map(|row| match row.get("referenceName") {
            Some(DocValue::Text(name)) => name.clone(),
            value => panic!("referenceName is {value:?}"),
        })
        .collect()
}

#[test]
fn a_pricing_rule_applies_within_its_quantities_amounts_and_dates() {
    let (_file, mut fyo) = shop();
    let mut jacket = jacket_rule(&mut fyo, &[("isEnabled", DocValue::Bool(false))]);
    assert_eq!(jacket.name(), Some("PRLE-1001"));
    let cap = cap_rule(&mut fyo, "1");
    assert_eq!(cap.name(), Some("PRLE-1002"));

    // A disabled rule doesn't apply.
    let sinv = invoice(&mut fyo, "2024-02-10", JACKET, 5.0, 1000.0);
    assert!(rules_applied(&sinv).is_empty());
    assert_eq!(
        sinv.get("isPricingRuleApplied"),
        Some(&DocValue::Bool(false))
    );

    jacket
        .set_and_sync(&mut fyo, "isEnabled", DocValue::Bool(true))
        .unwrap();
    let sinv = invoice(&mut fyo, "2024-02-10", JACKET, 5.0, 1000.0);
    assert_eq!(rules_applied(&sinv), ["PRLE-1001"]);
    assert_eq!(
        sinv.get("isPricingRuleApplied"),
        Some(&DocValue::Bool(true))
    );
    assert_eq!(money(&sinv.table("items")[0], "rate"), "800.00");

    // Too few, too many: no rule (the amount is the item's rate times the quantity).
    for quantity in [3.0, 7.0, 10.0, 2.0] {
        let sinv = invoice(&mut fyo, "2024-02-10", JACKET, quantity, 1000.0);
        assert!(rules_applied(&sinv).is_empty(), "{quantity} jackets");
    }

    // Outside the rule's dates, and inside them.
    for on in ["2024-01-01", "2024-03-01"] {
        let sinv = invoice(&mut fyo, on, CAP, 5.0, 100.0);
        assert!(rules_applied(&sinv).is_empty(), "on {on}");
    }
    let sinv = invoice(&mut fyo, "2024-02-01", CAP, 5.0, 100.0);
    assert_eq!(rules_applied(&sinv), ["PRLE-1002"]);
    let sinv = invoice(&mut fyo, "2024-02-01", CAP, 7.0, 100.0);
    assert!(rules_applied(&sinv).is_empty());
}

#[test]
fn the_highest_priority_rule_applies_and_none_on_a_tie() {
    let (_file, mut fyo) = shop();
    cap_rule(&mut fyo, "1");
    let mut higher = cap_rule(&mut fyo, "2");
    assert_eq!(higher.name(), Some("PRLE-1002"));

    let sinv = invoice(&mut fyo, "2024-02-01", CAP, 5.0, 100.0);
    assert_eq!(rules_applied(&sinv), ["PRLE-1002"]);

    higher
        .set_and_sync(&mut fyo, "priority", text("1"))
        .unwrap();
    let sinv = invoice(&mut fyo, "2024-02-01", CAP, 5.0, 100.0);
    assert!(rules_applied(&sinv).is_empty());
}

#[test]
fn a_price_discount_sets_the_rate_percent_or_amount() {
    let (_file, mut fyo) = shop();
    let mut rule = jacket_rule(&mut fyo, &[]);

    let values = map([
        ("priceDiscountType", text("percentage")),
        ("discountPercentage", number(69.0)),
    ]);
    rule.set_multiple(&mut fyo, values).unwrap();
    rule.sync(&mut fyo).unwrap();
    let sinv = invoice(&mut fyo, "2024-02-10", JACKET, 5.0, 1000.0);
    assert_eq!(
        sinv.table("items")[0].get("itemDiscountPercent"),
        Some(&number(69.0))
    );

    let values = map([
        ("priceDiscountType", text("amount")),
        ("discountAmount", number(500.0)),
    ]);
    rule.set_multiple(&mut fyo, values).unwrap();
    rule.sync(&mut fyo).unwrap();
    let sinv = invoice(&mut fyo, "2024-02-10", JACKET, 5.0, 1000.0);
    let row = &sinv.table("items")[0];
    assert_eq!(money(row, "itemDiscountAmount"), "500.00");
    assert_eq!(
        row.get("setItemDiscountAmount"),
        Some(&DocValue::Bool(true))
    );
}

#[test]
fn a_rule_that_stops_applying_takes_its_discount_back() {
    let (_file, mut fyo) = shop();
    let values = [
        ("priceDiscountType", text("amount")),
        ("discountAmount", number(500.0)),
    ];
    jacket_rule(&mut fyo, &values);
    let mut sinv = invoice(&mut fyo, "2024-02-10", JACKET, 5.0, 1000.0);
    assert_eq!(
        money(&sinv.table("items")[0], "itemDiscountAmount"),
        "500.00"
    );

    sinv.set_child(&mut fyo, "items", 0, "quantity", number(9.0))
        .unwrap();
    assert!(rules_applied(&sinv).is_empty());
    let row = &sinv.table("items")[0];
    assert_eq!(money(row, "itemDiscountAmount"), "0.00");
    assert_eq!(
        row.get("setItemDiscountAmount"),
        Some(&DocValue::Bool(false))
    );
}

#[test]
fn a_rate_rule_that_stops_applying_gives_the_items_rate_back() {
    let (_file, mut fyo) = shop();
    jacket_rule(&mut fyo, &[]);
    let mut sinv = invoice(&mut fyo, "2024-02-10", JACKET, 5.0, 1000.0);
    assert_eq!(money(&sinv.table("items")[0], "rate"), "800.00");

    sinv.set_child(&mut fyo, "items", 0, "quantity", number(9.0))
        .unwrap();
    assert!(rules_applied(&sinv).is_empty());
    assert_eq!(money(&sinv.table("items")[0], "rate"), "1000.00");
}

#[test]
fn a_product_discount_adds_free_items() {
    let (_file, mut fyo) = shop();
    let mut rule = cap_rule(&mut fyo, "1");

    let mut sinv = invoice(&mut fyo, "2024-02-01", CAP, 5.0, 100.0);
    sinv.sync(&mut fyo).unwrap();
    let items = sinv.table("items");
    assert_eq!(items.len(), 2);
    assert_eq!(items[1].get("item"), Some(&text(PEN)));
    assert_eq!(items[1].get("isFreeItem"), Some(&DocValue::Bool(true)));
    assert_eq!(items[1].get("quantity"), Some(&number(1.0)));
    assert_eq!(money(&items[1], "rate"), "0.00");

    // Saving again keeps one free row.
    sinv.sync(&mut fyo).unwrap();
    assert_eq!(sinv.table("items").len(), 2);

    // One free for every 2.
    let values = map([
        ("isRecursive", DocValue::Bool(true)),
        ("recurseEvery", number(2.0)),
    ]);
    rule.set_multiple(&mut fyo, values).unwrap();
    rule.sync(&mut fyo).unwrap();
    let mut sinv = invoice(&mut fyo, "2024-02-01", CAP, 5.0, 100.0);
    sinv.sync(&mut fyo).unwrap();
    assert_eq!(sinv.table("items")[1].get("quantity"), Some(&number(2.5)));
}

/// PRLE-1001 as a coupon's rule, and COUPON1 for it.
fn coupon(fyo: &mut Fyo, more: &[(&str, DocValue)]) -> Result<Doc, crate::Error> {
    let mut values = map([
        ("name", text("COUPON1")),
        ("couponName", text("coupon1")),
        ("pricingRule", text("PRLE-1001")),
        ("maximumUse", number(5.0)),
        ("minAmount", number(4000.0)),
        ("maxAmount", number(6000.0)),
        ("validFrom", date("2024-02-01")),
        ("validTo", date("2024-02-29")),
    ]);
    for (fieldname, value) in more {
        values.insert((*fieldname).to_owned(), value.clone());
    }
    let mut doc = fyo.get_new_doc("CouponCode", values)?;
    doc.sync(fyo)?;
    Ok(doc)
}

fn coupon_shop() -> (TempFile, Fyo) {
    let (file, mut fyo) = shop();
    let values = [
        ("isCouponCodeBased", DocValue::Bool(true)),
        ("validFrom", date("2024-02-01")),
        ("validTo", date("2024-02-29")),
    ];
    jacket_rule(&mut fyo, &values);
    settings(&mut fyo, map([("enableCouponCode", DocValue::Bool(true))]));
    (file, fyo)
}

#[test]
fn a_coupon_must_fit_its_pricing_rule() {
    let (_file, mut fyo) = coupon_shop();
    let cases = [
        (
            vec![("minAmount", number(7000.0))],
            "Minimum Amount should be less than the Maximum Amount.",
        ),
        (
            vec![("minAmount", number(3000.0))],
            "Minimum Amount should be greather than the Pricing Rule's Minimum Amount.",
        ),
        (
            vec![("maxAmount", number(7000.0))],
            "Maximum Amount should be lesser than Pricing Rule's Maximum Amount",
        ),
        (
            vec![("validTo", date("2024-01-10"))],
            "Valid From Date should be less than Valid To Date.",
        ),
        (
            vec![("validFrom", date("2024-01-01"))],
            "Valid From Date should be greather than Pricing Rule's Valid From Date.",
        ),
        (
            vec![("validTo", date("2024-03-01"))],
            "Valid To Date should be lesser than Pricing Rule's Valid To Date.",
        ),
    ];
    for (values, message) in cases {
        assert_eq!(validation_message(coupon(&mut fyo, &values)), message);
    }
    let doc = coupon(&mut fyo, &[]).unwrap();
    assert_eq!(doc.name(), Some("COUPON1"));
}

#[test]
fn a_coupon_applies_its_rule_and_is_counted_when_used() {
    let (_file, mut fyo) = coupon_shop();
    coupon(&mut fyo, &[]).unwrap();

    // Without the coupon, the coupon's rule doesn't apply.
    let sinv = invoice(&mut fyo, "2024-02-10", JACKET, 5.0, 1000.0);
    assert!(rules_applied(&sinv).is_empty());

    let mut sinv = invoice(&mut fyo, "2024-02-10", JACKET, 5.0, 1000.0);
    sinv.append(&mut fyo, "coupons", map([("coupons", text("COUPON1"))]))
        .unwrap();
    assert_eq!(rules_applied(&sinv), ["PRLE-1001"]);
    assert_eq!(money(&sinv.table("items")[0], "rate"), "800.00");

    sinv.sync(&mut fyo).unwrap();
    sinv.submit(&mut fyo).unwrap();
    let used = |fyo: &Fyo| fyo.get_value("CouponCode", Some("COUPON1"), "used");
    assert_eq!(used(&fyo), Some(number(1.0)));
    sinv.cancel(&mut fyo).unwrap();
    assert_eq!(used(&fyo), Some(number(0.0)));
}

#[test]
fn a_coupon_for_other_items_is_dropped() {
    let (_file, mut fyo) = coupon_shop();
    coupon(&mut fyo, &[]).unwrap();

    let mut sinv = invoice(&mut fyo, "2024-02-10", CAP, 5.0, 100.0);
    sinv.append(&mut fyo, "coupons", map([("coupons", text("COUPON1"))]))
        .unwrap();
    sinv.sync(&mut fyo).unwrap();
    assert!(sinv.table("coupons").is_empty());

    // Typed into a row, it's refused.
    let mut sinv = invoice(&mut fyo, "2024-02-10", CAP, 5.0, 100.0);
    sinv.append(&mut fyo, "coupons", DocValueMap::new())
        .unwrap();
    let result = sinv.set_child(&mut fyo, "coupons", 0, "coupons", text("COUPON1"));
    assert_eq!(
        validation_message(result),
        "Coupon COUPON1 is not applicable for applied items."
    );
}

#[test]
fn a_coupon_outside_its_dates_says_so() {
    let (_file, mut fyo) = coupon_shop();
    let values = [
        ("validFrom", date("2024-02-05")),
        ("validTo", date("2024-02-20")),
    ];
    coupon(&mut fyo, &values).unwrap();

    // Books says "Valid From Date should be less than Valid To Date." here.
    for (on, message) in [
        (
            "2024-02-02",
            "Coupon COUPON1 isn't valid yet on the invoice date.",
        ),
        (
            "2024-02-25",
            "Coupon COUPON1 is no longer valid on the invoice date.",
        ),
    ] {
        let mut sinv = invoice(&mut fyo, on, JACKET, 5.0, 1000.0);
        sinv.append(&mut fyo, "coupons", DocValueMap::new())
            .unwrap();
        let result = sinv.set_child(&mut fyo, "coupons", 0, "coupons", text("COUPON1"));
        assert_eq!(validation_message(result), message);
    }
}

/// A loyalty program for 2024 with a tier for 1000 and up, a point a rupee, a point
/// worth 0.5, posted to Discounts.
fn loyalty_shop() -> (TempFile, Fyo) {
    let (file, mut fyo) = shop();
    let values = map([
        ("name", text("Program")),
        ("fromDate", date("2024-01-01")),
        ("toDate", date("2024-12-31")),
        ("isEnabled", DocValue::Bool(true)),
        ("conversionFactor", number(0.5)),
        ("expiryDuration", number(30.0)),
        ("expenseAccount", text("Discounts")),
        (
            "collectionRules",
            DocValue::Table(vec![map([
                ("tierName", text("Silver")),
                ("collectionFactor", number(1.0)),
                ("minimumTotalSpent", number(1000.0)),
            ])]),
        ),
    ]);
    fyo.get_new_doc("LoyaltyProgram", values)
        .unwrap()
        .sync(&mut fyo)
        .unwrap();
    let mut party = fyo.get_doc("Party", Some(PARTY)).unwrap();
    party
        .set_and_sync(&mut fyo, "loyaltyProgram", text("Program"))
        .unwrap();
    settings(
        &mut fyo,
        map([("enablePricingRule", DocValue::Bool(false))]),
    );
    (file, fyo)
}

fn points(fyo: &Fyo) -> Option<DocValue> {
    fyo.get_value("Party", Some(PARTY), "loyaltyPoints")
}

fn ales(fyo: &Fyo, name: &str) -> Vec<(String, String, String)> {
    let options = GetAllOptions {
        fields: Some(["account", "debit", "credit"].map(str::to_owned).to_vec()),
        filters: QueryFilter::from([("referenceName".to_owned(), Filter::from(name))]),
        ..GetAllOptions::default()
    };
    let amount = |row: &DocValueMap, fieldname| match row.get(fieldname) {
        Some(DocValue::Money(money)) => money.round(None),
        value => panic!("{fieldname} is {value:?}"),
    };
    let mut rows: Vec<_> = fyo
        .db
        .get_all_docs("AccountingLedgerEntry", &options)
        .unwrap()
        .iter()
        .map(|row| {
            let Some(DocValue::Text(account)) = row.get("account") else {
                panic!("account is {:?}", row.get("account"));
            };
            (account.clone(), amount(row, "debit"), amount(row, "credit"))
        })
        .collect();
    rows.sort();
    rows
}

#[test]
fn invoices_earn_and_redeem_loyalty_points() {
    let (_file, mut fyo) = loyalty_shop();
    let today = jiff::Zoned::now().date().to_string();

    let mut sinv = invoice(&mut fyo, &today, JACKET, 2.0, 1000.0);
    assert_eq!(sinv.get("loyaltyProgram"), Some(&text("Program")));
    sinv.sync(&mut fyo).unwrap();
    sinv.submit(&mut fyo).unwrap();
    // The program runs in 2024 only.
    assert_eq!(points(&fyo), Some(number(0.0)));

    let mut program = fyo.get_doc("LoyaltyProgram", Some("Program")).unwrap();
    program
        .set_and_sync(&mut fyo, "toDate", date("2099-12-31"))
        .unwrap();
    let mut sinv = invoice(&mut fyo, &today, JACKET, 2.0, 1000.0);
    sinv.sync(&mut fyo).unwrap();
    sinv.submit(&mut fyo).unwrap();
    assert_eq!(points(&fyo), Some(number(2000.0)));

    let mut redeem = invoice(&mut fyo, &today, CAP, 10.0, 100.0);
    redeem
        .set(&mut fyo, "redeemLoyaltyPoints", DocValue::Bool(true))
        .unwrap();
    let result = redeem.set(&mut fyo, "loyaltyPoints", number(2001.0));
    assert_eq!(validation_message(result), "Daisy only has 2000 points");
    redeem
        .set(&mut fyo, "loyaltyPoints", number(400.0))
        .unwrap();
    assert_eq!(money(&redeem, "grandTotal"), "800.00");
    redeem.sync(&mut fyo).unwrap();
    redeem.submit(&mut fyo).unwrap();
    assert_eq!(points(&fyo), Some(number(1600.0)));
    let name = redeem.name().unwrap().to_owned();
    assert_eq!(
        ales(&fyo, &name),
        [
            ("Debtors".to_owned(), "800.00".to_owned(), "0.00".to_owned()),
            (
                "Discounts".to_owned(),
                "200.00".to_owned(),
                "0.00".to_owned()
            ),
            ("Sales".to_owned(), "0.00".to_owned(), "1000.00".to_owned()),
        ]
    );

    // Cancelling gives the points back, and takes back what an invoice earned.
    redeem.cancel(&mut fyo).unwrap();
    assert_eq!(points(&fyo), Some(number(2000.0)));
    sinv.cancel(&mut fyo).unwrap();
    assert_eq!(points(&fyo), Some(number(0.0)));
}

#[test]
fn a_lead_becomes_a_customer_and_gets_a_quote() {
    let (_file, mut fyo) = shop();
    let values = map([("name", text("Lia")), ("email", text("not an email"))]);
    let mut lead = fyo.get_new_doc("Lead", DocValueMap::new()).unwrap();
    let result = lead.set_multiple(&mut fyo, values);
    assert_eq!(validation_message(result), "Invalid email: not an email");
    let values = map([
        ("name", text("Lia")),
        ("email", text("lia@example.com")),
        ("mobile", text("+91 98765 43210")),
    ]);
    let mut lead = fyo.get_new_doc("Lead", values).unwrap();
    lead.sync(&mut fyo).unwrap();

    let mut quote = create_sales_quote(&lead, &fyo).unwrap();
    assert_eq!(quote.get("party"), Some(&text("Lia")));
    // Set as the form sets it, so the rate comes from the item.
    quote.append(&mut fyo, "items", DocValueMap::new()).unwrap();
    quote
        .set_child(&mut fyo, "items", 0, "item", text(CAP))
        .unwrap();
    quote
        .set_child(&mut fyo, "items", 0, "quantity", number(2.0))
        .unwrap();
    quote.sync(&mut fyo).unwrap();
    quote.submit(&mut fyo).unwrap();
    assert_eq!(
        fyo.get_value("Lead", Some("Lia"), "status"),
        Some(text("Quotation"))
    );
    assert!(ales(&fyo, quote.name().unwrap()).is_empty());

    let mut customer = create_customer(&lead, &fyo).unwrap();
    assert_eq!(customer.get("phone"), Some(&text("+91 98765 43210")));
    customer.sync(&mut fyo).unwrap();
    assert_eq!(
        fyo.get_value("Lead", Some("Lia"), "status"),
        Some(text("Converted"))
    );

    let mut sinv = get_invoice(&quote, &mut fyo).unwrap().unwrap();
    assert_eq!(sinv.get("quote"), quote.get("name"));
    assert_eq!(sinv.table("items").len(), 1);
    assert_eq!(money(&sinv, "grandTotal"), "200.00");
    sinv.sync(&mut fyo).unwrap();
    assert!(sinv.name().unwrap().starts_with("SINV-"));
}
