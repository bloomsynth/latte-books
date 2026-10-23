// Port of src/setup/ @ v0.36.0 (setupInstance.ts, createCOA.ts), the regional records
// of src/regional/, the defaults of utils/defaults.ts, what initializeInstance in
// src/utils/initialization.ts does to a new file, and the formulas and lists of
// models/baseModels/SetupWizard/SetupWizard.ts.
//
// Deviations:
// - The print templates aren't added: Latte prints with its own Typst templates.
// - Books makes the system settings' instance id twice, once when the file is opened and
//   again while setting up; Latte makes it once.
// - The standard chart of accounts is read from fixtures/verified/standardCOA.json, which
//   is the same tree as Books' standardCOA.ts. Books translates its account names into the
//   app's language; Latte keeps them in English, as Books looks some of them up by their
//   English names (a party's Debtors or Creditors).

use std::sync::OnceLock;

use indexmap::IndexMap;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{ToSpan, Zoned};
use latte_db::{Database, DocValue, DocValueMap, GetAllOptions, get_random_string};
use serde_json::{Map, Value};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::naming::create_number_series;

/// `DEFAULT_SERIES_START`.
const DEFAULT_SERIES_START: f64 = 1001.0;
/// `DEFAULT_LOCALE`.
const DEFAULT_LOCALE: &str = "en-IN";
/// `DEFAULT_CURRENCY`.
const DEFAULT_CURRENCY: &str = "INR";

/// `SetupWizardOptions`: what the setup wizard asks for.
#[derive(Debug, Clone, PartialEq)]
pub struct SetupWizardOptions {
    /// The logo, as a data URL.
    pub logo: Option<String>,
    pub company_name: String,
    pub country: String,
    pub fullname: String,
    pub email: String,
    pub bank_name: String,
    pub currency: String,
    pub fiscal_year_start: Date,
    pub fiscal_year_end: Date,
    pub chart_of_accounts: String,
}

impl SetupWizardOptions {
    /// The wizard's values for a country, as its formulas fill them in on `today`: the
    /// currency, the chart of accounts and, where the country has one, the fiscal year.
    pub fn for_country(country: &str, today: Date) -> Self {
        let info = country_info().get(country);
        let field = |key: &str| info.and_then(|info| info.get(key)?.as_str());
        let fiscal_year_start =
            field("fiscal_year_start").and_then(|d| fiscal_year(d, true, today));
        let fiscal_year_end = field("fiscal_year_end").and_then(|d| fiscal_year(d, false, today));
        Self {
            logo: None,
            company_name: String::new(),
            country: country.to_owned(),
            fullname: String::new(),
            email: String::new(),
            bank_name: String::new(),
            currency: setup_currency(country).unwrap_or_default(),
            fiscal_year_start: fiscal_year_start.unwrap_or(Date::constant(1, 1, 1)),
            fiscal_year_end: fiscal_year_end.unwrap_or(Date::constant(1, 1, 1)),
            chart_of_accounts: setup_chart_of_accounts(country).unwrap_or_default(),
        }
    }
}

/// `getCountryInfo`: fixtures/countryInfo.json, by country name.
pub fn country_info() -> &'static Map<String, Value> {
    static INFO: OnceLock<Map<String, Value>> = OnceLock::new();
    INFO.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../../../vendor/books/fixtures/countryInfo.json"
        ))
        .expect("countryInfo.json is valid")
    })
}

/// `getCountryCodeFromCountry`: "in" for India, and "" for a country not in the list.
pub fn country_code(country: &str) -> String {
    country_info()
        .get(country)
        .and_then(|info| info.get("code")?.as_str())
        .unwrap_or_default()
        .to_owned()
}

/// `getCOAList`: the charts of accounts, as `(name, countryCode)`.
pub const COA_LIST: [(&str, &str); 12] = [
    ("Standard Chart of Accounts", ""),
    ("U.A.E - Chart of Accounts", "ae"),
    (
        "Canada - Plan comptable pour les provinces francophones",
        "ca",
    ),
    ("Guatemala - Cuentas", "gt"),
    ("Hungary - Chart of Accounts", "hu"),
    ("Indonesia - Chart of Accounts", "id"),
    ("India - Chart of Accounts", "in"),
    ("Mexico - Plan de Cuentas", "mx"),
    ("Nicaragua - Catalogo de Cuentas", "ni"),
    ("Netherlands - Grootboekschema", "nl"),
    ("Singapore - Chart of Accounts", "sg"),
    ("France - Plan Comptable General", "fr"),
];

