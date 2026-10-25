//! The Classic UI: `ClassicPOS.vue` and `components/POS/Classic/` (`ItemsGrid.vue`,
//! `ItemsTable.vue`, `SelectedItemTable.vue`, `SelectedItemRow.vue`) @ v0.36.0, with the
//! pictures of the items, which the Modern UI shares.

use super::*;
use crate::pos_parts::{
    big_button, cell_text, floating_field, initials, labelled, ratio_cell, read_only_field,
    span_cell, table_header,
};
use crate::widgets::control_frame;
use gpui_kit::component::input::{Input, Textarea};
use latte_core::pos::Discount;

impl RowEditor {
    fn state(&self, field: RowField) -> &Entity<InputState> {
        match field {
            RowField::TransferQuantity => &self.transfer_quantity,
            RowField::Rate => &self.rate,
            RowField::DiscountAmount => &self.discount_amount,
            RowField::DiscountPercent => &self.discount_percent,
            _ => &self.quantity,
        }
    }
}

/// `SelectedItemTable`'s ratio: the toggler, Item, Quantity, Stock Unit, Rate, Amount,
/// and the trash icon.
const RATIO: [f32; 7] = [0.1, 0.9, 0.8, 0.8, 0.8, 0.8, 0.2];

impl PosPage {
    /// `ClassicPOS.vue`: the items at the left, the cart at the right.
    pub(super) fn render_classic(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        div()
            .h_flex()
            .flex_1()
            .min_h_0()
            .p_4()
            .gap_2()
            .bg(gray(25))
            .child(self.render_items(window, cx))
            .child(self.render_cart(cx))
    }

    /// The left of `ClassicPOS.vue`: the item search, `ItemsGrid` or `ItemsTable`, and
    /// `POSQuickActions` at the bottom.
    fn render_items(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let list = if self.table_view {
            self.render_items_table(cx)
        } else {
            self.render_items_grid(window, cx)
        };
        div()
            .relative()
            .flex_grow(5.)
            .flex_basis(px(0.))
            .min_w_0()
            .h_full()
            .bg(white())
            .border_1()
            .border_color(gray(200))
            .rounded(books::RADIUS_MD)
            .child(
                div()
                    .v_flex()
                    .size_full()
                    .p_4()
                    .child(self.render_search())
                    .child(list),
            )
            .child(
                div()
                    .absolute()
                    .bottom(px(11.))
                    .left(px(16.))
                    .child(self.render_quick_actions(cx)),
            )
    }

