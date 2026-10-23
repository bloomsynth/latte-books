//! The open company: the sidebar and the page it routes to (`src/pages/Desk.vue` and
//! `src/components/Sidebar.vue` @ v0.36.0).
//!
//! Each page asks to go somewhere with a [`Navigate`] event; the desk makes the new page
//! and keeps a history for Back.
//!
//! Not in Books: every two seconds the desk looks for changes another device, such as an AI
//! assistant, wrote to the file, and shows the page again when it may show them.

use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::t;
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
use crate::session::Shared;
use crate::settings::SettingsPage;
use crate::widgets::error_message;
use crate::{Nav, Navigate};

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
    page: AnyView,
    error: Option<String>,
    /// The last change in the file's change log that the page shows.
    last_change: i64,
    _subscription: Option<Subscription>,
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
            page: cx.new(|_| Placeholder::new("", "")).into(),
            error: None,
            last_change,
            _subscription: None,
        };
        desk.open(Nav::Route(route), false, window, cx);
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
            Route::Pos | Route::Settings { .. } | Route::ImportWizard => false,
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
    }

    /// Shows a route or a new document. With `remember`, Back returns to this page.
    pub fn open(&mut self, nav: Nav, remember: bool, window: &mut Window, cx: &mut Context<Self>) {
        let session = self.session.clone();
        self.error = None;
        let route = match &nav {
            Nav::Route(route) => route.clone(),
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
                } => {
                    let page = cx.new(|cx| {
                        ListPage::new(session, &schema, &t(&title), filters.as_deref(), window, cx)
                    });
                    self.watch(page, window, cx);
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
                        let page = cx.new(|cx| ReportPage::new(session, report, window, cx));
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
                    self.watch(page, window, cx);
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
                    self.watch(page, window, cx);
                    Ok(())
                }
                Route::Print { schema, name } => {
                    let page = cx.new(|cx| PrintPage::new(session, &schema, &name, window, cx));
                    self.watch(page, window, cx);
                    Ok(())
                }
            },
        };
        match result {
            Ok(()) => {
                if remember && self.route != route {
                    self.history.push(self.route.clone());
                }
                match &route {
                    Route::Form { .. } | Route::Print { .. } => {}
                    Route::Settings { .. } => self.highlight = Route::Settings { tab: None },
                    route => self.highlight = route.clone(),
                }
                self.route = route;
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }

    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(route) = self.history.pop() {
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
        let theme = cx.theme().clone();
        let company = self.session.borrow().company_name();
        let mut children: Vec<AnyElement> = Vec::new();
        for (g, group) in self.groups.iter().enumerate() {
            let open = self.group_is_open(group);
            let active = self.is_active(&group.route) && (group.items.is_empty() || !open);
            let route = group.route.clone();
            children.push(
                div()
                    .id(("group", g))
                    .px_3()
                    .py_1p5()
                    .rounded_md()
                    .cursor_pointer()
                    .font_weight(FontWeight::MEDIUM)
                    .when(active, |d| d.bg(theme.list_active))
                    .hover(|d| d.bg(theme.list_hover))
                    .child(t(&group.label))
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
                        .pl_6()
                        .pr_3()
                        .py_1()
                        .rounded_md()
                        .cursor_pointer()
                        .when(active, |d| d.bg(theme.list_active))
                        .when(!active, |d| d.text_color(theme.muted_foreground))
                        .hover(|d| d.bg(theme.list_hover))
                        .child(t(&item.label))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open(Nav::Route(route.clone()), true, window, cx)
                        }))
                        .into_any_element(),
                );
            }
        }
        div()
            .v_flex()
            .w(px(220.))
            .h_full()
            .flex_none()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.border)
            .child(
                div()
                    .h(px(52.))
                    .px_4()
                    .h_flex()
                    .gap_1()
                    .child(
                        div()
                            .flex_1()
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(company),
                    )
                    .children((!self.history.is_empty()).then(|| {
                        Button::new("back")
                            .ghost()
                            .xsmall()
                            .icon(IconName::ArrowLeft)
                            .tooltip("Back")
                            .on_click(cx.listener(|this, _, window, cx| this.back(window, cx)))
                    })),
            )
            .child(
                div()
                    .id("sidebar-items")
                    .v_flex()
                    .flex_1()
                    .px_2()
                    .gap_0p5()
                    .overflow_y_scroll()
                    .children(children),
            )
            .child(
                div()
                    .v_flex()
                    .p_2()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(
                        Button::new("change-file")
                            .ghost()
                            .small()
                            .label(t("Change DB"))
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(ChangeFile))),
                    )
                    .child(
                        div()
                            .px_3()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(format!("Latte Books v{}", env!("CARGO_PKG_VERSION"))),
                    ),
            )
    }
}

impl Render for Desk {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Settings turn sidebar items on and off.
        self.groups = sidebar::sidebar(&self.session.borrow().fyo);
        div()
            .size_full()
            .h_flex()
            .child(self.render_sidebar(cx))
            .child(
                div()
                    .v_flex()
                    .flex_1()
                    .h_full()
                    .overflow_hidden()
                    .children(self.error.clone().map(|e| error_message(e, cx)))
                    .child(self.page.clone()),
            )
    }
}
