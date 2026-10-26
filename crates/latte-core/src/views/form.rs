// What a form shows and asks besides its fields, @ v0.36.0:
// - the linked entries panel: `getLinkedEntries` in src/utils/doc.ts and
//   src/pages/CommonForm/LinkedEntries.vue;
// - the exchange rate control: src/components/Controls/ExchangeRate.vue;
// - the dialogs of src/utils/ui.ts: `getDocSyncMessage`, `getDocSubmitMessage`,
//   `showInsufficientInventoryDialog`, `cancelDocWithPrompt`, `deleteDocWithPrompt`,
//   `showCannotSaveOrSubmitToast` and `showCannotCancelOrDeleteToast`;
// - a Link field's suggestions and its Create entry: `getSuggestions`, `getCreateFilters`
//   and `openNewDoc` in src/components/Controls/Link.vue and AutoComplete.vue, and
//   QuickView.vue;
// - the quick edit panel's fields: src/pages/QuickEditForm.vue and TwoColumnForm.vue;
// - the barcode scanner: src/components/Controls/Barcode.vue and `addItem` in
//   models/helpers.ts.
//
// Differences from Books:
// - An item is short only when its stock is less than the quantity; Books also counts an
//   item with exactly enough stock, and lists it as "(0)".
// - A scanned item is added to a Stock Movement too; Books' `StockMovement.addItem`
//   only reads the item and adds no row.

use jiff::Timestamp;
use latte_db::{DocValue, DocValueMap, Filter, FilterOperand, FilterValue, GetAllOptions};
use latte_schema::{Field, FieldType};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::format::{Formatter, local_date};
use crate::fyo::model::doc::Doc;
use crate::fyo::model::helpers::is_truthy;
use crate::fyo::model::types::DocContext;
use crate::models::helpers::single_is_truthy;
use crate::models::inventory::helpers::get_stock_quantity;
use crate::views::field;
use crate::views::search::fuzzy_match;

/// `getDocReferenceLabel`: the document's name, or its schema's label when it is named at
/// random or has no name.
pub fn reference_label(doc: &Doc) -> String {
    let schema = doc.schema();
    let label = if schema.label.is_empty() {
        schema.name.clone()
    } else {
        schema.label.clone()
    };
    if schema.naming == Some(latte_schema::Naming::Random) {
        return label;
    }
    match doc.name() {
        Some(name) if !name.is_empty() => name.to_owned(),
        _ => label,
    }
}

fn is_invoice(doc: &Doc) -> bool {
    matches!(doc.schema_name(), "SalesInvoice" | "PurchaseInvoice")
}

fn money<'a>(doc: &'a Doc, fieldname: &str) -> Option<&'a latte_db::pesa::Money> {
    match doc.get(fieldname) {
        Some(DocValue::Money(m)) => Some(m),
        _ => None,
    }
}

fn text<'a>(doc: &'a Doc, fieldname: &str) -> Option<&'a str> {
    match doc.get(fieldname) {
        Some(DocValue::Text(t)) if !t.is_empty() => Some(t),
        _ => None,
    }
}

/// `getDocSyncMessage`: what Save asks when Ctrl+S saves (`commonDocSync` with a dialog).
pub fn sync_message(formatter: &Formatter, doc: &Doc) -> String {
    let label = reference_label(doc);
    let detail = format!("Create new {} entry?", doc.schema().label);
    if doc.inserted() {
        return format!("Save changes made to {label}?");
    }
    if is_invoice(doc)
        && let Some(grand_total) = money(doc, "grandTotal")
        && grand_total.is_zero()
    {
        let gt = formatter.currency(grand_total);
        return format!("{detail} Entry has Grand Total {gt}. Please verify amounts.");
    }
    detail
}

/// `getDocSubmitMessage`: what Submit asks, with the payment `makeAutoPayment` makes.
pub fn submit_message(fyo: &Fyo, formatter: &Formatter, doc: &Doc) -> String {
    let mut details = vec![format!("Mark {} as submitted?", doc.schema().label)];
    if is_invoice(doc) && is_truthy(doc.get("makeAutoPayment")) {
        let auto_payment_account = {
            let fieldname = if doc.schema_name() == "SalesInvoice" {
                "salesPaymentAccount"
            } else {
                "purchasePaymentAccount"
            };
            match fyo.get_value("Defaults", None, fieldname) {
                Some(DocValue::Text(account)) => account,
                _ => String::new(),
            }
        };
        let account = text(doc, "account").unwrap_or_default().to_owned();
        let (from, to) = if doc.schema_name() == "SalesInvoice" {
            (account, auto_payment_account)
        } else {
            (auto_payment_account, account)
        };
        let zero = fyo.pesa().zero();
        let amount = formatter.currency(money(doc, "outstandingAmount").unwrap_or(&zero));
        details.push(format!(
            "Payment of {amount} will be made from account \"{from}\" to account \"{to}\" on Submit."
        ));
    }
    details.join(" ")
}

