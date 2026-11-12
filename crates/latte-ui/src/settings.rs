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
//! - System has Updates: whether Latte Books checks GitHub for a newer version, and Check
//!   Now (`crate::updater`).
//! - System's Version is Latte Books' own; the Frappe Books version the file is kept at is
//!   under Compatibility, at the end (`latte_core::views::field::form_layout`).
//! - An Indian company has a GST tab: the GSTIN, which Books has in General, and Return
//!   Filing, monthly or quarterly, which the GSTR reports follow. It is the GST tab of
//!   AccountingSettings, so General and GST are one document, saved together. It links to
//!   the Tax Templates, and sets up the reverse charge's accounts and templates for a
//!   company made before Latte had them (`latte_core::reports::gstr::reverse_charge`).

use std::collections::HashMap;

use crate::books::{self, Text as _, black, gray, white};
use crate::widgets::{Button, DialogButton, Kind as DialogKind, Toast, dialog, show_toast};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::t;
use latte_core::fyo::model::helpers::is_truthy;

use crate::form::FormPage;
use crate::session::Shared;
use crate::widgets::page_header;
use crate::{Nav, Navigate};
use latte_core::views::sidebar::Route;

/// A tab of Settings: its id, which a route names, the settings schema it shows, and its
/// label.
struct Tab {
    id: &'static str,
    schema: &'static str,
    label: &'static str,
}

/// The tab id of GST, which shows AccountingSettings' GST tab.
pub const GST: &str = "GST";

const TABS: [Tab; 7] = [
    Tab {
        id: "AccountingSettings",
        schema: "AccountingSettings",
        label: "General",
    },
    Tab {
        id: GST,
        schema: "AccountingSettings",
        label: "GST",
    },
    Tab {
        id: "InventorySettings",
        schema: "InventorySettings",
        label: "Inventory",
    },
    Tab {
        id: "Defaults",
        schema: "Defaults",
        label: "Defaults",
    },
    Tab {
        id: "POSSettings",
        schema: "POSSettings",
        label: "POS Settings",
    },
    Tab {
        id: "PrintSettings",
        schema: "PrintSettings",
        label: "Print",
    },
    Tab {
        id: "SystemSettings",
        schema: "SystemSettings",
        label: "System",
    },
];

/// The field whose tab of the form a Settings tab shows: GST's, or the form's first.
fn tab_field(id: &str) -> Option<&'static str> {
    (id == GST).then_some("gstin")
}

pub struct SettingsPage {
    session: Shared,
    tab: &'static str,
    forms: HashMap<&'static str, Entity<FormPage>>,
    error: Option<String>,
    /// Whether System's Advanced section is unfolded; it starts folded.
    advanced_open: bool,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Navigate> for SettingsPage {}

impl SettingsPage {
    /// Settings at the tab `tab` names (a settings schema, or GST), or at the first tab.
    pub fn new(
        session: Shared,
        tab: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut page = Self {
            session,
            tab: TABS[0].id,
            forms: HashMap::new(),
            error: None,
            advanced_open: false,
            focus: cx.focus_handle(),
            _subscriptions: Vec::new(),
        };
        let tab = page
            .tabs()
            .into_iter()
            .find(|t| Some(t.id) == tab)
            .map_or(TABS[0].id, |t| t.id);
        page.show(tab, window, cx);
        page
    }

    /// The tabs shown: GST only for an Indian company, Inventory only with inventory on,
    /// POS only with POS on.
    fn tabs(&self) -> Vec<&'static Tab> {
        let s = self.session.borrow();
        let setting =
            |schema: &str, field: &str| is_truthy(s.fyo.get_value(schema, None, field).as_ref());
        let inventory = setting("AccountingSettings", "enableInventory");
        let pos = setting("InventorySettings", "enablePointOfSale");
        let gst = s
            .fyo
            .schema("AccountingSettings")
            .is_some_and(|schema| schema.field("gstin").is_some());
        TABS.iter()
            .filter(|t| match t.id {
                GST => gst,
                "InventorySettings" => inventory,
                "POSSettings" => pos,
                _ => true,
            })
            .filter(|t| s.fyo.schema(t.schema).is_some())
            .collect()
    }

    fn show(&mut self, id: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(schema) = TABS.iter().find(|t| t.id == id).map(|t| t.schema) else {
            return;
        };
        self.tab = id;
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
        if let Some(form) = self.forms.get(schema) {
            form.update(cx, |form, cx| form.show_tab_of(tab_field(id), window, cx));
        }
        cx.notify();
    }

