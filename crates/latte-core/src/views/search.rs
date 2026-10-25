//! The global search: the `Search` class and lists of `src/utils/search.ts`, and
//! `fuzzyMatch` of `src/utils/index.ts` @ v0.36.0.
//!
//! Differences from Books:
//! - Every search starts from scratch; Books narrows the previous results as letters are
//!   typed (`_intermediate`), which gives the same items.
//! - The index is built once, by `Searcher::new`; Books rebuilds a schema's keywords when
//!   one of its documents is saved or deleted. Make a new `Searcher` to pick changes up.
//! - A recent document opens the document. Books keeps its schema's label and opens
//!   `/list/<label>`, which is no list; and a recent item picked again keeps what it opens,
//!   where Books then keeps only its label.

use std::collections::BTreeMap;

use latte_db::{DocValueMap, FieldValue, GetAllOptions, Order, RawValue};

use crate::fyo::model::helpers::js_number_string;
use crate::models::helpers::{single_is_truthy, single_text};
use crate::reports::REPORTS;
use crate::views::list::create_values;
use crate::views::sidebar::Route;
use crate::{Error, Fyo};

/// `Number.MAX_SAFE_INTEGER`: the distance of a non-match.
const NO_MATCH: f64 = 9_007_199_254_740_991.0;

/// `SearchGroup`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SearchGroup {
    Docs,
    Create,
    List,
    Report,
    Page,
    Recent,
}

impl SearchGroup {
    /// `searchGroups`: the order of the filter buttons, and of their Ctrl+1 to Ctrl+6.
    pub const ALL: [SearchGroup; 6] = [
        SearchGroup::Docs,
        SearchGroup::List,
        SearchGroup::Create,
        SearchGroup::Report,
        SearchGroup::Page,
        SearchGroup::Recent,
    ];

    /// `getGroupLabelMap`.
    pub fn label(self) -> &'static str {
        match self {
            Self::Docs => "Docs",
            Self::Create => "Create",
            Self::List => "List",
            Self::Report => "Report",
            Self::Page => "Page",
            Self::Recent => "Recent",
        }
    }

    /// `groupColorMap` of `SearchBar.vue`.
    pub fn color(self) -> &'static str {
        match self {
            Self::Docs => "blue",
            Self::Create => "green",
            Self::List => "teal",
            Self::Report => "yellow",
            Self::Page => "orange",
            Self::Recent => "purple",
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Docs => 0,
            Self::Create => 1,
            Self::List => 2,
            Self::Report => 3,
            Self::Page => 4,
            Self::Recent => 5,
        }
    }

    fn from_label(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|g| g.label() == label)
    }
}

/// What picking an item does: its `route`, or its `action`.
#[derive(Debug, Clone, PartialEq)]
pub enum SearchTarget {
    Route(Route),
    /// A new document of `schema` starting with these values (Books' `getCreateAction` with `createFilters`).
    NewDoc {
        schema: String,
        values: DocValueMap,
    },
}

/// `SearchItem` and `DocSearchItem`.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchItem {
    pub label: String,
    pub group: SearchGroup,
    /// For Docs: the schema's label, shown at the right instead of the group label.
    pub schema_label: Option<String>,
    /// For Docs: the other keyword values, shown after the label. Empty values are kept,
    /// as Books keeps them; `SearchBar.vue` leaves them out when it joins them.
    pub more: Vec<String>,
    pub target: SearchTarget,
}

/// `StoredRecentItem`: a picked item, kept by its label and group, and for a document its
/// schema and name.
#[derive(Debug, Clone, PartialEq)]
struct RecentItem {
    label: String,
    group: SearchGroup,
    doc: Option<(String, String)>,
    timestamp: i64,
}

/// `maxRecentItems`.
const MAX_RECENT_ITEMS: usize = 10;

/// `Searchable`.
struct Searchable {
    schema_name: String,
    is_child: bool,
    is_submittable: bool,
}

/// `Keyword`.
struct Keyword {
    values: Vec<String>,
    schema_name: String,
    parent: Option<String>,
    parent_schema_name: Option<String>,
    priority: i64,
}

/// A schema filter of More Filters: the schema, its label, and whether it is searched.
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaFilter {
    pub schema_name: String,
    pub label: String,
    pub on: bool,
}

/// `SearchFilters`: what [`Searcher::filters`] keeps.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchFilters {
    group_filters: [bool; 6],
    schema_filters: Vec<(String, bool)>,
    skip_tables: bool,
    skip_transactions: bool,
}

/// `Search`.
pub struct Searcher {
    searchables: Vec<Searchable>,
    /// `keywords`, in the order the schemas were read.
    keywords: Vec<(String, Vec<Keyword>)>,
    schema_labels: Vec<(String, String)>,
    non_doc_items: Vec<SearchItem>,
    /// `filters.groupFilters`, by `SearchGroup::index`.
    group_filters: [bool; 6],
    /// `filters.schemaFilters`, by searchable.
    schema_filters: Vec<bool>,
    /// `filters.skipTables`.
    skip_tables: bool,
    /// `filters.skipTransactions`.
    skip_transactions: bool,
    /// What was picked, newest first (`searchRecents`).
    recents: Vec<RecentItem>,
}

