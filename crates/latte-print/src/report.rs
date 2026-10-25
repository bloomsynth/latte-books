//! A report's printout: `src/pages/PrintView/ReportPrintView.vue` @ v0.36.0.
//!
//! The company's name and the report's title, a grid of the picked columns and rows,
//! and the time it was printed, on pages of the picked size.

use std::collections::HashMap;

use serde_json::json;

use crate::{PrintError, Printout};

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

const SOURCE: &str = r##"
#let data = json("/report.json")
#let px(n) = n * 0.75pt
#set page(width: data.width * 1cm, height: data.height * 1cm, margin: 0pt)
#set text(font: ("Inter", "Liberation Sans"), size: px(16), top-edge: 1.113em, bottom-edge: -0.387em)
#set par(spacing: 0pt, leading: 0pt)
#set block(spacing: 0pt)
#let border = px(1) + rgb("#EDEDED")

#block(width: 100%, inset: px(8), text(size: px(18), weight: "semibold")[
  #data.company #h(1fr) #text(fill: rgb("#7C7C7C"), data.title)
])
#let n = data.columns.len()
#let aligns = data.columns.map(c => if c.align == "right" { right } else if c.align == "center" { center } else { left })
#let cell(c, r, value) = grid.cell(
  align: aligns.at(c),
  stroke: (top: border, left: if c > 0 { border }),
  inset: px(8),
  text(size: px(12), weight: if r == 0 { "semibold" } else { "regular" }, value),
)
#grid(
  columns: (auto,) * n,
  ..data.columns.enumerate().map(((c, col)) => cell(c, 0, col.label)),
  ..data.rows.enumerate().map(((r, row)) => row.enumerate().map(((c, v)) => cell(c, r + 1, v))).flatten(),
)
#block(width: 100%, inset: px(8), stroke: (top: border), align(right, text(size: px(11), data.printed)))
"##;

/// Lays the report out.
pub fn print_report(report: &ReportPrint) -> Result<Printout, Vec<PrintError>> {
    let data = json!({
        "company": report.company_name,
        "title": report.title,
        "columns": report.columns.iter().map(|c| json!({ "label": c.label, "align": c.align })).collect::<Vec<_>>(),
        "rows": report.rows,
        "printed": report.printed,
        "width": report.width,
        "height": report.height,
    });
    let mut files = HashMap::new();
    files.insert(
        "/report.json".to_owned(),
        serde_json::to_vec(&data).map_err(|e| vec![PrintError::new(e.to_string())])?,
    );
    crate::render_plain(SOURCE, files)
}
