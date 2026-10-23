//! Small pieces the screens share: page headers, badges, messages.

use gpui_kit::component::*;
use gpui_kit::*;

/// A page's header row: its title on the left, its buttons on the right
/// (`PageHeader.vue`).
pub fn page_header(title: impl Into<SharedString>, cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .h_flex()
        .w_full()
        .h(px(52.))
        .px_4()
        .gap_2()
        .justify_between()
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .text_xl()
                .font_weight(FontWeight::SEMIBOLD)
                .truncate()
                .child(title.into()),
        )
}

/// `Badge.vue`: a short coloured label, by Books' colour names.
pub fn badge(label: impl Into<SharedString>, color: &str, cx: &App) -> Div {
    let theme = cx.theme();
    let fg = match color {
        "green" => theme.green,
        "red" => theme.red,
        "orange" | "yellow" => theme.yellow,
        "blue" => theme.blue,
        "lime" => theme.cyan,
        _ => theme.muted_foreground,
    };
    div()
        .px_2()
        .rounded_md()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .bg(fg.opacity(0.14))
        .text_color(fg)
        .child(label.into())
}

/// An error under a page's header, as Books shows a toast or a dialog.
pub fn error_message(message: impl Into<SharedString>, cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .mx_4()
        .mt_2()
        .px_3()
        .py_2()
        .rounded_md()
        .border_l_2()
        .border_color(theme.red)
        .bg(theme.red.opacity(0.1))
        .text_color(theme.foreground)
        .child(message.into())
}

/// A message that something worked.
pub fn info_message(message: impl Into<SharedString>, cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .mx_4()
        .mt_2()
        .px_3()
        .py_2()
        .rounded_md()
        .border_l_2()
        .border_color(theme.green)
        .bg(theme.green.opacity(0.1))
        .text_color(theme.foreground)
        .child(message.into())
}

/// A field's label above its control.
pub fn field_label(label: impl Into<SharedString>, required: bool, cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .h_flex()
        .gap_1()
        .text_sm()
        .text_color(theme.muted_foreground)
        .child(label.into())
        .children(required.then(|| div().text_color(theme.red).child("*")))
}
