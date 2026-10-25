//! The POS's modals @ v0.36.0: `OpenPOSShiftModal.vue`, `ClosePOSShiftModal.vue`,
//! `PaymentModal.vue`, `SavedInvoiceModal.vue`, `BatchSelectionModal.vue`,
//! `AlertModal.vue`, `PriceListModal.vue`, `CouponCodeModal.vue`,
//! `LoyaltyProgramModal.vue`, `ItemEnquiryModal.vue`, `ReturnSalesInvoiceModal.vue` and
//! `KeyboardModal.vue`.

use super::*;
use crate::pos_parts::{
    big_button, cell_text, coins_icon, keypad_key, labelled, modal, modal_card, modal_title,
    ratio_cell, rule, shown_field, table_header,
};
use crate::widgets::{control_frame, field_label};
use gpui_kit::component::input::{Input, Textarea};

/// `padding: 1.35rem` around a big button's label.
const PADDED: Pixels = px(43.);

impl PosPage {
    /// The modals open over the till.
    pub(super) fn render_modals(&self, window: &Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut modals = Vec::new();
        match &self.mode {
            Mode::Opening(_) => modals.push(self.render_shift_modal(true, window, cx)),
            Mode::Closing(_) => modals.push(self.render_shift_modal(false, window, cx)),
            Mode::Selling => {}
        }
        if let Some(panel) = &self.payment
            && let Some(modal) = self.render_payment_modal(panel, window, cx)
        {
            modals.push(modal);
        }
        if let Some(panel) = &self.held {
            modals.push(self.render_held_modal(panel, window, cx));
        }
        modals.extend(self.render_batch_modal(window, cx));
        if let Some(overlay) = &self.overlay {
            modals.push(self.render_overlay(overlay, window, cx));
        }
        modals
    }

