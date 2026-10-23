// Port of models/baseModels/Address/Address.ts @ v0.36.0, and of
// models/regionalModels/in/Address.ts, India's, which adds the `pos` (place of supply)
// formula and list.
//
// Not ported yet, as the UI needs them: `emptyMessages` and `getListViewSettings`.

use latte_db::DocValue;
use latte_schema::SelectOption;

use crate::fyo::model::helpers::{is_truthy, js_string};
use crate::fyo::model::types::{DocContext, Formula, Model};
use crate::models::helpers::text;
use crate::setup::country_info;

/// India's states and union territories by GST state code: `codeStateMap` in
/// regional/in.ts.
pub const CODE_STATE_MAP: [(&str, &str); 36] = [
    ("01", "Jammu and Kashmir"),
    ("02", "Himachal Pradesh"),
    ("03", "Punjab"),
    ("04", "Chandigarh"),
    ("05", "Uttarakhand"),
    ("06", "Haryana"),
    ("07", "Delhi"),
    ("08", "Rajasthan"),
    ("09", "Uttar Pradesh"),
    ("10", "Bihar"),
    ("11", "Sikkim"),
    ("12", "Arunachal Pradesh"),
    ("13", "Nagaland"),
    ("14", "Manipur"),
    ("15", "Mizoram"),
    ("16", "Tripura"),
    ("17", "Meghalaya"),
    ("18", "Assam"),
    ("19", "West Bengal"),
    ("20", "Jharkhand"),
    ("21", "Odisha"),
    ("22", "Chattisgarh"),
    ("23", "Madhya Pradesh"),
    ("24", "Gujarat"),
    ("26", "Dadra and Nagar Haveli and Daman and Diu"),
    ("27", "Maharashtra"),
    ("29", "Karnataka"),
    ("30", "Goa"),
    ("31", "Lakshadweep"),
    ("32", "Kerala"),
    ("33", "Tamil Nadu"),
    ("34", "Puducherry"),
    ("35", "Andaman and Nicobar Islands"),
    ("36", "Telangana"),
    ("37", "Andhra Pradesh"),
    ("38", "Ladakh"),
];

/// India's states, sorted.
pub fn indian_states() -> Vec<&'static str> {
    let mut states: Vec<_> = CODE_STATE_MAP.iter().map(|(_, state)| *state).collect();
    states.sort_unstable();
    states
}

fn options(values: Vec<&str>) -> Vec<SelectOption> {
    values
        .into_iter()
        .map(|value| SelectOption {
            value: value.to_owned(),
            label: value.to_owned(),
        })
        .collect()
}

/// The `addressDisplay` formula: the address's parts that are set, joined by commas.
fn address_display() -> Formula {
    Formula {
        fieldname: "addressDisplay",
        depends_on: Some(&[
            "addressLine1",
            "addressLine2",
            "city",
            "state",
            "country",
            "postalCode",
        ]),
        formula: |ctx, _| {
            let parts: Vec<String> = [
                "addressLine1",
                "addressLine2",
                "city",
                "state",
                "country",
                "postalCode",
            ]
            .into_iter()
            .filter_map(|fieldname| {
                let value = ctx.doc.get(fieldname);
                is_truthy(value).then(|| js_string(value))
            })
            .collect();
            Ok(Some(DocValue::Text(parts.join(", ")).into()))
        },
    }
}

/// The `state` and `country` lists.
fn base_list(fieldname: &str, ctx: Option<&DocContext<'_>>) -> Option<Vec<SelectOption>> {
    match fieldname {
        "state" => {
            let country = ctx.and_then(|ctx| text(ctx.doc, "country"));
            Some(match country {
                Some("India") => options(indian_states()),
                _ => Vec::new(),
            })
        }
        "country" => {
            let mut countries: Vec<&str> = country_info().keys().map(String::as_str).collect();
            countries.sort_unstable();
            Some(options(countries))
        }
        _ => None,
    }
}

pub struct Address;

impl Model for Address {
    fn formulas(&self) -> Vec<Formula> {
        vec![address_display()]
    }

    fn list(&self, fieldname: &str, ctx: Option<&DocContext<'_>>) -> Option<Vec<SelectOption>> {
        base_list(fieldname, ctx)
    }
}

/// India's Address.
pub struct IndianAddress;

impl Model for IndianAddress {
    fn formulas(&self) -> Vec<Formula> {
        vec![
            address_display(),
            Formula {
                fieldname: "pos",
                depends_on: Some(&["state"]),
                formula: |ctx, _| {
                    let state = text(ctx.doc, "state").unwrap_or_default();
                    let pos = if CODE_STATE_MAP.iter().any(|(_, s)| *s == state) {
                        state
                    } else {
                        ""
                    };
                    Ok(Some(DocValue::Text(pos.to_owned()).into()))
                },
            },
        ]
    }

    fn list(&self, fieldname: &str, ctx: Option<&DocContext<'_>>) -> Option<Vec<SelectOption>> {
        if fieldname == "pos" {
            return Some(options(indian_states()));
        }
        base_list(fieldname, ctx)
    }
}
