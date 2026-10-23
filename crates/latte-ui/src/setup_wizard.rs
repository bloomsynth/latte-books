//! Setting up a new company: `src/pages/SetupWizard/SetupWizard.vue` and the `SetupWizard`
//! schema @ v0.36.0.
//!
//! The company is saved as `<Company Name>.books.db` in `Documents\Latte Books`, as Books
//! saves it in `Documents\Frappe Books`.
//!
//! Differences from Books:
//! - Dates are typed as YYYY-MM-DD.
//! - There is no logo; it can be added in Print settings.
//! - When a file of that name exists, a new file with the time in its name is made. Books
//!   asks whether to overwrite the existing file.

use std::collections::HashMap;
use std::path::PathBuf;

use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::*;

use crate::i18n::t;
use jiff::civil::Date;
use latte_core::setup::{self, COA_LIST, SetupWizardOptions};

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
            |this, _, event: &LinkPicked, _, _| this.options.currency = event.0.to_string(),
        ));

        let charts = COA_LIST.iter().map(|(name, _)| (*name).to_owned());
        let chart = cx.new(|cx| LinkInput::new(options(charts), "Select CoA", window, cx));
        subscriptions.push(
            cx.subscribe_in(&chart, window, |this, _, event: &LinkPicked, _, _| {
                this.options.chart_of_accounts = event.0.to_string()
            }),
        );

        let start = cx.new(|cx| InputState::new(window, cx).placeholder("Fiscal Year Start Date"));
        let end = cx.new(|cx| InputState::new(window, cx).placeholder("Fiscal Year End Date"));
        for state in [&start, &end] {
            subscriptions.push(cx.subscribe_in(
                state,
                window,
                |this, _, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Blur) {
                        let _ = this.read_dates(cx);
                    }
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

    fn submit(&mut self, cx: &mut Context<Self>) {
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let labelled = |label: &str, control: AnyElement, cx: &App| {
            div()
                .v_flex()
                .gap_1()
                .child(field_label(label.to_owned(), true, cx))
                .child(control)
        };
        let mut fields: Vec<AnyElement> = Vec::new();
        for (fieldname, label, _) in TEXT_FIELDS {
            let input = Input::new(&self.texts[fieldname])
                .small()
                .into_any_element();
            fields.push(labelled(label, input, cx).into_any_element());
        }
        fields.insert(
            3,
            labelled("Country", self.country.clone().into_any_element(), cx).into_any_element(),
        );
        fields.insert(
            4,
            labelled("Currency", self.currency.clone().into_any_element(), cx).into_any_element(),
        );
        fields.push(
            labelled(
                "Chart of Accounts",
                self.chart.clone().into_any_element(),
                cx,
            )
            .into_any_element(),
        );
        fields.push(
            labelled(
                "Fiscal Year Start Date",
                Input::new(&self.start).small().into_any_element(),
                cx,
            )
            .into_any_element(),
        );
        fields.push(
            labelled(
                "Fiscal Year End Date",
                Input::new(&self.end).small().into_any_element(),
                cx,
            )
            .into_any_element(),
        );

        div().size_full().h_flex().justify_center().child(
            div()
                .v_flex()
                .w(px(640.))
                .border_1()
                .border_color(theme.border)
                .rounded_lg()
                .child(
                    div()
                        .v_flex()
                        .p_4()
                        .border_b_1()
                        .border_color(theme.border)
                        .child(
                            div()
                                .text_xl()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Set up your organization"),
                        ),
                )
                .children(self.error.clone().map(|e| error_message(e, cx)))
                .child(
                    div()
                        .grid()
                        .grid_cols(2)
                        .gap_x_6()
                        .gap_y_3()
                        .p_4()
                        .children(fields),
                )
                .child(
                    div()
                        .h_flex()
                        .justify_between()
                        .p_4()
                        .border_t_1()
                        .border_color(theme.border)
                        .child(
                            Button::new("back")
                                .small()
                                .ghost()
                                .label(t("Cancel"))
                                .on_click(cx.listener(|_, _, _, cx| cx.emit(SetUp::Back))),
                        )
                        .child(
                            Button::new("submit")
                                .small()
                                .primary()
                                .label(t("Submit"))
                                .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
                        ),
                ),
        )
    }
}
