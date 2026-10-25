//! A document's form, drawn from its schema: `src/pages/CommonForm/CommonForm.vue`,
//! `CommonFormSection.vue`, `FormHeader.vue`, `RowEditForm.vue`, `LinkedEntries.vue`,
//! `src/pages/QuickEditForm.vue` and the controls in `src/components/Controls` @ v0.36.0.
//!
//! Every change goes through `Doc::set`, so formulas, validations and the model's `change`
//! hook run as in Books, and the form then shows what the document holds.
//!
//! As in Books, the page header holds the actions (Linked Entries, Print, the width
//! toggle, the Create and View groups, the "…" menu, then Save or Submit), and, on the left,
//! the barcode scanner and the exchange rate of a document in another currency; the form
//! is a 600px card below it, or the full width. Beside it one panel at a time: a child
//! row's form, the linked entries, or a quick edit form (a document made from a Link
//! field's Create, or the payment Make Payment makes). Submit, Cancel and Delete ask first
//! in a dialog, and Ctrl+S asks before it saves; what worked is said in a toast; a value
//! that can't be set is shown under its field.
//!
//! The same page, made with [`FormPage::new_quick`], is the quick edit panel: its title
//! (and picture) on top, then the schema's quick edit fields, label beside control.
//!
//! Shortcuts, as `useDocShortcuts` and `CommonForm.vue` set them: Ctrl+S saves or submits,
//! Ctrl+Backspace cancels or deletes, Ctrl+P opens the print view, Ctrl+L the linked
//! entries, and Esc closes a dialog, a menu or the side panel.
//!
//! Differences from Books (each in docs/DEVIATIONS.md):
//! - Dates are typed as YYYY-MM-DD, with a calendar drawn like Chromium's
//!   (`date_picker.rs`) in place of the browser's date input.
//! - A Color control's custom colour is typed as `#RRGGBB`; Books opens the system's
//!   colour picker.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use crate::books::{self, Text as _, black, gray, white};
use crate::widgets::{
    Button, DialogButton, Kind as DialogKind, Toast, check_box, control_frame, dialog,
    dropdown_item, field_label, mandatory_mark, pill, popover_panel, show_toast,
};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::date_picker::{DatePicked, DatePicker, PickerClosed, anchored_picker};
use crate::i18n::t;
use latte_core::Doc;
use latte_core::models::base_models::{invoice, lead, sales_quote};
use latte_core::models::helpers::single_is_truthy;
use latte_core::models::inventory::stock_transfer;
use latte_core::views::field::{self, Tab};
use latte_core::views::form::{self as form_view, LinkedGroup};
use latte_core::views::list;
use latte_core::views::sidebar::Route;
use latte_db::DocValue;
use latte_schema::{Field, FieldType};

use crate::link::{
    LinkCreate, LinkDisableFilters, LinkInput, LinkOpened, LinkOption, LinkPicked, QuickViewData,
};
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
        _subscriptions: [Subscription; 4],
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
    /// `Attachment.vue`: a file kept in the field as `{name, type, data}`.
    Attachment,
    /// `Color.vue`: a swatch and the colour's name, which opens the swatches.
    Color,
}

/// An action waiting for the user to confirm it, as Books' dialogs ask.
#[derive(Clone, PartialEq)]
enum Pending {
    /// Ctrl+S's "Save X?" (`showSubmitOrSyncDialog(doc, 'sync')`).
    Sync,
    Submit,
    /// `showInsufficientInventoryDialog`: the items short, then Submit is asked.
    Insufficient(Vec<(String, f64)>),
    Cancel,
    Delete,
}

/// The side panel beside the form; only one shows at a time.
enum Side {
    None,
    /// `RowEditForm.vue`: a child row's form, by its table and index.
    Row(String, usize),
    /// `LinkedEntries.vue`: the linked documents, and the schemas folded.
    Links(Vec<LinkedGroup>, HashSet<String>),
}

/// What a quick edit panel tells the page that opened it.
pub enum QuickEvent {
    /// Closed with its ×, Esc, or after a delete.
    Close,
    /// Saved (`afterSync`).
    Saved,
    /// A Link field of the panel made a new document to edit in a panel of its own.
    Open(Box<QuickRequest>),
}

impl EventEmitter<QuickEvent> for FormPage {}

/// What the page does once a quick edit panel's document is saved: the panel is passed.
pub type AfterSave = Rc<dyn Fn(Entity<FormPage>, &mut Window, &mut App)>;

/// A document to edit in a quick edit panel (`openQuickEdit`): its fields to hide, and
/// what follows its save. Without `after_save`, the panel stays open.
#[derive(Clone)]
pub struct QuickRequest {
    pub doc: Doc,
    pub hide: Vec<String>,
    pub after_save: Option<AfterSave>,
}

/// The quick edit panels a page shows, the last on top (Books' `edit` route query).
#[derive(Default)]
pub struct QuickPanels {
    panels: Vec<(Entity<FormPage>, Option<AfterSave>, [Subscription; 2])>,
}

impl QuickPanels {
    pub fn top(&self) -> Option<&Entity<FormPage>> {
        self.panels.last().map(|(form, ..)| form)
    }

    pub fn is_empty(&self) -> bool {
        self.panels.is_empty()
    }

    /// Whether the top panel edits this document.
    pub fn shows(&self, schema: &str, name: &str, cx: &App) -> bool {
        self.top().is_some_and(|form| {
            let doc = form.read(cx).doc();
            doc.schema_name() == schema && doc.name() == Some(name)
        })
    }

    pub fn clear(&mut self) {
        self.panels.clear();
    }

    fn remove(&mut self, form: &Entity<FormPage>) {
        self.panels.retain(|(f, ..)| f != form);
    }

    fn after_save(&self, form: &Entity<FormPage>) -> Option<AfterSave> {
        self.panels
            .iter()
            .find(|(f, ..)| f == form)
            .and_then(|(_, after, _)| after.clone())
    }
}

/// A page that shows quick edit panels: the form, the chart of accounts.
pub trait QuickHost: EventEmitter<Navigate> + Sized + 'static {
    fn quick_panels(&mut self) -> &mut QuickPanels;
    /// A panel opened, or saved, closed or deleted its document: show what changed.
    fn quick_changed(&mut self, window: &mut Window, cx: &mut Context<Self>);
}

/// `openQuickEdit`: shows `request` in a quick edit panel on top of the host's.
pub fn open_quick<H: QuickHost>(
    host: &mut H,
    session: Shared,
    request: QuickRequest,
    window: &mut Window,
    cx: &mut Context<H>,
) {
    // The same document open already: nothing to do (`openQuickEdit`'s name check).
    if let Some(name) = request.doc.name()
        && host
            .quick_panels()
            .shows(request.doc.schema_name(), name, cx)
    {
        return;
    }
    let QuickRequest {
        doc,
        hide,
        after_save,
    } = request;
    let form = cx.new(|cx| FormPage::new_quick(session.clone(), doc, hide, window, cx));
    let subscription = cx.subscribe_in(
        &form,
        window,
        move |host: &mut H, form, event: &QuickEvent, window, cx| match event {
            QuickEvent::Close => {
                host.quick_panels().remove(form);
                host.quick_changed(window, cx);
            }
            QuickEvent::Saved => {
                if let Some(after) = host.quick_panels().after_save(form) {
                    host.quick_panels().remove(form);
                    // Once the host is no longer being updated: `after` may update it.
                    let form = form.clone();
                    window.defer(cx, move |window, cx| after(form, window, cx));
                }
                host.quick_changed(window, cx);
            }
            QuickEvent::Open(request) => {
                open_quick(host, session.clone(), (**request).clone(), window, cx);
            }
        },
    );
    // What the panel opens (a linked document) opens in place of the host's page.
    let navigate = cx.subscribe(&form, |_: &mut H, _, event: &Navigate, cx| {
        cx.emit(Navigate(event.0.clone()))
    });
    host.quick_panels()
        .panels
        .push((form.clone(), after_save, [subscription, navigate]));
    form.update(cx, |form, cx| form.focus_first(window, cx));
    host.quick_changed(window, cx);
}

/// The exchange rate control's state (`ExchangeRate.vue`): the left amount, the right
/// one's input, and whether the company's currency is on the left.
struct ExchangeRate {
    from_value: Entity<InputState>,
    right: Entity<InputState>,
    swapped: bool,
    _subscriptions: [Subscription; 2],
}

/// A control the form needs: its key, field and value, whether read-only and required,
/// and a Link's options, whether it may bypass its filters and its options' groups.
struct Wanted {
    key: Key,
    field: Field,
    value: Option<DocValue>,
    read_only: bool,
    options: Vec<LinkOption>,
    required: bool,
    bypass: bool,
    groups: HashMap<String, String>,
}

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
    pending: Option<Pending>,
    /// The action group whose menu is open.
    menu: Option<&'static str>,
    /// "Toggle between form and full width".
    full_width: bool,
    side: Side,
    /// The quick edit panels this form opened (a Link's Create, Make Payment).
    quick_panels: QuickPanels,
    /// In a quick edit panel: the fields hidden (`hideFields`).
    quick: Option<Vec<String>>,
    /// Link fields whose filters were turned off ("No results found, disable filters").
    unfiltered: HashSet<Key>,
    /// The calendar open under a Date or Datetime control.
    picker: Option<(Key, Entity<DatePicker>, [Subscription; 2])>,
    /// The Color control whose swatches are open, and its Custom Hex input.
    color: Option<(Key, Entity<InputState>, Subscription)>,
    exchange: Option<ExchangeRate>,
    /// `Barcode.vue`'s input.
    barcode: Option<(Entity<InputState>, Subscription)>,
    /// The pictures of the AttachImage fields, by their data URL.
    images: HashMap<String, Arc<Image>>,
    /// Shown inside another page (Settings), without its own header.
    embedded: bool,
    scroll: ScrollHandle,
    /// Focused when the form opens, so its shortcuts work before anything is clicked.
    focus: FocusHandle,
    focus_on_open: bool,
    /// `scanListener`: the digits a scanner typed, and when the last came.
    scanned: (String, Option<std::time::Instant>),
    /// `cooldown`: the code just scanned, so one scan isn't added twice.
    cooldown: Option<(String, std::time::Instant)>,
}

