//! The point of sale: `src/pages/POS/` @ v0.36.0, the Classic UI (`ClassicPOS.vue`,
//! `components/POS/Classic/`) with its Open and Close POS Shift, Payment, Saved Invoice
//! and Batch Selection modals.
//!
//! The work is done by `latte_core::pos`; this draws it.
//!
//! Differences from Books:
//! - The item search filters the items shown and adds one on Enter, rather than being a
//!   Link with a dropdown; items show their initials, not their image.
//! - A row's details (the chevron) are shown, not edited, and Payment highlights nothing
//!   for the chosen method, as in Books, but starts at Cash for the grand total.
//! - The amounts of payment methods other than Cash are typed on opening and closing a
//!   shift; Books leaves them read-only.
//! - Not here yet: the Modern UI, item groups, the keyboard shortcuts, returns, coupons,
//!   loyalty points, price lists, item enquiry, and editing a row's rate or discount.
//!   Pricing rules apply as on any invoice.

use std::collections::{HashMap, HashSet};

use crate::books::{self, Text as _, black, gray, white};
use crate::pos_parts::{
    big_button, cell_text, floating_field, hex_colour, initials, modal, modal_card, ratio_cell,
    receipt_icon, shown_field, table_header,
};
use crate::widgets::{Button, Kind, control_frame, toast};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::tooltip::Tooltip;
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
use crate::widgets::{field_label, page_header};
use crate::{Nav, Navigate};

