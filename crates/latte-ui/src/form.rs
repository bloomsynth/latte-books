//! A document's form, drawn from its schema: `src/pages/CommonForm/CommonForm.vue`,
//! `CommonFormSection.vue`, `FormHeader.vue` and the controls in
//! `src/components/Controls` @ v0.36.0.
//!
//! Every change goes through `Doc::set`, so formulas, validations and the model's `change`
//! hook run as in Books, and the form then shows what the document holds.
//!
//! As in Books, the page header holds the actions (Print, the width toggle, the Create and
//! View groups, the "…" menu, then Save or Submit), and the form is a 600px card below it,
//! or the full width. Submit, Cancel and Delete ask first in a dialog; what worked is said
//! in a toast; a value that can't be set is shown under its field.
//!
//! Differences from Books:
//! - A child row is edited only in its table's columns; Books also opens a row form with
//!   the row's other fields.
//! - Attachments are shown but can't be changed yet.
//! - No "View linked entries" panel, and the View actions aren't there yet.

use std::collections::HashMap;

use std::rc::Rc;
use std::sync::Arc;

use crate::books::{self, Text as _, black, gray, white};
use crate::widgets::{
    Button, DialogButton, Kind as DialogKind, check_box, control_frame, dialog, dropdown_item,
    field_label, mandatory_mark, pill, popover_panel, toast,
};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
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

use crate::link::{LinkInput, LinkOpened, LinkOption, LinkPicked};
use crate::session::Shared;
use crate::widgets::page_header_with;
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
        _subscriptions: [Subscription; 2],
    },
    /// `Text.vue`: three rows of text.
    Text {
        state: Entity<TextareaState>,
        _subscription: Subscription,
    },
    Check,
    Display,
    /// `AttachImage.vue`: a picture kept in the field as a data URL.
    Image,
}

/// An action waiting for the user to confirm it, as Books' dialogs ask.
#[derive(Clone, Copy, PartialEq)]
enum Pending {
    Submit,
    Cancel,
    Delete,
}

/// A control a tab needs: its key, field, value, whether read-only, its options, and
/// whether required.
type Wanted = (Key, Field, Option<DocValue>, bool, Vec<LinkOption>, bool);

