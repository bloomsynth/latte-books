// Port of reports/BalanceSheet/BalanceSheet.ts @ v0.36.0.

use jiff::Zoned;

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::format::Formatter;
use crate::reports::account_report::{
    AccountReport, account_list, account_map, account_tree, root_nodes,
};
use crate::reports::{Column, Filter, FilterValue, Report, ReportRow};

#[derive(Debug, Clone, Default)]
pub struct BalanceSheet {
    pub base: AccountReport,
}

impl BalanceSheet {
    /// `setReportData` for a given day: assets, liabilities and equity, each with its
    /// total, with a blank row between them.
    pub fn run_on(
        &mut self,
        fyo: &Fyo,
        formatter: &Formatter,
        today: jiff::civil::Date,
    ) -> Result<Vec<ReportRow>, Error> {
        self.base.set_default_filters(fyo, today);
        let base = &self.base;
        let accounts = account_map(fyo)?;
        let map = base.grouped_entries(fyo, base.query_filters(fyo))?;
        let values = base.balances_by_range(fyo, &accounts, &map)?;
        let tree = account_tree(fyo, &accounts, &values)?;

        let mut rows = Vec::new();
        for (root_type, total_name) in [
            ("Asset", "Total Asset (Debit)"),
            ("Liability", "Total Liability (Credit)"),
            ("Equity", "Total Equity (Credit)"),
        ] {
            let roots = root_nodes(&tree, root_type);
            if roots.is_empty() {
                continue;
            }
            rows.extend(base.rows(fyo, formatter, &account_list(&roots)));
            let total = base.total_node(fyo, &roots, total_name)?;
            rows.push(base.row(fyo, formatter, &total));
            rows.push(base.empty_row(formatter));
        }
        if rows.last().is_some_and(|row| row.is_empty) {
            rows.pop();
        }
        Ok(rows)
    }
}

impl Report for BalanceSheet {
    fn title(&self) -> &'static str {
        "Balance Sheet"
    }

    fn report_name(&self) -> &'static str {
        "balance-sheet"
    }

    fn filters(&self, _fyo: &Fyo) -> Vec<Filter> {
        self.base.filters()
    }

    fn get(&self, fieldname: &str) -> FilterValue {
        self.base.get(fieldname)
    }

    fn set(&mut self, fieldname: &str, value: FilterValue) {
        self.base.set(fieldname, value);
    }

    fn columns(&self, _fyo: &Fyo, formatter: &Formatter) -> Result<Vec<Column>, Error> {
        Ok(self.base.columns(formatter))
    }

    fn run(&mut self, fyo: &Fyo, formatter: &Formatter) -> Result<Vec<ReportRow>, Error> {
        self.run_on(fyo, formatter, Zoned::now().date())
    }
}
