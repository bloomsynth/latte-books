//! A page that Latte Books doesn't have yet, saying so: a report not ported yet.

use gpui_kit::component::*;
use gpui_kit::*;

use crate::i18n::t;

use crate::Navigate;
use crate::widgets::page_header;

pub struct Placeholder {
    title: String,
    text: String,
}

impl EventEmitter<Navigate> for Placeholder {}

impl Placeholder {
    pub fn new(title: &str, text: &str) -> Self {
        Self {
            title: title.to_owned(),
            text: text.to_owned(),
        }
    }
}

impl Render for Placeholder {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        div()
            .v_flex()
            .size_full()
            .child(page_header(t(&self.title), cx))
            .child(
                div()
                    .flex_1()
                    .h_flex()
                    .justify_center()
                    .text_color(theme.muted_foreground)
                    .child(self.text.clone()),
            )
    }
}
