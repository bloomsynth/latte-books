//! Phase 0 spike 3: an editable invoice item grid, like the Items table on a Frappe Books
//! Sales Invoice.
//!
//! Follows Books @ v0.36.0: columns from `InvoiceItem.tableFields`
//! (`item, tax, quantity, rate, amount`), `src/components/Controls/Table.vue` and `TableRow.vue`
//! for adding and removing rows, and `amount = rate × quantity` from
//! `models/baseModels/InvoiceItem/InvoiceItem.ts`. Items come from Books' sample data.
//!
//! What to try: Tab/Shift+Tab through cells, type in Item to filter, ↑/↓/Enter to pick,
//! "Add Row", remove a row, and watch the totals update. "Add 50 rows" checks
//! that the grid stays responsive with many rows.
//!
//! `--bench [rows]` adds rows (default 500), then edits a rate 20 times, and prints how long each change
//! takes to reach the screen.

mod link;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::str::FromStr;
use std::time::Instant;

use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::*;
use rust_decimal::Decimal;
use serde::Deserialize;

use link::{LinkEvent, LinkInput};

#[derive(Deserialize)]
struct Item {
    name: SharedString,
    rate: Decimal,
    tax: SharedString,
}

fn load_items() -> Vec<Item> {
    serde_json::from_str(include_str!("../../../vendor/books/dummy/items.json"))
        .expect("vendored dummy/items.json is valid")
}

/// Rate of a sample-data tax template, read from its name ("GST-18" → 18).
fn tax_rate(template: &str) -> Decimal {
    template
        .rsplit('-')
        .next()
        .and_then(|rate| Decimal::from_str(rate).ok())
        .unwrap_or_default()
}

fn parse_decimal(text: &str) -> Decimal {
    Decimal::from_str(text.trim().replace(',', "").as_str()).unwrap_or_default()
}

fn money(amount: Decimal) -> String {
    let text = format!("{:.2}", amount.round_dp(2));
    let (whole, cents) = text.split_once('.').unwrap_or((&text, "00"));
    let (sign, digits) = whole.strip_prefix('-').map_or(("", whole), |d| ("-", d));
    let mut grouped = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    format!("{sign}₹ {grouped}.{cents}")
}

struct Row {
    id: usize,
    item: Entity<LinkInput>,
    tax: Entity<LinkInput>,
    quantity: Entity<InputState>,
    rate: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl Row {
    fn amount(&self, cx: &App) -> Decimal {
        parse_decimal(&self.quantity.read(cx).value()) * parse_decimal(&self.rate.read(cx).value())
    }
}

struct InvoiceGrid {
    items: Vec<Item>,
    taxes: Vec<SharedString>,
    rows: Vec<Row>,
    next_id: usize,
    scroll: UniformListScrollHandle,
}

impl InvoiceGrid {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let items = load_items();
        let mut taxes: Vec<SharedString> = items.iter().map(|item| item.tax.clone()).collect();
        taxes.sort();
        taxes.dedup();
        let mut grid = Self {
            items,
            taxes,
            rows: Vec::new(),
            next_id: 0,
            scroll: UniformListScrollHandle::new(),
        };
        grid.add_row(window, cx);
        grid
    }

    fn item_names(&self) -> Vec<SharedString> {
        self.items.iter().map(|item| item.name.clone()).collect()
    }

    fn add_row(&mut self, window: &mut Window, cx: &mut Context<Self>) -> usize {
        let id = self.next_id;
        self.next_id += 1;

        let names = self.item_names();
        let item = cx.new(|cx| LinkInput::new(names, "Item", true, window, cx));
        let taxes = self.taxes.clone();
        let tax = cx.new(|cx| LinkInput::new(taxes, "Tax", false, window, cx));
        // New rows start with quantity 1, as in Books.
        let quantity = cx.new(|cx| InputState::new(window, cx).default_value("1"));
        let rate = cx.new(|cx| InputState::new(window, cx).placeholder("0.00"));

        let subscriptions = vec![
            cx.subscribe_in(
                &item,
                window,
                move |this, _, event: &LinkEvent, window, cx| {
                    this.on_item_chosen(id, event, window, cx)
                },
            ),
            cx.subscribe(&tax, |_, _, _: &LinkEvent, cx| cx.notify()),
            cx.subscribe(&quantity, |_, _, _: &InputEvent, cx| cx.notify()),
            cx.subscribe(&rate, |_, _, _: &InputEvent, cx| cx.notify()),
        ];

        self.rows.push(Row {
            id,
            item,
            tax,
            quantity,
            rate,
            _subscriptions: subscriptions,
        });
        cx.notify();
        id
    }

