//! Settings: `src/pages/Settings/Settings.vue` @ v0.36.0. A tab for each settings
//! document, and one Save for all of them.
//!
//! Differences from Books:
//! - Saved settings take effect at once; Books asks to reload the window.
//! - There is no ERPNext Sync tab.

use std::collections::HashMap;

use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::t;
use latte_core::fyo::model::helpers::is_truthy;

use crate::Navigate;
use crate::form::FormPage;
use crate::session::Shared;
use crate::widgets::{error_message, info_message, page_header};

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
    info: Option<String>,
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
            info: None,
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
        let mut errors = Vec::new();
        for form in self.forms.values() {
            form.update(cx, |form, cx| {
                if let Err(e) = form.save_if_dirty(window, cx) {
                    errors.push(e);
                }
            });
        }
        self.session.borrow_mut().refresh_formatter();
        if errors.is_empty() {
            self.error = None;
            self.info = Some("Settings saved".to_owned());
        } else {
            self.error = Some(errors.join("\n"));
            self.info = None;
        }
        cx.notify();
    }

    fn can_save(&self, cx: &App) -> bool {
        self.forms.values().any(|form| form.read(cx).doc().dirty())
    }
}

impl Render for SettingsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let header = page_header(t("Settings"), cx).children(self.can_save(cx).then(|| {
            Button::new("save-settings")
                .small()
                .primary()
                .label(t("Save"))
                .on_click(cx.listener(|this, _, window, cx| this.save(window, cx)))
        }));
        let tabs = self.tabs();
        let tab_bar = div()
            .h_flex()
            .px_4()
            .gap_4()
            .border_b_1()
            .border_color(theme.border)
            .children(tabs.into_iter().map(|(schema, label)| {
                let active = schema == self.tab;
                div()
                    .id(schema)
                    .py_2()
                    .cursor_pointer()
                    .when(active, |d| {
                        d.border_b_2()
                            .border_color(theme.primary)
                            .font_weight(FontWeight::SEMIBOLD)
                    })
                    .when(!active, |d| d.text_color(theme.muted_foreground))
                    .child(label)
                    .on_click(cx.listener(move |this, _, window, cx| this.show(schema, window, cx)))
            }));
        div()
            .v_flex()
            .size_full()
            .child(header)
            .children(self.error.clone().map(|e| error_message(e, cx)))
            .children(self.info.clone().map(|m| info_message(m, cx)))
            .child(tab_bar)
            .children(self.forms.get(self.tab).cloned())
    }
}