impl Searcher {
    /// Builds the keyword index from the open file (`initializeKeywords`, `getNonDocSearchList`).
    pub fn new(fyo: &crate::Fyo) -> Result<Self, crate::Error> {
        let mut searcher = Self {
            searchables: Vec::new(),
            keywords: Vec::new(),
            schema_labels: Vec::new(),
            non_doc_items: non_doc_search_list(fyo),
            group_filters: [true; 6],
            schema_filters: Vec::new(),
            skip_tables: false,
            skip_transactions: false,
            recents: Vec::new(),
        };
        searcher.set_searchables(fyo);
        searcher.update_keywords(fyo)?;
        searcher.schema_filters = vec![true; searcher.searchables.len()];
        searcher.set_filter_defaults();
        Ok(searcher)
    }

    /// `search(input)`: the recent items while the input is empty or starts with "#" or
    /// "recent", then Docs by keyword priority, then the non-doc items, each sorted by
    /// fuzzy distance; with an empty input no Docs show (as Books).
    pub fn search(&self, input: &str) -> Vec<SearchItem> {
        let grouped = self.grouped_keywords();
        let mut keys: Vec<i64> = grouped.keys().copied().collect();
        if !keys.contains(&0) {
            keys.push(0);
        }
        keys.sort_by(|a, b| b.cmp(a));

        let mut items = Vec::new();
        let show_recent = input.is_empty()
            || input.starts_with('#')
            || input.to_lowercase().starts_with("recent");
        if show_recent && self.group_on(SearchGroup::Recent) {
            items.extend(self.recent_items(&recent_search_term(input)));
        }
        for key in keys {
            if let Some(keywords) = grouped.get(&key) {
                self.push_doc_search_items(keywords, &mut items, input);
            }
            if key == 0 {
                self.push_non_doc_search_items(&mut items, input);
            }
        }
        items
    }

    /// `set` for a group filter.
    pub fn set_group(&mut self, group: SearchGroup, on: bool) {
        self.group_filters[group.index()] = on;
    }

    pub fn group_on(&self, group: SearchGroup) -> bool {
        self.group_filters[group.index()]
    }

    /// Whether child tables are skipped (`filters.skipTables`).
    pub fn skip_tables(&self) -> bool {
        self.skip_tables
    }

    /// Whether submittable schemas are skipped (`filters.skipTransactions`).
    pub fn skip_transactions(&self) -> bool {
        self.skip_transactions
    }

    /// `set('skipTables', …)`: every child table's schema filter follows it.
    pub fn set_skip_tables(&mut self, skip: bool) {
        for (i, searchable) in self.searchables.iter().enumerate() {
            if searchable.is_child {
                self.schema_filters[i] = !skip;
            }
        }
        self.skip_tables = skip;
    }

    /// `set('skipTransactions', …)`: every submittable schema's filter follows it.
    pub fn set_skip_transactions(&mut self, skip: bool) {
        for (i, searchable) in self.searchables.iter().enumerate() {
            if searchable.is_submittable {
                self.schema_filters[i] = !skip;
            }
        }
        self.skip_transactions = skip;
    }

    /// `set(schemaName, …)`: a schema filter; Skip Child Tables and Skip Transactions are
    /// on when every schema they cover is off (the `skipT*` getters).
    pub fn set_schema(&mut self, schema_name: &str, on: bool) {
        let Some(i) = self
            .searchables
            .iter()
            .position(|s| s.schema_name == schema_name)
        else {
            return;
        };
        self.schema_filters[i] = on;
        let all_off = |pick: fn(&Searchable) -> bool| {
            self.searchables
                .iter()
                .zip(&self.schema_filters)
                .filter(|(s, _)| pick(s))
                .all(|(_, on)| !on)
        };
        self.skip_tables = all_off(|s| s.is_child);
        self.skip_transactions = all_off(|s| s.is_submittable);
    }

    /// `schemaFilters` of `SearchBar.vue`: submittable schemas first, then the others,
    /// then child tables.
    pub fn schema_filters(&self) -> Vec<SchemaFilter> {
        let mut filters: Vec<(usize, SchemaFilter)> = self
            .searchables
            .iter()
            .zip(&self.schema_filters)
            .map(|(s, on)| {
                let index = if s.is_submittable {
                    0
                } else if s.is_child {
                    2
                } else {
                    1
                };
                let label = self.schema_label(&s.schema_name);
                let filter = SchemaFilter {
                    schema_name: s.schema_name.clone(),
                    label,
                    on: *on,
                };
                (index, filter)
            })
            .collect();
        filters.sort_by_key(|(index, _)| *index);
        filters.into_iter().map(|(_, f)| f).collect()
    }

