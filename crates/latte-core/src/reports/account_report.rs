// Port of reports/AccountReport.ts @ v0.36.0: what Profit And Loss, the Balance Sheet
// and the Trial Balance share. Balances by account and period, rolled up the chart of
// accounts.
//
// Books bugs Latte fixes:
// - With Based On "Fiscal Year", the periods cover the whole span from the From Year's
//   start to the To Year's end. Books counts whole years between the two years, so a
//   January–December fiscal year with both years the same shows only its last period.
// - The default fiscal year is the one today is in. Books takes the one starting this
//   calendar year, which for an April–March year is next year's until April, and two
//   years for a January–December one.

use indexmap::IndexMap;
use jiff::civil::Date;
use jiff::{ToSpan, Zoned};
use latte_db::pesa::Money;
use latte_db::{
    DocValue, Filter as DbFilter, FilterOperand, FilterValue as DbFilterValue, GetAllOptions,
    QueryFilter,
};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::format::{Formatter, local_date, utc_date};
use crate::reports::ledger_report::{LedgerEntry, grouped_map, raw_ledger_entries};
use crate::reports::{
    Align, Column, Filter, FilterKind, FilterValue, RawValue, ReportCell, ReportRow,
};

/// `ACC_NAME_WIDTH`.
pub const ACC_NAME_WIDTH: f64 = 2.0;
/// `ACC_BAL_WIDTH`.
pub const ACC_BAL_WIDTH: f64 = 1.25;

/// `DateRange`: from `from_date` up to, not including, `to_date`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateRange {
    pub from_date: Date,
    pub to_date: Date,
}

impl DateRange {
    fn contains(&self, date: Date) -> bool {
        date >= self.from_date && date < self.to_date
    }
}

/// What a period adds up to for an account: its balance (for P&L and the Balance
/// Sheet), or its debits and credits (for the Trial Balance).
#[derive(Debug, Clone, PartialEq)]
pub struct RangeValue {
    pub balance: Money,
    pub debit: Money,
    pub credit: Money,
}

impl RangeValue {
    fn zero(fyo: &Fyo) -> Self {
        Self {
            balance: fyo.pesa().zero(),
            debit: fyo.pesa().zero(),
            credit: fyo.pesa().zero(),
        }
    }

    fn add(&self, other: &Self) -> Result<Self, Error> {
        Ok(Self {
            balance: self.balance.add(&other.balance)?,
            debit: self.debit.add(&other.debit)?,
            credit: self.credit.add(&other.credit)?,
        })
    }
}

/// `ValueMap`: a value per date range, by the range's position in the report's ranges.
pub type ValueMap = Vec<Option<RangeValue>>;

/// `Account`.
#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    pub name: String,
    pub root_type: String,
    pub is_group: bool,
    pub parent_account: Option<String>,
}

/// `AccountTreeNode`.
#[derive(Debug, Clone)]
pub struct AccountTreeNode {
    pub account: Account,
    pub value_map: Option<ValueMap>,
    pub children: Vec<AccountTreeNode>,
}

/// `AccountListNode`.
#[derive(Debug, Clone)]
pub struct AccountListNode {
    pub name: String,
    pub is_group: bool,
    pub value_map: Option<ValueMap>,
    pub level: usize,
}

/// `isCredit`: whether accounts of a root type are credit balanced.
pub fn is_credit(root_type: &str) -> bool {
    matches!(root_type, "Liability" | "Equity" | "Income")
}

/// `monthsMap`.
fn months_of(periodicity: &str) -> i32 {
    match periodicity {
        "Quarterly" => 3,
        "Half Yearly" => 6,
        "Yearly" => 12,
        _ => 1,
    }
}

/// `_fixMonthsJump`: if `ref_date` is the last day of its month, so is the result.
fn fix_months_jump(ref_date: Date, date: Date) -> Date {
    if ref_date.day() == ref_date.days_in_month() && date.day() != date.days_in_month() {
        return date.last_of_month();
    }
    date
}