pub struct FormPage {
    session: Shared,
    doc: Doc,
    tabs: Vec<Tab>,
    tab: usize,
    controls: HashMap<Key, Control>,
    /// An action's error, shown in a dialog titled by its kind (`handleErrorWithDialog`).
    error: Option<String>,
    error_title: &'static str,
    /// A value a control couldn't set, shown under it.
    field_errors: HashMap<Key, String>,
    /// What worked, shown in a toast.
    info: Option<String>,
    pending: Option<Pending>,
    /// The action group whose menu is open.
    menu: Option<&'static str>,
    /// "Toggle between form and full width".
    full_width: bool,
    /// The pictures of the AttachImage fields, by their data URL.
    images: HashMap<String, Arc<Image>>,
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
            error_title: "Error",
            field_errors: HashMap::new(),
            info: None,
            pending: None,
            menu: None,
            full_width: false,
            images: HashMap::new(),
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
        if self.run("saved", |doc, fyo| doc.sync(fyo), window, cx) {
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
                                field::is_required(&column, &row_ctx),
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
                    field::is_required(f, &ctx),
                ));
            }
        }
        drop(s);

        let keys: Vec<Key> = wanted.iter().map(|(key, ..)| key.clone()).collect();
        self.controls.retain(|key, _| keys.contains(key));
        for (key, f, value, read_only, opts, mandatory) in wanted {
            let kind = match control_kind(&f, read_only) {
                Kind::Input
                    if f.fieldtype == Some(FieldType::Text) && matches!(key, Key::Field(_)) =>
                {
                    Kind::Text
                }
                kind => kind,
            };
            let matches = matches!(
                (self.controls.get(&key), kind),
                (Some(Control::Input { .. }), Kind::Input)
                    | (Some(Control::Text { .. }), Kind::Text)
                    | (Some(Control::Link { .. }), Kind::Link)
                    | (Some(Control::Check), Kind::Check)
                    | (Some(Control::Display), Kind::Display)
                    | (Some(Control::Image), Kind::Image)
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
                Some(Control::Text { state, .. }) => {
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
                    let mandatory = value.is_none() && mandatory;
                    state.update(cx, |link, cx| {
                        link.set_mandatory(mandatory);
                        link.set_options(opts, cx);
                        if !link.is_focused(window, cx) {
                            link.set_value(value, window, cx);
                        }
                    });
                }
                _ => {}
            }
        }
        // The pictures shown, decoded once.
        let urls: Vec<String> = self
            .controls
            .iter()
            .filter(|(_, c)| matches!(c, Control::Image))
            .filter_map(|(key, _)| match self.current_value(key) {
                Some(DocValue::Text(url)) if !url.is_empty() => Some(url.clone()),
                _ => None,
            })
            .collect();
        self.images.retain(|url, _| urls.contains(url));
        for url in urls {
            if !self.images.contains_key(&url)
                && let Some(image) = image_from_data_url(&url)
            {
                self.images.insert(url, Arc::new(image));
            }
        }
        cx.notify();
    }

    /// `AttachImage.vue`'s upload: a picture from a file, kept as a data URL.
    fn pick_image(&mut self, key: Key, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t("Select Image").into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let url = data_url(&path);
            this.update_in(cx, |this, window, cx| match url {
                Ok(url) => this.set_value(&key, DocValue::Text(url), window, cx),
                Err(e) => {
                    this.field_errors.insert(key.clone(), e);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
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
            Kind::Text => {
                let state = cx.new(|cx| TextareaState::new(window, cx).placeholder(placeholder));
                let key = key.clone();
                let field = f.clone();
                let subscription = cx.subscribe_in(
                    &state,
                    window,
                    move |this, state, event: &InputEvent, window, cx| {
                        if matches!(event, InputEvent::Blur) {
                            let text = state.read(cx).value().to_string();
                            this.commit_text(&key, &field, &text, window, cx);
                        }
                    },
                );
                Control::Text {
                    state,
                    _subscription: subscription,
                }
            }
            Kind::Link => {
                let in_table = matches!(key, Key::Cell(..));
                let linkable = f.fieldtype == Some(FieldType::Link);
                let state = cx.new(|cx| {
                    let link = LinkInput::new(options, placeholder, window, cx).linkable(linkable);
                    if in_table { link.bare() } else { link }
                });
                let picked_key = key.clone();
                let picked = cx.subscribe_in(
                    &state,
                    window,
                    move |this, _, event: &LinkPicked, window, cx| {
                        let value = if event.0.is_empty() {
                            DocValue::Null
                        } else {
                            DocValue::Text(event.0.to_string())
                        };
                        this.set_value(&picked_key, value, window, cx);
                    },
                );
                let field = f.clone();
                let opened = cx.subscribe(&state, move |this, _, event: &LinkOpened, cx| {
                    this.open_link(&field, &event.0, cx);
                });
                Control::Link {
                    state,
                    _subscriptions: [picked, opened],
                }
            }
            Kind::Check => Control::Check,
            Kind::Display => Control::Display,
            Kind::Image => Control::Image,
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
                self.field_errors.insert(key.clone(), e.to_string());
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
        match result {
            Ok(_) => {
                self.field_errors.remove(key);
            }
            Err(e) => {
                self.field_errors.insert(key.clone(), e.to_string());
            }
        }
        self.info = None;
        self.refresh(window, cx);
    }

    /// The chevron of a Link field: opens the linked document (`routeToLinkedDoc`).
    fn open_link(&mut self, f: &Field, name: &str, cx: &mut Context<Self>) {
        let target = {
            let s = self.session.borrow();
            let ctx = field::context(&s.fyo, &self.doc, None);
            field::link_target(f, &ctx)
        };
        if let Some(schema) = target {
            cx.emit(Navigate(Nav::Route(Route::Form {
                schema,
                name: Some(name.to_owned()),
            })));
        }
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
                Control::Text { state, .. } => {
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

    /// Runs a document action; on success says "<name> `done`" in a toast (`showActionToast`)
    /// and lays the form out again.
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
                self.info = (!self.embedded).then(|| format!("{} {done}", self.reference_label()));
                self.controls.clear();
                self.refresh(window, cx);
                true
            }
            Err(e) => {
                self.error_title = error_label(&e);
                self.error = Some(e.to_string());
                self.info = None;
                cx.notify();
                false
            }
        }
    }

    /// `getDocReferenceLabel`: the document's name, or its schema's label.
    fn reference_label(&self) -> String {
        let schema = self.doc.schema();
        match self.doc.name() {
            Some(name) if schema.naming != Some(latte_schema::Naming::Random) => name.to_owned(),
            _ => schema.label.clone(),
        }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.run("saved", |doc, fyo| doc.sync(fyo), window, cx)
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
                self.run("submitted", |doc, fyo| doc.submit(fyo), window, cx);
            }
            Pending::Cancel => {
                self.run("cancelled", |doc, fyo| doc.cancel(fyo), window, cx);
            }
            Pending::Delete => {
                let schema = self.doc.schema_name().to_owned();
                let label = self.doc.schema().label.clone();
                if self.run("deleted", |doc, fyo| doc.delete(fyo), window, cx) {
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

    /// `StatusPill.vue`: the status beside the title, with what is owed when unpaid.
    fn status_pill(&self) -> Option<Div> {
        let doc = &self.doc;
        if doc.schema().is_single || doc.schema_name() == "SalesQuote" && doc.is_submitted() {
            return None;
        }
        let key = if doc.not_inserted() {
            "Draft".to_owned()
        } else if doc.dirty() {
            "NotSaved".to_owned()
        } else {
            let mut values = latte_db::DocValueMap::new();
            for f in &doc.schema().fields {
                if let Some(value) = doc.get(&f.fieldname) {
                    values.insert(f.fieldname.clone(), value.clone());
                }
            }
            list::doc_status(doc.schema(), &values).to_owned()
        };
        let status = list::status(&key);
        if status.label.is_empty() {
            return None;
        }
        let money = |fieldname: &str| match doc.get(fieldname) {
            Some(DocValue::Money(m)) => Some(m.clone()),
            _ => None,
        };
        let s = self.session.borrow();
        let label = match (key.as_str(), money("outstandingAmount")) {
            ("Unpaid" | "Outstanding", Some(outstanding)) => {
                format!("{} {}", t("Unpaid"), s.formatter.currency(&outstanding))
            }
            ("PartlyPaid", Some(outstanding)) => match money("grandTotal") {
                Some(total) => format!(
                    "{} {}",
                    t("Partly Paid"),
                    s.formatter
                        .currency(&total.sub(&outstanding).unwrap_or(total.clone()))
                ),
                None => status.label.to_string(),
            },
            _ => status.label.to_string(),
        };
        Some(pill(label, status.color))
    }

    /// `getActionsForDoc`: the model's actions, then Duplicate, New Entry, Delete and
    /// Cancel, each where its condition holds.
    fn actions(&self) -> Vec<Action> {
        let doc = &self.doc;
        let mut actions: Vec<Action> = Vec::new();
        let schema = doc.schema_name().to_owned();
        let is_invoice = matches!(schema.as_str(), "SalesInvoice" | "PurchaseInvoice");
        let is_transfer = matches!(schema.as_str(), "Shipment" | "PurchaseReceipt");
        let submitted = doc.is_submitted() && !doc.is_cancelled();
        let filled = |fieldname: &str| matches!(doc.get(fieldname), Some(DocValue::Text(t)) if !t.is_empty());
        let is_return =
            filled("returnAgainst") || matches!(doc.get("isReturn"), Some(DocValue::Bool(true)));
        // `getMakeReturnDocAction`: returns are turned on in the settings.
        let returns_on = {
            let s = self.session.borrow();
            single_is_truthy(&s.fyo, "AccountingSettings", "enableInvoiceReturns")
                || single_is_truthy(&s.fyo, "InventorySettings", "enableStockReturns")
        };
        let create = |label: &str, run: ActionFn| Action::new(CREATE, label, run);

        if is_invoice && submitted {
            // `getMakePaymentAction`.
            let outstanding =
                matches!(doc.get("outstandingAmount"), Some(DocValue::Money(m)) if !m.is_zero());
            if outstanding {
                actions.push(create(
                    "Payment",
                    Rc::new(|this, _, cx| this.open_made(invoice::get_payment, cx)),
                ));
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
                actions.push(create(
                    label,
                    Rc::new(|this, _, cx| {
                        this.open_made(|doc, fyo| invoice::get_stock_transfer(doc, fyo, false), cx)
                    }),
                ));
            }
            if returns_on && !is_return {
                actions.push(create(
                    "Return",
                    Rc::new(|this, _, cx| this.open_made(invoice::get_return_doc, cx)),
                ));
            }
        }
        if is_transfer && submitted {
            // `getMakeInvoiceAction`.
            if !filled("backReference") {
                let label = if schema == "Shipment" {
                    "Sales Invoice"
                } else {
                    "Purchase Invoice"
                };
                actions.push(create(
                    label,
                    Rc::new(|this, _, cx| this.open_made(stock_transfer::get_invoice, cx)),
                ));
            }
            if returns_on && !is_return {
                actions.push(create(
                    "Return",
                    Rc::new(|this, _, cx| this.open_made(stock_transfer::get_return_doc, cx)),
                ));
            }
        }
        // `getLeadActions`.
        if schema == "Lead" && !doc.not_inserted() {
            actions.push(create(
                "Customer",
                Rc::new(|this, _, cx| {
                    this.open_made(|doc, fyo| lead::create_customer(doc, fyo).map(Some), cx)
                }),
            ));
            actions.push(create(
                "Sales Quote",
                Rc::new(|this, _, cx| {
                    this.open_made(|doc, fyo| lead::create_sales_quote(doc, fyo).map(Some), cx)
                }),
            ));
        }
        // `getQuoteActions`.
        if schema == "SalesQuote" && submitted {
            actions.push(create(
                "Sales Invoice",
                Rc::new(|this, _, cx| this.open_made_mut(sales_quote::get_invoice, cx)),
            ));
        }
        // `Party.getActions`: a new invoice for the party.
        if schema == "Party" && !doc.not_inserted() {
            let role = match doc.get("role") {
                Some(DocValue::Text(role)) => role.clone(),
                _ => String::new(),
            };
            if role != "Customer" {
                actions.push(Action::new(
                    "",
                    "Create Purchase",
                    Rc::new(|this, _, cx| this.open_party_invoice("PurchaseInvoice", cx)),
                ));
            }
            if role != "Supplier" {
                actions.push(Action::new(
                    "",
                    "Create Sale",
                    Rc::new(|this, _, cx| this.open_party_invoice("SalesInvoice", cx)),
                ));
            }
        }
        // `Item.getActions`: a new invoice with the item.
        if schema == "Item" && !doc.not_inserted() {
            let r#for = match doc.get("for") {
                Some(DocValue::Text(f)) => f.clone(),
                _ => String::new(),
            };
            if r#for != "Purchases" {
                actions.push(create(
                    "Sales Invoice",
                    Rc::new(|this, _, cx| this.open_item_invoice("SalesInvoice", cx)),
                ));
            }
            if r#for != "Sales" {
                actions.push(create(
                    "Purchase Invoice",
                    Rc::new(|this, _, cx| this.open_item_invoice("PurchaseInvoice", cx)),
                ));
            }
        }
        // `getDuplicateAction`.
        let submittable = doc.schema().is_submittable;
        if !doc.schema().is_single && (!submittable || doc.is_submitted()) && !doc.not_inserted() {
            actions.push(create(
                "Duplicate",
                Rc::new(|this, _, cx| this.open_made(|doc, fyo| doc.duplicate(fyo).map(Some), cx)),
            ));
        }
        // `getNewAction`.
        if !doc.schema().is_single {
            actions.push(create(
                "New Entry",
                Rc::new(|this, _, cx| {
                    let schema = this.doc.schema_name().to_owned();
                    this.open_made(
                        move |_, fyo| fyo.get_new_doc(&schema, Default::default()).map(Some),
                        cx,
                    )
                }),
            ));
        }
        // `getDeleteAction`, in red.
        if doc.inserted() && doc.can_delete() && !doc.schema().is_single {
            let mut delete = Action::new(
                "",
                "Delete",
                Rc::new(|this, _, cx| {
                    this.pending = Some(Pending::Delete);
                    cx.notify();
                }),
            );
            delete.danger = true;
            actions.push(delete);
        }
        // `getCancelAction`.
        if doc.can_cancel() {
            actions.push(Action::new(
                "",
                "Cancel",
                Rc::new(|this, _, cx| {
                    this.pending = Some(Pending::Cancel);
                    cx.notify();
                }),
            ));
        }
        actions
    }

    /// `Party.getActions` "Create Sale" and "Create Purchase".
    fn open_party_invoice(&mut self, schema: &'static str, cx: &mut Context<Self>) {
        self.open_made(
            move |party, fyo| {
                let mut values = latte_db::DocValueMap::new();
                if let Some(name) = party.name() {
                    values.insert("party".into(), DocValue::Text(name.to_owned()));
                }
                if let Some(DocValue::Text(account)) = party.get("defaultAccount") {
                    values.insert("account".into(), DocValue::Text(account.clone()));
                }
                fyo.get_new_doc(schema, values).map(Some)
            },
            cx,
        );
    }

    /// `Item.getActions`: a new invoice with a row for the item, at its rate and tax.
    fn open_item_invoice(&mut self, schema: &'static str, cx: &mut Context<Self>) {
        self.open_made_mut(
            move |item, fyo| {
                let mut invoice = fyo.get_new_doc(schema, Default::default())?;
                let mut row = latte_db::DocValueMap::new();
                for (to, from) in [("item", "name"), ("rate", "rate"), ("tax", "tax")] {
                    if let Some(value) = item.get(from) {
                        row.insert(to.into(), value.clone());
                    }
                }
                invoice.append(fyo, "items", row)?;
                Ok(Some(invoice))
            },
            cx,
        );
    }

    /// `getGroupedActionsForDoc`: the named groups in order, then the ungrouped ("…").
    fn grouped_actions(&self) -> Vec<(&'static str, Vec<Action>)> {
        let mut groups: Vec<(&'static str, Vec<Action>)> = Vec::new();
        for action in self.actions() {
            match groups.iter_mut().find(|(g, _)| *g == action.group) {
                Some((_, list)) => list.push(action),
                None => groups.push((action.group, vec![action])),
            }
        }
        groups.sort_by_key(|(g, _)| (g.is_empty(), *g));
        groups
    }

    /// The header's buttons (`CommonForm.vue`'s `#header`).
    fn header_buttons(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let doc = &self.doc;
        let mut buttons: Vec<AnyElement> = Vec::new();
        let schema = doc.schema_name().to_owned();
        // `canPrint`.
        if doc.inserted() && !doc.is_cancelled() && !doc.dirty() && crate::print::can_print(&schema)
        {
            let name = doc.name().unwrap_or_default().to_owned();
            buttons.push(
                Button::new("print")
                    .icon("printer")
                    .tooltip(t("Open Print View"))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(Navigate(Nav::Route(Route::Print {
                            schema: schema.clone(),
                            name: name.clone(),
                        })))
                    }))
                    .into_any_element(),
            );
        }
        buttons.push(
            Button::new("toggle-width")
                .icon(if self.full_width {
                    "minimize"
                } else {
                    "maximize"
                })
                .tooltip(t("Toggle between form and full width"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.full_width = !this.full_width;
                    cx.notify();
                }))
                .into_any_element(),
        );
        for (group, actions) in self.grouped_actions() {
            buttons.push(
                self.render_menu(group, actions, window, cx)
                    .into_any_element(),
            );
        }
        if doc.can_save() || doc.not_inserted() || doc.schema().is_single && doc.dirty() {
            buttons.push(
                Button::new("save")
                    .primary()
                    .label(t("Save"))
                    .on_click(cx.listener(|this, _, window, cx| this.save(window, cx)))
                    .into_any_element(),
            );
        } else if doc.can_submit() {
            buttons.push(
                Button::new("submit")
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

    /// `DropdownWithActions.vue`: a group's button, and its menu below it when open.
    fn render_menu(
        &self,
        group: &'static str,
        actions: Vec<Action>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let open = self.menu == Some(group);
        let id = SharedString::from(format!("menu-{group}"));
        let button = Button::new(id.clone())
            .map(|b| {
                if group.is_empty() {
                    b.icon("more-horizontal")
                } else {
                    b.label(t(group))
                }
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.menu = if this.menu == Some(group) {
                    None
                } else {
                    Some(group)
                };
                cx.notify();
            }));
        let menu = open.then(|| {
            deferred(
                anchored().anchor(Anchor::TopRight).snap_to_window().child(
                    popover_panel()
                        .id(SharedString::from(format!("{id}-panel")))
                        .occlude()
                        .mt_2()
                        .min_w(px(160.))
                        .p_1()
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.menu = None;
                            cx.notify();
                        }))
                        .children(actions.into_iter().enumerate().map(|(i, action)| {
                            let run = action.run.clone();
                            let label = div()
                                .when(action.danger, |d| d.text_color(books::color("red", 700)))
                                .child(t(&action.label));
                            dropdown_item(("action", i), label)
                                .when(i > 0, |d| d.mt_1())
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.menu = None;
                                    run(this, window, cx);
                                    cx.notify();
                                }))
                        })),
                ),
            )
            .with_priority(1)
        });
        div().relative().child(button).children(menu)
    }

    /// The dialog Submit, Cancel and Delete ask in (`showSubmitOrSyncDialog`,
    /// `cancelDocWithPrompt`, `deleteDocWithPrompt`), or an action's error.
    fn render_dialog(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if let Some(error) = &self.error {
            let close = cx.listener(|this, _, _, cx| {
                this.error = None;
                cx.notify();
            });
            return Some(
                dialog(
                    t(self.error_title),
                    Some(error.clone().into()),
                    DialogKind::Error,
                    vec![DialogButton::new(t("Okay"), false, close)],
                    window,
                )
                .into_any_element(),
            );
        }
        let pending = self.pending?;
        let label = self.reference_label();
        let (title, detail) = match pending {
            Pending::Submit => (
                format!("Submit {label}?"),
                format!("Mark {} as submitted?", self.doc.schema().label),
            ),
            Pending::Cancel => (
                format!("Cancel {label}?"),
                "This action is permanent".to_owned(),
            ),
            Pending::Delete => (
                format!("Delete {label}?"),
                if self.doc.is_submitted() && self.doc.schema().is_submittable {
                    "This action is permanent and will delete associated ledger entries."
                } else {
                    "This action is permanent."
                }
                .to_owned(),
            ),
        };
        let kind = if pending == Pending::Submit {
            DialogKind::Info
        } else {
            DialogKind::Warning
        };
        let yes = cx.listener(move |this, _, window, cx| this.confirm(pending, window, cx));
        let no = cx.listener(|this, _, _, cx| {
            this.pending = None;
            cx.notify();
        });
        Some(
            dialog(
                title,
                Some(detail.into()),
                kind,
                vec![
                    DialogButton::new(t("Yes"), true, yes),
                    DialogButton::new(t("No"), false, no),
                ],
                window,
            )
            .into_any_element(),
        )
    }

    /// A control in Books' look (`Base.vue` and the controls that extend it): bordered in
    /// the form, bare in a table.
    fn render_control(
        &self,
        key: &Key,
        f: &Field,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let in_table = matches!(key, Key::Cell(..));
        let numeric = is_numeric(f);
        let s = self.session.borrow();
        match self.controls.get(key) {
            Some(Control::Input { state, .. }) => {
                let focused = state.focus_handle(cx).is_focused(window);
                let value = self.current_value(key);
                // Currency, numbers and dates show their formatted value until clicked
                // (`Currency.vue`, `Date.vue`).
                let formatted = (!focused && shows_formatted(f))
                    .then(|| field::display(&s.formatter, f, value));
                let is_date = matches!(f.fieldtype, Some(FieldType::Date | FieldType::Datetime));
                let input = Input::new(state)
                    .appearance(false)
                    .h(px(32.))
                    .text_13()
                    .map(|i| if in_table { i.px_2() } else { i.px_3() })
                    .when(numeric, |i| i.text_right())
                    .when(formatted.is_some(), |i| i.text_color(hsla(0., 0., 0., 0.)));
                let frame = if in_table {
                    div()
                        .h_flex()
                        .w_full()
                        .rounded(books::RADIUS)
                        .text_13()
                        .text_color(gray(900))
                        .when(focused, |d| d.bg(gray(100)))
                } else {
                    control_frame(false, focused)
                };
                let overlay = formatted.map(|text| {
                    let state = state.clone();
                    let empty = text.is_empty();
                    let placeholder = f
                        .placeholder
                        .clone()
                        .or_else(|| f.label.clone())
                        .unwrap_or_default();
                    div()
                        .id(SharedString::from(format!("{key:?}-shown")))
                        .absolute()
                        .inset_0()
                        .h_flex()
                        .gap_1()
                        .map(|d| if in_table { d.px_2() } else { d.px_3() })
                        .rounded(books::RADIUS)
                        .when(numeric, |d| d.justify_end())
                        .cursor_text()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .when(numeric, |d| d.h_flex().justify_end())
                                .when(empty, |d| d.text_color(gray(500)))
                                .child(if empty && !numeric { placeholder } else { text }),
                        )
                        .when(is_date, |d| {
                            d.child(books::icon("calendar", px(16.), gray(600)))
                        })
                        .on_click(move |_, window, cx| state.focus_handle(cx).focus(window, cx))
                });
                frame
                    .relative()
                    .child(div().flex_1().min_w_0().child(input))
                    .children(overlay)
                    .into_any_element()
            }
            Some(Control::Text { state, .. }) => {
                let focused = state.focus_handle(cx).is_focused(window);
                // Three rows of 13px text at Books' line height, and `py-2`.
                control_frame(false, focused)
                    .items_start()
                    .child(
                        Textarea::new(state)
                            .appearance(false)
                            .h(px(3. * 19.5 + 16.))
                            .px_1()
                            .py_0()
                            .text_13(),
                    )
                    .into_any_element()
            }
            Some(Control::Link { state, .. }) => state.clone().into_any_element(),
            Some(Control::Check) => {
                let checked = latte_core::fyo::model::helpers::is_truthy(self.current_value(key));
                let key2 = key.clone();
                let id: SharedString = format!("{key:?}").into();
                let label = match key {
                    Key::Field(_) => f.label.clone(),
                    Key::Cell(..) => None,
                };
                let read_only = {
                    let ctx = match key {
                        Key::Field(_) => field::context(&s.fyo, &self.doc, None),
                        Key::Cell(table, i, _) => match self.doc.table(table).get(*i) {
                            Some(row) => field::context(&s.fyo, row, Some(&self.doc)),
                            None => field::context(&s.fyo, &self.doc, None),
                        },
                    };
                    field::is_read_only(f, &ctx)
                };
                // `Check.vue`: the box, then the label, in a bordered frame in the form.
                div()
                    .id(ElementId::Name(id))
                    .h_flex()
                    .w_full()
                    .when(!in_table, |d| {
                        d.min_h(px(34.))
                            .px_3()
                            .rounded(books::RADIUS)
                            .border_1()
                            .border_color(gray(200))
                            .bg(if read_only { gray(50) } else { gray(25) })
                    })
                    .when(in_table, |d| d.px_2().h(px(32.)))
                    .when(!read_only, |d| d.cursor_pointer())
                    .child(check_box(checked))
                    .children(label.map(|l| div().ml_3().text_13().text_color(gray(600)).child(l)))
                    .when(!read_only, |d| {
                        d.on_click(cx.listener(move |this, _, window, cx| {
                            this.set_value(&key2, DocValue::Bool(!checked), window, cx);
                        }))
                    })
                    .into_any_element()
            }
            Some(Control::Image) => {
                let url = match self.current_value(key) {
                    Some(DocValue::Text(url)) if !url.is_empty() => Some(url.clone()),
                    _ => None,
                };
                let read_only = {
                    let ctx = field::context(&s.fyo, &self.doc, None);
                    field::is_read_only(f, &ctx)
                };
                let image = url.as_ref().and_then(|u| self.images.get(u).cloned());
                let group = SharedString::from(format!("{key:?}-image"));
                let upload_key = key.clone();
                let clear_key = key.clone();
                let has_value = url.is_some();
                div()
                    .id(group.clone())
                    .group(group.clone())
                    .relative()
                    .size(px(135.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .overflow_hidden()
                    .bg(white())
                    .border_1()
                    .border_color(gray(200))
                    .rounded(books::RADIUS)
                    .map(|d| match image {
                        Some(image) => {
                            d.child(img(image).size_full().object_fit(ObjectFit::Contain))
                        }
                        None => d.child(
                            svg()
                                .path("books/camera.svg")
                                .w_6()
                                .h(px(21.))
                                .text_color(gray(300)),
                        ),
                    })
                    .when(!read_only, |d| {
                        d.child(
                            div()
                                .absolute()
                                .inset_0()
                                .invisible()
                                .group_hover(group.clone(), |d| d.visible())
                                .flex()
                                .items_end()
                                .justify_center()
                                .bg(hsla(0., 0., 0., 0.2))
                                .child(
                                    div()
                                        .id(SharedString::from(format!("{key:?}-image-button")))
                                        .mb_1()
                                        .p(px(2.))
                                        .rounded(books::RADIUS)
                                        .bg(gray(100))
                                        .cursor_pointer()
                                        .child(books::icon(
                                            if has_value { "x" } else { "upload" },
                                            px(16.),
                                            gray(600),
                                        ))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            if has_value {
                                                this.set_value(
                                                    &clear_key,
                                                    DocValue::Null,
                                                    window,
                                                    cx,
                                                )
                                            } else {
                                                this.pick_image(upload_key.clone(), window, cx)
                                            }
                                        })),
                                ),
                        )
                    })
                    .into_any_element()
            }
            Some(Control::Display) | None => {
                let text = field::display(&s.formatter, f, self.current_value(key));
                let link = (f.fieldtype == Some(FieldType::Link) && !text.is_empty()).then(|| {
                    let field = f.clone();
                    let name = match self.current_value(key) {
                        Some(DocValue::Text(name)) => name.clone(),
                        _ => text.clone(),
                    };
                    div()
                        .id(SharedString::from(format!("{key:?}-open")))
                        .p(px(2.))
                        .cursor_pointer()
                        .child(books::icon("chevron-right", px(16.), gray(600)))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.open_link(&field, &name, cx)),
                        )
                });
                let frame = if in_table {
                    div()
                        .h_flex()
                        .w_full()
                        .h(px(32.))
                        .px_2()
                        .text_13()
                        .text_color(gray(800))
                } else {
                    control_frame(true, false).px_3()
                };
                // Books' read-only inputs still show their placeholder when empty.
                let placeholder = (text.is_empty() && !in_table).then(|| {
                    f.placeholder
                        .clone()
                        .or_else(|| f.label.clone())
                        .unwrap_or_default()
                });
                frame
                    .when(link.is_some(), |d| d.pr_2())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .when(numeric, |d| d.h_flex().justify_end())
                            .map(|d| match placeholder {
                                Some(placeholder) => d.text_color(gray(500)).child(placeholder),
                                None => d.child(text),
                            }),
                    )
                    .children(link)
                    .into_any_element()
            }
        }
    }

    /// `Table.vue` and `TableRow.vue`: the label, then a bordered table whose rows hold
    /// bare controls, the index turning into × on hover, and Add Row at the end.
    fn render_table(&self, f: &Field, window: &Window, cx: &mut Context<Self>) -> Div {
        let columns = {
            let s = self.session.borrow();
            field::table_columns(&s.fyo, f)
        };
        let editable = self.doc.can_edit();
        let table = f.fieldname.clone();
        let header = div()
            .h_flex()
            .w_full()
            .h_12()
            .px_2()
            .gap_0()
            .border_b_1()
            .border_color(gray(200))
            .text_color(gray(600))
            .child(index_cell().pl_2().child("#"))
            .children(columns.iter().map(|c| {
                cell(c)
                    .px_2()
                    .when(is_numeric(c), |d| d.justify_end())
                    .child(
                        div()
                            .truncate()
                            .child(c.label.clone().unwrap_or_else(|| c.fieldname.clone())),
                    )
            }));
        let count = self.doc.table(&f.fieldname).len();
        let rows = (0..count)
            .map(|i| {
                let table = table.clone();
                let group = SharedString::from(format!("{table}-row-{i}"));
                let error = columns.iter().find_map(|c| {
                    self.field_errors
                        .get(&Key::Cell(f.fieldname.clone(), i, c.fieldname.clone()))
                });
                div()
                    .id(SharedString::from(format!("{table}-row-{i}")))
                    .group(group.clone())
                    .v_flex()
                    .w_full()
                    .px_2()
                    .when(i + 1 < count, |d| d.border_b_1().border_color(gray(200)))
                    .when(editable, |d| d.hover(|d| d.bg(gray(25))))
                    .child(
                        div()
                            .h_flex()
                            .w_full()
                            .h_12()
                            .child(
                                index_cell()
                                    .pl_2()
                                    .text_color(gray(600))
                                    .when(editable, |d| {
                                        d.child(
                                            div()
                                                .id(("remove-row", i))
                                                .relative()
                                                .size_4()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .cursor_pointer()
                                                .child(
                                                    div()
                                                        .group_hover(group.clone(), |d| {
                                                            d.invisible()
                                                        })
                                                        .child(format!("{}", i + 1)),
                                                )
                                                .child(
                                                    div()
                                                        .absolute()
                                                        .inset_0()
                                                        .invisible()
                                                        .group_hover(group.clone(), |d| d.visible())
                                                        .child(books::icon(
                                                            "x",
                                                            px(16.),
                                                            gray(600),
                                                        )),
                                                )
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.remove_row(&table, i, window, cx)
                                                    },
                                                )),
                                        )
                                    })
                                    .when(!editable, |d| d.child(format!("{}", i + 1))),
                            )
                            .children(columns.iter().map(|c| {
                                let key = Key::Cell(f.fieldname.clone(), i, c.fieldname.clone());
                                cell(c).child(self.render_control(&key, c, window, cx))
                            })),
                    )
                    .children(error.map(|e| {
                        div()
                            .pl_2()
                            .pb_1()
                            .text_11()
                            .text_color(books::color("red", 600))
                            .child(e.clone())
                    }))
            })
            .collect::<Vec<_>>();
        let add_table = f.fieldname.clone();
        div()
            .v_flex()
            .w_full()
            .text_13()
            .child(field_label(f.label.clone().unwrap_or_default(), false, cx))
            .child(
                div()
                    .v_flex()
                    .w_full()
                    .border_1()
                    .border_color(gray(200))
                    .rounded(books::RADIUS_MD)
                    .child(header)
                    .children(rows)
                    .when(editable, |d| {
                        d.child(
                            div()
                                .id(SharedString::from(format!("add-{add_table}")))
                                .h_flex()
                                .w_full()
                                .h_12()
                                .px_2()
                                .when(count > 0, |d| d.border_t_1().border_color(gray(200)))
                                .text_color(gray(500))
                                .cursor_pointer()
                                .child(index_cell().pl_1().child(books::icon(
                                    "plus",
                                    px(16.),
                                    gray(500),
                                )))
                                .child(
                                    div()
                                        .flex_grow(columns.len() as f32)
                                        .flex_basis(px(0.))
                                        .px_2()
                                        .child(t("Add Row")),
                                )
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.add_row(&add_table, window, cx)
                                })),
                        )
                    }),
            )
    }

    /// `CommonFormSection.vue`: a section's title, then its fields two to a row, 16px
    /// apart and 32px between the columns.
    fn render_section(
        &self,
        section: &field::Section,
        show_title: bool,
        first: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if section.fields.is_empty() {
            return None;
        }
        let (required, read_only): (Vec<bool>, Vec<bool>) = {
            let s = self.session.borrow();
            let ctx = field::context(&s.fyo, &self.doc, None);
            section
                .fields
                .iter()
                .map(|f| (field::is_required(f, &ctx), field::is_read_only(f, &ctx)))
                .unzip()
        };
        let items = section.fields.iter().enumerate().map(|(i, f)| {
            let key = Key::Field(f.fieldname.clone());
            let wide = f.fieldtype == Some(FieldType::Table) || f.fieldname == "termsAndConditions";
            let body = if f.fieldtype == Some(FieldType::Table) {
                self.render_table(f, window, cx).into_any_element()
            } else if matches!(f.fieldtype, Some(FieldType::Check | FieldType::AttachImage)) {
                self.render_control(&key, f, window, cx)
            } else {
                let empty = self.current_value(&key).is_none_or(|v| {
                    matches!(v, DocValue::Null) || matches!(v, DocValue::Text(t) if t.is_empty())
                });
                div()
                    .v_flex()
                    .child(field_label(
                        f.label.clone().unwrap_or_else(|| f.fieldname.clone()),
                        required[i],
                        cx,
                    ))
                    .child(
                        div()
                            .relative()
                            .child(self.render_control(&key, f, window, cx))
                            .when(
                                required[i] && empty && !read_only[i] && !is_choice(f),
                                |d| d.child(mandatory_mark()),
                            ),
                    )
                    .into_any_element()
            };
            div()
                .when(wide, |d| d.col_span_full())
                .when(f.fieldtype == Some(FieldType::AttachImage), |d| {
                    d.row_span(2)
                })
                .when(f.fieldtype == Some(FieldType::Check), |d| d.mt_auto())
                .when(f.fieldtype != Some(FieldType::Check), |d| d.mb_auto())
                .child(body)
                .children(self.field_errors.get(&key).map(|e| {
                    div()
                        .mt_1()
                        .text_12()
                        .text_color(books::color("red", 600))
                        .child(e.clone())
                }))
        });
        Some(
            div()
                .v_flex()
                .p_4()
                .when(!first, |d| d.border_t_1().border_color(gray(200)))
                .when(show_title, |d| {
                    d.child(
                        div()
                            .mb_4()
                            .text_13()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(gray(900))
                            .child(section.name.clone()),
                    )
                })
                .child(
                    div()
                        .grid()
                        .grid_cols(2)
                        .gap_4()
                        .gap_x_8()
                        .w_full()
                        .children(items),
                )
                .into_any_element(),
        )
    }
}