/// `getCurrencyList`: each country's currency, as `(name, countryCode)`.
pub fn currency_list() -> Vec<(String, String)> {
    country_info()
        .values()
        .filter_map(|info| {
            let currency = info.get("currency")?.as_str()?;
            let code = info.get("code")?.as_str()?;
            Some((currency.to_owned(), code.to_owned()))
        })
        .collect()
}

/// The `currency` formula: the currency of the country.
fn setup_currency(country: &str) -> Option<String> {
    let code = country_code(country);
    if code.is_empty() {
        return None;
    }
    let list = currency_list();
    let found = list.iter().find(|(_, country_code)| *country_code == code);
    found.or(list.first()).map(|(name, _)| name.clone())
}

/// The `chartOfAccounts` formula: the country's chart, or the standard one.
fn setup_chart_of_accounts(country: &str) -> Option<String> {
    let code = country_code(country);
    if code.is_empty() {
        return None;
    }
    let found = COA_LIST
        .iter()
        .find(|(_, country_code)| *country_code == code);
    Some(found.unwrap_or(&COA_LIST[0]).0.to_owned())
}

/// `getFiscalYear`: a fiscal year's start or end ("MM-dd") as of `today`. In January to
/// March the year is the one that started last year.
pub fn fiscal_year(date: &str, is_start: bool, today: Date) -> Option<Date> {
    let (month, day) = date.split_once('-')?;
    let date = Date::new(today.year(), month.parse().ok()?, day.parse().ok()?).ok()?;
    let early = [1, 2, 3].contains(&today.month());
    let years = match (is_start, early) {
        (true, true) => -1,
        (false, false) => 1,
        _ => 0,
    };
    date.checked_add(years.years()).ok()
}

/// `setupInstance`: creates a company file at `path` and sets it up as the setup wizard
/// does. Every write is recorded as made on `device`.
pub fn setup_instance(
    path: impl AsRef<std::path::Path>,
    options: &SetupWizardOptions,
    device: &str,
) -> Result<Fyo, Error> {
    let db = Database::create(path, &country_code(&options.country), device)?;
    let mut fyo = Fyo::with_models(db);
    check_single_links(&mut fyo)?;
    set_open_count(&mut fyo)?;

    update_system_settings(options, &mut fyo)?;
    update_accounting_settings(options, &mut fyo)?;
    update_print_settings(options, &mut fyo)?;
    create_currency_records(&mut fyo)?;
    create_account_records(
        &options.bank_name,
        &options.country,
        &options.chart_of_accounts,
        &mut fyo,
    )?;
    create_regional_records(&options.country, &mut fyo)?;
    create_default_entries(&mut fyo)?;
    create_default_number_series(&mut fyo)?;
    update_inventory_settings(&mut fyo)?;
    set_single(
        &mut fyo,
        "AccountingSettings",
        "setupComplete",
        DocValue::Bool(true),
    )?;
    Ok(fyo)
}

fn text(value: &str) -> DocValue {
    DocValue::Text(value.to_owned())
}