    /// The filters, to be kept while the search is closed: Books keeps one `Search` while
    /// a company is open.
    pub fn filters(&self) -> SearchFilters {
        SearchFilters {
            group_filters: self.group_filters,
            schema_filters: self
                .searchables
                .iter()
                .zip(&self.schema_filters)
                .map(|(s, on)| (s.schema_name.clone(), *on))
                .collect(),
            skip_tables: self.skip_tables,
            skip_transactions: self.skip_transactions,
        }
    }

    /// Sets the filters kept by [`Searcher::filters`].
    pub fn set_filters(&mut self, filters: &SearchFilters) {
        self.group_filters = filters.group_filters;
        for (i, searchable) in self.searchables.iter().enumerate() {
            if let Some((_, on)) = filters
                .schema_filters
                .iter()
                .find(|(name, _)| *name == searchable.schema_name)
            {
                self.schema_filters[i] = *on;
            }
        }
        self.skip_tables = filters.skip_tables;
        self.skip_transactions = filters.skip_transactions;
    }

    fn schema_label(&self, schema_name: &str) -> String {
        self.schema_labels
            .iter()
            .find(|(name, _)| name == schema_name)
            .map_or_else(|| schema_name.to_owned(), |(_, label)| label.clone())
    }

    /// `addToRecent`: puts a picked item first in the recent items, once by its label,
    /// keeping ten.
    pub fn add_to_recent(&mut self, item: &SearchItem, timestamp: i64) {
        let recent = if item.group == SearchGroup::Recent {
            match self.recents.iter().find(|r| r.label == item.label) {
                Some(recent) => RecentItem {
                    timestamp,
                    ..recent.clone()
                },
                None => return,
            }
        } else {
            let doc = match (&item.group, &item.target) {
                (SearchGroup::Docs, SearchTarget::Route(Route::Form { schema, name })) => {
                    name.clone().map(|name| (schema.clone(), name))
                }
                _ => None,
            };
            RecentItem {
                label: item.label.clone(),
                group: item.group,
                doc,
                timestamp,
            }
        };
        self.recents.retain(|r| r.label != recent.label);
        self.recents.insert(0, recent);
        self.recents.truncate(MAX_RECENT_ITEMS);
    }

    /// `getRecentItems`: the recent items whose label or group has the term, as Recent.
    fn recent_items(&self, term: &str) -> Vec<SearchItem> {
        let term = term.to_lowercase();
        self.recents
            .iter()
            .filter(|r| {
                term.is_empty()
                    || r.label.to_lowercase().contains(&term)
                    || r.group.label().to_lowercase().contains(&term)
            })
            .filter_map(|r| {
                let target = match &r.doc {
                    Some((schema, name)) => SearchTarget::Route(Route::Form {
                        schema: schema.clone(),
                        name: Some(name.clone()),
                    }),
                    None => self
                        .non_doc_items
                        .iter()
                        .find(|i| i.group == r.group && i.label == r.label)?
                        .target
                        .clone(),
                };
                Some(SearchItem {
                    label: r.label.clone(),
                    group: SearchGroup::Recent,
                    schema_label: None,
                    more: Vec::new(),
                    target,
                })
            })
            .collect()
    }

    /// The recent items as Books stores them (`searchRecents`), as JSON.
    pub fn recents_json(&self) -> String {
        let items: Vec<serde_json::Value> = self
            .recents
            .iter()
            .map(|r| {
                let mut item = serde_json::json!({
                    "label": r.label,
                    "group": r.group.label(),
                    "timestamp": r.timestamp,
                });
                if let Some((schema, name)) = &r.doc {
                    item["schemaName"] = schema.clone().into();
                    item["name"] = name.clone().into();
                }
                item
            })
            .collect();
        serde_json::Value::Array(items).to_string()
    }

    /// Reads the recent items back from [`Searcher::recents_json`]'s JSON; what can't be
    /// read is left out.
    pub fn set_recents_json(&mut self, json: &str) {
        let Ok(serde_json::Value::Array(items)) = serde_json::from_str(json) else {
            return;
        };
        self.recents = items
            .iter()
            .filter_map(|item| {
                let text = |key: &str| item.get(key)?.as_str().map(str::to_owned);
                let doc = text("schemaName").zip(text("name"));
                Some(RecentItem {
                    label: text("label")?,
                    group: SearchGroup::from_label(&text("group")?)?,
                    doc,
                    timestamp: item.get("timestamp")?.as_i64()?,
                })
            })
            .take(MAX_RECENT_ITEMS)
            .collect();
    }

    /// `_setSearchables`: the schemas with keyword fields.
    fn set_searchables(&mut self, fyo: &Fyo) {
        for schema_name in fyo.db.schema_map().keys() {
            let Some(schema) = fyo.schema(schema_name) else {
                continue;
            };
            if schema.keyword_fields.as_ref().is_none_or(Vec::is_empty) {
                continue;
            }
            self.searchables.push(Searchable {
                schema_name: schema_name.clone(),
                is_child: schema.is_child,
                is_submittable: schema.is_submittable,
            });
            self.schema_labels
                .push((schema_name.clone(), schema.label.clone()));
        }
    }