/// A form action: its group ("Create", "View" or none), label and what it does.
type ActionFn = Rc<dyn Fn(&mut FormPage, &mut Window, &mut Context<FormPage>)>;

const CREATE: &str = "Create";

struct Action {
    group: &'static str,
    label: String,
    danger: bool,
    run: ActionFn,
}

impl Action {
    fn new(group: &'static str, label: &str, run: ActionFn) -> Self {
        Self {
            group,
            label: label.to_owned(),
            danger: false,
            run,
        }
    }
}

/// A picture stored as `data:image/png;base64,…`.
fn image_from_data_url(url: &str) -> Option<Image> {
    use base64::Engine as _;
    let (mime, data) = url.strip_prefix("data:")?.split_once(";base64,")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .ok()?;
    Some(Image::from_bytes(ImageFormat::from_mime_type(mime)?, bytes))
}

/// A picture file as a data URL, as Books stores it.
fn data_url(path: &std::path::Path) -> Result<String, String> {
    use base64::Engine as _;
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let mime = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        _ => return Err(format!("{} isn't a picture Latte can show", path.display())),
    };
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

/// `getErrorLabel`: a dialog's title for an error.
fn error_label(error: &latte_core::Error) -> &'static str {
    match error {
        latte_core::Error::Validation(_) => "Validation Error",
        latte_core::Error::Mandatory(_) => "Mandatory Error",
        latte_core::Error::NotFound(_) => "Not Found",
        latte_core::Error::Value(_) => "Value Error",
        latte_core::Error::Db(_) => "Database Error",
        _ => "Error",
    }
}

