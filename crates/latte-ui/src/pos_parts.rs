//! The stateless pieces of Books' point of sale (`src/pages/POS/`,
//! `src/components/POS/`) @ v0.36.0: its modal, its big coloured buttons, its floating
//! label totals and its tables' rows.

use std::sync::Arc;

use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::books::{self, Text as _, black, gray, white};

/// `Modal.vue` as the POS opens it: a card over the whole window, dimmed (`.backdrop`,
/// without its 2px blur), centred, or at the right for the payment modal
/// (`ml-auto mr-3.5`).
pub fn modal(card: impl IntoElement, at_right: bool, window: &Window) -> impl IntoElement {
    let viewport = window.viewport_size();
    deferred(
        anchored().position(point(px(0.), px(0.))).child(
            div()
                .id("pos-modal-backdrop")
                .occlude()
                .w(viewport.width)
                .h(viewport.height)
                .flex()
                .items_center()
                .map(|d| {
                    if at_right {
                        d.justify_end().pr(px(14.))
                    } else {
                        d.justify_center()
                    }
                })
                .bg(hsla(0., 0., 0., 0.1))
                .child(card),
        ),
    )
    .with_priority(1)
}

/// `Modal.vue`'s card: white, `rounded-lg shadow-2xl border`.
pub fn modal_card() -> Div {
    div()
        .v_flex()
        .bg(white())
        .rounded(books::RADIUS_LG)
        .border_1()
        .border_color(gray(200))
        .overflow_hidden()
        .text_color(black())
        .shadow(vec![BoxShadow {
            color: hsla(0., 0., 0., 0.25),
            offset: point(px(0.), px(25.)),
            blur_radius: px(50.),
            spread_radius: px(-12.),
            inset: false,
        }])
}

/// The POS's big buttons (`<Button class="w-full py-6" :style="{ backgroundColor }">`
/// with an uppercase `text-lg text-white font-semibold` label). `Button.vue`'s `h-8`
/// gives way to the padding, so a button is twice its vertical padding high.
pub fn big_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    bg: Hsla,
    height: Pixels,
) -> Stateful<Div> {
    let label: SharedString = label.into();
    div()
        .id(id)
        .flex_1()
        .min_w_0()
        .h(height)
        .flex()
        .items_center()
        .justify_center()
        .rounded(books::RADIUS_MD)
        .bg(bg)
        .cursor_pointer()
        .active(move |s| s.bg(bg.blend(hsla(0., 0., 0., 0.05))))
        .text_14()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(white())
        .whitespace_nowrap()
        .child(label.to_uppercase())
}

/// A `#rrggbb` colour, as Defaults and POS Profile store a button's colour.
pub fn hex_colour(value: &str) -> Option<Hsla> {
    let hex = value.trim().strip_prefix('#')?;
    (hex.len() == 6)
        .then(|| u32::from_str_radix(hex, 16).ok())
        .flatten()
        .map(books::hex)
}

