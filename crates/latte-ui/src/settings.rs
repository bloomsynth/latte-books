//! Settings: `src/pages/Settings/Settings.vue` @ v0.36.0. A tab for each settings
//! document, and one Save for all of them.
//!
//! As in Books, the tab's settings are a 600px card titled by the tab, with the tabs at
//! its foot.
//!
//! Differences from Books:
//! - Saved settings take effect at once, said in a toast; Books asks to reload the window.
//!   Ctrl+S (⌘S) saves, as in Books.
//! - There is no ERPNext Sync tab.
//! - The company's name can be changed in General; the window title and the recent files
//!   follow it.

use std::collections::HashMap;

use crate::books::{self, Text as _, black, gray, white};
use crate::widgets::{Button, DialogButton, Kind as DialogKind, Toast, dialog, show_toast};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::t;
use latte_core::fyo::model::helpers::is_truthy;

use crate::Navigate;
use crate::form::FormPage;
use crate::session::Shared;
use crate::widgets::page_header;

const TABS: [(&str, &str); 6] = [
    ("AccountingSettings", "General"),
    ("InventorySettings", "Inventory"),
    ("Defaults", "Defaults"),
    ("POSSettings", "POS Settings"),
    ("PrintSettings", "Print"),
    ("SystemSettings", "System"),
];

pub struct SettingsPage {
    session: Shared,
    tab: &'static str,
    forms: HashMap<&'static str, Entity<FormPage>>,
    error: Option<String>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Navigate> for SettingsPage {}

impl SettingsPage {
    /// Settings at the tab for `tab`, a settings schema, or at the first tab.
    pub fn new(
        session: Shared,
        tab: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut page = Self {
            session,
            tab: TABS[0].0,
            forms: HashMap::new(),
            error: None,
            focus: cx.focus_handle(),
            _subscriptions: Vec::new(),
        };
        let tab = TABS
            .iter()
            .find(|(schema, _)| Some(*schema) == tab)
            .map_or(TABS[0].0, |(schema, _)| schema);
        page.show(tab, window, cx);
        page
    }

    /// The tabs shown: Inventory only with inventory on, POS only with POS on.
    fn tabs(&self) -> Vec<(&'static str, &'static str)> {
        let s = self.session.borrow();
        let setting =
            |schema: &str, field: &str| is_truthy(s.fyo.get_value(schema, None, field).as_ref());
        let inventory = setting("AccountingSettings", "enableInventory");
        let pos = setting("InventorySettings", "enablePointOfSale");
        TABS.into_iter()
            .filter(|(schema, _)| match *schema {
                "InventorySettings" => inventory,
                "POSSettings" => pos,
                _ => true,
            })
            .filter(|(schema, _)| s.fyo.schema(schema).is_some())
            .collect()
    }

    fn show(&mut self, schema: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        self.tab = schema;
        if !self.forms.contains_key(schema) {
            match FormPage::build(&self.session, schema, None, latte_db::DocValueMap::new()) {
                Ok(doc) => {
                    let session = self.session.clone();
                    let form = cx.new(|cx| FormPage::new(session, doc, window, cx).embedded());
                    self._subscriptions.push(cx.subscribe_in(
                        &form,
                        window,
                        |_, _, event: &Navigate, _, cx| cx.emit(Navigate(event.0.clone())),
                    ));
                    self.forms.insert(schema, form);
                }
                Err(e) => self.error = Some(e),
            }
        }
        cx.notify();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let company = self.session.borrow().company_name();
        let mut errors = Vec::new();
        for form in self.forms.values() {
            form.update(cx, |form, cx| {
                if let Err(e) = form.save_if_dirty(window, cx) {
                    errors.push(e);
                }
            });
        }
        self.session.borrow_mut().refresh_formatter();
        // Dark mode, as Books shows it once the window is reloaded.
        let dark = self.session.borrow().dark_mode();
        books::set_dark(dark, cx);
        let renamed = self.session.borrow().company_name();
        if renamed != company {
            let path = self.session.borrow().path.clone();
            crate::session::remember_file(&path, &renamed);
            window.set_window_title(&format!("{renamed} - Latte Books"));
        }
        if errors.is_empty() {
            self.error = None;
            show_toast(
                Toast::new(t("Settings saved"), DialogKind::Success).short(),
                window,
                cx,
            );
        } else {
            self.error = Some(errors.join("\n"));
        }
        cx.notify();
    }

    fn can_save(&self, cx: &App) -> bool {
        self.forms.values().any(|form| form.read(cx).doc().dirty())
    }
}

impl Render for SettingsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = page_header("", cx)
            .border_b_0()
            .children(self.can_save(cx).then(|| {
                Button::new("save-settings")
                    .primary()
                    .label(t("Save"))
                    .on_click(cx.listener(|this, _, window, cx| this.save(window, cx)))
            }));
        let tabs = self.tabs();
        let title = tabs
            .iter()
            .find(|(schema, _)| *schema == self.tab)
            .map(|(_, label)| t(label))
            .unwrap_or_default();
        // `FormHeader.vue`: the tab's name, and "Settings" on the right.
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
            .child(div().text_color(black()).child(title))
            .child(div().text_color(gray(600)).child(t("Settings")));
        // `Settings.vue`'s tab bar, at the foot of the card.
        let tab_bar = (tabs.len() > 1).then(|| {
            div()
                .h_flex()
                .flex_none()
                .px_4()
                .pb_4()
                .gap_8()
                .border_t_1()
                .border_color(gray(200))
                .bg(white())
                .children(tabs.into_iter().map(|(schema, label)| {
                    let active = schema == self.tab;
                    div()
                        .id(schema)
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
                        .child(t(label))
                        .on_click(
                            cx.listener(move |this, _, window, cx| this.show(schema, window, cx)),
                        )
                }))
        });
        let card = div()
            .v_flex()
            .h_full()
            .w(books::W_FORM)
            .mx_4()
            .mb_4()
            .bg(white())
            .border_1()
            .border_color(gray(200))
            .rounded(books::RADIUS_LG)
            .shadow(books::shadow_lg())
            .overflow_hidden()
            .child(form_header)
            .child(
                div()
                    .id("settings-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(self.forms.get(self.tab).cloned()),
            )
            .children(tab_bar);

        let dialog = self.error.clone().map(|error| {
            let close = cx.listener(|this, _, _, cx| {
                this.error = None;
                cx.notify();
            });
            dialog(
                t("Error"),
                Some(error.into()),
                DialogKind::Error,
                vec![DialogButton::new(t("Okay"), false, close)],
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
                    .child(card),
            )
            .children(dialog)
            .track_focus(&self.focus)
            // `Settings.vue`: Ctrl+S (⌘S) saves, when there is something to save.
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let k = &event.keystroke;
                if k.key == "s" && (k.modifiers.control || k.modifiers.platform) {
                    if this.can_save(cx) {
                        this.save(window, cx);
                    }
                    cx.stop_propagation();
                }
            }))
    }
}

impl Focusable for SettingsPage {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