/// Fields shown formatted until clicked into.
fn shows_formatted(f: &Field) -> bool {
    matches!(
        f.fieldtype,
        Some(FieldType::Currency | FieldType::Float | FieldType::Date | FieldType::Datetime)
    )
}

#[derive(Clone, Copy)]
enum Kind {
    Input,
    Text,
    Link,
    Check,
    Display,
    Image,
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
        Some(FieldType::AttachImage) => Kind::Image,
        _ if read_only => Kind::Display,
        Some(FieldType::Attachment | FieldType::Button) => Kind::Display,
        _ if is_choice(f) => Kind::Link,
        _ => Kind::Input,
    }
}

/// A column of `Table.vue`'s `Row`: each `1fr`, numbers to the right.
fn cell(f: &Field) -> Div {
    div()
        .flex_1()
        .flex_basis(px(0.))
        .min_w_0()
        .h_flex()
        .when(is_numeric(f), |d| d.justify_end())
}

/// The index column: `0.3fr`.
fn index_cell() -> Div {
    div()
        .flex_grow(0.3)
        .flex_shrink(1.)
        .flex_basis(px(0.))
        .min_w_0()
        .h_flex()
}

impl Render for FormPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs = self.tabs.clone();
        let sections = tabs
            .get(self.tab)
            .map(|t| t.sections.clone())
            .unwrap_or_default();
        let many = sections.iter().filter(|s| !s.fields.is_empty()).count() > 1;
        let mut first = true;
        let sections: Vec<AnyElement> = sections
            .iter()
            .filter_map(|section| {
                let show_title = many && section.name != "Default";
                let element = self.render_section(section, show_title, first, window, cx)?;
                first = false;
                Some(element)
            })
            .collect();

        if self.embedded {
            return div()
                .v_flex()
                .w_full()
                .children(sections)
                .children(self.render_dialog(window, cx))
                .into_any_element();
        }

        // `CommonForm.vue`'s tab bar, at the foot of the form.
        let tab_bar = (tabs.len() > 1).then(|| {
            div()
                .h_flex()
                .flex_none()
                .mt_auto()
                .px_4()
                .pb_4()
                .gap_8()
                .border_t_1()
                .border_color(gray(200))
                .bg(white())
                .children(tabs.iter().enumerate().map(|(i, tab)| {
                    let active = i == self.tab;
                    div()
                        .id(("tab", i))
                        .text_12()
                        .cursor_pointer()
                        .when(active, |d| {
                            d.pt(px(14.))
                                .border_t_2()
                                .border_color(gray(800))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(gray(900))
                        })
                        .when(!active, |d| d.pt_4().text_color(gray(700)))
                        .child(tab.name.clone())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.tab = i;
                            this.refresh(window, cx);
                        }))
                }))
        });

        // `FormHeader.vue`: the title and status, 56px high.
        let title = if self.doc.schema().is_single {
            self.doc.schema().label.clone()
        } else if self.doc.schema().is_submittable && self.doc.not_inserted() {
            t("New Entry")
        } else {
            match self.doc.name() {
                Some(name) if !name.is_empty() => name.to_owned(),
                _ => t("New Entry"),
            }
        };
        let form_header = div()
            .h_flex()
            .flex_none()
            .h(px(56.))
            .px_4()
            .justify_between()
            .border_b_1()
            .border_color(gray(200))
            .text_18()
            .font_weight(FontWeight::SEMIBOLD)
            .child(div().truncate().text_color(black()).child(title))
            .children(self.status_pill());

        let body = div()
            .v_flex()
            .h_full()
            .bg(white())
            .overflow_hidden()
            .map(|d| {
                if self.full_width {
                    d.w_full().border_t_1().border_color(gray(200))
                } else {
                    d.w(books::W_FORM)
                        .mx_4()
                        .mb_4()
                        .border_1()
                        .border_color(gray(200))
                        .rounded(books::RADIUS_LG)
                        .shadow(books::shadow_lg())
                }
            })
            .child(form_header)
            .child(
                div()
                    .id("form-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(div().v_flex().w_full().children(sections)),
            )
            .children(tab_bar);

        let buttons = self.header_buttons(window, cx);
        // `CommonForm.vue`'s `#header-left`: the schema's label beside Back and Forward.
        let label = div()
            .text_18()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(gray(600))
            .whitespace_nowrap()
            .child(t(&self.doc.schema().label));
        let header = page_header_with("", label, cx)
            .border_b_0()
            .child(div().h_flex().gap_2().children(buttons));

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
            .bg(gray(25))
            .text_13()
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .h_flex()
                    .justify_center()
                    .child(body),
            )
            .children(self.render_dialog(window, cx))
            .children(toast)
            .into_any_element()
    }
}
