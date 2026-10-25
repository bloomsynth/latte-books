//! The point of sale: `src/pages/POS/` @ v0.36.0. `POS.vue` with its keyboard shortcuts
//! and the quick quantity (hold Q and type the quantity); the Classic UI
//! (`ClassicPOS.vue`, `components/POS/Classic/`, in pos_classic.rs) or the Modern UI
//! (`ModernPOS.vue`, `components/POS/Modern/`, in pos_modern.rs), as the POS profile or
//! POS Settings choose; `POSQuickActions.vue`; and the modals (pos_modals.rs).
//!
//! The work is done by `latte_core::pos`; this draws it.
//!
//! Differences from Books (docs/DEVIATIONS.md has them all):
//! - Toasts are the page's own message bar, one at a time.
//! - Save and Continue leaves for the Sales Invoice List only once the sale is saved.
//!   Books leaves whether or not it saved, and the error is lost.
//! - The Return modal's Cancel closes it. Books' opens the Saved Invoice modal instead.
//! - Shift+L needs the loyalty program on, as the quick action does. Books asks for price
//!   lists instead.
//! - The applied coupons' count is the sale's. Books shows a blank badge once a coupon is
//!   taken off.

#[path = "pos_classic.rs"]
mod classic;
#[path = "pos_modals.rs"]
mod modals;
#[path = "pos_modern.rs"]
mod modern;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::books::{self, Text as _, black, gray, white};
use crate::pos_link::{PosLink, PosLinkEvent};
use crate::pos_parts::{
    coupon_icon, enquiry_icon, hex_colour, image_from_data_url, loyalty_icon, price_list_icon,
    receipt_icon,
};
use crate::widgets::{Button, Kind, toast};
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use latte_core::Doc;
use latte_core::fyo::model::helpers::is_truthy;
use latte_core::models::inventory::point_of_sale::cash_amount;
use latte_core::pos::{self, ItemQtyMap, KeypadField, Loyalty, PosItem, PosPayment, PosUi};
use latte_core::views::sidebar::Route;
use latte_db::pesa::Money;
use latte_db::{DocValue, DocValueMap, Filter, GetAllOptions, QueryFilter};

use crate::i18n::t;
use crate::link::{LinkInput, LinkOption};
use crate::session::Shared;
use crate::widgets::page_header;
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
    /// The payment methods other than Cash, whose amounts are read only, as in Books.
    others: Vec<String>,
    /// At closing: what each method is expected to have.
    expected: HashMap<String, Money>,
    /// At closing: what each method opened with.
    opened: HashMap<String, Money>,
}

/// `PaymentModal.vue`'s state: no method until one is picked, and nothing paid.
struct PaymentPanel {
    method: Option<String>,
    amount: Entity<InputState>,
    reference: Entity<InputState>,
    clearance: Entity<InputState>,
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

/// ItemEnquiryModal's fields.
struct EnquiryForm {
    item: Entity<PosLink>,
    description: Entity<TextareaState>,
    customer: Entity<PosLink>,
    contact: Entity<InputState>,
    similar: Entity<PosLink>,
}

/// ReturnSalesInvoiceModal's list: the sales, and the page of them shown.
struct ReturnPanel {
    sales: Vec<DocValueMap>,
    page: usize,
    /// Rows a page; `None` is All.
    count: Option<usize>,
}

/// The modals the POS opens over the till, one at a time.
enum Overlay {
    /// `AlertModal.vue`, on leaving for the Sales Invoice List with a cart.
    Alert,
    PriceList(Entity<PosLink>),
    Coupon {
        link: Entity<PosLink>,
        invalid: bool,
    },
    Loyalty {
        points: Entity<InputState>,
        invalid: bool,
    },
    Enquiry(Box<EnquiryForm>),
    Return(ReturnPanel),
    /// `KeyboardModal.vue`: the Modern UI's keypad for a field of a row.
    Keyboard {
        row: usize,
        field: KeypadField,
        value: Entity<InputState>,
    },
}

/// The fields of a Classic row's details (`SelectedItemRow.vue`).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum RowField {
    Quantity,
    TransferQuantity,
    Rate,
    DiscountAmount,
    DiscountPercent,
    TransferUnit,
    Batch,
    SerialNumber,
}

