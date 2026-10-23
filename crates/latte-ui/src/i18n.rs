//! The app's language: `src/utils/language.ts` @ v0.36.0.
//!
//! The language is read from `language` in the config when the app starts, as in Books.
//! Changing it in the selector rebuilds the screen; Books reloads the window.

use std::sync::RwLock;

use latte_i18n::translation::{DEFAULT_LANGUAGE, LanguageMap, stitch};

use crate::session;

static MAP: RwLock<Option<LanguageMap>> = RwLock::new(None);

/// The language in the config, or English.
pub fn language() -> String {
    session::config_value("language").unwrap_or_else(|| DEFAULT_LANGUAGE.to_owned())
}

/// `setLanguageMap`: loads the language in the config.
pub fn load() {
    let map = LanguageMap::for_language(&language());
    if let Ok(mut current) = MAP.write() {
        *current = map;
    }
}

/// Stores a new language and loads it.
pub fn set_language(language: &str) {
    session::set_config_value("language", language);
    load();
}

/// `t` for a string without arguments.
pub fn t(text: &str) -> String {
    tr(text, &[])
}

/// `t` with `${0}`-style arguments.
pub fn tr(format: &str, args: &[&str]) -> String {
    match MAP.read().ok().as_deref() {
        Some(Some(map)) => map.translate(format, args),
        _ => stitch(format, args),
    }
}

/// The translation of a schema's text, or `None` when there is none.
pub fn translation(text: &str) -> Option<String> {
    match MAP.read().ok().as_deref() {
        Some(Some(map)) => map.get(text).map(str::to_owned),
        _ => None,
    }
}
