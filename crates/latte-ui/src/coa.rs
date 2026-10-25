//! The chart of accounts as a tree: `src/pages/ChartOfAccounts.vue` @ v0.36.0.
//!
//! A group opens on click; an account opens its form. Each account shows its balance, a
//! group can take a new account or group, and Expand/Collapse open or close every group.
//!
//! Hovering a row offers Add Account, Add Group and Delete, as in Books.
//!
//! Differences from Books:
//! - An account opens in the full form, not the quick-edit panel.
//! - Balances are summed as money, not as floating-point numbers.

use std::collections::{HashMap, HashSet};

use crate::books::{Text as _, gray};
use crate::widgets::{Button, DialogButton, Kind as DialogKind, dialog, toast};
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
    children: HashMap<Option<String>, Vec<Account>>,
    expanded: HashSet<String>,
    totals: HashMap<String, (Money, Money)>,
    adding: Option<Adding>,
    error: Option<String>,
    /// An account waiting for Delete to be confirmed (`deleteDocWithPrompt`).
    deleting: Option<Account>,
    /// A dialog's title and text, as `showDialog` shows an error.
    dialog: Option<(String, String)>,
    /// What worked, in a toast.
    info: Option<String>,
    /// `isAllExpanded`, `isAllCollapsed`: which of Expand and Collapse are offered.
    all_expanded: bool,
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
        let mut page = Self {
            session,
            children: HashMap::new(),
            expanded: HashSet::new(),
            totals: HashMap::new(),
            adding: None,
            error: None,
            deleting: None,
            dialog: None,
            info: None,
            all_expanded: false,
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
            self.all_expanded = false;
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
        self.all_expanded = expand;
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

    /// `canDeleteAccount`: a group with accounts under it can't go.
    fn ask_delete(&mut self, account: &Account, cx: &mut Context<Self>) {
        let key = Some(account.name.clone());
        let has_children = match self.children.get(&key) {
            Some(children) => !children.is_empty(),
            None => self.fetch_children(Some(&account.name), cx),
        };
        if has_children {
            self.dialog = Some((
                t("Cannot Delete Account"),
                format!("{} has linked child accounts.", account.name),
            ));
        } else {
            self.deleting = Some(account.clone());
        }
        cx.notify();
    }

    fn delete(&mut self, cx: &mut Context<Self>) {
        let Some(account) = self.deleting.take() else {
            return;
        };
        let result = {
            let mut s = self.session.borrow_mut();
            let fyo = &mut s.fyo;
            fyo.get_doc("Account", Some(&account.name))
                .and_then(|mut doc| doc.delete(fyo))
        };
        match result {
            Ok(()) => {
                self.info = Some(format!("{} deleted", account.name));
                let parent = self
                    .children
                    .iter()
                    .find(|(_, list)| list.iter().any(|a| a.name == account.name))
                    .map(|(parent, _)| parent.clone());
                if let Some(parent) = parent {
                    self.fetch_children(parent.as_deref(), cx);
                }
            }
            Err(e) => self.dialog = Some((t("Error"), e.to_string())),
        }
        cx.notify();
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

/// `getIconComponent`: the root accounts' own icons, else a folder or a circle.
fn account_icon(account: &Account) -> Img {
    let (path, size) = match account.name.as_str() {
        "Application of Funds (Assets)" => ("books/coa/assets.svg", 16.),
        "Expenses" => ("books/coa/expenses.svg", 16.),
        "Income" => ("books/coa/income.svg", 16.),
        "Source of Funds (Liabilities)" => ("books/coa/liabilities.svg", 16.),
        _ if account.is_group => ("books/coa/folder.svg", 12.),
        _ => ("books/coa/leaf.svg", 8.),
    };
    img(crate::books::themed_icon(path))
        .size(px(size))
        .flex_none()
}

/// The small text buttons a row shows on hover.
fn row_button(id: impl Into<ElementId>, label: String) -> Stateful<Div> {
    div()
        .id(id)
        .text_11()
        .text_color(gray(800))
        .cursor_pointer()
        .hover(|d| d.text_color(gray(900)))
        .child(label)
}

impl Render for ChartOfAccounts {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = page_header(t("Chart of Accounts"), cx).child(
            div()
                .h_flex()
                .gap_2()
                .when(!self.all_expanded, |d| {
                    d.child(
                        Button::new("expand")
                            .label(t("Expand"))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_all(true, cx))),
                    )
                })
                .when(!self.expanded.is_empty(), |d| {
                    d.child(
                        Button::new("collapse")
                            .label(t("Collapse"))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_all(false, cx))),
                    )
                }),
        );
        let mut rows = Vec::new();
        self.rows(None, 0, &mut rows);
        let mut children: Vec<AnyElement> = Vec::new();
        for (i, (account, level)) in rows.into_iter().enumerate() {
            // `getItemStyle`: `calc(1rem + 2rem * level)`, one row high plus its border.
            let indent = px(16. + 32. * level as f32);
            let click = account.clone();
            let add_account = account.clone();
            let add_group = account.clone();
            let delete = account.clone();
            let group = SharedString::from(format!("account-row-{i}"));
            children.push(
                div()
                    .id(("account", i))
                    .group(group.clone())
                    .h_flex()
                    .flex_none()
                    .h(px(49.))
                    .pl(indent)
                    .pr_4()
                    .border_b_1()
                    .border_color(gray(200))
                    .cursor_pointer()
                    .hover(|d| d.bg(gray(50)))
                    .map(|d| if level == 0 { d.text_14() } else { d.text_13() })
                    .on_click(cx.listener(move |this, _, _, cx| this.on_click(&click, cx)))
                    .child(
                        div()
                            .w_4()
                            .h_flex()
                            .justify_center()
                            .child(account_icon(&account)),
                    )
                    .child(
                        div()
                            .ml_4()
                            .when(level == 0, |d| d.font_weight(FontWeight::SEMIBOLD))
                            .child(account.name.clone()),
                    )
                    .child(
                        div()
                            .ml_6()
                            .h_flex()
                            .gap_3()
                            .invisible()
                            .group_hover(group, |d| d.visible())
                            .when(account.is_group, |d| {
                                d.child(row_button(("add-account", i), t("Add Account")).on_click(
                                    cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.start_adding(&add_account, false, window, cx)
                                    }),
                                ))
                                .child(
                                    row_button(("add-group", i), t("Add Group")).on_click(
                                        cx.listener(move |this, _, window, cx| {
                                            cx.stop_propagation();
                                            this.start_adding(&add_group, true, window, cx)
                                        }),
                                    ),
                                )
                            })
                            .child(
                                row_button(
                                    ("delete-account", i),
                                    t(if account.is_group {
                                        "Delete Group"
                                    } else {
                                        "Delete Account"
                                    }),
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.ask_delete(&delete, cx)
                                    },
                                )),
                            ),
                    )
                    .when(!account.is_group, |d| {
                        d.child(
                            div()
                                .ml_auto()
                                .text_13()
                                .text_color(gray(800))
                                .child(self.balance(&account)),
                        )
                    })
                    .into_any_element(),
            );
            if let Some(adding) = &self.adding
                && adding.parent == account.name
            {
                let icon = Account {
                    name: String::new(),
                    is_group: adding.is_group,
                    root_type: String::new(),
                    account_type: String::new(),
                };
                let parent = account.clone();
                let is_group = adding.is_group;
                let input = adding.input.clone();
                children.push(
                    div()
                        .h_flex()
                        .flex_none()
                        .h(px(49.))
                        .pl(px(16. + 32. * (level + 1) as f32))
                        .pr_4()
                        .border_b_1()
                        .border_color(gray(200))
                        .text_13()
                        .child(
                            div()
                                .w_4()
                                .h_flex()
                                .justify_center()
                                .child(account_icon(&icon)),
                        )
                        .child(
                            div()
                                .ml_4()
                                .h_flex()
                                .child(
                                    div().w(px(200.)).child(
                                        Input::new(&adding.input).appearance(false).text_13(),
                                    ),
                                )
                                .child(row_button("adding-save", t("Save")).ml_4().on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        let name = input.read(cx).value().trim().to_owned();
                                        if !name.is_empty() {
                                            this.create(&parent, &name, is_group, cx);
                                        }
                                    }),
                                ))
                                .child(row_button("adding-cancel", t("Cancel")).ml_4().on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.adding = None;
                                        cx.notify();
                                    }),
                                )),
                        )
                        .into_any_element(),
                );
            }
        }

        let dialog = if let Some((title, detail)) = self.dialog.clone() {
            let close = cx.listener(|this, _, _, cx| {
                this.dialog = None;
                cx.notify();
            });
            Some(
                dialog(
                    title,
                    Some(detail.into()),
                    DialogKind::Error,
                    vec![DialogButton::new(t("Okay"), false, close)],
                    window,
                )
                .into_any_element(),
            )
        } else {
            self.deleting.as_ref().map(|account| {
                let yes = cx.listener(|this, _, _, cx| this.delete(cx));
                let no = cx.listener(|this, _, _, cx| {
                    this.deleting = None;
                    cx.notify();
                });
                dialog(
                    format!("Delete {}?", account.name),
                    Some(t("This action is permanent.").into()),
                    DialogKind::Warning,
                    vec![
                        DialogButton::new(t("Yes"), true, yes),
                        DialogButton::new(t("No"), false, no),
                    ],
                    window,
                )
                .into_any_element()
            })
        };
        let toast = self.info.clone().map(|message| {
            toast(
                message,
                DialogKind::Success,
                cx.listener(|this, _, _, cx| {
                    this.info = None;
                    cx.notify();
                }),
                window,
            )
        });

        div()
            .v_flex()
            .size_full()
            .text_color(gray(900))
            .child(header)
            .children(self.error.clone().map(|e| error_message(e, cx)))
            .child(
                div()
                    .id("coa")
                    .v_flex()
                    .flex_1()
                    .mb_4()
                    .overflow_y_scroll()
                    .children(children),
            )
            .children(dialog)
            .children(toast)
    }
}