/// The controls of a Classic row's details.
struct RowEditor {
    quantity: Entity<InputState>,
    transfer_quantity: Entity<InputState>,
    rate: Entity<InputState>,
    discount_amount: Entity<InputState>,
    discount_percent: Entity<InputState>,
    transfer_unit: Entity<PosLink>,
    batch: Entity<PosLink>,
    serial_number: Entity<TextareaState>,
    /// What each text control last showed of the row, so that what is typed and not yet
    /// taken isn't overwritten.
    synced: HashMap<RowField, String>,
    _subscriptions: Vec<Subscription>,
}

pub struct PosPage {
    session: Shared,
    focus: FocusHandle,
    focus_pending: bool,
    ui: PosUi,
    mode: Mode,
    sale: Option<Doc>,
    qty_map: ItemQtyMap,
    items: Vec<PosItem>,
    /// The items' pictures, by item.
    images: HashMap<String, Arc<Image>>,
    search: Entity<PosLink>,
    group: Entity<PosLink>,
    item_group: Option<String>,
    customer: Entity<PosLink>,
    /// The customer's loyalty program and points (`loyaltyProgram`, `loyaltyPoints`).
    loyalty: Loyalty,
    payment: Option<PaymentPanel>,
    held: Option<HeldPanel>,
    /// The Saved Invoice modal's search by name.
    held_search: Entity<InputState>,
    /// The Return modal's search by name.
    return_search: Entity<InputState>,
    /// An item to pick a batch of, how many to add, and the batch picker.
    batch_for: Option<(PosItem, f64, Entity<LinkInput>)>,
    overlay: Option<Overlay>,
    /// `tableView`: the items as a table rather than a grid.
    table_view: bool,
    /// The cart rows whose details are shown.
    expanded: HashSet<usize>,
    editors: HashMap<usize, RowEditor>,
    /// `quickQtyRow`: the row last clicked, for the quick quantity.
    selected_row: Option<usize>,
    /// `itemSerialNumbers`: the serial numbers typed for each item.
    serial_numbers: HashMap<String, String>,
    /// While Q is held: the digits typed (`quickQtyBuffer`).
    quick_qty: Option<String>,
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

fn doc_text(doc: &Doc, fieldname: &str) -> String {
    match doc.get(fieldname) {
        Some(DocValue::Text(text)) => text.clone(),
        _ => String::new(),
    }
}

fn doc_number(doc: &Doc, fieldname: &str) -> f64 {
    match doc.get(fieldname) {
        Some(DocValue::Number(n)) => *n,
        _ => 0.0,
    }
}

impl PosPage {
    pub fn new(session: Shared, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (ui, item_names, groups, customers) = {
            let s = session.borrow();
            (
                pos::pos_ui(&s.fyo),
                names(&s.fyo, "Item", QueryFilter::new()),
                pos::item_groups(&s.fyo).unwrap_or_default(),
                customer_names(&s.fyo),
            )
        };
        let search = cx.new(|cx| {
            PosLink::new(item_names, t("Search Item (Name or Barcode)"), window, cx).clearable()
        });
        let group = cx.new(|cx| PosLink::new(groups, t("Filter by Group"), window, cx).clearable());
        let customer = cx.new(|cx| PosLink::new(customers, t("Customer"), window, cx).clearable());
        let held_search =
            cx.new(|cx| InputState::new(window, cx).placeholder(t("Search by Invoice Name")));
        let return_search =
            cx.new(|cx| InputState::new(window, cx).placeholder(t("Search by Invoice Name")));
        let subscriptions =
            vec![
                // A field that goes away with its modal leaves nothing focused; the till takes
                // the focus back, so that its shortcuts keep working.
                cx.on_focus_lost(window, |this: &mut Self, window, cx| {
                    this.focus.focus(window, cx)
                }),
                cx.observe(&held_search, |_, _, cx| cx.notify()),
                cx.observe(&return_search, |this, _, cx| {
                    if let Some(Overlay::Return(panel)) = &mut this.overlay {
                        panel.page = 1;
                    }
                    cx.notify()
                }),
                cx.subscribe_in(
                    &return_search,
                    window,
                    |this, _, event: &InputEvent, window, cx| {
                        if matches!(event, InputEvent::PressEnter { .. }) {
                            this.return_search_entered(window, cx);
                        }
                    },
                ),
                cx.subscribe_in(
                    &search,
                    window,
                    |this, _, event: &PosLinkEvent, window, cx| match event {
                        PosLinkEvent::Entered(text) => this.search_entered(text, window, cx),
                        PosLinkEvent::Opened(item) => this.open_form("Item", item, cx),
                        PosLinkEvent::Picked(_) => cx.notify(),
                    },
                ),
                cx.subscribe_in(
                    &group,
                    window,
                    |this, _, event: &PosLinkEvent, _, cx| match event {
                        PosLinkEvent::Picked(group) => {
                            this.item_group = (!group.is_empty()).then(|| group.to_string());
                            this.refresh_items();
                            cx.notify();
                        }
                        PosLinkEvent::Opened(group) => this.open_form("ItemGroup", group, cx),
                        PosLinkEvent::Entered(_) => {}
                    },
                ),
                cx.subscribe_in(&customer, window, |this, _, event: &PosLinkEvent, _, cx| {
                    match event {
                        PosLinkEvent::Picked(party) => this.set_customer(party.to_string(), cx),
                        PosLinkEvent::Opened(party) => this.open_form("Party", party, cx),
                        PosLinkEvent::Entered(_) => {}
                    }
                }),
            ];
        let mut page = Self {
            session,
            focus: cx.focus_handle(),
            focus_pending: true,
            ui,
            mode: Mode::Selling,
            sale: None,
            qty_map: ItemQtyMap::new(),
            items: Vec::new(),
            images: HashMap::new(),
            search,
            group,
            item_group: None,
            customer,
            loyalty: Loyalty::default(),
            payment: None,
            held: None,
            held_search,
            return_search,
            batch_for: None,
            overlay: None,
            table_view: true,
            expanded: HashSet::new(),
            editors: HashMap::new(),
            selected_row: None,
            serial_numbers: HashMap::new(),
            quick_qty: None,
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

    fn accounting(&self, fieldname: &str) -> bool {
        let s = self.session.borrow();
        latte_core::models::helpers::single_is_truthy(&s.fyo, "AccountingSettings", fieldname)
    }

    fn open_form(&self, schema: &str, name: &str, cx: &mut Context<Self>) {
        cx.emit(Navigate(Nav::Route(Route::Form {
            schema: schema.to_owned(),
            name: Some(name.to_owned()),
        })));
    }

    /// `setItems`, with the items' pictures.
    fn refresh_items(&mut self) {
        let s = self.session.borrow();
        match pos::item_qty_map(&s.fyo).and_then(|map| {
            let items = pos::items(&s.fyo, &map, self.item_group.as_deref())?;
            Ok((map, items))
        }) {
            Ok((map, items)) => {
                self.images
                    .retain(|name, _| items.iter().any(|i| &i.name == name));
                for item in &items {
                    if let Some(url) = &item.image
                        && !self.images.contains_key(&item.name)
                        && let Some(image) = image_from_data_url(url)
                    {
                        self.images.insert(item.name.clone(), Arc::new(image));
                    }
                }
                self.qty_map = map;
                self.items = items;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn clear_rows(&mut self) {
        self.expanded.clear();
        self.editors.clear();
        self.selected_row = None;
    }

    fn new_sale(&mut self, party: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_items();
        let sale = {
            let s = self.session.borrow();
            pos::new_sale(&s.fyo, party.as_deref())
        };
        match sale {
            Ok(sale) => {
                let party = doc_text(&sale, "party");
                self.show_customer(&party, window, cx);
                self.sale = Some(sale);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        self.payment = None;
        self.held = None;
        self.batch_for = None;
        self.serial_numbers.clear();
        self.clear_rows();
    }

    /// Shows a sale's customer, and reads their loyalty points.
    fn show_customer(&mut self, party: &str, window: &mut Window, cx: &mut Context<Self>) {
        let value = (!party.is_empty()).then(|| SharedString::from(party.to_owned()));
        self.customer
            .update(cx, |link, cx| link.set_value(value, window, cx));
        let s = self.session.borrow();
        self.loyalty = pos::customer_loyalty(&s.fyo, party);
    }

    /// `setCustomer`.
    fn set_customer(&mut self, party: String, cx: &mut Context<Self>) {
        let Some(sale) = &mut self.sale else {
            return;
        };
        let mut s = self.session.borrow_mut();
        let value = if party.is_empty() {
            DocValue::Null
        } else {
            DocValue::Text(party.clone())
        };
        if let Err(e) = sale.set(&mut s.fyo, "party", value) {
            self.error = Some(e.to_string());
        }
        self.loyalty = pos::customer_loyalty(&s.fyo, &party);
        cx.notify();
    }

    /// `handleItemSearch` on Enter: adds the item a name or barcode finds.
    fn search_entered(&mut self, search: &str, window: &mut Window, cx: &mut Context<Self>) {
        let search = search.trim().to_owned();
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
                        .update(cx, |link, cx| link.set_value(None, window, cx));
                }
            }
            Ok(None) => {}
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    /// `addItem`, and `handleBatchSelected` with a batch.
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
        // `validateInvoice` comes before the batch.
        if let Some(sale) = &self.sale {
            if sale.is_submitted() {
                self.error = Some(t("Cannot add an item to a submitted invoice."));
                cx.notify();
                return;
            }
            if pos::is_return_sale(sale) {
                self.error = Some(t("Unable to add an item to the return invoice."));
                cx.notify();
                return;
            }
        }
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
        let result = pos::add_item(
            &mut s.fyo,
            sale,
            &item,
            quantity,
            batch.as_deref(),
            &self.qty_map,
        );
        let batched = match result {
            // A row with a batch shows its details (`row.batch`'s watcher).
            Ok(()) => batch.and_then(|batch| {
                sale.table("items")
                    .iter()
                    .position(|row| doc_text(row, "batch") == batch)
            }),
            Err(e) => {
                self.error = Some(e.to_string());
                None
            }
        };
        drop(s);
        if let Some(idx) = batched {
            self.expand(idx, window, cx);
        }
        cx.notify();
    }

    /// Runs a change to the sale, showing what went wrong.
    fn change_sale(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(
            &mut latte_core::Fyo,
            &mut Doc,
            &ItemQtyMap,
        ) -> Result<(), latte_core::Error>,
    ) -> bool {
        self.error = None;
        let Some(sale) = &mut self.sale else {
            return false;
        };
        let mut s = self.session.borrow_mut();
        let result = change(&mut s.fyo, sale, &self.qty_map);
        cx.notify();
        match result {
            Ok(()) => true,
            Err(e) => {
                self.error = Some(e.to_string());
                false
            }
        }
    }

    /// `adjustQuantity`: the chevrons by a row's quantity.
    fn change_quantity(&mut self, idx: usize, by: f64, cx: &mut Context<Self>) {
        self.selected_row = Some(idx);
        self.change_sale(cx, |fyo, sale, qty_map| {
            pos::adjust_row_quantity(fyo, sale, idx, by, qty_map)
        });
    }

    /// `removeAddedItem`: the row's trash icon.
    fn remove_row(&mut self, idx: usize, cx: &mut Context<Self>) {
        self.change_sale(cx, |fyo, sale, _| {
            sale.remove(fyo, "items", idx)?;
            sale.run_formulas(fyo)?;
            Ok(())
        });
        self.clear_rows();
    }

    fn toggle_row(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        if !self.expanded.remove(&idx) {
            self.expand(idx, window, cx);
        }
        cx.notify();
    }

    /// `saveInvoiceAction` and `saveOrder`: saves the sale to take up later, and starts a
    /// new one. Whether it saved.
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.error = None;
        self.info = None;
        let Some(sale) = &mut self.sale else {
            return false;
        };
        if let Some(missing) = pos::missing_before(sale, "saving") {
            self.error = Some(missing);
            cx.notify();
            return false;
        }
        let result = {
            let mut s = self.session.borrow_mut();
            pos::save_sale(&mut s.fyo, sale)
        };
        let saved = match result {
            Ok(()) => {
                let name = sale.name().unwrap_or_default().to_owned();
                self.info = Some(format!("{} {name} {}", t("Sales Invoice"), t("is Saved")));
                let party = self.next_party(cx);
                self.new_sale(party, window, cx);
                true
            }
            Err(e) => {
                self.error = Some(e.to_string());
                false
            }
        };
        cx.notify();
        saved
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

    /// Shows a sale taken up from elsewhere: a saved sale, or a return.
    fn take_sale(&mut self, sale: Doc, window: &mut Window, cx: &mut Context<Self>) {
        let party = doc_text(&sale, "party");
        self.show_customer(&party, window, cx);
        self.clear_rows();
        let batched: Vec<usize> = sale
            .table("items")
            .iter()
            .enumerate()
            .filter(|(_, row)| !doc_text(row, "batch").is_empty())
            .map(|(idx, _)| idx)
            .collect();
        self.sale = Some(sale);
        for idx in batched {
            self.expand(idx, window, cx);
        }
        self.held = None;
        self.payment = None;
        self.batch_for = None;
        self.overlay = None;
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
                let submitted = sale.is_submitted();
                self.take_sale(sale, window, cx);
                if submitted {
                    self.open_payment(window, cx);
                }
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    /// The customer of the next sale, as `clearValues` leaves it: the one there was, or the
    /// default customer, when there is a default customer; none otherwise.
    fn next_party(&self, cx: &App) -> Option<String> {
        let default = {
            let s = self.session.borrow();
            pos::default_customer(&s.fyo)
        };
        default.map(|default| {
            self.customer
                .read(cx)
                .value()
                .map_or(default, |v| v.to_string())
        })
    }

    /// `clearValues`: the Cancel button and Ctrl+Shift+Backspace.
    fn clear_cart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let party = self.next_party(cx);
        self.new_sale(party, window, cx);
        cx.notify();
    }

    /// `handlePaymentAction`: the Payment modal, once the sale has items and a customer.
    fn open_payment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        let Some(sale) = &self.sale else {
            return;
        };
        if let Some(missing) = pos::missing_before(sale, "payment") {
            self.error = Some(missing);
            cx.notify();
            return;
        }
        let zero = self.session.borrow().fyo.pesa().zero().round(None);
        self.payment = Some(PaymentPanel {
            method: None,
            amount: input(zero, window, cx),
            reference: input("", window, cx),
            clearance: input("", window, cx),
        });
        cx.notify();
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
            panel.method = Some(method);
            if let Some(owed) = owed {
                panel
                    .amount
                    .update(cx, |input, cx| input.set_value(owed, window, cx));
            }
        }
        cx.notify();
    }

    /// `createTransaction`: Submit, Pay, and Pay & Print.
    fn transact(&mut self, pay: bool, print: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        self.info = None;
        let (Some(sale), Some(panel)) = (&mut self.sale, &self.payment) else {
            return;
        };
        let Some(method) = panel.method.clone() else {
            self.error = Some(if pay {
                t("Please select a payment method before proceeding with payment.")
            } else {
                t("Please select a payment method before submitting.")
            });
            cx.notify();
            return;
        };
        let serial_numbers = self.serial_numbers.clone();
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
                            method,
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
                let party = self.next_party(cx);
                self.new_sale(party, window, cx);
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
                    let party = self.next_party(cx);
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
                let others = shift
                    .table("openingAmounts")
                    .iter()
                    .map(|row| doc_text(row, "paymentMethod"))
                    .filter(|m| !m.is_empty() && m != "Cash")
                    .collect();
                self.mode = Mode::Opening(ShiftPanel {
                    opening: Some(shift),
                    cash,
                    others,
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
                let mut others = Vec::new();
                for row in closing.table("closingAmounts") {
                    let method = doc_text(row, "paymentMethod");
                    if method.is_empty() {
                        continue;
                    }
                    if let Some(DocValue::Money(money)) = row.get("expectedAmount") {
                        expected.insert(method.clone(), money.clone());
                    }
                    if let Some(DocValue::Money(money)) = row.get("openingAmount") {
                        opened.insert(method.clone(), money.clone());
                    }
                    if method != "Cash" {
                        others.push(method);
                    }
                }
                self.mode = Mode::Closing(ShiftPanel {
                    opening: Some(opening),
                    cash,
                    others,
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

    /// Puts the typed cash rows on a shift.
    fn fill_cash(
        &self,
        shift: &mut Doc,
        cash_table: &str,
        panel: &ShiftPanel,
        cx: &App,
    ) -> Result<(), latte_core::Error> {
        let cash = self.read_cash(panel, cx)?;
        let mut s = self.session.borrow_mut();
        let fyo = &mut s.fyo;
        while !shift.table(cash_table).is_empty() {
            shift.remove(fyo, cash_table, 0)?;
        }
        for row in cash {
            shift.append(fyo, cash_table, row)?;
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
            .fill_cash(&mut shift, "openingCash", panel, cx)
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
            self.fill_cash(&mut closing, "closingCash", panel, cx)?;
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
        self.accounting("enableDiscounting")
    }

    /// Whether a modal is open over the till (`hasAnyOpenModal`).
    fn has_open_modal(&self) -> bool {
        self.overlay.is_some()
            || self.payment.is_some()
            || self.held.is_some()
            || self.batch_for.is_some()
            || !matches!(self.mode, Mode::Selling)
    }

    /// `isModalOpen`: closes the first modal open, in Books' order, if one is.
    fn close_a_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let closed = if matches!(self.overlay, Some(Overlay::Keyboard { .. })) {
            self.overlay = None;
            true
        } else if self.payment.is_some() {
            self.payment = None;
            true
        } else if matches!(self.mode, Mode::Closing(_)) {
            self.cancel_closing(window, cx);
            true
        } else if self.held.is_some() && !matches!(self.overlay, Some(Overlay::Loyalty { .. })) {
            self.held = None;
            true
        } else if self.overlay.is_some() {
            self.overlay = None;
            true
        } else if self.batch_for.is_some() {
            self.batch_for = None;
            true
        } else {
            false
        };
        cx.notify();
        closed
    }

    /// The till's keyboard shortcuts (`setShortcuts`), and the quick quantity.
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let m = &k.modifiers;
        let pmod = m.control || m.platform;
        // As Books' `useKeys`: shortcuts without Ctrl, ⌘ or Alt wait while typing.
        let typing = !self.focus.is_focused(window);
        let key = k.key.as_str();
        // Tab leaves a field, which takes what was typed, as in a browser.
        if key == "tab" && typing {
            self.focus.focus(window, cx);
            return;
        }
        if pmod && m.shift && !m.alt {
            match key {
                "h" => {
                    if self.held.take().is_none() {
                        self.show_held(false, cx);
                    }
                }
                "backspace" => {
                    if !self.close_a_modal(window, cx) {
                        self.clear_cart(window, cx);
                    }
                }
                "p" => {
                    if self.sale.as_ref().is_some_and(pos::has_items_and_customer)
                        && self.payment.take().is_none()
                    {
                        self.open_payment(window, cx);
                    }
                }
                "s" => {
                    if !self.close_a_modal(window, cx)
                        && self.sale.as_ref().is_some_and(pos::has_items_and_customer)
                    {
                        self.save(window, cx);
                    }
                }
                _ => return,
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if typing || pmod || m.alt {
            return;
        }
        if m.shift {
            let ready = self.sale.as_ref().is_some_and(pos::has_items_and_customer);
            match key {
                "s" => self.route_to_list(cx),
                "v" => self.table_view = !self.table_view,
                "p" => self.toggle_overlay_kind(OverlayKind::PriceList, window, cx),
                "l" => {
                    if self.accounting("enableLoyaltyProgram")
                        && self.loyalty.points != 0.0
                        && ready
                    {
                        self.open_loyalty(window, cx);
                    }
                }
                "c" => {
                    if self.accounting("enableCouponCode") && ready {
                        self.toggle_overlay_kind(OverlayKind::Coupon, window, cx);
                    }
                }
                _ => return,
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // Hold Q and type the quantity.
        if self.has_open_modal() {
            return;
        }
        match (key, &mut self.quick_qty) {
            ("q", None) => self.quick_qty = Some(String::new()),
            (digit, Some(buffer)) if digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit() => {
                buffer.push_str(digit)
            }
            ("backspace", Some(buffer)) => {
                buffer.pop();
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    /// `onQuickQtyKeyUp`: letting go of Q sets the quantity typed.
    fn on_key_up(&mut self, event: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key != "q" {
            return;
        }
        let Some(buffer) = self.quick_qty.take() else {
            return;
        };
        let Ok(quantity) = buffer.parse::<f64>() else {
            return;
        };
        let selected = self.selected_row;
        self.change_sale(cx, |fyo, sale, qty_map| {
            pos::quick_quantity(fyo, sale, selected, quantity, qty_map)
        });
    }

    /// `routeToSinvList`: the Sales Invoice List, or the Alert modal first with a cart.
    fn route_to_list(&mut self, cx: &mut Context<Self>) {
        if self
            .sale
            .as_ref()
            .is_some_and(|sale| !sale.table("items").is_empty())
        {
            self.overlay = Some(Overlay::Alert);
            cx.notify();
            return;
        }
        Self::go_to_list(cx);
    }

    fn go_to_list(cx: &mut Context<Self>) {
        cx.emit(Navigate(Nav::Route(Route::list(
            "SalesInvoice",
            "Sales Invoices",
            None,
        ))));
    }

    /// `handleSaveAndContinue`.
    fn save_and_continue(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .sale
            .as_ref()
            .is_none_or(|sale| doc_text(sale, "party").is_empty())
        {
            self.error = Some(t("Please add a customer before saving"));
            cx.notify();
            return;
        }
        if self.save(window, cx) {
            self.overlay = None;
            Self::go_to_list(cx);
        }
    }

    /// `POSQuickActions`' checks before the coupon and loyalty modals.
    fn missing_for(&self, action: &str, loyalty: bool) -> Option<String> {
        let sale = self.sale.as_ref()?;
        pos::missing_before(sale, action).or_else(|| {
            (loyalty && self.loyalty.points == 0.0)
                .then(|| format!("Customer has no loyalty points to redeem before {action}"))
        })
    }

    fn quick_action(&mut self, kind: OverlayKind, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        let missing = match kind {
            OverlayKind::Coupon => self.missing_for("applying coupon", false),
            OverlayKind::Loyalty => self.missing_for("applying loyalty points", true),
            _ => None,
        };
        if let Some(missing) = missing {
            self.error = Some(missing);
            cx.notify();
            return;
        }
        if kind == OverlayKind::Loyalty {
            self.open_loyalty(window, cx);
        } else {
            self.toggle_overlay_kind(kind, window, cx);
        }
    }

    /// The quick actions at the bottom of the items (`POSQuickActions.vue`).
    fn render_quick_actions(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let view_label = if self.table_view {
            t("Grid View")
        } else {
            t("List View")
        };
        let action = |id: &'static str, label: String, icon: AnyElement| {
            div()
                .id(id)
                .relative()
                .rounded(books::RADIUS_MD)
                .bg(gray(100))
                .cursor_pointer()
                .child(icon)
                .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
        };
        let coupons = self
            .sale
            .as_ref()
            .map_or(0, |sale| sale.table("coupons").len());
        div()
            .id("quick-actions")
            .occlude()
            .h_flex()
            .gap_3()
            .p_1()
            .child(
                action(
                    "toggle-view",
                    view_label,
                    books::icon(
                        if self.table_view { "grid" } else { "list" },
                        px(20.),
                        black(),
                    )
                    .into_any_element(),
                )
                .p(px(6.))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.table_view = !this.table_view;
                    cx.notify();
                })),
            )
            .child(
                action(
                    "sales-invoice-list",
                    t("Sales Invoice List"),
                    receipt_icon().into_any_element(),
                )
                .px(px(6.))
                .py_1()
                .on_click(cx.listener(|this, _, _, cx| this.route_to_list(cx))),
            )
            .when(self.accounting("enableLoyaltyProgram"), |d| {
                d.child(
                    action(
                        "loyalty-program",
                        t("Loyalty Program"),
                        loyalty_icon().into_any_element(),
                    )
                    .p_1()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.quick_action(OverlayKind::Loyalty, window, cx)
                    })),
                )
            })
            .when(self.accounting("enableCouponCode"), |d| {
                d.child(
                    action(
                        "coupon-code",
                        t("Coupon Code"),
                        coupon_icon().into_any_element(),
                    )
                    .p(px(2.))
                    .when(coupons > 0, |d| {
                        d.child(
                            div()
                                .absolute()
                                .top(px(-8.))
                                .right(px(-8.))
                                .size_4()
                                .p_2()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .bg(books::color("green", 400))
                                .text_color(books::color("green", 900))
                                .text_11()
                                .child(coupons.to_string()),
                        )
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.quick_action(OverlayKind::Coupon, window, cx)
                    })),
                )
            })
            .when(self.accounting("enablePriceList"), |d| {
                d.child(
                    action(
                        "price-list",
                        t("Price List"),
                        price_list_icon().into_any_element(),
                    )
                    .p_1()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.quick_action(OverlayKind::PriceList, window, cx)
                    })),
                )
            })
            .when(self.accounting("enableItemEnquiry"), |d| {
                d.child(
                    action(
                        "item-enquiry",
                        t("Item Enquiry"),
                        enquiry_icon().into_any_element(),
                    )
                    .p_1()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.quick_action(OverlayKind::Enquiry, window, cx)
                    })),
                )
            })
    }

