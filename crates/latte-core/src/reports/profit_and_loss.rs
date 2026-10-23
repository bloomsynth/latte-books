// Port of reports/ProfitAndLoss/ProfitAndLoss.ts @ v0.36.0.
//
// Books bugs Latte fixes:
// - With income but no expenses, the report shows the income and its total. Books
//   shows nothing.
// - With expenses but no income, the total is labelled "Total Expense (Debit)". Books
//   labels it "Total Income (Credit)".

use jiff::Zoned;

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::format::Formatter;
use crate::reports::account_report::{
    AccountListNode, AccountReport, AccountTreeNode, RangeValue, account_list, account_map,
    account_tree, root_nodes,
};
use crate::reports::{CellColor, Column, Filter, FilterValue, RawValue, Report, ReportRow};

#[derive(Debug, Clone, Default)]
pub struct ProfitAndLoss {
    pub base: AccountReport,
}

impl ProfitAndLoss {
    /// `setReportData` for a given day.
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

        let income_roots = root_nodes(&tree, "Income");
        let income_rows = base.rows(fyo, formatter, &account_list(&income_roots));
        let expense_roots = root_nodes(&tree, "Expense");
        let expense_rows = base.rows(fyo, formatter, &account_list(&expense_roots));

        match (income_roots.is_empty(), expense_roots.is_empty()) {
            (false, true) => self.single_rows(
                fyo,
                formatter,
                &income_roots,
                income_rows,
                "Total Income (Credit)",
            ),
            (true, false) => self.single_rows(
                fyo,
                formatter,
                &expense_roots,
                expense_rows,
                "Total Expense (Debit)",
            ),
            (true, true) => Ok(Vec::new()),
            (false, false) => self.both_rows(
                fyo,
                formatter,
                income_rows,
                expense_rows,
                &income_roots,
                &expense_roots,
            ),
        }
    }

    /// `getIncomeOrExpenseRows`.
    fn single_rows(
        &self,
        fyo: &Fyo,
        formatter: &Formatter,
        roots: &[AccountTreeNode],
        mut rows: Vec<ReportRow>,
        total_name: &str,
    ) -> Result<Vec<ReportRow>, Error> {
        let total = self.base.total_node(fyo, roots, total_name)?;
        rows.push(self.base.row(fyo, formatter, &total));
        Ok(rows)
    }

    /// `getIncomeAndExpenseRows`.
    fn both_rows(
        &self,
        fyo: &Fyo,
        formatter: &Formatter,
        income_rows: Vec<ReportRow>,
        expense_rows: Vec<ReportRow>,
        income_roots: &[AccountTreeNode],
        expense_roots: &[AccountTreeNode],
    ) -> Result<Vec<ReportRow>, Error> {
        let base = &self.base;
        let total_income = base.total_node(fyo, income_roots, "Total Income (Credit)")?;
        let total_expense = base.total_node(fyo, expense_roots, "Total Expense (Debit)")?;

        let income_values = total_income.value_map.clone().unwrap_or_default();
        let expense_values = total_expense.value_map.clone().unwrap_or_default();
        let mut profit_values = Vec::new();
        for (income, expense) in income_values.iter().zip(&expense_values) {
            let zero = fyo.pesa().zero();
            let income = income.as_ref().map_or(zero.clone(), |v| v.balance.clone());
            let expense = expense.as_ref().map_or(zero.clone(), |v| v.balance.clone());
            profit_values.push(Some(RangeValue {
                balance: income.sub(&expense)?,
                debit: zero.clone(),
                credit: zero,
            }));
        }
        let profit = AccountListNode {
            name: "Total Profit".to_owned(),
            is_group: false,
            value_map: Some(profit_values),
            level: 0,
        };
        let mut profit_row = base.row(fyo, formatter, &profit);
        for cell in &mut profit_row.cells {
            cell.bold = true;
            if let RawValue::Money(money) = &cell.raw_value {
                if money.is_positive() {
                    cell.color = Some(CellColor::Green);
                } else if money.is_negative() {
                    cell.color = Some(CellColor::Red);
                }
            }
        }

        let empty = base.empty_row(formatter);
        let mut rows = income_rows;
        rows.push(base.row(fyo, formatter, &total_income));
        rows.push(empty.clone());
        rows.extend(expense_rows);
        rows.push(base.row(fyo, formatter, &total_expense));
        rows.push(empty);
        rows.push(profit_row);
        Ok(rows)
    }
}

impl Report for ProfitAndLoss {
    fn title(&self) -> &'static str {
        "Profit And Loss"
    }

    fn report_name(&self) -> &'static str {
        "profit-and-loss"
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