    /// Picking an item fills in its rate and tax template, as Books does.
    fn on_item_chosen(
        &mut self,
        row_id: usize,
        event: &LinkEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = match event {
            LinkEvent::Selected(name) => name.clone(),
            LinkEvent::Create(name) => {
                // Books opens a quick-create form here; the spike adds a zero-rate item.
                self.items.push(Item {
                    name: name.clone(),
                    rate: Decimal::ZERO,
                    tax: self.taxes.first().cloned().unwrap_or_default(),
                });
                let names = self.item_names();
                for row in &self.rows {
                    row.item
                        .update(cx, |link, cx| link.set_options(names.clone(), cx));
                }
                name.clone()
            }
        };
        let Some(item) = self.items.iter().find(|item| item.name == name) else {
            return;
        };
        let (rate, tax) = (item.rate, item.tax.clone());
        if let Some(row) = self.rows.iter().find(|row| row.id == row_id) {
            row.rate.update(cx, |input, cx| {
                input.set_value(format!("{:.2}", rate), window, cx)
            });
            row.tax
                .update(cx, |link, cx| link.set_value(tax, window, cx));
        }
        cx.notify();
    }

    /// Focuses the first (Item) or last (Rate) cell of a row and scrolls it into view.
    /// Returns false when there is no such row.
    fn focus_row(
        &mut self,
        index: usize,
        last_cell: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(row) = self.rows.get(index) else {
            return false;
        };
        if last_cell {
            let rate = row.rate.clone();
            rate.update(cx, |input, cx| input.focus(window, cx));
        } else {
            let item = row.item.clone();
            LinkInput::focus(&item, window, cx);
        }
        self.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
        cx.notify();
        true
    }

    fn remove_row(&mut self, row_id: usize, cx: &mut Context<Self>) {
        self.rows.retain(|row| row.id != row_id);
        cx.notify();
    }

    fn totals(&self, cx: &App) -> (Decimal, BTreeMap<SharedString, Decimal>, Decimal) {
        let mut net = Decimal::ZERO;
        let mut taxes = BTreeMap::new();
        for row in &self.rows {
            let amount = row.amount(cx);
            net += amount;
            if let Some(template) = row.tax.read(cx).value() {
                *taxes.entry(template.clone()).or_insert(Decimal::ZERO) +=
                    amount * tax_rate(template) / Decimal::ONE_HUNDRED;
            }
        }
        let grand = net + taxes.values().copied().sum::<Decimal>();
        (net, taxes, grand)
    }
}

impl InvoiceGrid {
    fn render_row(&self, index: usize, cx: &mut Context<Self>) -> Div {
        let border = cx.theme().border;
        let row = &self.rows[index];
        let id = row.id;
        // A uniform_list row is only as wide as its content unless told otherwise, and every
        // cell's flex basis is 0, so without w_full the cells shrink to their padding.
        div()
            .h_flex()
            .w_full()
            .py_1()
            .border_b_1()
            .border_color(border)
            .child(
                cell(0.3)
                    .h_flex()
                    .gap_1()
                    .child(format!("{}", index + 1))
                    .child(
                        Button::new(("remove", id))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Close)
                            .on_click(cx.listener(move |this, _, _, cx| this.remove_row(id, cx))),
                    ),
            )
            // Tab between rows is handled here: only rows in view are rendered, so the
            // next row's cells may not be in the window's tab order yet.
            .child(
                cell(1.)
                    .id(("item", id))
                    .key_context("FirstCell")
                    .child(row.item.clone())
                    .on_action(cx.listener(move |this, _: &PrevRow, window, cx| {
                        if index == 0 || !this.focus_row(index - 1, true, window, cx) {
                            cx.propagate();
                        }
                    })),
            )
            .child(cell(1.).id(("tax", id)).child(row.tax.clone()))
            .child(
                cell(1.)
                    .id(("quantity", id))
                    .child(Input::new(&row.quantity).id("input").small()),
            )
            .child(
                cell(1.)
                    .id(("rate", id))
                    .key_context("LastCell")
                    .child(Input::new(&row.rate).id("input").small())
                    .on_action(cx.listener(move |this, _: &NextRow, window, cx| {
                        if !this.focus_row(index + 1, false, window, cx) {
                            cx.propagate();
                        }
                    })),
            )
            .child(cell(1.).h_flex().justify_end().child(money(row.amount(cx))))
    }
}

/// Column widths: `#` is narrow, the rest share the width, as Books' `ratio` does.
fn cell(flex: f32) -> Div {
    div().flex_grow(flex).flex_basis(px(0.)).px_2()
}

impl Render for InvoiceGrid {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();

        let header = div()
            .h_flex()
            .py_2()
            .border_b_1()
            .border_color(theme.border)
            .text_color(theme.muted_foreground)
            .child(cell(0.3).child("#"))
            .child(cell(1.).child("Item"))
            .child(cell(1.).child("Tax"))
            .child(cell(1.).child("Quantity"))
            .child(cell(1.).child("Rate"))
            .child(cell(1.).h_flex().justify_end().child("Amount"));