/// The fiscal year's start and end month and day, from the accounting settings
/// (January to December if they aren't set).
fn fiscal_month_days(fyo: &Fyo) -> ((i8, i8), (i8, i8)) {
    let get = |fieldname: &str, default: (i8, i8)| match fyo.get_value(
        "AccountingSettings",
        None,
        fieldname,
    ) {
        Some(DocValue::Date(date)) => {
            let date = local_date(date);
            (date.month(), date.day())
        }
        _ => default,
    };
    (
        get("fiscalYearStart", (1, 1)),
        get("fiscalYearEnd", (12, 31)),
    )
}

fn date_in(year: i16, (month, day): (i8, i8)) -> Date {
    let first = Date::new(year, month, 1).unwrap_or(Date::constant(2000, 1, 1));
    Date::new(year, month, day.min(first.days_in_month())).unwrap_or(first)
}

/// `getFiscalEndpoints`: from the fiscal year start in `from_year` to the fiscal year
/// end in `to_year`.
pub fn get_fiscal_endpoints(fyo: &Fyo, to_year: i16, from_year: i16) -> (Date, Date) {
    let (start, end) = fiscal_month_days(fyo);
    (date_in(from_year, start), date_in(to_year, end))
}

/// The first and last day of the fiscal year `today` is in.
pub fn current_fiscal_year(fyo: &Fyo, today: Date) -> (Date, Date) {
    let (start, end) = fiscal_month_days(fyo);
    let mut from = date_in(today.year(), start);
    if from > today {
        from = date_in(today.year() - 1, start);
    }
    let end_year = if end > start {
        from.year()
    } else {
        from.year() + 1
    };
    (from, date_in(end_year, end))
}

/// The filters and date ranges of an account report.
#[derive(Debug, Clone)]
pub struct AccountReport {
    /// `Fiscal Year` or `Until Date`.
    pub based_on: String,
    /// `Monthly`, `Quarterly`, `Half Yearly` or `Yearly`.
    pub periodicity: String,
    pub to_date: Option<Date>,
    pub count: i64,
    pub from_year: Option<i16>,
    pub to_year: Option<i16>,
    pub consolidate_columns: bool,
    pub hide_group_amounts: bool,
    /// `_dateRanges`, latest first.
    pub date_ranges: Vec<DateRange>,
}

impl Default for AccountReport {
    fn default() -> Self {
        Self {
            based_on: "Until Date".to_owned(),
            periodicity: "Monthly".to_owned(),
            to_date: None,
            count: 3,
            from_year: None,
            to_year: None,
            consolidate_columns: false,
            hide_group_amounts: false,
            date_ranges: Vec::new(),
        }
    }
}

impl AccountReport {
    /// `setDefaultFilters`, then the date ranges.
    pub fn set_default_filters(&mut self, fyo: &Fyo, today: Date) {
        if self.based_on == "Until Date" && self.to_date.is_none() {
            self.to_date = today.checked_add(1.day()).ok();
        }
        if self.based_on == "Fiscal Year" && self.to_year.is_none() {
            let (from, to) = current_fiscal_year(fyo, today);
            self.from_year = Some(from.year());
            self.to_year = Some(to.year());
        }
        self.date_ranges = self.get_date_ranges(fyo);
    }

    /// `_getFromAndToDates`: the first day and the day after the last.
    pub fn from_and_to_dates(&self, fyo: &Fyo) -> (Date, Date) {
        if self.based_on == "Until Date" {
            let to_date = self.to_date.unwrap_or(Date::constant(2000, 1, 1));
            let months = months_of(&self.periodicity) * self.count.max(1) as i32;
            let from = to_date.checked_sub(months.months()).unwrap_or(to_date);
            let to = to_date.checked_add(1.day()).unwrap_or(to_date);
            return (from, to);
        }
        let this_year = Zoned::now().date().year();
        let (from, to) = get_fiscal_endpoints(
            fyo,
            self.to_year.unwrap_or(this_year),
            self.from_year.unwrap_or(this_year),
        );
        (from, to.checked_add(1.day()).unwrap_or(to))
    }

