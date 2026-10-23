//! The point of sale: `src/pages/POS/` @ v0.36.0 (the Classic UI, with the open and close
//! shift and payment dialogs).
//!
//! The work is done by `latte_core::pos`; this draws it.
//!
//! Differences from Books:
//! - Opening and closing a shift, and paying, are panels on the page rather than dialogs.
//! - Saved sales are listed beside the cart rather than in a dialog, without a search.
//! - Not here yet: the Modern UI, item groups, the keyboard shortcuts, returns, coupons,
//!   loyalty points, price lists, and editing a row's rate or discount. Pricing rules
//!   apply as on any invoice.

use std::collections::HashMap;

use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use latte_core::Doc;
use latte_core::fyo::model::helpers::is_truthy;
use latte_core::models::inventory::point_of_sale::cash_amount;
use latte_core::pos::{self, ItemQtyMap, PosItem, PosPayment};
use latte_core::views::sidebar::Route;
use latte_db::pesa::Money;
use latte_db::{DocValue, DocValueMap, Filter, GetAllOptions, QueryFilter};

use crate::i18n::t;
use crate::link::{LinkInput, LinkOption, LinkPicked};
use crate::session::Shared;
use crate::widgets::{error_message, field_label, info_message, page_header};
use crate::{Nav, Navigate};

/// A row of counted cash: its denomination and how many.
struct CashRow {
    denomination: Entity<InputState>,
    count: Entity<InputState>,
}

/// The shift being opened, or closed.
struct ShiftPanel {
    opening: Option<Doc>,
    cash: Vec<CashRow>,
    /// The amount of each payment method but Cash: opening amounts, or those counted at
    /// closing.
    amounts: Vec<(String, Entity<InputState>)>,
    /// At closing: what each method is expected to have.
    expected: HashMap<String, Money>,
}

struct PaymentPanel {
    method: String,
    amount: Entity<InputState>,
    reference: Entity<InputState>,
    clearance: Entity<InputState>,
    serial_numbers: Vec<(String, Entity<InputState>)>,
}

/// The saved sales: those not submitted, or those submitted and not paid.
struct HeldPanel {
    submitted: bool,
    sales: Vec<DocValueMap>,
}

enum Mode {
    Opening(ShiftPanel),
    Selling,
    Closing(ShiftPanel),
}

