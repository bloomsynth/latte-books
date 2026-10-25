//! The chart of accounts as a tree: `src/pages/ChartOfAccounts.vue` @ v0.36.0.
//!
//! A group opens on click; an account opens its form. Each account shows its balance, a
//! group can take a new account or group, and Expand/Collapse open or close every group.
//!
//! Differences from Books:
//! - An account opens in the full form, not the quick-edit panel.
//! - Balances are summed as money, not as floating-point numbers.

use std::collections::{HashMap, HashSet};

use crate::widgets::Button;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::t;
use latte_core::views::sidebar::Route;
use latte_db::pesa::Money;
use latte_db::{DocValue, DocValueMap, Filter, FilterValue, GetAllOptions, Order, QueryFilter};

use crate::session::Shared;
use crate::widgets::{error_message, page_header};
use crate::{Nav, Navigate};

#[derive(Clone)]
struct Account {
    name: String,
    is_group: bool,
    root_type: String,
    account_type: String,
}

/// A group being given a new account or group.
struct Adding {
    parent: String,
    is_group: bool,
    input: Entity<InputState>,
    _subscription: Subscription,
}

pub struct ChartOfAccounts {
    session: Shared,
    root_label: String,
    children: HashMap<Option<String>, Vec<Account>>,
    expanded: HashSet<String>,
    totals: HashMap<String, (Money, Money)>,
    adding: Option<Adding>,
    error: Option<String>,
}

impl EventEmitter<Navigate> for ChartOfAccounts {}

fn text(values: &DocValueMap, field: &str) -> String {
    match values.get(field) {
        Some(DocValue::Text(text)) => text.clone(),
        _ => String::new(),
    }
}

/// `isCredit`: Liability, Equity and Income balances are credit balances.
fn is_credit(root_type: &str) -> bool {
    matches!(root_type, "Liability" | "Equity" | "Income")
}

impl ChartOfAccounts {
    pub fn new(session: Shared, _: &mut Window, cx: &mut Context<Self>) -> Self {
        let root_label = session.borrow().company_name();
        let mut page = Self {
            session,
            root_label,
            children: HashMap::new(),
            expanded: HashSet::new(),
            totals: HashMap::new(),
            adding: None,
            error: None,
        };
        page.load_totals();
        page.fetch_children(None, cx);
        page
    }