    /// `_getDateRanges`, latest first.
    fn get_date_ranges(&self, fyo: &Fyo) -> Vec<DateRange> {
        let (from_date, to_date) = self.from_and_to_dates(fyo);
        if self.consolidate_columns {
            return vec![DateRange { from_date, to_date }];
        }

        let months = months_of(&self.periodicity);
        let minus = |date: Date| {
            fix_months_jump(to_date, date.checked_sub(months.months()).unwrap_or(date))
        };
        let mut ranges = vec![DateRange {
            to_date,
            from_date: minus(to_date),
        }];

        let mut count = self.count.max(1);
        if self.based_on == "Fiscal Year" {
            let mut span = i64::from(to_date.year() - from_date.year()) * 12
                + i64::from(to_date.month() - from_date.month());
            if to_date.day() > from_date.day() {
                span += 1;
            }
            count = (span + i64::from(months) - 1) / i64::from(months);
        }
        for _ in 1..count {
            let last = *ranges.last().expect("one range");
            ranges.push(DateRange {
                to_date: last.from_date,
                from_date: minus(last.from_date),
            });
        }
        ranges
    }

    /// `_getQueryFilters`.
    pub fn query_filters(&self, fyo: &Fyo) -> QueryFilter {
        let (from_date, to_date) = self.from_and_to_dates(fyo);
        let mut filters = QueryFilter::new();
        filters.insert(
            "date".to_owned(),
            DbFilter::Two(
                "<".to_owned(),
                FilterOperand::One(DbFilterValue::Text(to_date.to_string())),
                ">=".to_owned(),
                FilterOperand::One(DbFilterValue::Text(from_date.to_string())),
            ),
        );
        filters.insert(
            "reverted".to_owned(),
            DbFilter::Equals(DbFilterValue::Bool(false)),
        );
        filters
    }

    /// `getFilters`.
    pub fn filters(&self) -> Vec<Filter> {
        let options = |options: &[&str]| {
            options
                .iter()
                .map(|o| ((*o).to_owned(), (*o).to_owned()))
                .collect()
        };
        let period_name = match self.periodicity.as_str() {
            "Quarterly" => "Quarters",
            "Half Yearly" => "Half Years",
            "Yearly" => "Years",
            _ => "Months",
        };
        let mut filters = vec![
            Filter::new(
                "basedOn",
                "Based On",
                FilterKind::Select(options(&["Fiscal Year", "Until Date"])),
            ),
            Filter::new(
                "periodicity",
                "Periodicity",
                FilterKind::Select(options(&["Monthly", "Quarterly", "Half Yearly", "Yearly"])),
            ),
        ];
        if self.based_on == "Until Date" {
            filters.push(Filter::new("toDate", "To Date", FilterKind::Date).required());
            filters.push(
                Filter::new(
                    "count",
                    &format!("Number of {period_name}"),
                    FilterKind::Int { min: 1 },
                )
                .required(),
            );
        } else {
            filters.push(
                Filter::new("fromYear", "From Year", FilterKind::Int { min: 2000 }).required(),
            );
            filters
                .push(Filter::new("toYear", "To Year", FilterKind::Int { min: 2000 }).required());
        }
        filters.push(Filter::new(
            "consolidateColumns",
            "Consolidate Columns",
            FilterKind::Check,
        ));
        filters.push(Filter::new(
            "hideGroupAmounts",
            "Hide Group Amounts",
            FilterKind::Check,
        ));
        filters
    }

    pub fn get(&self, fieldname: &str) -> FilterValue {
        match fieldname {
            "basedOn" => FilterValue::Text(self.based_on.clone()),
            "periodicity" => FilterValue::Text(self.periodicity.clone()),
            "toDate" => FilterValue::date(self.to_date),
            "count" => FilterValue::Int(self.count),
            "fromYear" => self
                .from_year
                .map_or(FilterValue::Null, |y| FilterValue::Int(y.into())),
            "toYear" => self
                .to_year
                .map_or(FilterValue::Null, |y| FilterValue::Int(y.into())),
            "consolidateColumns" => FilterValue::Bool(self.consolidate_columns),
            "hideGroupAmounts" => FilterValue::Bool(self.hide_group_amounts),
            _ => FilterValue::Null,
        }
    }