impl EventEmitter<Navigate> for FormPage {}

impl QuickHost for FormPage {
    fn quick_panels(&mut self) -> &mut QuickPanels {
        &mut self.quick_panels
    }

    fn quick_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.quick_panels.is_empty() {
            // The panel had the focus: the form takes it back for its shortcuts.
            if self.quick.is_none() && !self.embedded {
                self.focus.focus(window, cx);
            }
        } else {
            self.side = Side::None;
        }
        self.refresh(window, cx);
    }
}

impl FormPage {
    pub fn new(session: Shared, doc: Doc, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut page = Self::blank(session, doc, cx);
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

    /// The quick edit panel for `doc` (`QuickEditForm.vue`), without the fields in `hide`.
    pub fn new_quick(
        session: Shared,
        doc: Doc,
        hide: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut page = Self::blank(session, doc, cx);
        page.quick = Some(hide);
        page.refresh(window, cx);
        page
    }

    fn blank(session: Shared, doc: Doc, cx: &mut Context<Self>) -> Self {
        Self {
            session,
            doc,
            tabs: Vec::new(),
            tab: 0,
            controls: HashMap::new(),
            error: None,
            error_title: "Error",
            field_errors: HashMap::new(),
            pending: None,
            menu: None,
            full_width: false,
            side: Side::None,
            quick_panels: QuickPanels::default(),
            quick: None,
            unfiltered: HashSet::new(),
            picker: None,
            color: None,
            exchange: None,
            barcode: None,
            images: HashMap::new(),
            embedded: false,
            scroll: ScrollHandle::new(),
            focus: cx.focus_handle(),
            focus_on_open: true,
            scanned: (String::new(), None),
            cooldown: None,
        }
    }

    /// `focusOrSelectFormControl`: a quick edit panel focuses its title, else its first
    /// input.
    fn focus_first(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let title = self.title_field().map(|f| Key::Field(f.fieldname));
        let key = title.filter(|k| self.controls.contains_key(k)).or_else(|| {
            self.shown_fields()
                .into_iter()
                .map(|f| Key::Field(f.fieldname))
                .find(|k| matches!(self.controls.get(k), Some(Control::Input { .. })))
        });
        if let Some(Control::Input { state, .. }) = key.and_then(|k| self.controls.get(&k)) {
            state.update(cx, |input, cx| input.focus(window, cx));
        }
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

    /// `QuickEditForm.vue`'s title field (`titleField ?? 'name'`), in a quick edit panel.
    fn title_field(&self) -> Option<Field> {
        self.quick.as_ref()?;
        let schema = self.doc.schema();
        let name = schema
            .title_field
            .clone()
            .unwrap_or_else(|| "name".to_owned());
        schema.field(&name).cloned()
    }

    /// `QuickEditForm.vue`'s picture, beside the title.
    fn image_field(&self) -> Option<Field> {
        self.quick.as_ref()?;
        self.doc
            .schema()
            .field("image")
            .filter(|f| f.fieldtype == Some(FieldType::AttachImage))
            .cloned()
    }

    /// The fields shown now: the tab's, or the quick edit panel's.
    fn shown_fields(&self) -> Vec<Field> {
        match &self.quick {
            Some(hide) => {
                let s = self.session.borrow();
                form_view::quick_edit_fields(&s.fyo, &self.doc, hide)
            }
            None => self
                .tabs
                .get(self.tab)
                .map(|t| t.sections.iter().flat_map(|s| s.fields.clone()).collect())
                .unwrap_or_default(),
        }
    }

    /// Lays the form out again and shows what the document holds now. Inputs being typed
    /// into keep their text.
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        {
            let s = self.session.borrow();
            self.tabs = field::form_layout(&s.fyo, &self.doc);
        }
        if self.tab >= self.tabs.len() {
            self.tab = 0;
        }
        let mut fields = self.shown_fields();
        fields.extend(self.title_field());
        fields.extend(self.image_field());
        let title = self.title_field().map(|f| f.fieldname);

        let session = self.session.clone();
        let s = session.borrow();
        let fyo = &s.fyo;
        let bypass = single_is_truthy(fyo, "SystemSettings", "allowFilterBypass");

        // Every control the fields need, with its field and value.
        let mut wanted: Vec<Wanted> = Vec::new();
        let mut option_cache: HashMap<String, Vec<LinkOption>> = HashMap::new();
        let unfiltered = self.unfiltered.clone();
        let mut options =
            |key: &Key, f: &Field, ctx: &latte_core::DocContext<'_>| -> Vec<LinkOption> {
                match f.fieldtype {
                    Some(FieldType::Link | FieldType::DynamicLink) => {
                        let filtered = !unfiltered.contains(key);
                        let filters = field::link_filters(f, ctx);
                        let target = field::link_target(f, ctx).unwrap_or_default();
                        let cache_key = format!("{target}{filtered}{filters:?}");
                        option_cache
                            .entry(cache_key)
                            .or_insert_with(|| {
                                field::link_options_filtered(fyo, f, ctx, filtered)
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
        let link_extra = |key: &Key, f: &Field, ctx: &latte_core::DocContext<'_>| {
            if !matches!(f.fieldtype, Some(FieldType::Link | FieldType::DynamicLink)) {
                return (false, HashMap::new());
            }
            let filtered =
                !self.unfiltered.contains(key) && form_view::link_is_filtered(fyo, f, ctx);
            (bypass && filtered, form_view::link_groups(fyo, f, ctx))
        };
        for f in &fields {
            if f.fieldtype == Some(FieldType::Table) {
                for column in field::table_columns(fyo, f) {
                    for (i, row) in self.doc.table(&f.fieldname).iter().enumerate() {
                        let row_ctx = field::context(fyo, row, Some(&self.doc));
                        let key = Key::Cell(f.fieldname.clone(), i, column.fieldname.clone());
                        let read_only = field::is_read_only(&column, &row_ctx);
                        let opts = if is_choice(&column) && !read_only {
                            options(&key, &column, &row_ctx)
                        } else {
                            Vec::new()
                        };
                        let (bypass, groups) = link_extra(&key, &column, &row_ctx);
                        wanted.push(Wanted {
                            value: row.get(&column.fieldname).cloned(),
                            read_only,
                            options: opts,
                            required: field::is_required(&column, &row_ctx),
                            bypass,
                            groups,
                            key,
                            field: column.clone(),
                        });
                    }
                }
                // The open row's other fields, for the row form.
                if let Side::Row(table, i) = &self.side
                    && table == &f.fieldname
                    && let Some(row) = self.doc.table(table).get(*i)
                {
                    let row_ctx = field::context(fyo, row, Some(&self.doc));
                    for column in quick_edit_fields(fyo, f) {
                        let key = Key::Cell(table.clone(), *i, column.fieldname.clone());
                        let read_only = field::is_read_only(&column, &row_ctx);
                        let opts = if is_choice(&column) && !read_only {
                            options(&key, &column, &row_ctx)
                        } else {
                            Vec::new()
                        };
                        let (bypass, groups) = link_extra(&key, &column, &row_ctx);
                        wanted.push(Wanted {
                            value: row.get(&column.fieldname).cloned(),
                            read_only,
                            options: opts,
                            required: field::is_required(&column, &row_ctx),
                            bypass,
                            groups,
                            key,
                            field: column.clone(),
                        });
                    }
                }
                continue;
            }
            let key = Key::Field(f.fieldname.clone());
            // The quick edit title can be changed only on a new document named by hand.
            let read_only = field::is_read_only(f, &ctx)
                || title.as_deref() == Some(&f.fieldname)
                    && (self.doc.inserted()
                        || self.doc.schema().naming != Some(latte_schema::Naming::Manual));
            let opts = if is_choice(f) && !read_only {
                options(&key, f, &ctx)
            } else {
                Vec::new()
            };
            let (bypass, groups) = link_extra(&key, f, &ctx);
            wanted.push(Wanted {
                value: self.doc.get(&f.fieldname).cloned(),
                read_only,
                options: opts,
                required: field::is_required(f, &ctx),
                bypass,
                groups,
                key,
                field: f.clone(),
            });
        }
        drop(s);

        let keys: Vec<Key> = wanted.iter().map(|w| w.key.clone()).collect();
        self.controls.retain(|key, _| keys.contains(key));
        for w in wanted {
            let Wanted {
                key,
                field: f,
                value,
                read_only,
                options: opts,
                required: mandatory,
                bypass,
                groups,
            } = w;
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
                    | (Some(Control::Attachment), Kind::Attachment)
                    | (Some(Control::Color), Kind::Color)
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
                    let groups = groups
                        .into_iter()
                        .map(|(k, v)| (k.into(), v.into()))
                        .collect();
                    state.update(cx, |link, cx| {
                        link.set_mandatory(mandatory);
                        link.set_bypass(bypass);
                        link.set_groups(groups);
                        link.set_options(opts, cx);
                        if !link.is_typing() {
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
        self.refresh_header(window, cx);
        cx.notify();
    }

    /// The header's barcode input and exchange rate, made when the document shows them.
    fn refresh_header(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.quick.is_some() || self.embedded {
            return;
        }
        let (barcode, rate) = {
            let s = self.session.borrow();
            (
                form_view::shows_barcode(&s.fyo, &self.doc),
                form_view::exchange_rate(&s.fyo, &self.doc),
            )
        };
        if !barcode {
            self.barcode = None;
        } else if self.barcode.is_none() {
            let state = cx.new(|cx| InputState::new(window, cx).placeholder(t("Enter barcode")));
            let subscription = cx.subscribe_in(
                &state,
                window,
                |this, state, event: &InputEvent, window, cx| {
                    // `@change`: Enter, or leaving the input with something typed.
                    if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        return;
                    }
                    let code = state.read(cx).value().to_string();
                    if code.is_empty() {
                        return;
                    }
                    state.update(cx, |input, cx| input.set_value("", window, cx));
                    this.scan(&code, window, cx);
                },
            );
            self.barcode = Some((state, subscription));
        }
        match rate {
            None => self.exchange = None,
            Some((_, _, rate)) => {
                if self.exchange.is_none() {
                    let from_value = cx.new(|cx| InputState::new(window, cx).default_value("1"));
                    let right = cx.new(|cx| InputState::new(window, cx));
                    let from_sub = cx.subscribe_in(
                        &from_value,
                        window,
                        |this, _, event: &InputEvent, window, cx| {
                            if matches!(event, InputEvent::Change) {
                                this.show_rate(window, cx);
                            }
                        },
                    );
                    let right_sub = cx.subscribe_in(
                        &right,
                        window,
                        |this, state, event: &InputEvent, window, cx| {
                            if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                                let text = state.read(cx).value().to_string();
                                this.set_rate_from_right(&text, window, cx);
                            }
                        },
                    );
                    self.exchange = Some(ExchangeRate {
                        from_value,
                        right,
                        swapped: false,
                        _subscriptions: [from_sub, right_sub],
                    });
                }
                let _ = rate;
                self.show_rate(window, cx);
            }
        }
    }

    fn rate_amount(&self, cx: &App) -> f64 {
        self.exchange
            .as_ref()
            .and_then(|e| e.from_value.read(cx).value().trim().parse::<f64>().ok())
            .unwrap_or(1.0)
    }

    /// Shows the right amount for the rate and the left amount.
    fn show_rate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rate = {
            let s = self.session.borrow();
            form_view::exchange_rate(&s.fyo, &self.doc).map_or(1.0, |(_, _, r)| r)
        };
        let from_value = self.rate_amount(cx);
        let Some(exchange) = &self.exchange else {
            return;
        };
        let right = form_view::exchange_rate_right(rate, from_value, exchange.swapped);
        let text = latte_core::fyo::model::helpers::js_number_string(right);
        exchange.right.update(cx, |input, cx| {
            if !input.focus_handle(cx).is_focused(window) && input.value() != text {
                input.set_value(text, window, cx);
            }
        });
        cx.notify();
    }

    /// `rightChange`: the rate the typed right amount makes is set on the document.
    fn set_rate_from_right(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(swapped) = self.exchange.as_ref().map(|e| e.swapped) else {
            return;
        };
        let right = text.trim().parse::<f64>().unwrap_or(0.0);
        let rate = form_view::exchange_rate_from_right(right, self.rate_amount(cx), swapped);
        if !rate.is_finite() || rate <= 0.0 {
            self.show_rate(window, cx);
            return;
        }
        let current = {
            let s = self.session.borrow();
            form_view::exchange_rate(&s.fyo, &self.doc).map(|(_, _, r)| r)
        };
        if current == Some(rate) {
            return;
        }
        self.set_value(
            &Key::Field("exchangeRate".to_owned()),
            DocValue::Number(rate),
            window,
            cx,
        );
        if let Some(e) = self
            .field_errors
            .remove(&Key::Field("exchangeRate".to_owned()))
        {
            self.error_title = "Error";
            self.error = Some(e);
        }
    }

    /// `scanListener`: a scanner types its digits faster than anyone can (20ms apart at
    /// most), ending with Enter or a pause; 12 digits or more are a barcode, wherever the
    /// focus is.
    fn scan_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        use std::time::{Duration, Instant};
        if self.barcode.is_none() {
            return;
        }
        if key == "enter" {
            if self.scanned.0.len() >= 12 {
                let code = std::mem::take(&mut self.scanned.0);
                self.scan(&code, window, cx);
            }
            self.scanned.0.clear();
            return;
        }
        if key.len() != 1 || !key.chars().all(|c| c.is_ascii_digit()) {
            return;
        }
        let now = Instant::now();
        if self
            .scanned
            .1
            .is_none_or(|last| now.duration_since(last) > Duration::from_millis(20))
        {
            self.scanned.0.clear();
        }
        self.scanned.0.push_str(key);
        self.scanned.1 = Some(now);
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(25))
                .await;
            this.update_in(cx, |this, window, cx| {
                let quiet = this
                    .scanned
                    .1
                    .is_some_and(|last| last.elapsed() >= Duration::from_millis(20));
                if quiet && this.scanned.0.len() >= 12 {
                    let code = std::mem::take(&mut this.scanned.0);
                    this.scan(&code, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// `Barcode.vue`'s `selectItem`, then `addItem`.
    fn scan(&mut self, code: &str, window: &mut Window, cx: &mut Context<Self>) {
        // `cooldown`: the same code within 100ms is one scan.
        let code = code.trim();
        if self.cooldown.as_ref().is_some_and(|(last, at)| {
            last == code && at.elapsed() < std::time::Duration::from_millis(100)
        }) {
            return;
        }
        self.cooldown = Some((code.to_owned(), std::time::Instant::now()));
        let item = {
            let s = self.session.borrow();
            form_view::barcode_item(&s.fyo, code)
        };
        match item {
            Ok(name) => {
                let result = {
                    let mut s = self.session.borrow_mut();
                    form_view::add_item(&mut s.fyo, &mut self.doc, &name)
                };
                match result {
                    Ok(()) => self.toast(
                        Toast::new(format!("{name} quantity 1 added."), DialogKind::Success),
                        window,
                        cx,
                    ),
                    Err(e) => self.toast(Toast::new(e.to_string(), DialogKind::Error), window, cx),
                }
            }
            Err(message) => self.toast(Toast::new(message, DialogKind::Error), window, cx),
        }
        self.refresh(window, cx);
    }

    /// `showToast`: the window's toast, which stays when the form or its panel closes.
    fn toast(&self, toast: Toast, window: &Window, cx: &mut App) {
        show_toast(toast, window, cx);
    }

    /// `showActionToast` and `showSubmitToast`: "X saved" (short), or "X submitted" with
    /// a link to its ledger (`getSubmitSuccessToastAction`).
    fn action_toast(&self, done: &str, window: &Window, cx: &mut App) {
        let message = format!("{} {done}", self.reference_label());
        if done != "submitted" {
            self.toast(Toast::new(message, DialogKind::Success).short(), window, cx);
            return;
        }
        let schema = self.doc.schema_name().to_owned();
        let stock = matches!(
            schema.as_str(),
            "Shipment" | "PurchaseReceipt" | "StockMovement"
        );
        let transactional = stock
            || matches!(
                schema.as_str(),
                "SalesInvoice" | "PurchaseInvoice" | "Payment" | "JournalEntry"
            );
        let mut toast = Toast::new(message, DialogKind::Success);
        if transactional {
            let (label, report) = if stock {
                ("View Stock Entries", "stock-ledger")
            } else {
                ("View Accounting Entries", "general-ledger")
            };
            let filters = vec![
                ("referenceType".to_owned(), schema),
                (
                    "referenceName".to_owned(),
                    self.doc.name().unwrap_or_default().to_owned(),
                ),
            ];
            toast = toast.action(t(label), move |window, cx| {
                crate::desk::navigate(
                    Route::FilteredReport {
                        name: report.to_owned(),
                        filters: filters.clone(),
                    },
                    window,
                    cx,
                )
            });
        }
        self.toast(toast, window, cx);
    }

    /// A control's value as shown (`fyo.format(value, field, doc)`), an amount in the
    /// currency its document gives it.
    fn display(&self, key: &Key, f: &Field, s: &crate::session::Session) -> String {
        let value = self.current_value(key);
        match key {
            Key::Field(_) => {
                let ctx = field::context(&s.fyo, &self.doc, None);
                form_view::display(&s.formatter, f, value, &ctx)
            }
            Key::Cell(table, i, _) => match self.doc.table(table).get(*i) {
                Some(row) => {
                    let ctx = field::context(&s.fyo, row, Some(&self.doc));
                    form_view::display(&s.formatter, f, value, &ctx)
                }
                None => field::display(&s.formatter, f, value),
            },
        }
    }

    /// `evaluateReadOnly` for a control.
    fn is_read_only(&self, key: &Key, f: &Field, fyo: &latte_core::Fyo) -> bool {
        match key {
            Key::Field(_) => field::is_read_only(f, &field::context(fyo, &self.doc, None)),
            Key::Cell(table, i, _) => match self.doc.table(table).get(*i) {
                Some(row) => field::is_read_only(f, &field::context(fyo, row, Some(&self.doc))),
                None => true,
            },
        }
    }

    /// `showPicker()`: the calendar under a Date or Datetime control.
    fn open_picker(&mut self, key: &Key, f: &Field, window: &mut Window, cx: &mut Context<Self>) {
        if self.picker.as_ref().is_some_and(|(k, ..)| k == key) {
            return;
        }
        let with_time = f.fieldtype == Some(FieldType::Datetime);
        let value = match self.current_value(key) {
            Some(DocValue::Date(ts)) => Some(if with_time {
                ts.to_zoned(jiff::tz::TimeZone::system()).datetime()
            } else {
                latte_core::fyo::format::local_date(*ts).to_datetime(jiff::civil::Time::midnight())
            }),
            _ => None,
        };
        let picker = cx.new(|_| DatePicker::new(value, with_time));
        let (picked_key, picked_field) = (key.clone(), f.clone());
        let picked = cx.subscribe_in(
            &picker,
            window,
            move |this, _, event: &DatePicked, window, cx| {
                let value = match event.0 {
                    None => Ok(DocValue::Null),
                    Some(datetime) if picked_field.fieldtype == Some(FieldType::Datetime) => {
                        datetime
                            .to_zoned(jiff::tz::TimeZone::system())
                            .map(|z| DocValue::Date(z.timestamp()))
                            .map_err(|e| e.to_string())
                    }
                    Some(datetime) => field::date_timestamp(datetime.date())
                        .map(DocValue::Date)
                        .map_err(|e| e.to_string()),
                };
                match value {
                    Ok(value) => this.set_value(&picked_key, value, window, cx),
                    Err(e) => {
                        this.field_errors.insert(picked_key.clone(), e);
                        cx.notify();
                    }
                }
            },
        );
        let closed = cx.subscribe_in(&picker, window, |this, _, _: &PickerClosed, window, cx| {
            this.picker = None;
            // The typed text is shown formatted again.
            window.blur(cx);
            cx.notify();
        });
        self.picker = Some((key.clone(), picker, [picked, closed]));
        cx.notify();
    }

    /// `Attachment.vue`'s upload: an image or PDF, kept as `{name, type, data}`.
    fn pick_attachment(&mut self, key: Key, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t("Attachment").into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let attachment = attachment(&path);
            this.update_in(cx, |this, window, cx| match attachment {
                Ok(value) => this.set_value(&key, DocValue::Attachment(value), window, cx),
                Err(e) => {
                    this.field_errors.insert(key.clone(), e);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// `Attachment.vue`'s download: the file saved where the user picks.
    fn download_attachment(
        &mut self,
        value: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use base64::Engine as _;
        let (Some(name), Some(data)) = (
            value.get("name").and_then(|v| v.as_str()),
            value.get("data").and_then(|v| v.as_str()),
        ) else {
            return;
        };
        let Some(bytes) = data
            .split_once(";base64,")
            .and_then(|(_, b64)| base64::engine::general_purpose::STANDARD.decode(b64).ok())
        else {
            return;
        };
        let dir = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(|home| std::path::PathBuf::from(home).join("Downloads"))
            .unwrap_or_default();
        let path = cx.prompt_for_new_path(&dir, Some(name));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = path.await else {
                return;
            };
            let result = std::fs::write(&path, bytes);
            this.update(cx, |this, cx| {
                if let Err(e) = result {
                    this.error_title = "Error";
                    this.error = Some(e.to_string());
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Opens or closes a Color control's swatches.
    fn toggle_color(&mut self, key: &Key, window: &mut Window, cx: &mut Context<Self>) {
        if self.color.as_ref().is_some_and(|(k, ..)| k == key) {
            self.color = None;
            cx.notify();
            return;
        }
        let hex = cx.new(|cx| InputState::new(window, cx).placeholder(t("Custom Hex")));
        let hex_key = key.clone();
        let subscription = cx.subscribe_in(
            &hex,
            window,
            move |this, state, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    let text = state.read(cx).value().trim().to_owned();
                    if !text.is_empty() {
                        this.set_color(&hex_key, &text, window, cx);
                    }
                }
            },
        );
        self.color = Some((key.clone(), hex, subscription));
        cx.notify();
    }

    /// `setColorValue`: a colour as `#RRGGBB`; anything else is ignored.
    fn set_color(&mut self, key: &Key, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        let value = if value.starts_with('#') {
            value.to_owned()
        } else {
            format!("#{value}")
        };
        if parse_hex(&value).is_none() {
            return;
        }
        self.set_value(key, DocValue::Text(value), window, cx);
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
                let in_table = self.bare(key);
                let is_link = matches!(f.fieldtype, Some(FieldType::Link | FieldType::DynamicLink));
                let weak = cx.entity().downgrade();
                let view_field = f.clone();
                let quick_view: crate::link::QuickViewFn = Rc::new(move |name, cx| {
                    weak.upgrade()
                        .map(|form| form.read(cx).quick_view_data(&view_field, name))
                        .unwrap_or(QuickViewData {
                            name: None,
                            schema_label: String::new(),
                            values: Vec::new(),
                        })
                });
                let create = f.create;
                let state = cx.new(|cx| {
                    let mut link = LinkInput::new(options, placeholder, window, cx)
                        .linkable(is_link)
                        .quick_view(quick_view);
                    if is_link {
                        link = link.link(create);
                    }
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
                let (create_key, create_field) = (key.clone(), f.clone());
                let created = cx.subscribe_in(
                    &state,
                    window,
                    move |this, _, event: &LinkCreate, window, cx| {
                        this.create_linked(&create_key, &create_field, &event.0, window, cx);
                    },
                );
                let bypass_key = key.clone();
                let bypassed = cx.subscribe_in(
                    &state,
                    window,
                    move |this, link, _: &LinkDisableFilters, window, cx| {
                        this.unfiltered.insert(bypass_key.clone());
                        this.refresh(window, cx);
                        link.update(cx, |link, cx| link.reopen(window, cx));
                    },
                );
                Control::Link {
                    state,
                    _subscriptions: [picked, opened, created, bypassed],
                }
            }
            Kind::Check => Control::Check,
            Kind::Display => Control::Display,
            Kind::Image => Control::Image,
            Kind::Attachment => Control::Attachment,
            Kind::Color => Control::Color,
        }
    }

    /// `QuickView.vue`'s data for a Link field's value.
    fn quick_view_data(&self, f: &Field, name: &str) -> QuickViewData {
        let s = self.session.borrow();
        let ctx = field::context(&s.fyo, &self.doc, None);
        let target = field::link_target(f, &ctx).unwrap_or_default();
        let Some(schema) = s.fyo.schema(&target) else {
            return QuickViewData {
                name: None,
                schema_label: String::new(),
                values: Vec::new(),
            };
        };
        QuickViewData {
            name: (schema.naming != Some(latte_schema::Naming::Random) && !schema.is_child)
                .then(|| name.to_owned()),
            schema_label: schema.label.clone(),
            values: form_view::quick_view(&s.fyo, &s.formatter, &target, name),
        }
    }

    /// `openNewDoc`: a new document named by what was typed opens in a quick edit panel;
    /// once saved, the panel closes and the field takes it.
    fn create_linked(
        &mut self,
        key: &Key,
        f: &Field,
        typed: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let doc = {
            let s = self.session.borrow();
            let values = match key {
                Key::Cell(table, i, _) => match self.doc.table(table).get(*i) {
                    Some(row) => {
                        let ctx = field::context(&s.fyo, row, Some(&self.doc));
                        form_view::create_values(&s.fyo, f, &ctx, typed)
                    }
                    None => return,
                },
                Key::Field(_) => {
                    let ctx = field::context(&s.fyo, &self.doc, None);
                    form_view::create_values(&s.fyo, f, &ctx, typed)
                }
            };
            let ctx = field::context(&s.fyo, &self.doc, None);
            let target = match key {
                Key::Cell(table, i, _) => self
                    .doc
                    .table(table)
                    .get(*i)
                    .and_then(|row| field::link_target(f, &field::context(&s.fyo, row, None))),
                Key::Field(_) => field::link_target(f, &ctx),
            };
            let Some(target) = target else {
                return;
            };
            s.fyo.get_new_doc(&target, values)
        };
        let doc = match doc {
            Ok(doc) => doc,
            Err(e) => {
                self.error_title = error_label(&e);
                self.error = Some(e.to_string());
                cx.notify();
                return;
            }
        };
        let weak = cx.entity().downgrade();
        let key = key.clone();
        let after_save: AfterSave = Rc::new(move |quick, window, cx| {
            let Some(name) = quick.read(cx).doc().name().map(str::to_owned) else {
                return;
            };
            if let Some(form) = weak.upgrade() {
                form.update(cx, |form, cx| {
                    form.set_value(&key, DocValue::Text(name), window, cx)
                });
            }
        });
        let request = QuickRequest {
            doc,
            hide: Vec::new(),
            after_save: Some(after_save),
        };
        self.open_quick_edit(request, window, cx);
    }

    /// Opens a quick edit panel beside this form, or, from a panel, beside its page.
    fn open_quick_edit(
        &mut self,
        request: QuickRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.quick.is_some() {
            cx.emit(QuickEvent::Open(Box::new(request)));
        } else {
            let session = self.session.clone();
            open_quick(self, session, request, window, cx);
        }
    }

    /// `getMakePaymentAction`: the invoice's payment in a quick edit panel, without the
    /// party and the rows; once saved it is submitted and the invoice shown again.
    fn make_payment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let payment = {
            let mut s = self.session.borrow_mut();
            let fyo = &mut s.fyo;
            invoice::get_payment(&self.doc, fyo).and_then(|payment| {
                let Some(mut payment) = payment else {
                    return Ok(None);
                };
                payment.set(
                    fyo,
                    "referenceType",
                    DocValue::Text(self.doc.schema_name().to_owned()),
                )?;
                payment.run_formulas(fyo)?;
                Ok(Some(payment))
            })
        };
        let payment = match payment {
            Ok(Some(payment)) => payment,
            Ok(None) => return,
            Err(e) => {
                self.error_title = error_label(&e);
                self.error = Some(e.to_string());
                cx.notify();
                return;
            }
        };
        let mut hide = vec!["party".to_owned(), "for".to_owned()];
        let returns_on = {
            let s = self.session.borrow();
            single_is_truthy(&s.fyo, "AccountingSettings", "enableInvoiceReturns")
        };
        if !returns_on {
            hide.push("paymentType".to_owned());
        }
        hide.push(if self.doc.schema_name() == "SalesInvoice" {
            "account".to_owned()
        } else {
            "paymentAccount".to_owned()
        });
        let weak = cx.entity().downgrade();
        let session = self.session.clone();
        let after_save: AfterSave = Rc::new(move |quick, window, cx| {
            let result = quick.update(cx, |quick, _| {
                let mut s = session.borrow_mut();
                quick.doc.submit(&mut s.fyo)
            });
            if let Some(form) = weak.upgrade() {
                form.update(cx, |form, cx| {
                    if let Err(e) = result {
                        form.error_title = error_label(&e);
                        form.error = Some(e.to_string());
                    }
                    form.reload(window, cx);
                });
            }
        });
        self.open_quick_edit(
            QuickRequest {
                doc: payment,
                hide,
                after_save: Some(after_save),
            },
            window,
            cx,
        );
    }

    /// `doc.load()`: the document as it is stored now.
    fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let result = {
            let mut s = self.session.borrow_mut();
            self.doc.load(&mut s.fyo)
        };
        if let Err(e) = result {
            self.error_title = error_label(&e);
            self.error = Some(e.to_string());
        }
        self.controls.clear();
        self.refresh(window, cx);
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
        if matches!(self.side, Side::Row(..)) {
            self.side = Side::None;
        }
        // Row controls are keyed by position, so they are made again.
        self.controls
            .retain(|key, _| !matches!(key, Key::Cell(t, ..) if t == table));
        self.refresh(window, cx);
    }

    /// Opens a child row's form beside the document's (`showRowEditForm`), in place of the
    /// linked entries.
    fn open_row(&mut self, table: &str, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_typed(window, cx);
        self.side = Side::Row(table.to_owned(), i);
        self.refresh(window, cx);
    }

    /// Closes the side panel: the row form or the linked entries.
    fn close_side(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_typed(window, cx);
        self.side = Side::None;
        self.refresh(window, cx);
    }

    /// `canShowLinks`: a saved document, submitted if it can be.
    fn can_show_links(&self) -> bool {
        let doc = &self.doc;
        !(doc.schema().is_submittable && !doc.is_submitted()) && doc.inserted()
    }

    /// Shows the linked entries beside the form (`showLinks`), or hides them.
    fn toggle_links(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.side, Side::Links(..)) {
            self.close_side(window, cx);
            return;
        }
        if !self.can_show_links() {
            return;
        }
        let groups = {
            let s = self.session.borrow();
            form_view::linked_entries(&s.fyo, &s.formatter, &self.doc)
        };
        match groups {
            Ok(groups) => {
                self.quick_panels.clear();
                self.side = Side::Links(groups, HashSet::new());
            }
            Err(e) => {
                self.error_title = error_label(&e);
                self.error = Some(e.to_string());
            }
        }
        self.refresh(window, cx);
    }

    /// `LinkedEntries.vue`: a 22rem panel with Close and its title, then each schema's
    /// documents, with their count, folded by a click on the heading.
    fn render_links(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let Side::Links(groups, folded) = &self.side else {
            return None;
        };
        let header = div()
            .h_flex()
            .flex_none()
            .h(books::H_PAGE_HEADER)
            .px_4()
            .justify_between()
            .child(
                Button::new("close-links")
                    .icon("x")
                    .on_click(cx.listener(|this, _, window, cx| this.close_side(window, cx))),
            )
            .child(
                div()
                    .text_18()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(gray(600))
                    .child(t("Linked Entries")),
            );
        let body = if groups.is_empty() {
            div()
                .p_4()
                .text_12()
                .text_color(gray(600))
                .child(t("No linked entries found"))
                .into_any_element()
        } else {
            div()
                .v_flex()
                .w_full()
                .border_t_1()
                .border_color(gray(200))
                .children(groups.iter().enumerate().map(|(g, group)| {
                    let collapsed = folded.contains(&group.schema);
                    let schema = group.schema.clone();
                    let heading = div()
                        .id(("links-heading", g))
                        .h_flex()
                        .justify_between()
                        .cursor_pointer()
                        .when(!collapsed, |d| d.pb_4())
                        .child(
                            div()
                                .h_flex()
                                .text_13()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(gray(600))
                                .child(group.label.clone())
                                .child(
                                    div()
                                        .font_weight(FontWeight::NORMAL)
                                        .child(format!(" – {}", group.entries.len())),
                                ),
                        )
                        .child(books::icon(
                            if collapsed {
                                "chevron-up"
                            } else {
                                "chevron-down"
                            },
                            px(16.),
                            gray(600),
                        ))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Side::Links(_, folded) = &mut this.side
                                && !folded.remove(&schema)
                            {
                                folded.insert(schema.clone());
                            }
                            cx.notify();
                        }));
                    let count = group.entries.len();
                    let entries = (!collapsed).then(|| {
                        div()
                            .v_flex()
                            .rounded(books::RADIUS_MD)
                            .border_1()
                            .border_color(gray(200))
                            .overflow_hidden()
                            .children(group.entries.iter().enumerate().map(|(e, entry)| {
                                let schema = group.schema.clone();
                                let name = entry.name.clone();
                                div()
                                    .id(SharedString::from(format!("link-{g}-{e}")))
                                    .p_2()
                                    .text_12()
                                    .cursor_pointer()
                                    .when(e + 1 < count, |d| d.border_b_1().border_color(gray(200)))
                                    .hover(|d| d.bg(gray(50)))
                                    .child(
                                        div()
                                            .h_flex()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .child(entry.name.clone()),
                                            )
                                            .children(entry.date.clone().map(|date| {
                                                div().text_11().text_color(gray(600)).child(date)
                                            })),
                                    )
                                    .when(!entry.pills.is_empty(), |d| {
                                        d.child(div().h_flex().flex_wrap().gap_2().mt_1().children(
                                            entry.pills.iter().map(|p| {
                                                let (bg, fg) = books::bg_text(p.color);
                                                div()
                                                    .px(px(6.))
                                                    .py(px(2.))
                                                    .rounded(books::RADIUS_MD)
                                                    .text_11()
                                                    .bg(bg)
                                                    .text_color(fg)
                                                    .child(p.text.clone())
                                            }),
                                        ))
                                    })
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.emit(Navigate(Nav::Route(Route::Form {
                                            schema: schema.clone(),
                                            name: Some(name.clone()),
                                        })))
                                    }))
                            }))
                    });
                    div()
                        .v_flex()
                        .p_4()
                        .border_b_1()
                        .border_color(gray(200))
                        .child(heading)
                        .children(entries)
                }))
                .into_any_element()
        };
        Some(
            div()
                .id("linked-entries")
                .v_flex()
                .flex_none()
                .w(rems(22.))
                .h_full()
                .bg(white())
                .border_l_1()
                .border_color(gray(200))
                .overflow_y_scroll()
                .child(header)
                .child(body)
                .into_any_element(),
        )
    }

    /// `RowEditForm.vue`: the open row's quick-edit fields, label beside control, in a
    /// 22rem panel to the right of the form, with Close and Previous/Next row.
    fn render_row_edit(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let Side::Row(table, i) = &self.side else {
            return None;
        };
        let (table, i) = (table.clone(), *i);
        let count = self.doc.table(&table).len();
        if i >= count {
            return None;
        }
        let (table_field, fields) = {
            let s = self.session.borrow();
            let f = self.doc.schema().field(&table)?.clone();
            let fields = quick_edit_fields(&s.fyo, &f);
            (f, fields)
        };
        let rows = fields.iter().enumerate().map(|(n, f)| {
            let key = Key::Cell(table.clone(), i, f.fieldname.clone());
            div()
                .grid()
                .grid_cols(3)
                .items_center()
                .border_b_1()
                .border_color(gray(200))
                .text_12()
                .when(n == 0, |d| d.border_t_1())
                .child(
                    div()
                        .pl_4()
                        .text_color(gray(600))
                        .child(f.label.clone().unwrap_or_else(|| f.fieldname.clone())),
                )
                .child(
                    div()
                        .col_span(2)
                        .py_2()
                        .pr_4()
                        .child(self.render_control(&key, f, window, cx))
                        .children(self.field_errors.get(&key).map(|e| {
                            div()
                                .mt_1()
                                .text_11()
                                .text_color(books::color("red", 600))
                                .child(e.clone())
                        })),
                )
        });
        let rows: Vec<Div> = rows.collect();
        let step = |id: &'static str, icon: &'static str, to: usize| {
            let table = table.clone();
            Button::new(id).icon(icon).on_click(
                cx.listener(move |this, _, window, cx| this.open_row(&table, to, window, cx)),
            )
        };
        let toolbar = div()
            .h_flex()
            .flex_none()
            .h_16()
            .px_4()
            .justify_between()
            .child(
                Button::new("close-row")
                    .icon("x")
                    .on_click(cx.listener(|this, _, window, cx| this.close_side(window, cx))),
            )
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .when(i > 0, |d| d.child(step("prev-row", "chevron-left", i - 1)))
                    .when(i + 1 < count, |d| {
                        d.child(step("next-row", "chevron-right", i + 1))
                    }),
            );
        let header = div()
            .h_flex()
            .flex_none()
            .h(px(56.))
            .px_4()
            .justify_between()
            .border_t_1()
            .border_color(gray(200))
            .text_18()
            .font_weight(FontWeight::SEMIBOLD)
            .child(
                div()
                    .text_color(black())
                    .child(format!("{} {}", t("Row"), i + 1)),
            )
            .child(
                div()
                    .text_color(gray(600))
                    .child(table_field.label.clone().unwrap_or_default()),
            );
        Some(
            div()
                .id("row-edit")
                .v_flex()
                .flex_none()
                .w(rems(22.))
                .h_full()
                .bg(white())
                .border_l_1()
                .border_color(gray(200))
                .overflow_y_scroll()
                .child(toolbar)
                .child(header)
                .children(rows)
                .into_any_element(),
        )
    }

    /// Sets every value typed into an input and not yet set.
    fn commit_typed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A Link or Select being typed into settles as when it is left.
        let links: Vec<(Key, Entity<LinkInput>)> = self
            .controls
            .iter()
            .filter_map(|(key, control)| match control {
                Control::Link { state, .. } => Some((key.clone(), state.clone())),
                _ => None,
            })
            .collect();
        for (key, link) in links {
            if let Some(value) = link.update(cx, |link, cx| link.commit(window, cx)) {
                let value = if value.is_empty() {
                    DocValue::Null
                } else {
                    DocValue::Text(value.to_string())
                };
                self.set_value(&key, value, window, cx);
            }
        }
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
                if !self.embedded {
                    self.action_toast(done, window, cx);
                }
                self.controls.clear();
                self.refresh(window, cx);
                if self.quick.is_some() && matches!(done, "saved" | "submitted") {
                    cx.emit(QuickEvent::Saved);
                }
                true
            }
            Err(e) => {
                self.error_title = error_label(&e);
                self.error = Some(e.to_string());
                cx.notify();
                false
            }
        }
    }

