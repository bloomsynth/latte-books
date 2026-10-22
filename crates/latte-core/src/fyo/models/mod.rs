// Port of fyo/models/index.ts @ v0.36.0: the models every file has.
//
// Not ported yet: CustomForm and CustomField, which only the custom-field UI uses.

pub mod number_series;
pub mod system_settings;

use std::collections::HashMap;
use std::sync::Arc;

use crate::fyo::model::types::Model;

/// `coreModels`.
pub fn core_models() -> HashMap<&'static str, Arc<dyn Model>> {
    HashMap::from([
        (
            "NumberSeries",
            Arc::new(number_series::NumberSeries) as Arc<dyn Model>,
        ),
        (
            "SystemSettings",
            Arc::new(system_settings::SystemSettings) as Arc<dyn Model>,
        ),
    ])
}