    pub fn set(&mut self, fieldname: &str, value: FilterValue) {
        let year = |value: &FilterValue| match value {
            FilterValue::Int(year) => i16::try_from(*year).ok(),
            _ => None,
        };
        match (fieldname, &value) {
            ("basedOn", FilterValue::Text(text)) => self.based_on = text.clone(),
            ("periodicity", FilterValue::Text(text)) => self.periodicity = text.clone(),
            ("toDate", FilterValue::Date(date)) => self.to_date = Some(*date),
            ("toDate", FilterValue::Null) => self.to_date = None,
            ("count", FilterValue::Int(count)) => self.count = *count,
            ("fromYear", _) => self.from_year = year(&value),
            ("toYear", _) => self.to_year = year(&value),
            ("consolidateColumns", FilterValue::Bool(b)) => self.consolidate_columns = *b,
            ("hideGroupAmounts", FilterValue::Bool(b)) => self.hide_group_amounts = *b,
            _ => {}
        }
    }

    /// `getColumns`: the account, then a column per period, latest first, labelled by its
    /// last day.
    pub fn columns(&self, formatter: &Formatter) -> Vec<Column> {
        let mut columns = vec![Column::new(
            "Account",
            "account",
            Align::Left,
            ACC_NAME_WIDTH,
        )];
        for range in &self.date_ranges {
            let last_day = range.to_date.checked_sub(1.day()).unwrap_or(range.to_date);
            columns.push(Column::new(
                formatter.date(last_day),
                "toDate",
                Align::Right,
                ACC_BAL_WIDTH,
            ));
        }
        columns
    }

    /// `_getRangeMapKey`: the position of the range `entry` falls in.
    pub fn range_index(&self, entry: &LedgerEntry) -> Option<usize> {
        let date = utc_date(entry.date?);
        self.date_ranges
            .iter()
            .position(|range| range.contains(date))
    }

    /// `_setRawData` and `_getGroupedMap(true, 'account')`.
    pub fn grouped_entries(
        &self,
        fyo: &Fyo,
        filters: QueryFilter,
    ) -> Result<IndexMap<String, Vec<LedgerEntry>>, Error> {
        let raw = raw_ledger_entries(fyo, filters, false)?;
        Ok(grouped_map(&raw, "account"))
    }

    /// `_getGroupedByDateRanges`: each account's balance per period, positive when it is
    /// on the account's usual side.
    pub fn balances_by_range(
        &self,
        fyo: &Fyo,
        accounts: &IndexMap<String, Account>,
        map: &IndexMap<String, Vec<LedgerEntry>>,
    ) -> Result<IndexMap<String, ValueMap>, Error> {
        let mut out = IndexMap::new();
        for (account, entries) in map {
            let mut value_map: ValueMap = vec![None; self.date_ranges.len()];
            for entry in entries {
                let Some(key) = self.range_index(entry) else {
                    continue;
                };
                let total = value_map[key]
                    .clone()
                    .unwrap_or_else(|| RangeValue::zero(fyo));
                let zero = fyo.pesa().zero();
                let balance = entry
                    .debit
                    .as_ref()
                    .unwrap_or(&zero)
                    .sub(entry.credit.as_ref().unwrap_or(&zero))?;
                let credit_balanced = accounts
                    .get(&entry.account)
                    .is_some_and(|a| is_credit(&a.root_type));
                let balance = if credit_balanced {
                    total.balance.sub(&balance)?
                } else {
                    total.balance.add(&balance)?
                };
                value_map[key] = Some(RangeValue { balance, ..total });
            }
            out.insert(account.clone(), value_map);
        }
        Ok(out)
    }

