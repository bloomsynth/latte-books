//! A document's form, drawn from its schema: `src/pages/CommonForm/CommonForm.vue`,
//! `CommonFormSection.vue`, `FormHeader.vue` and the controls in
//! `src/components/Controls` @ v0.36.0.
//!
//! Every change goes through `Doc::set`, so formulas, validations and the model's `change`
//! hook run as in Books, and the form then shows what the document holds.
//!
//! Differences from Books:
//! - A child row is edited only in its table's columns; Books also opens a row form with
//!   the row's other fields.
//! - Attachments and images are shown but can't be changed yet.

use std::collections::HashMap;

use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::t;
use latte_core::Doc;
use latte_core::models::base_models::{invoice, lead, sales_quote};
use latte_core::models::helpers::single_is_truthy;
use latte_core::models::inventory::stock_transfer;
use latte_core::views::field::{self, Tab};
use latte_core::views::list;
use latte_core::views::sidebar::Route;
use latte_db::DocValue;
use latte_schema::{Field, FieldType};

use crate::link::{LinkInput, LinkOption, LinkPicked};
use crate::session::Shared;
use crate::widgets::{badge, error_message, field_label, info_message, page_header};
use crate::{Nav, Navigate};

/// Where a control's value lives: a field of the document or a cell of a table row.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Key {
    Field(String),
    Cell(String, usize, String),
}

enum Control {
    Input {
        state: Entity<InputState>,
        _subscription: Subscription,
    },
    Link {
        state: Entity<LinkInput>,
        _subscription: Subscription,
    },
    Check,
    Display,
}

/// An action waiting for the user to confirm it, as Books' dialogs ask.
#[derive(Clone, Copy, PartialEq)]
enum Pending {
    Submit,
    Cancel,
    Delete,
}

/// A control a tab needs: its key, field, value, whether read-only, and its options.
type Wanted = (Key, Field, Option<DocValue>, bool, Vec<LinkOption>);

pub struct FormPage {
    session: Shared,
    doc: Doc,
    tabs: Vec<Tab>,
    tab: usize,
    controls: HashMap<Key, Control>,
    error: Option<String>,
    info: Option<String>,
    pending: Option<Pending>,
    /// Shown inside another page (Settings), without its own header.
    embedded: bool,
    scroll: ScrollHandle,
}

impl EventEmitter<Navigate> for FormPage {}

impl FormPage {
    pub fn new(session: Shared, doc: Doc, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut page = Self {
            session,
            doc,
            tabs: Vec::new(),
            tab: 0,
            controls: HashMap::new(),
            error: None,
            info: None,
            pending: None,
            embedded: false,
            scroll: ScrollHandle::new(),
        };
        // `focusOrSelectFormControl`: a new document named by hand starts with an empty
        // name, not "New Party 01".
        let temporary = {
            let s = page.session.borrow();
            let doc = &page.doc;
            doc.not_inserted()
                && doc.schema().naming == Some(latte_schema::Naming::Manual)
                && doc
                    .name()
                    .is_some_and(|name| s.fyo.is_temporary_name(name, doc.schema()))
        };
        if temporary {
            page.doc
                .set_value("name", latte_db::DocValue::Text(String::new()));
        }
        page.refresh(window, cx);
        page
    }

    /// Loads a stored document, the single `schema`, or a new one when `name` is `None`.
    pub fn build(
        session: &Shared,
        schema: &str,
        name: Option<&str>,
        values: latte_db::DocValueMap,
    ) -> Result<Doc, String> {
        let mut s = session.borrow_mut();
        let is_single = s.fyo.schema(schema).is_some_and(|schema| schema.is_single);
        if is_single {
            s.fyo.get_doc(schema, None)
        } else if let Some(name) = name {
            s.fyo.get_doc(schema, Some(name))
        } else {
            s.fyo.get_new_doc(schema, values)
        }
        .map_err(|e| e.to_string())
    }

    pub fn embedded(mut self) -> Self {
        self.embedded = true;
        self
    }

