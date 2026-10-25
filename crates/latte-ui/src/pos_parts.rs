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
        .text_color(books::hex(0xFFFFFF))
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

/// An SVG Books draws inline, as an image of the size it gives.
fn inline_svg(svg: &'static str, width: f32, height: f32) -> Img {
    img(Arc::new(Image::from_bytes(
        ImageFormat::Svg,
        svg.as_bytes().to_vec(),
    )))
    .w(px(width))
    .h(px(height))
    .flex_none()
}

/// POSQuickActions' Sales Invoice List icon (a Material Symbols receipt, inline in
/// Books).
pub fn receipt_icon() -> Img {
    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" height="24" viewBox="0 -960 960 960" width="21" fill="#000"><path d="M240-100q-41.92 0-70.96-29.04Q140-158.08 140-199.82V-300h120v-552.31l55.39 47.7 56.15-47.7 56.15 47.7 56.16-47.7 56.15 47.7 56.15-47.7 56.16 47.7 56.15-47.7 56.15 47.7 55.39-47.7V-200q0 41.92-29.04 70.96Q761.92-100 720-100H240Zm480-60q17 0 28.5-11.5T760-200v-560H320v460h360v100q0 17 11.5 28.5T720-160ZM367.69-610v-60h226.92v60H367.69Zm0 120v-60h226.92v60H367.69Zm310-114.62q-14.69 0-25.04-10.34-10.34-10.35-10.34-25.04t10.34-25.04q10.35-10.34 25.04-10.34t25.04 10.34q10.35 10.35 10.35 25.04t-10.35 25.04q-10.35 10.34-25.04 10.34Zm0 120q-14.69 0-25.04-10.34-10.34-10.35-10.34-25.04t10.34-25.04q10.35-10.34 25.04-10.34t25.04 10.34q10.35 10.35 10.35 25.04t-10.35 25.04q-10.35 10.34-25.04 10.34ZM240-160h380v-80H200v40q0 17 11.5 28.5T240-160Zm-40 0v-80 80Z"/></svg>"##;
    inline_svg(SVG, 21., 24.)
}

/// POSQuickActions' Loyalty Program icon.
pub fn loyalty_icon() -> Img {
    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" height="23px" viewBox="0 -960 960 960" width="25px" fill="#000"><path d="M100-180v-600h760v600H100Zm50.26-50.26h659.48v-499.48H150.26v499.48Zm0 0v-499.48 499.48Zm181.64-56.77h50.25v-42.56h48.67q14.37 0 23.6-10.38 9.22-10.38 9.22-24.25v-106.93q0-14.71-9.22-24.88-9.23-10.17-23.6-10.17H298.77v-73.95h164.87v-50.26h-81.49v-42.56H331.9v42.56h-48.41q-14.63 0-24.8 10.38-10.18 10.38-10.18 25v106.27q0 14.62 10.18 23.71 10.17 9.1 24.8 9.1h129.9v76.1H248.51v50.26h83.39v42.56Zm312.97-27.94L705.9-376H583.85l61.02 61.03ZM583.85-574H705.9l-61.03-61.03L583.85-574Z"/></svg>"##;
    inline_svg(SVG, 25., 23.)
}