    /// `getDocReferenceLabel`: the document's name, or its schema's label.
    fn reference_label(&self) -> String {
        form_view::reference_label(&self.doc)
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

    /// `commonDocSubmit`: a Sales Invoice with inventory on first says which items are
    /// short, then Submit is asked.
    fn ask_submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_typed(window, cx);
        let short = {
            let s = self.session.borrow();
            form_view::insufficient_quantity(&s.fyo, &self.doc)
        };
        self.pending = Some(match short {
            Ok(short) if !short.is_empty() => Pending::Insufficient(short),
            _ => Pending::Submit,
        });
        cx.notify();
    }

    /// Ctrl+S (`syncOrSubmitCallback`): asks to save or submit, or says there is nothing
    /// to do.
    fn save_or_submit_shortcut(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_typed(window, cx);
        let doc = &self.doc;
        if doc.can_save() || doc.not_inserted() || doc.schema().is_single && doc.dirty() {
            if self.embedded {
                self.save(window, cx);
            } else {
                self.pending = Some(Pending::Sync);
            }
        } else if doc.can_submit() {
            self.ask_submit(window, cx);
        } else {
            let toast = Toast::new(form_view::cannot_save_message(doc), DialogKind::Warning);
            self.toast(toast.short(), window, cx);
        }
        cx.notify();
    }