pub struct PosPage {
    session: Shared,
    mode: Mode,
    sale: Option<Doc>,
    qty_map: ItemQtyMap,
    items: Vec<PosItem>,
    search: Entity<InputState>,
    customer: Entity<LinkInput>,
    payment: Option<PaymentPanel>,
    held: Option<HeldPanel>,
    /// An item to pick a batch of, and how many to add.
    batch_for: Option<(PosItem, f64)>,
    error: Option<String>,
    info: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Navigate> for PosPage {}

fn input(text: impl Into<SharedString>, window: &mut Window, cx: &mut App) -> Entity<InputState> {
    let text = text.into();
    cx.new(|cx| InputState::new(window, cx).default_value(text))
}

fn value(state: &Entity<InputState>, cx: &App) -> String {
    state.read(cx).value().trim().to_owned()
}

fn money_text(value: Option<&DocValue>) -> String {
    match value {
        Some(DocValue::Money(money)) => money.round(None),
        Some(DocValue::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn number_text(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{n:.0}")
    } else {
        n.to_string()
    }
}

impl PosPage {
    pub fn new(session: Shared, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx
            .new(|cx| InputState::new(window, cx).placeholder(t("Search Item (Name or Barcode)")));
        let customers = customer_options(&session);
        let customer = cx.new(|cx| LinkInput::new(customers, t("Customer"), window, cx));
        let subscriptions = vec![
            cx.observe(&search, |_, _, cx| cx.notify()),
            cx.subscribe_in(
                &search,
                window,
                |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.search_entered(window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &customer,
                window,
                |this, _, event: &LinkPicked, window, cx| {
                    this.set_customer(event.0.to_string(), window, cx)
                },
            ),
        ];
        let mut page = Self {
            session,
            mode: Mode::Selling,
            sale: None,
            qty_map: ItemQtyMap::new(),
            items: Vec::new(),
            search,
            customer,
            payment: None,
            held: None,
            batch_for: None,
            error: None,
            info: None,
            _subscriptions: subscriptions,
        };
        page.start(window, cx);
        page
    }

    /// `activated`: checks the settings, then opens a shift or starts a sale.
    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let open = {
            let s = self.session.borrow();
            if let Err(e) = pos::validate_settings(&s.fyo) {
                self.error = Some(e.to_string());
            }
            latte_core::models::helpers::single_is_truthy(&s.fyo, "POSSettings", "isShiftOpen")
        };
        if open {
            self.mode = Mode::Selling;
            self.new_sale(None, window, cx);
        } else {
            self.begin_opening(window, cx);
        }
        cx.notify();
    }

    fn refresh_items(&mut self) {
        let s = self.session.borrow();
        match pos::item_qty_map(&s.fyo).and_then(|map| {
            let items = pos::items(&s.fyo, &map, None)?;
            Ok((map, items))
        }) {
            Ok((map, items)) => {
                self.qty_map = map;
                self.items = items;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn new_sale(&mut self, party: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_items();
        let sale = {
            let s = self.session.borrow();
            pos::new_sale(&s.fyo, party.as_deref())
        };
        match sale {
            Ok(sale) => {
                let party = match sale.get("party") {
                    Some(DocValue::Text(p)) if !p.is_empty() => Some(p.clone().into()),
                    _ => None,
                };
                self.customer
                    .update(cx, |link, cx| link.set_value(party, window, cx));
                self.sale = Some(sale);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        self.payment = None;
        self.held = None;
        self.batch_for = None;
    }

    fn set_customer(&mut self, party: String, _: &mut Window, cx: &mut Context<Self>) {
        let Some(sale) = &mut self.sale else {
            return;
        };
        let mut s = self.session.borrow_mut();
        let value = if party.is_empty() {
            DocValue::Null
        } else {
            DocValue::Text(party)
        };
        if let Err(e) = sale.set(&mut s.fyo, "party", value) {
            self.error = Some(e.to_string());
        }
        cx.notify();
    }

    /// `handleItemSearch` on Enter: adds the item a name or barcode finds.
    fn search_entered(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let search = value(&self.search, cx);
        if search.is_empty() {
            return;
        }
        let found = {
            let s = self.session.borrow();
            pos::find_item(&s.fyo, &search)
        };
        match found {
            Ok(Some((name, quantity))) => {
                if let Some(item) = self.items.iter().find(|i| i.name == name).cloned() {
                    self.add(item, None, quantity, cx);
                    self.search
                        .update(cx, |input, cx| input.set_value("", window, cx));
                }
            }
            Ok(None) => {}
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    fn add(&mut self, item: PosItem, batch: Option<String>, quantity: f64, cx: &mut Context<Self>) {
        self.error = None;
        self.info = None;
        if item.has_batch && batch.is_none() {
            self.batch_for = Some((item, quantity));
            cx.notify();
            return;
        }
        self.batch_for = None;
        let Some(sale) = &mut self.sale else {
            return;
        };
        let mut s = self.session.borrow_mut();
        if let Err(e) = pos::add_item(
            &mut s.fyo,
            sale,
            &item,
            quantity,
            batch.as_deref(),
            &self.qty_map,
        ) {
            self.error = Some(e.to_string());
        }
        cx.notify();
    }

    fn change_quantity(&mut self, idx: usize, by: f64, cx: &mut Context<Self>) {
        self.error = None;
        let Some(sale) = &mut self.sale else {
            return;
        };
        let mut s = self.session.borrow_mut();
        let Some(row) = sale.table("items").get(idx) else {
            return;
        };
        let quantity = match row.get("quantity") {
            Some(DocValue::Number(n)) => *n,
            _ => 0.0,
        } + by;
        let item = match row.get("item") {
            Some(DocValue::Text(item)) => item.clone(),
            _ => String::new(),
        };
        let result = if quantity <= 0.0 {
            sale.remove(&mut s.fyo, "items", idx).map(|_| ())
        } else {
            let tracked = is_truthy(s.fyo.get_value("Item", Some(&item), "trackItem").as_ref());
            let available = self.qty_map.get(&item).map_or(0.0, |q| q.available);
            if by > 0.0 && tracked && quantity > available {
                Err(latte_core::Error::Validation(
                    "Cannot add more than the available quantity".to_owned(),
                ))
            } else {
                sale.set_child(
                    &mut s.fyo,
                    "items",
                    idx,
                    "quantity",
                    DocValue::Number(quantity),
                )
                .map(|_| ())
            }
        };
        if let Err(e) = result {
            self.error = Some(e.to_string());
        }
        cx.notify();
    }

    /// `saveOrder`: saves the sale to take up later, and starts a new one.
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        self.info = None;
        let Some(sale) = &mut self.sale else {
            return;
        };
        let result = {
            let mut s = self.session.borrow_mut();
            pos::save_sale(&mut s.fyo, sale)
        };
        match result {
            Ok(()) => {
                let name = sale.name().unwrap_or_default().to_owned();
                self.info = Some(format!("{} {name} {}", t("Sales Invoice"), t("is Saved")));
                let default = {
                    let s = self.session.borrow();
                    pos::default_customer(&s.fyo)
                };
                self.new_sale(default, window, cx);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    fn show_held(&mut self, submitted: bool, cx: &mut Context<Self>) {
        self.error = None;
        let sales = {
            let s = self.session.borrow();
            pos::held_sales(&s.fyo, submitted)
        };
        match sales {
            Ok(sales) => self.held = Some(HeldPanel { submitted, sales }),
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    /// `selectedInvoiceName`: takes up a saved sale; a submitted one goes on to payment.
    fn take_up(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        self.info = None;
        let sale = {
            let mut s = self.session.borrow_mut();
            s.fyo.get_doc("SalesInvoice", Some(&name))
        };
        match sale {
            Ok(sale) => {
                let party = match sale.get("party") {
                    Some(DocValue::Text(p)) if !p.is_empty() => Some(p.clone().into()),
                    _ => None,
                };
                self.customer
                    .update(cx, |link, cx| link.set_value(party, window, cx));
                let submitted = sale.is_submitted();
                self.sale = Some(sale);
                self.held = None;
                self.payment = None;
                self.batch_for = None;
                if submitted {
                    self.open_payment(window, cx);
                }
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    fn clear_cart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let party = self.customer.read(cx).value().map(|v| v.to_string());
        self.new_sale(party, window, cx);
        cx.notify();
    }

    fn open_payment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        let Some(sale) = &self.sale else {
            return;
        };
        if sale.table("items").is_empty() {
            return;
        }
        let grand_total = money_text(sale.get("grandTotal"));
        let serial_items: Vec<String> = {
            let s = self.session.borrow();
            sale.table("items")
                .iter()
                .filter_map(|row| match row.get("item") {
                    Some(DocValue::Text(item)) => Some(item.clone()),
                    _ => None,
                })
                .filter(|item| {
                    is_truthy(
                        s.fyo
                            .get_value("Item", Some(item), "hasSerialNumber")
                            .as_ref(),
                    )
                })
                .collect()
        };
        let today = jiff::Zoned::now().date().to_string();
        self.payment = Some(PaymentPanel {
            method: "Cash".to_owned(),
            amount: input(grand_total, window, cx),
            reference: input("", window, cx),
            clearance: input(today, window, cx),
            serial_numbers: serial_items
                .into_iter()
                .map(|item| (item, input("", window, cx)))
                .collect(),
        });
        cx.notify();
    }

    fn transact(&mut self, pay: bool, print: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        self.info = None;
        let (Some(sale), Some(panel)) = (&mut self.sale, &self.payment) else {
            return;
        };
        let serial_numbers: HashMap<String, String> = panel
            .serial_numbers
            .iter()
            .map(|(item, state)| (item.clone(), state.read(cx).value().to_string()))
            .filter(|(_, numbers)| !numbers.trim().is_empty())
            .collect();
        let result = {
            let mut s = self.session.borrow_mut();
            let payment = if pay {
                let amount = s
                    .fyo
                    .pesa()
                    .parse(&value(&panel.amount, cx))
                    .map_err(|_| latte_core::Error::Validation(t("Invalid amount")));
                let clearance = value(&panel.clearance, cx);
                let clearance = if clearance.is_empty() {
                    Ok(None)
                } else {
                    format!("{clearance}T00:00:00Z")
                        .parse::<jiff::Timestamp>()
                        .map(Some)
                        .map_err(|_| {
                            latte_core::Error::Validation(format!(
                                "\"{clearance}\" is not a date like 2026-04-01"
                            ))
                        })
                };
                match (amount, clearance) {
                    (Ok(amount), Ok(clearance_date)) => {
                        let reference = value(&panel.reference, cx);
                        Some(Ok(PosPayment {
                            method: panel.method.clone(),
                            amount,
                            reference_id: (!reference.is_empty()).then_some(reference),
                            clearance_date,
                        }))
                    }
                    (Err(e), _) | (_, Err(e)) => Some(Err(e)),
                }
            } else {
                None
            };
            match payment.transpose() {
                Ok(payment) => {
                    pos::create_transaction(&mut s.fyo, sale, &serial_numbers, payment.as_ref())
                }
                Err(e) => Err(e),
            }
        };
        match result {
            Ok(transaction) => {
                let mut done = vec![format!(
                    "{} {} {}",
                    t("Sales Invoice"),
                    transaction.invoice,
                    t("is Submitted")
                )];
                if let Some(shipment) = &transaction.shipment {
                    done.push(format!(
                        "{} {shipment} {}",
                        t("Shipment"),
                        t("is Submitted")
                    ));
                }
                if let Some(payment) = &transaction.payment {
                    done.push(format!("{} {payment} {}", t("Payment"), t("is Saved")));
                }
                self.info = Some(done.join(". "));
                let party = self.customer.read(cx).value().map(|v| v.to_string());
                let default = {
                    let s = self.session.borrow();
                    pos::default_customer(&s.fyo)
                };
                self.new_sale(default.or(party), window, cx);
                if print {
                    cx.emit(Navigate(Nav::Route(Route::Print {
                        schema: "SalesInvoice".to_owned(),
                        name: transaction.invoice,
                    })));
                }
            }
            Err(e) => {
                // A sale that failed part way is shown again from what was saved.
                self.error = Some(e.to_string());
                if let Some(name) = sale.name().map(str::to_owned)
                    && sale.is_submitted()
                {
                    self.payment = None;
                    self.info = Some(format!(
                        "{} {name} {}",
                        t("Sales Invoice"),
                        t("is Submitted")
                    ));
                    let party = self.customer.read(cx).value().map(|v| v.to_string());
                    self.new_sale(party, window, cx);
                }
            }
        }
        cx.notify();
    }

    fn cash_rows(rows: &[Doc], window: &mut Window, cx: &mut Context<Self>) -> Vec<CashRow> {
        rows.iter()
            .map(|row| CashRow {
                denomination: input(money_text(row.get("denomination")), window, cx),
                count: input(
                    match row.get("count") {
                        Some(DocValue::Number(n)) => number_text(*n),
                        _ => "0".to_owned(),
                    },
                    window,
                    cx,
                ),
            })
            .collect()
    }

    fn begin_opening(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shift = {
            let mut s = self.session.borrow_mut();
            pos::opening_shift(&mut s.fyo)
        };
        match shift {
            Ok(shift) => {
                let cash = Self::cash_rows(shift.table("openingCash"), window, cx);
                let amounts = shift
                    .table("openingAmounts")
                    .iter()
                    .filter_map(|row| match row.get("paymentMethod") {
                        Some(DocValue::Text(m)) if m != "Cash" => Some(m.clone()),
                        _ => None,
                    })
                    .map(|method| (method, input("0", window, cx)))
                    .collect();
                self.mode = Mode::Opening(ShiftPanel {
                    opening: Some(shift),
                    cash,
                    amounts,
                    expected: HashMap::new(),
                });
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn begin_closing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        self.info = None;
        let result = {
            let mut s = self.session.borrow_mut();
            pos::opening_shift(&mut s.fyo).and_then(|opening| {
                let closing = pos::new_closing_shift(&mut s.fyo, &opening)?;
                Ok((opening, closing))
            })
        };
        match result {
            Ok((opening, closing)) => {
                let cash = Self::cash_rows(closing.table("closingCash"), window, cx);
                let mut expected = HashMap::new();
                let mut amounts = Vec::new();
                for row in closing.table("closingAmounts") {
                    let Some(DocValue::Text(method)) = row.get("paymentMethod") else {
                        continue;
                    };
                    if let Some(DocValue::Money(money)) = row.get("expectedAmount") {
                        expected.insert(method.clone(), money.clone());
                    }
                    if method != "Cash" {
                        let text = money_text(row.get("expectedAmount"));
                        amounts.push((method.clone(), input(text, window, cx)));
                    }
                }
                self.mode = Mode::Closing(ShiftPanel {
                    opening: Some(opening),
                    cash,
                    amounts,
                    expected,
                });
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    fn add_cash_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let row = CashRow {
            denomination: input("", window, cx),
            count: input("0", window, cx),
        };
        if let Mode::Opening(panel) | Mode::Closing(panel) = &mut self.mode {
            panel.cash.push(row);
        }
        cx.notify();
    }

    /// The cash rows as typed, or what is wrong with them.
    fn read_cash(
        &self,
        panel: &ShiftPanel,
        cx: &App,
    ) -> Result<Vec<DocValueMap>, latte_core::Error> {
        let s = self.session.borrow();
        let mut rows = Vec::new();
        for row in &panel.cash {
            let denomination = value(&row.denomination, cx);
            if denomination.is_empty() {
                continue;
            }
            let denomination = s.fyo.pesa().parse(&denomination).map_err(|_| {
                latte_core::Error::Validation(format!("\"{denomination}\" is not an amount"))
            })?;
            let count = value(&row.count, cx);
            let count = if count.is_empty() {
                0.0
            } else {
                count.parse::<f64>().map_err(|_| {
                    latte_core::Error::Validation(format!("\"{count}\" is not a number"))
                })?
            };
            rows.push(DocValueMap::from([
                ("denomination".to_owned(), DocValue::Money(denomination)),
                ("count".to_owned(), DocValue::Number(count)),
            ]));
        }
        Ok(rows)
    }

    fn read_amounts(
        &self,
        panel: &ShiftPanel,
        cx: &App,
    ) -> Result<Vec<(String, Money)>, latte_core::Error> {
        let s = self.session.borrow();
        panel
            .amounts
            .iter()
            .map(|(method, state)| {
                let text = value(state, cx);
                let text = if text.is_empty() {
                    "0".to_owned()
                } else {
                    text
                };
                s.fyo
                    .pesa()
                    .parse(&text)
                    .map(|money| (method.clone(), money))
                    .map_err(|_| {
                        latte_core::Error::Validation(format!("\"{text}\" is not an amount"))
                    })
            })
            .collect()
    }

    /// Puts the typed cash rows and amounts on a shift.
    fn fill_shift(
        &self,
        shift: &mut Doc,
        cash_table: &str,
        amounts_table: &str,
        amount_field: &str,
        panel: &ShiftPanel,
        cx: &App,
    ) -> Result<(), latte_core::Error> {
        let cash = self.read_cash(panel, cx)?;
        let amounts = self.read_amounts(panel, cx)?;
        let mut s = self.session.borrow_mut();
        let fyo = &mut s.fyo;
        while !shift.table(cash_table).is_empty() {
            shift.remove(fyo, cash_table, 0)?;
        }
        for row in cash {
            shift.append(fyo, cash_table, row)?;
        }
        for (method, amount) in amounts {
            let idx = shift
                .table(amounts_table)
                .iter()
                .position(|row| row.get("paymentMethod") == Some(&DocValue::Text(method.clone())));
            if let Some(idx) = idx {
                shift.set_child(
                    fyo,
                    amounts_table,
                    idx,
                    amount_field,
                    DocValue::Money(amount),
                )?;
            }
        }
        Ok(())
    }

    fn open_shift(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        let Mode::Opening(panel) = &self.mode else {
            return;
        };
        let Some(mut shift) = panel.opening.clone() else {
            return;
        };
        let result = self
            .fill_shift(
                &mut shift,
                "openingCash",
                "openingAmounts",
                "amount",
                panel,
                cx,
            )
            .and_then(|()| {
                let mut s = self.session.borrow_mut();
                pos::open_shift(&mut s.fyo, &mut shift)
            });
        match result {
            Ok(()) => {
                self.mode = Mode::Selling;
                self.new_sale(None, window, cx);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    fn close_shift(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        let Mode::Closing(panel) = &self.mode else {
            return;
        };
        let Some(opening) = panel.opening.clone() else {
            return;
        };
        let result = (|| {
            let mut closing = {
                let mut s = self.session.borrow_mut();
                pos::new_closing_shift(&mut s.fyo, &opening)?
            };
            self.fill_shift(
                &mut closing,
                "closingCash",
                "closingAmounts",
                "closingAmount",
                panel,
                cx,
            )?;
            let mut s = self.session.borrow_mut();
            pos::set_closing_cash_amount(&mut s.fyo, &mut closing)?;
            pos::close_shift(&mut s.fyo, &mut closing, &opening)
        })();
        match result {
            Ok(()) => {
                self.info = Some(t("Shift closed"));
                self.sale = None;
                self.begin_opening(window, cx);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    fn cancel_closing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = Mode::Selling;
        if self.sale.is_none() {
            self.new_sale(None, window, cx);
        }
        cx.notify();
    }

    fn money(&self, value: Option<&DocValue>) -> String {
        let s = self.session.borrow();
        match value {
            Some(DocValue::Money(money)) => s.formatter.currency(money),
            _ => s.formatter.currency(&s.fyo.pesa().zero()),
        }
    }

    fn currency(&self, money: &Money) -> String {
        self.session.borrow().formatter.currency(money)
    }

    /// What the typed cash rows add up to.
    fn counted_cash(&self, panel: &ShiftPanel, cx: &App) -> Option<Money> {
        let rows = self.read_cash(panel, cx).ok()?;
        let s = self.session.borrow();
        let mut doc = s
            .fyo
            .get_new_doc("POSClosingShift", DocValueMap::new())
            .ok()?;
        for row in rows {
            doc.push_row(&s.fyo, "closingCash", row).ok()?;
        }
        cash_amount(&doc, "closingCash", &s.fyo).ok()
    }

    fn render_shift(&self, opening: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let (Mode::Opening(panel) | Mode::Closing(panel)) = &self.mode else {
            return div().into_any_element();
        };
        let counted = self.counted_cash(panel, cx);
        let title = if opening {
            t("Open POS Shift")
        } else {
            t("Close POS Shift")
        };
        let cash_table = div()
            .v_flex()
            .gap_1()
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(div().w(px(160.)).child(t("Denomination")))
                    .child(div().w(px(100.)).child(t("Count"))),
            )
            .children(panel.cash.iter().map(|row| {
                div()
                    .h_flex()
                    .gap_2()
                    .child(
                        div()
                            .w(px(160.))
                            .child(Input::new(&row.denomination).small()),
                    )
                    .child(div().w(px(100.)).child(Input::new(&row.count).small()))
            }))
            .child(
                div().child(
                    Button::new("add-denomination")
                        .small()
                        .ghost()
                        .label(t("Add Row"))
                        .on_click(cx.listener(|this, _, window, cx| this.add_cash_row(window, cx))),
                ),
            );

        let cash_line = {
            let counted_text = counted
                .as_ref()
                .map_or_else(String::new, |m| self.currency(m));
            let mut line = div()
                .h_flex()
                .gap_4()
                .child(div().w(px(120.)).child(t("Cash")))
                .child(div().w(px(140.)).child(counted_text));
            if !opening {
                let expected = panel.expected.get("Cash");
                let difference = match (counted.as_ref(), expected) {
                    (Some(c), Some(e)) => c.sub(e).ok(),
                    _ => None,
                };
                line = line
                    .child(
                        div()
                            .w(px(140.))
                            .child(expected.map(|m| self.currency(m)).unwrap_or_default()),
                    )
                    .child(
                        div()
                            .w(px(140.))
                            .when(difference.as_ref().is_some_and(|d| d.is_negative()), |d| {
                                d.text_color(theme.red)
                            })
                            .child(difference.map(|d| self.currency(&d)).unwrap_or_default()),
                    );
            }
            line
        };
        let amounts = div()
            .v_flex()
            .gap_1()
            .child(
                div()
                    .h_flex()
                    .gap_4()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(div().w(px(120.)).child(t("Payment Method")))
                    .child(div().w(px(140.)).child(if opening {
                        t("Opening Amount")
                    } else {
                        t("Closing Amount")
                    }))
                    .when(!opening, |d| {
                        d.child(div().w(px(140.)).child(t("Expected Amount")))
                            .child(div().w(px(140.)).child(t("Difference Amount")))
                    }),
            )
            .child(cash_line)
            .children(panel.amounts.iter().map(|(method, state)| {
                let mut line = div()
                    .h_flex()
                    .gap_4()
                    .child(div().w(px(120.)).child(method.clone()))
                    .child(div().w(px(140.)).child(Input::new(state).small()));
                if !opening {
                    let expected = panel.expected.get(method);
                    let typed = self
                        .session
                        .borrow()
                        .fyo
                        .pesa()
                        .parse(&value(state, cx))
                        .ok();
                    let difference = match (typed, expected) {
                        (Some(c), Some(e)) => c.sub(e).ok(),
                        _ => None,
                    };
                    line = line
                        .child(
                            div()
                                .w(px(140.))
                                .child(expected.map(|m| self.currency(m)).unwrap_or_default()),
                        )
                        .child(
                            div()
                                .w(px(140.))
                                .child(difference.map(|d| self.currency(&d)).unwrap_or_default()),
                        );
                }
                line
            }));

        let actions = div()
            .h_flex()
            .gap_2()
            .when(!opening, |d| {
                d.child(
                    Button::new("cancel-closing")
                        .small()
                        .ghost()
                        .label(t("Cancel"))
                        .on_click(
                            cx.listener(|this, _, window, cx| this.cancel_closing(window, cx)),
                        ),
                )
            })
            .child(
                Button::new("shift-submit")
                    .small()
                    .primary()
                    .label(if opening {
                        t("Open Shift")
                    } else {
                        t("Close Shift")
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if opening {
                            this.open_shift(window, cx)
                        } else {
                            this.close_shift(window, cx)
                        }
                    })),
            );

        div()
            .id("shift")
            .v_flex()
            .flex_1()
            .gap_4()
            .p_4()
            .overflow_y_scroll()
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(field_label(t("Cash Denominations"), false, cx))
            .child(cash_table)
            .child(field_label(t("Amounts"), false, cx))
            .child(amounts)
            .child(actions)
            .into_any_element()
    }

    fn render_items(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let query = self.search.read(cx).value().to_lowercase();
        let items: Vec<PosItem> = self
            .items
            .iter()
            .filter(|item| query.is_empty() || item.name.to_lowercase().contains(&query))
            .cloned()
            .collect();
        let show_stock = {
            let s = self.session.borrow();
            pos::sells_inventory_items(&s.fyo)
        };
        let cards = items.into_iter().enumerate().map(|(i, item)| {
            let rate = item
                .rate
                .as_ref()
                .map(|r| self.currency(r))
                .unwrap_or_default();
            let available = item.available;
            div()
                .id(("item", i))
                .w(px(150.))
                .p_3()
                .v_flex()
                .gap_1()
                .rounded_md()
                .border_1()
                .border_color(theme.border)
                .cursor_pointer()
                .hover(|d| d.bg(theme.list_hover))
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .truncate()
                        .child(item.name.clone()),
                )
                .child(div().text_sm().child(rate))
                .when(show_stock, |d| {
                    d.child(
                        div()
                            .text_xs()
                            .text_color(if available > 0.0 {
                                theme.muted_foreground
                            } else {
                                theme.red
                            })
                            .child(format!(
                                "{} {}",
                                number_text(available),
                                item.unit.clone().unwrap_or_default()
                            )),
                    )
                })
                .on_click(cx.listener(move |this, _, _, cx| this.add(item.clone(), None, 1.0, cx)))
        });
        let batches = self.batch_for.as_ref().map(|(item, quantity)| {
            let quantity = *quantity;
            let rows: Vec<(String, f64)> = self
                .qty_map
                .get(&item.name)
                .map(|q| q.batches.iter().map(|(b, n)| (b.clone(), *n)).collect())
                .unwrap_or_default();
            div()
                .v_flex()
                .gap_1()
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(theme.border)
                .child(format!("{} {}", t("Select a batch of"), item.name))
                .children(rows.into_iter().enumerate().map(|(i, (batch, qty))| {
                    let item = item.clone();
                    let label = format!("{batch} ({})", number_text(qty));
                    Button::new(("batch", i))
                        .small()
                        .ghost()
                        .label(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.add(item.clone(), Some(batch.clone()), quantity, cx)
                        }))
                }))
        });
        div()
            .v_flex()
            .flex_1()
            .gap_3()
            .p_4()
            .child(Input::new(&self.search).small())
            .children(batches)
            .child(
                div()
                    .id("items")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(div().h_flex().flex_wrap().gap_2().children(cards)),
            )
    }

    fn render_held(&self, panel: &HeldPanel, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let tab = |id: &'static str, label: &str, submitted: bool| {
            let button = Button::new(id).small().label(t(label));
            let button = if panel.submitted == submitted {
                button.primary()
            } else {
                button.ghost()
            };
            button.on_click(cx.listener(move |this, _, _, cx| this.show_held(submitted, cx)))
        };
        let rows = panel.sales.iter().enumerate().map(|(i, row)| {
            let cell = |fieldname: &str| match row.get(fieldname) {
                Some(DocValue::Text(text)) => text.clone(),
                Some(DocValue::Date(date)) => latte_core::fyo::format::utc_date(*date).to_string(),
                value => self.money(value),
            };
            let name = cell("name");
            div()
                .id(("held", i))
                .h_flex()
                .gap_2()
                .p_2()
                .border_b_1()
                .border_color(theme.border)
                .cursor_pointer()
                .hover(|d| d.bg(theme.list_hover))
                .child(div().flex_1().child(name.clone()))
                .child(div().flex_1().child(cell("party")))
                .child(div().w(px(100.)).child(cell("date")))
                .child(
                    div()
                        .w(px(100.))
                        .h_flex()
                        .justify_end()
                        .child(cell("grandTotal")),
                )
                .on_click(
                    cx.listener(move |this, _, window, cx| this.take_up(name.clone(), window, cx)),
                )
        });
        div()
            .v_flex()
            .flex_1()
            .gap_3()
            .p_4()
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .child(tab("held-saved", "Saved", false))
                    .child(tab("held-submitted", "Submitted", true))
                    .child(div().flex_1())
                    .child(
                        Button::new("held-close")
                            .small()
                            .ghost()
                            .label(t("Close"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.held = None;
                                cx.notify();
                            })),
                    ),
            )
            .when(panel.sales.is_empty(), |d| {
                d.child(
                    div()
                        .text_color(theme.muted_foreground)
                        .child(t("No entries found")),
                )
            })
            .child(
                div()
                    .id("held-sales")
                    .flex_1()
                    .overflow_y_scroll()
                    .children(rows),
            )
    }

    fn render_cart(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let Some(sale) = &self.sale else {
            return div();
        };
        let rows = sale.table("items").iter().enumerate().map(|(idx, row)| {
            let item = match row.get("item") {
                Some(DocValue::Text(item)) => item.clone(),
                _ => String::new(),
            };
            let batch = match row.get("batch") {
                Some(DocValue::Text(b)) if !b.is_empty() => format!(" · {b}"),
                _ => String::new(),
            };
            let quantity = match row.get("quantity") {
                Some(DocValue::Number(n)) => number_text(*n),
                _ => String::new(),
            };
            let free = is_truthy(row.get("isFreeItem"));
            div()
                .h_flex()
                .gap_2()
                .py_1()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .flex_1()
                        .v_flex()
                        .child(div().truncate().child(format!("{item}{batch}")))
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(self.money(row.get("rate"))),
                        ),
                )
                .when(!free, |d| {
                    d.child(
                        Button::new(("less", idx))
                            .xsmall()
                            .ghost()
                            .icon(IconName::Minus)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.change_quantity(idx, -1.0, cx)
                            })),
                    )
                })
                .child(div().w(px(36.)).h_flex().justify_center().child(quantity))
                .when(!free, |d| {
                    d.child(
                        Button::new(("more", idx))
                            .xsmall()
                            .ghost()
                            .icon(IconName::Plus)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.change_quantity(idx, 1.0, cx)
                            })),
                    )
                })
                .child(
                    div()
                        .w(px(100.))
                        .h_flex()
                        .justify_end()
                        .child(self.money(row.get("amount"))),
                )
        });

        let (quantity, discounts, taxes) = {
            let s = self.session.borrow();
            (
                pos::total_quantity(sale),
                pos::item_discounts(&s.fyo, sale).ok(),
                pos::total_taxed_amount(&s.fyo, sale).ok(),
            )
        };
        let line = |label: String, value: String| {
            div().h_flex().justify_between().child(label).child(value)
        };
        let totals = div()
            .v_flex()
            .gap_1()
            .pt_2()
            .text_sm()
            .child(line(t("Total Quantity"), number_text(quantity)))
            .child(line(t("Net Total"), self.money(sale.get("netTotal"))))
            .child(line(
                t("Discounts"),
                discounts.map(|d| self.currency(&d)).unwrap_or_default(),
            ))
            .child(line(
                t("Taxes"),
                taxes.map(|d| self.currency(&d)).unwrap_or_default(),
            ))
            .child(
                div()
                    .h_flex()
                    .justify_between()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t("Grand Total"))
                    .child(self.money(sale.get("grandTotal"))),
            );
        let has_items = !sale.table("items").is_empty();
        div()
            .v_flex()
            .w(px(420.))
            .h_full()
            .flex_none()
            .gap_2()
            .p_4()
            .border_l_1()
            .border_color(theme.border)
            .child(field_label(t("Customer"), true, cx))
            .child(self.customer.clone())
            .child(
                div()
                    .id("cart")
                    .v_flex()
                    .flex_1()
                    .overflow_y_scroll()
                    .children(rows),
            )
            .child(totals)
            .children(
                self.payment
                    .as_ref()
                    .map(|panel| self.render_payment(panel, cx)),
            )
            .when(self.payment.is_none(), |d| {
                d.child(
                    div()
                        .h_flex()
                        .gap_2()
                        .child(
                            Button::new("clear")
                                .small()
                                .ghost()
                                .label(t("Cancel"))
                                .disabled(!has_items)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.clear_cart(window, cx)),
                                ),
                        )
                        .child(
                            Button::new("save")
                                .small()
                                .label(t("Save"))
                                .disabled(!has_items || sale.is_submitted())
                                .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                        )
                        .child(
                            Button::new("held")
                                .small()
                                .ghost()
                                .label(t("Held"))
                                .on_click(cx.listener(|this, _, _, cx| this.show_held(false, cx))),
                        )
                        .child(
                            Button::new("pay")
                                .small()
                                .primary()
                                .flex_1()
                                .label(t("Pay"))
                                .disabled(!has_items)
                                .on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.open_payment(window, cx)
                                    }),
                                ),
                        ),
                )
            })
    }

    fn render_payment(&self, panel: &PaymentPanel, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let methods = {
            let s = self.session.borrow();
            pos::payment_methods(&s.fyo).unwrap_or_default()
        };
        let is_cash = {
            let s = self.session.borrow();
            s.fyo
                .get_value("PaymentMethod", Some(&panel.method), "type")
                == Some(DocValue::Text("Cash".to_owned()))
        };
        let change = self.sale.as_ref().and_then(|sale| {
            let s = self.session.borrow();
            let paid = s.fyo.pesa().parse(&value(&panel.amount, cx)).ok()?;
            let Some(DocValue::Money(total)) = sale.get("grandTotal") else {
                return None;
            };
            paid.gt(total).ok()?.then(|| paid.sub(total).ok()).flatten()
        });
        div()
            .v_flex()
            .gap_2()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .child(div().h_flex().flex_wrap().gap_1().children(
                methods.into_iter().enumerate().map(|(i, method)| {
                    let selected = method == panel.method;
                    let button = Button::new(("method", i)).small().label(method.clone());
                    let button = if selected {
                        button.primary()
                    } else {
                        button.ghost()
                    };
                    button.on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(panel) = &mut this.payment {
                            panel.method = method.clone();
                        }
                        cx.notify();
                    }))
                }),
            ))
            .child(field_label(t("Amount Paid"), true, cx))
            .child(Input::new(&panel.amount).small())
            .children(change.map(|c| {
                div()
                    .text_sm()
                    .child(format!("{} {}", t("Change"), self.currency(&c)))
            }))
            .when(!is_cash, |d| {
                d.child(field_label(t("Ref. / Cheque No."), true, cx))
                    .child(Input::new(&panel.reference).small())
                    .child(field_label(t("Clearance Date"), true, cx))
                    .child(Input::new(&panel.clearance).small())
            })
            .children(panel.serial_numbers.iter().map(|(item, state)| {
                div()
                    .v_flex()
                    .gap_1()
                    .child(field_label(
                        format!("{} {item}", t("Serial Numbers of")),
                        true,
                        cx,
                    ))
                    .child(Input::new(state).small())
            }))
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .child(
                        Button::new("back-to-cart")
                            .small()
                            .ghost()
                            .label(t("Cancel"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.payment = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("submit-only")
                            .small()
                            .label(t("Submit"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.transact(false, false, window, cx)
                            })),
                    )
                    .child(
                        Button::new("pay-now")
                            .small()
                            .primary()
                            .label(t("Pay"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.transact(true, false, window, cx)
                            })),
                    )
                    .child(
                        Button::new("pay-print")
                            .small()
                            .label(t("Pay and Print"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.transact(true, true, window, cx)
                            })),
                    ),
            )
    }
}