fn values<const N: usize>(values: [(&str, DocValue); N]) -> DocValueMap {
    values.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

/// Sets values of a single and saves it (`setAndSync`).
fn set_singles(fyo: &mut Fyo, schema_name: &str, values: DocValueMap) -> Result<(), Error> {
    let mut doc = fyo.get_doc(schema_name, None)?;
    doc.set_multiple(fyo, values)?;
    doc.sync(fyo)
}

fn set_single(
    fyo: &mut Fyo,
    schema_name: &str,
    fieldname: &str,
    value: DocValue,
) -> Result<(), Error> {
    set_singles(fyo, schema_name, values([(fieldname, value)]))
}

/// A date field's value for a date: its midnight in the system time zone.
fn date_value(date: Date) -> Result<DocValue, Error> {
    let zoned = date
        .to_zoned(TimeZone::system())
        .map_err(|e| Error::Value(e.to_string()))?;
    Ok(DocValue::Date(zoned.timestamp()))
}

/// `checkSingleLinks`: drops single values that link to a document that doesn't exist,
/// such as the point of sale's default accounts before there are any.
fn check_single_links(fyo: &mut Fyo) -> Result<(), Error> {
    let mut targets = IndexMap::new();
    for schema in fyo.db.schema_map().values().filter(|s| s.is_single) {
        for field in &schema.fields {
            if field.fieldtype == Some(latte_schema::FieldType::Link)
                && let Some(target) = &field.target
            {
                targets.insert(
                    (schema.name.clone(), field.fieldname.clone()),
                    target.clone(),
                );
            }
        }
    }

    let options = GetAllOptions {
        fields: Some(vec![
            "name".into(),
            "parent".into(),
            "fieldname".into(),
            "value".into(),
        ]),
        ..GetAllOptions::default()
    };
    for row in fyo.db.get_all_docs("SingleValue", &options)? {
        let get = |key: &str| match row.get(key) {
            Some(DocValue::Text(value)) => value.clone(),
            _ => String::new(),
        };
        let Some(target) = targets.get(&(get("parent"), get("fieldname"))) else {
            continue;
        };
        let value = get("value");
        if value.is_empty() || fyo.db.exists(target, Some(&value))? {
            continue;
        }
        fyo.db.delete("SingleValue", &get("name"))?;
    }
    Ok(())
}

/// `setOpenCount` for a file opened for the first time.
fn set_open_count(fyo: &mut Fyo) -> Result<(), Error> {
    set_single(fyo, "Misc", "openCount", DocValue::Number(1.0))
}

/// `updateSystemSettings`.
fn update_system_settings(options: &SetupWizardOptions, fyo: &mut Fyo) -> Result<(), Error> {
    let info = country_info().get(&options.country);
    let field = |key: &str| info.and_then(|info| info.get(key)?.as_str());
    let currency = if options.currency.is_empty() {
        field("currency").unwrap_or(DEFAULT_CURRENCY)
    } else {
        &options.currency
    };
    let locale = field("locale").unwrap_or(DEFAULT_LOCALE);
    set_singles(
        fyo,
        "SystemSettings",
        values([
            ("locale", text(locale)),
            ("currency", text(currency)),
            ("instanceId", text(&get_random_string())),
            ("countryCode", text(&country_code(&options.country))),
        ]),
    )
}

/// `updateAccountingSettings`.
fn update_accounting_settings(options: &SetupWizardOptions, fyo: &mut Fyo) -> Result<(), Error> {
    set_singles(
        fyo,
        "AccountingSettings",
        values([
            ("companyName", text(&options.company_name)),
            ("country", text(&options.country)),
            ("fullname", text(&options.fullname)),
            ("email", text(&options.email)),
            ("bankName", text(&options.bank_name)),
            ("fiscalYearStart", date_value(options.fiscal_year_start)?),
            ("fiscalYearEnd", date_value(options.fiscal_year_end)?),
        ]),
    )
}

/// `updatePrintSettings`.
fn update_print_settings(options: &SetupWizardOptions, fyo: &mut Fyo) -> Result<(), Error> {
    let logo = options.logo.as_deref().map_or(DocValue::Null, text);
    set_singles(
        fyo,
        "PrintSettings",
        values([
            ("logo", logo),
            ("companyName", text(&options.company_name)),
            ("email", text(&options.email)),
            ("displayLogo", DocValue::Bool(options.logo.is_some())),
        ]),
    )
}

/// `createCurrencyRecords`: a currency for each currency in countryInfo.json.
fn create_currency_records(fyo: &mut Fyo) -> Result<(), Error> {
    let mut done: Vec<&str> = Vec::new();
    for info in country_info().values() {
        let Some(currency) = info.get("currency").and_then(Value::as_str) else {
            continue;
        };
        if currency.is_empty() || done.contains(&currency) {
            continue;
        }
        let field = |key: &str| info.get(key).and_then(Value::as_str).unwrap_or_default();
        let number =
            |key: &str, default: f64| info.get(key).and_then(Value::as_f64).unwrap_or(default);
        check_and_create_doc(
            fyo,
            "Currency",
            values([
                ("name", text(currency)),
                ("fraction", text(field("currency_fraction"))),
                (
                    "fractionUnits",
                    DocValue::Number(number("currency_fraction_units", 100.0)),
                ),
                (
                    "smallestValue",
                    DocValue::Number(number("smallest_currency_fraction_value", 0.01)),
                ),
                ("symbol", text(field("currency_symbol"))),
            ]),
        )?;
        done.push(currency);
    }
    Ok(())
}

/// `checkAndCreateDoc`: makes the document unless one with the same values exists. One
/// with the same name and other values is deleted first.
fn check_and_create_doc(fyo: &mut Fyo, schema_name: &str, data: DocValueMap) -> Result<(), Error> {
    let Some(DocValue::Text(name)) = data.get("name") else {
        return Err(Error::Value(format!("{schema_name} needs a name")));
    };
    if let Ok(stored) = fyo.db.get_doc(schema_name, name) {
        let same = data
            .iter()
            .all(|(key, value)| loosely_equal(value, stored.get(key)));
        if same {
            return Ok(());
        }
        fyo.db.delete(schema_name, name)?;
    }
    let mut doc = fyo.get_new_doc(schema_name, data)?;
    doc.sync(fyo)
}

/// JavaScript's `==` for the values compared by `checkIfExactRecordAbsent`.
fn loosely_equal(value: &DocValue, stored: Option<&DocValue>) -> bool {
    let number = |value: &DocValue| match value {
        DocValue::Number(n) => Some(*n),
        DocValue::Bool(b) => Some(f64::from(u8::from(*b))),
        DocValue::Money(m) => m.to_string().parse().ok(),
        DocValue::Text(t) => t.trim().parse().ok().or(t.trim().is_empty().then_some(0.0)),
        _ => None,
    };
    match (value, stored) {
        (DocValue::Null, None | Some(DocValue::Null)) => true,
        (_, None | Some(DocValue::Null)) | (DocValue::Null, _) => false,
        (DocValue::Text(a), Some(DocValue::Text(b))) => a == b,
        (a, Some(b)) => match (number(a), number(b)) {
            (Some(a), Some(b)) => a == b,
            _ => a == b,
        },
    }
}

/// `createAccountRecords`: the chart of accounts, the bank account, the discount account
/// and the default accounts.
fn create_account_records(
    bank_name: &str,
    country: &str,
    chart_of_accounts: &str,
    fyo: &mut Fyo,
) -> Result<(), Error> {
    let chart = get_coa(chart_of_accounts);
    create_coa_accounts(fyo, &chart, None, "")?;

    let parent_account = bank_account_parent_name(country, fyo)?;
    check_and_create_doc(
        fyo,
        "Account",
        values([
            ("name", text(bank_name)),
            ("rootType", text("Asset")),
            ("parentAccount", text(&parent_account)),
            ("accountType", text("Bank")),
            ("isGroup", DocValue::Bool(false)),
        ]),
    )?;
    create_discount_account(fyo)?;

    set_default_account(fyo, "writeOffAccount", "Write Off")?;
    if !set_default_account(fyo, "roundOffAccount", "Rounded Off")? {
        set_default_account(fyo, "roundOffAccount", "Round Off")?;
    }
    Ok(())
}

/// The keys of a chart's account that are its values, not its children (`accountFields`).
const ACCOUNT_FIELDS: [&str; 4] = ["accountType", "accountNumber", "rootType", "isGroup"];

/// `getCOA`: the tree of a chart of accounts, by its name.
fn get_coa(chart_of_accounts: &str) -> Map<String, Value> {
    let code = COA_LIST
        .iter()
        .find(|(name, _)| *name == chart_of_accounts)
        .map_or("", |(_, code)| *code);
    let json = match code {
        "ae" => include_str!("../../../vendor/books/fixtures/verified/ae.json"),
        "ca" => include_str!("../../../vendor/books/fixtures/verified/ca.json"),
        "gt" => include_str!("../../../vendor/books/fixtures/verified/gt.json"),
        "hu" => include_str!("../../../vendor/books/fixtures/verified/hu.json"),
        "id" => include_str!("../../../vendor/books/fixtures/verified/id.json"),
        "in" => include_str!("../../../vendor/books/fixtures/verified/in.json"),
        "mx" => include_str!("../../../vendor/books/fixtures/verified/mx.json"),
        "ni" => include_str!("../../../vendor/books/fixtures/verified/ni.json"),
        "nl" => include_str!("../../../vendor/books/fixtures/verified/nl.json"),
        "sg" => include_str!("../../../vendor/books/fixtures/verified/sg.json"),
        "fr" => include_str!("../../../vendor/books/fixtures/verified/fr.json"),
        _ => {
            return serde_json::from_str(include_str!(
                "../../../vendor/books/fixtures/verified/standardCOA.json"
            ))
            .expect("standardCOA.json is valid");
        }
    };
    let chart: Map<String, Value> = serde_json::from_str(json).expect("the chart is valid");
    match chart.get("tree") {
        Some(Value::Object(tree)) => tree.clone(),
        _ => Map::new(),
    }
}

/// `createCOAAccounts`: each account of the tree under `parent`.
fn create_coa_accounts(
    fyo: &mut Fyo,
    children: &Map<String, Value>,
    parent: Option<&str>,
    root_type: &str,
) -> Result<(), Error> {
    for (key, child) in children {
        if ACCOUNT_FIELDS.contains(&key.as_str()) {
            continue;
        }
        let Value::Object(child) = child else {
            continue;
        };
        let field = |key: &str| child.get(key).and_then(Value::as_str);
        let root_type = if parent.is_none() {
            field("rootType").unwrap_or_default()
        } else {
            root_type
        };
        let account_type = field("accountType").unwrap_or_default();
        let name = match field("accountNumber") {
            Some(number) if !number.is_empty() => format!("{key} - {number}"),
            _ => key.clone(),
        };
        // `identifyIsGroup`.
        let is_group = match child.get("isGroup") {
            Some(Value::Bool(is_group)) => *is_group,
            _ => child
                .keys()
                .any(|key| !ACCOUNT_FIELDS.contains(&key.as_str())),
        };

        let mut doc = fyo.get_new_doc(
            "Account",
            values([
                ("name", text(&name)),
                ("parentAccount", parent.map_or(DocValue::Null, text)),
                ("isGroup", DocValue::Bool(is_group)),
                ("rootType", text(root_type)),
                ("accountType", text(account_type)),
            ]),
        )?;
        doc.sync(fyo)?;
        create_coa_accounts(fyo, child, Some(&name), root_type)?;
    }
    Ok(())
}

/// `getBankAccountParentName`: the bank account group.
fn bank_account_parent_name(country: &str, fyo: &Fyo) -> Result<String, Error> {
    let mut options = GetAllOptions {
        fields: Some(vec!["name".into()]),
        ..GetAllOptions::default()
    };
    options.filters.insert(
        "isGroup".into(),
        latte_db::Filter::Equals(latte_db::FilterValue::Bool(true)),
    );
    options.filters.insert(
        "accountType".into(),
        latte_db::Filter::Equals("Bank".into()),
    );
    let groups = fyo.db.get_all_docs("Account", &options)?;
    if groups.len() > 1 && country == "Indonesia" {
        return Ok("Bank Rupiah - 1121.000".into());
    }
    Ok(match groups.first().and_then(|group| group.get("name")) {
        Some(DocValue::Text(name)) => name.clone(),
        // "This should not happen if the fixtures are correct."
        _ => "Bank Accounts".into(),
    })
}

/// `createDiscountAccount`.
fn create_discount_account(fyo: &mut Fyo) -> Result<(), Error> {
    let income = "Indirect Income";
    if !fyo.db.exists("Account", Some(income))? {
        return Ok(());
    }
    let discounts = "Discounts";
    check_and_create_doc(
        fyo,
        "Account",
        values([
            ("name", text(discounts)),
            ("rootType", text("Income")),
            ("parentAccount", text(income)),
            ("accountType", text("Income Account")),
            ("isGroup", DocValue::Bool(false)),
        ]),
    )?;
    set_single(
        fyo,
        "AccountingSettings",
        "discountAccount",
        text(discounts),
    )
}

/// `setDefaultAccount`: sets an accounting setting to an account if the account exists.
fn set_default_account(fyo: &mut Fyo, key: &str, account: &str) -> Result<bool, Error> {
    if !fyo.db.exists("Account", Some(account))? {
        return Ok(false);
    }
    set_single(fyo, "AccountingSettings", key, text(account))?;
    Ok(true)
}

/// `createRegionalRecords`: India's GST taxes.
fn create_regional_records(country: &str, fyo: &mut Fyo) -> Result<(), Error> {
    if country != "India" {
        return Ok(());
    }
    let rates = [28.0, 18.0, 12.0, 6.0, 5.0, 3.0, 0.25, 0.0];
    let types: [(&str, &[f64]); 4] = [
        ("GST", &rates),
        ("IGST", &rates),
        ("Exempt-GST", &[0.0]),
        ("Exempt-IGST", &[0.0]),
    ];
    for (tax_type, percents) in types {
        for &percent in percents {
            // `getTaxDetails`.
            let details = if tax_type == "GST" {
                vec![("CGST", percent / 2.0), ("SGST", percent / 2.0)]
            } else {
                vec![(tax_type.split('-').next().unwrap_or(tax_type), percent)]
            };
            let details = details
                .into_iter()
                .map(|(account, rate)| {
                    values([("account", text(account)), ("rate", DocValue::Number(rate))])
                })
                .collect();
            let mut tax = fyo.get_new_doc(
                "Tax",
                values([
                    ("name", text(&format!("{tax_type}-{percent}"))),
                    ("details", DocValue::Table(details)),
                ]),
            )?;
            tax.sync(fyo)?;
        }
    }
    Ok(())
}

/// `createDefaultEntries`: the units (`getDefaultUOMs`) and the Stores location.
fn create_default_entries(fyo: &mut Fyo) -> Result<(), Error> {
    for (name, is_whole) in [
        ("Unit", true),
        ("Kg", false),
        ("Gram", false),
        ("Meter", false),
        ("Hour", false),
        ("Day", false),
    ] {
        check_and_create_doc(
            fyo,
            "UOM",
            values([("name", text(name)), ("isWhole", DocValue::Bool(is_whole))]),
        )?;
    }
    check_and_create_doc(fyo, "Location", values([("name", text("Stores"))]))
}

/// `createDefaultNumberSeries`: the number series named in the schemas, set as the
/// defaults.
fn create_default_number_series(fyo: &mut Fyo) -> Result<(), Error> {
    let series: Vec<(String, String)> = fyo
        .db
        .schema_map()
        .values()
        .flat_map(|schema| schema.fields.iter().map(move |field| (schema, field)))
        .filter(|(_, field)| field.fieldname == "numberSeries")
        .filter_map(|(schema, field)| match &field.default {
            Some(latte_schema::RawValue::Text(prefix)) => {
                Some((prefix.clone(), schema.name.clone()))
            }
            _ => None,
        })
        .collect();

    for (prefix, schema_name) in series {
        create_number_series(&prefix, &schema_name, DEFAULT_SERIES_START, fyo)?;
        let Some(key) = crate::models::helpers::number_series_defaults_key(&schema_name) else {
            continue;
        };
        let current = fyo.get_value("Defaults", None, key);
        if crate::fyo::model::helpers::is_truthy(current.as_ref()) {
            continue;
        }
        set_single(fyo, "Defaults", key, text(&prefix))?;
    }
    Ok(())
}

/// `updateInventorySettings`: the stock accounts and the Stores location. Books also sets a
/// valuation method, which InventorySettings has no field for, so it isn't saved.
fn update_inventory_settings(fyo: &mut Fyo) -> Result<(), Error> {
    let mut settings = fyo.get_doc("InventorySettings", None)?;
    for (account_type, setting) in [
        ("Stock", "stockInHand"),
        ("Stock Received But Not Billed", "stockReceivedButNotBilled"),
        ("Cost of Goods Sold", "costOfGoodsSold"),
    ] {
        let mut options = GetAllOptions {
            fields: Some(vec!["name".into()]),
            ..GetAllOptions::default()
        };
        options.filters.insert(
            "accountType".into(),
            latte_db::Filter::Equals(account_type.into()),
        );
        options.filters.insert(
            "isGroup".into(),
            latte_db::Filter::Equals(latte_db::FilterValue::Bool(false)),
        );
        let accounts = fyo.db.get_all_docs("Account", &options)?;
        if let Some(DocValue::Text(name)) = accounts.first().and_then(|a| a.get("name")) {
            settings.set(fyo, setting, text(name))?;
        }
    }
    if fyo.db.exists("Location", Some("Stores"))? {
        settings.set(fyo, "defaultLocation", text("Stores"))?;
    }
    settings.sync(fyo)
}

/// Today in the system time zone.
pub fn today() -> Date {
    Zoned::now().date()
}