    /// The item search and, with item groups on, Filter by Group.
    fn render_search(&self) -> Div {
        div()
            .h_flex()
            .gap_x_2()
            .child(div().flex_1().min_w_0().child(self.search.clone()))
            .when(self.accounting("enableitemGroup"), |d| {
                d.child(div().flex_1().min_w_0().child(self.group.clone()))
            })
    }
}

/// Which modal: for the shortcuts that toggle one.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OverlayKind {
    PriceList,
    Coupon,
    Loyalty,
    Enquiry,
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

/// The names of a schema's documents, by name.
fn names(fyo: &latte_core::Fyo, schema_name: &str, filters: QueryFilter) -> Vec<String> {
    let options = GetAllOptions {
        fields: Some(vec!["name".to_owned()]),
        filters,
        order_by: Some(vec!["name".to_owned()]),
        order: Some(latte_db::Order::Asc),
        ..GetAllOptions::default()
    };
    fyo.db
        .get_all_docs(schema_name, &options)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|row| match row.get("name") {
            Some(DocValue::Text(name)) => Some(name.clone()),
            _ => None,
        })
        .collect()
}

/// The parties a POS sale can be for: customers, and those that are both.
fn customer_names(fyo: &latte_core::Fyo) -> Vec<String> {
    names(
        fyo,
        "Party",
        QueryFilter::from([(
            "role".to_owned(),
            Filter::list("in", vec!["Customer".into(), "Both".into()]),
        )]),
    )
}

impl Render for PosPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.focus_pending {
            self.focus_pending = false;
            self.focus.focus(window, cx);
        }
        self.sync_editors(window, cx);
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
        let body = match self.ui {
            PosUi::Classic => self.render_classic(window, cx),
            PosUi::Modern => self.render_modern(window, cx),
        };
        let modals = self.render_modals(window, cx);
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
            .id("pos")
            .v_flex()
            .size_full()
            .bg(white())
            .text_color(black())
            .track_focus(&self.focus)
            // A click away from a field leaves it, as in a browser, so that the shortcuts
            // without Ctrl work again; a field clicked takes the focus back.
            .capture_any_mouse_down(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                this.focus.focus(window, cx);
            }))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_key_up(cx.listener(Self::on_key_up))
            .child(header)
            .child(body)
            .children(modals)
            .children(toast)
    }
}
