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
// An amount keeps its symbol: the spaces of a right-aligned value don't break.
#let rows = data.rows.map(row => row.enumerate().map(((c, v)) => if aligns.at(c, default: left) == right { v.replace(" ", "\u{a0}") } else { v }))
#let cell(c, r, value) = grid.cell(
  align: aligns.at(c),
  stroke: (top: border, left: if c > 0 { border }),
  inset: px(8),
  text(size: px(12), weight: if r == 0 { "semibold" } else { "regular" }, value),
)
// `repeat(n, minmax(0, auto))`: each column as wide as its widest cell, and what is
// left of the page shared out between them, as a CSS grid stretches auto tracks. When
// the page is too narrow, each column keeps the width of its longest word, so a GSTIN
// or an amount isn't cut, and the rest of the page goes to the columns that wrap.
#layout(size => context {
  let width(v, weight) = measure(text(size: px(12), weight: weight, v)).width
  let longest(v, weight) = calc.max(0pt, ..v.split(" ").map(w => width(w, weight)))
  // The first rows are enough to size a column, and keep a long ledger quick.
  let sample = rows.slice(0, calc.min(100, rows.len()))
  let pad = px(16) + 1pt
  let natural = range(n).map(c => calc.max(
    width(data.columns.at(c).label, "semibold"),
    ..sample.map(row => width(row.at(c, default: ""), "regular")),
  ) + pad)
  let least = range(n).map(c => calc.max(
    longest(data.columns.at(c).label, "semibold"),
    ..sample.map(row => longest(row.at(c, default: ""), "regular")),
  ) + pad)
  let free = size.width - natural.sum(default: 0pt)
  let room = size.width - least.sum(default: 0pt)
  let wanted = range(n).map(c => natural.at(c) - least.at(c))
  let columns = if n == 0 { () } else if free >= 0pt {
    natural.map(w => w + free / n)
  } else if room > 0pt and wanted.sum() > 0pt {
    range(n).map(c => least.at(c) + wanted.at(c) * (room / wanted.sum()))
  } else { least }
  grid(
  columns: columns,
  ..data.columns.enumerate().map(((c, col)) => cell(c, 0, col.label)),
  ..rows.enumerate().map(((r, row)) => row.enumerate().map(((c, v)) => cell(c, r + 1, v))).flatten(),
  )
})
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
