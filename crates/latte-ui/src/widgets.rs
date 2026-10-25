//! Small pieces the screens share, drawn as Books draws them: buttons, page headers,
//! badges, messages, labels and the controls' frames.

use std::rc::Rc;
use std::time::Duration;

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::books::{self, Text as _, black, gray, white};

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
type MouseDownHandler = dyn Fn(&MouseDownEvent, &mut Window, &mut App);
type ClickFn = dyn Fn(&ClickEvent, &mut Window, &mut App);

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

    #[allow(dead_code)]
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
        // `bg-black text-white dark:bg-gray-300 dark:text-black`, or
        // `bg-gray-200 text-gray-700 dark:bg-gray-900 dark:text-gray-200`.
        let (bg, fg) = if self.primary {
            (
                books::pick(black(), books::raw_gray(300)),
                books::pick(white(), books::hex(0x1E293B)),
            )
        } else {
            (
                books::pick(books::raw_gray(200), books::raw_gray(900)),
                books::pick(books::raw_gray(700), books::raw_gray(200)),
            )
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

/// `PageHeaderNavGroup.vue`: Search, Back and Forward, joined; Back and Forward gray
/// until there is somewhere to go.
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
            .bg(books::pick(books::raw_gray(200), books::raw_gray(900)))
            .when(enabled, |d| d.cursor_pointer())
            .child(books::icon(
                icon,
                px(16.),
                if enabled {
                    gray(700)
                } else {
                    books::pick(books::raw_gray(400), books::raw_gray(700))
                },
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
    // `SearchBar.vue`'s button, joined to Back and Forward.
    let search = div()
        .id("nav-search")
        .h_flex()
        .h_8()
        .px_3()
        .bg(books::pick(books::raw_gray(200), books::raw_gray(900)))
        .rounded_l(books::RADIUS_MD)
        .cursor_pointer()
        .child(books::icon("search", px(16.), gray(700)))
        .on_click(|_, window, cx| {
            if let Some(desk) = cx
                .try_global::<crate::desk::NavState>()
                .and_then(|n| n.desk.upgrade())
            {
                desk.update(cx, |desk, cx| desk.open_search(window, cx));
            }
        });
    div()
        .h_flex()
        .flex_none()
        .rounded(books::RADIUS_MD)
        .overflow_hidden()
        .child(search)
        .child(
            link("nav-back", "chevron-left", can_back, true)
                .border_l_1()
                .border_r_1()
                .border_color(books::pick(white(), books::raw_gray(850))),
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
        .bg(books::surface())
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
            (false, true) => d
                .bg(books::pick(books::raw_gray(100), books::raw_gray(850)))
                .text_color(gray(900)),
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
        .bg(books::surface())
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
        .hover(|d| d.bg(books::pick(books::raw_gray(100), books::raw_gray(875))))
        .child(label)
}

/// What a dialog or toast is about (`ToastType`), which sets its icon and colour
/// (`getIconConfig`).
#[derive(Clone, Copy, PartialEq, Debug)]
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

/// A button of a [`dialog`]: its label, whether primary, what Esc presses, and what it
/// does.
pub struct DialogButton {
    pub label: SharedString,
    pub primary: bool,
    /// `isEscape`: the button Esc presses.
    pub escape: bool,
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
            escape: false,
            on_click: Box::new(on_click),
        }
    }

    /// `isEscape: true`.
    pub fn escape(mut self) -> Self {
        self.escape = true;
        self
    }
}

/// `handleEscape`: Esc presses a lone button whatever it is; of several, the one marked
/// `isEscape`, or else the first that isn't primary, the No or Cancel Books marks so.
fn escape_index(buttons: &[DialogButton]) -> Option<usize> {
    if buttons.len() == 1 {
        return Some(0);
    }
    buttons
        .iter()
        .position(|b| b.escape)
        .or_else(|| buttons.iter().position(|b| !b.primary))
}

/// `focusButton`: the primary button has focus, or else the first, so Enter presses it.
fn enter_index(buttons: &[DialogButton]) -> Option<usize> {
    buttons
        .iter()
        .position(|b| b.primary)
        .or((!buttons.is_empty()).then_some(0))
}

/// `shadow-2xl`.
fn shadow_2xl() -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: hsla(0., 0., 0., 0.25),
        offset: point(px(0.), px(25.)),
        blur_radius: px(50.),
        spread_radius: px(-12.),
        inset: false,
    }]
}