/// POSQuickActions' Coupon Code icon.
pub fn coupon_icon() -> Img {
    const SVG: &str = r##"<svg fill="#000000" width="28px" height="28px" viewBox="0 0 512.00 512.00" xmlns="http://www.w3.org/2000/svg" stroke="#000000" stroke-width="3.312000000000001"><g><path d="M412.7,134.4H229.6c-2,0-3.9,0.8-5.3,2.2l-27.8,27.8L169.1,137c-1.4-1.4-3.3-2.2-5.3-2.2H99.3c-4.1,0-7.5,3.4-7.5,7.5 v227.4c0,4.1,3.4,7.5,7.5,7.5h64.5c2,0,3.9-0.8,5.3-2.2l27.4-27.4l27.8,27.8c1.4,1.4,3.3,2.2,5.3,2.2h183.1c4.1,0,7.5-3.4,7.5-7.5 V141.9C420.2,137.7,416.8,134.4,412.7,134.4z M405.2,362.6H232.7l-30.9-30.9c-2.9-2.9-7.7-2.9-10.6,0l-30.5,30.5h-53.9V149.8h53.9 l30.5,30.5c2.9,2.9,7.7,2.9,10.6,0l30.9-30.9h172.5V362.6z"></path><path d="M276.9,235.2c15.4,0,28-12.6,28-28s-12.6-28-28-28s-28,12.6-28,28S261.4,235.2,276.9,235.2z M276.9,194.2 c7.2,0,13,5.8,13,13s-5.8,13-13,13s-13-5.8-13-13S269.7,194.2,276.9,194.2z"></path><path d="M360,262.4c-15.4,0-28,12.6-28,28s12.6,28,28,28s28-12.6,28-28S375.4,262.4,360,262.4z M360,303.4c-7.2,0-13-5.8-13-13 s5.8-13,13-13s13,5.8,13,13S367.2,303.4,360,303.4z"></path><path d="M256.6,310.7c1.5,1.5,3.4,2.2,5.3,2.2s3.8-0.7,5.3-2.2l113.1-113.1c2.9-2.9,2.9-7.7,0-10.6c-2.9-2.9-7.7-2.9-10.6,0 L256.6,300.1C253.6,303,253.6,307.7,256.6,310.7z"></path><path d="M196.5,202.5c-2,0-3.9,0.8-5.3,2.2c-1.4,1.4-2.2,3.3-2.2,5.3c0,2,0.8,3.9,2.2,5.3c1.4,1.4,3.3,2.2,5.3,2.2 c2,0,3.9-0.8,5.3-2.2c1.4-1.4,2.2-3.3,2.2-5.3c0-2-0.8-3.9-2.2-5.3C200.4,203.3,198.4,202.5,196.5,202.5z"></path><path d="M196.5,233.2c-2,0-3.9,0.8-5.3,2.2c-1.4,1.4-2.2,3.3-2.2,5.3c0,2,0.8,3.9,2.2,5.3c1.4,1.4,3.3,2.2,5.3,2.2 c2,0,3.9-0.8,5.3-2.2c1.4-1.4,2.2-3.3,2.2-5.3c0-2-0.8-3.9-2.2-5.3C200.4,234,198.4,233.2,196.5,233.2z"></path><path d="M196.5,263.8c-2,0-3.9,0.8-5.3,2.2c-1.4,1.4-2.2,3.3-2.2,5.3c0,2,0.8,3.9,2.2,5.3c1.4,1.4,3.3,2.2,5.3,2.2 c2,0,3.9-0.8,5.3-2.2c1.4-1.4,2.2-3.3,2.2-5.3c0-2-0.8-3.9-2.2-5.3C200.4,264.6,198.4,263.8,196.5,263.8z"></path><path d="M196.5,294.5c-2,0-3.9,0.8-5.3,2.2c-1.4,1.4-2.2,3.3-2.2,5.3c0,2,0.8,3.9,2.2,5.3c1.4,1.4,3.3,2.2,5.3,2.2 c2,0,3.9-0.8,5.3-2.2c1.4-1.4,2.2-3.3,2.2-5.3c0-2-0.8-3.9-2.2-5.3C200.4,295.3,198.4,294.5,196.5,294.5z"></path></g></svg>"##;
    inline_svg(SVG, 28., 28.)
}

/// POSQuickActions' Price List icon.
pub fn price_list_icon() -> Img {
    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" height="23px" viewBox="0 -960 960 960" width="24px" fill="#000"><path d="M180.31-164q-27.01 0-45.66-18.65Q116-201.3 116-228.31v-503.38q0-27.01 18.65-45.66Q153.3-796 180.31-796h599.38q27.01 0 45.66 18.65Q844-758.7 844-731.69v503.38q0 27.01-18.65 45.66Q806.7-164 779.69-164H180.31Zm0-52h599.38q4.62 0 8.46-3.85 3.85-3.84 3.85-8.46v-503.38q0-4.62-3.85-8.46-3.84-3.85-8.46-3.85H180.31q-4.62 0-8.46 3.85-3.85 3.84-3.85 8.46v503.38q0 4.62 3.85 8.46 3.84 3.85 8.46 3.85ZM221-297h172v-52H221v52Zm361-77.23L737.77-530 701-566.77l-119 119-51-51L494.23-462 582-374.23ZM221-454h172v-52H221v52Zm0-156h172v-52H221v52Zm-53 394v-528 528Z"/></svg>"##;
    inline_svg(SVG, 24., 23.)
}

/// POSQuickActions' Item Enquiry icon (a Heroicons pencil square).
pub fn enquiry_icon() -> Img {
    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke="#000" stroke-width="1.5" width="24" height="24"><path stroke-linecap="round" stroke-linejoin="round" d="M16.862 4.487l1.686 1.686a1.25 1.25 0 010 1.768L10.06 16.43a.75.75 0 01-.49.219l-2.563.128a.75.75 0 01-.791-.79l.128-2.564a.75.75 0 01.218-.49l8.488-8.489a1.25 1.25 0 011.768 0zM19.5 13.25v4.75a2 2 0 01-2 2h-11a2 2 0 01-2-2v-11a2 2 0 012-2h4.75"/></svg>"##;
    inline_svg(SVG, 24., 24.)
}