    /// `updateKeywords` with `_setKeywords`, `_setKeywordValues`, `_setMeta` and
    /// `_setPriority`.
    fn update_keywords(&mut self, fyo: &Fyo) -> Result<(), Error> {
        for searchable in &self.searchables {
            let Some(schema) = fyo.schema(&searchable.schema_name) else {
                continue;
            };
            let fields = schema.keyword_fields.clone().unwrap_or_default();
            let mut meta: Vec<&str> = Vec::new();
            if schema.is_child {
                meta.extend(["parent", "parentSchemaName"]);
            }
            if schema.is_submittable {
                meta.extend(["submitted", "cancelled"]);
            }

            let options = GetAllOptions {
                fields: Some(
                    fields
                        .iter()
                        .cloned()
                        .chain(meta.iter().map(|m| (*m).to_owned()))
                        .collect(),
                ),
                order: Some(Order::Desc),
                order_by: (!schema.is_child).then(|| vec!["modified".to_owned()]),
                ..Default::default()
            };
            let rows = fyo.db.core().get_all(&searchable.schema_name, &options)?;
            if rows.is_empty() {
                continue;
            }

            let base_priority = priority(&searchable.schema_name);
            let mut keywords = Vec::with_capacity(rows.len());
            for row in &rows {
                let raw = |fieldname: &str| match row.get(fieldname) {
                    Some(FieldValue::Value(value)) => value.clone(),
                    _ => RawValue::Null,
                };
                let text = |fieldname: &str| match raw(fieldname) {
                    RawValue::Text(text) => Some(text),
                    _ => None,
                };
                // A number is a check: `Boolean(meta)`.
                let check = |fieldname: &str| match raw(fieldname) {
                    RawValue::Integer(i) => i != 0,
                    RawValue::Real(r) => r != 0.0,
                    _ => false,
                };

                let mut values: Vec<String> = fields
                    .iter()
                    .map(|fieldname| {
                        let value = match raw(fieldname) {
                            RawValue::Null => return String::new(),
                            RawValue::Text(text) => text,
                            RawValue::Integer(i) => i.to_string(),
                            RawValue::Real(r) => js_number_string(r),
                        };
                        // Option labels in place of values.
                        schema
                            .field(fieldname)
                            .and_then(|field| field.options.as_ref())
                            .and_then(|options| options.iter().find(|o| o.value == value))
                            .map_or(value, |option| option.label.clone())
                    })
                    .collect();

                let parent = text("parent").filter(|p| !p.is_empty());
                if let Some(parent) = &parent {
                    values.insert(0, parent.clone());
                }

                let mut priority = base_priority;
                if check("submitted") {
                    priority += 25;
                }
                if check("cancelled") {
                    priority -= 200;
                }
                if searchable.is_child {
                    priority -= 150;
                }

                keywords.push(Keyword {
                    values,
                    schema_name: searchable.schema_name.clone(),
                    parent,
                    parent_schema_name: text("parentSchemaName").filter(|p| !p.is_empty()),
                    priority,
                });
            }
            self.keywords
                .push((searchable.schema_name.clone(), keywords));
        }
        Ok(())
    }

    /// `_setFilterDefaults`: child tables are skipped when they have over 2000 rows.
    fn set_filter_defaults(&mut self) {
        let total_child_keywords: usize = self
            .keywords
            .iter()
            .filter(|(name, _)| self.is_child(name))
            .map(|(_, keywords)| keywords.len())
            .sum();
        if total_child_keywords > 2_000 {
            self.set_skip_tables(true);
        }
    }

    fn is_child(&self, schema_name: &str) -> bool {
        self.searchables
            .iter()
            .any(|s| s.schema_name == schema_name && s.is_child)
    }

    /// `_getGroupedKeywords`: the keywords of the schemas searched, by priority.
    fn grouped_keywords(&self) -> BTreeMap<i64, Vec<&Keyword>> {
        let mut grouped: BTreeMap<i64, Vec<&Keyword>> = BTreeMap::new();
        if !self.group_on(SearchGroup::Docs) {
            return grouped;
        }
        for (schema_name, keywords) in &self.keywords {
            let Some(i) = self
                .searchables
                .iter()
                .position(|s| s.schema_name == *schema_name)
            else {
                continue;
            };
            let searchable = &self.searchables[i];
            if !self.schema_filters[i]
                || (searchable.is_child && self.skip_tables)
                || (searchable.is_submittable && self.skip_transactions)
            {
                continue;
            }
            for keyword in keywords {
                grouped.entry(keyword.priority).or_default().push(keyword);
            }
        }
        grouped
    }

    /// `_pushDocSearchItems`.
    fn push_doc_search_items(
        &self,
        keywords: &[&Keyword],
        items: &mut Vec<SearchItem>,
        input: &str,
    ) {
        if input.is_empty() || !self.group_on(SearchGroup::Docs) {
            return;
        }
        let matches = keywords.iter().filter_map(|keyword| {
            // `_getValueListFromKeyword`: the values and the schema's name.
            let values = keyword
                .values
                .iter()
                .map(String::as_str)
                .chain([keyword.schema_name.as_str()]);
            match_and_distance(input, values).map(|d| (self.doc_search_item(keyword), d))
        });
        items.extend(sorted(matches));
    }

