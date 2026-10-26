//! The open company: the sidebar and the page it routes to (`src/pages/Desk.vue` and
//! `src/components/Sidebar.vue` @ v0.36.0).
//!
//! Each page asks to go somewhere with a [`Navigate`] event; the desk makes the new page
//! and keeps a history for Back and Forward.
//!
//! Books' shortcuts work here: Ctrl+K (⌘K) searches, Shift+Backspace goes back, Shift+H
//! shows or hides the sidebar and F1 opens the documentation of the page shown; the
//! sidebar's Shortcuts lists them all.
//!
//! Not in Books: the Show Sidebar button is always shown, not only when hovered, and the
//! sidebar buttons name Shift+H in a tooltip.
//!
//! Not in Books: every two seconds the desk looks for changes another device, such as an AI
//! assistant, wrote to the file, and shows the page again when it may show them.
//!
//! Differences from Books: Shift+Backspace and Shift+H do nothing while typing in a field;
//! Books' Shift+Backspace goes back even then.

use crate::books::{self, Text as _, black, gray, pick, raw_gray, white};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::t;
use latte_core::views::search::SearchTarget;
use latte_core::views::sidebar::{self, Route, SidebarGroup};

use crate::assistants_page::AssistantsPage;
use crate::coa::ChartOfAccounts;
use crate::dashboard::DashboardPage;
use crate::form::FormPage;
use crate::get_started::GetStartedPage;
use crate::import_wizard::ImportWizard;
use crate::list_page::ListPage;
use crate::placeholder::Placeholder;
use crate::pos_page::PosPage;
use crate::print::PrintPage;
use crate::report_page::ReportPage;
use crate::report_print::ReportPrintPage;
use crate::search_bar::{SearchBar, SearchEvent};
use crate::session::Shared;
use crate::settings::SettingsPage;
use crate::shortcuts::ShortcutsHelper;
use crate::template_builder::TemplateBuilder;
use crate::widgets::{error_message, modal};
use crate::{Nav, Navigate};

/// Whether a text field has the keyboard, so a plain key is typing.
pub fn typing(window: &Window) -> bool {
    window.context_stack().iter().any(|c| c.contains("Input"))
}

/// The user asked to open another company file.
pub struct ChangeFile;

/// The company is to be opened again, at a route.
pub struct Reopen(pub Route);

pub struct Desk {
    session: Shared,
    groups: Vec<SidebarGroup>,
    route: Route,
    /// The sidebar entry shown as current: the page, or for a form the list it came from.
    highlight: Route,
    history: Vec<Route>,
    forward: Vec<Route>,
    sidebar_hidden: bool,
    /// The search window, while open (`SearchBar.vue`).
    search: Option<(Entity<SearchBar>, Subscription)>,
    /// The Shortcuts window, while open (`ShortcutsHelper.vue`).
    shortcuts: Option<Entity<ShortcutsHelper>>,
    focus: FocusHandle,
    /// Where the keyboard goes for the page's own shortcuts, for the pages with some.
    page_focus: Option<FocusHandle>,
    page: AnyView,
    error: Option<String>,
    /// The last change in the file's change log that the page shows.
    last_change: i64,
    _subscription: Option<Subscription>,
    _focus_lost: Option<Subscription>,
}

impl EventEmitter<ChangeFile> for Desk {}
impl EventEmitter<Reopen> for Desk {}