    /// Saves the document if it has changes (Settings' Save).
    pub fn save_if_dirty(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        self.commit_typed(window, cx);
        if !self.doc.can_save() {
            return Ok(());
        }
        let label = self.doc.schema().label.clone();
        if self.run("Saved", |doc, fyo| doc.sync(fyo), window, cx) {
            Ok(())
        } else {
            Err(format!(
                "{label}: {}",
                self.error.clone().unwrap_or_default()
            ))
        }
    }

    pub fn doc(&self) -> &Doc {
        &self.doc
    }

    fn title(&self) -> String {
        if self.doc.schema().is_single {
            return self.doc.schema().label.clone();
        }
        match self.doc.name() {
            Some(name) if self.doc.inserted() => name.to_owned(),
            _ => format!("New {}", self.doc.schema().label),
        }
    }

    /// Lays the form out again and shows what the document holds now. Inputs being typed
    /// into keep their text.
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let session = self.session.clone();
        let s = session.borrow();
        let fyo = &s.fyo;
        self.tabs = field::form_layout(fyo, &self.doc);
        if self.tab >= self.tabs.len() {
            self.tab = 0;
        }

        // Every control the current tab needs, with its field and value.
        let mut wanted: Vec<Wanted> = Vec::new();
        let mut option_cache: HashMap<String, Vec<LinkOption>> = HashMap::new();
        let mut options = |f: &Field, ctx: &latte_core::DocContext<'_>| -> Vec<LinkOption> {
            match f.fieldtype {
                Some(FieldType::Link | FieldType::DynamicLink) => {
                    let filters = field::link_filters(f, ctx);
                    let target = field::link_target(f, ctx).unwrap_or_default();
                    let cache_key = format!("{target}{filters:?}");
                    option_cache
                        .entry(cache_key)
                        .or_insert_with(|| {
                            field::link_options(fyo, f, ctx)
                                .unwrap_or_default()
                                .into_iter()
                                .map(|(value, label)| LinkOption {
                                    value: value.into(),
                                    label: label.into(),
                                })
                                .collect()
                        })
                        .clone()
                }
                _ => field::select_options(f, ctx)
                    .into_iter()
                    .map(|o| LinkOption {
                        value: o.value.into(),
                        label: o.label.into(),
                    })
                    .collect(),
            }
        };
        let ctx = field::context(fyo, &self.doc, None);
        for section in self
            .tabs
            .get(self.tab)
            .map(|t| &t.sections[..])
            .unwrap_or(&[])
        {
            for f in &section.fields {
                if f.fieldtype == Some(FieldType::Table) {
                    for column in field::table_columns(fyo, f) {
                        for (i, row) in self.doc.table(&f.fieldname).iter().enumerate() {
                            let row_ctx = field::context(fyo, row, Some(&self.doc));
                            let read_only = field::is_read_only(&column, &row_ctx);
                            let opts = if is_choice(&column) && !read_only {
                                options(&column, &row_ctx)
                            } else {
                                Vec::new()
                            };
                            wanted.push((
                                Key::Cell(f.fieldname.clone(), i, column.fieldname.clone()),
                                column.clone(),
                                row.get(&column.fieldname).cloned(),
                                read_only,
                                opts,
                            ));
                        }
                    }
                    continue;
                }
                let read_only = field::is_read_only(f, &ctx);
                let opts = if is_choice(f) && !read_only {
                    options(f, &ctx)
                } else {
                    Vec::new()
                };
                wanted.push((
                    Key::Field(f.fieldname.clone()),
                    f.clone(),
                    self.doc.get(&f.fieldname).cloned(),
                    read_only,
                    opts,
                ));
            }
        }
        drop(s);

        let keys: Vec<Key> = wanted.iter().map(|(key, ..)| key.clone()).collect();
        self.controls.retain(|key, _| keys.contains(key));
        for (key, f, value, read_only, opts) in wanted {
            let kind = control_kind(&f, read_only);
            let matches = matches!(
                (self.controls.get(&key), kind),
                (Some(Control::Input { .. }), Kind::Input)
                    | (Some(Control::Link { .. }), Kind::Link)
                    | (Some(Control::Check), Kind::Check)
                    | (Some(Control::Display), Kind::Display)
            );
            if !matches {
                let control = self.make_control(&key, &f, kind, opts.clone(), window, cx);
                self.controls.insert(key.clone(), control);
            }
            match self.controls.get(&key) {
                Some(Control::Input { state, .. }) => {
                    let text = field::edit_text(&f, value.as_ref());
                    state.update(cx, |input, cx| {
                        if !input.focus_handle(cx).is_focused(window) && input.value() != text {
                            input.set_value(text, window, cx);
                        }
                    });
                }
                Some(Control::Link { state, .. }) => {
                    let value = match value {
                        Some(DocValue::Text(text)) if !text.is_empty() => Some(text.into()),
                        _ => None,
                    };
                    state.update(cx, |link, cx| {
                        link.set_options(opts, cx);
                        if !link.is_focused(window, cx) {
                            link.set_value(value, window, cx);
                        }
                    });
                }
                _ => {}
            }
        }
        cx.notify();
    }

    fn make_control(
        &mut self,
        key: &Key,
        f: &Field,
        kind: Kind,
        options: Vec<LinkOption>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Control {
        let placeholder = f
            .placeholder
            .clone()
            .or_else(|| f.label.clone())
            .unwrap_or_default();
        match kind {
            Kind::Input => {
                let state = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
                let key = key.clone();
                let field = f.clone();
                let subscription = cx.subscribe_in(
                    &state,
                    window,
                    move |this, state, event: &InputEvent, window, cx| {
                        if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                            let text = state.read(cx).value().to_string();
                            this.commit_text(&key, &field, &text, window, cx);
                        }
                    },
                );
                Control::Input {
                    state,
                    _subscription: subscription,
                }
            }
            Kind::Link => {
                let state = cx.new(|cx| LinkInput::new(options, placeholder, window, cx));
                let key = key.clone();
                let subscription = cx.subscribe_in(
                    &state,
                    window,
                    move |this, _, event: &LinkPicked, window, cx| {
                        let value = if event.0.is_empty() {
                            DocValue::Null
                        } else {
                            DocValue::Text(event.0.to_string())
                        };
                        this.set_value(&key, value, window, cx);
                    },
                );
                Control::Link {
                    state,
                    _subscription: subscription,
                }
            }
            Kind::Check => Control::Check,
            Kind::Display => Control::Display,
        }
    }

    fn current_value(&self, key: &Key) -> Option<&DocValue> {
        match key {
            Key::Field(fieldname) => self.doc.get(fieldname),
            Key::Cell(table, i, column) => self.doc.table(table).get(*i)?.get(column),
        }
    }

    fn commit_text(
        &mut self,
        key: &Key,
        f: &Field,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if field::edit_text(f, self.current_value(key)) == text {
            return;
        }
        let parsed = {
            let s = self.session.borrow();
            field::parse(&s.fyo, f, text)
        };
        match parsed {
            Ok(value) => self.set_value(key, value, window, cx),
            Err(e) => {
                self.error = Some(e.to_string());
                cx.notify();
            }
        }
    }

    fn set_value(
        &mut self,
        key: &Key,
        value: DocValue,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = {
            let mut s = self.session.borrow_mut();
            match key {
                Key::Field(fieldname) => self.doc.set(&mut s.fyo, fieldname, value),
                Key::Cell(table, i, column) => {
                    self.doc.set_child(&mut s.fyo, table, *i, column, value)
                }
            }
        };
        self.error = result.err().map(|e| e.to_string());
        self.info = None;
        self.refresh(window, cx);
    }

    fn add_row(&mut self, table: &str, window: &mut Window, cx: &mut Context<Self>) {
        let result = {
            let mut s = self.session.borrow_mut();
            self.doc
                .append(&mut s.fyo, table, latte_db::DocValueMap::new())
        };
        self.error = result.err().map(|e| e.to_string());
        self.refresh(window, cx);
    }

    fn remove_row(&mut self, table: &str, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let idx = self
            .doc
            .table(table)
            .get(i)
            .and_then(|row| match row.get("idx") {
                Some(DocValue::Number(n)) => Some(*n as usize),
                _ => None,
            });
        let result = {
            let mut s = self.session.borrow_mut();
            self.doc.remove(&mut s.fyo, table, idx.unwrap_or(i))
        };
        self.error = result.err().map(|e| e.to_string());
        // Row controls are keyed by position, so they are made again.
        self.controls
            .retain(|key, _| !matches!(key, Key::Cell(t, ..) if t == table));
        self.refresh(window, cx);
    }

    /// Sets every value typed into an input and not yet set.
    fn commit_typed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed: Vec<(Key, String)> = self
            .controls
            .iter()
            .filter_map(|(key, control)| match control {
                Control::Input { state, .. } => {
                    Some((key.clone(), state.read(cx).value().to_string()))
                }
                _ => None,
            })
            .collect();
        for (key, text) in typed {
            let field = {
                let s = self.session.borrow();
                match &key {
                    Key::Field(fieldname) => self.doc.schema().field(fieldname).cloned(),
                    Key::Cell(table, _, column) => self.doc.schema().field(table).and_then(|f| {
                        field::table_columns(&s.fyo, f)
                            .into_iter()
                            .find(|c| &c.fieldname == column)
                    }),
                }
            };
            if let Some(field) = field {
                self.commit_text(&key, &field, &text, window, cx);
            }
        }
    }

    /// Runs a document action; on success shows `done` and lays the form out again.
    fn run(
        &mut self,
        done: &str,
        action: impl FnOnce(&mut Doc, &mut latte_core::Fyo) -> Result<(), latte_core::Error>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.pending = None;
        // Commit what is being typed first, as Books blurs the input before saving.
        self.commit_typed(window, cx);
        let result = {
            let mut s = self.session.borrow_mut();
            let result = action(&mut self.doc, &mut s.fyo);
            if self.doc.schema().is_single {
                s.refresh_formatter();
            }
            result
        };
        match result {
            Ok(()) => {
                self.error = None;
                // Settings says it once for all its tabs.
                self.info = (!self.embedded).then(|| done.to_owned());
                self.controls.clear();
                self.refresh(window, cx);
                true
            }
            Err(e) => {
                self.error = Some(e.to_string());
                self.info = None;
                cx.notify();
                false
            }
        }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.run("Saved", |doc, fyo| doc.sync(fyo), window, cx)
            && self.doc.schema_name() == "CustomForm"
        {
            let name = self.doc.name().map(str::to_owned);
            cx.emit(Navigate(Nav::Reopen(Route::Form {
                schema: "CustomForm".to_owned(),
                name,
            })));
        }
    }

    fn confirm(&mut self, pending: Pending, window: &mut Window, cx: &mut Context<Self>) {
        match pending {
            Pending::Submit => {
                self.run("Submitted", |doc, fyo| doc.submit(fyo), window, cx);
            }
            Pending::Cancel => {
                self.run("Cancelled", |doc, fyo| doc.cancel(fyo), window, cx);
            }
            Pending::Delete => {
                let schema = self.doc.schema_name().to_owned();
                let label = self.doc.schema().label.clone();
                if self.run("Deleted", |doc, fyo| doc.delete(fyo), window, cx) {
                    let list = Route::list(&schema, &label, None);
                    cx.emit(Navigate(if schema == "CustomForm" {
                        Nav::Reopen(list)
                    } else {
                        Nav::Route(list)
                    }));
                }
            }
        }
    }

    /// Opens a document made from this one (a payment, a return, a copy) as a new form.
    fn open_made(
        &mut self,
        make: impl FnOnce(&Doc, &latte_core::Fyo) -> Result<Option<Doc>, latte_core::Error>,
        cx: &mut Context<Self>,
    ) {
        let made = {
            let s = self.session.borrow();
            make(&self.doc, &s.fyo)
        };
        match made {
            Ok(Some(doc)) => cx.emit(Navigate(Nav::Doc(doc))),
            Ok(None) => {}
            Err(e) => {
                self.error = Some(e.to_string());
                cx.notify();
            }
        }
    }

    /// As `open_made`, for a document made with the file open for writing.
    fn open_made_mut(
        &mut self,
        make: impl FnOnce(&Doc, &mut latte_core::Fyo) -> Result<Option<Doc>, latte_core::Error>,
        cx: &mut Context<Self>,
    ) {
        let made = {
            let mut s = self.session.borrow_mut();
            make(&self.doc, &mut s.fyo)
        };
        match made {
            Ok(Some(doc)) => cx.emit(Navigate(Nav::Doc(doc))),
            Ok(None) => {}
            Err(e) => {
                self.error = Some(e.to_string());
                cx.notify();
            }
        }
    }

    fn status_badge(&self, cx: &App) -> Option<Div> {
        let doc = &self.doc;
        if doc.schema().is_single {
            return None;
        }
        let status = if doc.not_inserted() {
            "Draft"
        } else if doc.dirty() {
            "NotSaved"
        } else {
            let mut values = latte_db::DocValueMap::new();
            for f in &doc.schema().fields {
                if let Some(value) = doc.get(&f.fieldname) {
                    values.insert(f.fieldname.clone(), value.clone());
                }
            }
            list::doc_status(doc.schema(), &values)
        };
        let status = list::status(status);
        (!status.label.is_empty()).then(|| badge(status.label, status.color, cx))
    }

    fn buttons(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let doc = &self.doc;
        let mut buttons: Vec<AnyElement> = Vec::new();
        let schema = doc.schema_name().to_owned();
        let is_invoice = matches!(schema.as_str(), "SalesInvoice" | "PurchaseInvoice");
        let is_transfer = matches!(schema.as_str(), "Shipment" | "PurchaseReceipt");
        let open = doc.is_submitted() && !doc.is_cancelled();
        let filled = |fieldname: &str| matches!(doc.get(fieldname), Some(DocValue::Text(t)) if !t.is_empty());
        // `getMakeReturnDocAction`: returns are turned on in the settings.
        let returns_on = {
            let s = self.session.borrow();
            single_is_truthy(&s.fyo, "AccountingSettings", "enableInvoiceReturns")
                || single_is_truthy(&s.fyo, "InventorySettings", "enableStockReturns")
        };

        if open && is_invoice {
            let outstanding =
                matches!(doc.get("outstandingAmount"), Some(DocValue::Money(m)) if !m.is_zero());
            if outstanding {
                buttons.push(
                    Button::new("make-payment")
                        .small()
                        .label(t("Make Payment"))
                        .on_click(
                            cx.listener(|this, _, _, cx| this.open_made(invoice::get_payment, cx)),
                        )
                        .into_any_element(),
                );
            }
            // `getMakeStockTransferAction`.
            let not_transferred =
                matches!(doc.get("stockNotTransferred"), Some(DocValue::Number(n)) if *n != 0.0);
            if not_transferred {
                let label = if schema == "SalesInvoice" {
                    "Shipment"
                } else {
                    "Purchase Receipt"
                };
                buttons.push(
                    Button::new("make-stock-transfer")
                        .small()
                        .label(t(label))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.open_made(
                                |doc, fyo| invoice::get_stock_transfer(doc, fyo, false),
                                cx,
                            )
                        }))
                        .into_any_element(),
                );
            }
            if returns_on && !filled("returnAgainst") {
                buttons.push(
                    Button::new("make-return")
                        .small()
                        .label(t("Return"))
                        .on_click(
                            cx.listener(|this, _, _, cx| {
                                this.open_made(invoice::get_return_doc, cx)
                            }),
                        )
                        .into_any_element(),
                );
            }
        }
        if open && is_transfer {
            // `getMakeInvoiceAction`.
            if !filled("backReference") {
                let label = if schema == "Shipment" {
                    "Sales Invoice"
                } else {
                    "Purchase Invoice"
                };
                buttons.push(
                    Button::new("make-invoice")
                        .small()
                        .label(t(label))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.open_made(stock_transfer::get_invoice, cx)
                        }))
                        .into_any_element(),
                );
            }
            if returns_on && !filled("returnAgainst") {
                buttons.push(
                    Button::new("make-return")
                        .small()
                        .label(t("Return"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.open_made(stock_transfer::get_return_doc, cx)
                        }))
                        .into_any_element(),
                );
            }
        }
        // `getLeadActions`.
        if schema == "Lead" && !doc.not_inserted() {
            buttons.push(
                Button::new("make-customer")
                    .small()
                    .label(t("Customer"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.open_made(|doc, fyo| lead::create_customer(doc, fyo).map(Some), cx)
                    }))
                    .into_any_element(),
            );
            buttons.push(
                Button::new("make-sales-quote")
                    .small()
                    .label(t("Sales Quote"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.open_made(|doc, fyo| lead::create_sales_quote(doc, fyo).map(Some), cx)
                    }))
                    .into_any_element(),
            );
        }
        // `getQuoteActions`.
        if open && schema == "SalesQuote" {
            buttons.push(
                Button::new("make-invoice")
                    .small()
                    .label(t("Sales Invoice"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.open_made_mut(sales_quote::get_invoice, cx)
                    }))
                    .into_any_element(),
            );
        }
        if open && crate::print::can_print(&schema) {
            let name = doc.name().unwrap_or_default().to_owned();
            buttons.push(
                Button::new("print")
                    .small()
                    .label(t("Print"))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(Navigate(Nav::Route(Route::Print {
                            schema: schema.clone(),
                            name: name.clone(),
                        })))
                    }))
                    .into_any_element(),
            );
        }
        if doc.inserted() && !doc.schema().is_single && !doc.schema().is_submittable
            || doc.inserted() && doc.schema().is_submittable
        {
            buttons.push(
                Button::new("duplicate")
                    .small()
                    .ghost()
                    .label(t("Duplicate"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.open_made(|doc, fyo| doc.duplicate(fyo).map(Some), cx)
                    }))
                    .into_any_element(),
            );
        }
        if doc.inserted() && doc.can_delete() && !doc.schema().is_single {
            buttons.push(
                Button::new("delete")
                    .small()
                    .ghost()
                    .label(t("Delete"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.pending = Some(Pending::Delete);
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        if doc.can_cancel() {
            buttons.push(
                Button::new("cancel")
                    .small()
                    .label(t("Cancel"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.pending = Some(Pending::Cancel);
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        if doc.can_save() || doc.not_inserted() || doc.schema().is_single && doc.dirty() {
            buttons.push(
                Button::new("save")
                    .small()
                    .primary()
                    .label(t("Save"))
                    .on_click(cx.listener(|this, _, window, cx| this.save(window, cx)))
                    .into_any_element(),
            );
        } else if doc.can_submit() {
            buttons.push(
                Button::new("submit")
                    .small()
                    .primary()
                    .label(t("Submit"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.pending = Some(Pending::Submit);
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        buttons
    }

    fn render_control(&self, key: &Key, f: &Field, cx: &mut Context<Self>) -> AnyElement {
        let s = self.session.borrow();
        match self.controls.get(key) {
            Some(Control::Input { state, .. }) => Input::new(state).small().into_any_element(),
            Some(Control::Link { state, .. }) => state.clone().into_any_element(),
            Some(Control::Check) => {
                let checked = latte_core::fyo::model::helpers::is_truthy(self.current_value(key));
                let key = key.clone();
                let id: SharedString = format!("{key:?}").into();
                let label = match key {
                    Key::Field(_) => f.label.clone().unwrap_or_else(|| f.fieldname.clone()),
                    Key::Cell(..) => String::new(),
                };
                Checkbox::new(ElementId::Name(id))
                    .checked(checked)
                    .label(label)
                    .on_click(cx.listener(move |this, checked: &bool, window, cx| {
                        this.set_value(&key, DocValue::Bool(*checked), window, cx);
                    }))
                    .into_any_element()
            }
            Some(Control::Display) | None => {
                let text = field::display(&s.formatter, f, self.current_value(key));
                div()
                    .h(px(28.))
                    .px_2()
                    .h_flex()
                    .when(is_numeric(f), |d| d.justify_end())
                    .rounded_md()
                    .bg(cx.theme().muted)
                    .truncate()
                    .child(text)
                    .into_any_element()
            }
        }
    }

    fn render_table(&self, f: &Field, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let columns = {
            let s = self.session.borrow();
            field::table_columns(&s.fyo, f)
        };
        let editable = self.doc.can_edit();
        let table = f.fieldname.clone();
        let header = div()
            .h_flex()
            .w_full()
            .py_1()
            .border_b_1()
            .border_color(theme.border)
            .text_sm()
            .text_color(theme.muted_foreground)
            .child(div().w(px(36.)).child("#"))
            .children(
                columns
                    .iter()
                    .map(|c| cell(c).child(c.label.clone().unwrap_or_else(|| c.fieldname.clone()))),
            )
            .child(div().w(px(28.)));
        let rows = self
            .doc
            .table(&f.fieldname)
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let table = table.clone();
                div()
                    .h_flex()
                    .w_full()
                    .py_1()
                    .gap_1()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().w(px(36.)).child(format!("{}", i + 1)))
                    .children(columns.iter().map(|c| {
                        let key = Key::Cell(f.fieldname.clone(), i, c.fieldname.clone());
                        cell(c).child(self.render_control(&key, c, cx))
                    }))
                    .child(div().w(px(28.)).children(editable.then(|| {
                        Button::new(("remove-row", i))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Close)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.remove_row(&table, i, window, cx)
                            }))
                    })))
            })
            .collect::<Vec<_>>();
        let add_table = f.fieldname.clone();
        div()
            .v_flex()
            .w_full()
            .gap_1()
            .child(field_label(f.label.clone().unwrap_or_default(), false, cx))
            .child(
                div()
                    .v_flex()
                    .w_full()
                    .border_1()
                    .border_color(theme.border)
                    .rounded_md()
                    .px_2()
                    .child(header)
                    .children(rows)
                    .children(editable.then(|| {
                        div().py_1().child(
                            Button::new(SharedString::from(format!("add-{add_table}")))
                                .ghost()
                                .small()
                                .icon(IconName::Plus)
                                .label(t("Add Row"))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.add_row(&add_table, window, cx)
                                })),
                        )
                    })),
            )
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Input,
    Link,
    Check,
    Display,
}

fn is_choice(f: &Field) -> bool {
    matches!(
        f.fieldtype,
        Some(
            FieldType::Link | FieldType::DynamicLink | FieldType::Select | FieldType::AutoComplete
        )
    )
}

fn is_numeric(f: &Field) -> bool {
    matches!(
        f.fieldtype,
        Some(FieldType::Int | FieldType::Float | FieldType::Currency)
    )
}

fn control_kind(f: &Field, read_only: bool) -> Kind {
    match f.fieldtype {
        Some(FieldType::Check) => Kind::Check,
        _ if read_only => Kind::Display,
        Some(FieldType::Attachment | FieldType::AttachImage | FieldType::Button) => Kind::Display,
        _ if is_choice(f) => Kind::Link,
        _ => Kind::Input,
    }
}

/// A table cell, sized as Books' `Row` sizes columns: amounts narrower than names.
fn cell(f: &Field) -> Div {
    let grow = match f.fieldtype {
        Some(FieldType::Link | FieldType::Data | FieldType::Text) => 2.,
        _ => 1.,
    };
    div()
        .flex_grow(grow)
        .flex_basis(px(0.))
        .min_w(px(60.))
        .when(is_numeric(f), |d| d.h_flex().justify_end())
}

impl Render for FormPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let title = self.title();
        let status = self.status_badge(cx);
        let buttons = self.buttons(cx);
        let header =
            page_header(title, cx).child(div().h_flex().gap_2().children(status).children(buttons));

        let confirm = self.pending.map(|pending| {
            let (question, ok) = match pending {
                Pending::Submit => (
                    format!(
                        "Submit {}? It can't be edited after submitting.",
                        self.doc.schema().label
                    ),
                    "Submit",
                ),
                Pending::Cancel => (format!("Cancel {}?", self.title()), "Yes, cancel it"),
                Pending::Delete => (
                    format!("Delete {}? This can't be undone.", self.title()),
                    "Delete",
                ),
            };
            div()
                .h_flex()
                .mx_4()
                .mt_2()
                .px_3()
                .py_2()
                .gap_2()
                .rounded_md()
                .border_l_2()
                .border_color(theme.yellow)
                .bg(theme.yellow.opacity(0.12))
                .child(div().flex_1().child(question))
                .child(
                    Button::new("confirm-no")
                        .small()
                        .ghost()
                        .label(t("No"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.pending = None;
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("confirm-yes")
                        .small()
                        .primary()
                        .label(ok)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.confirm(pending, window, cx)
                        })),
                )
        });

        let tab_bar = (self.tabs.len() > 1).then(|| {
            div()
                .h_flex()
                .px_4()
                .gap_4()
                .border_b_1()
                .border_color(theme.border)
                .children(self.tabs.iter().enumerate().map(|(i, tab)| {
                    let active = i == self.tab;
                    div()
                        .id(("tab", i))
                        .py_2()
                        .cursor_pointer()
                        .when(active, |d| {
                            d.border_b_2()
                                .border_color(theme.primary)
                                .font_weight(FontWeight::SEMIBOLD)
                        })
                        .when(!active, |d| d.text_color(theme.muted_foreground))
                        .child(tab.name.clone())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.tab = i;
                            this.refresh(window, cx);
                        }))
                }))
        });

        let sections: Vec<AnyElement> = self
            .tabs
            .get(self.tab)
            .map(|tab| tab.sections.clone())
            .unwrap_or_default()
            .into_iter()
            .map(|section| {
                let (tables, fields): (Vec<Field>, Vec<Field>) = section
                    .fields
                    .into_iter()
                    .partition(|f| f.fieldtype == Some(FieldType::Table));
                let required = {
                    let s = self.session.borrow();
                    let ctx = field::context(&s.fyo, &self.doc, None);
                    fields
                        .iter()
                        .map(|f| field::is_required(f, &ctx))
                        .collect::<Vec<_>>()
                };
                let grid = div()
                    .grid()
                    .grid_cols(2)
                    .gap_x_6()
                    .gap_y_3()
                    .w_full()
                    .children(fields.iter().zip(required).map(|(f, required)| {
                        let key = Key::Field(f.fieldname.clone());
                        if f.fieldtype == Some(FieldType::Check) {
                            return div().pt_5().child(self.render_control(&key, f, cx));
                        }
                        div()
                            .v_flex()
                            .gap_1()
                            .child(field_label(
                                f.label.clone().unwrap_or_else(|| f.fieldname.clone()),
                                required,
                                cx,
                            ))
                            .child(self.render_control(&key, f, cx))
                    }));
                div()
                    .v_flex()
                    .gap_3()
                    .p_4()
                    .border_b_1()
                    .border_color(theme.border)
                    .children((section.name != "Default").then(|| {
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(section.name.clone())
                    }))
                    .when(!fields.is_empty(), |d| d.child(grid))
                    .children(tables.iter().map(|f| self.render_table(f, cx)))
                    .into_any_element()
            })
            .collect();

        div()
            .v_flex()
            .size_full()
            .when(!self.embedded, |d| d.child(header))
            .children(confirm)
            .children(self.error.clone().map(|e| error_message(e, cx)))
            .children(self.info.clone().map(|m| info_message(m, cx)))
            .children(tab_bar)
            .child(
                div()
                    .id("form-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(div().v_flex().w_full().max_w(px(1000.)).children(sections)),
            )
    }
}