/// The parties a POS sale can be for: customers, and those that are both.
fn customer_options(session: &Shared) -> Vec<LinkOption> {
    let s = session.borrow();
    let options = GetAllOptions {
        fields: Some(vec!["name".to_owned()]),
        filters: QueryFilter::from([(
            "role".to_owned(),
            Filter::list("in", vec!["Customer".into(), "Both".into()]),
        )]),
        order_by: Some(vec!["name".to_owned()]),
        order: Some(latte_db::Order::Asc),
        ..GetAllOptions::default()
    };
    s.fyo
        .db
        .get_all_docs("Party", &options)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|row| match row.get("name") {
            Some(DocValue::Text(name)) => Some(LinkOption {
                value: name.clone().into(),
                label: name.clone().into(),
            }),
            _ => None,
        })
        .collect()
}

impl Render for PosPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selling = matches!(self.mode, Mode::Selling);
        let header = page_header(t("Point of Sale"), cx).when(selling, |h| {
            h.child(
                Button::new("close-shift")
                    .small()
                    .label(t("Close Shift"))
                    .on_click(cx.listener(|this, _, window, cx| this.begin_closing(window, cx))),
            )
        });
        let body = match &self.mode {
            Mode::Opening(_) => self.render_shift(true, cx),
            Mode::Closing(_) => self.render_shift(false, cx),
            Mode::Selling => div()
                .h_flex()
                .flex_1()
                .min_h_0()
                .child(match &self.held {
                    Some(panel) => self.render_held(panel, cx),
                    None => self.render_items(cx),
                })
                .child(self.render_cart(cx))
                .into_any_element(),
        };
        div()
            .v_flex()
            .size_full()
            .child(header)
            .children(self.error.clone().map(|e| error_message(e, cx)))
            .children(self.info.clone().map(|m| info_message(m, cx)))
            .child(body)
    }
}
