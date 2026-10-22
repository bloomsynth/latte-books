//! UI tests for the grid: real key and mouse events in a headless GPUI window.
//! They check state and focus, not pixels.

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Entity, Focusable, InputEvent as _, MouseButton,
    MouseDownEvent, MouseUpEvent, Point, TestAppContext, Window, WindowBounds, WindowOptions,
    point, px, size,
};
use rust_decimal::Decimal;

use crate::InvoiceGrid;
use crate::link::LinkInput;

fn open(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<InvoiceGrid>) {
    cx.update(crate::init);
    let opened = cx.update(|cx| {
        let bounds = Bounds {
            origin: Point::default(),
            size: size(px(1100.), px(820.)),
        };
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| InvoiceGrid::new(window, cx))
        })
        .expect("open test window")
    });
    // GPUI reports focus changes (and so InputEvent::Focus/Blur) only in an active window.
    cx.update_window(opened.0, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    opened
}

/// Runs `f` in the window, then lets subscriptions and deferred work settle.
fn act<R>(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    f: impl FnOnce(&mut Window, &mut App) -> R,
) -> R {
    let result = cx
        .update_window(handle, |_, window, cx| {
            let result = f(window, cx);
            window.render_frame(cx);
            result
        })
        .unwrap();
    cx.run_until_parked();
    result
}

fn press(cx: &mut TestAppContext, handle: AnyWindowHandle, key: &str) {
    act(cx, handle, |window, cx| window.press(key, cx));
}

fn type_text(cx: &mut TestAppContext, handle: AnyWindowHandle, text: &str) {
    act(cx, handle, |window, cx| window.input(text, cx));
}

/// The focused cell as (row index, column).
fn focused(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    grid: &Entity<InvoiceGrid>,
) -> Option<(usize, &'static str)> {
    act(cx, handle, |window, cx| {
        grid.read(cx).rows.iter().enumerate().find_map(|(i, row)| {
            if row.item.read(cx).is_focused(window, cx) {
                Some((i, "item"))
            } else if row.tax.read(cx).is_focused(window, cx) {
                Some((i, "tax"))
            } else if row.quantity.focus_handle(cx).is_focused(window) {
                Some((i, "quantity"))
            } else if row.rate.focus_handle(cx).is_focused(window) {
                Some((i, "rate"))
            } else {
                None
            }
        })
    })
}

struct RowState {
    item: Option<String>,
    tax: Option<String>,
    quantity: String,
    rate: String,
    dropdown: Option<String>,
}

fn row(cx: &mut TestAppContext, grid: &Entity<InvoiceGrid>, index: usize) -> RowState {
    cx.read(|cx| {
        let row = &grid.read(cx).rows[index];
        RowState {
            item: row.item.read(cx).value().map(|v| v.to_string()),
            tax: row.tax.read(cx).value().map(|v| v.to_string()),
            quantity: row.quantity.read(cx).value().to_string(),
            rate: row.rate.read(cx).value().to_string(),
            dropdown: row.item.read(cx).dropdown().map(|v| v.to_string()),
        }
    })
}

fn focus_item(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    grid: &Entity<InvoiceGrid>,
    index: usize,
) {
    act(cx, handle, |window, cx| {
        let item = grid.read(cx).rows[index].item.clone();
        LinkInput::focus(&item, window, cx);
    });
}

fn totals(
    cx: &mut TestAppContext,
    grid: &Entity<InvoiceGrid>,
) -> (Decimal, Vec<(String, Decimal)>, Decimal) {
    cx.read(|cx| {
        let (net, taxes, grand) = grid.read(cx).totals(cx);
        let taxes = taxes
            .into_iter()
            .map(|(name, amount)| (name.to_string(), amount));
        (net, taxes.collect(), grand)
    })
}

/// Clicks the input in a row's cell, as a person would.
fn click_cell(window: &mut Window, column: &'static str, row_id: usize, cx: &mut App) {
    window.within((column, row_id)).click("input", cx);
}

fn dec(text: &str) -> Decimal {
    text.parse().unwrap()
}

#[gpui_kit::test]
fn typing_an_item_and_pressing_enter_fills_rate_and_tax(cx: &mut TestAppContext) {
    let (handle, grid) = open(cx);
    focus_item(cx, handle, &grid, 0);
    assert_eq!(focused(cx, handle, &grid), Some((0, "item")));
    assert!(
        row(cx, &grid, 0).dropdown.is_some(),
        "the list opens on focus"
    );

    type_text(cx, handle, "dry");
    assert_eq!(row(cx, &grid, 0).dropdown.as_deref(), Some("Dry-Cleaning"));

    press(cx, handle, "enter");
    let state = row(cx, &grid, 0);
    assert_eq!(state.item.as_deref(), Some("Dry-Cleaning"));
    assert_eq!(state.rate, "69.00");
    assert_eq!(state.tax.as_deref(), Some("GST-18"));
    assert_eq!(state.quantity, "1");
    assert_eq!(state.dropdown, None, "Enter closes the list");
    assert_eq!(
        focused(cx, handle, &grid),
        Some((0, "item")),
        "focus stays put"
    );
}

