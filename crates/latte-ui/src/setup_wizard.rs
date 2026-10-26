//! Setting up a new company: `src/pages/SetupWizard/SetupWizard.vue` and the `SetupWizard`
//! schema @ v0.36.0.
//!
//! The company is saved as `<Company Name>.books.db` in `Documents\Latte Books`, as Books
//! saves it in `Documents\Frappe Books`.
//!
//! Differences from Books:
//! - Dates are typed as YYYY-MM-DD.
//! - When a file of that name exists, a new file with the time in its name is made. Books
//!   asks whether to overwrite the existing file.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::books::{self, Text as _, black, gray, white};
use crate::widgets::{Button, control_frame, mandatory_mark};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::t;
use jiff::civil::Date;
use latte_core::setup::{self, COA_LIST, SetupWizardOptions};

use crate::attach_image;
use crate::link::{LinkInput, LinkOption, LinkPicked};
use crate::session;
use crate::widgets::{error_message, field_label};

pub enum SetUp {
    Done(PathBuf),
    Back,
}

const TEXT_FIELDS: [(&str, &str, &str); 4] = [
    ("companyName", "Company Name", "Company Name"),
    ("fullname", "Full Name", "John Doe"),
    ("email", "Email", "john@doe.com"),
    ("bankName", "Bank Name", "Prime Bank"),
];

pub struct SetupWizard {
    options: SetupWizardOptions,
    texts: HashMap<&'static str, Entity<InputState>>,
    country: Entity<LinkInput>,
    currency: Entity<LinkInput>,
    chart: Entity<LinkInput>,
    start: Entity<InputState>,
    end: Entity<InputState>,
    error: Option<String>,
    /// The company logo picked, and its picture.
    logo: Option<(String, std::sync::Arc<Image>)>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SetUp> for SetupWizard {}

fn options(values: impl IntoIterator<Item = String>) -> Vec<LinkOption> {
    values
        .into_iter()
        .map(|value| LinkOption {
            value: value.clone().into(),
            label: value.into(),
        })
        .collect()
}

fn date_text(date: Date) -> String {
    if date.year() <= 1 {
        String::new()
    } else {
        date.to_string()
    }
}

impl SetupWizard {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut subscriptions = Vec::new();
        let mut texts = HashMap::new();
        for (fieldname, _, placeholder) in TEXT_FIELDS {
            let state = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
            // Submit is offered once every value is filled (`areAllValuesFilled`).
            subscriptions.push(cx.subscribe_in(
                &state,
                window,
                |_, _, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                },
            ));
            texts.insert(fieldname, state);
        }

        let mut countries: Vec<String> = setup::country_info().keys().cloned().collect();
        countries.sort();
        let country = cx.new(|cx| LinkInput::new(options(countries), "Select Country", window, cx));
        subscriptions.push(cx.subscribe_in(
            &country,
            window,
            |this, _, event: &LinkPicked, window, cx| this.set_country(&event.0, window, cx),
        ));

        let mut currencies: Vec<String> = setup::currency_list()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        currencies.sort();
        currencies.dedup();
        let currency = cx.new(|cx| LinkInput::new(options(currencies), "Currency", window, cx));
        subscriptions.push(cx.subscribe_in(
            &currency,
            window,
            |this, _, event: &LinkPicked, _, cx| {
                this.options.currency = event.0.to_string();
                cx.notify();
            },
        ));

        let charts = COA_LIST.iter().map(|(name, _)| (*name).to_owned());
        let chart = cx.new(|cx| LinkInput::new(options(charts), "Select CoA", window, cx));
        subscriptions.push(cx.subscribe_in(
            &chart,
            window,
            |this, _, event: &LinkPicked, _, cx| {
                this.options.chart_of_accounts = event.0.to_string();
                cx.notify();
            },
        ));

        let start = cx.new(|cx| InputState::new(window, cx).placeholder("Fiscal Year Start Date"));
        let end = cx.new(|cx| InputState::new(window, cx).placeholder("Fiscal Year End Date"));
        for state in [&start, &end] {
            subscriptions.push(cx.subscribe_in(
                state,
                window,
                |this, _, event: &InputEvent, _, cx| match event {
                    InputEvent::Blur => {
                        let _ = this.read_dates(cx);
                    }
                    InputEvent::Change => cx.notify(),
                    _ => {}
                },
            ));
        }

