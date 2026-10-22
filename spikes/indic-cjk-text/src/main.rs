//! Phase 0 spike 4: does GPUI render and edit Hindi, Gujarati, Chinese and Korean correctly?
//!
//! Each panel shows Books v0.36.0 labels from the vendored translation files, a customer
//! name, an amount, a line of hard shaping cases, a sentence wrapped in a narrow box, and an
//! input holding the hard line so the cursor and Backspace can be tried.

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::*;
use gpui_kit::*;

/// Labels shown in every panel, as keys into Books' translation files.
const LABELS: [&str; 10] = [
    "Customer",
    "Item",
    "Quantity",
    "Rate",
    "Amount",
    "Tax",
    "Add Row",
    "Submit",
    "Payment",
    "Print",
];

const SENTENCE: &str = "Sets the internal precision used for monetary calculations. \
                        Above 6 should be sufficient for most currencies.";

pub struct Language {
    pub code: &'static str,
    /// The language's own name for itself.
    pub name: &'static str,
    csv: &'static str,
    pub customer: &'static str,
    pub amount: &'static str,
    /// Conjuncts, reph, nukta, pre-base vowel signs, full-width punctuation, Hangul syllables.
    pub hard: &'static str,
}

pub const LANGUAGES: [Language; 4] = [
    Language {
        code: "hi",
        name: "हिन्दी",
        csv: include_str!("../../../vendor/books/translations/hi.csv"),
        customer: "राजेश क्षत्रिय",
        amount: "₹ 1,23,456.00",
        hard: "क्षत्रिय श्रीमती ज्ञान द्वार अर्थ कि ड़ ॐ",
    },
    Language {
        code: "gu",
        name: "ગુજરાતી",
        csv: include_str!("../../../vendor/books/translations/gu.csv"),
        customer: "ધર્મેશ ત્રિવેદી",
        amount: "₹ 1,23,456.00",
        hard: "ક્ષત્રિય શ્રીમતી જ્ઞાન દ્વાર અર્થ કિ",
    },
    Language {
        code: "zh-CN",
        name: "简体中文",
        csv: include_str!("../../../vendor/books/translations/zh-CN.csv"),
        customer: "王小明",
        amount: "¥ 12,345.67",
        hard: "增值税发票（含税）「客户」",
    },
    Language {
        code: "ko",
        name: "한국어",
        csv: include_str!("../../../vendor/books/translations/ko.csv"),
        customer: "김민준",
        amount: "₩ 1,234,567",
        hard: "세금계산서 (부가세 포함) 똠방각하",
    },
];

/// Books' translation CSV: English key, translation, context. Empty translations are skipped,
/// so lookups fall back to English as Books does.
pub fn load_translations(csv: &str) -> HashMap<String, String> {
    csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(csv.as_bytes())
        .records()
        .filter_map(Result::ok)
        .filter_map(|row| {
            let key = row.get(0)?;
            let text = row.get(1).filter(|text| !text.trim().is_empty())?;
            Some((key.to_string(), text.to_string()))
        })
        .collect()
}

struct Panel {
    language: &'static Language,
    translations: HashMap<String, String>,
    input: Entity<InputState>,
}

impl Panel {
    fn t(&self, key: &str) -> SharedString {
        self.translations
            .get(key)
            .cloned()
            .unwrap_or_else(|| key.to_string())
            .into()
    }
}

pub struct TextSpike {
    panels: Vec<Panel>,
}

impl TextSpike {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let panels = LANGUAGES
            .iter()
            .map(|language| Panel {
                language,
                translations: load_translations(language.csv),
                input: cx.new(|cx| InputState::new(window, cx).default_value(language.hard)),
            })
            .collect();
        Self { panels }
    }

    #[cfg(test)]
    pub fn input(&self, code: &str) -> Entity<InputState> {
        let panel = self.panels.iter().find(|p| p.language.code == code);
        panel.expect("known language").input.clone()
    }
}

impl Render for TextSpike {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let panels = self.panels.iter().map(|panel| {
            let language = panel.language;
            let field = |label: SharedString, value: SharedString| {
                div()
                    .h_flex()
                    .justify_between()
                    .gap_4()
                    .child(div().text_color(theme.muted_foreground).child(label))
                    .child(value)
            };
            let chips = LABELS.iter().map(|key| {
                div()
                    .px_2()
                    .py_0p5()
                    .border_1()
                    .border_color(theme.border)
                    .rounded_md()
                    .child(panel.t(key))
            });
            div()
                .v_flex()
                .gap_3()
                .p_4()
                .border_1()
                .border_color(theme.border)
                .rounded_lg()
                .child(
                    div()
                        .h_flex()
                        .items_baseline()
                        .gap_2()
                        .child(
                            div()
                                .text_xl()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(panel.t("Sales Invoice")),
                        )
                        .child(
                            div()
                                .text_color(theme.muted_foreground)
                                .child(format!("{} · {}", language.name, language.code)),
                        ),
                )
                .child(field(panel.t("Customer"), language.customer.into()))
                .child(field(panel.t("Grand Total"), language.amount.into()))
                .child(field(panel.t("Outstanding Amount"), language.amount.into()))
                .child(div().h_flex().flex_wrap().gap_1().children(chips))
                .child(div().text_lg().child(language.hard))
                .child(
                    div()
                        .text_lg()
                        .font_weight(FontWeight::BOLD)
                        .child(language.hard),
                )
                .child(
                    div()
                        .w(px(300.))
                        .p_2()
                        .bg(theme.muted)
                        .rounded_md()
                        .child(panel.t(SENTENCE)),
                )
                .child(Input::new(&panel.input).id(language.code))
        });

        div()
            .size_full()
            .p_6()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_4()
                    .children(panels),
            )
    }
}

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            let bounds = Bounds::centered(None, size(px(1200.), px(1250.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Latte Books - Indic and CJK Text Spike".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| TextSpike::new(window, cx))
            })
            .expect("failed to open window");
            cx.activate(true);
        });
}
