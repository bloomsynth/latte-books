//! The search window: `src/components/SearchBar.vue` @ v0.36.0, opened from the search
//! button beside Back and Forward, or with Ctrl+K (⌘K).
//!
//! Type to find documents, lists, reports and pages; ↑↓ move, Enter opens, Esc closes.
//! The groups at the foot turn each kind on or off, as do Ctrl+1 to Ctrl+6; More Filters
//! skips child tables, transactions or any document type. What was opened is listed under
//! Recent. The search is `latte_core::views::search`.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use latte_core::views::search::{SearchGroup, SearchItem, SearchTarget, Searcher};

use crate::books::{self, Text as _, gray};
use crate::i18n::t;
use crate::session::{self, Shared};

/// What the search window asks of the desk.
pub enum SearchEvent {
    Open(SearchTarget),
    Close,
}

/// Where Books keeps the recent items (`recentKey`).
pub const RECENTS_KEY: &str = "searchRecents";

pub struct SearchBar {
    session: Shared,
    searcher: Option<Searcher>,
    input: Entity<InputState>,
    items: Vec<SearchItem>,
    /// The highlighted suggestion.
    idx: usize,
    /// How many suggestions show; `usize::MAX` for all.
    limit: usize,
    /// `showMore`: the More Filters shown.
    show_more: bool,
    error: Option<String>,
    scroll: ScrollHandle,
    _subscription: Subscription,
}

impl EventEmitter<SearchEvent> for SearchBar {}

/// `allowedLimits`.
const LIMITS: [usize; 4] = [50, 100, 500, usize::MAX];

