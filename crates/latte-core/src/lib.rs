//! Port of `fyo/` (documents and their models) and, later, `models/` and `reports/`
//! @ v0.36.0. This crate must never depend on GPUI.

pub mod config;
pub mod dashboard;
pub mod dummy;
pub mod fyo;
pub mod importer;
pub mod models;
pub mod pos;
pub mod reports;
pub mod setup;
pub mod views;

use std::fmt;

pub use crate::fyo::Fyo;
pub use crate::fyo::model::doc::Doc;
pub use crate::fyo::model::types::{
    BaseModel, DocContext, Formula, FormulaFn, FormulaValue, Model,
};

/// `fyo/utils/errors.ts`, for the errors documents raise.
#[derive(Debug)]
pub enum Error {
    Db(latte_db::Error),
    Pesa(latte_db::pesa::PesaError),
    /// `ValidationError`: a value that isn't allowed.
    Validation(String),
    /// `MandatoryError`: required values that are missing.
    Mandatory(String),
    /// `ValueError`: a value that isn't one of a field's options.
    Value(String),
    /// `ConflictError`: the document changed in the file after it was loaded.
    Conflict(String),
    /// `NotFoundError`, which Books gives the status code 404.
    NotFound(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Db(e) => write!(f, "{e}"),
            Self::Pesa(e) => write!(f, "{e}"),
            Self::Validation(m)
            | Self::Mandatory(m)
            | Self::Value(m)
            | Self::Conflict(m)
            | Self::NotFound(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Error {}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Db(e.into())
    }
}

impl From<latte_db::Error> for Error {
    fn from(e: latte_db::Error) -> Self {
        match e {
            latte_db::Error::NotFound(m) => Self::NotFound(m),
            e => Self::Db(e),
        }
    }
}

impl From<latte_db::pesa::PesaError> for Error {
    fn from(e: latte_db::pesa::PesaError) -> Self {
        Self::Pesa(e)
    }
}

#[cfg(test)]
mod setup_tests;
#[cfg(test)]
mod tests;