/// `showInsufficientInventoryDialog`'s check, made before a Sales Invoice is submitted
/// with inventory on: each tracked item whose stock before the invoice's date is less
/// than its quantity, with what is short.
pub fn insufficient_quantity(fyo: &Fyo, doc: &Doc) -> Result<Vec<(String, f64)>, Error> {
    if doc.schema_name() != "SalesInvoice"
        || !single_is_truthy(fyo, "AccountingSettings", "enableInventory")
    {
        return Ok(Vec::new());
    }
    let date = match doc.get("date") {
        Some(DocValue::Date(date)) => Some(*date),
        _ => None,
    };
    let mut insufficient = Vec::new();
    for row in doc.table("items") {
        let (Some(item), Some(DocValue::Number(quantity))) =
            (text(row, "item"), row.get("quantity"))
        else {
            continue;
        };
        if !is_truthy(fyo.get_value("Item", Some(item), "trackItem").as_ref()) {
            continue;
        }
        let stock = get_stock_quantity(fyo, item, None, None, date, text(row, "batch"), &[])?
            .unwrap_or(0.0);
        if stock >= *quantity {
            continue;
        }
        insufficient.push((item.to_owned(), quantity - stock));
    }
    Ok(insufficient)
}

/// The Insufficient Quantity dialog's two lines.
pub fn insufficient_quantity_detail(insufficient: &[(String, f64)]) -> [String; 2] {
    let list = insufficient
        .iter()
        .map(|(item, quantity)| {
            format!(
                "{item} ({})",
                crate::fyo::model::helpers::js_number_string(*quantity)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    [
        format!("The following items have insufficient quantity for Shipment: {list}"),
        "Continue submitting Sales Invoice?".to_owned(),
    ]
}

/// `cancelDocWithPrompt`'s detail: an invoice says which payments its cancel cancels.
pub fn cancel_message(fyo: &Fyo, doc: &Doc) -> Result<String, Error> {
    let detail = "This action is permanent".to_owned();
    if !is_invoice(doc) {
        return Ok(detail);
    }
    let options = GetAllOptions {
        fields: Some(vec!["parent".to_owned()]),
        filters: [(
            "referenceName".to_owned(),
            Filter::from(doc.name().unwrap_or_default()),
        )]
        .into(),
        ..GetAllOptions::default()
    };
    let mut payments: Vec<String> = Vec::new();
    for row in fyo.db.get_all_docs("PaymentFor", &options)? {
        let Some(DocValue::Text(parent)) = row.get("parent") else {
            continue;
        };
        if payments.contains(parent)
            || is_truthy(fyo.get_value("Payment", Some(parent), "cancelled").as_ref())
        {
            continue;
        }
        payments.push(parent.clone());
    }
    Ok(match payments.len() {
        0 => detail,
        1 => format!(
            "This action is permanent and will cancel the following payment: {}",
            payments[0]
        ),
        _ => format!(
            "This action is permanent and will cancel the following payments: {}",
            payments.join(", ")
        ),
    })
}

/// `deleteDocWithPrompt`'s detail.
pub fn delete_message(doc: &Doc) -> &'static str {
    if doc.schema().is_submittable && doc.is_submitted() {
        "This action is permanent and will delete associated ledger entries."
    } else {
        "This action is permanent."
    }
}

/// Whether an error is Books' `LinkValidationError`: the database refused to delete a
/// document other documents link to.
pub fn is_link_validation_error(error: &Error) -> bool {
    match error {
        Error::Db(latte_db::Error::Sqlite(rusqlite::Error::SqliteFailure(e, message))) => {
            e.code == rusqlite::ErrorCode::ConstraintViolation
                && (e.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY
                    || message
                        .as_deref()
                        .is_some_and(|m| m.contains("FOREIGN KEY")))
        }
        _ => false,
    }
}

/// `deleteDocWithPrompt`'s "Delete Failed" detail for a document with linked entries.
pub fn delete_failed_message(doc: &Doc) -> String {
    format!(
        "Cannot delete {} \"{}\" because of linked entries.",
        doc.schema().label,
        doc.name().unwrap_or_default()
    )
}

/// `showCannotSaveOrSubmitToast`: Ctrl+S on a document with nothing to save or submit.
pub fn cannot_save_message(doc: &Doc) -> String {
    let label = reference_label(doc);
    if doc.schema().is_submittable && doc.is_submitted() {
        format!("{label} already submitted")
    } else {
        format!("{label} already saved")
    }
}

/// `showCannotCancelOrDeleteToast`: Ctrl+Backspace on a document that can't go.
pub fn cannot_cancel_message(doc: &Doc) -> String {
    let label = reference_label(doc);
    if doc.schema().is_submittable && !doc.is_cancelled() {
        format!("{label} cannot be cancelled")
    } else {
        format!("{label} cannot be deleted")
    }
}

/// The exchange rate control of a document in another currency (`canShowExchangeRate`):
/// the document's currency, the company's, and the rate.
pub fn exchange_rate(fyo: &Fyo, doc: &Doc) -> Option<(String, String, f64)> {
    let currency = text(doc, "currency")?;
    let company = match fyo.get_value("SystemSettings", None, "currency") {
        Some(DocValue::Text(c)) if !c.is_empty() => c,
        _ => crate::fyo::format::DEFAULT_CURRENCY.to_owned(),
    };
    if currency == company {
        return None;
    }
    let rate = match doc.get("exchangeRate") {
        Some(DocValue::Number(rate)) => *rate,
        _ => 1.0,
    };
    Some((currency.to_owned(), company, rate))
}

/// What `ExchangeRate.vue` shows: `from_value` of the left currency, equal to the right
/// value in the right currency. Swapped, the company's currency is on the left.
pub fn exchange_rate_right(rate: f64, from_value: f64, swapped: bool) -> f64 {
    if swapped {
        from_value / rate
    } else {
        rate * from_value
    }
}

/// `rightChange`: the rate a typed right value makes.
pub fn exchange_rate_from_right(right: f64, from_value: f64, swapped: bool) -> f64 {
    if swapped {
        from_value / right
    } else {
        right / from_value
    }
}

/// `fuzzyMatch` over the options' labels: the indexes of those that match `keyword`,
/// closest first, as `getSuggestions` orders them. An empty keyword keeps them all.
pub fn suggestions(labels: &[&str], keyword: &str) -> Vec<usize> {
    if keyword.is_empty() {
        return (0..labels.len()).collect();
    }
    let mut matched: Vec<(usize, f64)> = labels
        .iter()
        .enumerate()
        .filter_map(|(i, label)| {
            let (is_match, distance) = fuzzy_match(keyword, label);
            is_match.then_some((i, distance))
        })
        .collect();
    matched.sort_by(|a, b| a.1.total_cmp(&b.1));
    matched.into_iter().map(|(i, _)| i).collect()
}

/// `getFilters`: whether a Link field's options are filtered. SystemSettings'
/// `removeFilter` drops the model's filters, not those in the schema.
pub fn link_is_filtered(fyo: &Fyo, f: &Field, ctx: &DocContext<'_>) -> bool {
    !field::link_filters(f, ctx).is_empty()
        && (f.filters.is_some() || !single_is_truthy(fyo, "SystemSettings", "removeFilter"))
}

/// `fyo.format(value, field, doc)`: as [`field::display`], with an amount in the currency
/// the document's model gives the field (`getCurrencies`), as an invoice in another
/// currency shows its rates and totals.
pub fn display(
    formatter: &Formatter,
    f: &Field,
    value: Option<&DocValue>,
    ctx: &DocContext<'_>,
) -> String {
    if let Some(DocValue::Money(money)) = value
        && let Some(currency) = ctx.doc.model().currency(&f.fieldname, ctx)
    {
        return formatter.currency_in(money, &currency);
    }
    field::display(formatter, f, value)
}

/// `groupBy`: the group of each of a Link field's options, by its name, for the fields
/// whose dropdown lists them under headings (a journal entry's accounts by root type).
pub fn link_groups(
    fyo: &Fyo,
    f: &Field,
    ctx: &DocContext<'_>,
) -> std::collections::HashMap<String, String> {
    let mut groups = std::collections::HashMap::new();
    let (Some(group_by), Some(target)) = (f.group_by.as_deref(), field::link_target(f, ctx)) else {
        return groups;
    };
    let options = GetAllOptions {
        fields: Some(vec!["name".to_owned(), group_by.to_owned()]),
        ..GetAllOptions::default()
    };
    for row in fyo.db.get_all_docs(&target, &options).unwrap_or_default() {
        if let (Some(DocValue::Text(name)), Some(DocValue::Text(group))) =
            (row.get("name"), row.get(group_by))
        {
            groups.insert(name.clone(), group.clone());
        }
    }
    groups
}

/// `getCreateFilters` and `openNewDoc`: the values a new document made from a Link
/// field's Create starts with, its name being what was typed or a temporary name.
pub fn create_values(fyo: &Fyo, f: &Field, ctx: &DocContext<'_>, typed: &str) -> DocValueMap {
    let mut values = DocValueMap::new();
    let target = field::link_target(f, ctx).unwrap_or_default();
    let name = if typed.is_empty() {
        fyo.schema(&target)
            .map(|schema| fyo.get_temporary_name(schema))
            .unwrap_or_default()
    } else {
        typed.to_owned()
    };
    values.insert("name".to_owned(), DocValue::Text(name));
    let schema_name = f.schema_name.as_deref().unwrap_or_default();
    let is_sales = matches!(
        ctx.parent.unwrap_or(ctx.doc).schema_name(),
        "SalesInvoice" | "Shipment" | "SalesQuote"
    );
    let text = |v: &str| DocValue::Text(v.to_owned());
    // The models' `createFilters`.
    let own: Option<Vec<(&str, DocValue)>> = match (schema_name, f.fieldname.as_str()) {
        (
            "SalesInvoice" | "PurchaseInvoice" | "SalesQuote" | "Shipment" | "PurchaseReceipt",
            "party",
        ) => Some(vec![(
            "role",
            text(if is_sales { "Customer" } else { "Supplier" }),
        )]),
        ("StockMovementItem", "item") => Some(vec![
            ("trackItem", DocValue::Bool(true)),
            ("itemType", text("Product")),
        ]),
        ("SalesInvoiceItem" | "PurchaseInvoiceItem" | "SalesQuoteItem" | "InvoiceItem", "item") => {
            Some(vec![(
                "for",
                text(if is_sales { "Sales" } else { "Purchases" }),
            )])
        }
        _ => None,
    };
    if let Some(own) = own {
        for (key, value) in own {
            values.insert(key.to_owned(), value);
        }
        return values;
    }
    // `getCreateFiltersFromListViewFilters`: the filters that name one value.
    if !link_is_filtered(fyo, f, ctx) {
        return values;
    }
    for (key, filter) in field::link_filters(f, ctx) {
        let value = match filter {
            Filter::Equals(value) => Some(value),
            Filter::Op(op, FilterOperand::List(list)) if op == "in" => list
                .into_iter()
                .find(|v| *v != FilterValue::Text("Both".to_owned())),
            _ => None,
        };
        let value = match value {
            Some(FilterValue::Text(t)) => DocValue::Text(t),
            Some(FilterValue::Bool(b)) => DocValue::Bool(b),
            Some(FilterValue::Number(n)) => DocValue::Number(n),
            Some(FilterValue::Null) => DocValue::Null,
            None => continue,
        };
        values.insert(key, value);
    }
    values
}

/// `QuickEditForm.vue`'s fields: the schema's `quickEditFields` (or the name) less
/// `hide`, else its required fields, without those hidden now.
pub fn quick_edit_fields(fyo: &Fyo, doc: &Doc, hide: &[String]) -> Vec<Field> {
    let schema = doc.schema();
    let names: Vec<String> = schema
        .quick_edit_fields
        .clone()
        .unwrap_or_else(|| vec!["name".to_owned()])
        .into_iter()
        .filter(|f| !hide.contains(f))
        .collect();
    let mut fields: Vec<Field> = names
        .iter()
        .filter_map(|name| schema.field(name).cloned())
        .collect();
    if fields.is_empty() {
        fields = schema
            .fields
            .iter()
            .filter(|f| f.required)
            .cloned()
            .collect();
    }
    let ctx = field::context(fyo, doc, None);
    fields.retain(|f| !field::is_hidden(f, &ctx));
    fields
}

/// `QuickView.vue`: the values of a linked document a Link field's chevron shows on
/// hover, as `(label, value)`.
pub fn quick_view(
    fyo: &Fyo,
    formatter: &Formatter,
    schema_name: &str,
    name: &str,
) -> Vec<(String, String)> {
    let Some(schema) = fyo.schema(schema_name) else {
        return Vec::new();
    };
    let fields: Vec<&Field> = schema
        .fields
        .iter()
        .filter(|f| {
            !matches!(
                f.fieldtype,
                Some(FieldType::Table | FieldType::AttachImage | FieldType::Attachment)
            ) && f.fieldname != "name"
                && !f.hidden
                && !f.meta
                && !f.is_abstract
                && !f.computed
        })
        .collect();
    let Ok(values) = fyo.db.get_doc(schema_name, name) else {
        return Vec::new();
    };
    fields
        .into_iter()
        .filter_map(|f| {
            let value = values.get(&f.fieldname)?;
            let falsy = match value {
                DocValue::Null | DocValue::Bool(false) => true,
                DocValue::Number(n) => *n == 0.0,
                DocValue::Text(t) => t.is_empty(),
                DocValue::Money(m) => m.is_zero(),
                DocValue::Table(rows) => rows.is_empty(),
                _ => false,
            };
            if falsy {
                return None;
            }
            let shown = field::display(formatter, f, Some(value));
            (!shown.is_empty()).then(|| (f.label.clone().unwrap_or_default(), shown))
        })
        .collect()
}

/// A pill of the linked entries panel: its text and Books' colour name.
#[derive(Debug, Clone, PartialEq)]
pub struct Pill {
    pub text: String,
    pub color: &'static str,
}

/// A document of the linked entries panel.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkedEntry {
    pub name: String,
    pub date: Option<String>,
    pub pills: Vec<Pill>,
}

/// The linked documents of one schema.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkedGroup {
    pub schema: String,
    pub label: String,
    pub entries: Vec<LinkedEntry>,
}