        let (net, taxes, grand) = self.totals(cx);
        let total_line = |label: SharedString, amount: Decimal| {
            div()
                .h_flex()
                .justify_between()
                .gap_8()
                .child(label)
                .child(money(amount))
        };

        div()
            .v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(div().text_xl().child("Sales Invoice — Items"))
            .child(
                div()
                    .v_flex()
                    .border_1()
                    .border_color(theme.border)
                    .rounded_md()
                    .child(header)
                    .child(
                        // Only the rows in view are built each frame; their state lives in
                        // entities, so rows scrolled out of view keep their values.
                        uniform_list(
                            "rows",
                            self.rows.len(),
                            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                range.map(|index| this.render_row(index, cx)).collect()
                            }),
                        )
                        .track_scroll(&self.scroll)
                        .h(px(440.)),
                    )
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .p_1()
                            .child(
                                Button::new("add-row")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Plus)
                                    .label("Add Row")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        let id = this.add_row(window, cx);
                                        this.scroll.scroll_to_item(
                                            this.rows.len() - 1,
                                            ScrollStrategy::Nearest,
                                        );
                                        if let Some(row) = this.rows.iter().find(|r| r.id == id) {
                                            let item = row.item.clone();
                                            LinkInput::focus(&item, window, cx);
                                        }
                                    })),
                            )
                            .child(
                                Button::new("add-50")
                                    .ghost()
                                    .small()
                                    .label("Add 50 rows")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        for _ in 0..50 {
                                            this.add_row(window, cx);
                                        }
                                    })),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .h_flex()
                                    .justify_end()
                                    .px_2()
                                    .text_color(theme.muted_foreground)
                                    .child(format!("{} rows", self.rows.len())),
                            ),
                    ),
            )
            .child(
                div()
                    .v_flex()
                    .self_end()
                    .w(px(320.))
                    .gap_1()
                    .child(total_line("Net Total".into(), net))
                    .children(
                        taxes
                            .into_iter()
                            .map(|(template, amount)| total_line(template, amount)),
                    )
                    .child(
                        div()
                            .border_t_1()
                            .border_color(theme.border)
                            .pt_1()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(total_line("Grand Total".into(), grand)),
                    ),
            )
    }
}

actions!(invoice_grid, [NextRow, PrevRow]);

/// Tab from a row's last cell and Shift+Tab from its first go to the neighbouring row.
/// These bindings are more specific than Root's, which handles every other Tab.
fn init(cx: &mut App) {
    gpui_kit::init(cx);
    cx.bind_keys([
        KeyBinding::new("tab", NextRow, Some("LastCell")),
        KeyBinding::new("shift-tab", PrevRow, Some("FirstCell")),
    ]);
}

const BENCH_EDITS: usize = 20;

fn ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.
}

/// Times one change until the frame showing it is presented, so it includes the wait for vsync.
fn bench(grid: Entity<InvoiceGrid>, rows: usize, window: &mut Window, cx: &mut App) {
    let start = Instant::now();
    grid.update(cx, |grid, cx| {
        for _ in 0..rows {
            grid.add_row(window, cx);
        }
    });
    window.on_next_frame(move |window, cx| {
        println!("add {rows} rows -> frame: {:>7.1} ms", ms(start));
        bench_edits(grid, Vec::new(), window, cx);
    });
}

fn bench_edits(grid: Entity<InvoiceGrid>, mut times: Vec<f64>, window: &mut Window, cx: &mut App) {
    if times.len() == BENCH_EDITS {
        times.sort_by(f64::total_cmp);
        let rows = grid.read(cx).rows.len();
        println!(
            "edit with {rows} rows -> frame: median {:.1} ms, max {:.1} ms",
            times[times.len() / 2],
            times[times.len() - 1]
        );
        cx.quit();
        return;
    }
    let start = Instant::now();
    let value = format!("{}.00", times.len() + 1);
    grid.update(cx, |grid, cx| {
        let rate = grid.rows[0].rate.clone();
        rate.update(cx, |input, cx| input.set_value(value, window, cx));
        cx.notify();
    });
    window.on_next_frame(move |window, cx| {
        times.push(ms(start));
        bench_edits(grid, times, window, cx);
    });
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bench_rows = args.iter().position(|arg| arg == "--bench").map(|i| {
        args.get(i + 1)
            .and_then(|rows| rows.parse().ok())
            .unwrap_or(500)
    });
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            init(cx);
            let bounds = Bounds::centered(None, size(px(1100.), px(820.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Latte Books - Invoice Grid Spike".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            let (handle, grid) = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| InvoiceGrid::new(window, cx))
            })
            .expect("failed to open window");
            if let Some(rows) = bench_rows {
                handle
                    .update(cx, |_, window, _| {
                        window.on_next_frame(move |window, cx| bench(grid, rows, window, cx))
                    })
                    .expect("window is open");
            }
            cx.activate(true);
        });
}