/// `Dialog.vue`: a 384px card over a dimmed window, its title and icon on top, the
/// detail below, and the buttons at the end. It takes the keyboard: Esc presses the
/// escape button and Enter the primary one.
pub fn dialog(
    title: impl Into<SharedString>,
    detail: Option<SharedString>,
    kind: Kind,
    buttons: Vec<DialogButton>,
    window: &Window,
) -> Deferred {
    let viewport = window.viewport_size();
    deferred(
        anchored()
            .position(point(px(0.), px(0.)))
            .child(DialogCard {
                title: title.into(),
                detail,
                kind,
                buttons,
                viewport,
            }),
    )
    .with_priority(2)
}

#[derive(IntoElement)]
struct DialogCard {
    title: SharedString,
    detail: Option<SharedString>,
    kind: Kind,
    buttons: Vec<DialogButton>,
    viewport: gpui_kit::Size<Pixels>,
}

impl RenderOnce for DialogCard {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (icon, color) = self.kind.icon();
        let focus = window
            .use_keyed_state("dialog-focus", cx, |window, cx| {
                let focus = cx.focus_handle();
                focus.focus(window, cx);
                focus
            })
            .read(cx)
            .clone();
        let escape = escape_index(&self.buttons);
        let enter = enter_index(&self.buttons);
        let mut handlers: Vec<Rc<ClickHandler>> = Vec::new();
        let mut buttons = Vec::new();
        for (i, b) in self.buttons.into_iter().enumerate() {
            let handler: Rc<ClickHandler> = Rc::new(b.on_click);
            handlers.push(handler.clone());
            let button = Button::new(("dialog-button", i))
                .label(b.label)
                .min_w(px(80.))
                .on_click(move |e, window, cx| handler(e, window, cx));
            buttons.push(if b.primary { button.primary() } else { button });
        }
        div()
            .id("dialog-backdrop")
            .occlude()
            .track_focus(&focus)
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                let pressed = match event.keystroke.key.as_str() {
                    "escape" => escape,
                    "enter" => enter,
                    _ => return,
                };
                cx.stop_propagation();
                if let Some(handler) = pressed.and_then(|i| handlers.get(i)) {
                    handler(&ClickEvent::default(), window, cx);
                }
            })
            .w(self.viewport.width)
            .h(self.viewport.height)
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
                    .bg(books::surface())
                    .border_1()
                    .border_color(gray(200))
                    .rounded(books::RADIUS_LG)
                    .shadow(shadow_2xl())
                    .text_color(books::pick(gray(900), books::raw_gray(25)))
                    .child(
                        div()
                            .h_flex()
                            .justify_between()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(self.title))
                            .child(books::icon(icon, px(24.), books::color(color, 400))),
                    )
                    .children(self.detail.map(|d| div().text_13().child(d)))
                    .child(
                        div()
                            .h_flex()
                            .justify_end()
                            .gap_4()
                            .mt_4()
                            .children(buttons),
                    ),
            )
    }
}

/// The unexpected-error dialog (`handleErrorWithDialog` with `reportError`): the error,
/// cut to 128 characters, with Report, which opens a new issue about it, and Cancel.
pub fn error_dialog(error: &str, on_close: Action, window: &Window) -> Deferred {
    let url = latte_core::views::report::issue_url(Some(error), &report_info());
    let close = on_close.clone();
    dialog(
        crate::i18n::t("Error"),
        Some(latte_core::views::report::truncate(error, 128).into()),
        Kind::Error,
        vec![
            DialogButton::new(crate::i18n::t("Report"), true, move |_, window, cx| {
                cx.open_url(&url);
                close(window, cx);
            }),
            DialogButton::new(crate::i18n::t("Cancel"), false, move |_, window, cx| {
                on_close(window, cx)
            })
            .escape(),
        ],
        window,
    )
}