impl Desk {
    /// The desk at `route`, or at the first page of the sidebar: Get Started until it's
    /// hidden (`setDeskRoute`).
    pub fn new(
        session: Shared,
        route: Option<Route>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let groups = sidebar::sidebar(&session.borrow().fyo);
        let last_change = session.borrow().fyo.db.core().last_change_id().unwrap_or(0);
        let route =
            route.unwrap_or_else(|| groups.first().map_or(Route::Dashboard, |g| g.route.clone()));
        let mut desk = Self {
            session,
            groups,
            route: route.clone(),
            highlight: route.clone(),
            history: Vec::new(),
            forward: Vec::new(),
            sidebar_hidden: false,
            search: None,
            shortcuts: None,
            focus: cx.focus_handle(),
            page_focus: None,
            page: cx.new(|_| Placeholder::new("", "")).into(),
            error: None,
            last_change,
            _subscription: None,
            _focus_lost: None,
        };
        desk.open(Nav::Route(route), false, window, cx);
        desk.restore_focus(window, cx);
        // When what had the keyboard goes, such as a closed dialog, the page takes it back,
        // so the shortcuts keep working.
        desk._focus_lost =
            Some(cx.on_focus_lost(window, |this, window, cx| this.restore_focus(window, cx)));
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(2))
                    .await;
                if this
                    .update_in(cx, |this, window, cx| this.show_changes(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        desk
    }

    /// Shows the page again when another device changed the file. A form shows the changes
    /// only when they are to its document and it has none of its own; the pages where
    /// something is being entered (the till, Settings, the import wizard) aren't shown
    /// again.
    fn show_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let changed = {
            let session = self.session.borrow();
            match session.fyo.db.core().changes_from_others(self.last_change) {
                Ok((last, changed)) => {
                    self.last_change = last;
                    changed
                }
                Err(_) => return,
            }
        };
        if changed.is_empty() {
            return;
        }
        {
            let mut session = self.session.borrow_mut();
            session.refresh_formatter();
            self.groups = sidebar::sidebar(&session.fyo);
        }
        let show = match &self.route {
            Route::Form { .. } => {
                self.page
                    .clone()
                    .downcast::<FormPage>()
                    .ok()
                    .is_some_and(|form| {
                        let doc = form.read(cx).doc();
                        !doc.not_inserted()
                            && !doc.dirty()
                            && doc.name().is_some_and(|name| {
                                changed
                                    .iter()
                                    .any(|(s, n)| s == doc.schema_name() && n == name)
                            })
                    })
            }
            Route::Print { schema, name } => changed.iter().any(|(s, n)| s == schema && n == name),
            Route::Pos
            | Route::Settings { .. }
            | Route::ImportWizard
            | Route::TemplateBuilder { .. }
            | Route::ReportPrint(_) => false,
            _ => true,
        };
        if !show {
            cx.notify();
            return;
        }
        let route = match &self.route {
            // A new document that's since been saved is opened by its name.
            Route::Form { schema, .. } => {
                let form = self.page.clone().downcast::<FormPage>().ok();
                Route::Form {
                    schema: schema.clone(),
                    name: form.and_then(|f| f.read(cx).doc().name().map(str::to_owned)),
                }
            }
            route => route.clone(),
        };
        self.open(Nav::Route(route), false, window, cx);
    }

    /// The company file.
    pub fn path(&self) -> std::path::PathBuf {
        self.session.borrow().path.clone()
    }

    fn watch<V: Render + EventEmitter<Navigate>>(
        &mut self,
        page: Entity<V>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._subscription = Some(cx.subscribe_in(
            &page,
            window,
            |this, _, event: &Navigate, window, cx| this.open(event.0.clone(), true, window, cx),
        ));
        self.page = page.into();
        self.page_focus = None;
    }

