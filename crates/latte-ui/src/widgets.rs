//! Small pieces the screens share, drawn as Books draws them: buttons, page headers,
//! badges, messages, labels and the controls' frames.

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::books::{self, Text as _, black, gray, white};

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// `src/components/Button.vue`: a 32px-high button, `secondary` (gray) unless made
/// `primary` (black). An icon-only button is narrower (`:icon="true"`).
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: Option<SharedString>,
    icon: Option<&'static str>,
    primary: bool,
    background: bool,
    disabled: bool,
    tooltip: Option<SharedString>,
    on_click: Option<ClickHandler>,
    style: StyleRefinement,
}

impl Button {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            label: None,
            icon: None,
            primary: false,
            background: true,
            disabled: false,
            tooltip: None,
            on_click: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// A feather icon, by its name.
    pub fn icon(mut self, name: &'static str) -> Self {
        self.icon = Some(name);
        self
    }

    pub fn primary(mut self) -> Self {
        self.primary = true;
        self
    }

    /// `:background="false"`: text only.
    pub fn ghost(mut self) -> Self {
        self.background = false;
        self
    }

    /// Books' buttons come in one size; kept so the screens read as before.
    pub fn small(self) -> Self {
        self
    }

    pub fn xsmall(self) -> Self {
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Books' `title`, shown on hover.
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Button {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let icon_only = self.icon.is_some() && self.label.is_none();
        let (bg, fg) = if self.primary {
            (black(), white())
        } else {
            (gray(200), gray(700))
        };
        let mut button = div()
            .id(self.id)
            .h_flex()
            .flex_none()
            .justify_center()
            .items_center()
            .gap_1()
            .h_8()
            .when(icon_only, |d| d.px_3())
            .when(!icon_only, |d| d.px_6())
            .rounded(books::RADIUS_MD)
            .text_12()
            .text_color(fg)
            .whitespace_nowrap()
            .when(self.background, |d| d.bg(bg))
            .when(self.disabled, |d| d.opacity(0.5))
            .when(!self.disabled, |d| d.cursor_pointer())
            .when(!self.disabled && self.background, |d| {
                d.active(move |s| s.bg(bg.blend(hsla(0., 0., 0., 0.05))))
            })
            .when(!self.disabled && !self.background, |d| {
                d.hover(|s| s.text_color(gray(900)))
            })
            .children(self.icon.map(|name| books::icon(name, px(16.), fg)))
            .children(self.label);
        *button.style() = {
            let mut style = button.style().clone();
            style.refine(&self.style);
            style
        };
        if let Some(tooltip) = self.tooltip {
            button =
                button.tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx));
        }
        if let (Some(handler), false) = (self.on_click, self.disabled) {
            button = button.on_click(handler);
        }
        button
    }
}

/// A page's header row (`PageHeader.vue`): 64px high, Back and Forward
/// (`PageHeaderNavGroup.vue`) and the title on the left, the buttons, 8px apart, on the
/// right.
pub fn page_header(title: impl Into<SharedString>, cx: &App) -> Div {
    page_header_with(title, div(), cx)
}

/// A page header with more on the left after the title (`PageHeader.vue`'s `#left`).
pub fn page_header_with(title: impl Into<SharedString>, left: impl IntoElement, cx: &App) -> Div {
    let title: SharedString = title.into();
    div()
        .h_flex()
        .w_full()
        .flex_none()
        .h(books::H_PAGE_HEADER)
        .px_4()
        .gap_2()
        .justify_between()
        .border_b_1()
        .border_color(gray(200))
        .child(
            div()
                .h_flex()
                .gap_4()
                .min_w_0()
                .child(nav_group(cx))
                .when(!title.is_empty(), |d| {
                    d.child(
                        div()
                            .text_18()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(black())
                            .whitespace_nowrap()
                            .truncate()
                            .child(title),
                    )
                })
                .child(left),
        )
}

/// `PageHeaderNavGroup.vue`: Back and Forward, joined, gray until there is somewhere to go.
fn nav_group(cx: &App) -> Div {
    let (can_back, can_forward) = cx
        .try_global::<crate::desk::NavState>()
        .map_or((false, false), |n| (n.can_back, n.can_forward));
    let link = |id: &'static str, icon: &'static str, enabled: bool, back: bool| {
        div()
            .id(id)
            .h_flex()
            .h_8()
            .px_3()
            .bg(gray(200))
            .when(enabled, |d| d.cursor_pointer())
            .child(books::icon(
                icon,
                px(16.),
                if enabled { gray(700) } else { gray(400) },
            ))
            .on_click(move |_, window, cx| {
                let Some(desk) = cx
                    .try_global::<crate::desk::NavState>()
                    .and_then(|n| n.desk.upgrade())
                else {
                    return;
                };
                desk.update(cx, |desk, cx| {
                    if back {
                        desk.go_back(window, cx)
                    } else {
                        desk.go_forward(window, cx)
                    }
                });
            })
    };
    div()
        .h_flex()
        .flex_none()
        .rounded(books::RADIUS_MD)
        .overflow_hidden()
        .child(
            link("nav-back", "chevron-left", can_back, true)
                .rounded_l(books::RADIUS_MD)
                .border_r_1()
                .border_color(white()),
        )
        .child(link("nav-forward", "chevron-right", can_forward, false).rounded_r(books::RADIUS_MD))
}