/// What an issue report says about the app (`getIssueUrlQuery`'s Info).
pub fn report_info() -> latte_core::views::report::ReportInfo {
    latte_core::views::report::ReportInfo {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        platform: match std::env::consts::OS {
            "windows" => "Windows",
            "macos" => "Mac",
            _ => "Linux",
        }
        .to_owned(),
        language: crate::i18n::language(),
    }
}

/// How long a toast stays (`toastDurationMap`).
#[derive(Clone, Copy, PartialEq)]
pub enum ToastDuration {
    Short,
    Long,
}

impl ToastDuration {
    fn time(self) -> Duration {
        match self {
            ToastDuration::Short => Duration::from_millis(2_500),
            ToastDuration::Long => Duration::from_millis(5_000),
        }
    }
}

type Action = Rc<dyn Fn(&mut Window, &mut App)>;

/// `ToastOptions`: the message, its kind, how long it stays, and an action under it.
#[derive(Clone)]
pub struct Toast {
    pub message: SharedString,
    pub kind: Kind,
    pub duration: ToastDuration,
    pub action: Option<(SharedString, Action)>,
}

impl Toast {
    /// A toast that stays for Books' default `long`.
    pub fn new(message: impl Into<SharedString>, kind: Kind) -> Self {
        Self {
            message: message.into(),
            kind,
            duration: ToastDuration::Long,
            action: None,
        }
    }

    pub fn short(mut self) -> Self {
        self.duration = ToastDuration::Short;
        self
    }

    /// `actionText` and `action`: clicking the message runs it and closes the toast.
    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        action: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.action = Some((label.into(), Rc::new(action)));
        self
    }

    /// `showExportInFolder`: "Open Folder" shows the file in its folder.
    pub fn open_folder(self, path: &std::path::Path) -> Self {
        let path = path.to_owned();
        self.action(crate::i18n::t("Open Folder"), move |_, cx| {
            cx.reveal_path(&path)
        })
    }
}

/// The toasts shown, oldest first (`#toast-container`).
#[derive(Default)]
struct Toasts {
    shown: Vec<(usize, Toast)>,
    next: usize,
}

impl Global for Toasts {}

/// `showToast`: shows a toast at the window's bottom right until its time is up or it is
/// closed.
pub fn show_toast(toast: Toast, window: &Window, cx: &mut App) {
    let duration = toast.duration.time();
    let toasts = cx.default_global::<Toasts>();
    let id = toasts.next;
    toasts.next += 1;
    toasts.shown.push((id, toast));
    window
        .spawn(cx, async move |cx| {
            cx.background_executor().timer(duration).await;
            cx.update(|_, cx| close_toast(id, cx)).ok();
        })
        .detach();
    cx.refresh_windows();
}

fn close_toast(id: usize, cx: &mut App) {
    if let Some(toasts) = cx.try_global::<Toasts>()
        && toasts.shown.iter().any(|(i, _)| *i == id)
    {
        cx.global_mut::<Toasts>().shown.retain(|(i, _)| *i != id);
        cx.refresh_windows();
    }
}

/// The toasts of [`show_toast`], stacked at the window's bottom right; drawn by the
/// window's root.
pub fn toasts(window: &Window, cx: &App) -> Option<impl IntoElement + use<>> {
    let shown = cx.try_global::<Toasts>()?;
    if shown.shown.is_empty() {
        return None;
    }
    let viewport = window.viewport_size();
    let cards: Vec<Stateful<Div>> = shown
        .shown
        .iter()
        .map(|(id, toast)| {
            let id = *id;
            toast_card(
                ("toast", id),
                toast,
                Rc::new(move |_: &mut Window, cx: &mut App| close_toast(id, cx)),
            )
            .mb_3()
        })
        .collect();
    Some(
        deferred(
            anchored()
                .position(point(viewport.width - px(24.), viewport.height - px(12.)))
                .anchor(Anchor::BottomRight)
                .child(div().v_flex().items_end().children(cards)),
        )
        .with_priority(3),
    )
}