impl SearchBar {
    pub fn new(session: &Shared, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (searcher, error) = match Searcher::new(&session.borrow().fyo) {
            Ok(mut searcher) => {
                if let Some(filters) = &session.borrow().search_filters {
                    searcher.set_filters(filters);
                }
                if let Some(json) = session::config_value(RECENTS_KEY) {
                    searcher.set_recents_json(&json);
                }
                (Some(searcher), None)
            }
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
            session: session.clone(),
            searcher,
            input,
            items: Vec::new(),
            idx: 0,
            limit: LIMITS[0],
            show_more: false,
            error,
            scroll: ScrollHandle::new(),
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
        if let Some(searcher) = &self.searcher {
            self.session.borrow_mut().search_filters = Some(searcher.filters());
        }
        cx.notify();
    }

    fn shown(&self) -> usize {
        self.items.len().min(self.limit)
    }

    /// `select`: opens the item, keeps it among the recent ones, and closes.
    fn select(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(item) = self.items.get(i).cloned() else {
            cx.emit(SearchEvent::Close);
            return;
        };
        if let Some(searcher) = self.searcher.as_mut() {
            let now = jiff::Timestamp::now().as_millisecond();
            searcher.add_to_recent(&item, now);
            session::set_config_value(RECENTS_KEY, &searcher.recents_json());
        }
        cx.emit(SearchEvent::Open(item.target));
    }

    fn toggle_group(&mut self, group: SearchGroup, cx: &mut Context<Self>) {
        if let Some(searcher) = self.searcher.as_mut() {
            let on = searcher.group_on(group);
            searcher.set_group(group, !on);
        }
        self.idx = 0;
        self.refresh(cx);
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let count = self.shown();
        // Ctrl+1 to Ctrl+6 (⌘ on a Mac) toggle the groups.
        if k.modifiers.control || k.modifiers.platform {
            let group = k
                .key
                .parse::<usize>()
                .ok()
                .and_then(|n| n.checked_sub(1))
                .and_then(|i| SearchGroup::ALL.get(i).copied());
            if let Some(group) = group {
                self.toggle_group(group, cx);
                cx.stop_propagation();
            }
            return;
        }
        match k.key.as_str() {
            "down" => {
                self.idx = (self.idx + 1).min(count.saturating_sub(1));
                self.scroll.scroll_to_item(self.idx);
            }
            "up" => {
                self.idx = self.idx.saturating_sub(1);
                self.scroll.scroll_to_item(self.idx);
            }
            "enter" => self.select(self.idx, cx),
            "escape" => cx.emit(SearchEvent::Close),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// A filter button of the foot: bordered, rounded, its colours when on.
    fn filter_button(id: impl Into<ElementId>, label: String) -> Stateful<Div> {
        div()
            .id(id)
            .px_1()
            .py(px(2.))
            .rounded(books::RADIUS_LG)
            .border_1()
            .cursor_pointer()
            .whitespace_nowrap()
            .child(label)
    }

    fn render_more_filters(&self, cx: &mut Context<Self>) -> Option<Div> {
        let searcher = self.searcher.as_ref()?;
        if !self.show_more {
            return None;
        }
        let skips = [
            (
                "skip-tables",
                "Skip Child Tables",
                searcher.skip_tables(),
                true,
            ),
            (
                "skip-transactions",
                "Skip Transactions",
                searcher.skip_transactions(),
                false,
            ),
        ]
        .into_iter()
        .map(|(id, label, on, tables)| {
            Self::filter_button(id, t(label))
                .border_color(gray(200))
                .when(on, |d| d.bg(gray(200)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(searcher) = this.searcher.as_mut() {
                        if tables {
                            searcher.set_skip_tables(!on);
                        } else {
                            searcher.set_skip_transactions(!on);
                        }
                    }
                    this.idx = 0;
                    this.refresh(cx);
                }))
        });
        let schemas = searcher.schema_filters().into_iter().map(|filter| {
            let name = filter.schema_name.clone();
            let on = filter.on;
            Self::filter_button(SharedString::from(format!("schema-{name}")), filter.label)
                .border_color(books::color("blue", 100))
                .when(on, |d| d.bg(books::color("blue", 100)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(searcher) = this.searcher.as_mut() {
                        searcher.set_schema(&name, !on);
                    }
                    this.idx = 0;
                    this.refresh(cx);
                }))
        });
        Some(
            div()
                .v_flex()
                .mt(px(-4.))
                .child(div().h_flex().gap_1().text_color(gray(800)).children(skips))
                .child(
                    div()
                        .h_flex()
                        .flex_wrap()
                        .mt_1()
                        .gap_1()
                        .text_color(books::pick(
                            books::raw_color("blue", 500),
                            books::raw_color("blue", 100),
                        ))
                        .children(schemas),
                ),
        )
    }
}

impl Render for SearchBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let shown = self.shown();
        let hover_bg = books::pick(gray(50), books::raw_gray(875));
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
                    .hover(|d| d.bg(hover_bg))
                    .when(active, |d| {
                        d.bg(hover_bg)
                            .border_l_4()
                            .border_color(books::pick(gray(700), books::raw_gray(200)))
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
                                            .when(!active, |d| {
                                                d.text_color(books::pick(
                                                    gray(700),
                                                    books::raw_gray(400),
                                                ))
                                            })
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
                                    .text_color(books::raw_color(item.group.color(), 500))
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
            Self::filter_button(
                SharedString::from(format!("group-{}", group.label())),
                t(group.label()),
            )
            .border_color(books::pick(
                books::raw_color(color, 100),
                books::raw_color(color, 800),
            ))
            .when(on, |d| d.bg(bg).text_color(fg))
            .when(!on, |d| {
                d.text_color(books::pick(
                    books::raw_color(color, 600),
                    books::raw_color(color, 400),
                ))
            })
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_group(group, cx)))
        });
        let more_label = if self.show_more {
            "Less Filters"
        } else {
            "More Filters"
        };
        let limits: Vec<usize> = LIMITS
            .into_iter()
            .filter(|&c| c < total || c == usize::MAX)
            .collect();

        div()
            .v_flex()
            .w(books::W_FORM)
            .capture_key_down(cx.listener(Self::on_key_down))
            .child(
                div().p_1().child(
                    div()
                        .rounded(books::RADIUS_MD)
                        .bg(books::pick(gray(100), books::raw_gray(800)))
                        .child(
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
                    .track_scroll(&self.scroll)
                    .children(rows),
            )
            .child(hr())
            .child(
                div()
                    .v_flex()
                    .m_1()
                    .gap_2()
                    .text_12()
                    .child(
                        div()
                            .h_flex()
                            .justify_between()
                            .child(div().h_flex().gap_1().children(groups))
                            .child(
                                div()
                                    .id("more-filters")
                                    .py(px(2.))
                                    .cursor_pointer()
                                    .text_color(gray(700))
                                    .hover(|d| d.text_color(gray(900)))
                                    .child(t(more_label))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.show_more = !this.show_more;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .children(self.render_more_filters(cx))
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
                                    .child(format!("esc {}", t("Close")))
                                    .child(
                                        div()
                                            .id("search-help")
                                            .h_flex()
                                            .cursor_pointer()
                                            .group("search-help")
                                            .hover(|d| d.text_color(gray(800)))
                                            .child(
                                                books::icon("help-circle", px(16.), gray(500))
                                                    .mr_1()
                                                    .group_hover("search-help", |s| {
                                                        s.text_color(gray(800))
                                                    }),
                                            )
                                            .child(t("Help"))
                                            .on_click(|_, _, cx| {
                                                let path = latte_core::views::docs::docs_path_map(
                                                    "Search",
                                                )
                                                .unwrap_or_default();
                                                cx.open_url(&format!(
                                                    "{}{path}",
                                                    latte_core::views::docs::DOCS_URL
                                                ))
                                            }),
                                    ),
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
                                                .children(limits.into_iter().map(|limit| {
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