/// `Badge.vue`: a short label on its colour's 200 shade, in its 700, as lists show a
/// status.
pub fn badge(label: impl Into<SharedString>, color: &str, _: &App) -> Div {
    let (bg, fg) = books::bg_text(color);
    div()
        .flex_none()
        .px_2()
        .py_1()
        .rounded(books::RADIUS_MD)
        .text_11()
        .whitespace_nowrap()
        .truncate()
        .bg(bg)
        .text_color(fg)
        .child(label.into())
}

/// `StatusPill.vue`: a form's status beside its title.
pub fn pill(label: impl Into<SharedString>, color: &str) -> Div {
    let (bg, fg) = books::bg_text(color);
    div()
        .flex_none()
        .px(px(6.))
        .py(px(2.))
        .rounded(books::RADIUS_MD)
        .text_11()
        .font_weight(FontWeight::MEDIUM)
        .whitespace_nowrap()
        .bg(bg)
        .text_color(fg)
        .child(label.into())
}

/// An error under a page's header, as Books shows a toast (`Toast.vue`, red).
pub fn error_message(message: impl Into<SharedString>, _: &App) -> Div {
    message_bar(message, "red", "alert-circle")
}

/// A message that something worked (`Toast.vue`, green).
pub fn info_message(message: impl Into<SharedString>, _: &App) -> Div {
    message_bar(message, "green", "check-circle")
}

fn message_bar(message: impl Into<SharedString>, color: &str, icon: &'static str) -> Div {
    div()
        .h_flex()
        .mx_4()
        .mt_2()
        .px_4()
        .py_3()
        .gap_3()
        .rounded(books::RADIUS_LG)
        .border_1()
        .border_color(gray(200))
        .bg(white())
        .shadow_md()
        .text_13()
        .text_color(gray(900))
        .child(books::icon(icon, px(20.), books::color(color, 400)))
        .child(div().flex_1().child(message.into()))
}

/// A field's label above its control (`Base.vue` `labelClasses`).
pub fn field_label(label: impl Into<SharedString>, _required: bool, _: &App) -> Div {
    div()
        .mb_1()
        .text_12()
        .text_color(gray(600))
        .child(label.into())
}

/// The frame Books draws around a form control (`Base.vue` with `border`): gray-25 with
/// a gray-200 border, gray-50 when read-only, gray-100 while focused.
pub fn control_frame(read_only: bool, focused: bool) -> Div {
    div()
        .h_flex()
        .w_full()
        .min_h(px(34.))
        .rounded(books::RADIUS)
        .border_1()
        .border_color(gray(200))
        .text_13()
        .map(|d| match (read_only, focused) {
            (true, _) => d.bg(gray(50)).text_color(gray(800)),
            (false, true) => d.bg(gray(100)).text_color(gray(900)),
            (false, false) => d.bg(gray(25)).text_color(gray(900)),
        })
}

/// `show-mandatory`: a red asterisk after an empty required control.
pub fn mandatory_mark() -> Div {
    div()
        .absolute()
        .top(px(9.))
        .right(px(7.))
        .text_13()
        .text_color(books::color("red", 500))
        .child("*")
}

/// `Check.vue`: Books' 14px checkbox, in `#A1ABB4`.
pub fn check_box(checked: bool) -> Img {
    img(if checked {
        "books/check-on.svg"
    } else {
        "books/check-off.svg"
    })
    .size(px(14.))
    .flex_none()
}

/// The up-and-down chevron at the end of a Link or Select control.
pub fn selector_icon(mandatory: bool) -> Svg {
    svg()
        .path("books/selector.svg")
        .w(px(12.))
        .h(px(12.))
        .mr(px(-3.))
        .flex_none()
        .text_color(if mandatory {
            books::color("red", 400)
        } else {
            gray(400)
        })
}

/// `Popover.vue`'s panel: white, rounded, bordered, with a large shadow.
pub fn popover_panel() -> Div {
    div()
        .bg(white())
        .rounded(books::RADIUS_MD)
        .border_1()
        .border_color(gray(200))
        .shadow(books::shadow_lg())
        .text_color(gray(900))
}