/// LoyaltyProgramModal's coins icon, in `#F19E39`.
pub fn coins_icon() -> Img {
    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" height="20px" viewBox="0 -960 960 960" width="20px" fill="#F19E39"><path d="M480-60q-117 0-217.22-65.23T108.33-287.77v133.92H58.08v-220h220v50.26H146.05q44.87 89.95 137.44 151.64 92.56 61.69 196.51 61.69 74.95 0 141.19-27.89 66.25-27.9 116.04-76.35 49.8-48.45 79.81-114.31 30.01-65.86 32.32-141.19h50.25q-1.92 85.23-35.65 159.85-33.73 74.61-90.46 130.38Q716.77-124 641.19-92 565.61-60 480-60Zm-24.92-144.31v-51.54q-42.85-10.64-72.5-35.38-29.66-24.74-48.58-68.21l43.44-16.41q14.3 37.85 43.39 58.77 29.09 20.93 65.58 20.93 37.8 0 66.31-18.94 28.51-18.94 28.51-56.14 0-33.21-23.55-53.89t-89.22-46.62q-62.36-24.06-90.82-51.52-28.46-27.46-28.46-73.61 0-40.18 28.54-71.05 28.54-30.87 79.36-38.49v-48.87h47.43v48.87q34.51 3.03 62.12 22.91 27.6 19.88 43.11 51.81l-41.53 18.36q-13.7-24.54-34.75-38.53-21.05-13.99-51.46-13.99-39.21 0-62.42 19.18-23.22 19.18-23.22 49.8 0 31.28 22.18 48.95 22.18 17.66 84.43 41.1 70.52 27.49 97.72 58.31 27.21 30.82 27.21 77.28 0 26.97-10.17 47.92-10.17 20.95-27.12 35.48-16.94 14.52-39.7 23.06t-48.4 10.92v49.54h-47.43ZM60.39-490q2.69-87.15 36.8-161.96 34.12-74.81 91.23-130.19 57.12-55.39 132.12-86.62Q395.54-900 480-900q115.85 0 217.22 65.43 101.37 65.42 154.45 163.88v-135.46h50.25v220h-220v-50.26h132.03q-43.72-88.15-136.14-150.74-92.42-62.59-197.81-62.59-73.41 0-139.59 27.51t-116.23 75.58q-50.05 48.06-80.64 113.92-30.59 65.86-32.9 142.73H60.39Z"/></svg>"##;
    inline_svg(SVG, 20., 20.)
}

/// An item's picture, kept in Books as `data:image/png;base64,…`.
pub fn image_from_data_url(url: &str) -> Option<Image> {
    use base64::Engine as _;
    let (mime, data) = url.strip_prefix("data:")?.split_once(";base64,")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .ok()?;
    Some(Image::from_bytes(ImageFormat::from_mime_type(mime)?, bytes))
}

/// A modal's title: `text-center font-semibold py-3`.
pub fn modal_title(text: impl Into<SharedString>) -> Div {
    div()
        .py_3()
        .text_center()
        .font_weight(FontWeight::SEMIBOLD)
        .child(text.into())
}

/// `<hr>`.
pub fn rule() -> Div {
    div().w_full().h(px(1.)).flex_none().bg(gray(200))
}

/// A labelled control (`show-label`): the label above, gray-600.
pub fn labelled(label: impl Into<SharedString>, control: impl IntoElement) -> Div {
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
        .child(control)
}

/// A labelled, read-only, bordered control, its text to the left or, for numbers, to the
/// right.
pub fn read_only_field(
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    numeric: bool,
) -> Div {
    labelled(
        label,
        crate::widgets::control_frame(true, false)
            .px_3()
            .when(numeric, |d| d.justify_end())
            .child(div().truncate().child(value.into())),
    )
}

/// A cell of a `Row` spanning columns of the given ratios.
pub fn span_cell(ratios: &[f32]) -> Div {
    ratio_cell(ratios.iter().sum())
}

/// KeyboardModal's keys: `py-2.5 bg-gray-100 text-2xl rounded-lg`, gray-200 when hovered.
pub fn keypad_key(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(50.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(books::RADIUS_LG)
        .bg(gray(100))
        .hover(|d| d.bg(gray(200)))
        .cursor_pointer()
        .text_20()
        .font_weight(FontWeight::BOLD)
        .child(label.into())
}