    /// An item's picture, or its initials on gray, with the quantity available at the
    /// corner (`ItemsGrid.vue`, `ModernPOSItemsGrid.vue`).
    pub(super) fn item_picture(&self, item: &PosItem) -> Div {
        let (badge_bg, badge_fg) = if item.available > 0.0 {
            (books::color("green", 100), books::color("green", 900))
        } else {
            (books::color("red", 100), books::color("red", 900))
        };
        let picture = match self.images.get(&item.name) {
            Some(image) => img(image.clone())
                .size_full()
                .rounded(books::RADIUS_LG)
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            None => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .rounded(books::RADIUS_LG)
                .bg(gray(100))
                .child(
                    div()
                        .text_size(px(28.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(gray(400))
                        .child(initials(&item.name)),
                )
                .into_any_element(),
        };
        div().relative().size_full().child(picture).child(
            div()
                .absolute()
                .top_1()
                .right_1()
                .size_6()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(badge_bg)
                .text_color(badge_fg)
                .child(number_text(item.available)),
        )
    }

    /// The rate under an item's name: the currency's symbol, then the rate.
    pub(super) fn item_rate(&self, item: &PosItem) -> String {
        let symbol = self.symbol().unwrap_or_default();
        item.rate
            .as_ref()
            .map(|r| format!("{symbol} {}", r.round(None)))
            .unwrap_or_default()
    }

    /// `ItemsGrid.vue`: a card per item, its picture, then its name and rate; one to four
    /// a row as the window widens.
    fn render_items_grid(&self, window: &Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let width = window.viewport_size().width;
        let columns = if width >= px(1280.) {
            4
        } else if width >= px(1024.) {
            3
        } else if width >= px(768.) {
            2
        } else {
            1
        };
        let cards = self.items.iter().enumerate().map(|(i, item)| {
            let rate = self.item_rate(item);
            let picture = self.item_picture(item).p_2();
            let item = item.clone();
            div()
                .id(("item", i))
                .v_flex()
                .p_1()
                .border_1()
                .border_color(gray(300))
                .text_12()
                .text_color(black())
                .text_center()
                .cursor_pointer()
                .child(
                    div()
                        .flex()
                        .justify_center()
                        .child(div().size(px(128.)).max_w_full().p_1().child(picture)),
                )
                .child(
                    div()
                        .text_14()
                        .font_weight(FontWeight::MEDIUM)
                        .child(item.name.clone()),
                )
                .child(div().text_14().font_weight(FontWeight::MEDIUM).child(rate))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.add(item.clone(), None, 1.0, window, cx)
                }))
        });
        div()
            .id("items")
            .flex_1()
            .min_h_0()
            .py_2()
            .overflow_y_scroll()
            .child(
                div()
                    .grid()
                    .grid_cols(columns)
                    .gap_2()
                    .pb_12()
                    .children(cards),
            )
    }

    /// A row of an items table (`ItemsTable.vue`, `ModernPOSItemsTable.vue`): Item, Rate,
    /// Qty and Unit.
    pub(super) fn item_row(
        &self,
        id: impl Into<ElementId>,
        item: &PosItem,
        ratios: [f32; 4],
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let rate = item
            .rate
            .as_ref()
            .map(|r| self.currency(r))
            .unwrap_or_default();
        let picked = item.clone();
        div()
            .id(id)
            .h_flex()
            .w_full()
            .h_12()
            .px_2()
            .border_b_1()
            .border_l_1()
            .border_r_1()
            .border_color(gray(200))
            .cursor_pointer()
            .hover(|d| d.bg(gray(25)))
            .child(ratio_cell(ratios[0]).child(cell_text(item.name.clone(), false)))
            .child(ratio_cell(ratios[1]).child(cell_text(rate, true)))
            .child(ratio_cell(ratios[2]).child(cell_text(number_text(item.available), true)))
            .child(
                ratio_cell(ratios[3])
                    .child(cell_text(item.unit.clone().unwrap_or_default(), false)),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.add(picked.clone(), None, 1.0, window, cx)
            }))
    }

    /// `ItemsTable.vue`.
    fn render_items_table(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let ratios = [1., 1., 1., 0.7];
        let columns = [
            (t("Item"), ratios[0], false),
            (t("Rate"), ratios[1], true),
            (t("Qty"), ratios[2], true),
            (t("Unit"), ratios[3], false),
        ];
        let rows: Vec<_> = self
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| self.item_row(("item-row", i), item, ratios, cx))
            .collect();
        div()
            .id("items")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(table_header(&columns).mt_4())
            .child(div().pb_12().children(rows))
    }

    /// The totals: Total Quantity, Add'l Discounts, Item Discounts and Grand Total.
    pub(super) fn render_totals(&self, gap: Pixels) -> Div {
        let sale = self.sale.as_ref();
        let (quantity, discounts, grand_total) = match sale {
            Some(sale) => {
                let s = self.session.borrow();
                (
                    pos::total_quantity(sale),
                    pos::item_discounts(&s.fyo, sale).ok(),
                    match sale.get("grandTotal") {
                        Some(DocValue::Money(m)) => Some(m.clone()),
                        _ => None,
                    },
                )
            }
            None => (0.0, None, None),
        };
        let symbol = self.symbol();
        let pair = |a: Div, b: Div| {
            div()
                .h_flex()
                .gap_2()
                .child(div().flex_1().min_w_0().child(a))
                .child(div().flex_1().min_w_0().child(b))
        };
        div()
            .v_flex()
            .child(pair(
                floating_field(t("Total Quantity"), number_text(quantity), None),
                floating_field(t("Add'l Discounts"), self.rounded(None), symbol.clone()),
            ))
            .child(
                pair(
                    floating_field(
                        t("Item Discounts"),
                        self.rounded(discounts.as_ref()),
                        symbol.clone(),
                    ),
                    floating_field(t("Grand Total"), self.rounded(grand_total.as_ref()), symbol),
                )
                .mt(gap),
            )
    }

    /// A big button's: Save, Cancel, Held, Return and Pay.
    pub(super) fn till_button(
        &self,
        which: &'static str,
        height: Pixels,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (label, fieldname, default, from_profile) = match which {
            "save" => (t("Save"), "saveButtonColour", GREEN_BUTTON, true),
            "cancel" => (t("Cancel"), "cancelButtonColour", RED_BUTTON, true),
            "held" => (t("held"), "heldButtonColour", RED_BUTTON, true),
            "return" => (t("Return"), "returnButtonColour", RED_BUTTON, true),
            // Classic's Pay reads `profile?.payButton`, which no profile has.
            "pay-classic" => (t("Pay"), "payButtonColour", GREEN_BUTTON, false),
            _ => (t("Pay"), "payButtonColour", GREEN_BUTTON, true),
        };
        let button = big_button(
            which,
            label,
            self.button_colour(fieldname, default, from_profile),
            height,
        );
        match which {
            "save" => button.on_click(cx.listener(|this, _, window, cx| {
                this.save(window, cx);
            })),
            "cancel" => {
                button.on_click(cx.listener(|this, _, window, cx| this.clear_cart(window, cx)))
            }
            "held" => button.on_click(cx.listener(|this, _, _, cx| this.show_held(false, cx))),
            "return" => button.on_click(cx.listener(|this, _, _, cx| this.open_return(cx))),
            _ => button.on_click(cx.listener(|this, _, window, cx| this.open_payment(window, cx))),
        }
    }

    /// The right of `ClassicPOS.vue`: the customer and `SelectedItemTable` above, the
    /// totals and the big buttons below.
    fn render_cart(&self, cx: &mut Context<Self>) -> Div {
        let columns = [
            (String::new(), RATIO[0], false),
            (t("Item"), RATIO[1], false),
            (t("Quantity"), RATIO[2], true),
            (t("Stock Unit"), RATIO[3], false),
            (t("Rate"), RATIO[4], true),
            (t("Amount"), RATIO[5], true),
            (String::new(), RATIO[6], false),
        ];
        let rows: Vec<_> = self
            .sale
            .as_ref()
            .map(|sale| sale.table("items"))
            .unwrap_or_default()
            .iter()
            .enumerate()
            .map(|(idx, row)| self.render_cart_row(idx, row, cx))
            .collect();

        let returns = self.accounting("enableInvoiceReturns");
        let tall = if returns { px(40.) } else { px(48.) };
        let gap = if returns { px(8.) } else { px(16.) };
        let pair = |a: Stateful<Div>, b: Stateful<Div>| div().h_flex().gap_2().child(a).child(b);
        let buttons = div()
            .v_flex()
            .flex_1()
            .min_w_0()
            .child(pair(
                self.till_button("save", tall, cx),
                self.till_button("cancel", tall, cx),
            ))
            .child(
                pair(
                    self.till_button("held", tall, cx),
                    if returns {
                        self.till_button("return", tall, cx)
                    } else {
                        self.till_button("pay-classic", tall, cx)
                    },
                )
                .mt(gap),
            )
            .when(returns, |d| {
                d.child(
                    div()
                        .h_flex()
                        .mt_2()
                        .child(self.till_button("pay", px(40.), cx)),
                )
            });
        let card = || {
            div()
                .bg(white())
                .border_1()
                .border_color(gray(200))
                .rounded(books::RADIUS_MD)
        };
        div()
            .v_flex()
            .flex_grow(7.)
            .flex_basis(px(0.))
            .min_w_0()
            .h_full()
            .gap_3()
            .child(
                card()
                    .v_flex()
                    .flex_1()
                    .min_h_0()
                    .p_4()
                    .child(div().flex_none().child(self.customer.clone()))
                    .child(table_header(&columns).mt_2())
                    .child(
                        div()
                            .id("cart")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .children(rows),
                    ),
            )
            .child(
                card().flex_none().p_3().child(
                    div()
                        .h_flex()
                        .gap_3()
                        .child(
                            div()
                                .v_flex()
                                .flex_1()
                                .min_w_0()
                                .justify_end()
                                .child(self.render_totals(px(16.))),
                        )
                        .child(buttons),
                ),
            )
    }

    /// `SelectedItemRow.vue`: the chevron for the row's details, the item, its quantity
    /// with the chevrons that change it, its unit, rate and amount, and the trash icon;
    /// then, when open, the details to edit.
    fn render_cart_row(&self, idx: usize, row: &Doc, cx: &mut Context<Self>) -> Stateful<Div> {
        let free = is_truthy(row.get("isFreeItem"));
        let expanded = self.expanded.contains(&idx);
        let small = |value: String, numeric: bool| cell_text(value, numeric).px_2().py_1();
        let chevron = |id: (&'static str, usize), name: &'static str, by: f64| {
            div()
                .id(id)
                .cursor_pointer()
                .child(books::icon(name, px(12.), gray(700)))
                .on_click(cx.listener(move |this, _, _, cx| this.change_quantity(idx, by, cx)))
        };
        let select = cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.selected_row = Some(idx);
            cx.notify();
        });
        let line = div()
            .h_flex()
            .w_full()
            .child(
                ratio_cell(RATIO[0]).child(
                    div()
                        .id(("expand", idx))
                        .cursor_pointer()
                        .child(books::icon(
                            if expanded {
                                "chevron-up"
                            } else {
                                "chevron-down"
                            },
                            px(16.),
                            gray(700),
                        ))
                        .on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.toggle_row(idx, window, cx)
                            }),
                        ),
                ),
            )
            .child(
                ratio_cell(RATIO[1]).child(
                    div()
                        .id(("row-item", idx))
                        .relative()
                        .w_full()
                        .pt_2()
                        .when(free, |d| {
                            d.child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .ml_2()
                                    .text_size(px(9.6))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(books::color("green", 800))
                                    .child(doc_text(row, "pricingRule")),
                            )
                        })
                        .child(small(doc_text(row, "item"), false))
                        .on_click(select),
                ),
            )
            .child(
                ratio_cell(RATIO[2])
                    .child(
                        div()
                            .id(("row-quantity", idx))
                            .flex_1()
                            .min_w_0()
                            .child(small(number_text(doc_number(row, "quantity")), true))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.selected_row = Some(idx);
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .v_flex()
                            .ml_1()
                            .child(chevron(("more", idx), "chevron-up", 1.0))
                            .child(chevron(("less", idx), "chevron-down", -1.0)),
                    ),
            )
            .child(
                ratio_cell(RATIO[3]).child(
                    div()
                        .ml_5()
                        .min_w_0()
                        .child(small(doc_text(row, "unit"), false)),
                ),
            )
            .child(ratio_cell(RATIO[4]).child(small(self.money(row.get("rate")), true)))
            .child(ratio_cell(RATIO[5]).child(small(self.money(row.get("amount")), true)))
            .child(
                ratio_cell(RATIO[6]).child(
                    div().px_4().child(
                        div()
                            .id(("remove", idx))
                            .cursor_pointer()
                            .child(books::icon("trash", px(16.), books::color("red", 500)))
                            .on_click(cx.listener(move |this, _, _, cx| this.remove_row(idx, cx))),
                    ),
                ),
            );
        div()
            .id(("cart-row", idx))
            .v_flex()
            .w_full()
            .px_2()
            .py_2()
            .border_1()
            .border_color(gray(200))
            .hover(|d| d.bg(gray(25)))
            .child(line)
            .when(expanded, |d| d.children(self.render_row_details(idx, row)))
    }

    /// The details of a row (`SelectedItemRow.vue`'s `isExapanded`), laid out on the
    /// table's seven columns as Books' grid places them.
    fn render_row_details(&self, idx: usize, row: &Doc) -> Vec<Div> {
        let Some(editor) = self.editors.get(&idx) else {
            return Vec::new();
        };
        let (uom, has_batch, has_serial, rate_read_only, amount_read_only, percent_read_only) = {
            let s = self.session.borrow();
            let item = doc_text(row, "item");
            let flag =
                |fieldname| is_truthy(s.fyo.get_value("Item", Some(&item), fieldname).as_ref());
            (
                latte_core::models::helpers::single_is_truthy(
                    &s.fyo,
                    "InventorySettings",
                    "enableUomConversions",
                ),
                flag("hasBatch"),
                flag("hasSerialNumber"),
                pos::rate_read_only(&s.fyo, row),
                pos::discount_read_only(&s.fyo, row, false),
                pos::discount_read_only(&s.fyo, row, true),
            )
        };
        let free = is_truthy(row.get("isFreeItem"));
        let discounting = self.discounting();
        let field =
            |label: String, state: &Entity<InputState>, read_only: bool, which: RowField| {
                if read_only {
                    return read_only_field(label, self.row_field_text(row, which), false)
                        .into_any_element();
                }
                let numeric = false;
                labelled(
                    label,
                    control_frame(false, false).child(
                        Input::new(state)
                            .appearance(false)
                            .h(px(32.))
                            .px_3()
                            .text_13()
                            .when(numeric, |i| i.text_right()),
                    ),
                )
                .into_any_element()
            };
        let cell = |ratios: &[f32], content: Option<AnyElement>| {
            span_cell(ratios).items_start().children(content)
        };
        let mut lines = Vec::new();

        // Transfer Quantity, Transfer Unit and Quantity, after the row's empty eighth cell.
        let transfer_quantity = uom.then(|| {
            field(
                t("Transfer Quantity"),
                &editor.transfer_quantity,
                free,
                RowField::TransferQuantity,
            )
        });
        let transfer_unit = uom.then(|| {
            if free {
                read_only_field(t("Transfer Unit"), doc_text(row, "transferUnit"), false)
                    .into_any_element()
            } else {
                labelled(t("Transfer Unit"), editor.transfer_unit.clone()).into_any_element()
            }
        });
        lines.push(
            div()
                .h_flex()
                .w_full()
                .items_start()
                .child(cell(&RATIO[..1], None))
                .child(cell(&RATIO[1..2], transfer_quantity).px_4().pt_6())
                .child(cell(&RATIO[2..4], transfer_unit).px_4().pt_6())
                .child(
                    cell(
                        &RATIO[4..6],
                        Some(field(
                            t("Quantity"),
                            &editor.quantity,
                            uom,
                            RowField::Quantity,
                        )),
                    )
                    .px_4()
                    .pt_6(),
                )
                .child(cell(&RATIO[6..], None)),
        );

        // Rate, Discount Amount and Discount Percent.
        lines.push(
            div()
                .h_flex()
                .w_full()
                .items_start()
                .child(cell(&RATIO[..1], None))
                .child(
                    cell(
                        &RATIO[1..2],
                        Some(field(
                            t("Rate"),
                            &editor.rate,
                            rate_read_only,
                            RowField::Rate,
                        )),
                    )
                    .px_4()
                    .pt_6(),
                )
                .child(
                    cell(
                        &RATIO[2..4],
                        discounting.then(|| {
                            field(
                                t("Discount Amount"),
                                &editor.discount_amount,
                                amount_read_only,
                                RowField::DiscountAmount,
                            )
                        }),
                    )
                    .px_6()
                    .pt_6(),
                )
                .child(
                    cell(
                        &RATIO[4..6],
                        discounting.then(|| {
                            field(
                                t("Discount Percent"),
                                &editor.discount_percent,
                                percent_read_only,
                                RowField::DiscountPercent,
                            )
                        }),
                    )
                    .px_4()
                    .pt_6(),
                )
                .child(cell(&RATIO[6..], None)),
        );

        // Batch, Qty in Batch and Serial Number.
        let serial = has_serial.then(|| {
            labelled(
                t("Serial Number"),
                control_frame(false, false).items_start().child(
                    Textarea::new(&editor.serial_number)
                        .appearance(false)
                        .h(px(3. * 19.5 + 16.))
                        .px_1()
                        .py_0()
                        .text_13(),
                ),
            )
            .into_any_element()
        });
        if has_batch {
            let batch = doc_text(row, "batch");
            let in_batch = if batch.is_empty() {
                0.0
            } else {
                let s = self.session.borrow();
                pos::batch_quantity(&s.fyo, &doc_text(row, "item"), &batch).unwrap_or(0.0)
            };
            lines.push(
                div()
                    .h_flex()
                    .w_full()
                    .items_start()
                    .child(
                        cell(
                            &RATIO[..2],
                            Some(labelled(t("Batch"), editor.batch.clone()).into_any_element()),
                        )
                        .pl_6()
                        .pr_4()
                        .pt_6(),
                    )
                    .child(
                        cell(
                            &RATIO[2..4],
                            Some(
                                read_only_field(t("Qty in Batch"), number_text(in_batch), true)
                                    .into_any_element(),
                            ),
                        )
                        .px_5()
                        .pt_6(),
                    )
                    .child(cell(&RATIO[4..], serial).px_6().pt_6()),
            );
        } else if serial.is_some() {
            lines.push(
                div()
                    .h_flex()
                    .w_full()
                    .items_start()
                    .child(cell(&RATIO[..3], serial).px_6().pt_6())
                    .child(cell(&RATIO[3..], None)),
            );
        }
        lines
    }

    /// The text a row's field shows.
    fn row_field_text(&self, row: &Doc, field: RowField) -> String {
        match field {
            RowField::Quantity => number_text(doc_number(row, "quantity")),
            RowField::TransferQuantity => {
                // `getDisplayTransferQuantity`: negative on a return.
                let quantity = doc_number(row, "transferQuantity");
                let return_row = self.sale.as_ref().is_some_and(pos::is_return_sale);
                number_text(if return_row && quantity != 0.0 {
                    -quantity.abs()
                } else {
                    quantity
                })
            }
            RowField::Rate => money_text(row.get("rate")),
            RowField::DiscountAmount => money_text(row.get("itemDiscountAmount")),
            RowField::DiscountPercent => number_text(doc_number(row, "itemDiscountPercent")),
            RowField::TransferUnit => doc_text(row, "transferUnit"),
            RowField::Batch => doc_text(row, "batch"),
            RowField::SerialNumber => self
                .serial_numbers
                .get(&doc_text(row, "item"))
                .cloned()
                .unwrap_or_default(),
        }
    }

    /// Makes the controls of the rows whose details are open, and shows each row's values
    /// in the controls not being typed in.
    pub(super) fn sync_editors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rows: Vec<Doc> = self
            .sale
            .as_ref()
            .map(|sale| sale.table("items").to_vec())
            .unwrap_or_default();
        self.expanded.retain(|idx| *idx < rows.len());
        self.editors.retain(|idx, _| *idx < rows.len());
        let ids: Vec<usize> = self.editors.keys().copied().collect();
        for idx in ids {
            let row = &rows[idx];
            let texts: Vec<(RowField, String)> = [
                RowField::Quantity,
                RowField::TransferQuantity,
                RowField::Rate,
                RowField::DiscountAmount,
                RowField::DiscountPercent,
                RowField::SerialNumber,
            ]
            .into_iter()
            .map(|field| (field, self.row_field_text(row, field)))
            .collect();
            let links = [
                (
                    RowField::TransferUnit,
                    self.row_field_text(row, RowField::TransferUnit),
                ),
                (RowField::Batch, self.row_field_text(row, RowField::Batch)),
            ];
            let Some(editor) = self.editors.get_mut(&idx) else {
                continue;
            };
            for (field, text) in texts {
                let synced = editor.synced.get(&field).cloned();
                let update = |current: SharedString, focused: bool| {
                    // Typed and not yet taken: left alone.
                    !focused
                        && current.as_ref() != text
                        && synced.as_deref().is_none_or(|s| s == current.as_ref())
                };
                let changed = if field == RowField::SerialNumber {
                    editor.serial_number.update(cx, |input, cx| {
                        let focused = input.focus_handle(cx).is_focused(window);
                        let go = update(input.value(), focused);
                        if go {
                            input.set_value(text.clone(), window, cx);
                        }
                        go
                    })
                } else {
                    let state = editor.state(field).clone();
                    state.update(cx, |input, cx| {
                        let focused = input.focus_handle(cx).is_focused(window);
                        let go = update(input.value(), focused);
                        if go {
                            input.set_value(text.clone(), window, cx);
                        }
                        go
                    })
                };
                if changed || synced.is_none() {
                    editor.synced.insert(field, text);
                }
            }
            for (field, text) in links {
                let link = if field == RowField::Batch {
                    &editor.batch
                } else {
                    &editor.transfer_unit
                };
                link.update(cx, |link, cx| {
                    if !link.is_focused(window, cx)
                        && link.value().map(|v| v.to_string()).unwrap_or_default() != text
                    {
                        link.set_value(Some(text.into()), window, cx);
                    }
                });
            }
        }
    }

    /// After a field is taken, or refused, it shows the row's value again.
    fn reset_row_field(
        &mut self,
        idx: usize,
        field: RowField,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self.sale.as_ref().and_then(|s| s.table("items").get(idx)) else {
            return;
        };
        let text = self.row_field_text(row, field);
        let Some(editor) = self.editors.get_mut(&idx) else {
            return;
        };
        if matches!(field, RowField::TransferUnit | RowField::Batch) {
            return;
        }
        if field == RowField::SerialNumber {
            editor
                .serial_number
                .update(cx, |input, cx| input.set_value(text.clone(), window, cx));
        } else {
            editor
                .state(field)
                .clone()
                .update(cx, |input, cx| input.set_value(text.clone(), window, cx));
        }
        editor.synced.insert(field, text);
    }

    /// Shows a row's details. Their controls are made here, not while drawing, so that
    /// they hear when they lose the focus.
    pub(super) fn expand(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self
            .sale
            .as_ref()
            .and_then(|s| s.table("items").get(idx))
            .cloned()
        else {
            return;
        };
        self.expanded.insert(idx);
        if !self.editors.contains_key(&idx) {
            let editor = self.make_editor(idx, &row, window, cx);
            self.editors.insert(idx, editor);
        }
    }

    fn make_editor(
        &mut self,
        idx: usize,
        row: &Doc,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> RowEditor {
        let (units, batches) = {
            let s = self.session.borrow();
            (
                names(&s.fyo, "UOM", QueryFilter::new()),
                pos::batches(&s.fyo, &doc_text(row, "item")).unwrap_or_default(),
            )
        };
        let mut subscriptions = Vec::new();
        let mut text_input = |field: RowField, window: &mut Window, cx: &mut Context<Self>| {
            let state = input(self.row_field_text(row, field), window, cx);
            subscriptions.push(cx.subscribe_in(
                &state,
                window,
                move |this, state, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                        let text = state.read(cx).value().trim().to_owned();
                        this.commit_row_field(idx, field, text, window, cx);
                    }
                },
            ));
            state
        };
        let quantity = text_input(RowField::Quantity, window, cx);
        let transfer_quantity = text_input(RowField::TransferQuantity, window, cx);
        let rate = text_input(RowField::Rate, window, cx);
        let discount_amount = text_input(RowField::DiscountAmount, window, cx);
        let discount_percent = text_input(RowField::DiscountPercent, window, cx);

        let mut link = |field: RowField,
                        options: Vec<String>,
                        label: String,
                        window: &mut Window,
                        cx: &mut Context<Self>| {
            let current = self.row_field_text(row, field);
            let link = cx.new(|cx| {
                let mut link = PosLink::new(options, label, window, cx);
                link.set_value(Some(current.into()), window, cx);
                link
            });
            subscriptions.push(cx.subscribe_in(
                &link,
                window,
                move |this, _, event: &PosLinkEvent, window, cx| match event {
                    PosLinkEvent::Picked(value) => {
                        this.commit_row_field(idx, field, value.to_string(), window, cx)
                    }
                    PosLinkEvent::Opened(value) => {
                        let schema = if field == RowField::Batch {
                            "Batch"
                        } else {
                            "UOM"
                        };
                        this.open_form(schema, value, cx)
                    }
                    PosLinkEvent::Entered(_) => {}
                },
            ));
            link
        };
        let transfer_unit = link(
            RowField::TransferUnit,
            units,
            t("Transfer Unit"),
            window,
            cx,
        );
        let batch = link(RowField::Batch, batches, t("Batch"), window, cx);

        let serial = self.row_field_text(row, RowField::SerialNumber);
        let serial_number = cx.new(|cx| TextareaState::new(window, cx).default_value(serial));
        subscriptions.push(cx.subscribe_in(
            &serial_number,
            window,
            move |this, state, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Blur) {
                    let text = state.read(cx).value().to_string();
                    this.commit_row_field(idx, RowField::SerialNumber, text, window, cx);
                }
            },
        ));
        RowEditor {
            quantity,
            transfer_quantity,
            rate,
            discount_amount,
            discount_percent,
            transfer_unit,
            batch,
            serial_number,
            synced: HashMap::new(),
            _subscriptions: subscriptions,
        }
    }

    /// A Classic row's field, changed (`setQuantity`, `setRate`, `setItemDiscount`,
    /// `setBatch`, `setSerialNumber`, and the transfer unit and quantity).
    fn commit_row_field(
        &mut self,
        idx: usize,
        field: RowField,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_row_value(idx, field, text, cx);
        self.reset_row_field(idx, field, window, cx);
    }

    fn commit_row_value(
        &mut self,
        idx: usize,
        field: RowField,
        text: String,
        cx: &mut Context<Self>,
    ) {
        self.selected_row = Some(idx);
        let Some(row) = self.sale.as_ref().and_then(|s| s.table("items").get(idx)) else {
            return;
        };
        if text == self.row_field_text(row, field) {
            return;
        }
        let item = doc_text(row, "item");
        let quantity = doc_number(row, "quantity");
        let number = |text: &str| {
            text.parse::<f64>()
                .map_err(|_| latte_core::Error::Validation(format!("{text} is not a number")))
        };
        match field {
            RowField::SerialNumber => {
                // `setSerialNumber` keeps what is typed, and says when the count is off.
                if text.is_empty() {
                    return;
                }
                self.serial_numbers.insert(item.clone(), text.clone());
                self.error = pos::validate_serial_number_count(&text, quantity.abs(), &item)
                    .err()
                    .map(|e| e.to_string());
                cx.notify();
            }
            RowField::Quantity => {
                self.change_sale(cx, |fyo, sale, qty_map| {
                    let quantity = number(&text)?;
                    pos::set_row_quantity(fyo, sale, idx, quantity, qty_map)
                });
            }
            RowField::TransferQuantity => {
                self.change_sale(cx, |fyo, sale, _| {
                    let quantity = number(&text)?;
                    sale.set_child(
                        fyo,
                        "items",
                        idx,
                        "transferQuantity",
                        DocValue::Number(quantity),
                    )?;
                    Ok(())
                });
            }
            RowField::Rate => {
                self.change_sale(cx, |fyo, sale, _| {
                    let rate = fyo
                        .pesa()
                        .parse(if text.is_empty() { "0" } else { &text })?;
                    pos::set_row_rate(fyo, sale, idx, rate)
                });
            }
            RowField::DiscountAmount => {
                self.change_sale(cx, |fyo, sale, _| {
                    let amount = fyo
                        .pesa()
                        .parse(if text.is_empty() { "0" } else { &text })?;
                    pos::set_row_discount(fyo, sale, idx, Discount::Amount(amount))
                });
            }
            RowField::DiscountPercent => {
                self.change_sale(cx, |fyo, sale, _| {
                    let percent = if text.is_empty() { 0.0 } else { number(&text)? };
                    pos::set_row_discount(fyo, sale, idx, Discount::Percent(percent))
                });
            }
            RowField::TransferUnit => {
                self.change_sale(cx, |fyo, sale, _| {
                    let value = if text.is_empty() {
                        DocValue::Null
                    } else {
                        DocValue::Text(text.clone())
                    };
                    sale.set_child(fyo, "items", idx, "transferUnit", value)?;
                    Ok(())
                });
            }
            RowField::Batch => {
                self.change_sale(cx, |fyo, sale, _| pos::set_row_batch(fyo, sale, idx, &text));
            }
        }
    }
}