    /// Gives the keyboard to the page, for its shortcuts, or to the desk.
    fn restore_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match &self.page_focus {
            Some(focus) => focus.focus(window, cx),
            None => self.focus.focus(window, cx),
        }
    }

    /// `openDocumentation`: the documentation of the page shown (`docsPathRef`).
    fn open_docs(&self, cx: &mut App) {
        cx.open_url(&latte_core::views::docs::docs_url(&self.route));
    }

    /// Shows a route or a new document. With `remember`, Back returns to this page.
    pub fn open(&mut self, nav: Nav, remember: bool, window: &mut Window, cx: &mut Context<Self>) {
        let session = self.session.clone();
        self.error = None;
        // A PrintTemplate opens in the Template Builder (`formRoute`).
        let nav = match nav {
            Nav::Route(Route::Form { schema, name }) if schema == "PrintTemplate" => {
                Nav::Route(Route::TemplateBuilder { name })
            }
            nav => nav,
        };
        let route = match &nav {
            Nav::Route(route) => route.clone(),
            Nav::Doc(doc) if doc.schema_name() == "PrintTemplate" => {
                Route::TemplateBuilder { name: None }
            }
            Nav::Doc(doc) => Route::Form {
                schema: doc.schema_name().to_owned(),
                name: None,
            },
            Nav::Reopen(route) => {
                cx.emit(Reopen(route.clone()));
                return;
            }
        };
        let result: Result<(), String> = match nav {
            Nav::Reopen(_) => Ok(()),
            Nav::Doc(doc) if doc.schema_name() == "PrintTemplate" => {
                let page = cx.new(|cx| TemplateBuilder::new(session, doc, window, cx));
                self.watch(page, window, cx);
                Ok(())
            }
            Nav::Doc(doc) => {
                let page = cx.new(|cx| FormPage::new(session, doc, window, cx));
                self.watch(page, window, cx);
                Ok(())
            }
            Nav::Route(route) => match route {
                Route::List {
                    schema,
                    title,
                    filters,
                    conditions,
                } => {
                    let page = cx.new(|cx| {
                        ListPage::new(
                            session,
                            &schema,
                            &t(&title),
                            filters.as_deref(),
                            conditions,
                            window,
                            cx,
                        )
                    });
                    self.watch(page.clone(), window, cx);
                    self.page_focus = Some(page.focus_handle(cx));
                    Ok(())
                }
                Route::Form { schema, name } => {
                    let values = latte_db::DocValueMap::new();
                    match FormPage::build(&session, &schema, name.as_deref(), values) {
                        Ok(doc) => {
                            let page = cx.new(|cx| FormPage::new(session, doc, window, cx));
                            self.watch(page, window, cx);
                            Ok(())
                        }
                        Err(e) => Err(e),
                    }
                }
                Route::Report(name) => match ReportPage::build(&name) {
                    Some(report) => {
                        let page = cx.new(|cx| ReportPage::new(session, report, &[], window, cx));
                        self.watch(page, window, cx);
                        Ok(())
                    }
                    None => {
                        let page = cx.new(|_| {
                            Placeholder::new(&name, "This report isn't in Latte Books yet.")
                        });
                        self.watch(page, window, cx);
                        Ok(())
                    }
                },
                Route::FilteredReport { name, filters } => match ReportPage::build(&name) {
                    Some(report) => {
                        let page =
                            cx.new(|cx| ReportPage::new(session, report, &filters, window, cx));
                        self.watch(page, window, cx);
                        Ok(())
                    }
                    None => {
                        let page = cx.new(|_| {
                            Placeholder::new(&name, "This report isn't in Latte Books yet.")
                        });
                        self.watch(page, window, cx);
                        Ok(())
                    }
                },
                Route::ChartOfAccounts => {
                    let page = cx.new(|cx| ChartOfAccounts::new(session, window, cx));
                    self.watch(page, window, cx);
                    Ok(())
                }
                Route::GetStarted => {
                    let page = cx.new(|cx| GetStartedPage::new(session, window, cx));
                    self.watch(page, window, cx);
                    Ok(())
                }
                Route::Dashboard => {
                    let page = cx.new(|cx| DashboardPage::new(session, window, cx));
                    self.watch(page, window, cx);
                    Ok(())
                }
                Route::ImportWizard => {
                    let page = cx.new(|cx| ImportWizard::new(session, window, cx));
                    self.watch(page.clone(), window, cx);
                    self.page_focus = Some(page.focus_handle(cx));
                    Ok(())
                }
                Route::AiAssistants => {
                    let page = cx.new(|cx| AssistantsPage::new(session, window, cx));
                    self.watch(page, window, cx);
                    Ok(())
                }
                Route::Pos => {
                    let page = cx.new(|cx| PosPage::new(session, window, cx));
                    self.watch(page, window, cx);
                    Ok(())
                }
                Route::Settings { tab } => {
                    let page = cx.new(|cx| SettingsPage::new(session, tab.as_deref(), window, cx));
                    self.watch(page.clone(), window, cx);
                    self.page_focus = Some(page.focus_handle(cx));
                    Ok(())
                }
                Route::Print { schema, name } => {
                    let page = cx.new(|cx| PrintPage::new(session, &schema, &name, window, cx));
                    self.watch(page, window, cx);
                    Ok(())
                }
                Route::TemplateBuilder { name } => {
                    match TemplateBuilder::build(&session, name.as_deref()) {
                        Ok(doc) => {
                            let page = cx.new(|cx| TemplateBuilder::new(session, doc, window, cx));
                            self.watch(page, window, cx);
                            Ok(())
                        }
                        Err(e) => Err(e),
                    }
                }
                Route::ReportPrint(name) => match ReportPrintPage::snapshot(&session, &name, cx) {
                    Ok(report) => {
                        let page = cx.new(|cx| ReportPrintPage::new(session, report, window, cx));
                        self.watch(page, window, cx);
                        Ok(())
                    }
                    Err(e) => Err(e),
                },
            },
        };
        match result {
            Ok(()) => {
                if remember && self.route != route {
                    self.history.push(self.route.clone());
                    self.forward.clear();
                }
                match &route {
                    Route::Form { .. }
                    | Route::Print { .. }
                    | Route::TemplateBuilder { .. }
                    | Route::ReportPrint(_) => {}
                    Route::Settings { .. } => self.highlight = Route::Settings { tab: None },
                    Route::FilteredReport { name, .. } => {
                        self.highlight = Route::Report(name.clone())
                    }
                    route => self.highlight = route.clone(),
                }
                // POS.vue hides the sidebar while the till is open, and shows it on leaving.
                if route == Route::Pos {
                    self.sidebar_hidden = true;
                } else if self.route == Route::Pos {
                    self.sidebar_hidden = false;
                }
                self.route = route;
                self.restore_focus(window, cx);
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }

    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(route) = self.history.pop() {
            self.forward.push(self.route.clone());
            self.open(Nav::Route(route), false, window, cx);
        }
    }

    /// Whether a sidebar entry is the page shown, as `isItemActive` decides.
    fn is_active(&self, route: &Route) -> bool {
        &self.highlight == route
    }

    fn group_is_open(&self, group: &SidebarGroup) -> bool {
        self.is_active(&group.route) || group.items.iter().any(|item| self.is_active(&item.route))
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let company = self.session.borrow().company_name();
        // `hover:bg-gray-100 dark:hover:bg-gray-875`, and the current entry's
        // `border-gray-800 dark:border-gray-100`.
        let hover = pick(raw_gray(100), raw_gray(875));
        let mark = pick(raw_gray(800), raw_gray(100));
        let mut children: Vec<AnyElement> = Vec::new();
        for (g, group) in self.groups.iter().enumerate() {
            let open = self.group_is_open(group);
            // `isGroupActive(group) && !group.items`: the row itself is the page.
            let marked = open && group.items.is_empty();
            let route = group.route.clone();
            children.push(
                div()
                    .id(("group", g))
                    .h_flex()
                    .h_10()
                    .px_4()
                    .cursor_pointer()
                    .hover(|d| d.bg(hover))
                    .when(marked, |d| {
                        d.bg(hover).border_l_4().border_color(mark).pl(px(12.))
                    })
                    .child(
                        img(books::sidebar_icon(group.icon, open))
                            .size_5()
                            .flex_none(),
                    )
                    .child(
                        div()
                            .ml_2()
                            .text_14()
                            .text_color(if marked {
                                pick(raw_gray(900), raw_gray(25))
                            } else {
                                pick(raw_gray(700), raw_gray(300))
                            })
                            .child(t(&group.label)),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open(Nav::Route(route.clone()), true, window, cx)
                    }))
                    .into_any_element(),
            );
            if !open {
                continue;
            }
            for (i, item) in group.items.iter().enumerate() {
                let active = self.is_active(&item.route);
                let route = item.route.clone();
                children.push(
                    div()
                        .id(("item", g * 100 + i))
                        .h_flex()
                        .h_10()
                        .pl_10()
                        .text_13()
                        .cursor_pointer()
                        .hover(|d| d.bg(hover))
                        .when(active, |d| {
                            d.bg(hover)
                                .text_color(pick(raw_gray(900), raw_gray(100)))
                                .border_l_4()
                                .border_color(mark)
                                .pl(px(36.))
                        })
                        .when(!active, |d| {
                            d.text_color(pick(raw_gray(700), raw_gray(400)))
                        })
                        .child(t(&item.label))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open(Nav::Route(route.clone()), true, window, cx)
                        }))
                        .into_any_element(),
                );
            }
        }
        // `text-gray-600 dark:text-gray-500 hover:text-gray-800 dark:hover:text-gray-400`.
        let (link_color, link_hover) = (
            pick(raw_gray(600), raw_gray(500)),
            pick(raw_gray(800), raw_gray(400)),
        );
        let link = |id: &'static str, icon: &'static str, label: &str| {
            div()
                .id(id)
                .h_flex()
                .gap_1()
                .text_12()
                .text_color(link_color)
                .cursor_pointer()
                .hover(|d| d.text_color(link_hover))
                .group(id)
                .child(
                    books::icon(icon, px(16.), link_color)
                        .group_hover(id, |s| s.text_color(link_hover)),
                )
                .child(t(label))
        };
        div()
            .v_flex()
            .relative()
            .w(books::W_SIDEBAR)
            .h_full()
            .flex_none()
            .py_2()
            .justify_between()
            .bg(pick(raw_gray(25), raw_gray(900)))
            .border_r_1()
            .border_color(gray(200))
            .child(
                div()
                    .v_flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .px_4()
                            .mt_2()
                            .mb_4()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(pick(black(), raw_gray(200)))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .child(company),
                    )
                    .child(
                        div()
                            .id("sidebar-items")
                            .v_flex()
                            .flex_1()
                            .overflow_y_scroll()
                            .children(children),
                    ),
            )
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .py_2()
                    .px_4()
                    .child(
                        link("help", "help-circle", "Help")
                            .on_click(cx.listener(|this, _, _, cx| this.open_docs(cx))),
                    )
                    .child(
                        link("shortcuts", "command", "Shortcuts").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.shortcuts = Some(cx.new(|_| ShortcutsHelper::new()));
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        link("change-db", "database", "Change DB")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(ChangeFile))),
                    )
                    .child(
                        link("report-issue", "flag", "Report Issue").on_click(|_, _, cx| {
                            let info = crate::widgets::report_info();
                            cx.open_url(&latte_core::views::report::issue_url(None, &info))
                        }),
                    ),
            )
            .child(
                div()
                    .id("hide-sidebar")
                    .absolute()
                    .bottom_0()
                    .right_0()
                    .m_4()
                    .p_1()
                    .rounded(books::RADIUS)
                    .cursor_pointer()
                    .hover(|d| d.bg(hover))
                    .tooltip(sidebar_tooltip)
                    .child(books::icon("chevrons-left", px(16.), link_color))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.sidebar_hidden = true;
                        cx.notify();
                    })),
            )
    }
}

