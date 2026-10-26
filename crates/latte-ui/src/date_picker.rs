//! The calendar a Date or Datetime control opens: Books' `Date.vue` and `Datetime.vue`
//! call the browser's `showPicker()` on their `<input type="date">` and
//! `type="datetime-local"`, so this draws Chromium's picker.
//!
//! The month and year with ↑ and ↓ to step months (a click on the month shows the year's
//! months), the days of the month from Sunday, with the picked day filled and today
//! outlined, then Clear and Today. A Datetime picker adds an hour and a minute column. A
//! date picker closes on a pick; a datetime picker when clicked outside.
//!
//! The host anchors the panel under its control and listens for [`DatePicked`] and
//! [`PickerClosed`].

use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use jiff::ToSpan as _;
use jiff::civil::{Date, DateTime, Time};

use crate::books::{self, Text as _, gray};
use crate::i18n::t;
use crate::widgets::popover_panel;

/// A value was picked, or cleared (`None`).
pub struct DatePicked(pub Option<DateTime>);

/// The picker is to close.
pub struct PickerClosed;

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

pub struct DatePicker {
    /// The first day of the month shown.
    month: Date,
    value: Option<DateTime>,
    with_time: bool,
    /// The year's months are shown instead of the days.
    months_view: bool,
    hours: ScrollHandle,
    minutes: ScrollHandle,
    /// The time columns were scrolled to the value once shown.
    scrolled: bool,
}

impl EventEmitter<DatePicked> for DatePicker {}
impl EventEmitter<PickerClosed> for DatePicker {}

fn today() -> Date {
    jiff::Zoned::now().date()
}

fn first_of(date: Date) -> Date {
    date.first_of_month()
}

impl DatePicker {
    /// A picker showing `value`'s month, or this month. `with_time` adds the time columns.
    pub fn new(value: Option<DateTime>, with_time: bool) -> Self {
        let month = first_of(value.map_or_else(today, |v| v.date()));
        Self {
            month,
            value,
            with_time,
            months_view: false,
            hours: ScrollHandle::new(),
            minutes: ScrollHandle::new(),
            scrolled: false,
        }
    }

    fn step(&mut self, months: i32, cx: &mut Context<Self>) {
        if let Ok(month) = self.month.checked_add(months.months()) {
            self.month = month;
        }
        cx.notify();
    }

    fn pick(&mut self, date: Date, cx: &mut Context<Self>) {
        let time = self.value.map_or(Time::midnight(), |v| v.time());
        let value = date.to_datetime(time);
        self.value = Some(value);
        self.month = first_of(date);
        cx.emit(DatePicked(Some(value)));
        if !self.with_time {
            cx.emit(PickerClosed);
        }
        cx.notify();
    }

    fn pick_time(&mut self, hour: Option<i8>, minute: Option<i8>, cx: &mut Context<Self>) {
        let current = self
            .value
            .unwrap_or_else(|| today().to_datetime(Time::midnight()));
        let time = Time::new(
            hour.unwrap_or(current.hour()),
            minute.unwrap_or(current.minute()),
            0,
            0,
        )
        .unwrap_or(Time::midnight());
        let value = current.date().to_datetime(time);
        self.value = Some(value);
        cx.emit(DatePicked(Some(value)));
        cx.notify();
    }