/// `linkSequence`: the order the panel lists schemas in; others follow.
const LINK_SEQUENCE: [&str; 13] = [
    "SalesInvoice",
    "PurchaseInvoice",
    "Shipment",
    "PurchaseReceipt",
    "Payment",
    "JournalEntry",
    "StockMovement",
    "Party",
    "Item",
    "Account",
    "Location",
    "AccountingLedgerEntry",
    "StockLedgerEntry",
];

/// `linkEntryDisplayFields`.
fn display_fields(schema_name: &str) -> &'static [&'static str] {
    match schema_name {
        "SalesInvoice" | "PurchaseInvoice" => &[
            "name",
            "date",
            "party",
            "grandTotal",
            "outstandingAmount",
            "stockNotTransferred",
        ],
        "Shipment" | "PurchaseReceipt" => &["name", "date", "party", "grandTotal"],
        "Payment" => &["name", "date", "party", "amount"],
        "JournalEntry" => &["name", "date", "entryType"],
        "StockMovement" => &["name", "date", "amount"],
        "AccountingLedgerEntry" => &["name", "date", "account", "credit", "debit"],
        "StockLedgerEntry" => &["name", "date", "item", "location", "quantity"],
        _ => &["name"],
    }
}

/// `getLinkedEntries`: the names of the documents that link to `doc`, by schema, newest
/// first. Links from a child row count for its parent; cancelled documents are left out.
pub fn linked_entry_names(fyo: &Fyo, doc: &Doc) -> Result<Vec<(String, Vec<String>)>, Error> {
    let target = doc.schema_name();
    let Some(name) = doc.name() else {
        return Ok(Vec::new());
    };
    let schema_map = fyo.db.schema_map();
    let mut schemas: Vec<_> = schema_map.values().filter(|s| !s.is_single).collect();
    schemas.sort_by(|a, b| a.name.cmp(&b.name));
    // (schema, name, created)
    let mut entries: Vec<(String, String, Option<Timestamp>)> = Vec::new();
    let mut parents: Vec<(String, String)> = Vec::new();
    let created = |row: &DocValueMap| match row.get("created") {
        Some(DocValue::Date(ts)) => Some(*ts),
        Some(DocValue::Text(t)) => t.parse::<Timestamp>().ok(),
        _ => None,
    };
    for schema in schemas {
        for f in &schema.fields {
            let linking = match f.fieldtype {
                Some(FieldType::Link) => f.target.as_deref() == Some(target),
                Some(FieldType::DynamicLink) => true,
                _ => false,
            };
            if !linking {
                continue;
            }
            let mut filters = latte_db::QueryFilter::new();
            filters.insert(f.fieldname.clone(), Filter::from(name));
            if f.fieldtype == Some(FieldType::DynamicLink) {
                let Some(references) = f.references.as_deref() else {
                    continue;
                };
                filters.insert(references.to_owned(), Filter::from(target));
            }
            let mut fields = vec!["name".to_owned()];
            if schema.is_child {
                fields.extend(["parent".to_owned(), "parentSchemaName".to_owned()]);
            } else {
                fields.push("created".to_owned());
            }
            if schema.is_submittable {
                filters.insert(
                    "cancelled".to_owned(),
                    Filter::Equals(FilterValue::Bool(false)),
                );
            }
            let options = GetAllOptions {
                fields: Some(fields),
                filters,
                ..GetAllOptions::default()
            };
            for row in fyo.db.get_all_docs(&schema.name, &options)? {
                let Some(DocValue::Text(row_name)) = row.get("name") else {
                    continue;
                };
                if schema.is_child {
                    if let (Some(DocValue::Text(parent)), Some(DocValue::Text(parent_schema))) =
                        (row.get("parent"), row.get("parentSchemaName"))
                    {
                        let key = (parent_schema.clone(), parent.clone());
                        if !parents.contains(&key) {
                            parents.push(key);
                        }
                    }
                } else {
                    entries.push((schema.name.clone(), row_name.clone(), created(&row)));
                }
            }
        }
    }
    for (schema_name, parent) in parents {
        let options = GetAllOptions {
            fields: Some(vec!["name".to_owned(), "created".to_owned()]),
            filters: [("name".to_owned(), Filter::from(parent.as_str()))].into(),
            ..GetAllOptions::default()
        };
        for row in fyo.db.get_all_docs(&schema_name, &options)? {
            if let Some(DocValue::Text(n)) = row.get("name") {
                entries.push((schema_name.clone(), n.clone(), created(&row)));
            }
        }
    }
    // Each schema's documents, with when they were made.
    type Made = Vec<(String, Option<Timestamp>)>;
    let mut groups: Vec<(String, Made)> = Vec::new();
    for (schema_name, entry, created) in entries {
        match groups.iter_mut().find(|(s, _)| *s == schema_name) {
            Some((_, list)) => list.push((entry, created)),
            None => groups.push((schema_name, vec![(entry, created)])),
        }
    }
    // The sequence first, then the rest in the order found.
    groups.sort_by_key(|(s, _)| {
        LINK_SEQUENCE
            .iter()
            .position(|q| q == s)
            .unwrap_or(LINK_SEQUENCE.len())
    });
    Ok(groups
        .into_iter()
        .map(|(schema, mut list)| {
            list.sort_by_key(|e| std::cmp::Reverse(e.1));
            (schema, list.into_iter().map(|(n, _)| n).collect())
        })
        .collect())
}