    /// `OpenPOSShiftModal.vue` and `ClosePOSShiftModal.vue`. The amounts of payment
    /// methods other than Cash are read only, as Books' tables are.
    fn render_shift_modal(
        &self,
        opening: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (Mode::Opening(panel) | Mode::Closing(panel)) = &self.mode else {
            return div().into_any_element();
        };
        let counted = self.counted_cash(panel, cx);
        let counted_text = counted
            .as_ref()
            .map_or_else(String::new, |m| self.currency(m));
        let zero = self.session.borrow().fyo.pesa().zero();
        let text_cell = |text: String, numeric: bool| {
            div()
                .w_full()
                .min_w_0()
                .px_2()
                .truncate()
                .when(numeric, |d| d.text_right())
                .child(text)
                .into_any_element()
        };
        let input_cell = |state: &Entity<InputState>| {
            Input::new(state)
                .appearance(false)
                .w_full()
                .px_2()
                .text_13()
                .text_right()
                .into_any_element()
        };

        let cash_rows = panel
            .cash
            .iter()
            .map(|row| vec![input_cell(&row.denomination), input_cell(&row.count)])
            .collect();
        let add_row = div()
            .id("add-denomination")
            .h_flex()
            .w_full()
            .h_12()
            .px_2()
            .border_t_1()
            .border_color(gray(200))
            .text_color(gray(500))
            .cursor_pointer()
            .child(
                ratio_cell(0.3)
                    .pl_1()
                    .child(books::icon("plus", px(16.), gray(500))),
            )
            .child(
                div()
                    .flex_grow(2.)
                    .flex_basis(px(0.))
                    .px_2()
                    .child(t("Add Row")),
            )
            .on_click(cx.listener(|this, _, window, cx| this.add_cash_row(window, cx)))
            .into_any_element();
        let cash_table = shift_table(
            &[(t("Denomination"), true), (t("Count"), true)],
            cash_rows,
            Some(add_row),
        );

        let amounts_table = if opening {
            let mut rows = vec![vec![
                text_cell("Cash".to_owned(), false),
                text_cell(counted_text, true),
            ]];
            rows.extend(panel.others.iter().map(|method| {
                vec![
                    text_cell(method.clone(), false),
                    text_cell(self.currency(&zero), true),
                ]
            }));
            shift_table(
                &[(t("Payment Method"), false), (t("Amount"), true)],
                rows,
                None,
            )
        } else {
            let money = |m: Option<&Money>| m.map(|m| self.currency(m)).unwrap_or_default();
            let difference =
                |closing: Option<Money>, expected: Option<&Money>| match (closing, expected) {
                    (Some(c), Some(e)) => c.sub(e).ok(),
                    _ => None,
                };
            let cash_expected = panel.expected.get("Cash");
            let mut rows = vec![vec![
                text_cell("Cash".to_owned(), false),
                text_cell(money(panel.opened.get("Cash")), true),
                text_cell(counted_text, true),
                text_cell(money(cash_expected), true),
                text_cell(
                    money(difference(counted.clone(), cash_expected).as_ref()),
                    true,
                ),
            ]];
            rows.extend(panel.others.iter().map(|method| {
                let expected = panel.expected.get(method);
                vec![
                    text_cell(method.clone(), false),
                    text_cell(money(panel.opened.get(method)), true),
                    text_cell(self.currency(&zero), true),
                    text_cell(money(expected), true),
                    text_cell(
                        money(difference(Some(zero.clone()), expected).as_ref()),
                        true,
                    ),
                ]
            }));
            shift_table(
                &[
                    (t("Payment Method"), false),
                    (t("Opening Amount"), true),
                    (t("Closing Amount"), true),
                    (t("Expected Amount"), true),
                    (t("Difference Amount"), true),
                ],
                rows,
                None,
            )
        };

        let buttons = div()
            .h_flex()
            .gap_4()
            .mt_4()
            .child(
                big_button(
                    "shift-back",
                    if opening { t("Back") } else { t("Cancel") },
                    books::color("red", 500),
                    px(40.),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    if opening {
                        go_back(window, cx)
                    } else {
                        this.cancel_closing(window, cx)
                    }
                })),
            )
            .child(
                big_button(
                    "shift-submit",
                    t("Submit"),
                    books::color("green", 500),
                    px(40.),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    if opening {
                        this.open_shift(window, cx)
                    } else {
                        this.close_shift(window, cx)
                    }
                })),
            );
        let heading = |text: String| div().text_14().font_weight(FontWeight::MEDIUM).child(text);
        let body = if opening {
            div()
                .h_flex()
                .items_start()
                .gap_6()
                .child(
                    div()
                        .v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(heading(t("Cash In Denominations")))
                        .child(cash_table.mt_4()),
                )
                .child(
                    div()
                        .v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(heading(t("Opening Amount")))
                        .child(amounts_table.mt_4())
                        .child(buttons),
                )
        } else {
            div()
                .v_flex()
                .child(heading(t("Closing Cash")).mt_4().mb_2())
                .child(cash_table)
                .child(heading(t("Closing Amounts")).mt_6().mb_2())
                .child(amounts_table)
                .child(buttons)
        };
        let viewport = window.viewport_size();
        modal(
            modal_card()
                .id("shift-modal")
                .w(viewport.width * 0.5)
                .max_h(viewport.height)
                .overflow_y_scroll()
                .p_4()
                .child(
                    div()
                        .pb_4()
                        .text_18()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_center()
                        .child(if opening {
                            t("Open POS Shift")
                        } else {
                            t("Close POS Shift")
                        }),
                )
                .child(body),
            false,
            window,
        )
        .into_any_element()
    }

    /// `PaymentModal.vue`: the amount paid, a button per payment method, the transfer's
    /// reference and clearance date, the change or balance, the sale's totals, and Submit,
    /// Cancel, Pay and Pay & Print.
    fn render_payment_modal(
        &self,
        panel: &PaymentPanel,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let sale = self.sale.as_ref()?;
        let (methods, zero, taxes, discounts) = {
            let s = self.session.borrow();
            (
                pos::payment_methods(&s.fyo).unwrap_or_default(),
                s.fyo.pesa().zero(),
                pos::total_taxed_amount(&s.fyo, sale).ok(),
                pos::item_discounts(&s.fyo, sale).ok(),
            )
        };
        // `isPaymentMethodIsCash`: by the method's name, so with none picked the transfer's
        // fields show.
        let is_cash = panel.method.as_deref() == Some("Cash");
        let grand_total = match sale.get("grandTotal") {
            Some(DocValue::Money(m)) => m.clone(),
            _ => zero.clone(),
        };
        let paid = self
            .session
            .borrow()
            .fyo
            .pesa()
            .parse(&value(&panel.amount, cx))
            .unwrap_or_else(|_| zero.clone());
        let paid_zero = paid.is_zero();
        let show_change =
            !pos::is_return_sale(sale) && !paid_zero && paid.gt(&grand_total).unwrap_or(false);
        let show_balance = !paid_zero && paid.lt(&grand_total).unwrap_or(false);
        let field = |label: String, value: String| {
            div().flex_1().min_w_0().child(shown_field(label, value))
        };
        let blank = || div().flex_1().min_w_0();
        let pair = |a: Div, b: Div| div().h_flex().gap_6().child(a).child(b);
        let labelled_input = |label: String, state: &Entity<InputState>| {
            div()
                .v_flex()
                .flex_1()
                .min_w_0()
                .child(field_label(label, true, cx))
                .child(
                    control_frame(false, false).child(
                        Input::new(state)
                            .appearance(false)
                            .h(px(32.))
                            .px_3()
                            .text_13(),
                    ),
                )
        };

        let amount = control_frame(false, false).child(
            Input::new(&panel.amount)
                .appearance(false)
                .h(px(32.))
                .px_3()
                .text_13()
                .text_right(),
        );
        let method_buttons =
            div()
                .grid()
                .grid_cols(2)
                .gap_6()
                .children(methods.into_iter().enumerate().map(|(i, method)| {
                    big_button(
                        ("method", i),
                        method.clone(),
                        books::color("teal", 500),
                        px(40.),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_payment_method(method.clone(), window, cx)
                    }))
                }));
        let transfer = (!is_cash).then(|| {
            pair(
                labelled_input(t("Ref. / Cheque No."), &panel.reference),
                labelled_input(t("Clearance Date"), &panel.clearance),
            )
        });
        let change = pair(
            if show_change {
                field(
                    t("Paid Change"),
                    paid.sub(&grand_total)
                        .map(|m| self.currency(&m))
                        .unwrap_or_default(),
                )
            } else {
                blank()
            },
            if show_balance {
                field(
                    t("Balance Amount"),
                    grand_total
                        .sub(&paid)
                        .map(|m| self.currency(&m))
                        .unwrap_or_default(),
                )
            } else {
                blank()
            },
        );
        let mut totals = vec![
            field(t("Net Total"), self.money(sale.get("netTotal"))),
            field(
                t("Taxes and Charges"),
                self.currency(taxes.as_ref().unwrap_or(&zero)),
            ),
            field(
                t("Base Grand Total"),
                self.money(sale.get("baseGrandTotal")),
            ),
        ];
        if self.discounting() {
            totals.push(field(
                t("Discount Amount"),
                self.currency(discounts.as_ref().unwrap_or(&zero)),
            ));
        }
        totals.push(field(t("Grand Total"), self.money(sale.get("grandTotal"))));
        totals.push(field(
            t("Outstanding Amount"),
            self.money(sale.get("outstandingAmount")),
        ));
        let totals = div()
            .grid()
            .grid_cols(2)
            .gap_x_6()
            .gap_y_11()
            .children(totals);
        let button = |id: &'static str, label: String, fieldname: &str, default: u32| {
            big_button(
                id,
                label,
                self.button_colour(fieldname, default, false),
                PADDED,
            )
        };
        let buttons = div()
            .grid()
            .grid_cols(2)
            .gap_4()
            .child(
                button(
                    "submit-only",
                    t("Submit"),
                    "submitButtonColour",
                    GREEN_BUTTON,
                )
                .on_click(
                    cx.listener(|this, _, window, cx| this.transact(false, false, window, cx)),
                ),
            )
            .child(
                button(
                    "cancel-payment",
                    t("Cancel"),
                    "cancelButtonColour",
                    RED_BUTTON,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.payment = None;
                    cx.notify();
                })),
            )
            .child(
                button("pay-now", t("Pay"), "payButtonColour", GREEN_BUTTON).on_click(
                    cx.listener(|this, _, window, cx| this.transact(true, false, window, cx)),
                ),
            )
            .child(
                button(
                    "pay-print",
                    t("Pay & Print"),
                    "payAndPrintButtonColour",
                    GREEN_BUTTON,
                )
                .on_click(cx.listener(|this, _, window, cx| this.transact(true, true, window, cx))),
            );
        let viewport = window.viewport_size();
        Some(
            modal(
                modal_card()
                    .w(viewport.width / 3.)
                    .h(viewport.height * 0.95)
                    .child(
                        div()
                            .id("payment")
                            .v_flex()
                            .size_full()
                            .px_4()
                            .py_6()
                            .gap_4()
                            .justify_between()
                            .overflow_y_scroll()
                            .child(amount)
                            .child(method_buttons)
                            .children(transfer)
                            .child(totals)
                            .child(change)
                            .child(buttons),
                    ),
                true,
                window,
            )
            .into_any_element(),
        )
    }

    /// A list of sales, as the Saved Invoice and Return modals show them: Name, Customer,
    /// Date and Grand Total, a row a sale.
    fn sales_rows(
        &self,
        id: &'static str,
        sales: &[&DocValueMap],
        cx: &mut Context<Self>,
        on_pick: fn(&mut Self, String, &mut Window, &mut Context<Self>),
    ) -> Vec<Stateful<Div>> {
        sales
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let cell = |fieldname: &str| match row.get(fieldname) {
                    Some(DocValue::Text(text)) => text.clone(),
                    Some(DocValue::Date(date)) => self
                        .session
                        .borrow()
                        .formatter
                        .date(latte_core::fyo::format::utc_date(*date)),
                    value => self.money(value),
                };
                let name = cell("name");
                div()
                    .id((id, i))
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
                    .child(ratio_cell(1.).child(cell_text(name.clone(), false)))
                    .child(ratio_cell(1.).child(cell_text(cell("party"), false)))
                    .child(ratio_cell(1.).child(cell_text(cell("date"), false)))
                    .child(ratio_cell(0.8).child(cell_text(cell("grandTotal"), true)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        on_pick(this, name.clone(), window, cx)
                    }))
            })
            .collect()
    }

    fn sales_columns() -> [(String, f32, bool); 4] {
        [
            (t("Name"), 1., false),
            (t("Customer"), 1., false),
            (t("Date"), 1., false),
            (t("Grand Total"), 0.8, true),
        ]
    }

    /// `SavedInvoiceModal.vue`: the saved or submitted sales, searched by name.
    fn render_held_modal(
        &self,
        panel: &HeldPanel,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let viewport = window.viewport_size();
        let query = self.held_search.read(cx).value().to_lowercase();
        let sales: Vec<&DocValueMap> = panel
            .sales
            .iter()
            .filter(|row| {
                query.is_empty()
                    || matches!(row.get("name"), Some(DocValue::Text(n)) if n.to_lowercase().contains(&query))
            })
            .collect();
        let rows = self.sales_rows("held", &sales, cx, |this, name, window, cx| {
            this.take_up(name, window, cx)
        });
        let tab = |id: &'static str, label: String, submitted: bool| {
            div()
                .id(id)
                .flex_1()
                .mt_2()
                .p_2()
                .flex()
                .justify_center()
                .text_12()
                .text_color(gray(700))
                .cursor_pointer()
                .when(panel.submitted == submitted, |d| d.underline())
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| this.show_held(submitted, cx)))
        };
        let has_rows = !rows.is_empty();
        modal(
            modal_card()
                .w(viewport.height * 0.6 + px(40.))
                .p_5()
                .child(
                    div()
                        .text_center()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(t("Invoices")),
                )
                .child(rule().mt_2())
                .child(
                    div()
                        .mt_4()
                        .p_2()
                        .border_1()
                        .border_color(gray(200))
                        .rounded(books::RADIUS_MD)
                        .child(Input::new(&self.held_search).appearance(false).h(px(24.))),
                )
                .child(
                    div()
                        .h_flex()
                        .child(tab("held-saved", t("Saved"), false))
                        .child(tab("held-submitted", t("Submitted"), true)),
                )
                .child(table_header(&Self::sales_columns()).mt_2())
                .when(has_rows, |d| {
                    d.child(
                        div()
                            .id("held-sales")
                            .h(viewport.height * 0.65)
                            .overflow_y_scroll()
                            .children(rows),
                    )
                })
                .child(
                    div().mt_4().h_flex().child(
                        big_button(
                            "held-cancel",
                            t("Cancel"),
                            books::color("red", 500),
                            px(40.),
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.held = None;
                            cx.notify();
                        })),
                    ),
                ),
            false,
            window,
        )
        .into_any_element()
    }

    /// `BatchSelectionModal.vue`: a batch of the item, then Select or Cancel.
    fn render_batch_modal(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (_, _, picker) = self.batch_for.as_ref()?;
        let chosen = picker.read(cx).value().is_some();
        Some(
            modal(
                modal_card()
                    .w(px(384.))
                    .child(
                        div()
                            .py_3()
                            .text_center()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(gray(800))
                            .child(t("Select the Batch")),
                    )
                    .child(
                        div()
                            .px_10()
                            .pt_6()
                            .child(field_label(t("Batch"), true, cx))
                            .child(picker.clone())
                            .child(
                                div()
                                    .h_flex()
                                    .gap_4()
                                    .mt_8()
                                    .mb_6()
                                    .child(
                                        big_button(
                                            "batch-select",
                                            t("Select"),
                                            books::color("green", 500),
                                            PADDED,
                                        )
                                        .when(!chosen, |d| d.opacity(0.5))
                                        .on_click(
                                            cx.listener(|this, _, window, cx| {
                                                let Some((item, quantity, picker)) =
                                                    this.batch_for.clone()
                                                else {
                                                    return;
                                                };
                                                let Some(batch) =
                                                    picker.read(cx).value().map(|v| v.to_string())
                                                else {
                                                    return;
                                                };
                                                this.add(item, Some(batch), quantity, window, cx)
                                            }),
                                        ),
                                    )
                                    .child(
                                        big_button(
                                            "batch-cancel",
                                            t("Cancel"),
                                            books::color("red", 500),
                                            PADDED,
                                        )
                                        .on_click(
                                            cx.listener(|this, _, _, cx| {
                                                this.batch_for = None;
                                                cx.notify();
                                            }),
                                        ),
                                    ),
                            ),
                    ),
                false,
                window,
            )
            .into_any_element(),
        )
    }

    /// Opens a modal, or closes it if it is the one open (`toggleModal`).
    pub(super) fn toggle_overlay_kind(
        &mut self,
        kind: OverlayKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let open = matches!(
            (&self.overlay, kind),
            (Some(Overlay::PriceList(_)), OverlayKind::PriceList)
                | (Some(Overlay::Coupon { .. }), OverlayKind::Coupon)
                | (Some(Overlay::Loyalty { .. }), OverlayKind::Loyalty)
                | (Some(Overlay::Enquiry(_)), OverlayKind::Enquiry)
        );
        if open {
            self.overlay = None;
            cx.notify();
            return;
        }
        match kind {
            OverlayKind::PriceList => self.open_price_list(window, cx),
            OverlayKind::Coupon => self.open_coupon(window, cx),
            OverlayKind::Loyalty => self.open_loyalty(window, cx),
            OverlayKind::Enquiry => self.open_enquiry(window, cx),
        }
        cx.notify();
    }

    fn open_price_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (lists, label) = {
            let s = self.session.borrow();
            (
                pos::price_lists(&s.fyo).unwrap_or_default(),
                field_label_of(&s.fyo, "SalesInvoice", "priceList"),
            )
        };
        let current = self
            .sale
            .as_ref()
            .map(|sale| doc_text(sale, "priceList"))
            .unwrap_or_default();
        let link = cx.new(|cx| {
            let mut link = PosLink::new(lists, label, window, cx);
            link.set_value(Some(current.into()), window, cx);
            link
        });
        self._subscriptions.push(cx.subscribe_in(
            &link,
            window,
            |this, _, event: &PosLinkEvent, _, cx| match event {
                PosLinkEvent::Picked(list) => this.apply_price_list(list.to_string(), cx),
                PosLinkEvent::Opened(list) => this.open_form("PriceList", list, cx),
                PosLinkEvent::Entered(_) => {}
            },
        ));
        link.update(cx, |link, cx| link.focus(window, cx));
        self.overlay = Some(Overlay::PriceList(link));
    }

    /// `applyPriceList`: a new list applies, and the modal closes.
    fn apply_price_list(&mut self, list: String, cx: &mut Context<Self>) {
        let current = self.sale.as_ref().map(|sale| doc_text(sale, "priceList"));
        if list.is_empty() || current.as_deref() == Some(list.as_str()) {
            return;
        }
        if self.change_sale(cx, |fyo, sale, _| {
            pos::set_price_list(fyo, sale, Some(&list))
        }) {
            self.overlay = None;
        }
    }

    fn open_coupon(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (codes, label) = {
            let s = self.session.borrow();
            (
                pos::coupon_codes(&s.fyo).unwrap_or_default(),
                field_label_of(&s.fyo, "AppliedCouponCodes", "coupons"),
            )
        };
        let link = cx.new(|cx| PosLink::new(codes, label, window, cx));
        self._subscriptions.push(cx.subscribe_in(
            &link,
            window,
            |this, link, event: &PosLinkEvent, window, cx| match event {
                PosLinkEvent::Picked(code) => {
                    this.apply_coupon(code.to_string(), link.clone(), window, cx)
                }
                PosLinkEvent::Opened(code) => this.open_form("CouponCode", code, cx),
                PosLinkEvent::Entered(_) => {}
            },
        ));
        link.update(cx, |link, cx| link.focus(window, cx));
        self.overlay = Some(Overlay::Coupon {
            link,
            invalid: false,
        });
    }

    /// `updateCouponCode`.
    fn apply_coupon(
        &mut self,
        code: String,
        link: Entity<PosLink>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if code.trim().is_empty() {
            return;
        }
        let applied =
            self.change_sale(cx, |fyo, sale, _| pos::apply_coupon(fyo, sale, code.trim()));
        if let Some(Overlay::Coupon { invalid, .. }) = &mut self.overlay {
            *invalid = !applied;
        }
        if applied {
            link.update(cx, |link, cx| link.set_value(None, window, cx));
        }
    }

    pub(super) fn open_loyalty(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let points = self
            .sale
            .as_ref()
            .map_or(0.0, |sale| doc_number(sale, "loyaltyPoints"));
        let state = input(number_text(points), window, cx);
        self._subscriptions.push(cx.subscribe_in(
            &state,
            window,
            |this, state, event: &InputEvent, _, cx| match event {
                InputEvent::Blur => {
                    let text = state.read(cx).value().trim().to_owned();
                    this.redeem_points(&text, cx);
                }
                InputEvent::PressEnter { .. } => {
                    let text = state.read(cx).value().trim().to_owned();
                    if this.redeem_points(&text, cx) {
                        this.overlay = None;
                    }
                }
                _ => {}
            },
        ));
        state.update(cx, |state, cx| state.focus(window, cx));
        self.overlay = Some(Overlay::Loyalty {
            points: state,
            invalid: false,
        });
    }

    /// `updateLoyaltyPoints`. Whether the points are redeemed.
    fn redeem_points(&mut self, text: &str, cx: &mut Context<Self>) -> bool {
        let current = self
            .sale
            .as_ref()
            .map_or(0.0, |sale| doc_number(sale, "loyaltyPoints"));
        let Ok(points) = text.parse::<f64>() else {
            self.error = Some(format!("{text} is not a number"));
            self.set_loyalty_invalid(true);
            cx.notify();
            return false;
        };
        if points == current {
            return true;
        }
        let available = self.loyalty.points;
        let ok = self.change_sale(cx, |fyo, sale, _| {
            pos::redeem_loyalty_points(fyo, sale, points, available)
        });
        self.set_loyalty_invalid(!ok);
        ok
    }

    fn set_loyalty_invalid(&mut self, value: bool) {
        if let Some(Overlay::Loyalty { invalid, .. }) = &mut self.overlay {
            *invalid = value;
        }
    }

    fn open_enquiry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (items, parties) = {
            let s = self.session.borrow();
            (
                names(&s.fyo, "Item", QueryFilter::new()),
                names(&s.fyo, "Party", QueryFilter::new()),
            )
        };
        let item = cx.new(|cx| PosLink::new(items.clone(), t("Item"), window, cx));
        let similar = cx.new(|cx| PosLink::new(items, t("Similar Product"), window, cx));
        let customer = cx.new(|cx| PosLink::new(parties, t("Customer"), window, cx));
        let description = cx.new(|cx| TextareaState::new(window, cx).placeholder(t("Description")));
        let contact = cx.new(|cx| InputState::new(window, cx).placeholder(t("Contact")));
        let for_contact = contact.clone();
        self._subscriptions.push(cx.subscribe_in(
            &customer,
            window,
            move |this, _, event: &PosLinkEvent, window, cx| {
                // `updateCustomerContact`.
                if let PosLinkEvent::Picked(party) = event {
                    let phone = {
                        let s = this.session.borrow();
                        pos::party_phone(&s.fyo, party)
                    };
                    for_contact.update(cx, |input, cx| input.set_value(phone, window, cx));
                }
            },
        ));
        self.overlay = Some(Overlay::Enquiry(Box::new(EnquiryForm {
            item,
            description,
            customer,
            contact,
            similar,
        })));
    }

    /// `submitForm`.
    fn submit_enquiry(&mut self, cx: &mut Context<Self>) {
        let Some(Overlay::Enquiry(form)) = &self.overlay else {
            return;
        };
        let link_text = |link: &Entity<PosLink>| link.read(cx).text(cx).trim().to_owned();
        let enquiry = pos::ItemEnquiry {
            item: link_text(&form.item),
            description: form.description.read(cx).value().trim().to_owned(),
            customer: link_text(&form.customer),
            contact: value(&form.contact, cx),
            similar_product: link_text(&form.similar),
        };
        let result = {
            let mut s = self.session.borrow_mut();
            pos::submit_item_enquiry(&mut s.fyo, &enquiry)
        };
        match result {
            Ok(_) => {
                self.error = None;
                self.info = Some(t("Item enquiry submitted"));
                self.overlay = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    /// The Return button: `ReturnSalesInvoiceModal.vue`.
    pub(super) fn open_return(&mut self, cx: &mut Context<Self>) {
        let sales = {
            let s = self.session.borrow();
            pos::returnable_sales(&s.fyo)
        };
        match sales {
            Ok(sales) => {
                self.overlay = Some(Overlay::Return(ReturnPanel {
                    sales,
                    page: 1,
                    count: Some(20),
                }))
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    fn filtered_returns<'a>(&self, panel: &'a ReturnPanel, cx: &App) -> Vec<&'a DocValueMap> {
        let query = self.return_search.read(cx).value().to_lowercase();
        panel
            .sales
            .iter()
            .filter(|row| {
                matches!(row.get("name"), Some(DocValue::Text(n)) if n.to_lowercase().contains(&query))
            })
            .collect()
    }

    /// `handleSearchEnter`: the one sale found is returned.
    pub(super) fn return_search_entered(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Overlay::Return(panel)) = &self.overlay else {
            return;
        };
        let found = self.filtered_returns(panel, cx);
        if let [row] = found.as_slice()
            && let Some(DocValue::Text(name)) = row.get("name")
        {
            let name = name.clone();
            self.pick_return(name, window, cx);
        }
    }

    /// `selectedReturnInvoice`: the return of a sale becomes the cart, to pay back.
    fn pick_return(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        let result = {
            let mut s = self.session.borrow_mut();
            pos::return_sale(&mut s.fyo, &name)
        };
        match result {
            Ok(Some(sale)) => self.take_sale(sale, window, cx),
            Ok(None) => self.overlay = None,
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    /// `handleOpenKeyboard`.
    pub(super) fn open_keyboard(
        &mut self,
        row: usize,
        field: KeypadField,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(doc) = self.sale.as_ref().and_then(|s| s.table("items").get(row)) else {
            return;
        };
        self.selected_row = Some(row);
        let value = input(pos::keypad_start(doc, field), window, cx);
        value.update(cx, |state, cx| state.focus(window, cx));
        self.overlay = Some(Overlay::Keyboard { row, field, value });
        cx.notify();
    }

    fn keypad(&mut self, key: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(Overlay::Keyboard { value, .. }) = &self.overlay {
            let next = pos::keypad_press(&value.read(cx).value(), key);
            value.update(cx, |state, cx| {
                state.set_value(next, window, cx);
                state.focus(window, cx);
            });
        }
    }

    /// `saveSelectedItem`.
    fn save_keypad(&mut self, cx: &mut Context<Self>) {
        let Some(Overlay::Keyboard { row, field, value }) = &self.overlay else {
            return;
        };
        let (row, field, text) = (*row, *field, value.read(cx).value().to_string());
        if self.change_sale(cx, |fyo, sale, qty_map| {
            pos::keypad_save(fyo, sale, row, field, &text, qty_map)
        }) {
            self.overlay = None;
        }
    }

    fn render_overlay(
        &self,
        overlay: &Overlay,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let close = |id: &'static str, label: String, cx: &mut Context<Self>| {
            big_button(id, label, books::color("red", 500), PADDED).on_click(cx.listener(
                |this, _, _, cx| {
                    this.overlay = None;
                    cx.notify();
                },
            ))
        };
        let card = match overlay {
            Overlay::Alert => self.render_alert(cx),
            Overlay::PriceList(link) => modal_card()
                .w(px(384.))
                .child(modal_title(t("Apply Price List")))
                .child(
                    div()
                        .px_10()
                        .child(rule())
                        .child(
                            div()
                                .h_flex()
                                .w_full()
                                .pt_10()
                                .mb_20()
                                .child(div().flex_1().min_w_0().child(link.clone()))
                                .child(
                                    div()
                                        .id("remove-price-list")
                                        .w_10()
                                        .h_flex()
                                        .justify_end()
                                        .cursor_pointer()
                                        .child(books::icon(
                                            "trash",
                                            px(20.),
                                            books::color("red", 500),
                                        ))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.change_sale(cx, |fyo, sale, _| {
                                                pos::set_price_list(fyo, sale, None)
                                            });
                                            if let Some(Overlay::PriceList(link)) = &this.overlay {
                                                link.update(cx, |link, cx| {
                                                    link.set_value(None, window, cx)
                                                });
                                            }
                                        })),
                                ),
                        )
                        .child(
                            div().h_flex().mb_2().child(
                                big_button(
                                    "price-list-save",
                                    t("Save"),
                                    books::color("green", 500),
                                    PADDED,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.overlay = None;
                                        cx.notify();
                                    },
                                )),
                            ),
                        )
                        .child(div().h_flex().mb_8().child(close(
                            "price-list-cancel",
                            t("Cancel"),
                            cx,
                        ))),
                ),
            Overlay::Coupon { link, invalid } => self.render_coupon(link, *invalid, window, cx),
            Overlay::Loyalty { points, invalid } => self.render_loyalty(points, *invalid, cx),
            Overlay::Enquiry(form) => self.render_enquiry(form, cx),
            Overlay::Return(panel) => self.render_return(panel, window, cx),
            Overlay::Keyboard { row, field, value } => {
                self.render_keyboard(*row, *field, value, window, cx)
            }
        };
        modal(card, false, window).into_any_element()
    }

    /// `AlertModal.vue`.
    fn render_alert(&self, cx: &mut Context<Self>) -> Div {
        modal_card()
            .px_6()
            .child(modal_title(t("Alert")))
            .child(rule())
            .child(
                div()
                    .py_6()
                    .child(t("Clicking continue will remove all the selected items.")),
            )
            .child(
                div()
                    .v_flex()
                    .pb_6()
                    .child(
                        div()
                            .h_flex()
                            .gap_5()
                            .child(
                                big_button(
                                    "alert-cancel",
                                    t("Cancel"),
                                    books::color("red", 500),
                                    px(40.),
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.overlay = None;
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                big_button(
                                    "alert-continue",
                                    t("Continue"),
                                    books::color("green", 500),
                                    px(40.),
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.overlay = None;
                                        Self::go_to_list(cx);
                                    },
                                )),
                            ),
                    )
                    .child(
                        div().h_flex().mt_3().child(
                            big_button(
                                "alert-save",
                                t("Save and Continue"),
                                books::color("blue", 500),
                                px(40.),
                            )
                            .on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.save_and_continue(window, cx)
                                }),
                            ),
                        ),
                    ),
            )
    }

    /// `CouponCodeModal.vue`: the coupons applied, each with its trash icon, and the
    /// Coupons link to apply one.
    fn render_coupon(
        &self,
        link: &Entity<PosLink>,
        invalid: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let applied: Vec<String> = self
            .sale
            .as_ref()
            .map(|sale| {
                sale.table("coupons")
                    .iter()
                    .map(|row| doc_text(row, "coupons"))
                    .collect()
            })
            .unwrap_or_default();
        let list = (!applied.is_empty()).then(|| {
            let height =
                window.viewport_size().height * if applied.len() >= 2 { 0.11 } else { 0.08 };
            div()
                .v_flex()
                .child(
                    div()
                        .m_2()
                        .text_11()
                        .text_color(gray(500))
                        .child(t("Applied Coupon Codes")),
                )
                .child(
                    div()
                        .id("applied-coupons")
                        .mt_2()
                        .h(height)
                        .overflow_y_scroll()
                        .children(applied.into_iter().enumerate().map(|(idx, coupon)| {
                            div()
                                .id(("coupon", idx))
                                .relative()
                                .h_flex()
                                .w_full()
                                .h(px(40.))
                                .border_b_1()
                                .border_l_1()
                                .border_r_1()
                                .border_color(gray(200))
                                .hover(|d| d.bg(gray(25)))
                                .child(cell_text(coupon, false))
                                .child(
                                    div()
                                        .id(("remove-coupon", idx))
                                        .absolute()
                                        .right_3()
                                        .cursor_pointer()
                                        .child(books::icon(
                                            "trash",
                                            px(16.),
                                            books::color("red", 500),
                                        ))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.change_sale(cx, |fyo, sale, _| {
                                                pos::remove_coupon(fyo, sale, idx)
                                            });
                                        })),
                                )
                        })),
                )
        });
        let has_applied = list.is_some();
        modal_card()
            .w(px(384.))
            .child(modal_title(t("Apply Coupon Code")))
            .child(
                div()
                    .px_10()
                    .child(rule())
                    .children(list)
                    .child(
                        div()
                            .h_flex()
                            .justify_center()
                            .map(|d| if has_applied { d.pt_4() } else { d.pt_10() })
                            .child(
                                div()
                                    .w(px(320.))
                                    .map(|d| if has_applied { d.pb_4() } else { d.pb_10() })
                                    .child(labelled(t("Coupons"), link.clone())),
                            ),
                    )
                    .child(
                        div().h_flex().mb_2().child(
                            big_button(
                                "coupon-save",
                                t("Save"),
                                books::color("green", 500),
                                PADDED,
                            )
                            .when(invalid, |d| d.opacity(0.5))
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if !invalid {
                                        this.overlay = None;
                                        cx.notify();
                                    }
                                },
                            )),
                        ),
                    )
                    .child(
                        div().h_flex().mb_8().child(
                            big_button(
                                "coupon-cancel",
                                t("Cancel"),
                                books::color("red", 500),
                                PADDED,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.overlay = None;
                                cx.notify();
                            })),
                        ),
                    ),
            )
    }

    /// `LoyaltyProgramModal.vue`: the customer's points, and how many to redeem.
    fn render_loyalty(
        &self,
        points: &Entity<InputState>,
        invalid: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let label = {
            let s = self.session.borrow();
            field_label_of(&s.fyo, "SalesInvoice", "loyaltyPoints")
        };
        modal_card()
            .w(px(384.))
            .h(px(384.))
            .child(div().py_4().text_center().child(t("Redeem Loyalty Points")))
            .child(rule())
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .p_3()
                    .pt_10()
                    .justify_end()
                    .child(coins_icon())
                    .child(number_text(self.loyalty.points)),
            )
            .child(
                div().px_10().pb_10().child(labelled(
                    label,
                    control_frame(false, false).child(
                        Input::new(points)
                            .appearance(false)
                            .h(px(32.))
                            .px_3()
                            .text_13(),
                    ),
                )),
            )
            .child(
                div().h_flex().px_10().mb_2().child(
                    big_button(
                        "loyalty-save",
                        t("Save"),
                        books::color("green", 500),
                        PADDED,
                    )
                    .when(invalid, |d| d.opacity(0.5))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let text = match &this.overlay {
                            Some(Overlay::Loyalty { points, .. }) => value(points, cx),
                            _ => return,
                        };
                        if this.redeem_points(&text, cx) {
                            this.overlay = None;
                        }
                        cx.notify();
                    })),
                ),
            )
            .child(
                div().h_flex().px_10().child(
                    big_button(
                        "loyalty-cancel",
                        t("Cancel"),
                        books::color("red", 500),
                        PADDED,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.change_sale(cx, |fyo, sale, _| pos::cancel_loyalty_points(fyo, sale));
                        this.overlay = None;
                    })),
                ),
            )
    }

    /// `ItemEnquiryModal.vue`.
    fn render_enquiry(&self, form: &EnquiryForm, cx: &mut Context<Self>) -> Div {
        let text_input = |state: &Entity<InputState>| {
            control_frame(false, false).child(
                Input::new(state)
                    .appearance(false)
                    .h(px(32.))
                    .px_3()
                    .text_13(),
            )
        };
        modal_card()
            .w(px(384.))
            .child(modal_title(t("Item Enquiry")))
            .child(
                div()
                    .px_10()
                    .child(rule())
                    .child(
                        div()
                            .v_flex()
                            .gap_5()
                            .pt_8()
                            .child(labelled(t("Item"), form.item.clone()))
                            .child(labelled(
                                t("Description"),
                                control_frame(false, false).items_start().child(
                                    Textarea::new(&form.description)
                                        .appearance(false)
                                        .h(px(3. * 19.5 + 16.))
                                        .px_1()
                                        .py_0()
                                        .text_13(),
                                ),
                            ))
                            .child(labelled(t("Customer"), form.customer.clone()))
                            .child(labelled(t("Contact"), text_input(&form.contact)))
                            .child(labelled(t("Similar Product"), form.similar.clone())),
                    )
                    .child(
                        div().h_flex().mt_10().mb_4().child(
                            big_button(
                                "enquiry-submit",
                                t("Submit"),
                                books::color("green", 500),
                                PADDED,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.submit_enquiry(cx))),
                        ),
                    )
                    .child(
                        div().h_flex().mb_6().child(
                            big_button(
                                "enquiry-cancel",
                                t("Cancel"),
                                books::color("red", 500),
                                PADDED,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.overlay = None;
                                cx.notify();
                            })),
                        ),
                    ),
            )
    }

    /// `ReturnSalesInvoiceModal.vue`: the sales that can be returned, searched by name, a
    /// page at a time.
    fn render_return(&self, panel: &ReturnPanel, window: &Window, cx: &mut Context<Self>) -> Div {
        let viewport = window.viewport_size();
        let found = self.filtered_returns(panel, cx);
        let total = found.len();
        let count = panel.count.unwrap_or(total.max(1)).max(1);
        let pages = total.div_ceil(count).max(1);
        let page = panel.page.clamp(1, pages);
        let shown: Vec<&DocValueMap> = found
            .iter()
            .skip((page - 1) * count)
            .take(count)
            .copied()
            .collect();
        let rows = self.sales_rows("return", &shown, cx, |this, name, window, cx| {
            this.pick_return(name, window, cx)
        });
        let set_page = |to: usize| {
            cx.listener(move |this: &mut Self, _: &ClickEvent, _, cx| {
                if let Some(Overlay::Return(panel)) = &mut this.overlay {
                    panel.page = to;
                }
                cx.notify();
            })
        };
        let arrow = |id: &'static str, icon: &'static str, enabled: bool| {
            div()
                .id(id)
                .when(enabled, |d| d.cursor_pointer())
                .child(books::icon(
                    icon,
                    px(16.),
                    if enabled {
                        gray(600)
                    } else {
                        white().opacity(0.)
                    },
                ))
        };
        let counts: Vec<Option<usize>> = [Some(20), Some(40), None]
            .into_iter()
            .filter(|c| match c {
                Some(c) => total >= *c,
                None => total >= 20,
            })
            .collect();
        let paginator = div()
            .h_flex()
            .h(px(50.))
            .text_12()
            .text_color(gray(800))
            .child(div().flex_1().child(format!(
                "{} - {}",
                (page - 1) * count + 1,
                (page * count).min(total)
            )))
            .child(
                div()
                    .flex_1()
                    .h_flex()
                    .justify_center()
                    .gap_1()
                    .child(
                        arrow("return-back", "chevron-left", page > 1)
                            .on_click(set_page(page.saturating_sub(1).max(1))),
                    )
                    .child(
                        div()
                            .h_flex()
                            .gap_1()
                            .bg(gray(100))
                            .rounded(books::RADIUS)
                            .child(div().w_7().h_flex().justify_end().child(page.to_string()))
                            .child(div().text_color(gray(600)).child("/"))
                            .child(div().w_7().child(pages.to_string())),
                    )
                    .child(
                        arrow("return-next", "chevron-right", page < pages)
                            .on_click(set_page((page + 1).min(pages))),
                    ),
            )
            .child(
                div().flex_1().h_flex().justify_end().child(
                    div()
                        .h_flex()
                        .when(!counts.is_empty(), |d| {
                            d.border_1().border_color(gray(100)).rounded(books::RADIUS)
                        })
                        .children(counts.into_iter().map(|c| {
                            let selected = panel.count == c;
                            div()
                                .id(SharedString::from(format!("return-count-{c:?}")))
                                .w_9()
                                .h_flex()
                                .justify_center()
                                .cursor_pointer()
                                .when(selected, |d| d.bg(gray(100)).rounded(books::RADIUS))
                                .child(c.map_or_else(|| t("All"), |c| c.to_string()))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(Overlay::Return(panel)) = &mut this.overlay {
                                        panel.count = c;
                                        panel.page = 1;
                                    }
                                    cx.notify();
                                }))
                        })),
                ),
            );
        modal_card()
            .p_5()
            .child(
                div()
                    .text_center()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t("Invoices")),
            )
            .child(rule().mt_2())
            .child(
                div()
                    .mt_4()
                    .p_2()
                    .border_1()
                    .border_color(gray(200))
                    .rounded(books::RADIUS_MD)
                    .child(Input::new(&self.return_search).appearance(false).h(px(24.))),
            )
            .child(rule().mt_2())
            .child(table_header(&Self::sales_columns()).mt_2())
            .child(
                div()
                    .id("return-sales")
                    .h(viewport.height * 0.65)
                    .w(viewport.height * 0.6)
                    .overflow_y_scroll()
                    .children(rows),
            )
            .child(div().mt_1().mb_1().child(paginator))
            .child(
                div().h_flex().mt_1().child(
                    big_button(
                        "return-cancel",
                        t("Cancel"),
                        books::color("red", 500),
                        px(40.),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.overlay = None;
                        cx.notify();
                    })),
                ),
            )
    }

    /// `KeyboardModal.vue`: the field, the keys, Save and Cancel.
    fn render_keyboard(
        &self,
        row: usize,
        field: KeypadField,
        state: &Entity<InputState>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let _ = row;
        let label = {
            let s = self.session.borrow();
            field_label_of(&s.fyo, "SalesInvoiceItem", field.fieldname())
        };
        let key = |key: &'static str, label: &'static str| {
            keypad_key(SharedString::from(format!("key-{key}")), label)
                .on_click(cx.listener(move |this, _, window, cx| this.keypad(key, window, cx)))
        };
        let keys = div()
            .grid()
            .grid_cols(4)
            .gap_3()
            .py_4()
            .children([
                key("7", "7"),
                key("8", "8"),
                key("9", "9"),
                key("Del", "Del"),
                key("4", "4"),
                key("5", "5"),
                key("6", "6"),
                key("-", "-"),
                key("1", "1"),
                key("2", "2"),
                key("3", "3"),
                key("+", "+"),
                key(".", "•"),
                key("0", "0"),
            ])
            .child(div().col_span(2).child(key("Clear", "Clear")));
        modal_card().child(
            div()
                .px_5()
                .w(window.viewport_size().width * 0.3)
                .child(modal_title(t("Keyboard")))
                .child(rule())
                .child(
                    div()
                        .mx_6()
                        .my_3()
                        .child(
                            div().mb_3().child(labelled(
                                label,
                                control_frame(false, true).child(
                                    Input::new(state)
                                        .appearance(false)
                                        .h(px(32.))
                                        .px_3()
                                        .text_13(),
                                ),
                            )),
                        )
                        .child(keys),
                )
                .child(
                    div()
                        .px_5()
                        .child(
                            div().h_flex().mb_3().child(
                                big_button(
                                    "keyboard-save",
                                    t("Save"),
                                    books::color("green", 500),
                                    PADDED,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.save_keypad(cx))),
                            ),
                        )
                        .child(
                            div().h_flex().mb_8().child(
                                big_button(
                                    "keyboard-cancel",
                                    t("Cancel"),
                                    books::color("red", 500),
                                    PADDED,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.overlay = None;
                                        cx.notify();
                                    },
                                )),
                            ),
                        ),
                ),
        )
    }
}

