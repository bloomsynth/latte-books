// Port of reports/TrialBalance/TrialBalance.ts @ v0.36.0.
//
// Books bugs Latte fixes:
// - Closing is the opening plus the period. Books shows the entries after the To Date
//   there instead.
// - The period includes the To Date. Books leaves it out, and its default To Date is the
//   day before the fiscal year ends, so the last two days of the year were missing.
// - The default period is the fiscal year today is in (see account_report.rs).
// - An entry after the To Date is left out. Books stops with "invalid entry in trial
//   balance" for one before the year 0001 or after 9999.

use indexmap::IndexMap;
use jiff::civil::Date;
use jiff::{ToSpan, Zoned};
use latte_db::{Filter as DbFilter, FilterValue as DbFilterValue, QueryFilter};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::format::Formatter;
use crate::reports::account_report::{
    ACC_BAL_WIDTH, ACC_NAME_WIDTH, AccountListNode, AccountReport, DateRange, RangeValue, ValueMap,
    account_list, account_map, account_tree, current_fiscal_year, name_cell, root_nodes,
};
use crate::reports::{
    Align, Column, Filter, FilterKind, FilterValue, RawValue, Report, ReportCell, ReportRow,
};

#[derive(Debug, Clone, Default)]
pub struct TrialBalance {
    pub from_date: Option<Date>,
    pub to_date: Option<Date>,
    pub hide_group_amounts: bool,
}

impl TrialBalance {
    /// `setDefaultFilters`: the fiscal year today is in.
    pub fn set_default_filters(&mut self, fyo: &Fyo, today: Date) {
        if self.from_date.is_none() || self.to_date.is_none() {
            let (from, to) = current_fiscal_year(fyo, today);
            self.from_date = Some(from);
            self.to_date = Some(to);
        }
    }

    /// The opening range (before the From Date) and the period.
    fn date_ranges(&self) -> Vec<DateRange> {
        let from_date = self.from_date.unwrap_or(Date::constant(2000, 1, 1));
        let to_date = self.to_date.unwrap_or(from_date);
        vec![
            DateRange {
                from_date: Date::constant(1, 1, 1),
                to_date: from_date,
            },
            DateRange {
                from_date,
                to_date: to_date.checked_add(1.day()).unwrap_or(to_date),
            },
        ]
    }

    /// `setReportData` for a given day.
    pub fn run_on(
        &mut self,
        fyo: &Fyo,
        formatter: &Formatter,
        today: Date,
    ) -> Result<Vec<ReportRow>, Error> {
        self.set_default_filters(fyo, today);
        let base = AccountReport {
            date_ranges: self.date_ranges(),
            ..AccountReport::default()
        };
        let mut filters = QueryFilter::new();
        filters.insert(
            "reverted".to_owned(),
            DbFilter::Equals(DbFilterValue::Bool(false)),
        );
        let accounts = account_map(fyo)?;
        let map = base.grouped_entries(fyo, filters)?;

        // `_getGroupedByDateRanges`: debits and credits per range.
        let mut values: IndexMap<String, ValueMap> = IndexMap::new();
        for (account, entries) in &map {
            let mut value_map: ValueMap = vec![None; base.date_ranges.len()];
            for entry in entries {
                let Some(key) = base.range_index(entry) else {
                    continue;
                };
                let zero = fyo.pesa().zero();
                let total = value_map[key].clone().unwrap_or(RangeValue {
                    balance: zero.clone(),
                    debit: zero.clone(),
                    credit: zero.clone(),
                });
                value_map[key] = Some(RangeValue {
                    debit: total.debit.add(entry.debit.as_ref().unwrap_or(&zero))?,
                    credit: total.credit.add(entry.credit.as_ref().unwrap_or(&zero))?,
                    balance: zero,
                });
            }
            values.insert(account.clone(), value_map);
        }
        let tree = account_tree(fyo, &accounts, &values)?;

        let mut rows = Vec::new();
        for root_type in ["Asset", "Liability", "Income", "Expense", "Equity"] {
            let roots = root_nodes(&tree, root_type);
            if roots.is_empty() {
                continue;
            }
            for node in account_list(&roots) {
                rows.push(self.row(fyo, formatter, &node)?);
            }
            rows.push(self.empty_row());
        }
        rows.pop();
        Ok(rows)
    }

