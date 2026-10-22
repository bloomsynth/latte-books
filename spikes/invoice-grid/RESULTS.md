# Spike 3: editable invoice item grid

**Question:** can GPUI (through gpui-kit) do the Items table of a Books Sales Invoice: tab
between cells, add and remove rows, item autocomplete, and totals that update live?
**Plan reference:** PLAN.md, Phase 0, spike 3.

## What was built

`src/main.rs` and `src/link.rs` (gpui-kit `=0.8.0`, GPUI `gpui-pre 0.3.9`):

- Columns from Books v0.36.0 `InvoiceItem.tableFields`: Item, Tax, Quantity, Rate, Amount.
  New rows start with quantity 1; Amount = Rate × Quantity.
- Item and Tax are a custom **Link field** (`LinkInput`). It works like Books' `AutoComplete.vue`:
  - type into the cell to filter the list
  - ↑/↓ move the highlight, Enter picks, Esc closes
  - "Create" is offered when nothing matches exactly
  - text that names no record is dropped when the cell loses focus

  gpui-kit's `Combobox` is a button that opens a search box, so it doesn't fit.
- Picking an item fills in its rate and tax template, from Books' sample `items.json`.
- Add Row appends a row and focuses its Item cell. × removes a row.
- Net Total, one line per tax template, and Grand Total update on every edit.
- Rows are **virtualized** with GPUI's `uniform_list`: only the rows in view are built each
  frame. Each row's state lives in entities, so rows that scroll out of view keep their values.
- Tab from a row's last cell goes to the next row's first cell, and Shift+Tab does the
  reverse, scrolling that row into view. Virtualization needs this: GPUI's Tab order only
  includes rendered elements. It is done with key bindings scoped to the edge cells, which
  take precedence over gpui-kit Root's Tab binding.

## Correctness: UI tests

`src/tests.rs` has 11 tests built on gpui-kit's test harness (`#[gpui_kit::test]`). They send real
key and mouse events to a headless window, then check focus and state. They don't check pixels.

| Test | Covers |
|---|---|
| `typing_an_item_and_pressing_enter_fills_rate_and_tax` | List opens on focus; "dry" + Enter → Dry-Cleaning, rate 69.00, GST-18 |
| `tab_and_shift_tab_move_through_the_cells_in_order` | Item → Tax → Quantity → Rate and back |
| `arrow_keys_move_the_highlight` | ↓/↑ through matches and the Create entry |
| `escape_closes_and_tab_away_restores_unmatched_text` | Esc closes the list; unmatched text is dropped on blur |
| `create_adds_a_new_item` | Create entry adds an item with rate 0.00 |
| `add_row_focuses_the_new_item_cell_and_remove_deletes_the_row` | Mouse clicks on Add Row and × |
| `totals_follow_edits` | Quantity edit plus a second row: 6,207.00 net, 37.26 GST-18, 6,244.26 grand |
| `tab_runs_through_every_row_of_a_long_grid` | Tab and Shift+Tab across all cells of 20 rows, beyond the ~12 in view |
| `cells_fill_their_columns` | Each input in a virtualized row is wider than 150 px in a 1,100 px window |
| `clicking_a_cell_focuses_it_and_clicking_item_reopens_the_list` | Mouse clicks on every cell; clicking Item opens the list again after Esc and after picking |
| `clicking_outside_closes_the_list_and_clicking_an_entry_picks_it` | A click on empty space closes the list; a mouse click on an entry picks it |

All 11 pass. `cargo test -p invoice-grid` takes about 50 s in a debug build; the long-grid test
takes most of that.

## Performance

`invoice-grid.exe --bench N` adds N rows. Then it changes a rate 20 times, timing each change
until the frame showing it is presented. Each time therefore includes the wait for vsync.

Release build, Ryzen 7 3700X, RTX 2060, 3840×2160 at **120 Hz**, so one frame is 8.3 ms:

| Rows | Edit → frame, **without** virtualization | Edit → frame, virtualized | Adding the rows → frame |
|---|---|---|---|
| 1 | 8.3 ms | 8.3 ms | – |
| 11 | 8.2 ms | 8.3 ms | 12 ms |
| 26 | 12.9 ms | – | – |
| 51 | 23.7 ms | 8.3 ms | 13 ms |
| 101 | 46.8 ms | 8.3 ms | 16 ms |
| 201 | 97.8 ms | – | – |
| 501 | 261.6 ms | 8.3 ms | 46 ms |
| 2,001 | – | 8.2 ms (max 11.6) | 428 ms |

The first version rendered every row on every change, at about 0.5 ms per row. Past about 25
rows it missed frames, and at 500 rows it took a quarter of a second per keystroke. With
`uniform_list`, every edit lands on the next frame regardless of row count.

Binary size: 23.1 MB (debug info stripped).

## Things learned

- **Use `uniform_list` (or `list`) for every table that can grow.** Render cost is per element
  built, so anything that grows must be virtualized. This applies to the list views and
  reports in Phase 2 as well.
- **Key bindings run before key listeners.** gpui-kit's Root binds Tab, so a
  `capture_key_down` handler for "tab" never runs. Custom Tab behaviour needs a binding in a
  more specific key context, which calls `cx.propagate()` to fall back to Root.
- **Focus events only fire in an active window.** In tests, call `window.activate_window()`
  first. Separately, a just-created input can miss its first focus change in a test, because
  its focus listener is registered once the current update ends. `LinkInput::focus` therefore
  opens the list itself. Whether this also happens in the real app is unverified.
- **Lock file.** With gpui-kit's `test-support` feature, Cargo also locks gpui-kit's optional
  `gpui-fast` backend. That backend pins older patch versions of `cocoa` and
  `unicode-properties` than our lock had. `spikes/Cargo.lock` now starts from gpui-kit's own
  published lock. Latte Books proper will need the same.

## Verdict

**Pass.** Everything the spike asked for works and is covered by tests that send real input
events. With virtualization, editing stays within one frame at 2,000 rows.

## Still open

- **Tried by hand (2026-10-22): three bugs found and fixed.** The tests had focused cells from code
  and never clicked one, so all three slipped through:
  - Every cell was about 18 px wide. A `uniform_list` row is only as wide as its content, and
    the cells' flex basis is 0, so they shrank to their padding. Fixed with `w_full()` on the row.
  - Clicking the Item cell didn't reopen the list after Esc or after picking an item, because
    the input already had focus. It now also opens on click, as in Books.
  - Clicking empty space didn't close the list. In a browser that click takes focus off the
    input; GPUI leaves focus where it is. The list now closes on any click outside it
    (`on_mouse_down_out`). Unmatched typed text is still only cleared when the input loses focus.

  Each now has a test with real mouse clicks, and each test fails with its fix removed. Apart from
  these, the hand test went well.
- **Fuzzy matching.** Books ranks matches fuzzily; the spike uses a substring match.
- **Quick-create form.** Books opens a quick-create form for "Create"; the spike adds a
  zero-rate item.
- **Column widths and resizing**, and the Books row-detail ("edit full row") form.
- **Low-end laptop.** The same benchmark should be run there, as for spike 1.