    /// The schema the shown tab shows.
    fn schema(&self) -> &'static str {
        TABS.iter()
            .find(|t| t.id == self.tab)
            .map_or(TABS[0].schema, |t| t.schema)
    }

    /// GST's link to the Tax Templates, which the invoices' taxes come from.
    /// Adds the reverse charge's accounts and templates the file doesn't have yet.
    fn set_up_reverse_charge(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let added = {
            let mut s = self.session.borrow_mut();
            latte_core::reports::gstr::reverse_charge::set_up(&mut s.fyo)
        };
        match added {
            Ok(added) => {
                self.error = None;
                let message = if added.is_empty() {
                    t("The reverse charge is already set up")
                } else {
                    format!("{} {}", t("Added"), added.join(", "))
                };
                show_toast(Toast::new(message, DialogKind::Success).short(), window, cx);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    fn tax_templates(&self, cx: &mut Context<Self>) -> Div {
        let set_up = {
            let s = self.session.borrow();
            latte_core::reports::gstr::reverse_charge::is_set_up(&s.fyo).unwrap_or(true)
        };
        div()
            .v_flex()
            .p_4()
            .gap_2()
            .border_t_1()
            .border_color(gray(200))
            .child(
                div()
                    .text_13()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(gray(900))
                    .child(t("Tax Templates")),
            )
            .child(div().text_12().text_color(gray(600)).child(t(
                "The rates an item's tax is worked out at: GST-18, IGST-18 and the others.",
            )))
            .child(
                div().h_flex().child(
                    Button::new("open-tax-templates")
                        .label(t("Open Tax Templates"))
                        .on_click(cx.listener(|_, _, _, cx| {
                            cx.emit(Navigate(Nav::Route(Route::list(
                                "Tax",
                                "Tax Templates",
                                None,
                            ))))
                        })),
                ),
            )
            .child(
                div()
                    .mt_2()
                    .text_13()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(gray(900))
                    .child(t("Reverse Charge")),
            )
            .child(div().text_12().text_color(gray(600)).child(if set_up {
                t("For a purchase on which you pay the GST, such as a service from abroad,                    use IGST-18-RCM (or GST-18-RCM within your state). The GST is owed on                    IGST (RCM) and claimed on IGST; GSTR3B sums it up.")
            } else {
                t("For a purchase on which you pay the GST, such as a service from abroad.                    Adds the IGST (RCM), CGST (RCM) and SGST (RCM) accounts and the                    IGST-18-RCM, GST-18-RCM, IGST-5-RCM and GST-5-RCM templates.")
            }))
            .when(!set_up, |d| {
                d.child(
                    div().h_flex().child(
                        Button::new("set-up-reverse-charge")
                            .label(t("Set Up Reverse Charge"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.set_up_reverse_charge(window, cx)
                            })),
                    ),
                )
            })
    }

    /// Not in Books: System's Updates, which Latte keeps in its config, not in the file
    /// (`crate::updater`).
    fn updates(&self, cx: &mut Context<Self>) -> Div {
        let on = crate::updater::enabled();
        let status = crate::updater::status_text(cx);
        let available = matches!(
            crate::updater::state(cx),
            crate::updater::State::Available(_)
        );
        div()
            .v_flex()
            .p_4()
            .gap_2()
            .border_t_1()
            .border_color(gray(200))
            .child(
                div()
                    .text_13()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(gray(900))
                    .child(t("Updates")),
            )
            .child(
                div()
                    .id("check-for-updates")
                    .h_flex()
                    .gap_2()
                    .cursor_pointer()
                    .child(crate::widgets::check_box(on))
                    .child(
                        div()
                            .text_13()
                            .text_color(gray(800))
                            .child(t("Check for updates")),
                    )
                    .on_click(cx.listener(move |_, _, _, cx| {
                        crate::updater::set_enabled(!on, cx);
                        cx.notify();
                    })),
            )
            .child(div().text_12().text_color(gray(600)).child(t(
                "When Latte Books starts and once a day, it asks GitHub, where its releases are, for a newer version. Nothing else is sent with it.",
            )))
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .child(
                        Button::new("check-now")
                            .label(t("Check Now"))
                            .on_click(cx.listener(|_, _, _, cx| {
                                crate::updater::check_now(cx);
                                cx.notify();
                            })),
                    )
                    .when(available, |d| {
                        d.child(
                            Button::new("update-now")
                                .label(t("Update"))
                                .on_click(|_, _, cx| crate::updater::open_dialog(cx)),
                        )
                    }),
            )
            .children(status.map(|status| div().text_12().text_color(gray(700)).child(status)))
    }

    /// Not in Books: System's Advanced, folded until its heading is clicked. It holds the
    /// usage counts checkbox, which Latte keeps in its config (`crate::telemetry`).
    fn advanced(&self, cx: &mut Context<Self>) -> Div {
        let open = self.advanced_open;
        div()
            .v_flex()
            .p_4()
            .gap_2()
            .border_t_1()
            .border_color(gray(200))
            .child(
                div()
                    .id("advanced-settings")
                    .h_flex()
                    .justify_between()
                    .cursor_pointer()
                    .child(
                        div()
                            .text_13()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(gray(900))
                            .child(t("Advanced")),
                    )
                    .child(div().text_12().text_color(gray(600)).child(if open {
                        t("Hide")
                    } else {
                        t("Show")
                    }))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.advanced_open = !this.advanced_open;
                        cx.notify();
                    })),
            )
            .when(open, |d| d.child(self.usage_counts(cx)))
    }

    fn usage_counts(&self, cx: &mut Context<Self>) -> Div {
        let on = crate::telemetry::enabled();
        div()
            .v_flex()
            .gap_1()
            .child(
                div()
                    .id("send-usage-counts")
                    .h_flex()
                    .gap_2()
                    .cursor_pointer()
                    .child(crate::widgets::check_box(on))
                    .child(
                        div()
                            .text_13()
                            .text_color(gray(800))
                            .child(t("Send anonymous usage counts")),
                    )
                    .on_click(cx.listener(move |_, _, _, cx| {
                        crate::telemetry::set_enabled(!on, cx);
                        cx.notify();
                    })),
            )
            .child(div().text_12().text_color(gray(600)).child(t(
                "The app's version, your system and the features you open. Never your company's data.",
            )))
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
            .find(|tab| tab.id == self.tab)
            .map(|tab| t(tab.label))
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
                .children(tabs.into_iter().map(|tab| {
                    let (id, active) = (tab.id, tab.id == self.tab);
                    div()
                        .id(id)
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
                        .child(t(tab.label))
                        .on_click(cx.listener(move |this, _, window, cx| this.show(id, window, cx)))
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
                    .children(self.forms.get(self.schema()).cloned())
                    .when(self.tab == GST, |d| d.child(self.tax_templates(cx)))
                    .when(self.tab == "SystemSettings", |d| {
                        d.child(self.updates(cx)).child(self.advanced(cx))
                    }),
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