    /// Ctrl+Backspace (`cancelOrDeleteCallback`).
    fn cancel_or_delete_shortcut(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let doc = &self.doc;
        if doc.can_cancel() {
            self.pending = Some(Pending::Cancel);
        } else if doc.inserted() && doc.can_delete() && !doc.schema().is_single {
            self.pending = Some(Pending::Delete);
        } else {
            let toast = Toast::new(form_view::cannot_cancel_message(doc), DialogKind::Warning);
            self.toast(toast.short(), window, cx);
        }
        cx.notify();
    }

    /// `canPrint`.
    fn can_print(&self) -> bool {
        let doc = &self.doc;
        doc.inserted()
            && !doc.is_cancelled()
            && !doc.dirty()
            && crate::print::can_print(&self.session, doc.schema_name())
    }

    fn open_print(&mut self, cx: &mut Context<Self>) {
        if !self.can_print() {
            return;
        }
        cx.emit(Navigate(Nav::Route(Route::Print {
            schema: self.doc.schema_name().to_owned(),
            name: self.doc.name().unwrap_or_default().to_owned(),
        })));
    }

    fn confirm(&mut self, pending: Pending, window: &mut Window, cx: &mut Context<Self>) {
        match pending {
            Pending::Sync => self.save(window, cx),
            Pending::Insufficient(_) => {
                self.pending = Some(Pending::Submit);
                cx.notify();
            }
            Pending::Submit => {
                self.run("submitted", |doc, fyo| doc.submit(fyo), window, cx);
            }
            Pending::Cancel => {
                self.run("cancelled", |doc, fyo| doc.cancel(fyo), window, cx);
            }
            Pending::Delete => self.delete(window, cx),
        }
    }