/// `LinkedEntries.vue`: the linked documents with what the panel shows of each.
pub fn linked_entries(
    fyo: &Fyo,
    formatter: &Formatter,
    doc: &Doc,
) -> Result<Vec<LinkedGroup>, Error> {
    let mut groups = Vec::new();
    for (schema_name, names) in linked_entry_names(fyo, doc)? {
        if names.is_empty() {
            continue;
        }
        let Some(schema) = fyo.schema(&schema_name) else {
            continue;
        };
        let fields: Vec<String> = display_fields(&schema_name)
            .iter()
            .filter(|f| **f == "name" || schema.field(f).is_some())
            .map(|f| (*f).to_owned())
            .collect();
        let options = GetAllOptions {
            fields: Some(fields),
            filters: [(
                "name".to_owned(),
                Filter::list(
                    "in",
                    names
                        .iter()
                        .map(|n| FilterValue::from(n.as_str()))
                        .collect(),
                ),
            )]
            .into(),
            ..GetAllOptions::default()
        };
        let entries = fyo
            .db
            .get_all_docs(&schema_name, &options)?
            .iter()
            .map(|row| linked_entry(formatter, row))
            .collect();
        groups.push(LinkedGroup {
            schema: schema_name.clone(),
            label: schema.label.clone(),
            entries,
        });
    }
    Ok(groups)
}

