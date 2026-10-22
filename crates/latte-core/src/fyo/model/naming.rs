// Port of fyo/model/naming.ts @ v0.36.0.
//
// Deviation: a Number Series that is set but isn't text (null, say) is an error. Books
// would look up the Number Series `null` and fail with "NumberSeries is not a Single
// Schema".

use latte_db::{DocValue, DocValueMap};
use latte_schema::Naming;

use crate::Error;
use crate::fyo::model::doc::Doc;
use crate::fyo::models::number_series;
use crate::fyo::{DEFAULT_SERIES_START, Fyo};

/// `isNameAutoSet`.
pub fn is_name_auto_set(schema_name: &str, fyo: &Fyo) -> bool {
    let Some(schema) = fyo.schema(schema_name) else {
        return false;
    };
    match schema.naming {
        Some(Naming::Manual) => false,
        Some(Naming::Autoincrement | Naming::Random) => true,
        _ => schema.field("numberSeries").is_some(),
    }
}

/// `setName`: names a document that is about to be inserted.
pub fn set_name(doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
    if doc.schema().naming == Some(Naming::Manual) {
        return Ok(());
    }

    if doc.schema().naming == Some(Naming::Autoincrement) {
        let name = get_next_id(doc.schema_name(), fyo)?;
        doc.set_name(DocValue::Text(name));
        return Ok(());
    }

    if let Some(number_series) = doc.get("numberSeries") {
        let DocValue::Text(prefix) = number_series else {
            return Err(Error::Value(format!(
                "Number Series for {} is not set",
                doc.schema().label
            )));
        };
        let prefix = prefix.clone();
        let name = get_series_next(&prefix, doc.schema_name(), fyo)?;
        doc.set_name(DocValue::Text(name));
        return Ok(());
    }

    if doc.schema().is_single {
        let name = doc.schema_name().to_owned();
        doc.set_name(DocValue::Text(name));
        return Ok(());
    }

    if doc.name().is_none_or(str::is_empty) {
        doc.set_name(DocValue::Text(latte_db::get_random_string()));
    }
    Ok(())
}

/// `getNextId`.
pub fn get_next_id(schema_name: &str, fyo: &Fyo) -> Result<String, Error> {
    let last_inserted = fyo.db.core().get_last_inserted(schema_name)?;
    Ok(format!("{:0>9}", last_inserted + 1))
}

/// `getSeriesNext`: the next name in a Number Series, which is made if it doesn't exist.
pub fn get_series_next(prefix: &str, schema_name: &str, fyo: &mut Fyo) -> Result<String, Error> {
    let mut series = match fyo.get_doc("NumberSeries", Some(prefix)) {
        Ok(series) => series,
        Err(Error::NotFound(_)) => {
            create_number_series(prefix, schema_name, DEFAULT_SERIES_START, fyo)?;
            fyo.get_doc("NumberSeries", Some(prefix))?
        }
        Err(e) => return Err(e),
    };

    number_series::next(&mut series, fyo, schema_name)
}

/// `createNumberSeries`.
pub fn create_number_series(
    prefix: &str,
    reference_type: &str,
    start: f64,
    fyo: &mut Fyo,
) -> Result<(), Error> {
    if fyo.db.core().exists("NumberSeries", Some(prefix))? {
        return Ok(());
    }

    let data = DocValueMap::from([
        ("name".to_owned(), DocValue::Text(prefix.to_owned())),
        ("start".to_owned(), DocValue::Number(start)),
        (
            "referenceType".to_owned(),
            DocValue::Text(reference_type.to_owned()),
        ),
    ]);
    let mut series = fyo.get_new_doc("NumberSeries", data)?;
    series.sync(fyo)
}