    /// `commongDocDelete`: deletes, then goes back to the page before (`router.back()`).
    /// A document others link to can't go: "Delete Failed".
    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pending = None;
        let result = {
            let mut s = self.session.borrow_mut();
            self.doc.delete(&mut s.fyo)
        };
        if let Err(e) = result {
            if form_view::is_link_validation_error(&e) {
                self.error_title = "Delete Failed";
                self.error = Some(form_view::delete_failed_message(&self.doc));
            } else {
                self.error_title = error_label(&e);
                self.error = Some(e.to_string());
            }
            cx.notify();
            return;
        }
        // `showActionToast(doc, 'delete')`.
        self.action_toast("deleted", window, cx);
        if self.quick.is_some() {
            cx.emit(QuickEvent::Close);
            return;
        }
        let schema = self.doc.schema_name().to_owned();
        let label = self.doc.schema().label.clone();
        let list = Route::list(&schema, &label, None);
        if schema == "CustomForm" {
            cx.emit(Navigate(Nav::Reopen(list)));
            return;
        }
        // `router.back()`; with nowhere to go back to, the list.
        let desk = cx
            .try_global::<crate::desk::NavState>()
            .filter(|n| n.can_back)
            .and_then(|n| n.desk.upgrade());
        match desk {
            Some(desk) => window.defer(cx, move |window, cx| {
                desk.update(cx, |desk, cx| desk.go_back(window, cx));
            }),
            None => cx.emit(Navigate(Nav::Route(list))),
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
        // `getLedgerLinkAction`: the document's ledger entries, once submitted.
        let ledger = |stock: bool| {
            let (label, report) = if stock {
                ("Stock Entries", "stock-ledger")
            } else {
                ("Accounting Entries", "general-ledger")
            };
            let filters = vec![
                ("referenceType".to_owned(), schema.clone()),
                (
                    "referenceName".to_owned(),
                    doc.name().unwrap_or_default().to_owned(),
                ),
            ];
            Action::new(
                VIEW,
                label,
                Rc::new(move |_, _, cx| {
                    cx.emit(Navigate(Nav::Route(Route::FilteredReport {
                        name: report.to_owned(),
                        filters: filters.clone(),
                    })))
                }),
            )
        };

        if is_invoice && submitted {
            // `getMakePaymentAction`.
            let outstanding =
                matches!(doc.get("outstandingAmount"), Some(DocValue::Money(m)) if !m.is_zero());
            if outstanding {
                actions.push(create(
                    "Payment",
                    Rc::new(|this, window, cx| this.make_payment(window, cx)),
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
            actions.push(ledger(false));
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
            actions.push(ledger(false));
            actions.push(ledger(true));
            if returns_on && !is_return {
                actions.push(create(
                    "Return",
                    Rc::new(|this, _, cx| this.open_made(stock_transfer::get_return_doc, cx)),
                ));
            }
        }
        if matches!(schema.as_str(), "JournalEntry" | "Payment") && submitted {
            actions.push(ledger(false));
        }
        if schema == "StockMovement" && submitted {
            actions.push(ledger(true));
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
            let party = doc.name().unwrap_or_default().to_owned();
            // "View Sales" and "View Purchases": the invoices of the party.
            let view = |schema: &'static str, title: &'static str| {
                let party = party.clone();
                Rc::new(
                    move |_: &mut FormPage, _: &mut Window, cx: &mut Context<FormPage>| {
                        cx.emit(Navigate(Nav::Route(Route::filtered_list(
                            schema,
                            title,
                            &[("party", "=", &party)],
                        ))))
                    },
                ) as ActionFn
            };
            if role != "Customer" {
                actions.push(Action::new(
                    "",
                    "Create Purchase",
                    Rc::new(|this, _, cx| this.open_party_invoice("PurchaseInvoice", cx)),
                ));
                actions.push(Action::new(
                    "",
                    "View Purchases",
                    view("PurchaseInvoice", "Purchase Invoices"),
                ));
            }
            if role != "Supplier" {
                actions.push(Action::new(
                    "",
                    "Create Sale",
                    Rc::new(|this, _, cx| this.open_party_invoice("SalesInvoice", cx)),
                ));
                actions.push(Action::new(
                    "",
                    "View Sales",
                    view("SalesInvoice", "Sales Invoices"),
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
        // `getViewActions`: a party's General Ledger.
        if schema == "Party" && doc.inserted() {
            let filters = vec![(
                "party".to_owned(),
                doc.name().unwrap_or_default().to_owned(),
            )];
            actions.push(Action::new(
                VIEW,
                "General Ledger",
                Rc::new(move |_, _, cx| {
                    cx.emit(Navigate(Nav::Route(Route::FilteredReport {
                        name: "general-ledger".to_owned(),
                        filters: filters.clone(),
                    })))
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
        if self.can_show_links() {
            buttons.push(
                Button::new("links")
                    .icon("link")
                    .tooltip(t("View linked entries"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        if !matches!(this.side, Side::Links(..)) {
                            this.toggle_links(window, cx)
                        }
                    }))
                    .into_any_element(),
            );
        }
        if self.can_print() {
            buttons.push(
                Button::new("print")
                    .icon("printer")
                    .tooltip(t("Open Print View"))
                    .on_click(cx.listener(|this, _, _, cx| this.open_print(cx)))
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
                    .on_click(cx.listener(|this, _, window, cx| this.ask_submit(window, cx)))
                    .into_any_element(),
            );
        }
        buttons
    }

    /// `CommonForm.vue`'s `#header-left`: the barcode input and the exchange rate when
    /// shown, else the schema's label.
    fn header_left(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let mut left = div().h_flex().gap_2();
        if let Some((state, _)) = &self.barcode {
            let focused = state.focus_handle(cx).is_focused(window);
            let focus = state.clone();
            // `Barcode.vue`: a 9rem input with the scanner icon.
            left = left.child(
                div()
                    .id("barcode")
                    .h_flex()
                    .w(rems(9.))
                    .h_8()
                    .px_2()
                    .rounded(books::RADIUS)
                    .border_1()
                    .border_color(gray(200))
                    .bg(if focused { gray(100) } else { gray(50) })
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(state)
                                .appearance(false)
                                .h(px(28.))
                                .px_0()
                                .text_13(),
                        ),
                    )
                    .child(
                        div()
                            .id("barcode-icon")
                            .cursor_text()
                            .child(books::icon("maximize", px(12.), gray(600)))
                            .on_click(move |_, window, cx| {
                                focus.focus_handle(cx).focus(window, cx)
                            }),
                    ),
            );
        }
        if let Some(exchange) = &self.exchange {
            left = left.child(self.render_exchange_rate(exchange, cx));
        }
        if self.barcode.is_none() && self.exchange.is_none() {
            left = left.child(
                div()
                    .text_18()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(gray(600))
                    .whitespace_nowrap()
                    .child(t(&self.doc.schema().label)),
            );
        }
        left
    }

    /// `ExchangeRate.vue`: "1 USD = 82.5 INR", the right amount typed to set the rate, and
    /// the green swap button.
    fn render_exchange_rate(&self, exchange: &ExchangeRate, cx: &mut Context<Self>) -> Div {
        let (from, to) = {
            let s = self.session.borrow();
            form_view::exchange_rate(&s.fyo, &self.doc)
                .map(|(from, to, _)| (from, to))
                .unwrap_or_default()
        };
        let disabled = self.doc.is_submitted() || self.doc.is_cancelled();
        let (left, right) = if exchange.swapped {
            (to, from)
        } else {
            (from, to)
        };
        let rate_box = |state: &Entity<InputState>, currency: String| {
            div()
                .h_flex()
                .gap_2()
                .px_1()
                .rounded(books::RADIUS_MD)
                .border_1()
                .border_color(gray(100))
                .bg(if disabled { gray(100) } else { gray(25) })
                .text_12()
                .text_color(gray(900))
                .child(
                    div().w_12().child(
                        Input::new(state)
                            .appearance(false)
                            .disabled(disabled)
                            .h(px(22.))
                            .px(px(2.))
                            .text_right()
                            .text_12(),
                    ),
                )
                .child(currency)
        };
        div()
            .h_flex()
            .h_8()
            .p_1()
            .rounded(books::RADIUS_MD)
            .border_1()
            .border_color(gray(200))
            .bg(gray(50))
            .text_12()
            .child(rate_box(&exchange.from_value, left))
            .child(div().mx_1().text_color(gray(600)).child("="))
            .child(rate_box(&exchange.right, right))
            .when(!disabled, |d| {
                d.child(
                    div()
                        .id("swap-rate")
                        .h_flex()
                        .h_full()
                        .px_2()
                        .ml_1()
                        .mr(px(-2.))
                        .border_l_1()
                        .border_color(gray(200))
                        .bg(books::color("green", 100))
                        .cursor_pointer()
                        .child(books::icon("refresh-cw", px(12.), gray(600)))
                        .on_click(cx.listener(|this, _, window, cx| {
                            if let Some(e) = &mut this.exchange {
                                e.swapped = !e.swapped;
                            }
                            this.show_rate(window, cx);
                        })),
                )
            })
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

    /// The dialog Save, Submit, Cancel and Delete ask in (`showSubmitOrSyncDialog`,
    /// `showInsufficientInventoryDialog`, `cancelDocWithPrompt`, `deleteDocWithPrompt`),
    /// or an action's error.
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
        let pending = self.pending.clone()?;
        let label = self.reference_label();
        let (title, detail, kind) = {
            let s = self.session.borrow();
            match &pending {
                Pending::Sync => (
                    format!("Save {label}?"),
                    form_view::sync_message(&s.formatter, &self.doc),
                    DialogKind::Info,
                ),
                Pending::Submit => (
                    format!("Submit {label}?"),
                    form_view::submit_message(&s.fyo, &s.formatter, &self.doc),
                    DialogKind::Info,
                ),
                Pending::Insufficient(short) => (
                    t("Insufficient Quantity"),
                    form_view::insufficient_quantity_detail(short).join("\n"),
                    DialogKind::Warning,
                ),
                Pending::Cancel => (
                    format!("Cancel {label}?"),
                    form_view::cancel_message(&s.fyo, &self.doc)
                        .unwrap_or_else(|_| "This action is permanent".to_owned()),
                    DialogKind::Warning,
                ),
                Pending::Delete => (
                    format!("Delete {label}?"),
                    form_view::delete_message(&self.doc).to_owned(),
                    DialogKind::Warning,
                ),
            }
        };
        let yes = cx.listener(move |this, _, window, cx| this.confirm(pending.clone(), window, cx));
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
                    DialogButton::new(t("No"), false, no).escape(),
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
        let in_table = self.bare(key);
        let numeric = is_numeric(f);
        let s = self.session.borrow();
        match self.controls.get(key) {
            Some(Control::Input { state, .. }) => {
                let focused = state.focus_handle(cx).is_focused(window);
                // Currency, numbers and dates show their formatted value until clicked
                // (`Currency.vue`, `Date.vue`).
                let formatted = (!focused && shows_formatted(f)).then(|| self.display(key, f, &s));
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
                        .on_click(cx.listener({
                            let key = key.clone();
                            let f = f.clone();
                            move |this, _, window, cx| {
                                state.focus_handle(cx).focus(window, cx);
                                // `activateInput`: the input, and its picker.
                                if is_date {
                                    this.open_picker(&key, &f, window, cx);
                                }
                            }
                        }))
                });
                let picker = self
                    .picker
                    .as_ref()
                    .filter(|(k, ..)| k == key)
                    .map(|(_, picker, _)| anchored_picker(picker));
                div()
                    .v_flex()
                    .relative()
                    .w_full()
                    .child(
                        frame
                            .relative()
                            .child(div().flex_1().min_w_0().child(input))
                            .children(overlay),
                    )
                    .children(picker)
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
                    Key::Field(_) if self.quick.is_none() => f.label.clone(),
                    Key::Field(_) => None,
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
                // In a quick edit panel, a 5rem circle with the title's first letter
                // (`letterPlaceholder`); in the form, the `form` size.
                let letter = self.quick.as_ref().and_then(|_| {
                    let title = self.title_field()?;
                    match self.doc.get(&title.fieldname) {
                        Some(DocValue::Text(t)) => t.chars().next().map(|c| c.to_string()),
                        _ => None,
                    }
                });
                let round = self.quick.is_some();
                div()
                    .id(group.clone())
                    .group(group.clone())
                    .relative()
                    .size(if round { px(80.) } else { px(135.) })
                    .flex()
                    .items_center()
                    .justify_center()
                    .overflow_hidden()
                    .bg(white())
                    .border_1()
                    .border_color(gray(200))
                    .map(|d| {
                        if round {
                            d.rounded_full()
                        } else {
                            d.rounded(books::RADIUS)
                        }
                    })
                    .map(|d| match image {
                        Some(image) => {
                            d.child(img(image).size_full().object_fit(ObjectFit::Contain))
                        }
                        None if letter.is_some() => d.child(
                            div()
                                .text_size(px(36.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(gray(400))
                                .children(letter),
                        ),
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
            Some(Control::Attachment) => {
                let read_only = self.is_read_only(key, f, &s.fyo);
                let value = match self.current_value(key) {
                    Some(DocValue::Attachment(value)) => Some(value.clone()),
                    _ => None,
                };
                let name = value
                    .as_ref()
                    .and_then(|v| v.get("name"))
                    .and_then(|n| n.as_str())
                    .map(str::to_owned);
                let label = name.clone().unwrap_or_else(|| {
                    f.placeholder
                        .clone()
                        .or_else(|| f.label.clone())
                        .unwrap_or_else(|| t("Attachment"))
                });
                let button = |id: &str, icon: &'static str| {
                    div()
                        .id(SharedString::from(format!("{key:?}-{id}")))
                        .p(px(2.))
                        .rounded(books::RADIUS)
                        .cursor_pointer()
                        .child(books::icon(icon, px(16.), gray(600)))
                };
                let (upload_key, clear_key) = (key.clone(), key.clone());
                let has_value = value.is_some();
                self.frame(key, read_only)
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .when(!has_value, |d| d.text_color(gray(600)))
                            .when(has_value && read_only, |d| d.text_color(gray(800)))
                            .child(label),
                    )
                    .child(
                        div()
                            .h_flex()
                            .gap_1()
                            .when(!has_value && !read_only, |d| {
                                d.child(button("upload", "upload").on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.pick_attachment(upload_key.clone(), window, cx)
                                    },
                                )))
                            })
                            .children(value.clone().map(|value| {
                                button("download", "download").on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.download_attachment(&value, window, cx)
                                    },
                                ))
                            }))
                            .when(has_value && !read_only, |d| {
                                d.child(button("clear", "x").on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.set_value(&clear_key, DocValue::Null, window, cx)
                                    },
                                )))
                            }),
                    )
                    .into_any_element()
            }
            Some(Control::Color) => {
                let read_only = self.is_read_only(key, f, &s.fyo);
                let value = match self.current_value(key) {
                    Some(DocValue::Text(v)) if !v.is_empty() => Some(v.clone()),
                    _ => None,
                };
                let label = value.as_ref().map(|v| {
                    f.options
                        .iter()
                        .flatten()
                        .find(|o| &o.value == v)
                        .map_or_else(|| v.clone(), |o| o.label.clone())
                });
                let placeholder = f
                    .placeholder
                    .clone()
                    .or_else(|| f.label.clone())
                    .unwrap_or_default();
                let open = self.color.as_ref().filter(|(k, ..)| k == key);
                let popover = open.map(|(_, hex, _)| {
                    let swatches = f.options.clone().unwrap_or_default();
                    let hex = hex.clone();
                    // `placement="bottom-end"`.
                    div().absolute().right_0().bottom_0().size_0().child(
                        deferred(
                            anchored().snap_to_window().anchor(Anchor::TopRight).child(
                                popover_panel()
                                    .id("color-popover")
                                    .occlude()
                                    .mt_2()
                                    .p_2()
                                    .text_12()
                                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                        this.color = None;
                                        cx.notify();
                                    }))
                                    .child(div().grid().grid_cols(5).gap_2().children(
                                        swatches.into_iter().enumerate().map(|(i, o)| {
                                            let key = key.clone();
                                            let value = o.value.clone();
                                            div()
                                                .id(("swatch", i))
                                                .size_4()
                                                .rounded(books::RADIUS)
                                                .cursor_pointer()
                                                .bg(parse_hex(&o.value).unwrap_or(white()))
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.set_color(&key, &value, window, cx)
                                                    },
                                                ))
                                        }),
                                    ))
                                    .child(
                                        div().mt_3().w(rems(7.)).child(
                                            control_frame(false, false).min_h(px(28.)).child(
                                                Input::new(&hex)
                                                    .appearance(false)
                                                    .h(px(26.))
                                                    .px_2()
                                                    .text_12(),
                                            ),
                                        ),
                                    ),
                            ),
                        )
                        .with_priority(1),
                    )
                });
                let toggle_key = key.clone();
                div()
                    .v_flex()
                    .relative()
                    .w_full()
                    .child(
                        self.frame(key, read_only)
                            .id(SharedString::from(format!("{key:?}-color")))
                            .when(!read_only, |d| d.cursor_pointer())
                            .map(|d| match (&value, label) {
                                (Some(v), Some(label)) => d
                                    .child(
                                        div()
                                            .size_3()
                                            .mr_1()
                                            .rounded(books::RADIUS)
                                            .bg(parse_hex(v).unwrap_or(white())),
                                    )
                                    .child(label),
                                _ => d.child(div().text_color(gray(400)).child(placeholder)),
                            })
                            .when(!read_only, |d| {
                                d.on_click(cx.listener(move |this, _, window, cx| {
                                    this.toggle_color(&toggle_key, window, cx)
                                }))
                            }),
                    )
                    .children(popover)
                    .into_any_element()
            }
            Some(Control::Display) | None => {
                let text = self.display(key, f, &s);
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
            }))
            .when(f.edit, |d| d.child(index_cell()));
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
                            }))
                            // `canEditRow`: the row's edit button opens its form beside this one.
                            .when(f.edit, |d| {
                                let table = f.fieldname.clone();
                                d.child(index_cell().justify_center().child(
                                    Button::new(("edit-row", i)).icon("edit").ghost().on_click(
                                        cx.listener(move |this, _, window, cx| {
                                            this.open_row(&table, i, window, cx)
                                        }),
                                    ),
                                ))
                            }),
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
const VIEW: &str = "View";

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

