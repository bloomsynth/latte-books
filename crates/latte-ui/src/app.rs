//! The window's root: the file selector, the setup wizard or the desk (`src/App.vue`
//! @ v0.36.0).
//!
//! As in Books, the app opens the company it last had open, unless that file is gone or
//! was never set up.

use std::path::PathBuf;

use gpui_kit::component::*;
use gpui_kit::*;

use crate::desk::{ChangeFile, Desk, Reopen};
use crate::selector::{Selected, Selector};
use crate::session::{self, Session, Shared};
use crate::setup_wizard::{SetUp, SetupWizard};
use latte_core::views::sidebar::Route;

enum Screen {
    Selector(Entity<Selector>),
    Setup(Entity<SetupWizard>),
    Desk(Entity<Desk>),
}

pub struct LatteApp {
    screen: Screen,
    _subscriptions: Vec<Subscription>,
}

impl LatteApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut app = Self {
            screen: Screen::Selector(cx.new(|cx| Selector::new(None, window, cx))),
            _subscriptions: Vec::new(),
        };
        let last = session::config_value("lastSelectedFilePath")
            .map(PathBuf::from)
            .filter(|path| path.exists());
        match last.map(|path| Session::open(&path)) {
            Some(Ok(session)) if session.setup_complete() => {
                app.show_desk(session, None, window, cx)
            }
            _ => app.show_selector(None, window, cx),
        }
        app
    }

    fn show_selector(
        &mut self,
        error: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selector = cx.new(|cx| Selector::new(error, window, cx));
        self._subscriptions = vec![cx.subscribe_in(
            &selector,
            window,
            |this, _, event: &Selected, window, cx| match event {
                Selected::Open(path) => match Session::open(path) {
                    Ok(session) if session.setup_complete() => {
                        this.show_desk(session, None, window, cx)
                    }
                    Ok(_) => this.show_selector(
                        Some(format!(
                            "{} was not set up. Make a new company instead.",
                            path.display()
                        )),
                        window,
                        cx,
                    ),
                    Err(e) => this.show_selector(Some(e), window, cx),
                },
                Selected::New => this.show_setup(window, cx),
            },
        )];
        self.screen = Screen::Selector(selector);
        cx.notify();
    }

    fn show_setup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let wizard = cx.new(|cx| SetupWizard::new(window, cx));
        self._subscriptions = vec![cx.subscribe_in(
            &wizard,
            window,
            |this, _, event: &SetUp, window, cx| match event {
                SetUp::Done(path) => match Session::open(path) {
                    Ok(session) => this.show_desk(session, None, window, cx),
                    Err(e) => this.show_selector(Some(e), window, cx),
                },
                SetUp::Back => this.show_selector(None, window, cx),
            },
        )];
        self.screen = Screen::Setup(wizard);
        cx.notify();
    }

    fn show_desk(
        &mut self,
        session: Session,
        route: Option<Route>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        session::remember_file(&session.path, &session.company_name());
        let title = format!("{} - Latte Books", session.company_name());
        window.set_window_title(&title);
        let shared: Shared = std::rc::Rc::new(std::cell::RefCell::new(session));
        let desk = cx.new(|cx| Desk::new(shared, route, window, cx));
        self._subscriptions = vec![
            cx.subscribe_in(&desk, window, |this, _, _: &ChangeFile, window, cx| {
                window.set_window_title("Latte Books");
                this.show_selector(None, window, cx)
            }),
            cx.subscribe_in(&desk, window, |this, desk, event: &Reopen, window, cx| {
                let path = desk.read(cx).path();
                match Session::open(&path) {
                    Ok(session) => this.show_desk(session, Some(event.0.clone()), window, cx),
                    Err(e) => this.show_selector(Some(e), window, cx),
                }
            }),
        ];
        self.screen = Screen::Desk(desk);
        cx.notify();
    }
}

impl Render for LatteApp {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let screen = match &self.screen {
            Screen::Selector(view) => view.clone().into_any_element(),
            Screen::Setup(view) => view.clone().into_any_element(),
            Screen::Desk(view) => view.clone().into_any_element(),
        };
        div()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_sm()
            .child(screen)
    }
}
