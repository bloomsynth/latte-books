//! The open company file, shared by every screen.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use latte_core::Fyo;
use latte_core::fyo::format::Formatter;
use latte_db::{Database, DocValue};

pub use latte_core::config::*;

/// The open company.
pub struct Session {
    pub fyo: Fyo,
    pub formatter: Formatter,
    pub path: PathBuf,
    /// The search's filters while it is closed (Books keeps one `Search` while a
    /// company is open).
    pub search_filters: Option<latte_core::views::search::SearchFilters>,
}

pub type Shared = Rc<RefCell<Session>>;

impl Session {
    /// Opens a company file for reading and writing.
    pub fn open(path: &Path) -> Result<Self, String> {
        let db = Database::open(path, device_id()).map_err(|e| e.to_string())?;
        Self::from_fyo(Fyo::with_models(db), path)
    }

    pub fn from_fyo(mut fyo: Fyo, path: &Path) -> Result<Self, String> {
        fyo.translate_schemas(&crate::i18n::translation);
        let formatter = Formatter::new(&fyo).map_err(|e| e.to_string())?;
        Ok(Self {
            fyo,
            formatter,
            path: path.to_owned(),
            search_filters: None,
        })
    }

    /// Reads the settings again after they change.
    pub fn refresh_formatter(&mut self) {
        if let Ok(formatter) = Formatter::new(&self.fyo) {
            self.formatter = formatter;
        }
    }

    pub fn company_name(&self) -> String {
        match self
            .fyo
            .get_value("AccountingSettings", None, "companyName")
        {
            Some(DocValue::Text(name)) => name,
            _ => String::new(),
        }
    }

    /// SystemSettings' Dark Mode.
    pub fn dark_mode(&self) -> bool {
        latte_core::fyo::model::helpers::is_truthy(
            self.fyo
                .get_value("SystemSettings", None, "darkMode")
                .as_ref(),
        )
    }

    pub fn setup_complete(&self) -> bool {
        latte_core::fyo::model::helpers::is_truthy(
            self.fyo
                .get_value("AccountingSettings", None, "setupComplete")
                .as_ref(),
        )
    }
}