fn linked_entry(formatter: &Formatter, row: &DocValueMap) -> LinkedEntry {
    let text = |f: &str| match row.get(f) {
        Some(DocValue::Text(t)) if !t.is_empty() => Some(t.clone()),
        _ => None,
    };
    let positive = |f: &str| match row.get(f) {
        Some(DocValue::Money(m)) if m.is_positive() => Some(formatter.currency(m)),
        _ => None,
    };
    let number = |f: &str| match row.get(f) {
        Some(DocValue::Number(n)) if *n != 0.0 => Some(formatter.float(*n)),
        _ => None,
    };
    let pill = |text: String, color: &'static str| Pill { text, color };
    let mut pills = Vec::new();
    if let Some(credit) = positive("credit") {
        pills.push(pill(format!("Cr. {credit}"), "gray"));
    } else if let Some(debit) = positive("debit") {
        pills.push(pill(format!("Dr. {debit}"), "gray"));
    }
    if let Some(party) = text("party")
        .or_else(|| text("entryType"))
        .or_else(|| text("account"))
    {
        pills.push(pill(party, "gray"));
    }
    if let Some(item) = text("item") {
        pills.push(pill(item, "gray"));
    }
    if let Some(location) = text("location") {
        pills.push(pill(location, "gray"));
    }
    if let Some(outstanding) = positive("outstandingAmount") {
        pills.push(pill(format!("Unpaid {outstanding}"), "orange"));
    } else if let Some(total) = positive("grandTotal").or_else(|| positive("amount")) {
        pills.push(pill(total, "green"));
    }
    if let Some(pending) = number("stockNotTransferred") {
        pills.push(pill(format!("Pending qty. {pending}"), "orange"));
    } else if let Some(quantity) = number("quantity") {
        pills.push(pill(format!("Qty. {quantity}"), "gray"));
    }
    let date = match row.get("date") {
        Some(DocValue::Date(ts)) => Some(formatter.date(local_date(*ts))),
        _ => None,
    };
    LinkedEntry {
        name: text("name").unwrap_or_default(),
        date,
        pills,
    }
}

