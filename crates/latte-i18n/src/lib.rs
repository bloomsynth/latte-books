//! Loads Frappe Books translation CSVs (`vendor/books/translations`, see [`translation`]) and formats
//! numbers, dates and currency.
//!
//! Port of `main/getLanguageMap.ts`, `fyo/utils/translation.ts` and the formatting in
//! `fyo/utils/format.ts` @ v0.36.0.

pub mod format;
pub mod translation;
