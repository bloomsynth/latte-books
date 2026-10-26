//! The Modern UI: `ModernPOS.vue` and `components/POS/Modern/`
//! (`ModernPOSItemsGrid.vue`, `ModernPOSItemsTable.vue`, `ModernPOSSelectedItemTable.vue`,
//! `ModernPOSSelectedItemRow.vue`) @ v0.36.0. A row's numbers open the keypad
//! (`KeyboardModal.vue`, in pos_modals.rs).

use super::*;
use crate::pos_parts::{big_button, cell_text, labelled, ratio_cell, read_only_field};
use crate::widgets::control_frame;
use gpui_kit::component::input::Textarea;

/// `ModernPOSSelectedItemTable`'s ratio: the toggler, Item, Quantity, Rate, Amount, and
/// the trash icon.
const RATIO: [f32; 6] = [0.1, 0.8, 0.4, 0.8, 0.8, 0.3];

impl PosPage {
    /// `ModernPOS.vue`: the cart at the left, the items at the right.
    pub(super) fn render_modern(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        div()
            .h_flex()
            .flex_1()
            .min_h_0()
            .p_4()
            .gap_3()
            .bg(gray(25))
            .child(self.render_modern_cart(window, cx))
            .child(self.render_modern_items(window, cx))
    }

    fn render_modern_cart(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let header = div()
            .h_flex()
            .w_full()
            .mt_2()
            .px_2()
            .border_1()
            .border_color(gray(200))
            .rounded_t(books::RADIUS)
            .text_color(gray(600))
            .text_14()
            .children(
                [
                    (String::new(), false),
                    (t("Item"), false),
                    (t("Quantity"), true),
                    (t("Rate"), true),
                    (t("Amount"), true),
                    (String::new(), false),
                ]
                .into_iter()
                .zip(RATIO)
                .map(|((label, numeric), ratio)| {
                    ratio_cell(ratio)
                        .when(numeric, |d| d.justify_end())
                        .child(div().m_2().whitespace_nowrap().child(label))
                }),
            );
        let rows: Vec<_> = self
            .sale
            .as_ref()
            .map(|sale| sale.table("items"))
            .unwrap_or_default()
            .iter()
            .enumerate()
            .map(|(idx, row)| self.render_modern_row(idx, row, window, cx))
            .collect();

        let returns = self.accounting("enableInvoiceReturns");
        let tall = if returns { px(40.) } else { px(48.) };
        let column = |top: Stateful<Div>, bottom: Stateful<Div>| {
            div()
                .v_flex()
                .flex_1()
                .min_w_0()
                .child(div().h_flex().mt_2().child(top))
                .child(div().h_flex().mt_2().child(bottom))
        };
        let held = big_button(
            "held",
            t("Held"),
            self.button_colour("heldButtonColour", RED_BUTTON, true),
            tall,
        )
        .on_click(cx.listener(|this, _, _, cx| this.show_held(false, cx)));
        let buttons = div()
            .v_flex()
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .gap_2()
                    .child(column(self.till_button("save", tall, cx), held))
                    .child(column(
                        self.till_button("cancel", tall, cx),
                        if returns {
                            self.till_button("return", px(40.), cx)
                        } else {
                            self.till_button("pay", px(40.), cx)
                        },
                    )),
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
            .flex_grow(3.)
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
                    .child(header)
                    .child(
                        div()
                            .id("cart")
                            .flex_1()
                            .min_h_0()
                            .overflow_scroll()
                            .children(rows),
                    ),
            )
            .child(
                card()
                    .flex_none()
                    .p_2()
                    .child(self.render_totals(px(8.)))
                    .child(buttons),
            )
    }

    /// A number of a row's details: shown, and opening the keypad when clicked.
    fn keypad_field(
        &self,
        idx: usize,
        label: String,
        text: String,
        field: KeypadField,
        read_only: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // `handleOpenKeyboard`: not for a free item.
        let free = self
            .sale
            .as_ref()
            .and_then(|sale| sale.table("items").get(idx))
            .is_some_and(|row| is_truthy(row.get("isFreeItem")));
        labelled(
            label,
            control_frame(read_only, false)
                .id(SharedString::from(format!(
                    "keypad-{idx}-{}",
                    field.fieldname()
                )))
                .px_3()
                .cursor_pointer()
                .child(div().truncate().child(text))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if !free {
                        this.open_keyboard(idx, field, window, cx);
                    }
                })),
        )
        .into_any_element()
    }

    /// `ModernPOSSelectedItemRow.vue`.
    fn render_modern_row(
        &self,
        idx: usize,
        row: &Doc,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let free = is_truthy(row.get("isFreeItem"));
        let expanded = self.expanded.contains(&idx);
        let small = |value: String, numeric: bool| cell_text(value, numeric).px_2().py_1();
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
                            black(),
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
                        .cursor_pointer()
                        .when(free, |d| {
                            d.mt_2().child(
                                div()
                                    .absolute()
                                    .top(px(-8.))
                                    .ml_2()
                                    .text_size(px(9.6))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(books::color("green", 800))
                                    .child(doc_text(row, "pricingRule")),
                            )
                        })
                        .child(small(doc_text(row, "item"), false))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.selected_row = Some(idx);
                            this.toggle_row(idx, window, cx)
                        })),
                ),
            )
            .child(
                ratio_cell(RATIO[2]).child(small(number_text(doc_number(row, "quantity")), true)),
            )
            .child(ratio_cell(RATIO[3]).child(small(self.money(row.get("rate")), true)))
            .child(ratio_cell(RATIO[4]).child(small(self.money(row.get("amount")), true)))
            .child(
                ratio_cell(RATIO[5]).justify_center().child(
                    div()
                        .id(("remove", idx))
                        .cursor_pointer()
                        .child(books::icon("trash", px(16.), books::color("red", 500)))
                        .on_click(cx.listener(move |this, _, _, cx| this.remove_row(idx, cx))),
                ),
            );
        let details = expanded.then(|| self.render_modern_details(idx, row, window, cx));
        div()
            .id(("cart-row", idx))
            .v_flex()
            .w_full()
            .p_2()
            .border_1()
            .border_color(gray(200))
            .hover(|d| d.bg(gray(25)))
            .child(line)
            .children(details)
    }

    /// The details of a Modern row: four columns, 27% of the window wide, from the
    /// table's second column.
    fn render_modern_details(
        &self,
        idx: usize,
        row: &Doc,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let (uom, has_batch, has_serial) = {
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
            )
        };
        let free = is_truthy(row.get("isFreeItem"));
        let discounting = self.discounting();
        let half = |content: Option<AnyElement>| div().flex_1().min_w_0().px_4().children(content);
        let pair = |a: Div, b: Div| div().h_flex().w_full().items_start().child(a).child(b);

        let quantity = self.keypad_field(
            idx,
            t("Quantity"),
            number_text(doc_number(row, "quantity")),
            KeypadField::Quantity,
            free,
            cx,
        );
        let transfer_unit = uom.then(|| {
            read_only_field(t("Transfer Unit"), doc_text(row, "transferUnit"), false)
                .into_any_element()
        });
        let transfer_quantity = uom.then(|| {
            self.keypad_field(
                idx,
                t("Transfer Quantity"),
                number_text(doc_number(row, "transferQuantity")),
                KeypadField::TransferQuantity,
                free,
                cx,
            )
        });
        let rate = self.keypad_field(
            idx,
            t("Rate"),
            self.money(row.get("rate")),
            KeypadField::Rate,
            free,
            cx,
        );
        let percent = doc_number(row, "itemDiscountPercent");
        let amount_set =
            matches!(row.get("itemDiscountAmount"), Some(DocValue::Money(m)) if !m.is_zero());
        let discount_amount = discounting.then(|| {
            self.keypad_field(
                idx,
                t("Discount Amount"),
                self.money(row.get("itemDiscountAmount")),
                KeypadField::ItemDiscountAmount,
                percent > 0.0 || free,
                cx,
            )
        });
        let discount_percent = discounting.then(|| {
            self.keypad_field(
                idx,
                t("Discount Percent"),
                number_text(percent),
                KeypadField::ItemDiscountPercent,
                amount_set || free,
                cx,
            )
        });

        let editor = self.editors.get(&idx);
        let mut details = div()
            .v_flex()
            .w(window.viewport_size().width * 0.27)
            .my_3()
            .rounded(books::RADIUS_MD)
            .child(pair(half(Some(quantity)), half(transfer_unit)))
            .child(pair(
                half(transfer_quantity).pt_6(),
                half(Some(rate)).pt_6(),
            ))
            .child(pair(
                half(discount_amount).mt_5(),
                half(discount_percent).mt_5(),
            ));
        if has_batch && let Some(editor) = editor {
            let batch = doc_text(row, "batch");
            let in_batch = if batch.is_empty() {
                0.0
            } else {
                let s = self.session.borrow();
                pos::batch_quantity(&s.fyo, &doc_text(row, "item"), &batch).unwrap_or(0.0)
            };
            details = details.child(pair(
                half(Some(
                    labelled(t("Batch"), editor.batch.clone()).into_any_element(),
                ))
                .pt_6(),
                half(Some(
                    read_only_field(t("Qty in Batch"), number_text(in_batch), true)
                        .into_any_element(),
                ))
                .pt_6(),
            ));
        }
        if has_serial && let Some(editor) = editor {
            details = details.child(
                div().w_full().px_4().pt_6().child(labelled(
                    t("Serial Number"),
                    control_frame(false, false).items_start().child(
                        Textarea::new(&editor.serial_number)
                            .appearance(false)
                            .h(px(3. * 19.5 + 16.))
                            .px_1()
                            .py_0()
                            .text_13(),
                    ),
                )),
            );
        }
        div()
            .h_flex()
            .w_full()
            .items_start()
            .child(ratio_cell(RATIO[0]))
            .child(ratio_cell(RATIO[1..].iter().sum()).child(details))
    }

    /// The right of `ModernPOS.vue`: the item search, `ModernPOSItemsTable` or
    /// `ModernPOSItemsGrid`, and `POSQuickActions`.
    fn render_modern_items(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let list = if self.table_view {
            self.render_modern_table(window, cx).into_any_element()
        } else {
            self.render_modern_grid(window, cx).into_any_element()
        };
        div()
            .relative()
            .v_flex()
            .flex_grow(6.)
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
                    .left(px(28.))
                    .child(self.render_quick_actions(cx)),
            )
    }

    /// `ModernPOSItemsGrid.vue`: up to six cards a row.
    fn render_modern_grid(&self, window: &Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let width = window.viewport_size().width;
        let columns = if width >= px(1024.) {
            6
        } else if width >= px(768.) {
            4
        } else if width >= px(640.) {
            2
        } else {
            1
        };
        let tall = if width >= px(1024.) {
            px(112.)
        } else {
            px(128.)
        };
        let cards = self.items.iter().enumerate().map(|(i, item)| {
            let rate = self.item_rate(item);
            let picture = self.item_picture(item);
            let item = item.clone();
            div()
                .id(("item", i))
                .v_flex()
                .pb_3()
                .border_1()
                .border_color(gray(300))
                .text_12()
                .text_color(black())
                .text_center()
                .cursor_pointer()
                .child(div().w_full().h(tall).p_1().child(picture))
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
            .my_3()
            .px_4()
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

    /// `ModernPOSItemsTable.vue`: the items in two tables side by side, half in each.
    fn render_modern_table(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let ratios = [1., 1., 0.6, 0.7];
        let header = || {
            div()
                .h_flex()
                .w_full()
                .mt_2()
                .px_2()
                .border_1()
                .border_color(gray(200))
                .rounded_t(books::RADIUS_MD)
                .text_color(gray(600))
                .text_14()
                .children(
                    [
                        (t("Item"), false),
                        (t("Rate"), true),
                        (t("Qty"), true),
                        (t("Unit"), false),
                    ]
                    .into_iter()
                    .zip(ratios)
                    .map(|((label, numeric), ratio)| {
                        ratio_cell(ratio)
                            .when(numeric, |d| d.justify_end())
                            .child(div().p_2().truncate().child(label))
                    }),
                )
        };
        let half = self.items.len().div_ceil(2);
        let height = window.viewport_size().height;
        let column = |id: &'static str, items: &[PosItem], h: Pixels, cx: &mut Context<Self>| {
            let rows: Vec<_> = items
                .iter()
                .enumerate()
                .map(|(i, item)| self.item_row((id, i), item, ratios, cx))
                .collect();
            div()
                .id(id)
                .w_1_2()
                .h(h)
                .overflow_y_scroll()
                .child(header())
                .children(rows)
        };
        div()
            .h_flex()
            .items_start()
            .gap_2()
            .child(column(
                "first-items",
                &self.items[..half],
                height * 0.81,
                cx,
            ))
            .child(column(
                "second-items",
                &self.items[half..],
                (height * 0.8 - px(320.)).max(px(48.)),
                cx,
            ))
    }
}
