// Port of models/inventory/StockTransfer.ts @ v0.36.0, the model of Shipment and
// PurchaseReceipt: the stock moves of Transfer, and a posting to the stock accounts.
//
// Books bugs Latte fixes:
// - The invoice discount in the grand total follows the invoice's own discount settings.
//   Books reads `setDiscountAmount`, `discountAmount` and `discountAfterTax` from the
//   shipment or receipt, which has none of them, so it always takes the percent of the
//   discounted rows.
// - A row's discount percent after tax is taken on the row's amount with its taxes, as on
//   its invoice row. Books reads the taxed total from the shipment's row, which has none,
//   so the discount is lost.
// - A shipment's cost of goods is worked out whatever stock its items have (see
//   reports/stock.rs).
// - A return posts the size of its amount on the swapped sides, so it reverses what was
//   posted. Books posts its negative amount there, which posts it again.
// - Returns, as for invoices: what is left to return counts only submitted returns, an
//   item on two rows is returned once, a return can't take back more than is left, and a
//   return doesn't copy whether its shipment or receipt was returned. A return also
//   leaves out the invoice its shipment or receipt was made from: Books copies it, so the
//   return's grand total adds the invoice's taxes to its negative amounts, and submitting
//   it adds to the invoice's stock not shipped or received.
// - Each row of a return takes back only its serial numbers that aren't returned yet.
//
// Not ported yet, as the UI needs them: `createFilters` and `addItem` (the barcode
// scanner).

use indexmap::IndexMap;
use jiff::Timestamp;
use latte_db::pesa::Money;
use latte_db::{DocValue, DocValueMap, Filter, GetAllOptions, QueryFilter};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::helpers::is_truthy;
use crate::fyo::model::types::{DocContext, Formula, Model};
use crate::models::base_models::invoice::{
    self, get_return_qty_total, map_number, map_text, posting_amount, submitted_returns,
    table_rows, validate_return_quantities,
};
use crate::models::helpers::{
    get_number_series, money, money_or_zero, number, one_of, query, single_is_truthy, single_text,
    text,
};
use crate::models::inventory::helpers::{
    can_validate_serial_number, get_serial_number_from_doc, get_serial_numbers,
    serial_number_status, update_serial_numbers, validate_batch, validate_serial_number,
};
use crate::models::inventory::stock_manager::TransferDetails;
use crate::models::inventory::transfer;
use crate::models::transactional::ledger_posting::LedgerPosting;
use crate::models::transactional::{self, Transactional};
use crate::reports::stock::get_shipment_cogs_amount;

pub struct StockTransfer;

/// `isSales`.
pub(crate) fn is_sales(doc: &Doc) -> bool {
    doc.schema_name() == "Shipment"
}

/// `isReturn`.
fn is_return(doc: &Doc) -> bool {
    is_truthy(doc.get("returnAgainst"))
}

/// `invoiceSchemaName`.
fn invoice_schema_name(doc: &Doc) -> &'static str {
    if is_sales(doc) {
        "SalesInvoice"
    } else {
        "PurchaseInvoice"
    }
}