    fn header(&self, cx: &mut Context<Self>) -> Div {
        let title = if self.months_view {
            format!("{}", self.month.year())
        } else {
            format!(
                "{} {}",
                t(MONTHS[self.month.month() as usize - 1]),
                self.month.year()
            )
        };
        let step = if self.months_view { 12 } else { 1 };
        let arrow = |id: &'static str, icon: &'static str, by: i32| {
            div()
                .id(id)
                .p_1()
                .rounded(books::RADIUS)
                .cursor_pointer()
                .hover(|d| d.bg(gray(100)))
                .child(books::icon(icon, px(14.), gray(700)))
                .on_click(cx.listener(move |this, _, _, cx| this.step(by, cx)))
        };
        div()
            .h_flex()
            .justify_between()
            .px_1()
            .pb_2()
            .child(
                div()
                    .id("picker-title")
                    .h_flex()
                    .gap_1()
                    .px_1()
                    .rounded(books::RADIUS)
                    .cursor_pointer()
                    .hover(|d| d.bg(gray(100)))
                    .text_13()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title)
                    .child(books::icon("chevron-down", px(12.), gray(700)))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.months_view = !this.months_view;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .h_flex()
                    .child(arrow("picker-prev", "arrow-up", -step))
                    .child(arrow("picker-next", "arrow-down", step)),
            )
    }

    fn days(&self, cx: &mut Context<Self>) -> Div {
        let selected = self.value.map(|v| v.date());
        let today = today();
        // Sunday first, as Chromium does in English.
        let offset = self.month.weekday().to_sunday_zero_offset() as i32;
        let start = self.month.checked_sub(offset.days()).unwrap_or(self.month);
        let weekdays = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];
        let cell = || {
            div()
                .w(px(30.))
                .h(px(26.))
                .flex()
                .items_center()
                .justify_center()
        };
        let mut rows = vec![
            div()
                .h_flex()
                .text_11()
                .text_color(gray(600))
                .children(weekdays.map(|d| cell().child(t(d)))),
        ];
        for week in 0..6 {
            let mut row = div().h_flex();
            for day in 0..7 {
                let n = week * 7 + day;
                let Ok(date) = start.checked_add(n.days()) else {
                    continue;
                };
                let in_month = date.month() == self.month.month();
                let is_selected = selected == Some(date);
                row = row.child(
                    cell()
                        .id(("day", n as usize))
                        .rounded(books::RADIUS)
                        .text_12()
                        .cursor_pointer()
                        .map(|d| match (is_selected, in_month) {
                            (true, _) => d.bg(books::color("blue", 600)).text_color(gray(25)),
                            (false, true) => d.text_color(gray(900)).hover(|d| d.bg(gray(100))),
                            (false, false) => d.text_color(gray(400)).hover(|d| d.bg(gray(100))),
                        })
                        .when(date == today && !is_selected, |d| {
                            d.border_1().border_color(books::color("blue", 600))
                        })
                        .child(date.day().to_string())
                        .on_click(cx.listener(move |this, _, _, cx| this.pick(date, cx))),
                );
            }
            rows.push(row);
        }
        div().v_flex().children(rows)
    }

    fn months(&self, cx: &mut Context<Self>) -> Div {
        let year = self.month.year();
        let shown = self.value.map(|v| (v.year(), v.month()));
        div()
            .grid()
            .grid_cols(3)
            .gap_1()
            .w(px(210.))
            .children(MONTHS.iter().enumerate().map(|(i, name)| {
                let month = i as i8 + 1;
                let selected = shown == Some((year, month));
                div()
                    .id(("month", i))
                    .h(px(36.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(books::RADIUS)
                    .text_12()
                    .cursor_pointer()
                    .map(|d| {
                        if selected {
                            d.bg(books::color("blue", 600)).text_color(gray(25))
                        } else {
                            d.hover(|d| d.bg(gray(100)))
                        }
                    })
                    .child(t(name).chars().take(3).collect::<String>())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Ok(date) = Date::new(year, month, 1) {
                            this.month = date;
                        }
                        this.months_view = false;
                        cx.notify();
                    }))
            }))
    }

    fn time_column(
        &self,
        id: &'static str,
        count: i8,
        current: Option<i8>,
        hour: bool,
        scroll: &ScrollHandle,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        div()
            .id(id)
            .v_flex()
            .w(px(40.))
            .h(px(212.))
            .overflow_y_scroll()
            .track_scroll(scroll)
            .border_l_1()
            .border_color(gray(200))
            .children((0..count).map(|n| {
                div()
                    .id((id, n as usize))
                    .flex_none()
                    .h(px(26.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_12()
                    .cursor_pointer()
                    .map(|d| {
                        if current == Some(n) {
                            d.bg(books::color("blue", 600)).text_color(gray(25))
                        } else {
                            d.hover(|d| d.bg(gray(100)))
                        }
                    })
                    .child(format!("{n:02}"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if hour {
                            this.pick_time(Some(n), None, cx)
                        } else {
                            this.pick_time(None, Some(n), cx)
                        }
                    }))
            }))
    }
}

impl Render for DatePicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !std::mem::replace(&mut self.scrolled, true)
            && let Some(v) = self.value
        {
            // The value's hour and minute a few rows down their columns.
            let offset = |n: i8, count: f32| {
                let top = (26. * (n as f32 - 3.)).clamp(0., 26. * count - 212.);
                point(px(0.), px(-top))
            };
            self.hours.set_offset(offset(v.hour(), 24.));
            self.minutes.set_offset(offset(v.minute(), 60.));
        }
        let link = |id: &'static str, label: String| {
            div()
                .id(id)
                .px_2()
                .py_1()
                .rounded(books::RADIUS)
                .text_12()
                .text_color(books::color("blue", 600))
                .cursor_pointer()
                .hover(|d| d.bg(gray(100)))
                .child(label)
        };
        let footer = div()
            .h_flex()
            .justify_between()
            .pt_2()
            .child(
                link("picker-clear", t("Clear")).on_click(cx.listener(|_, _, _, cx| {
                    cx.emit(DatePicked(None));
                    cx.emit(PickerClosed);
                })),
            )
            .child(
                link("picker-today", t("Today"))
                    .on_click(cx.listener(|this, _, _, cx| this.pick(today(), cx))),
            );
        let calendar = div()
            .v_flex()
            .p_2()
            .child(self.header(cx))
            .child(if self.months_view {
                self.months(cx)
            } else {
                self.days(cx)
            })
            .child(footer);
        let time = self.with_time.then(|| {
            let hours = self.hours.clone();
            let minutes = self.minutes.clone();
            div()
                .h_flex()
                .items_start()
                .py_2()
                .child(self.time_column(
                    "picker-hours",
                    24,
                    self.value.map(|v| v.hour()),
                    true,
                    &hours,
                    cx,
                ))
                .child(self.time_column(
                    "picker-minutes",
                    60,
                    self.value.map(|v| v.minute()),
                    false,
                    &minutes,
                    cx,
                ))
        });
        popover_panel()
            .id("date-picker")
            .occlude()
            .h_flex()
            .items_start()
            .text_color(gray(900))
            .on_mouse_down_out(cx.listener(|_, _, _, cx| cx.emit(PickerClosed)))
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(PickerClosed);
                    cx.stop_propagation();
                }
            }))
            .child(calendar)
            .children(time)
    }
}

/// The panel under its control, whose parent is `relative`: deferred, so it draws over
/// what follows.
pub fn anchored_picker(picker: &Entity<DatePicker>) -> Div {
    // `anchored` is laid out absolutely, so it goes in a corner of its own.
    div().absolute().left_0().bottom_0().size_0().child(
        deferred(
            anchored()
                .snap_to_window()
                .child(div().mt_1().child(picker.clone())),
        )
        .with_priority(1),
    )
}