/// Whether a document takes scanned items (`typeof doc.addItem === 'function'`).
pub fn takes_items(doc: &Doc) -> bool {
    matches!(
        doc.schema_name(),
        "SalesInvoice" | "PurchaseInvoice" | "Shipment" | "PurchaseReceipt" | "StockMovement"
    )
}

/// `canShowBarcode`: barcodes are on in InventorySettings and the document takes items
/// and isn't submitted or cancelled.
pub fn shows_barcode(fyo: &Fyo, doc: &Doc) -> bool {
    single_is_truthy(fyo, "InventorySettings", "enableBarcodes")
        && takes_items(doc)
        && !doc.is_submitted()
        && !doc.is_cancelled()
}

/// `Barcode.vue`'s `selectItem`: the item with a scanned or typed barcode, or the error
/// toast's message.
pub fn barcode_item(fyo: &Fyo, code: &str) -> Result<String, String> {
    let barcode = code.trim();
    if barcode.len() < 12 || !barcode.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(format!("Invalid barcode value {barcode}."));
    }
    let options = GetAllOptions {
        fields: Some(vec!["name".to_owned()]),
        filters: [("barcode".to_owned(), Filter::from(barcode))].into(),
        ..GetAllOptions::default()
    };
    let rows = fyo.db.get_all_docs("Item", &options).unwrap_or_default();
    match rows.first().and_then(|r| r.get("name")) {
        Some(DocValue::Text(name)) => Ok(name.clone()),
        _ => Err(format!("Item with barcode {barcode} not found.")),
    }
}

/// `addItem`: one more of a row's item, or a new row for it.
pub fn add_item(fyo: &mut Fyo, doc: &mut Doc, name: &str) -> Result<(), Error> {
    if !doc.can_edit() {
        return Ok(());
    }
    let existing = doc
        .table("items")
        .iter()
        .position(|row| text(row, "item") == Some(name));
    if let Some(i) = existing {
        let quantity = match doc.table("items")[i].get("quantity") {
            Some(DocValue::Number(q)) => *q,
            _ => 0.0,
        };
        doc.set_child(
            fyo,
            "items",
            i,
            "quantity",
            DocValue::Number(quantity + 1.0),
        )?;
        return Ok(());
    }
    doc.append(fyo, "items", DocValueMap::new())?;
    let last = doc.table("items").len() - 1;
    doc.set_child(fyo, "items", last, "item", DocValue::Text(name.to_owned()))?;
    Ok(())
}