#[gpui_kit::test]
fn tab_and_shift_tab_move_through_the_cells_in_order(cx: &mut TestAppContext) {
    let (handle, grid) = open(cx);
    focus_item(cx, handle, &grid, 0);
    for expected in ["tax", "quantity", "rate"] {
        press(cx, handle, "tab");
        assert_eq!(focused(cx, handle, &grid), Some((0, expected)));
    }
    for expected in ["quantity", "tax", "item"] {
        press(cx, handle, "shift-tab");
        assert_eq!(focused(cx, handle, &grid), Some((0, expected)));
    }
}

#[gpui_kit::test]
fn arrow_keys_move_the_highlight(cx: &mut TestAppContext) {
    let (handle, grid) = open(cx);
    focus_item(cx, handle, &grid, 0);
    type_text(cx, handle, "office");
    assert_eq!(row(cx, &grid, 0).dropdown.as_deref(), Some("Office Rent"));
    press(cx, handle, "down");
    assert_eq!(
        row(cx, &grid, 0).dropdown.as_deref(),
        Some("Office Cleaning")
    );
    press(cx, handle, "down");
    // Last entry: "Create" for text that matches no item exactly.
    assert_eq!(row(cx, &grid, 0).dropdown.as_deref(), Some("office"));
    press(cx, handle, "up");
    press(cx, handle, "enter");
    let state = row(cx, &grid, 0);
    assert_eq!(state.item.as_deref(), Some("Office Cleaning"));
    assert_eq!(state.rate, "7500.00");
}

#[gpui_kit::test]
fn escape_closes_and_tab_away_restores_unmatched_text(cx: &mut TestAppContext) {
    let (handle, grid) = open(cx);
    focus_item(cx, handle, &grid, 0);
    type_text(cx, handle, "zzz");
    press(cx, handle, "escape");
    assert_eq!(row(cx, &grid, 0).dropdown, None);
    assert_eq!(focused(cx, handle, &grid), Some((0, "item")));

    press(cx, handle, "tab");
    assert_eq!(focused(cx, handle, &grid), Some((0, "tax")));
    let text = cx.read(|cx| grid.read(cx).rows[0].item.read(cx).text(cx).to_string());
    assert_eq!(text, "", "text that names no item is dropped on blur");
    assert_eq!(row(cx, &grid, 0).item, None);
}

#[gpui_kit::test]
fn cells_fill_their_columns(cx: &mut TestAppContext) {
    let (handle, _) = open(cx);
    // 1,100 px window: each of the five full columns gets about 190 px, and the input fills it.
    for column in ["item", "tax", "quantity", "rate"] {
        let width = act(cx, handle, |window, _| {
            window.within((column, 0usize)).find("input").bounds().size.width
        });
        assert!(width > px(150.), "{column} cell is {width:?} wide");
    }
}

#[gpui_kit::test]
fn clicking_a_cell_focuses_it_and_clicking_item_reopens_the_list(cx: &mut TestAppContext) {
    let (handle, grid) = open(cx);
    for column in ["quantity", "rate", "tax", "item"] {
        act(cx, handle, |window, cx| click_cell(window, column, 0, cx));
        assert_eq!(focused(cx, handle, &grid), Some((0, column)));
    }
    assert!(row(cx, &grid, 0).dropdown.is_some(), "clicking opens the list");

    // The input keeps focus after Esc, so only the click itself can reopen the list.
    press(cx, handle, "escape");
    assert_eq!(row(cx, &grid, 0).dropdown, None);
    act(cx, handle, |window, cx| click_cell(window, "item", 0, cx));
    assert!(row(cx, &grid, 0).dropdown.is_some(), "clicking again reopens it");

    // And after picking an item.
    type_text(cx, handle, "dry");
    press(cx, handle, "enter");
    assert_eq!(row(cx, &grid, 0).dropdown, None);
    act(cx, handle, |window, cx| click_cell(window, "item", 0, cx));
    assert!(row(cx, &grid, 0).dropdown.is_some());
}

/// Presses and releases the mouse at a point, such as empty space no element ID covers.
fn click_at(window: &mut Window, x: f32, y: f32, cx: &mut App) {
    let position = point(px(x), px(y));
    let down = MouseDownEvent {
        button: MouseButton::Left,
        position,
        click_count: 1,
        ..Default::default()
    };
    let up = MouseUpEvent {
        button: MouseButton::Left,
        position,
        click_count: 1,
        ..Default::default()
    };
    window.dispatch_event(down.to_platform_input(), cx);
    window.dispatch_event(up.to_platform_input(), cx);
}

