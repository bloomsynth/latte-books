// Port of models/baseModels/Lead/Lead.ts @ v0.36.0.
//
// Not ported yet, as the UI needs them: `getListViewSettings`.

use latte_db::DocValue;

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::types::{DocContext, Model};
use crate::models::base_models::party::{validate_email, validate_phone_number};

pub struct Lead;

impl Model for Lead {
    fn validate_field(
        &self,
        fieldname: &str,
        value: &DocValue,
        _ctx: &DocContext<'_>,
    ) -> Result<(), Error> {
        match fieldname {
            "email" => validate_email(value),
            "mobile" => validate_phone_number(value),
            _ => Ok(()),
        }
    }
}

/// `createCustomer`: a new customer with the lead's details.
pub fn create_customer(lead: &Doc, fyo: &Fyo) -> Result<Doc, Error> {
    let mut data = lead.get_valid_dict(false, false);
    data.insert(
        "fromLead".to_owned(),
        DocValue::Text(lead.name().unwrap_or_default().to_owned()),
    );
    data.insert(
        "phone".to_owned(),
        lead.get("mobile").cloned().unwrap_or(DocValue::Null),
    );
    data.insert("role".to_owned(), DocValue::Text("Customer".to_owned()));
    fyo.get_new_doc("Party", data)
}

/// `createSalesQuote`: a new quote for the lead.
pub fn create_sales_quote(lead: &Doc, fyo: &Fyo) -> Result<Doc, Error> {
    let data = latte_db::DocValueMap::from([
        (
            "party".to_owned(),
            DocValue::Text(lead.name().unwrap_or_default().to_owned()),
        ),
        (
            "referenceType".to_owned(),
            DocValue::Text("Lead".to_owned()),
        ),
    ]);
    fyo.get_new_doc("SalesQuote", data)
}