    /// `getRowFromAccountListNode`.
    pub fn row(&self, fyo: &Fyo, formatter: &Formatter, node: &AccountListNode) -> ReportRow {
        let mut cells = vec![name_cell(node)];
        for key in 0..self.date_ranges.len() {
            let balance = node
                .value_map
                .as_ref()
                .and_then(|values| values.get(key).cloned().flatten())
                .map(|value| value.balance);
            let raw = balance.unwrap_or_else(|| fyo.pesa().zero());
            let value = if self.hide_group_amounts && node.is_group {
                String::new()
            } else {
                formatter.currency(&raw)
            };
            cells.push(ReportCell {
                value,
                raw_value: RawValue::Money(raw),
                align: Align::Right,
                width: ACC_BAL_WIDTH,
                ..ReportCell::default()
            });
        }
        ReportRow {
            cells,
            level: Some(node.level),
            is_group: node.is_group,
            ..ReportRow::default()
        }
    }

    /// `getReportRowsFromAccountList`.
    pub fn rows(
        &self,
        fyo: &Fyo,
        formatter: &Formatter,
        list: &[AccountListNode],
    ) -> Vec<ReportRow> {
        list.iter()
            .map(|node| self.row(fyo, formatter, node))
            .collect()
    }

    /// `getEmptyRow`.
    pub fn empty_row(&self, formatter: &Formatter) -> ReportRow {
        ReportRow {
            is_empty: true,
            cells: self
                .columns(formatter)
                .iter()
                .map(|c| ReportCell {
                    width: c.width,
                    ..ReportCell::default()
                })
                .collect(),
            ..ReportRow::default()
        }
    }

    /// `getTotalNode`: the sum of the leaves under `roots`, per period. A root with no
    /// accounts under it counts as a leaf; Books leaves it out.
    pub fn total_node(
        &self,
        fyo: &Fyo,
        roots: &[AccountTreeNode],
        name: &str,
    ) -> Result<AccountListNode, Error> {
        let mut leaves = Vec::new();
        for root in roots {
            if root.children.is_empty() {
                leaves.push(root);
            }
            collect_leaves(&root.children, &mut leaves);
        }
        let mut totals: ValueMap = vec![None; self.date_ranges.len()];
        for (key, total) in totals.iter_mut().enumerate() {
            let mut balance = fyo.pesa().zero();
            for leaf in &leaves {
                if let Some(value) = leaf.value_map.as_ref().and_then(|v| v[key].as_ref()) {
                    balance = balance.add(&value.balance)?;
                }
            }
            *total = Some(RangeValue {
                balance,
                ..RangeValue::zero(fyo)
            });
        }
        Ok(AccountListNode {
            name: name.to_owned(),
            is_group: false,
            value_map: Some(totals),
            level: 0,
        })
    }
}

/// `getListOfLeafNodes` over children: the nodes without children below them.
fn collect_leaves<'a>(children: &'a [AccountTreeNode], leaves: &mut Vec<&'a AccountTreeNode>) {
    for child in children {
        if child.children.is_empty() {
            leaves.push(child);
        } else {
            collect_leaves(&child.children, leaves);
        }
    }
}

/// The name cell of an account row: bold at the top level, indented by its level.
pub fn name_cell(node: &AccountListNode) -> ReportCell {
    ReportCell {
        value: node.name.clone(),
        raw_value: RawValue::Text(node.name.clone()),
        align: Align::Left,
        width: ACC_NAME_WIDTH,
        bold: node.level == 0,
        indent: node.level,
        ..ReportCell::default()
    }
}

/// `_setAndReturnAccountMap`: every account, in the order Books reads them.
pub fn account_map(fyo: &Fyo) -> Result<IndexMap<String, Account>, Error> {
    let options = GetAllOptions {
        fields: Some(
            ["name", "rootType", "isGroup", "parentAccount"]
                .iter()
                .map(|f| (*f).to_owned())
                .collect(),
        ),
        ..GetAllOptions::default()
    };
    let mut map = IndexMap::new();
    for row in fyo.db.get_all_docs("Account", &options)? {
        let text = |fieldname: &str| match row.get(fieldname) {
            Some(DocValue::Text(text)) if !text.is_empty() => Some(text.clone()),
            _ => None,
        };
        let Some(name) = text("name") else { continue };
        map.insert(
            name.clone(),
            Account {
                name,
                root_type: text("rootType").unwrap_or_default(),
                is_group: matches!(row.get("isGroup"), Some(DocValue::Bool(true))),
                parent_account: text("parentAccount"),
            },
        );
    }
    Ok(map)
}