#[gpui_kit::test]
fn clicking_outside_closes_the_list_and_clicking_an_entry_picks_it(cx: &mut TestAppContext) {
    let (handle, grid) = open(cx);
    act(cx, handle, |window, cx| click_cell(window, "item", 0, cx));
    assert!(row(cx, &grid, 0).dropdown.is_some());

    // Empty space below the totals, in the 1,100 × 820 window.
    act(cx, handle, |window, cx| click_at(window, 300., 780., cx));
    assert_eq!(row(cx, &grid, 0).dropdown, None, "a click elsewhere closes it");

    act(cx, handle, |window, cx| click_cell(window, "item", 0, cx));
    type_text(cx, handle, "dry");
    act(cx, handle, |window, cx| {
        window.within("link-dropdown").click(0usize, cx)
    });
    let state = row(cx, &grid, 0);
    assert_eq!(state.item.as_deref(), Some("Dry-Cleaning"));
    assert_eq!(state.rate, "69.00");
    assert_eq!(state.dropdown, None);
}

#[gpui_kit::test]
fn create_adds_a_new_item(cx: &mut TestAppContext) {
    let (handle, grid) = open(cx);
    focus_item(cx, handle, &grid, 0);
    type_text(cx, handle, "Widget");
    assert_eq!(row(cx, &grid, 0).dropdown.as_deref(), Some("Widget"));
    press(cx, handle, "enter");
    let state = row(cx, &grid, 0);
    assert_eq!(state.item.as_deref(), Some("Widget"));
    assert_eq!(state.rate, "0.00");
    assert!(cx.read(|cx| grid.read(cx).items.iter().any(|item| item.name == "Widget")));
}

#[gpui_kit::test]
fn add_row_focuses_the_new_item_cell_and_remove_deletes_the_row(cx: &mut TestAppContext) {
    let (handle, grid) = open(cx);
    act(cx, handle, |window, cx| window.click("add-row", cx));
    assert_eq!(cx.read(|cx| grid.read(cx).rows.len()), 2);
    assert_eq!(focused(cx, handle, &grid), Some((1, "item")));
    assert!(row(cx, &grid, 1).dropdown.is_some());
    assert_eq!(
        row(cx, &grid, 1).quantity,
        "1",
        "new rows start with quantity 1"
    );

    act(cx, handle, |window, cx| {
        window.click(("remove", 0usize), cx)
    });
    let ids: Vec<usize> = cx.read(|cx| grid.read(cx).rows.iter().map(|row| row.id).collect());
    assert_eq!(ids, vec![1]);
}

#[gpui_kit::test]
fn totals_follow_edits(cx: &mut TestAppContext) {
    let (handle, grid) = open(cx);
    focus_item(cx, handle, &grid, 0);
    type_text(cx, handle, "dry");
    press(cx, handle, "enter");
    // Item → Tax → Quantity, then replace the quantity.
    press(cx, handle, "tab");
    press(cx, handle, "tab");
    assert_eq!(focused(cx, handle, &grid), Some((0, "quantity")));
    press(cx, handle, "ctrl-a");
    type_text(cx, handle, "3");
    assert_eq!(row(cx, &grid, 0).quantity, "3");

    act(cx, handle, |window, cx| window.click("add-row", cx));
    type_text(cx, handle, "electricity");
    press(cx, handle, "enter");

    let (net, taxes, grand) = totals(cx, &grid);
    // 3 × 69 + 1 × 6000; GST-18 on 207 = 37.26; GST-0 on 6000.
    assert_eq!(net, dec("6207"));
    assert_eq!(
        taxes,
        vec![("GST-0".into(), dec("0")), ("GST-18".into(), dec("37.26"))]
    );
    assert_eq!(grand, dec("6244.26"));
}

#[gpui_kit::test]
fn tab_runs_through_every_row_of_a_long_grid(cx: &mut TestAppContext) {
    const ROWS: usize = 20;
    let (handle, grid) = open(cx);
    act(cx, handle, |window, cx| {
        grid.update(cx, |grid, cx| {
            for _ in 1..ROWS {
                grid.add_row(window, cx);
            }
        })
    });
    focus_item(cx, handle, &grid, 0);
    let columns = ["item", "tax", "quantity", "rate"];
    // Rows past the first dozen are out of view (and not rendered) until Tab reaches them.
    for index in 0..ROWS {
        for (c, column) in columns.into_iter().enumerate() {
            if index > 0 || c > 0 {
                press(cx, handle, "tab");
            }
            assert_eq!(focused(cx, handle, &grid), Some((index, column)));
        }
    }
    for index in (0..ROWS).rev() {
        for (c, column) in columns.into_iter().enumerate().rev() {
            if index == ROWS - 1 && c == 3 {
                continue;
            }
            press(cx, handle, "shift-tab");
            assert_eq!(focused(cx, handle, &grid), Some((index, column)));
        }
    }
}