/// `FloatingLabelFloatInput` / `FloatingLabelCurrencyInput` (`size="large"`, read
/// only, `text-right`): a gray-25 box, its label sitting on the top border, and a
/// currency's symbol inside at the left.
pub fn floating_field(
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    symbol: Option<String>,
) -> Div {
    div()
        .relative()
        .w_full()
        .child(
            div()
                .h_flex()
                .w_full()
                .justify_end()
                .px(px(10.))
                .pt_4()
                .pb(px(10.))
                .bg(gray(25))
                .rounded(books::RADIUS_LG)
                .border_1()
                .border_color(gray(200))
                .text_14()
                .line_height(px(21.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(gray(900))
                .child(value.into()),
        )
        .child(floating_label(label.into()).top(px(-9.)))
        .children(symbol.map(|s| floating_label(s.into()).top(px(19.))))
}

/// The label of a floating label input: `text-xl` scaled to 75%, gray-500.
fn floating_label(text: SharedString) -> Div {
    div()
        .absolute()
        .left(px(10.))
        .text_size(px(13.5))
        .line_height(px(20.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(gray(500))
        .whitespace_nowrap()
        .child(text)
}

/// A labelled, read-only, bordered control (`show-label` `border` `read-only`), as the
/// payment modal's totals are; numbers to the right.
pub fn shown_field(label: impl Into<SharedString>, value: impl Into<SharedString>) -> Div {
    div()
        .v_flex()
        .w_full()
        .child(
            div()
                .mb_1()
                .text_12()
                .text_color(gray(600))
                .child(label.into()),
        )
        .child(
            crate::widgets::control_frame(true, false)
                .px_3()
                .justify_end()
                .child(value.into()),
        )
}

/// A cell of `Row.vue` with a `ratio`: `minmax(0, <ratio>fr)`.
pub fn ratio_cell(ratio: f32) -> Div {
    div()
        .flex_grow(ratio)
        .flex_shrink(1.)
        .flex_basis(px(0.))
        .min_w_0()
        .h_flex()
}

/// The header `Row` of the POS's tables: bordered, rounded at the top, its labels
/// `px-2 py-2 text-lg` in gray-600, numbers' to the right (`ms-auto`).
pub fn table_header(columns: &[(String, f32, bool)]) -> Div {
    div()
        .h_flex()
        .w_full()
        .px_2()
        .border_1()
        .border_color(gray(200))
        .rounded_t(books::RADIUS_MD)
        .text_color(gray(600))
        .text_14()
        .children(columns.iter().map(|(label, ratio, numeric)| {
            ratio_cell(*ratio)
                .when(*numeric, |d| d.justify_end())
                .child(div().p_2().truncate().child(label.clone()))
        }))
}

/// A form control's read-only text in a table row (`FormControl` `size="large"`):
/// `px-3 py-2 text-base`, gray-800, numbers to the right.
pub fn cell_text(text: impl Into<SharedString>, numeric: bool) -> Div {
    div()
        .w_full()
        .min_w_0()
        .px_3()
        .py_2()
        .text_13()
        .text_color(gray(800))
        .truncate()
        .when(numeric, |d| d.text_right())
        .child(text.into())
}

/// `getExtractedWords`: an item's initials, for the grid when it has no image.
pub fn initials(name: &str) -> String {
    name.split(' ')
        .filter_map(|w| w.chars().next())
        .flat_map(char::to_uppercase)
        .collect()
}

/// POSQuickActions' Sales Invoice List icon (a Material Symbols receipt, inline in
/// Books).
pub fn receipt_icon() -> Img {
    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" height="24" viewBox="0 -960 960 960" width="21" fill="#000"><path d="M240-100q-41.92 0-70.96-29.04Q140-158.08 140-199.82V-300h120v-552.31l55.39 47.7 56.15-47.7 56.15 47.7 56.16-47.7 56.15 47.7 56.15-47.7 56.16 47.7 56.15-47.7 56.15 47.7 55.39-47.7V-200q0 41.92-29.04 70.96Q761.92-100 720-100H240Zm480-60q17 0 28.5-11.5T760-200v-560H320v460h360v100q0 17 11.5 28.5T720-160ZM367.69-610v-60h226.92v60H367.69Zm0 120v-60h226.92v60H367.69Zm310-114.62q-14.69 0-25.04-10.34-10.34-10.35-10.34-25.04t10.34-25.04q10.35-10.34 25.04-10.34t25.04 10.34q10.35 10.35 10.35 25.04t-10.35 25.04q-10.35 10.34-25.04 10.34Zm0 120q-14.69 0-25.04-10.34-10.34-10.35-10.34-25.04t10.34-25.04q10.35-10.34 25.04-10.34t25.04 10.34q10.35 10.35 10.35 25.04t-10.35 25.04q-10.35 10.34-25.04 10.34ZM240-160h380v-80H200v40q0 17 11.5 28.5T240-160Zm-40 0v-80 80Z"/></svg>"##;
    img(Arc::new(Image::from_bytes(
        ImageFormat::Svg,
        SVG.as_bytes().to_vec(),
    )))
    .w(px(21.))
    .h(px(24.))
}