/// `quickEditFields`: the fields a child row's form shows.
fn quick_edit_fields(fyo: &latte_core::Fyo, table: &Field) -> Vec<Field> {
    let Some(schema) = table.target.as_deref().and_then(|t| fyo.schema(t)) else {
        return Vec::new();
    };
    schema
        .quick_edit_fields
        .iter()
        .flatten()
        .filter_map(|fieldname| schema.field(fieldname).cloned())
        .collect()
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

/// A file as `Attachment.vue` keeps it: `{name, type, data}`, the data a data URL.
fn attachment(path: &std::path::Path) -> Result<serde_json::Value, String> {
    use base64::Engine as _;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    // `accept="image/*,.pdf"`.
    let mime = match extension.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        _ => return Err(format!("{name} isn't an image or a PDF")),
    };
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let data = format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    );
    Ok(serde_json::json!({ "name": name, "type": mime, "data": data }))
}

/// A `#RRGGBB` colour.
fn parse_hex(value: &str) -> Option<Hsla> {
    let hex = value.strip_prefix('#')?;
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(hex, 16).ok().map(books::hex)
}

#[derive(Clone, Copy)]
enum Kind {
    Input,
    Text,
    Link,
    Check,
    Display,
    Image,
    Attachment,
    Color,
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
        Some(FieldType::Attachment) => Kind::Attachment,
        _ if read_only => Kind::Display,
        Some(FieldType::Color) => Kind::Color,
        Some(FieldType::Button) => Kind::Display,
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

impl FormPage {
    /// Controls drawn bare, without Books' border: in a table, a row form or a quick
    /// edit panel (`TwoColumnForm.vue` doesn't pass `border`).
    fn bare(&self, key: &Key) -> bool {
        matches!(key, Key::Cell(..)) || self.quick.is_some()
    }