/// `_getAccountTree`: the root accounts with their children, leaving out accounts
/// with no entries below them. Each account's values are its own, or for a group the
/// sum of its children's.
pub fn account_tree(
    fyo: &Fyo,
    accounts: &IndexMap<String, Account>,
    values: &IndexMap<String, ValueMap>,
) -> Result<Vec<AccountTreeNode>, Error> {
    let mut value_maps: IndexMap<String, ValueMap> = IndexMap::new();
    let mut keep: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (name, value_map) in values {
        if !accounts.contains_key(name) {
            continue;
        }
        value_maps.insert(name.clone(), value_map.clone());
        keep.insert(name.clone());

        let mut parent = accounts[name].parent_account.clone();
        let mut seen = 0;
        while let Some(parent_name) = parent {
            let Some(parent_account) = accounts.get(&parent_name) else {
                break;
            };
            keep.insert(parent_name.clone());
            let parent_values = value_maps
                .entry(parent_name.clone())
                .or_insert_with(|| vec![None; value_map.len()]);
            for (key, child) in value_map.iter().enumerate() {
                let Some(child) = child else { continue };
                parent_values[key] = Some(match &parent_values[key] {
                    Some(value) => value.add(child)?,
                    None => child.clone(),
                });
            }
            parent = parent_account.parent_account.clone();
            seen += 1;
            if seen > accounts.len() {
                break;
            }
        }
    }

    fn build(
        name: &str,
        accounts: &IndexMap<String, Account>,
        children_of: &IndexMap<String, Vec<String>>,
        keep: &std::collections::HashSet<String>,
        value_maps: &IndexMap<String, ValueMap>,
        depth: usize,
    ) -> AccountTreeNode {
        let children = if depth > accounts.len() {
            Vec::new()
        } else {
            children_of
                .get(name)
                .map(|names| {
                    names
                        .iter()
                        .filter(|child| keep.contains(*child))
                        .map(|child| {
                            build(child, accounts, children_of, keep, value_maps, depth + 1)
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        AccountTreeNode {
            account: accounts[name].clone(),
            value_map: value_maps.get(name).cloned(),
            children,
        }
    }

    let _ = fyo;
    let mut children_of: IndexMap<String, Vec<String>> = IndexMap::new();
    for account in accounts.values() {
        if let Some(parent) = &account.parent_account
            && accounts.contains_key(parent)
        {
            children_of
                .entry(parent.clone())
                .or_default()
                .push(account.name.clone());
        }
    }
    Ok(accounts
        .values()
        .filter(|a| a.parent_account.is_none() && keep.contains(&a.name))
        .map(|a| build(&a.name, accounts, &children_of, &keep, &value_maps, 0))
        .collect())
}

/// `convertAccountRootNodesToAccountList`: the accounts in tree order, with their level.
pub fn account_list(roots: &[AccountTreeNode]) -> Vec<AccountListNode> {
    fn push(node: &AccountTreeNode, level: usize, list: &mut Vec<AccountListNode>) {
        list.push(AccountListNode {
            name: node.account.name.clone(),
            is_group: node.account.is_group,
            value_map: node.value_map.clone(),
            level,
        });
        for child in &node.children {
            push(child, level + 1, list);
        }
    }
    let mut list = Vec::new();
    for root in roots {
        push(root, 0, &mut list);
    }
    list
}

/// `getRootNodes`: the roots of a root type.
pub fn root_nodes(tree: &[AccountTreeNode], root_type: &str) -> Vec<AccountTreeNode> {
    tree.iter()
        .filter(|node| node.account.root_type == root_type)
        .cloned()
        .collect()
}
