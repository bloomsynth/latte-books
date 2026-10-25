//! The search window: `src/components/SearchBar.vue` @ v0.36.0, opened from the search
//! button beside Back and Forward, or with Ctrl+K (⌘K).
//!
//! Type to find documents, lists, reports and pages; ↑↓ move, Enter opens, Esc closes.
//! The groups at the foot turn each kind on or off. The search is `latte_core::views::search`.
//!
//! Differences from Books: no Recent group, and no More Filters.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use latte_core::views::search::{SearchGroup, SearchItem, SearchTarget, Searcher};

use crate::books::{self, Text as _, gray};
use crate::i18n::t;
use crate::session::Shared;

/// What the search window asks of the desk.
pub enum SearchEvent {
    Open(SearchTarget),
    Close,
}

pub struct SearchBar {
    searcher: Option<Searcher>,
    input: Entity<InputState>,
    items: Vec<SearchItem>,
    /// The highlighted suggestion.
    idx: usize,
    /// How many suggestions show; `usize::MAX` for all.
    limit: usize,
    error: Option<String>,
    _subscription: Subscription,
}

impl EventEmitter<SearchEvent> for SearchBar {}

/// `allowedLimits`.
const LIMITS: [usize; 4] = [50, 100, 500, usize::MAX];

impl SearchBar {
    pub fn new(session: &Shared, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (searcher, error) = match Searcher::new(&session.borrow().fyo) {
            Ok(searcher) => (Some(searcher), None),
            Err(e) => (None, Some(e.to_string())),
        };
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(t("Type to search...")));
        input.update(cx, |input, cx| input.focus(window, cx));
        let subscription = cx.subscribe_in(&input, window, |this, _, event: &InputEvent, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.idx = 0;
                this.refresh(cx);
            }
        });
        let mut bar = Self {
            searcher,
            input,
            items: Vec::new(),
            idx: 0,
            limit: LIMITS[0],
            error,
            _subscription: subscription,
        };
        bar.refresh(cx);
        bar
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().to_string();
        self.items = self
            .searcher
            .as_ref()
            .map(|s| s.search(&text))
            .unwrap_or_default();
        cx.notify();
    }

    fn shown(&self) -> usize {
        self.items.len().min(self.limit)
    }

    fn select(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(item) = self.items.get(i) {
            cx.emit(SearchEvent::Open(item.target.clone()));
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let count = self.shown();
        match event.keystroke.key.as_str() {
            "down" if count > 0 => self.idx = (self.idx + 1) % count,
            "up" if count > 0 => self.idx = (self.idx + count - 1) % count,
            "enter" => self.select(self.idx, cx),
            "escape" => cx.emit(SearchEvent::Close),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for SearchBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let shown = self.shown();
        let rows: Vec<AnyElement> = self.items[..shown]
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let active = i == self.idx;
                let more: Vec<&str> = item
                    .more
                    .iter()
                    .map(String::as_str)
                    .filter(|m| !m.is_empty())
                    .collect();
                let right = match (&item.group, &item.schema_label) {
                    (SearchGroup::Docs, Some(label)) => label.clone(),
                    _ => t(item.group.label()),
                };
                div()
                    .id(("suggestion", i))
                    .flex_none()
                    .cursor_pointer()
                    .hover(|d| d.bg(gray(50)))
                    .when(active, |d| {
                        d.bg(gray(50)).border_l_4().border_color(gray(700))
                    })
                    .when(i + 1 < shown, |d| d.border_b_1().border_color(gray(200)))
                    .child(
                        div()
                            .h_flex()
                            .h_12()
                            .px_3()
                            .justify_between()
                            .child(
                                div()
                                    .h_flex()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .when(active, |d| d.ml(px(-4.)).text_color(gray(900)))
                                            .when(!active, |d| d.text_color(gray(700)))
                                            .child(item.label.clone()),
                                    )
                                    .when(!more.is_empty(), |d| {
                                        d.child(
                                            div()
                                                .ml_3()
                                                .text_12()
                                                .text_color(gray(600))
                                                .truncate()
                                                .child(more.join(", ")),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .ml_2()
                                    .text_12()
                                    .text_color(books::color(item.group.color(), 500))
                                    .child(right),
                            ),
                    )
                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if this.idx != i {
                            this.idx = i;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| this.select(i, cx)))
                    .into_any_element()
            })
            .collect();
        let hr = || div().w_full().h(px(1.)).flex_none().bg(gray(200));
        let total = self.items.len();

        // The group filters: on in the group's colours, off in its text colour only.
        let groups = SearchGroup::ALL.into_iter().map(|group| {
            let on = self.searcher.as_ref().is_some_and(|s| s.group_on(group));
            let color = group.color();
            let (bg, fg) = books::bg_text(color);
            div()
                .id(SharedString::from(format!("group-{}", group.label())))
                .px_1()
                .py(px(2.))
                .rounded(books::RADIUS_LG)
                .border_1()
                .border_color(books::color(color, 100))
                .cursor_pointer()
                .when(on, |d| d.bg(bg).text_color(fg))
                .when(!on, |d| d.text_color(books::color(color, 600)))
                .child(t(group.label()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(searcher) = this.searcher.as_mut() {
                        searcher.set_group(group, !on);
                    }
                    this.idx = 0;
                    this.refresh(cx);
                }))
        });

        div()
            .v_flex()
            .w(books::W_FORM)
            .capture_key_down(cx.listener(Self::on_key_down))
            .child(
                div().p_1().child(
                    div().rounded(books::RADIUS_MD).bg(gray(100)).child(
                        Input::new(&self.input)
                            .appearance(false)
                            .h(px(54.))
                            .px_3()
                            .text_20(),
                    ),
                ),
            )
            .when(!rows.is_empty(), |d| d.child(hr()))
            .children(self.error.clone().map(|e| {
                div()
                    .p_3()
                    .text_12()
                    .text_color(books::color("red", 600))
                    .child(e)
            }))
            .child(
                div()
                    .id("suggestions")
                    .max_h(px(49. * 6. - 1.))
                    .overflow_y_scroll()
                    .children(rows),
            )
            .child(hr())
            .child(
                div()
                    .v_flex()
                    .m_1()
                    .gap_2()
                    .text_12()
                    .child(div().h_flex().gap_1().children(groups))
                    .child(
                        div()
                            .h_flex()
                            .justify_between()
                            .text_color(gray(500))
                            .child(
                                div()
                                    .h_flex()
                                    .gap_4()
                                    .child(format!("↑↓ {}", t("Navigate")))
                                    .child(format!("↩ {}", t("Select")))
                                    .child(format!("esc {}", t("Close"))),
                            )
                            .child(
                                div()
                                    .h_flex()
                                    .gap_2()
                                    .when(total > 0, |d| {
                                        d.child(format!("{shown} {} {total}", t("out of")))
                                    })
                                    .when(total > LIMITS[0], |d| {
                                        d.child(
                                            div()
                                                .h_flex()
                                                .border_1()
                                                .border_color(gray(100))
                                                .rounded(books::RADIUS)
                                                .children(LIMITS.into_iter().map(|limit| {
                                                    div()
                                                        .id(SharedString::from(format!(
                                                            "search-limit-{limit}"
                                                        )))
                                                        .w_9()
                                                        .h_flex()
                                                        .justify_center()
                                                        .cursor_pointer()
                                                        .when(self.limit == limit, |d| {
                                                            d.bg(gray(100)).rounded(books::RADIUS)
                                                        })
                                                        .child(if limit == usize::MAX {
                                                            t("All")
                                                        } else {
                                                            limit.to_string()
                                                        })
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                this.limit = limit;
                                                                cx.notify();
                                                            },
                                                        ))
                                                })),
                                        )
                                    }),
                            ),
                    ),
            )
    }
}