    /// `_pushNonDocSearchItems`.
    fn push_non_doc_search_items(&self, items: &mut Vec<SearchItem>, input: &str) {
        let matches = self
            .non_doc_items
            .iter()
            .filter(|item| self.group_on(item.group))
            .filter_map(|item| {
                if input.is_empty() {
                    return Some((item.clone(), 0.0));
                }
                // `_getValueListFromSearchItem`.
                let values = [item.label.as_str(), item.group.label()];
                match_and_distance(input, values.into_iter()).map(|d| (item.clone(), d))
            });
        items.extend(sorted(matches));
    }

    /// `_getDocSearchItemFromKeyword` with `_getRouteFromKeyword`: a child row opens its
    /// parent.
    fn doc_search_item(&self, keyword: &Keyword) -> SearchItem {
        let schema_label = self
            .schema_labels
            .iter()
            .find(|(name, _)| *name == keyword.schema_name)
            .map_or_else(|| keyword.schema_name.clone(), |(_, label)| label.clone());
        let label = keyword.values.first().cloned().unwrap_or_default();
        let (schema, name) = match (&keyword.parent, &keyword.parent_schema_name) {
            (Some(parent), Some(parent_schema)) => (parent_schema.clone(), parent.clone()),
            _ => (keyword.schema_name.clone(), label.clone()),
        };
        SearchItem {
            label,
            group: SearchGroup::Docs,
            schema_label: Some(schema_label),
            more: keyword.values.iter().skip(1).cloned().collect(),
            target: SearchTarget::Route(Route::Form {
                schema,
                name: Some(name),
            }),
        }
    }
}

/// `_getSubSortedArray`'s sort: by distance, keeping the order of equal ones.
fn sorted(matches: impl Iterator<Item = (SearchItem, f64)>) -> Vec<SearchItem> {
    let mut matches: Vec<(SearchItem, f64)> = matches.collect();
    matches.sort_by(|a, b| a.1.total_cmp(&b.1));
    matches.into_iter().map(|(item, _)| item).collect()
}

