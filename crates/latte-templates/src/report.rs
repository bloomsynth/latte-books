//! What a report's printout shows: `src/pages/PrintView/ReportPrintView.vue` @ v0.36.0.
//! `latte-print` lays it out.

use serde_json::{Value, json};

/// A column of the printout.
#[derive(Debug, Clone)]
pub struct ReportColumn {
    pub label: String,
    /// `left`, `right` or `center`.
    pub align: &'static str,
}

/// What a report printout shows.
#[derive(Debug, Clone)]
pub struct ReportPrint {
    pub company_name: String,
    pub title: String,
    pub columns: Vec<ReportColumn>,
    /// The cells' shown values, a list per row.
    pub rows: Vec<Vec<String>>,
    /// When it was printed, as shown at its foot.
    pub printed: String,
    /// The page, in centimetres.
    pub width: f64,
    pub height: f64,
}

impl ReportPrint {
    /// The values the report's layout reads.
    pub fn data(&self) -> Value {
        json!({
            "company": self.company_name,
            "title": self.title,
            "columns": self.columns.iter().map(|c| json!({ "label": c.label, "align": c.align })).collect::<Vec<_>>(),
            "rows": self.rows,
            "printed": self.printed,
            "width": self.width,
            "height": self.height,
        })
    }
}