    /// `getRowFromAccountListNode`: opening, period and closing debits and credits.
    fn row(
        &self,
        fyo: &Fyo,
        formatter: &Formatter,
        node: &AccountListNode,
    ) -> Result<ReportRow, Error> {
        let zero = fyo.pesa().zero();
        let value = |key: usize| {
            node.value_map
                .as_ref()
                .and_then(|values| values.get(key).cloned().flatten())
        };
        let (opening, period) = (value(0), value(1));
        let debit = |v: &Option<RangeValue>| v.as_ref().map_or(zero.clone(), |v| v.debit.clone());
        let credit = |v: &Option<RangeValue>| v.as_ref().map_or(zero.clone(), |v| v.credit.clone());
        let amounts = [
            debit(&opening),
            credit(&opening),
            debit(&period),
            credit(&period),
            debit(&opening).add(&debit(&period))?,
            credit(&opening).add(&credit(&period))?,
        ];

        let hide = self.hide_group_amounts && node.is_group;
        let mut cells = vec![name_cell(node)];
        for amount in amounts {
            cells.push(ReportCell {
                value: if hide {
                    String::new()
                } else {
                    formatter.currency(&amount)
                },
                raw_value: RawValue::Money(amount),
                align: Align::Right,
                width: ACC_BAL_WIDTH,
                ..ReportCell::default()
            });
        }
        Ok(ReportRow {
            cells,
            level: Some(node.level),
            is_group: node.is_group,
            ..ReportRow::default()
        })
    }

    fn empty_row(&self) -> ReportRow {
        let widths = [ACC_NAME_WIDTH].into_iter().chain([ACC_BAL_WIDTH; 6]);
        ReportRow {
            is_empty: true,
            cells: widths
                .map(|width| ReportCell {
                    width,
                    ..ReportCell::default()
                })
                .collect(),
            ..ReportRow::default()
        }
    }
}

impl Report for TrialBalance {
    fn title(&self) -> &'static str {
        "Trial Balance"
    }

    fn report_name(&self) -> &'static str {
        "trial-balance"
    }

    fn filters(&self, _fyo: &Fyo) -> Vec<Filter> {
        vec![
            Filter::new("fromDate", "From Date", FilterKind::Date).required(),
            Filter::new("toDate", "To Date", FilterKind::Date).required(),
            Filter::new("hideGroupAmounts", "Hide Group Amounts", FilterKind::Check),
        ]
    }

    fn get(&self, fieldname: &str) -> FilterValue {
        match fieldname {
            "fromDate" => FilterValue::date(self.from_date),
            "toDate" => FilterValue::date(self.to_date),
            "hideGroupAmounts" => FilterValue::Bool(self.hide_group_amounts),
            _ => FilterValue::Null,
        }
    }

    fn set(&mut self, fieldname: &str, value: FilterValue) {
        match (fieldname, value) {
            ("fromDate", FilterValue::Date(date)) => self.from_date = Some(date),
            ("toDate", FilterValue::Date(date)) => self.to_date = Some(date),
            ("fromDate", FilterValue::Null) => self.from_date = None,
            ("toDate", FilterValue::Null) => self.to_date = None,
            ("hideGroupAmounts", FilterValue::Bool(b)) => self.hide_group_amounts = b,
            _ => {}
        }
    }

    fn columns(&self, _fyo: &Fyo, _formatter: &Formatter) -> Result<Vec<Column>, Error> {
        let mut columns = vec![Column::new(
            "Account",
            "account",
            Align::Left,
            ACC_NAME_WIDTH,
        )];
        for (label, fieldname) in [
            ("Opening (Dr)", "openingDebit"),
            ("Opening (Cr)", "openingCredit"),
            ("Debit", "debit"),
            ("Credit", "credit"),
            ("Closing (Dr)", "closingDebit"),
            ("Closing (Cr)", "closingCredit"),
        ] {
            columns.push(Column::new(label, fieldname, Align::Right, ACC_BAL_WIDTH));
        }
        Ok(columns)
    }

    fn run(&mut self, fyo: &Fyo, formatter: &Formatter) -> Result<Vec<ReportRow>, Error> {
        self.run_on(fyo, formatter, Zoned::now().date())
    }
}