/// `Toast.vue`: a 384px note, its icon, the message with its action under it, and ×.
fn toast_card(id: impl Into<ElementId>, toast: &Toast, close: Action) -> Stateful<Div> {
    let (icon, color) = toast.kind.icon();
    let id: ElementId = id.into();
    let action = toast.action.clone();
    let on_message = close.clone();
    div()
        .id(id.clone())
        .occlude()
        .h_flex()
        .w(px(384.))
        .px_3()
        .py_2()
        .bg(books::surface())
        .border_1()
        .border_color(books::color(color, 300))
        .rounded(books::RADIUS_LG)
        .shadow(books::shadow_lg())
        .text_color(books::pick(gray(900), books::raw_gray(25)))
        .child(books::icon(icon, px(24.), books::color(color, 400)).mr_3())
        .child(
            div()
                .id((id.clone(), "message"))
                .flex_1()
                .min_w_0()
                .when(action.is_some(), |d| d.cursor_pointer())
                .child(
                    div()
                        .text_13()
                        .whitespace_normal()
                        .child(toast.message.clone()),
                )
                .children(action.as_ref().map(|(label, _)| {
                    div()
                        .text_12()
                        .text_color(gray(700))
                        .hover(|d| d.text_color(gray(800)))
                        .child(label.clone())
                }))
                .on_click(move |_, window, cx| {
                    if let Some((_, action)) = &action {
                        action(window, cx);
                        on_message(window, cx);
                    }
                }),
        )
        .child(
            div()
                .id((id, "close"))
                .ml_2()
                .cursor_pointer()
                .child(books::icon("x", px(16.), gray(600)))
                .on_click(move |_, window, cx| close(window, cx)),
        )
}

/// `Toast.vue` drawn by a page that keeps its message itself: a 384px note at the bottom
/// right of the window, closed with its × or after Books' `long` five seconds.
pub fn toast(
    message: impl Into<SharedString>,
    kind: Kind,
    on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    window: &Window,
) -> Deferred {
    let viewport = window.viewport_size();
    deferred(
        anchored()
            .position(point(viewport.width - px(24.), viewport.height - px(24.)))
            .anchor(Anchor::BottomRight)
            .child(PageToast {
                message: message.into(),
                kind,
                on_close: Rc::new(on_close),
            }),
    )
    .with_priority(1)
}

#[derive(IntoElement)]
struct PageToast {
    message: SharedString,
    kind: Kind,
    on_close: Rc<ClickFn>,
}

impl RenderOnce for PageToast {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let on_close = self.on_close.clone();
        // The timer lives while this message is shown; a new message starts another.
        let key = SharedString::from(format!("page-toast-{:?}-{}", self.kind, self.message));
        window.use_keyed_state(ElementId::Name(key), cx, |window, cx| {
            let on_close = on_close.clone();
            cx.spawn_in(window, async move |_, cx| {
                cx.background_executor()
                    .timer(ToastDuration::Long.time())
                    .await;
                cx.update(|window, cx| on_close(&ClickEvent::default(), window, cx))
                    .ok();
            })
        });
        let close = self.on_close;
        toast_card(
            "toast",
            &Toast::new(self.message, self.kind),
            Rc::new(move |window: &mut Window, cx: &mut App| {
                close(&ClickEvent::default(), window, cx)
            }),
        )
    }
}

/// A text input in Books' small bordered frame (`Base.vue`, `size="small"`).
pub fn small_input(state: &Entity<gpui_kit::component::input::InputState>) -> Div {
    use gpui_kit::component::input::Input;
    control_frame(false, false).min_h(px(30.)).child(
        Input::new(state)
            .appearance(false)
            .h(px(28.))
            .px_2()
            .text_13(),
    )
}

/// `Modal.vue`: a panel over the dimmed window, closed by a click outside it or by Esc.
pub fn modal(
    content: impl IntoElement,
    on_close: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    window: &Window,
) -> Deferred {
    let viewport = window.viewport_size();
    deferred(
        anchored()
            .position(point(px(0.), px(0.)))
            .child(ModalFrame {
                content: content.into_any_element(),
                on_close: Rc::new(on_close),
                viewport,
            }),
    )
    .with_priority(2)
}