        Self {
            options: SetupWizardOptions::for_country("", setup::today()),
            texts,
            country,
            currency,
            chart,
            start,
            end,
            error: None,
            logo: None,
            _subscriptions: subscriptions,
        }
    }

    /// The wizard's formulas: a country sets the currency, the chart and the fiscal year.
    fn set_country(&mut self, country: &str, window: &mut Window, cx: &mut Context<Self>) {
        let mut options = SetupWizardOptions::for_country(country, setup::today());
        for (fieldname, ..) in TEXT_FIELDS {
            let value = self.texts[fieldname].read(cx).value().to_string();
            match fieldname {
                "companyName" => options.company_name = value,
                "fullname" => options.fullname = value,
                "email" => options.email = value,
                _ => options.bank_name = value,
            }
        }
        let currency = (!options.currency.is_empty()).then(|| options.currency.clone().into());
        self.currency
            .update(cx, |link, cx| link.set_value(currency, window, cx));
        let chart = (!options.chart_of_accounts.is_empty())
            .then(|| options.chart_of_accounts.clone().into());
        self.chart
            .update(cx, |link, cx| link.set_value(chart, window, cx));
        let (start, end) = (
            date_text(options.fiscal_year_start),
            date_text(options.fiscal_year_end),
        );
        self.start
            .update(cx, |input, cx| input.set_value(start, window, cx));
        self.end
            .update(cx, |input, cx| input.set_value(end, window, cx));
        self.options = options;
        cx.notify();
    }

    fn read_dates(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        let parse = |state: &Entity<InputState>, label: &str| {
            let text = state.read(cx).value().trim().to_owned();
            text.parse::<Date>()
                .map_err(|_| format!("{label}: \"{text}\" is not a date like 2026-04-01"))
        };
        let start = parse(&self.start, "Fiscal Year Start Date");
        let end = parse(&self.end, "Fiscal Year End Date");
        if let Ok(start) = start {
            self.options.fiscal_year_start = start;
        }
        if let Ok(end) = end {
            self.options.fiscal_year_end = end;
        }
        start.and(end).map(|_| ())
    }

    /// The logo's upload, or its clear when there is one (`AttachImage.vue`).
    fn pick_logo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.logo.take().is_some() {
            cx.notify();
            return;
        }
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
            let url = attach_image::data_url(&path);
            this.update(cx, |this, cx| {
                match url.and_then(|url| {
                    let image = attach_image::image_from_data_url(&url)
                        .ok_or_else(|| format!("{} can't be read", path.display()))?;
                    Ok((url, std::sync::Arc::new(image)))
                }) {
                    Ok(logo) => this.logo = Some(logo),
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        self.options.logo = self.logo.as_ref().map(|(url, _)| url.clone());
        let text = |fieldname: &str| self.texts[fieldname].read(cx).value().trim().to_owned();
        self.options.company_name = text("companyName");
        self.options.fullname = text("fullname");
        self.options.email = text("email");
        self.options.bank_name = text("bankName");
        self.options.country = self
            .country
            .read(cx)
            .value()
            .map(|v| v.to_string())
            .unwrap_or_default();

        let missing: Vec<&str> = [
            ("Company Name", self.options.company_name.is_empty()),
            ("Full Name", self.options.fullname.is_empty()),
            ("Email", self.options.email.is_empty()),
            ("Country", self.options.country.is_empty()),
            ("Currency", self.options.currency.is_empty()),
            ("Bank Name", self.options.bank_name.is_empty()),
            (
                "Chart of Accounts",
                self.options.chart_of_accounts.is_empty(),
            ),
        ]
        .into_iter()
        .filter_map(|(label, missing)| missing.then_some(label))
        .collect();
        let result = if !missing.is_empty() {
            Err(format!("Please fill in {}", missing.join(", ")))
        } else if !is_email(&self.options.email) {
            Err(format!("Invalid email: {}", self.options.email))
        } else {
            self.read_dates(cx)
        };
        if let Err(e) = result {
            self.error = Some(e);
            cx.notify();
            return;
        }

        let path = match new_file_path(&self.options.company_name) {
            Ok(path) => path,
            Err(e) => {
                self.error = Some(e);
                cx.notify();
                return;
            }
        };
        match setup::setup_instance(&path, &self.options, &session::device_id()) {
            Ok(_) => cx.emit(SetUp::Done(path)),
            Err(e) => {
                let _ = std::fs::remove_file(&path);
                self.error = Some(e.to_string());
                cx.notify();
            }
        }
    }
}

/// Books' `email` validation (`fyo/model/validationFunction.ts`).
fn is_email(email: &str) -> bool {
    let Some((user, domain)) = email.split_once('@') else {
        return false;
    };
    !user.is_empty()
        && !domain.contains('@')
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !email.contains(char::is_whitespace)
}

/// Where Books' `getDbDefaultPath` would put the company, in a `Latte Books` folder.
fn new_file_path(company_name: &str) -> Result<PathBuf, String> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or("Can't find the Documents folder")?;
    let dir = home.join("Documents").join("Latte Books");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file_name: String = company_name
        .chars()
        .map(|c| if r#"\/:*?"<>|"#.contains(c) { '-' } else { c })
        .collect();
    let path = dir.join(format!("{file_name}.books.db"));
    if !path.exists() {
        return Ok(path);
    }
    let stamp = jiff::Zoned::now().strftime("%Y%m%d%H%M%S").to_string();
    Ok(dir.join(format!("{file_name}_{stamp}.books.db")))
}

impl Render for SetupWizard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A required field: its label, then its control with Books' red mark while empty.
        let labelled = |label: &str, control: AnyElement, empty: bool, cx: &App| {
            div()
                .v_flex()
                .mb_auto()
                .child(field_label(label.to_owned(), true, cx))
                .child(
                    div()
                        .relative()
                        .child(control)
                        .when(empty, |d| d.child(mandatory_mark())),
                )
                .into_any_element()
        };
        let text = |state: &Entity<InputState>, window: &Window, cx: &App| {
            let focused = state.focus_handle(cx).is_focused(window);
            control_frame(false, focused)
                .child(
                    Input::new(state)
                        .appearance(false)
                        .h(px(32.))
                        .px_3()
                        .text_13(),
                )
                .into_any_element()
        };
        let empty = |state: &Entity<InputState>, cx: &App| state.read(cx).value().trim().is_empty();
        let picked = |link: &Entity<LinkInput>, cx: &App| link.read(cx).value().is_some();
        for link in [&self.country, &self.currency, &self.chart] {
            let mandatory = !picked(link, cx);
            link.update(cx, |link, _| link.set_mandatory(mandatory));
        }
        let filled = TEXT_FIELDS.iter().all(|(f, ..)| !empty(&self.texts[f], cx))
            && [&self.country, &self.currency, &self.chart]
                .into_iter()
                .all(|l| picked(l, cx))
            && !empty(&self.start, cx)
            && !empty(&self.end, cx);

        let field = |name: &str, window: &Window, cx: &App| {
            let state = &self.texts[name];
            let label = TEXT_FIELDS
                .iter()
                .find(|(f, ..)| *f == name)
                .map_or("", |(_, l, _)| l);
            labelled(label, text(state, window, cx), empty(state, cx), cx)
        };
        // `CommonFormSection.vue` for the schema's sections: Default, Locale, Accounting.
        let section = |title: Option<&str>, fields: Vec<AnyElement>, first: bool| {
            div()
                .v_flex()
                .p_4()
                .when(!first, |d| d.border_t_1().border_color(gray(200)))
                .children(title.map(|title| {
                    div()
                        .mb_4()
                        .text_13()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(t(title))
                }))
                .child(div().grid().grid_cols(2).gap_4().gap_x_8().children(fields))
        };
        // `logo` first, as `CommonFormSection.vue` gives an AttachImage two rows.
        let logo = attach_image::attach_image(
            "logo",
            self.logo.as_ref().map(|(_, image)| image.clone()),
            cx.listener(|this, _, window, cx| this.pick_logo(window, cx)),
        );
        let default = vec![
            div().row_span(2).mb_auto().child(logo).into_any_element(),
            field("companyName", window, cx),
            field("fullname", window, cx),
            field("email", window, cx),
        ];
        let locale = vec![
            labelled(
                "Country",
                self.country.clone().into_any_element(),
                false,
                cx,
            ),
            labelled(
                "Currency",
                self.currency.clone().into_any_element(),
                false,
                cx,
            ),
        ];
        let accounting = vec![
            field("bankName", window, cx),
            labelled(
                "Chart of Accounts",
                self.chart.clone().into_any_element(),
                false,
                cx,
            ),
            labelled(
                "Fiscal Year Start Date",
                text(&self.start, window, cx),
                empty(&self.start, cx),
                cx,
            ),
            labelled(
                "Fiscal Year End Date",
                text(&self.end, window, cx),
                empty(&self.end, cx),
                cx,
            ),
        ];

        div()
            .size_full()
            .h_flex()
            .justify_center()
            .bg(gray(25))
            .child(
                div()
                    .v_flex()
                    .w(books::W_FORM)
                    .mx_4()
                    .mb_4()
                    .bg(white())
                    .border_1()
                    .border_color(gray(200))
                    .rounded(books::RADIUS_LG)
                    .shadow(books::shadow_lg())
                    .text_13()
                    .text_color(gray(900))
                    .child(
                        div()
                            .h_flex()
                            .h(px(56.))
                            .px_4()
                            .border_b_1()
                            .border_color(gray(200))
                            .text_18()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(black())
                            .child(t("Set up your organization")),
                    )
                    .children(self.error.clone().map(|e| error_message(e, cx)))
                    .child(section(None, default, true))
                    .child(section(Some("Locale"), locale, false))
                    .child(section(Some("Accounting"), accounting, false))
                    .child(
                        div()
                            .h_flex()
                            .justify_between()
                            .p_4()
                            .border_t_1()
                            .border_color(gray(200))
                            .child(
                                Button::new("back")
                                    .label(t("Cancel"))
                                    .w_24()
                                    .border_1()
                                    .border_color(gray(200))
                                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SetUp::Back))),
                            )
                            .child(
                                Button::new("submit")
                                    .primary()
                                    .label(t("Submit"))
                                    .w_24()
                                    .disabled(!filled)
                                    .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
                            ),
                    ),
            )
    }
}