/// The sidebar buttons' tooltip, which names the shortcut.
fn sidebar_tooltip(window: &mut Window, cx: &mut App) -> AnyView {
    Tooltip::new(format!("{} (Shift+H)", t("Toggle sidebar"))).build(window, cx)
}

/// Hides or shows the sidebar: the Template Builder's edit mode hides it.
pub fn set_sidebar_hidden(hidden: bool, cx: &mut App) {
    if let Some(desk) = cx.try_global::<NavState>().and_then(|n| n.desk.upgrade()) {
        desk.update(cx, |desk, cx| {
            desk.sidebar_hidden = hidden;
            cx.notify();
        });
    }
}

/// What the page headers' back and forward buttons need from the desk
/// (`PageHeaderNavGroup.vue`).
pub struct NavState {
    pub desk: WeakEntity<Desk>,
    pub can_back: bool,
    pub can_forward: bool,
}

impl Global for NavState {}

/// Opens a route from outside a page, such as a toast's action: Books' "View Accounting
/// Entries" after a submit (`getSubmitSuccessToastAction`) opens the General Ledger so.
#[allow(dead_code)]
pub fn navigate(route: Route, window: &mut Window, cx: &mut App) {
    if let Some(desk) = cx.try_global::<NavState>().and_then(|n| n.desk.upgrade()) {
        desk.update(cx, |desk, cx| {
            desk.open(Nav::Route(route), true, window, cx)
        });
    }
}