fn non_empty(row: &Doc, fieldname: &str) -> Option<String> {
    text(row, fieldname)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

/// `_getTransferDetails`.
fn transfer_details(doc: &Doc) -> Vec<TransferDetails> {
    let sales = is_sales(doc);
    doc.table("items")
        .iter()
        .map(|row| {
            let location = non_empty(row, "location");
            TransferDetails {
                item: text(row, "item").unwrap_or_default().to_owned(),
                rate: money(row, "rate"),
                quantity: number(row, "quantity").unwrap_or(0.0),
                batch: non_empty(row, "batch"),
                serial_number: non_empty(row, "serialNumber"),
                from_location: if sales { location.clone() } else { None },
                to_location: if sales { None } else { location },
                is_return: is_return(doc),
            }
        })
        .collect()
}

/// `getItemDiscountAmount`: the discounts of the rows, with their invoice's rows' taxed
/// totals where the discount is after tax.
fn get_item_discount_amount(doc: &Doc, invoice: &Doc, fyo: &Fyo) -> Result<Money, Error> {
    let mut discount = fyo.pesa().zero();
    let after_tax = is_truthy(invoice.get("discountAfterTax"));
    for row in doc.table("items") {
        let amount = money(row, "itemDiscountAmount");
        let percent = number(row, "itemDiscountPercent").unwrap_or(0.0) / 100.0;
        if let Some(amount) = amount.filter(|a| !a.is_zero()) {
            discount = discount.add(&amount)?;
        } else if !after_tax {
            discount = discount.add(&money_or_zero(row, "amount", fyo).mul(percent)?)?;
        } else {
            discount = discount.add(&row_taxed_total(row, invoice, fyo)?.mul(percent)?)?;
        }
    }
    Ok(discount)
}

/// The taxed total of a row, from its invoice row in proportion to its quantity.
fn row_taxed_total(row: &Doc, invoice: &Doc, fyo: &Fyo) -> Result<Money, Error> {
    let Some(invoice_row) = invoice
        .table("items")
        .iter()
        .find(|i| i.get("item") == row.get("item"))
    else {
        return Ok(fyo.pesa().zero());
    };
    let invoice_quantity = number(invoice_row, "quantity").unwrap_or(0.0);
    if invoice_quantity == 0.0 {
        return Ok(fyo.pesa().zero());
    }
    let share = number(row, "quantity").unwrap_or(0.0) / invoice_quantity;
    Ok(money_or_zero(invoice_row, "itemTaxedTotal", fyo).mul(share)?)
}

/// `getInvoiceDiscountAmount`, with the invoice's discount settings.
fn get_invoice_discount_amount(invoice: &Doc, fyo: &Fyo) -> Result<Money, Error> {
    if is_truthy(invoice.get("setDiscountAmount")) {
        return Ok(money_or_zero(invoice, "discountAmount", fyo));
    }

    let after_tax = is_truthy(invoice.get("discountAfterTax"));
    let mut total = fyo.pesa().zero();
    for row in invoice.table("items") {
        let fieldname = if after_tax {
            "itemTaxedTotal"
        } else {
            "itemDiscountedTotal"
        };
        total = total.add(&money_or_zero(row, fieldname, fyo))?;
    }
    Ok(total.percent(number(invoice, "discountPercent").unwrap_or(0.0))?)
}

/// `getTotalDiscount`.
fn get_total_discount(doc: &Doc, invoice: &Doc, fyo: &Fyo) -> Result<Money, Error> {
    if !single_is_truthy(fyo, "AccountingSettings", "enableDiscounting") {
        return Ok(fyo.pesa().zero());
    }
    Ok(get_item_discount_amount(doc, invoice, fyo)?
        .add(&get_invoice_discount_amount(invoice, fyo)?)?)
}

/// `getGrandTotal`: the amounts, with the taxes and less the discounts of the invoice the
/// shipment or receipt was made from.
fn get_grand_total(doc: &Doc, fyo: &Fyo) -> Result<Money, Error> {
    let net_total = doc.get_sum(fyo, "items", "amount")?;
    let Some(back_reference) = non_empty_field(doc, "backReference") else {
        return Ok(net_total);
    };

    let invoice = Doc::read(fyo, invoice_schema_name(doc), &back_reference)?;
    let total_discount = get_total_discount(doc, &invoice, fyo)?;
    let mut total = net_total;
    for tax in invoice.table("taxes") {
        total = total.add(&money_or_zero(tax, "amount", fyo))?;
    }
    Ok(total.sub(&total_discount)?)
}

fn non_empty_field(doc: &Doc, fieldname: &str) -> Option<String> {
    text(doc, fieldname)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

/// `validateAccounts`.
fn validate_accounts(doc: &Doc, fyo: &Fyo) -> Result<(), Error> {
    let mut settings = vec!["stockInHand"];
    settings.push(if is_sales(doc) {
        "costOfGoodsSold"
    } else {
        "stockReceivedButNotBilled"
    });

    let mut messages = Vec::new();
    for setting in settings {
        let Some(value) = single_text(fyo, "InventorySettings", setting) else {
            let label = fyo
                .schema("InventorySettings")
                .and_then(|s| s.field(setting))
                .and_then(|f| f.label.clone())
                .unwrap_or_else(|| setting.to_owned());
            messages.push(format!("{label} account not set in Inventory Settings."));
            continue;
        };
        if !fyo.db.exists("Account", Some(&value))? {
            messages.push(format!("Account {value} does not exist."));
        }
    }

    if !messages.is_empty() {
        return Err(Error::Validation(messages.join(" ")));
    }
    Ok(())
}

/// `getPostingAmount`: a receipt's grand total, or what a shipment's items cost.
fn get_posting_amount(doc: &Doc, fyo: &Fyo) -> Result<Money, Error> {
    let amount = if is_sales(doc) {
        get_shipment_cogs_amount(doc, fyo)?
    } else {
        money_or_zero(doc, "grandTotal", fyo)
    };
    posting_amount(amount, is_return(doc))
}

/// `validateSerialNumberStatus`.
fn validate_serial_number_status(doc: &Doc, fyo: &Fyo) -> Result<(), Error> {
    if doc.is_cancelled() {
        return Ok(());
    }

    for (serial_number, row) in get_serial_number_from_doc(doc) {
        if !can_validate_serial_number(fyo, doc.schema_name(), row, &serial_number)? {
            continue;
        }
        if !fyo.db.exists("SerialNumber", Some(&serial_number))? {
            continue;
        }

        // As in Books: the first serial number that can be checked ends the checks of a
        // submitted document or a return.
        if doc.is_submitted() || is_return(doc) {
            return Ok(());
        }

        let status = serial_number_status(fyo, &serial_number);
        if doc.schema_name() == "PurchaseReceipt" && status != "Inactive" {
            return Err(Error::Validation(format!(
                "Serial Number {serial_number} is not Inactive"
            )));
        }
        if doc.schema_name() == "Shipment" && status != "Active" {
            return Err(Error::Validation(format!(
                "Serial Number {serial_number} is not Active."
            )));
        }
    }
    Ok(())
}

/// `_validateHasReturnDocs`.
fn validate_has_return_docs(doc: &Doc, fyo: &Fyo) -> Result<(), Error> {
    let Some(name) = doc.name() else {
        return Ok(());
    };
    if !doc.is_cancelled() {
        return Ok(());
    }

    let options = GetAllOptions {
        filters: query([("returnAgainst", Filter::from(name))]),
        ..GetAllOptions::default()
    };
    let names: Vec<String> = fyo
        .db
        .get_all_docs(doc.schema_name(), &options)?
        .iter()
        .filter_map(|row| map_text(row, "name"))
        .collect();
    if names.is_empty() {
        return Ok(());
    }

    let label = &doc.schema().label;
    Err(Error::Validation(format!(
        "Cannot cancel {label} {name} because of the following {label}: {}",
        names.join(", ")
    )))
}

/// `_getTransferMap`: the quantity of each item.
fn get_transfer_map(doc: &Doc) -> IndexMap<String, f64> {
    let mut map = IndexMap::new();
    for row in doc.table("items") {
        let Some(item) = non_empty(row, "item") else {
            continue;
        };
        let quantity = number(row, "quantity").unwrap_or(0.0);
        if quantity == 0.0 || quantity.is_nan() {
            continue;
        }
        *map.entry(item).or_insert(0.0) += quantity;
    }
    map
}

/// `_updateBackReference`: takes what was shipped or received off what the invoice has
/// left to ship or receive, or puts it back on cancel.
fn update_back_reference(doc: &Doc, fyo: &mut Fyo) -> Result<(), Error> {
    if !doc.is_cancelled() && !doc.is_submitted() {
        return Ok(());
    }
    let Some(back_reference) = non_empty_field(doc, "backReference") else {
        return Ok(());
    };

    let mut invoice = fyo.get_doc(invoice_schema_name(doc), Some(&back_reference))?;
    let mut transfer_map = get_transfer_map(doc);
    let rows = invoice.table("items").len();
    for idx in 0..rows {
        let row = &invoice.table("items")[idx];
        let Some(item) = non_empty(row, "item") else {
            continue;
        };
        let quantity = number(row, "quantity").unwrap_or(0.0);
        let not_transferred = number(row, "stockNotTransferred").unwrap_or(0.0);
        let Some(&transferred) = transfer_map.get(&item) else {
            continue;
        };

        let (value, left) = if doc.is_cancelled() {
            (
                (not_transferred + transferred).min(quantity),
                (transferred + not_transferred - quantity).max(0.0),
            )
        } else {
            (
                (not_transferred - transferred).max(0.0),
                (transferred - not_transferred).max(0.0),
            )
        };
        invoice.set_child(
            fyo,
            "items",
            idx,
            "stockNotTransferred",
            DocValue::Number(value),
        )?;
        transfer_map.insert(item, left);
    }

    let not_transferred = invoice::get_stock_not_transferred(&invoice);
    invoice.set_and_sync(
        fyo,
        "stockNotTransferred",
        DocValue::Number(not_transferred),
    )
}

/// `_updateItemsReturned`: marks the shipment or receipt a return is against as returned
/// while it has a submitted return.
fn update_items_returned(doc: &Doc, fyo: &mut Fyo) -> Result<(), Error> {
    let Some(against) = non_empty_field(doc, "returnAgainst") else {
        return Ok(());
    };
    if !fyo.db.exists(doc.schema_name(), Some(&against))? {
        return Ok(());
    }
    let is_returned = !submitted_returns(fyo, doc.schema_name(), &against, None)?.is_empty();
    let mut reference = fyo.get_doc(doc.schema_name(), Some(&against))?;
    reference.set_and_sync(fyo, "isReturned", DocValue::Bool(is_returned))
}

/// `setFieldsFromBackReference`: takes the party, terms, date and rows from the invoice
/// a shipment or receipt is made from.
fn set_fields_from_back_reference(doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
    let Some(back_reference) = non_empty_field(doc, "backReference") else {
        return Ok(());
    };
    let schema_name = invoice_schema_name(doc);
    if !fyo.db.exists(schema_name, Some(&back_reference))? {
        return Ok(());
    }
    let invoice = Doc::read(fyo, schema_name, &back_reference)?;
    let Some(transfer) = invoice::get_stock_transfer(&invoice, fyo, false)? else {
        return Ok(());
    };

    for fieldname in ["party", "terms", "date"] {
        let value = transfer.get(fieldname).cloned().unwrap_or(DocValue::Null);
        doc.set(fyo, fieldname, value)?;
    }
    let rows: Vec<DocValueMap> = transfer
        .table("items")
        .iter()
        .map(|row| row.get_valid_dict(false, false))
        .collect();
    doc.set(fyo, "items", DocValue::Table(rows))?;
    Ok(())
}

/// `getInvoice`: a new invoice of a submitted shipment or receipt that wasn't made from
/// one, or `None`.
pub fn get_invoice(doc: &Doc, fyo: &Fyo) -> Result<Option<Doc>, Error> {
    if !doc.is_submitted() || non_empty_field(doc, "backReference").is_some() {
        return Ok(None);
    }

    let (terms, number_series) = if is_sales(doc) {
        ("salesInvoiceTerms", "salesInvoiceNumberSeries")
    } else {
        ("purchaseInvoiceTerms", "purchaseInvoiceNumberSeries")
    };
    let mut data = DocValueMap::from([
        (
            "party".to_owned(),
            doc.get("party").cloned().unwrap_or(DocValue::Null),
        ),
        ("date".to_owned(), DocValue::Date(Timestamp::now())),
        (
            "terms".to_owned(),
            DocValue::Text(single_text(fyo, "Defaults", terms).unwrap_or_default()),
        ),
        (
            "backReference".to_owned(),
            DocValue::Text(doc.name().unwrap_or_default().to_owned()),
        ),
    ]);
    if let Some(series) = single_text(fyo, "Defaults", number_series) {
        data.insert("numberSeries".to_owned(), DocValue::Text(series));
    }

    let mut invoice = fyo.get_new_doc(invoice_schema_name(doc), data)?;
    for row in doc.table("items") {
        if non_empty(row, "item").is_none() {
            continue;
        }
        let quantity = number(row, "quantity").unwrap_or(0.0);
        if quantity == 0.0 || quantity.is_nan() {
            continue;
        }

        let mut values = DocValueMap::new();
        for fieldname in ["item", "unit", "description", "hsnCode"] {
            values.insert(
                fieldname.to_owned(),
                row.get(fieldname).cloned().unwrap_or(DocValue::Null),
            );
        }
        values.insert("quantity".to_owned(), DocValue::Number(quantity));
        values.insert(
            "rate".to_owned(),
            DocValue::Money(money_or_zero(row, "rate", fyo)),
        );
        values.insert(
            "batch".to_owned(),
            non_empty(row, "batch").map_or(DocValue::Null, DocValue::Text),
        );
        invoice.push_row(fyo, "items", values)?;
    }

    if invoice.table("items").is_empty() {
        return Ok(None);
    }
    Ok(Some(invoice))
}

/// The serial numbers of an item already returned by the submitted returns.
fn returned_serial_numbers(
    fyo: &Fyo,
    schema_name: &str,
    name: &str,
) -> Result<Vec<(String, String)>, Error> {
    let mut returned = Vec::new();
    for return_doc in submitted_returns(fyo, schema_name, name, None)? {
        for row in table_rows(&return_doc, "items") {
            let (Some(item), Some(serial_numbers)) =
                (map_text(&row, "item"), map_text(&row, "serialNumber"))
            else {
                continue;
            };
            for serial_number in get_serial_numbers(&serial_numbers) {
                returned.push((item.clone(), serial_number));
            }
        }
    }
    Ok(returned)
}

/// `getReturnDoc`: a new return of what is left to return on a shipment or receipt,
/// dated now, or `None` if it isn't saved.
pub fn get_return_doc(doc: &Doc, fyo: &Fyo) -> Result<Option<Doc>, Error> {
    let Some(name) = doc.name() else {
        return Ok(None);
    };

    let mut data = doc.get_valid_dict(true, true);
    let Some(DocValue::Table(rows)) = data.get("items") else {
        return Ok(None);
    };

    let mut left = get_return_qty_total(fyo, doc.schema_name(), name, None)?;
    let returned_serials = returned_serial_numbers(fyo, doc.schema_name(), name)?;
    let mut items = Vec::new();
    for row in rows {
        let mut row = row.clone();
        row.shift_remove("name");
        let Some(item) = map_text(&row, "item") else {
            continue;
        };
        let Some(left) = left.get_mut(&(item.clone(), map_text(&row, "batch"))) else {
            continue;
        };
        let returned = map_number(&row, "quantity").min(*left);
        *left -= returned.max(0.0);
        if returned <= 0.0 {
            continue;
        }

        if let Some(serial_numbers) = map_text(&row, "serialNumber") {
            let serial_numbers: Vec<String> = get_serial_numbers(&serial_numbers)
                .into_iter()
                .filter(|sn| !returned_serials.contains(&(item.clone(), sn.clone())))
                .collect();
            row.insert(
                "serialNumber".to_owned(),
                DocValue::Text(serial_numbers.join("\n")),
            );
        }
        let unit_conversion_factor = map_number(&row, "unitConversionFactor");
        row.insert("quantity".to_owned(), DocValue::Number(-returned));
        row.insert(
            "transferQuantity".to_owned(),
            DocValue::Number(-returned / unit_conversion_factor),
        );
        items.push(row);
    }

    data.shift_remove("name");
    data.shift_remove("backReference");
    data.insert("date".to_owned(), DocValue::Date(Timestamp::now()));
    data.insert("items".to_owned(), DocValue::Table(items));
    data.insert("returnAgainst".to_owned(), DocValue::Text(name.to_owned()));
    data.insert("isReturned".to_owned(), DocValue::Bool(false));

    let mut return_doc = fyo.get_new_doc(doc.schema_name(), data)?;
    return_doc.run_formulas(fyo)?;
    Ok(Some(return_doc))
}

impl Transactional for StockTransfer {
    fn get_posting(&self, doc: &Doc, fyo: &mut Fyo) -> Result<Option<LedgerPosting>, Error> {
        validate_accounts(doc, fyo)?;
        let stock_in_hand =
            single_text(fyo, "InventorySettings", "stockInHand").unwrap_or_default();
        let amount = get_posting_amount(doc, fyo)?;
        let other = if is_sales(doc) {
            single_text(fyo, "InventorySettings", "costOfGoodsSold")
        } else {
            single_text(fyo, "InventorySettings", "stockReceivedButNotBilled")
        }
        .unwrap_or_default();

        // A shipment debits the cost of goods sold and a receipt the stock in hand; a
        // return swaps the sides.
        let (debit, credit) = match (is_sales(doc), is_return(doc)) {
            (true, false) | (false, true) => (other, stock_in_hand),
            (true, true) | (false, false) => (stock_in_hand, other),
        };
        let mut posting = LedgerPosting::new(doc);
        posting.debit(fyo, &debit, &amount)?;
        posting.credit(fyo, &credit, &amount)?;
        posting.make_round_off_entry(fyo)?;
        Ok(Some(posting))
    }
}

impl Model for StockTransfer {
    fn default(&self, fieldname: &str, doc: &Doc, fyo: &Fyo) -> Option<DocValue> {
        match fieldname {
            "numberSeries" => get_number_series(doc.schema_name(), fyo).map(DocValue::Text),
            "terms" => {
                let fieldname = if is_sales(doc) {
                    "shipmentTerms"
                } else {
                    "purchaseReceiptTerms"
                };
                Some(DocValue::Text(
                    single_text(fyo, "Defaults", fieldname).unwrap_or_default(),
                ))
            }
            "date" => Some(DocValue::Date(Timestamp::now())),
            _ => None,
        }
    }

    fn link_filters(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<QueryFilter> {
        match fieldname {
            "party" => {
                let role = if is_sales(ctx.doc) {
                    "Customer"
                } else {
                    "Supplier"
                };
                Some(query([("role", one_of(&[role, "Both"]))]))
            }
            "numberSeries" => Some(query([(
                "referenceType",
                Filter::from(ctx.doc.schema_name()),
            )])),
            "backReference" => Some(query([
                ("stockNotTransferred", Filter::op("!=", 0.0)),
                ("submitted", Filter::from(true)),
                ("cancelled", Filter::from(false)),
            ])),
            _ => None,
        }
    }

    fn formulas(&self) -> Vec<Formula> {
        vec![Formula {
            fieldname: "grandTotal",
            depends_on: Some(&["items"]),
            formula: |ctx, _| {
                Ok(Some(
                    DocValue::Money(get_grand_total(ctx.doc, ctx.fyo)?).into(),
                ))
            },
        }]
    }

    fn hidden(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<bool> {
        let doc = ctx.doc;
        let done = doc.is_submitted() || doc.is_cancelled();
        let set = |fieldname: &str| is_truthy(doc.get(fieldname));
        match fieldname {
            "backReference" | "terms" | "attachment" => Some(!(set(fieldname) || !done)),
            "returnAgainst" => Some(done && !set("returnAgainst")),
            _ => None,
        }
    }

    /// A copy isn't made from the invoice.
    fn duplicate(&self, doc: &mut Doc) {
        doc.set_value("backReference", DocValue::Null);
    }

    fn change(&self, doc: &mut Doc, fyo: &mut Fyo, changed: Option<&str>) -> Result<(), Error> {
        if changed == Some("backReference") {
            set_fields_from_back_reference(doc, fyo)?;
        }
        Ok(())
    }

    fn validate(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        transactional::validate(self, doc, fyo)?;
        validate_batch(doc, fyo)?;
        validate_serial_number(doc, fyo)?;
        validate_serial_number_status(doc, fyo)?;
        validate_has_return_docs(doc, fyo)
    }

    fn before_submit(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        if is_return(doc) {
            validate_return_quantities(doc, fyo)?;
        }
        transfer::before_submit(doc, fyo, &transfer_details(doc))
    }

    fn after_submit(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        transactional::after_submit(self, doc, fyo)?;
        transfer::after_submit(doc, fyo, &transfer_details(doc))?;
        update_serial_numbers(doc, fyo, false, is_return(doc))?;
        update_back_reference(doc, fyo)?;
        update_items_returned(doc, fyo)
    }

    fn before_cancel(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        transfer::before_cancel(doc, fyo, &transfer_details(doc))
    }

    fn after_cancel(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        transactional::after_cancel(self, doc, fyo)?;
        transfer::after_cancel(doc, fyo)?;
        update_serial_numbers(doc, fyo, true, is_return(doc))?;
        update_back_reference(doc, fyo)?;
        update_items_returned(doc, fyo)
    }

    fn after_delete(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        transactional::after_delete(self, doc, fyo)
    }
}