    /// `getTotalCreditAndDebit`: each account's total credit and debit.
    fn load_totals(&mut self) {
        let s = self.session.borrow();
        let options = GetAllOptions {
            fields: Some(vec!["account".into(), "debit".into(), "credit".into()]),
            ..GetAllOptions::default()
        };
        let zero = s.fyo.pesa().zero();
        let money = |values: &DocValueMap, field: &str| match values.get(field) {
            Some(DocValue::Money(m)) => m.clone(),
            _ => zero.clone(),
        };
        self.totals.clear();
        match s.fyo.db.get_all_docs("AccountingLedgerEntry", &options) {
            Ok(entries) => {
                for values in entries {
                    let entry = self
                        .totals
                        .entry(text(&values, "account"))
                        .or_insert_with(|| (zero.clone(), zero.clone()));
                    if let (Ok(credit), Ok(debit)) = (
                        entry.0.add(&money(&values, "credit")),
                        entry.1.add(&money(&values, "debit")),
                    ) {
                        *entry = (credit, debit);
                    }
                }
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn fetch_children(&mut self, parent: Option<&str>, cx: &mut Context<Self>) -> bool {
        let s = self.session.borrow();
        let filter = match parent {
            Some(parent) => Filter::Equals(FilterValue::Text(parent.to_owned())),
            None => Filter::Equals(FilterValue::Null),
        };
        let options = GetAllOptions {
            fields: Some(
                [
                    "name",
                    "parentAccount",
                    "isGroup",
                    "rootType",
                    "accountType",
                ]
                .map(str::to_owned)
                .to_vec(),
            ),
            filters: QueryFilter::from([("parentAccount".to_owned(), filter)]),
            order_by: Some(vec!["name".to_owned()]),
            order: Some(Order::Asc),
            ..GetAllOptions::default()
        };
        let accounts: Vec<Account> = match s.fyo.db.get_all_docs("Account", &options) {
            Ok(rows) => rows
                .iter()
                .map(|values| Account {
                    name: text(values, "name"),
                    is_group: matches!(values.get("isGroup"), Some(DocValue::Bool(true)))
                        || matches!(values.get("isGroup"), Some(DocValue::Number(n)) if *n != 0.0),
                    root_type: text(values, "rootType"),
                    account_type: text(values, "accountType"),
                })
                .collect(),
            Err(e) => {
                drop(s);
                self.error = Some(e.to_string());
                cx.notify();
                return false;
            }
        };
        drop(s);
        let has = !accounts.is_empty();
        self.children.insert(parent.map(str::to_owned), accounts);
        cx.notify();
        has
    }

    fn on_click(&mut self, account: &Account, cx: &mut Context<Self>) {
        if !account.is_group {
            cx.emit(Navigate(Nav::Route(Route::Form {
                schema: "Account".to_owned(),
                name: Some(account.name.clone()),
            })));
            return;
        }
        if self.expanded.remove(&account.name) {
            cx.notify();
            return;
        }
        let key = Some(account.name.clone());
        let has = self
            .children
            .get(&key)
            .map(|c| !c.is_empty())
            .unwrap_or_else(|| self.fetch_children(Some(&account.name), cx));
        if has {
            self.expanded.insert(account.name.clone());
        } else {
            // An empty group opens its form, as Books opens it when it has no children.
            cx.emit(Navigate(Nav::Route(Route::Form {
                schema: "Account".to_owned(),
                name: Some(account.name.clone()),
            })));
        }
        cx.notify();
    }

    fn toggle_all(&mut self, expand: bool, cx: &mut Context<Self>) {
        if !expand {
            self.expanded.clear();
            cx.notify();
            return;
        }
        let mut queue: Vec<Option<String>> = vec![None];
        while let Some(parent) = queue.pop() {
            if !self.children.contains_key(&parent) {
                self.fetch_children(parent.as_deref(), cx);
            }
            for account in self.children.get(&parent).cloned().unwrap_or_default() {
                if account.is_group {
                    self.expanded.insert(account.name.clone());
                    queue.push(Some(account.name));
                }
            }
        }
        cx.notify();
    }

    fn start_adding(
        &mut self,
        parent: &Account,
        is_group: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = Some(parent.name.clone());
        if !self.children.contains_key(&key) {
            self.fetch_children(Some(&parent.name), cx);
        }
        self.expanded.insert(parent.name.clone());
        let placeholder = if is_group { "New Group" } else { "New Account" };
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        input.update(cx, |input, cx| input.focus(window, cx));
        let parent_account = parent.clone();
        let subscription = cx.subscribe_in(
            &input,
            window,
            move |this, input, event: &InputEvent, _, cx| match event {
                InputEvent::PressEnter { .. } => {
                    let name = input.read(cx).value().trim().to_owned();
                    if !name.is_empty() {
                        this.create(&parent_account, &name, is_group, cx);
                    }
                }
                InputEvent::Blur => {
                    this.adding = None;
                    cx.notify();
                }
                _ => {}
            },
        );
        self.adding = Some(Adding {
            parent: parent.name.clone(),
            is_group,
            input,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// `createNewAccount`: the new account takes its parent's root and account types.
    fn create(&mut self, parent: &Account, name: &str, is_group: bool, cx: &mut Context<Self>) {
        let result = {
            let mut s = self.session.borrow_mut();
            let fyo = &mut s.fyo;
            fyo.get_new_doc("Account", DocValueMap::new())
                .and_then(|mut doc| {
                    let values = DocValueMap::from([
                        ("name".to_owned(), DocValue::Text(name.to_owned())),
                        (
                            "parentAccount".to_owned(),
                            DocValue::Text(parent.name.clone()),
                        ),
                        (
                            "rootType".to_owned(),
                            DocValue::Text(parent.root_type.clone()),
                        ),
                        (
                            "accountType".to_owned(),
                            if parent.account_type.is_empty() {
                                DocValue::Null
                            } else {
                                DocValue::Text(parent.account_type.clone())
                            },
                        ),
                        ("isGroup".to_owned(), DocValue::Bool(is_group)),
                    ]);
                    doc.set_multiple(fyo, values)?;
                    doc.sync(fyo)
                })
        };
        match result {
            Ok(()) => {
                self.adding = None;
                self.error = None;
                self.fetch_children(Some(&parent.name), cx);
            }
            Err(e) => {
                self.error = Some(e.to_string());
                cx.notify();
            }
        }
    }

    fn balance(&self, account: &Account) -> String {
        let s = self.session.borrow();
        let zero = s.fyo.pesa().zero();
        let (credit, debit) = self
            .totals
            .get(&account.name)
            .cloned()
            .unwrap_or((zero.clone(), zero));
        let credit_side = is_credit(&account.root_type);
        let balance = if credit_side {
            credit.sub(&debit)
        } else {
            debit.sub(&credit)
        };
        let suffix = if credit_side { "Cr." } else { "Dr." };
        match balance {
            Ok(balance) => format!("{} {suffix}", s.formatter.currency(&balance)),
            Err(_) => String::new(),
        }
    }

    fn rows(&self, parent: Option<&String>, level: usize, out: &mut Vec<(Account, usize)>) {
        for account in self.children.get(&parent.cloned()).into_iter().flatten() {
            out.push((account.clone(), level));
            if account.is_group && self.expanded.contains(&account.name) {
                self.rows(Some(&account.name), level + 1, out);
            }
        }
    }
}

impl Render for ChartOfAccounts {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let header = page_header(t("Chart of Accounts"), cx).child(
            div()
                .h_flex()
                .gap_2()
                .child(
                    Button::new("expand")
                        .small()
                        .ghost()
                        .label(t("Expand"))
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_all(true, cx))),
                )
                .child(
                    Button::new("collapse")
                        .small()
                        .ghost()
                        .label(t("Collapse"))
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_all(false, cx))),
                ),
        );
        let mut rows = Vec::new();
        self.rows(None, 1, &mut rows);
        let mut children: Vec<AnyElement> = vec![
            div()
                .h_flex()
                .h(px(36.))
                .px_4()
                .font_weight(FontWeight::SEMIBOLD)
                .child(self.root_label.clone())
                .into_any_element(),
        ];
        for (i, (account, level)) in rows.into_iter().enumerate() {
            let indent = px(16. + 24. * level as f32);
            let expanded = self.expanded.contains(&account.name);
            let marker = match (account.is_group, expanded) {
                (true, true) => "▾ ",
                (true, false) => "▸ ",
                _ => "",
            };
            let click = account.clone();
            let add_account = account.clone();
            let add_group = account.clone();
            children.push(
                div()
                    .id(("account", i))
                    .group("account-row")
                    .h_flex()
                    .h(px(34.))
                    .pl(indent)
                    .pr_4()
                    .gap_2()
                    .cursor_pointer()
                    .hover(|d| d.bg(theme.list_hover))
                    .on_click(cx.listener(move |this, _, _, cx| this.on_click(&click, cx)))
                    .child(div().flex_1().child(format!("{marker}{}", account.name)))
                    .when(account.is_group, |d| {
                        d.child(
                            div()
                                .h_flex()
                                .gap_1()
                                .invisible()
                                .group_hover("account-row", |d| d.visible())
                                .child(
                                    Button::new(("add-account", i))
                                        .xsmall()
                                        .ghost()
                                        .label(t("Add Account"))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            cx.stop_propagation();
                                            this.start_adding(&add_account, false, window, cx)
                                        })),
                                )
                                .child(
                                    Button::new(("add-group", i))
                                        .xsmall()
                                        .ghost()
                                        .label(t("Add Group"))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            cx.stop_propagation();
                                            this.start_adding(&add_group, true, window, cx)
                                        })),
                                ),
                        )
                    })
                    .when(!account.is_group, |d| {
                        d.child(
                            div()
                                .text_color(theme.muted_foreground)
                                .child(self.balance(&account)),
                        )
                    })
                    .into_any_element(),
            );
            if let Some(adding) = &self.adding
                && adding.parent == account.name
            {
                let label = if adding.is_group {
                    "Add Group"
                } else {
                    "Add Account"
                };
                children.push(
                    div()
                        .h_flex()
                        .h(px(36.))
                        .pl(px(16. + 24. * (level + 1) as f32))
                        .pr_4()
                        .gap_2()
                        .child(div().w(px(280.)).child(Input::new(&adding.input).small()))
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(format!("{label}: press Enter to create")),
                        )
                        .into_any_element(),
                );
            }
        }
        div()
            .v_flex()
            .size_full()
            .child(header)
            .children(self.error.clone().map(|e| error_message(e, cx)))
            .child(
                div()
                    .id("coa")
                    .flex_1()
                    .overflow_y_scroll()
                    .children(children),
            )
    }
}