impl Desk {
    pub fn go_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.back(window, cx);
    }

    /// Opens the search window, with the index made afresh.
    pub fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let bar = cx.new(|cx| SearchBar::new(&self.session, window, cx));
        let subscription =
            cx.subscribe_in(&bar, window, |this, _, event: &SearchEvent, window, cx| {
                this.search = None;
                if let SearchEvent::Open(target) = event {
                    match target {
                        SearchTarget::Route(route) => {
                            this.open(Nav::Route(route.clone()), true, window, cx)
                        }
                        SearchTarget::NewDoc { schema, values } => {
                            let doc = this
                                .session
                                .borrow()
                                .fyo
                                .get_new_doc(schema, values.clone());
                            match doc {
                                Ok(doc) => this.open(Nav::Doc(doc), true, window, cx),
                                Err(e) => this.error = Some(e.to_string()),
                            }
                        }
                    }
                }
                this.restore_focus(window, cx);
                cx.notify();
            });
        self.search = Some((bar, subscription));
        cx.notify();
    }

    pub fn go_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(route) = self.forward.pop() {
            self.history.push(self.route.clone());
            self.open(Nav::Route(route), false, window, cx);
        }
    }
}

impl Render for Desk {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.groups = sidebar::sidebar(&self.session.borrow().fyo);
        cx.set_global(NavState {
            desk: cx.entity().downgrade(),
            can_back: !self.history.is_empty(),
            can_forward: !self.forward.is_empty(),
        });
        div()
            .size_full()
            .h_flex()
            .relative()
            .track_focus(&self.focus)
            // The global shortcuts (`ShortcutsHelper.vue`'s Global group).
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let k = &event.keystroke;
                let pmod = k.modifiers.control || k.modifiers.platform;
                let plain_shift = k.modifiers.shift && !pmod && !k.modifiers.alt;
                let open_modal = this.search.is_some() || this.shortcuts.is_some();
                if k.key == "k" && pmod && !open_modal {
                    // `SearchBar.vue`: Ctrl+K, ⌘K on a Mac.
                    this.open_search(window, cx);
                } else if k.key == "backspace" && plain_shift && !typing(window) && !open_modal {
                    // `PageHeaderNavGroup.vue`: Shift+Backspace goes back.
                    this.back(window, cx);
                } else if k.key == "h" && plain_shift && !typing(window) && !open_modal {
                    // `Sidebar.vue`: Shift+H, unless typing.
                    this.sidebar_hidden = !this.sidebar_hidden;
                    cx.notify();
                } else if k.key == "f1" {
                    this.open_docs(cx);
                } else {
                    return;
                }
                cx.stop_propagation();
            }))
            .children(self.search.as_ref().map(|(bar, _)| {
                modal(
                    bar.clone(),
                    cx.listener(|this, _, window, cx| {
                        this.search = None;
                        this.restore_focus(window, cx);
                        cx.notify();
                    }),
                    window,
                )
            }))
            .children(self.shortcuts.as_ref().map(|helper| {
                modal(
                    helper.clone(),
                    cx.listener(|this, _, window, cx| {
                        this.shortcuts = None;
                        this.restore_focus(window, cx);
                        cx.notify();
                    }),
                    window,
                )
            }))
            .when(!self.sidebar_hidden, |d| d.child(self.render_sidebar(cx)))
            .child(
                div()
                    .v_flex()
                    .flex_1()
                    .h_full()
                    .overflow_hidden()
                    .bg(white())
                    .children(self.error.clone().map(|e| error_message(e, cx)))
                    .child(self.page.clone()),
            )
            .when(self.sidebar_hidden, |d| {
                // Desk.vue's Show Sidebar button, which Books leaves unseen until hovered;
                // here it is always shown, so a hidden sidebar can be found again.
                d.child(
                    div()
                        .id("show-sidebar")
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .m_4()
                        .p_1()
                        .rounded(books::RADIUS)
                        .border_1()
                        .border_color(gray(200))
                        .bg(white())
                        .text_color(gray(600))
                        .cursor_pointer()
                        .hover(|d| d.bg(gray(100)).shadow_md())
                        .tooltip(sidebar_tooltip)
                        .child(books::icon("chevrons-right", px(16.), gray(600)))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.sidebar_hidden = false;
                            cx.notify();
                        })),
                )
            })
    }
}