/// `input.replace(/^#|recent/gi, '').trim()`: what the recent items are searched for.
fn recent_search_term(input: &str) -> String {
    let input = input.strip_prefix('#').unwrap_or(input);
    let mut out = String::new();
    let mut rest = input;
    while !rest.is_empty() {
        let lower = rest.get(..6).map(str::to_lowercase);
        if lower.as_deref() == Some("recent") {
            rest = &rest[6..];
        } else {
            let c = rest.chars().next().unwrap_or_default();
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    out.trim().to_owned()
}

/// `priorityMap`.
fn priority(schema_name: &str) -> i64 {
    match schema_name {
        "SalesInvoice" => 125,
        "PurchaseInvoice" => 100,
        "Payment" | "StockMovement" | "Shipment" | "PurchaseReceipt" => 75,
        "Item" | "Party" | "JournalEntry" => 50,
        _ => 0,
    }
}

/// `_getMatchAndDistance`: every space-separated part of the input has to match one of the
/// values; the distance is the smallest of the parts'. Empty values are skipped.
fn match_and_distance<'a>(input: &str, values: impl Iterator<Item = &'a str>) -> Option<f64> {
    let values: Vec<&str> = values.filter(|v| !v.is_empty()).collect();
    let mut distance = NO_MATCH;
    for part in input.split(' ').filter(|p| !p.is_empty()) {
        // `_getInternalMatch`.
        let mut is_match = false;
        let mut part_distance = NO_MATCH;
        for value in &values {
            let (m, d) = fuzzy_match(part, value);
            is_match |= m;
            part_distance = part_distance.min(d);
        }
        if !is_match {
            return None;
        }
        distance = distance.min(part_distance);
    }
    Some(distance)
}

/// `fuzzyMatch`: (is_match, distance), distance as f64 since Books adds 0.5.
pub fn fuzzy_match(input: &str, target: &str) -> (bool, f64) {
    let mut keyword_letters = input.chars();
    let mut candidate_letters = target.chars();
    let mut keyword_letter = keyword_letters.next();
    let mut candidate_letter = candidate_letters.next();
    let mut distance = 0.0;

    while let (Some(k), Some(c)) = (keyword_letter, candidate_letter) {
        if k == c {
            keyword_letter = keyword_letters.next();
        } else if k.to_lowercase().eq(c.to_lowercase()) {
            keyword_letter = keyword_letters.next();
            distance += 0.5;
        } else {
            distance += 1.0;
        }
        candidate_letter = candidate_letters.next();
    }

    if keyword_letter.is_some() {
        return (false, NO_MATCH);
    }
    // As Books, the letter last taken from the target isn't counted.
    (true, distance + candidate_letters.count() as f64)
}

/// `getNonDocSearchList`.
fn non_doc_search_list(fyo: &Fyo) -> Vec<SearchItem> {
    [
        list_view_list(fyo),
        create_list(fyo),
        report_list(fyo),
        setup_list(),
    ]
    .concat()
}

fn item(label: &str, group: SearchGroup, target: SearchTarget) -> SearchItem {
    SearchItem {
        label: label.to_owned(),
        group,
        schema_label: None,
        more: Vec::new(),
        target,
    }
}

/// `getCreateList`.
fn create_list(fyo: &Fyo) -> Vec<SearchItem> {
    let create = |label: &str, schema: &str, values: DocValueMap| {
        let target = SearchTarget::NewDoc {
            schema: schema.to_owned(),
            values,
        };
        item(label, SearchGroup::Create, target)
    };

    let mut schemas = vec!["SalesInvoice", "PurchaseInvoice", "JournalEntry"];
    if single_is_truthy(fyo, "AccountingSettings", "enableInventory") {
        schemas.extend(["Shipment", "PurchaseReceipt", "StockMovement"]);
    }
    let mut items: Vec<SearchItem> = schemas
        .into_iter()
        .map(|schema| {
            let label = fyo.schema(schema).map_or(schema, |s| s.label.as_str());
            create(label, schema, DocValueMap::new())
        })
        .collect();

    // With `createFilters`.
    for (label, schema, filters) in [
        ("Sales Payment", "Payment", "SalesPayments"),
        ("Purchase Payment", "Payment", "PurchasePayments"),
        ("Customer", "Party", "Customers"),
        ("Supplier", "Party", "Suppliers"),
        ("Party", "Party", "Party"),
        ("Sales Item", "Item", "SalesItems"),
        ("Purchase Item", "Item", "PurchaseItems"),
        ("Item", "Item", "Items"),
    ] {
        items.push(create(label, schema, create_values(filters)));
    }
    items
}

/// `getReportList`: inventory reports only with inventory on, GST ones only with a GSTIN.
fn report_list(fyo: &Fyo) -> Vec<SearchItem> {
    let has_gstin = single_text(fyo, "AccountingSettings", "gstin").is_some();
    let has_inventory = single_is_truthy(fyo, "AccountingSettings", "enableInventory");
    REPORTS
        .iter()
        .filter(|(name, title)| {
            let is_inventory = matches!(*name, "stock-ledger" | "stock-balance");
            (has_inventory || !is_inventory) && (has_gstin || !title.starts_with("GST"))
        })
        .map(|(name, title)| {
            let target = SearchTarget::Route(Route::Report((*name).to_owned()));
            item(title, SearchGroup::Report, target)
        })
        .collect()
}

/// `getListViewList`: a list of every schema that isn't a child or single (Books builds a
/// shorter list, then overwrites it with every schema), and the filtered lists.
fn list_view_list(fyo: &Fyo) -> Vec<SearchItem> {
    let list = |label: &str, schema: &str, filters: Option<&str>| {
        let target = SearchTarget::Route(Route::list(schema, label, filters));
        item(label, SearchGroup::List, target)
    };

    let mut items: Vec<SearchItem> = fyo
        .db
        .schema_map()
        .keys()
        .filter_map(|name| fyo.schema(name))
        .filter(|s| !s.is_child && !s.is_single)
        .map(|s| list(&s.label, &s.name, None))
        .collect();

    for (label, schema, filters) in [
        ("Customers", "Party", "Customers"),
        ("Suppliers", "Party", "Suppliers"),
        ("Party", "Party", "Party"),
        ("Sales Items", "Item", "SalesItems"),
        ("Sales Payments", "Payment", "SalesPayments"),
        ("Purchase Items", "Item", "PurchaseItems"),
        ("Items", "Item", "Items"),
        ("Purchase Payments", "Payment", "PurchasePayments"),
    ] {
        items.push(list(label, schema, Some(filters)));
    }
    items
}

/// `getSetupList`.
fn setup_list() -> Vec<SearchItem> {
    let page =
        |label: &str, route: Route| item(label, SearchGroup::Page, SearchTarget::Route(route));
    vec![
        page("Dashboard", Route::Dashboard),
        page("Chart of Accounts", Route::ChartOfAccounts),
        page("Import Wizard", Route::ImportWizard),
        page("Settings", Route::Settings { tab: None }),
    ]
}

#[cfg(test)]
mod tests {
    use latte_db::DocValue;

    use super::*;
    use crate::setup_tests::{TempFile, indian_company};
    use crate::tests::{map, text};

    #[test]
    fn fuzzy_match_follows_books() {
        assert_eq!(fuzzy_match("abc", "abc"), (true, 0.0));
        // A case difference costs half a letter.
        assert_eq!(fuzzy_match("ABC", "abc"), (true, 1.5));
        // A skipped letter costs one.
        assert_eq!(fuzzy_match("ac", "abc"), (true, 1.0));
        // The letters after the match count, less the last one read.
        assert_eq!(fuzzy_match("a", "abc"), (true, 1.0));
        assert_eq!(fuzzy_match("", "abc"), (true, 2.0));
        assert_eq!(fuzzy_match("abd", "abc"), (false, NO_MATCH));
        assert_eq!(fuzzy_match("ba", "ab"), (false, NO_MATCH));
    }

    #[test]
    fn an_empty_input_shows_only_the_non_doc_items() {
        let file = TempFile::new("search empty");
        let fyo = indian_company(&file);
        let searcher = Searcher::new(&fyo).unwrap();
        let items = searcher.search("");
        assert!(!items.is_empty());
        assert!(items.iter().all(|i| i.group != SearchGroup::Docs));
        assert_eq!(items[0].group, SearchGroup::List);
        assert_eq!(items.last().unwrap().label, "Settings");
        // No inventory and no GSTIN: neither stock nor GST reports.
        let reports: Vec<&str> = items
            .iter()
            .filter(|i| i.group == SearchGroup::Report)
            .map(|i| i.label.as_str())
            .collect();
        assert_eq!(
            reports,
            [
                "General Ledger",
                "Profit And Loss",
                "Balance Sheet",
                "Trial Balance"
            ]
        );
        let customer = items
            .iter()
            .find(|i| i.group == SearchGroup::Create && i.label == "Customer")
            .unwrap();
        assert_eq!(
            customer.target,
            SearchTarget::NewDoc {
                schema: "Party".to_owned(),
                values: create_values("Customers"),
            }
        );
    }

    #[test]
    fn a_typed_input_matches_and_sorts_by_distance() {
        let file = TempFile::new("search typed");
        let fyo = indian_company(&file);
        let mut searcher = Searcher::new(&fyo).unwrap();
        let items = searcher.search("Import");
        assert_eq!(items[0].label, "Import Wizard");
        assert_eq!(items[0].target, SearchTarget::Route(Route::ImportWizard));

        let items = searcher.search("Customers");
        assert_eq!(items[0].label, "Customers");
        assert_eq!(
            items[0].target,
            SearchTarget::Route(Route::list("Party", "Customers", Some("Customers")))
        );

        searcher.set_group(SearchGroup::List, false);
        assert!(!searcher.group_on(SearchGroup::List));
        let items = searcher.search("Customers");
        assert!(items.iter().all(|i| i.group != SearchGroup::List));
        assert!(searcher.search("zzzz").is_empty());
    }

    #[test]
    fn docs_come_first_by_priority_and_child_rows_open_their_parent() {
        let file = TempFile::new("search docs");
        let mut fyo = indian_company(&file);
        let values = map([("name", text("Bharat Traders")), ("role", text("Customer"))]);
        fyo.get_new_doc("Party", values)
            .unwrap()
            .sync(&mut fyo)
            .unwrap();
        let values = map([("name", text("Widget")), ("rate", DocValue::Number(100.0))]);
        fyo.get_new_doc("Item", values)
            .unwrap()
            .sync(&mut fyo)
            .unwrap();
        let values = map([
            ("party", text("Bharat Traders")),
            ("account", text("Debtors")),
        ]);
        let mut invoice = fyo.get_new_doc("SalesInvoice", values).unwrap();
        let row = map([
            ("item", text("Widget")),
            ("quantity", DocValue::Number(2.0)),
        ]);
        invoice.append(&mut fyo, "items", row).unwrap();
        invoice.sync(&mut fyo).unwrap();
        let invoice_name = invoice.name().unwrap().to_owned();

        let searcher = Searcher::new(&fyo).unwrap();
        // With an empty input no docs show.
        assert!(
            searcher
                .search("")
                .iter()
                .all(|i| i.group != SearchGroup::Docs)
        );

        let items = searcher.search("Bharat");
        let invoice_item = &items[0];
        assert_eq!(invoice_item.group, SearchGroup::Docs);
        assert_eq!(invoice_item.label, invoice_name);
        assert_eq!(invoice_item.schema_label.as_deref(), Some("Sales Invoice"));
        assert_eq!(invoice_item.more, ["Bharat Traders"]);
        assert_eq!(
            invoice_item.target,
            SearchTarget::Route(Route::Form {
                schema: "SalesInvoice".to_owned(),
                name: Some(invoice_name.clone()),
            })
        );
        let party = &items[1];
        assert_eq!(party.label, "Bharat Traders");
        assert_eq!(party.schema_label.as_deref(), Some("Party"));
        // The role's option label.
        assert_eq!(party.more, ["", "Customer"]);

        // An invoice row (priority -150) comes after the non-doc items and opens the
        // invoice.
        let items = searcher.search("Widget");
        let row = items
            .iter()
            .position(|i| i.schema_label.as_deref() == Some("Sales Invoice Item"))
            .unwrap();
        assert_eq!(items[row].label, invoice_name);
        assert_eq!(
            items[row].target,
            SearchTarget::Route(Route::Form {
                schema: "SalesInvoice".to_owned(),
                name: Some(invoice_name),
            })
        );
        let item = items
            .iter()
            .position(|i| i.schema_label.as_deref() == Some("Item"))
            .unwrap();
        assert!(item < row);
    }

    #[test]
    fn recent_items_come_first_while_the_input_asks_for_them() {
        let file = TempFile::new("search recent");
        let fyo = indian_company(&file);
        let mut searcher = Searcher::new(&fyo).unwrap();
        assert!(
            searcher
                .search("")
                .iter()
                .all(|i| i.group != SearchGroup::Recent)
        );

        let import = searcher.search("Import")[0].clone();
        searcher.add_to_recent(&import, 1);
        let customers = searcher.search("Customers")[0].clone();
        searcher.add_to_recent(&customers, 2);
        let doc = SearchItem {
            label: "SINV-1001".into(),
            group: SearchGroup::Docs,
            schema_label: Some("Sales Invoice".into()),
            more: Vec::new(),
            target: SearchTarget::Route(Route::Form {
                schema: "SalesInvoice".into(),
                name: Some("SINV-1001".into()),
            }),
        };
        searcher.add_to_recent(&doc, 3);
        // Picked again, an item moves to the top once.
        searcher.add_to_recent(&import, 4);

        let items = searcher.search("");
        let recent: Vec<&str> = items
            .iter()
            .take_while(|i| i.group == SearchGroup::Recent)
            .map(|i| i.label.as_str())
            .collect();
        assert_eq!(recent, ["Import Wizard", "SINV-1001", "Customers"]);
        assert_eq!(items[0].target, import.target);
        // A recent document opens the document.
        assert_eq!(items[1].target, doc.target);

        // "#" and "recent" search the recent items by label or group.
        let labels = |input: &str| -> Vec<String> {
            searcher
                .search(input)
                .into_iter()
                .filter(|i| i.group == SearchGroup::Recent)
                .map(|i| i.label)
                .collect()
        };
        assert_eq!(labels("#sinv"), ["SINV-1001"]);
        assert_eq!(labels("Recent list"), ["Customers"]);
        assert!(labels("Import").is_empty());

        // Kept as JSON, as Books keeps them in localStorage.
        let json = searcher.recents_json();
        let mut other = Searcher::new(&fyo).unwrap();
        other.set_recents_json(&json);
        assert_eq!(other.search("")[..3], items[..3]);

        searcher.set_group(SearchGroup::Recent, false);
        assert!(
            searcher
                .search("")
                .iter()
                .all(|i| i.group != SearchGroup::Recent)
        );
    }

    #[test]
    fn more_filters_skip_tables_transactions_and_schemas() {
        let file = TempFile::new("search filters");
        let mut fyo = indian_company(&file);
        let values = map([("name", text("Widget")), ("rate", DocValue::Number(100.0))]);
        fyo.get_new_doc("Item", values)
            .unwrap()
            .sync(&mut fyo)
            .unwrap();
        let values = map([("name", text("Acme")), ("role", text("Customer"))]);
        fyo.get_new_doc("Party", values)
            .unwrap()
            .sync(&mut fyo)
            .unwrap();
        let values = map([("party", text("Acme")), ("account", text("Debtors"))]);
        let mut invoice = fyo.get_new_doc("SalesInvoice", values).unwrap();
        let row = map([
            ("item", text("Widget")),
            ("quantity", DocValue::Number(1.0)),
        ]);
        invoice.append(&mut fyo, "items", row).unwrap();
        invoice.sync(&mut fyo).unwrap();

        let mut searcher = Searcher::new(&fyo).unwrap();
        let schemas = |searcher: &Searcher, input: &str| -> Vec<String> {
            searcher
                .search(input)
                .into_iter()
                .filter_map(|i| i.schema_label)
                .collect()
        };
        assert!(schemas(&searcher, "Widget").contains(&"Sales Invoice Item".to_owned()));

        // Submittable schemas first, child tables last.
        let filters = searcher.schema_filters();
        assert!(filters.iter().all(|f| f.on));
        let first_child = filters
            .iter()
            .position(|f| f.label == "Sales Invoice Item")
            .unwrap();
        let sales_invoice = filters
            .iter()
            .position(|f| f.label == "Sales Invoice")
            .unwrap();
        let item = filters.iter().position(|f| f.label == "Item").unwrap();
        assert!(sales_invoice < item && item < first_child);

        searcher.set_skip_tables(true);
        assert!(!schemas(&searcher, "Widget").contains(&"Sales Invoice Item".to_owned()));
        assert!(searcher.schema_filters().iter().any(|f| !f.on));

        searcher.set_skip_transactions(true);
        assert!(!schemas(&searcher, "Acme").contains(&"Sales Invoice".to_owned()));
        assert!(schemas(&searcher, "Acme").contains(&"Party".to_owned()));

        // A transaction turned back on clears Skip Transactions.
        searcher.set_schema("SalesInvoice", true);
        assert!(!searcher.skip_transactions());
        assert!(searcher.skip_tables());
        searcher.set_schema("Party", false);
        assert!(!schemas(&searcher, "Acme").contains(&"Party".to_owned()));
    }

    #[test]
    fn the_recent_term_drops_hash_and_recent() {
        assert_eq!(recent_search_term("#inv"), "inv");
        assert_eq!(recent_search_term("Recent sales RECENT"), "sales");
        assert_eq!(recent_search_term(""), "");
    }
}