#[derive(IntoElement)]
struct ModalFrame {
    content: AnyElement,
    on_close: Rc<MouseDownHandler>,
    viewport: gpui_kit::Size<Pixels>,
}

impl RenderOnce for ModalFrame {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        // The modal takes the keyboard for Esc, unless what it shows took it first, such
        // as the search's input.
        let focus = window
            .use_keyed_state("modal-focus", cx, |window, cx| {
                let focus = cx.focus_handle();
                let handle = focus.clone();
                window.on_next_frame(move |window, cx| {
                    if !handle.contains_focused(window, cx) {
                        handle.focus(window, cx);
                    }
                });
                focus
            })
            .read(cx)
            .clone();
        let on_close = self.on_close;
        let on_escape = on_close.clone();
        div()
            .id("modal-backdrop")
            .occlude()
            .track_focus(&focus)
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    on_escape(&MouseDownEvent::default(), window, cx);
                }
            })
            .w(self.viewport.width)
            .h(self.viewport.height)
            .flex()
            .items_center()
            .justify_center()
            .bg(hsla(0., 0., 0., 0.1))
            .on_mouse_down(MouseButton::Left, move |e, window, cx| {
                on_close(e, window, cx)
            })
            .child(
                div()
                    .id("modal")
                    .bg(books::surface())
                    .rounded(books::RADIUS_LG)
                    .border_1()
                    .border_color(gray(200))
                    .overflow_hidden()
                    .shadow(shadow_2xl())
                    .text_color(gray(900))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(self.content),
            )
    }
}

/// `Loading.vue`: a 384px card at the window's bottom right with a message and a bar,
/// empty and darker while the percent is below 0 (not known yet).
pub fn loading(message: impl Into<SharedString>, percent: f64, window: &Window) -> Deferred {
    let message: SharedString = message.into();
    let viewport = window.viewport_size();
    deferred(
        anchored()
            .position(point(viewport.width - px(24.), viewport.height - px(24.)))
            .anchor(Anchor::BottomRight)
            .child(
                div()
                    .occlude()
                    .v_flex()
                    .w(px(384.))
                    .p_3()
                    .bg(books::pick(white(), books::raw_gray(900)))
                    .border_1()
                    .border_color(gray(200))
                    .rounded(books::RADIUS_LG)
                    .shadow(books::shadow_lg())
                    .when(!message.is_empty(), |d| {
                        d.child(
                            div()
                                .pb_2()
                                .text_13()
                                .text_color(gray(600))
                                .child(message.clone()),
                        )
                    })
                    .child(
                        div()
                            .w_full()
                            .h_3()
                            .mr_2()
                            .rounded(books::RADIUS)
                            .bg(if percent >= 0. { gray(200) } else { gray(300) })
                            .when(percent >= 0., |d| {
                                d.child(
                                    div()
                                        .h_3()
                                        .rounded(books::RADIUS)
                                        .bg(gray(800))
                                        .w(relative(percent.clamp(0., 1.) as f32)),
                                )
                            }),
                    ),
            ),
    )
    .with_priority(1)
}

#[cfg(test)]
mod tests {
    use super::{DialogButton, enter_index, escape_index};

    fn button(primary: bool) -> DialogButton {
        DialogButton::new("b", primary, |_, _, _| {})
    }

    #[test]
    fn esc_and_enter_press_books_buttons() {
        // A lone button is pressed by both.
        let one = [button(false)];
        assert_eq!(escape_index(&one), Some(0));
        assert_eq!(enter_index(&one), Some(0));
        // Yes (primary) and No: Esc is No, Enter is Yes.
        let yes_no = [button(true), button(false)];
        assert_eq!(escape_index(&yes_no), Some(1));
        assert_eq!(enter_index(&yes_no), Some(0));
        // A button marked `isEscape` wins.
        let marked = [button(false), button(false).escape(), button(true)];
        assert_eq!(escape_index(&marked), Some(1));
        assert_eq!(enter_index(&marked), Some(2));
        assert_eq!(escape_index(&[]), None);
        assert_eq!(enter_index(&[]), None);
    }
}