/// A field's label in its schema, as `fieldMap` gives it.
fn field_label_of(fyo: &latte_core::Fyo, schema: &str, fieldname: &str) -> String {
    fyo.schema(schema)
        .and_then(|s| s.field(fieldname))
        .and_then(|f| f.label.clone())
        .map_or_else(|| fieldname.to_owned(), |label| t(&label))
}

/// `Table.vue` with `show-header` and `border`, as the shift modals show theirs: `#` and
/// the columns, 48px rows, and Add Row when rows can be added.
fn shift_table(
    columns: &[(String, bool)],
    rows: Vec<Vec<AnyElement>>,
    add_row: Option<AnyElement>,
) -> Div {
    let header = div()
        .h_flex()
        .w_full()
        .h_12()
        .px_2()
        .text_color(gray(600))
        .child(ratio_cell(0.3).pl_2().child("#"))
        .children(columns.iter().map(|(label, numeric)| {
            ratio_cell(1.)
                .px_2()
                .when(*numeric, |d| d.justify_end())
                .child(div().truncate().child(label.clone()))
        }));
    div()
        .v_flex()
        .w_full()
        .text_13()
        .text_color(gray(900))
        .border_1()
        .border_color(gray(200))
        .rounded(books::RADIUS_MD)
        .child(header)
        .children(rows.into_iter().enumerate().map(|(i, cells)| {
            div()
                .h_flex()
                .w_full()
                .h_12()
                .px_2()
                .border_t_1()
                .border_color(gray(200))
                .child(
                    ratio_cell(0.3)
                        .pl_2()
                        .text_color(gray(600))
                        .child(format!("{}", i + 1)),
                )
                .children(cells.into_iter().map(|cell| ratio_cell(1.).child(cell)))
        }))
        .children(add_row)
}