    /// A control's frame (`Base.vue`'s container): bordered in the form, bare elsewhere.
    fn frame(&self, key: &Key, read_only: bool) -> Div {
        if self.bare(key) {
            div()
                .h_flex()
                .w_full()
                .min_h(px(32.))
                .px_2()
                .rounded(books::RADIUS)
                .text_13()
                .text_color(if read_only { gray(800) } else { gray(900) })
        } else {
            control_frame(read_only, false).px_3()
        }
    }

    /// `QuickEditForm.vue`: Close and Save or Submit, the title (and picture), then the
    /// fields, label beside control (`TwoColumnForm.vue`, columns 1.1 : 2).
    fn render_quick(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let doc = &self.doc;
        let action = if doc.can_save() || doc.not_inserted() {
            Some(
                Button::new("quick-save")
                    .primary()
                    .label(t("Save"))
                    .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
            )
        } else if doc.can_submit() {
            Some(
                Button::new("quick-submit")
                    .primary()
                    .label(t("Submit"))
                    .on_click(cx.listener(|this, _, window, cx| this.ask_submit(window, cx))),
            )
        } else {
            None
        };
        let toolbar = div()
            .h_flex()
            .flex_none()
            .h(books::H_PAGE_HEADER)
            .px_4()
            .justify_between()
            .child(
                Button::new("quick-close")
                    .icon("x")
                    .on_click(cx.listener(|this, _, window, cx| this.close_quick(window, cx))),
            )
            .children(action);

        let title_field = self.title_field();
        let image_field = self.image_field();
        let title_row = (title_field.is_some() || image_field.is_some()).then(|| {
            let title = title_field.as_ref().map(|f| {
                let key = Key::Field(f.fieldname.clone());
                match self.controls.get(&key) {
                    Some(Control::Input { state, .. }) => div()
                        .w_full()
                        .rounded(books::RADIUS)
                        .when(state.focus_handle(cx).is_focused(window), |d| {
                            d.bg(gray(100))
                        })
                        .child(
                            Input::new(state)
                                .appearance(false)
                                .h(px(32.))
                                .px_2()
                                .text_18()
                                .font_weight(FontWeight::SEMIBOLD)
                                .when(image_field.is_none(), |i| i.text_center()),
                        )
                        .into_any_element(),
                    _ => {
                        let s = self.session.borrow();
                        div()
                            .w_full()
                            .px_2()
                            .truncate()
                            .text_18()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(gray(800))
                            .when(image_field.is_none(), |d| d.h_flex().justify_center())
                            .child(self.display(&key, f, &s))
                            .into_any_element()
                    }
                }
            });
            let image = image_field.as_ref().map(|f| {
                let key = Key::Field(f.fieldname.clone());
                self.render_control(&key, f, window, cx)
            });
            let has_image = image.is_some();
            div()
                .h_flex()
                .flex_none()
                .border_t_1()
                .border_b_1()
                .border_color(gray(200))
                .h(if has_image { px(97.) } else { px(48.) })
                .map(|d| {
                    if has_image {
                        d.child(
                            div()
                                .flex_grow(1.1)
                                .flex_basis(px(0.))
                                .min_w_0()
                                .pl_4()
                                .children(image),
                        )
                        .child(
                            div()
                                .flex_grow(2.)
                                .flex_basis(px(0.))
                                .min_w_0()
                                .pr_4()
                                .children(title),
                        )
                    } else {
                        d.justify_center()
                            .child(div().w_full().mx_4().children(title))
                    }
                })
        });

        let fields = self.shown_fields();
        let rows: Vec<AnyElement> = fields
            .iter()
            .map(|f| {
                if f.fieldtype == Some(FieldType::Table) {
                    return div()
                        .p_4()
                        .child(self.render_table(f, window, cx))
                        .into_any_element();
                }
                let key = Key::Field(f.fieldname.clone());
                let tall = matches!(f.fieldtype, Some(FieldType::AttachImage | FieldType::Text))
                    || self.field_errors.contains_key(&key);
                div()
                    .h_flex()
                    .flex_none()
                    .min_h(if tall { px(97.) } else { px(49.) })
                    .border_b_1()
                    .border_color(gray(200))
                    .text_12()
                    .child(
                        div()
                            .flex_grow(1.1)
                            .flex_basis(px(0.))
                            .min_w_0()
                            .pl_4()
                            .text_color(gray(600))
                            .child(f.label.clone().unwrap_or_else(|| f.fieldname.clone())),
                    )
                    .child(
                        div()
                            .flex_grow(2.)
                            .flex_basis(px(0.))
                            .min_w_0()
                            .py_2()
                            .pr_4()
                            .when(f.fieldtype == Some(FieldType::AttachImage), |d| d.pl_2())
                            .child(self.render_control(&key, f, window, cx))
                            .children(self.field_errors.get(&key).map(|e| {
                                div()
                                    .mt_2()
                                    .pl_2()
                                    .text_12()
                                    .text_color(books::color("red", 600))
                                    .child(e.clone())
                            })),
                    )
                    .into_any_element()
            })
            .collect();

        div()
            .id("quick-edit")
            .track_focus(&self.focus)
            .v_flex()
            .flex_none()
            .w(rems(22.))
            .h_full()
            .bg(white())
            .border_l_1()
            .border_color(gray(200))
            .overflow_y_scroll()
            .text_13()
            .on_key_down(
                cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    this.on_key(event, window, cx)
                }),
            )
            .child(toolbar)
            .children(title_row)
            .children(rows)
            .children(self.render_dialog(window, cx))
            .into_any_element()
    }

    /// `routeToPrevious`: a saved document's changes are dropped, and the panel closes.
    fn close_quick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.doc.dirty() && self.doc.inserted() {
            let mut s = self.session.borrow_mut();
            self.doc.load(&mut s.fyo).ok();
        }
        let _ = window;
        cx.emit(QuickEvent::Close);
    }

    /// Books' shortcuts (`useDocShortcuts`, `CommonForm.vue`, `LinkedEntries.vue`,
    /// `RowEditForm.vue`, `QuickEditForm.vue`).
    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let pmod = k.modifiers.control || k.modifiers.platform;
        if !pmod && self.quick.is_none() {
            self.scan_key(&k.key.clone(), window, cx);
        }
        let handled = match k.key.as_str() {
            "s" if pmod => {
                if self.pending.is_none() && self.error.is_none() {
                    self.save_or_submit_shortcut(window, cx);
                }
                true
            }
            "backspace" if pmod && !self.embedded => {
                if self.pending.is_none() && self.error.is_none() {
                    self.commit_typed(window, cx);
                    self.cancel_or_delete_shortcut(window, cx);
                }
                true
            }
            "p" if pmod && self.quick.is_none() && !self.embedded => {
                self.open_print(cx);
                true
            }
            "l" if pmod && self.quick.is_none() && !self.embedded => {
                if self.can_show_links() || matches!(self.side, Side::Links(..)) {
                    self.toggle_links(window, cx);
                }
                true
            }
            "escape" => {
                if self.pending.is_some() || self.error.is_some() || self.menu.is_some() {
                    self.pending = None;
                    self.error = None;
                    self.menu = None;
                } else if self.picker.is_some() || self.color.is_some() {
                    self.picker = None;
                    self.color = None;
                } else if self.quick.is_some() {
                    self.close_quick(window, cx);
                } else if !matches!(self.side, Side::None) {
                    self.close_side(window, cx);
                } else {
                    return;
                }
                true
            }
            _ => false,
        };
        if handled {
            cx.stop_propagation();
            cx.notify();
        }
    }
}

impl Render for FormPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.quick.is_some() {
            return self.render_quick(window, cx);
        }
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
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        this.on_key(event, window, cx)
                    }
                }))
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

        // The form takes the focus from the desk when it opens, so its shortcuts work.
        if std::mem::take(&mut self.focus_on_open) {
            self.focus.focus(window, cx);
        }
        let buttons = self.header_buttons(window, cx);
        let left = self.header_left(window, cx);
        let header = page_header_with("", left, cx)
            .border_b_0()
            .child(div().h_flex().gap_2().children(buttons));

        // `FormContainer.vue`: the header and form, then the side panel the full height.
        let side = self
            .quick_panels
            .top()
            .map(|panel| panel.clone().into_any_element())
            .or_else(|| self.render_links(cx))
            .or_else(|| self.render_row_edit(window, cx));

        div()
            .h_flex()
            .size_full()
            .bg(gray(25))
            .text_13()
            .track_focus(&self.focus)
            .on_key_down(
                cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    this.on_key(event, window, cx)
                }),
            )
            .child(
                div()
                    .v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(header)
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .h_flex()
                            .justify_center()
                            .child(body),
                    ),
            )
            .children(side)
            .children(self.render_dialog(window, cx))
            .into_any_element()
    }
}
