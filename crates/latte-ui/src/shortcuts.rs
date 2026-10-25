//! The Shortcuts window: `src/components/ShortcutsHelper.vue` and `ShortcutKeys.vue`
//! @ v0.36.0, opened from the sidebar's Shortcuts. A group's header folds it.

use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use latte_core::views::shortcuts::{GROUPS, key_label};

use crate::books::{self, Text as _, gray};
use crate::i18n::t;

pub struct ShortcutsHelper {
    collapsed: [bool; GROUPS.len()],
}

impl ShortcutsHelper {
    pub fn new() -> Self {
        Self {
            collapsed: [false; GROUPS.len()],
        }
    }
}

/// `ShortcutKeys.vue`: each key in a gray monospace cap with a deeper bottom edge.
pub fn shortcut_keys(keys: &[&'static str]) -> Div {
    let mac = cfg!(target_os = "macos");
    div()
        .h_flex()
        .flex_none()
        .gap_2()
        .children(keys.iter().map(|key| {
            div()
                .px(px(6.))
                .py(px(2.))
                .rounded(books::RADIUS_MD)
                .bg(books::raw_gray(200))
                .text_color(books::raw_gray(700))
                .font_family("monospace")
                .font_weight(FontWeight::SEMIBOLD)
                .border_b_4()
                .border_color(books::raw_gray(400))
                .shadow_md()
                .child(key_label(key, mac))
        }))
}

impl Render for ShortcutsHelper {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let hr = || div().w_full().h(px(1.)).flex_none().bg(gray(200));
        let mut body: Vec<AnyElement> = Vec::new();
        for (g, group) in GROUPS.iter().enumerate() {
            let collapsed = self.collapsed[g];
            body.push(
                div()
                    .v_flex()
                    .p_4()
                    .w_full()
                    .child(
                        div()
                            .id(("shortcut-group", g))
                            .mb_4()
                            .cursor_pointer()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t(group.label)),
                            )
                            .child(div().text_13().child(t(group.description)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.collapsed[g] = !this.collapsed[g];
                                cx.notify();
                            })),
                    )
                    .map(|d| {
                        if collapsed {
                            d.child(div().text_13().text_color(gray(600)).child(format!(
                                "{} {}",
                                group.shortcuts.len(),
                                t("shortcuts")
                            )))
                        } else {
                            d.child(div().v_flex().gap_4().children(group.shortcuts.iter().map(
                                |s| {
                                    div()
                                        .h_flex()
                                        .items_start()
                                        .gap_4()
                                        .child(
                                            div()
                                                .w_32()
                                                .flex_none()
                                                .text_13()
                                                .child(shortcut_keys(s.keys)),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_13()
                                                .child(t(s.description)),
                                        )
                                },
                            )))
                        }
                    })
                    .into_any_element(),
            );
            body.push(hr().into_any_element());
        }
        body.push(
            div()
                .p_4()
                .text_13()
                .text_color(gray(600))
                .child(t("More shortcuts will be added soon."))
                .into_any_element(),
        );
        div()
            .v_flex()
            .w(books::W_FORM)
            .text_color(gray(900))
            // `FormHeader.vue`.
            .child(
                div()
                    .h_flex()
                    .h(px(56.))
                    .px_4()
                    .flex_none()
                    .text_18()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t("Shortcuts")),
            )
            .child(hr())
            .child(
                div()
                    .id("shortcuts-scroll")
                    .h(px(384.))
                    .overflow_y_scroll()
                    .children(body),
            )
    }
}