/// `Dropdown.vue`'s list of items, each `p-2 rounded-md`, the hovered one gray-100.
pub fn dropdown_item(id: impl Into<ElementId>, label: impl IntoElement) -> Stateful<Div> {
    div()
        .id(id)
        .p_2()
        .rounded(books::RADIUS_MD)
        .text_12()
        .cursor_pointer()
        .whitespace_nowrap()
        .hover(|d| d.bg(gray(100)))
        .child(label)
}

/// What a dialog or toast is about (`ToastType`), which sets its icon and colour
/// (`getIconConfig`).
#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Info,
    Warning,
    Error,
    Success,
}

impl Kind {
    fn icon(self) -> (&'static str, &'static str) {
        match self {
            Kind::Info => ("alert-circle", "blue"),
            Kind::Warning => ("alert-triangle", "orange"),
            Kind::Error => ("alert-circle", "red"),
            Kind::Success => ("check-circle", "green"),
        }
    }
}

/// A button of a [`dialog`]: its label, whether primary, and what it does.
pub struct DialogButton {
    pub label: SharedString,
    pub primary: bool,
    pub on_click: ClickHandler,
}

impl DialogButton {
    pub fn new(
        label: impl Into<SharedString>,
        primary: bool,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            primary,
            on_click: Box::new(on_click),
        }
    }
}

/// `Dialog.vue`: a 384px card over a dimmed window, its title and icon on top, the
/// detail below, and the buttons at the end.
pub fn dialog(
    title: impl Into<SharedString>,
    detail: Option<SharedString>,
    kind: Kind,
    buttons: Vec<DialogButton>,
    window: &Window,
) -> impl IntoElement {
    let (icon, color) = kind.icon();
    let viewport = window.viewport_size();
    deferred(
        anchored().position(point(px(0.), px(0.))).child(
            div()
                .id("dialog-backdrop")
                .occlude()
                .w(viewport.width)
                .h(viewport.height)
                .flex()
                .items_center()
                .justify_center()
                .bg(hsla(0., 0., 0., 0.1))
                .child(
                    div()
                        .v_flex()
                        .w(px(384.))
                        .p_4()
                        .gap_4()
                        .bg(white())
                        .border_1()
                        .border_color(gray(200))
                        .rounded(books::RADIUS_LG)
                        .shadow(vec![BoxShadow {
                            color: hsla(0., 0., 0., 0.25),
                            offset: point(px(0.), px(25.)),
                            blur_radius: px(50.),
                            spread_radius: px(-12.),
                            inset: false,
                        }])
                        .text_color(gray(900))
                        .child(
                            div()
                                .h_flex()
                                .justify_between()
                                .child(div().font_weight(FontWeight::SEMIBOLD).child(title.into()))
                                .child(books::icon(icon, px(24.), books::color(color, 400))),
                        )
                        .children(detail.map(|d| div().text_13().child(d)))
                        .child(div().h_flex().justify_end().gap_4().mt_4().children(
                            buttons.into_iter().enumerate().map(|(i, b)| {
                                let button = Button::new(("dialog-button", i))
                                    .label(b.label)
                                    .min_w(px(80.))
                                    .on_click(b.on_click);
                                if b.primary { button.primary() } else { button }
                            }),
                        )),
                ),
        ),
    )
    .with_priority(2)
}

/// `Toast.vue`: a 384px note at the bottom right of the window, closed with its ×.
pub fn toast(
    message: impl Into<SharedString>,
    kind: Kind,
    on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    window: &Window,
) -> impl IntoElement {
    let (icon, color) = kind.icon();
    let viewport = window.viewport_size();
    deferred(
        anchored()
            .position(point(viewport.width - px(24.), viewport.height - px(12.)))
            .anchor(Anchor::BottomRight)
            .child(
                div()
                    .occlude()
                    .h_flex()
                    .w(px(384.))
                    .px_3()
                    .py_2()
                    .bg(white())
                    .border_1()
                    .border_color(books::color(color, 300))
                    .rounded(books::RADIUS_LG)
                    .shadow(books::shadow_lg())
                    .text_color(gray(900))
                    .child(books::icon(icon, px(24.), books::color(color, 400)).mr_3())
                    .child(div().flex_1().text_13().child(message.into()))
                    .child(
                        div()
                            .id("toast-close")
                            .ml_2()
                            .cursor_pointer()
                            .child(books::icon("x", px(16.), gray(600)))
                            .on_click(on_close),
                    ),
            ),
    )
    .with_priority(1)
}
