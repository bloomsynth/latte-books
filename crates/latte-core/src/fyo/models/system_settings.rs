// Port of fyo/models/SystemSettings.ts @ v0.36.0.
//
// Not ported yet: `lists` for locale and currency, which need the country info.

use latte_db::DocValue;

use crate::Error;
use crate::fyo::model::types::{DocContext, Model};

pub struct SystemSettings;

impl Model for SystemSettings {
    fn validate_field(
        &self,
        fieldname: &str,
        value: &DocValue,
        _ctx: &DocContext<'_>,
    ) -> Result<(), Error> {
        if fieldname != "displayPrecision" {
            return Ok(());
        }
        if let DocValue::Number(n) = value
            && (0.0..=9.0).contains(n)
        {
            return Ok(());
        }

        Err(Error::Validation(
            "Display Precision should have a value between 0 and 9.".into(),
        ))
    }
}