/// The default colours of the POS's buttons (`Defaults.json`).
const GREEN_BUTTON: u32 = 0x86EFAC;
const RED_BUTTON: u32 = 0xF98080;

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
    /// At closing: what each method opened with.
    opened: HashMap<String, Money>,
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
    /// The Saved Invoice modal's search by name.
    held_search: Entity<InputState>,
    /// An item to pick a batch of, how many to add, and the batch picker.
    batch_for: Option<(PosItem, f64, Entity<LinkInput>)>,
    /// `tableView`: the items as a table rather than a grid.
    table_view: bool,
    /// The cart rows whose details are shown.
    expanded: HashSet<usize>,
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
    // No "-0".
    let n = n + 0.0;
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
        let held_search =
            cx.new(|cx| InputState::new(window, cx).placeholder(t("Search by Invoice Name")));
        let subscriptions = vec![
            cx.observe(&search, |_, _, cx| cx.notify()),
            cx.observe(&held_search, |_, _, cx| cx.notify()),
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
            held_search,
            batch_for: None,
            table_view: false,
            expanded: HashSet::new(),
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
        self.expanded.clear();
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
                    self.add(item, None, quantity, window, cx);
                    self.search
                        .update(cx, |input, cx| input.set_value("", window, cx));
                }
            }
            Ok(None) => {}
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    fn add(
        &mut self,
        item: PosItem,
        batch: Option<String>,
        quantity: f64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.error = None;
        self.info = None;
        if item.has_batch && batch.is_none() {
            let options = self
                .qty_map
                .get(&item.name)
                .map(|q| {
                    q.batches
                        .keys()
                        .map(|b| LinkOption {
                            value: b.clone().into(),
                            label: b.clone().into(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            let picker = cx.new(|cx| LinkInput::new(options, t("Batch"), window, cx));
            self._subscriptions
                .push(cx.observe(&picker, |_, _, cx| cx.notify()));
            self.batch_for = Some((item, quantity, picker));
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

    /// `removeAddedItem`: the row's trash icon.
    fn remove_row(&mut self, idx: usize, cx: &mut Context<Self>) {
        self.error = None;
        let Some(sale) = &mut self.sale else {
            return;
        };
        let mut s = self.session.borrow_mut();
        if let Err(e) = sale.remove(&mut s.fyo, "items", idx) {
            self.error = Some(e.to_string());
        }
        self.expanded.clear();
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
                self.expanded.clear();
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
                    opened: HashMap::new(),
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
                let mut opened = HashMap::new();
                let mut amounts = Vec::new();
                for row in closing.table("closingAmounts") {
                    let Some(DocValue::Text(method)) = row.get("paymentMethod") else {
                        continue;
                    };
                    if let Some(DocValue::Money(money)) = row.get("expectedAmount") {
                        expected.insert(method.clone(), money.clone());
                    }
                    if let Some(DocValue::Money(money)) = row.get("openingAmount") {
                        opened.insert(method.clone(), money.clone());
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
                    opened,
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

    /// A big button's colour: the POS profile's (when Books reads it), else Defaults',
    /// else Books' default.
    fn button_colour(&self, fieldname: &str, default: u32, from_profile: bool) -> Hsla {
        let s = self.session.borrow();
        let text = |value: Option<DocValue>| match value {
            Some(DocValue::Text(text)) if !text.is_empty() => Some(text),
            _ => None,
        };
        from_profile
            .then(|| text(s.fyo.get_value("POSSettings", None, "posProfile")))
            .flatten()
            .and_then(|profile| text(s.fyo.get_value("POSProfile", Some(&profile), fieldname)))
            .or_else(|| text(s.fyo.get_value("Defaults", None, fieldname)))
            .and_then(|colour| hex_colour(&colour))
            .unwrap_or_else(|| books::hex(default))
    }

    /// `fyo.currencySymbols[currency]`.
    fn symbol(&self) -> Option<String> {
        let s = self.session.borrow();
        s.formatter
            .currency_symbols
            .get(&s.formatter.currency)
            .cloned()
    }

    /// Money as the floating inputs show it: rounded, without its symbol.
    fn rounded(&self, money: Option<&Money>) -> String {
        match money {
            Some(money) => money.round(None),
            None => self.session.borrow().fyo.pesa().zero().round(None),
        }
    }

    fn discounting(&self) -> bool {
        let s = self.session.borrow();
        latte_core::models::helpers::single_is_truthy(
            &s.fyo,
            "AccountingSettings",
            "enableDiscounting",
        )
    }

    /// The left of `ClassicPOS.vue`: the item search, `ItemsGrid` or `ItemsTable`, and
    /// `POSQuickActions` at the bottom.
    fn render_items(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let query = self.search.read(cx).value().to_lowercase();
        let items: Vec<PosItem> = self
            .items
            .iter()
            .filter(|item| query.is_empty() || item.name.to_lowercase().contains(&query))
            .cloned()
            .collect();
        let list = if self.table_view {
            self.render_items_table(items, cx)
        } else {
            self.render_items_grid(items, window, cx)
        };
        let view_label = if self.table_view {
            t("Grid View")
        } else {
            t("List View")
        };
        let list_label = t("Sales Invoice List");
        let quick_actions = div()
            .absolute()
            .bottom(px(11.))
            .left(px(16.))
            .p_1()
            .h_flex()
            .gap_3()
            .child(
                div()
                    .id("toggle-view")
                    .p(px(6.))
                    .rounded(books::RADIUS_MD)
                    .bg(gray(100))
                    .cursor_pointer()
                    .child(books::icon(
                        if self.table_view { "grid" } else { "list" },
                        px(20.),
                        black(),
                    ))
                    .tooltip(move |window, cx| Tooltip::new(view_label.clone()).build(window, cx))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.table_view = !this.table_view;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("sales-invoice-list")
                    .px(px(6.))
                    .py_1()
                    .rounded(books::RADIUS_MD)
                    .bg(gray(100))
                    .cursor_pointer()
                    .child(receipt_icon())
                    .tooltip(move |window, cx| Tooltip::new(list_label.clone()).build(window, cx))
                    .on_click(cx.listener(|_, _, _, cx| {
                        cx.emit(Navigate(Nav::Route(Route::list(
                            "SalesInvoice",
                            "Sales Invoices",
                            None,
                        ))))
                    })),
            );
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
                    .child(
                        control_frame(false, false).child(
                            Input::new(&self.search)
                                .appearance(false)
                                .h(px(32.))
                                .px_3()
                                .text_13(),
                        ),
                    )
                    .child(list),
            )
            .child(quick_actions)
    }

    /// `ItemsGrid.vue`: a card per item, its initials with the quantity available at the
    /// corner, then its name and rate; one to four a row as the window widens.
    fn render_items_grid(
        &self,
        items: Vec<PosItem>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
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
        let symbol = self.symbol().unwrap_or_default();
        let cards = items.into_iter().enumerate().map(|(i, item)| {
            let rate = item
                .rate
                .as_ref()
                .map(|r| format!("{symbol} {}", r.round(None)))
                .unwrap_or_default();
            let (badge_bg, badge_fg) = if item.available > 0.0 {
                (books::color("green", 100), books::color("green", 900))
            } else {
                (books::color("red", 100), books::color("red", 900))
            };
            let picture = div()
                .relative()
                .size_full()
                .p_2()
                .child(
                    div()
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
                        ),
                )
                .child(
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
                );
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

    /// `ItemsTable.vue`: Item, Rate, Qty and Unit.
    fn render_items_table(&self, items: Vec<PosItem>, cx: &mut Context<Self>) -> Stateful<Div> {
        let columns = [
            (t("Item"), 1., false),
            (t("Rate"), 1., true),
            (t("Qty"), 1., true),
            (t("Unit"), 0.7, false),
        ];
        let rows = items.into_iter().enumerate().map(|(i, item)| {
            let rate = item
                .rate
                .as_ref()
                .map(|r| self.currency(r))
                .unwrap_or_default();
            div()
                .id(("item-row", i))
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
                .child(ratio_cell(1.).child(cell_text(item.name.clone(), false)))
                .child(ratio_cell(1.).child(cell_text(rate, true)))
                .child(ratio_cell(1.).child(cell_text(number_text(item.available), true)))
                .child(
                    ratio_cell(0.7).child(cell_text(item.unit.clone().unwrap_or_default(), false)),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.add(item.clone(), None, 1.0, window, cx)
                }))
        });
        div()
            .id("items")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(table_header(&columns).mt_4())
            .child(div().pb_12().children(rows))
    }

    /// The right of `ClassicPOS.vue`: the customer and `SelectedItemTable` above, the
    /// totals and the big buttons below.
    fn render_cart(&self, cx: &mut Context<Self>) -> Div {
        let sale = self.sale.as_ref();
        let columns = [
            (String::new(), 0.1, false),
            (t("Item"), 0.9, false),
            (t("Quantity"), 0.8, true),
            (t("Stock Unit"), 0.8, false),
            (t("Rate"), 0.8, true),
            (t("Amount"), 0.8, true),
            (String::new(), 0.2, false),
        ];
        let discounting = self.discounting();
        let rows: Vec<_> = sale
            .map(|sale| sale.table("items"))
            .unwrap_or_default()
            .iter()
            .enumerate()
            .map(|(idx, row)| self.render_cart_row(idx, row, discounting, cx))
            .collect();

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
        let totals = div()
            .v_flex()
            .flex_1()
            .min_w_0()
            .justify_end()
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
                .mt_4(),
            );
        let tall = px(48.);
        let buttons = div()
            .v_flex()
            .flex_1()
            .min_w_0()
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .child(
                        big_button(
                            "save",
                            t("Save"),
                            self.button_colour("saveButtonColour", GREEN_BUTTON, true),
                            tall,
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    )
                    .child(
                        big_button(
                            "clear",
                            t("Cancel"),
                            self.button_colour("cancelButtonColour", RED_BUTTON, true),
                            tall,
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.clear_cart(window, cx))),
                    ),
            )
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .mt_4()
                    .child(
                        big_button(
                            "held",
                            t("held"),
                            self.button_colour("heldButtonColour", RED_BUTTON, true),
                            tall,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.show_held(false, cx))),
                    )
                    .child(
                        // `profile?.payButton`, which no profile has.
                        big_button(
                            "pay",
                            t("Pay"),
                            self.button_colour("payButtonColour", GREEN_BUTTON, false),
                            tall,
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.open_payment(window, cx))),
                    ),
            );
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
                card()
                    .flex_none()
                    .p_3()
                    .child(div().h_flex().gap_3().child(totals).child(buttons)),
            )
    }

    /// `SelectedItemRow.vue`: the chevron for the row's details, the item, its quantity
    /// with the chevrons that change it, its unit, rate and amount, and the trash icon.
    fn render_cart_row(
        &self,
        idx: usize,
        row: &Doc,
        discounting: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let text = |fieldname: &str| match row.get(fieldname) {
            Some(DocValue::Text(text)) => text.clone(),
            _ => String::new(),
        };
        let quantity = match row.get("quantity") {
            Some(DocValue::Number(n)) => number_text(*n),
            _ => String::new(),
        };
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
        let line = div()
            .h_flex()
            .w_full()
            .child(
                ratio_cell(0.1).child(
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
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.expanded.remove(&idx) {
                                this.expanded.insert(idx);
                            }
                            cx.notify();
                        })),
                ),
            )
            .child(
                ratio_cell(0.9).child(
                    div()
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
                                    .child(text("pricingRule")),
                            )
                        })
                        .child(small(text("item"), false)),
                ),
            )
            .child(
                ratio_cell(0.8)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(small(quantity.clone(), true)),
                    )
                    .when(!free, |d| {
                        d.child(
                            div()
                                .v_flex()
                                .ml_1()
                                .child(chevron(("more", idx), "chevron-up", 1.0))
                                .child(chevron(("less", idx), "chevron-down", -1.0)),
                        )
                    }),
            )
            .child(ratio_cell(0.8).child(div().ml_5().min_w_0().child(small(text("unit"), false))))
            .child(ratio_cell(0.8).child(small(self.money(row.get("rate")), true)))
            .child(ratio_cell(0.8).child(small(self.money(row.get("amount")), true)))
            .child(ratio_cell(0.2).child(div().px_4().when(!free, |d| {
                d.child(
                    div()
                        .id(("remove", idx))
                        .cursor_pointer()
                        .child(books::icon("trash-2", px(16.), books::color("red", 500)))
                        .on_click(cx.listener(move |this, _, _, cx| this.remove_row(idx, cx))),
                )
            })));
        let details = expanded.then(|| {
            let field = |label: String, value: String| {
                div().flex_1().min_w_0().child(shown_field(label, value))
            };
            let percent = match row.get("itemDiscountPercent") {
                Some(DocValue::Number(n)) => number_text(*n),
                _ => "0".to_owned(),
            };
            let batch = text("batch");
            div()
                .h_flex()
                .w_full()
                .gap_4()
                .px_4()
                .pt_6()
                .pb_2()
                .child(field(t("Quantity"), quantity))
                .child(field(t("Rate"), self.money(row.get("rate"))))
                .when(discounting, |d| {
                    d.child(field(
                        t("Discount Amount"),
                        self.money(row.get("itemDiscountAmount")),
                    ))
                    .child(field(t("Discount Percent"), percent))
                })
                .when(!batch.is_empty(), |d| d.child(field(t("Batch"), batch)))
        });
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
            .children(details)
    }

    /// `OpenPOSShiftModal.vue` and `ClosePOSShiftModal.vue`.
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

        let amounts_table =
            if opening {
                let mut rows = vec![vec![
                    text_cell("Cash".to_owned(), false),
                    text_cell(counted_text, true),
                ]];
                rows.extend(panel.amounts.iter().map(|(method, state)| {
                    vec![text_cell(method.clone(), false), input_cell(state)]
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
                rows.extend(panel.amounts.iter().map(|(method, state)| {
                    let typed = self
                        .session
                        .borrow()
                        .fyo
                        .pesa()
                        .parse(&value(state, cx))
                        .ok();
                    let expected = panel.expected.get(method);
                    vec![
                        text_cell(method.clone(), false),
                        text_cell(money(panel.opened.get(method)), true),
                        input_cell(state),
                        text_cell(money(expected), true),
                        text_cell(money(difference(typed, expected).as_ref()), true),
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
        let (methods, is_cash, zero, taxes, discounts) = {
            let s = self.session.borrow();
            (
                pos::payment_methods(&s.fyo).unwrap_or_default(),
                s.fyo
                    .get_value("PaymentMethod", Some(&panel.method), "type")
                    == Some(DocValue::Text("Cash".to_owned())),
                s.fyo.pesa().zero(),
                pos::total_taxed_amount(&s.fyo, sale).ok(),
                pos::item_discounts(&s.fyo, sale).ok(),
            )
        };
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
        let show_change = !paid_zero && paid.gt(&grand_total).unwrap_or(false);
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
        let serial_numbers = panel.serial_numbers.iter().map(|(item, state)| {
            labelled_input(format!("{} {item}", t("Serial Numbers of")), state)
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
                px(43.),
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
                            .children(serial_numbers)
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

    /// `setPaymentMethodAndAmount`: the method, and the amount still owed.
    fn set_payment_method(&mut self, method: String, window: &mut Window, cx: &mut Context<Self>) {
        let owed = self.sale.as_ref().map(|sale| {
            match (sale.get("outstandingAmount"), sale.get("grandTotal")) {
                (Some(DocValue::Money(m)), _) if !m.is_zero() => m.round(None),
                (_, total) => money_text(total),
            }
        });
        if let Some(panel) = &mut self.payment {
            panel.method = method;
            if let Some(owed) = owed {
                panel
                    .amount
                    .update(cx, |input, cx| input.set_value(owed, window, cx));
            }
        }
        cx.notify();
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
        let columns = [
            (t("Name"), 1., false),
            (t("Customer"), 1., false),
            (t("Date"), 1., false),
            (t("Grand Total"), 0.8, true),
        ];
        let rows: Vec<_> = panel
            .sales
            .iter()
            .filter(|row| {
                query.is_empty()
                    || matches!(row.get("name"), Some(DocValue::Text(n)) if n.to_lowercase().contains(&query))
            })
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
                    .id(("held", i))
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
                        this.take_up(name.clone(), window, cx)
                    }))
            })
            .collect();
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
                .child(div().mt_2().h(px(1.)).bg(gray(200)))
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
                .child(table_header(&columns).mt_2())
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
                                            px(43.),
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
                                            px(43.),
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
}

/// `$router.back()`: the page before the POS.
fn go_back(window: &mut Window, cx: &mut App) {
    let Some(desk) = cx
        .try_global::<crate::desk::NavState>()
        .and_then(|n| n.desk.upgrade())
    else {
        return;
    };
    desk.update(cx, |desk, cx| desk.go_back(window, cx));
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = page_header(t("Point of Sale"), cx).child(
            Button::new("close-shift")
                .label(t("Close POS Shift"))
                .bg(books::color("red", 500))
                .text_color(white())
                .font_weight(FontWeight::MEDIUM)
                .on_click(cx.listener(|this, _, window, cx| {
                    if matches!(this.mode, Mode::Selling) {
                        this.begin_closing(window, cx)
                    }
                })),
        );
        let body = div()
            .h_flex()
            .flex_1()
            .min_h_0()
            .p_4()
            .gap_2()
            .bg(gray(25))
            .child(self.render_items(window, cx))
            .child(self.render_cart(cx));
        let shift = match &self.mode {
            Mode::Opening(_) => Some(self.render_shift_modal(true, window, cx)),
            Mode::Closing(_) => Some(self.render_shift_modal(false, window, cx)),
            Mode::Selling => None,
        };
        let payment = self
            .payment
            .as_ref()
            .and_then(|panel| self.render_payment_modal(panel, window, cx));
        let held = self
            .held
            .as_ref()
            .map(|panel| self.render_held_modal(panel, window, cx));
        let batch = self.render_batch_modal(window, cx);
        let message = match (&self.error, &self.info) {
            (Some(error), _) => Some((error.clone(), Kind::Error)),
            (None, Some(info)) => Some((info.clone(), Kind::Success)),
            (None, None) => None,
        };
        let toast = message.map(|(message, kind)| {
            toast(
                message,
                kind,
                cx.listener(|this, _, _, cx| {
                    this.error = None;
                    this.info = None;
                    cx.notify();
                }),
                window,
            )
        });
        div()
            .v_flex()
            .size_full()
            .bg(white())
            .text_color(black())
            .child(header)
            .child(body)
            .children(shift)
            .children(payment)
            .children(held)
            .children(batch)
            .children(toast)
    }
}
