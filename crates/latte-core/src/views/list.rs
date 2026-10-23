// The list view: `getListViewSettings` of each model, `List.vue` and `ListCell.vue`, and the
// status helpers of models/helpers.ts (`getDocStatus`, `getInvoiceStatus`, `statusColor`,
// `getStatusText`) @ v0.36.0.
//
// Differences from Books:
// - Books pages the list 20 to 100 rows at a time; here every row is in one scrolled list.
// - A search box filters by name and title; Books has a filter dropdown instead.

use latte_db::{DocValue, DocValueMap, Filter, FilterValue, GetAllOptions, QueryFilter};
use latte_schema::{Field, FieldType, Schema};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::format::Formatter;
use crate::fyo::model::helpers::is_truthy;
use crate::views::field;

/// A column of a list.
// A list has a handful of columns, so the size of a field column doesn't matter.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum ListColumn {
    Field(Field),
    /// The status badge (`getTransactionStatusColumn`, `getDocStatusListColumn`,
    /// `getLeadStatusColumn`, `getSerialNumberStatusColumn`).
    Status,
    /// `getIsDocEnabledColumn`.
    Enabled,
    /// `getPriceListStatusColumn`.
    EnabledFor,
}

impl ListColumn {
    pub fn label(&self) -> String {
        match self {
            Self::Field(field) => field
                .label
                .clone()
                .unwrap_or_else(|| field.fieldname.clone()),
            Self::Status => "Status".to_owned(),
            Self::Enabled => "Enabled".to_owned(),
            Self::EnabledFor => "Enabled For".to_owned(),
        }
    }

    /// `isNumeric`: numbers are right-aligned.
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            Self::Field(Field {
                fieldtype: Some(FieldType::Int | FieldType::Float | FieldType::Currency),
                ..
            })
        )
    }
}

/// `getListViewSettings(fyo).columns` of the models that have one.
fn settings_columns(schema_name: &str) -> Option<&'static [&'static str]> {
    const STATUS: &str = "$status";
    const ENABLED: &str = "$enabled";
    Some(match schema_name {
        "Account" => &["name", "rootType", "isGroup", "parentAccount"],
        "Party" => &["name", "email", "phone", "outstandingAmount"],
        "SalesInvoice" | "PurchaseInvoice" => &[
            "name",
            STATUS,
            "party",
            "date",
            "baseGrandTotal",
            "outstandingAmount",
        ],
        "SalesQuote" => &[
            "name",
            STATUS,
            "party",
            "date",
            "baseGrandTotal",
            "outstandingAmount",
        ],
        "Payment" => &["name", STATUS, "party", "date", "amount"],
        "JournalEntry" => &["name", STATUS, "date", "entryType", "referenceNumber"],
        "Item" => &["name", "unit", "tax", "rate"],
        "Tax" => &["name"],
        "AccountingLedgerEntry" => &[
            "date",
            "account",
            "party",
            "debit",
            "credit",
            "referenceName",
        ],
        "Address" => &["name", "addressLine1", "city", "state", "country"],
        "PaymentMethod" => &["name", "type"],
        "PriceList" => &["name", ENABLED, "$enabledFor"],
        "PricingRule" => &["name", "title", ENABLED, "discountType"],
        "CouponCode" => &["name", "couponName", "pricingRule", "maximumUse", "used"],
        "Lead" => &["name", STATUS, "email", "mobile"],
        "LoyaltyProgram" => &["name", "fromDate", "toDate", "expiryDuration"],
        "LoyaltyPointEntry" => &[
            "loyaltyProgram",
            "customer",
            "purchaseAmount",
            "loyaltyPoints",
        ],
        "StockMovement" => &["name", STATUS, "date", "movementType"],
        "StockLedgerEntry" => &[
            "date",
            "item",
            "location",
            "rate",
            "quantity",
            "referenceName",
        ],
        "Shipment" | "PurchaseReceipt" => &["name", STATUS, "party", "date", "grandTotal"],
        "POSOpeningShift" => &["name", "openingDate"],
        "POSClosingShift" => &["name", "closingDate"],
        "Batch" => &["name", "expiryDate", "manufactureDate"],
        "SerialNumber" => &["name", STATUS, "item", "description", "party"],
        "ItemGroup" => &["name", "tax", "hsnCode"],
        "ItemEnquiry" => &[
            "item",
            "customer",
            "contact",
            "description",
            "similarProduct",
        ],
        "PrintTemplate" => &["name", "type", "isCustom"],
        _ => return None,
    })
}

/// The list's columns. A model without list settings shows its name and quick-edit
/// fields, as `List.vue` does.
pub fn columns(fyo: &Fyo, schema_name: &str) -> Vec<ListColumn> {
    let Some(schema) = fyo.schema(schema_name) else {
        return Vec::new();
    };
    let names: Vec<String> = match settings_columns(schema_name) {
        Some(columns) => columns.iter().map(|c| (*c).to_owned()).collect(),
        None => {
            let mut names = vec!["name".to_owned()];
            for name in schema.quick_edit_fields.iter().flatten() {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
            names
        }
    };
    names
        .iter()
        .filter_map(|name| match name.as_str() {
            "$status" => Some(ListColumn::Status),
            "$enabled" => Some(ListColumn::Enabled),
            "$enabledFor" => Some(ListColumn::EnabledFor),
            _ => schema.field(name).cloned().map(ListColumn::Field),
        })
        .collect()
}

/// A badge: its label and colour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub label: String,
    pub color: &'static str,
}

/// `statusColor`.
pub fn status_color(status: &str) -> &'static str {
    match status {
        "Replied" | "Opportunity" | "PartlyPaid" | "Interested" => "yellow",
        "Unpaid" => "orange",
        "Paid" | "Converted" | "Quotation" | "Submitted" => "green",
        "Saved" => "blue",
        "Cancelled" | "DonotContact" => "red",
        "Return" | "ReturnIssued" => "lime",
        // `serialNumberStatusColor`.
        "Active" => "green",
        "Delivered" => "blue",
        _ => "gray",
    }
}

/// `getStatusText`, with the lead statuses of `getStatusTextOfLead`.
pub fn status_text(status: &str) -> &'static str {
    match status {
        "Draft" => "Draft",
        "Saved" => "Saved",
        "NotSaved" => "Not Saved",
        "Submitted" => "Submitted",
        "Cancelled" => "Cancelled",
        "Paid" => "Paid",
        "Unpaid" => "Unpaid",
        "PartlyPaid" => "Partly Paid",
        "Return" => "Return",
        "ReturnIssued" => "Return Issued",
        "Open" => "Open",
        "Replied" => "Replied",
        "Opportunity" => "Opportunity",
        "Interested" => "Interested",
        "Converted" => "Converted",
        "Quotation" => "Quotation",
        "DonotContact" => "Do not Contact",
        _ => "",
    }
}

/// `getDocStatus` for a stored document (never Draft or Not Saved), as the list shows it.
pub fn doc_status(schema: &Schema, values: &DocValueMap) -> &'static str {
    let truthy = |fieldname: &str| is_truthy(values.get(fieldname));
    if schema.name == "Lead" {
        return match values.get("status") {
            Some(DocValue::Text(status)) => match status.as_str() {
                "Open" => "Open",
                "Replied" => "Replied",
                "Opportunity" => "Opportunity",
                "Interested" => "Interested",
                "Converted" => "Converted",
                "Quotation" => "Quotation",
                "DonotContact" => "DonotContact",
                _ => "",
            },
            _ => "",
        };
    }
    if !schema.is_submittable {
        return "Saved";
    }
    let (submitted, cancelled) = (truthy("submitted"), truthy("cancelled"));
    if matches!(schema.name.as_str(), "SalesInvoice" | "PurchaseInvoice") {
        return invoice_status(values, submitted, cancelled);
    }
    if matches!(schema.name.as_str(), "Shipment" | "PurchaseReceipt") {
        if truthy("returnAgainst") && submitted && !cancelled {
            return "Return";
        }
        if truthy("isReturned") && submitted && !cancelled {
            return "ReturnIssued";
        }
    }
    match (submitted, cancelled) {
        (true, false) => "Submitted",
        (true, true) => "Cancelled",
        _ => "Saved",
    }
}

/// `getInvoiceStatus`.
fn invoice_status(values: &DocValueMap, submitted: bool, cancelled: bool) -> &'static str {
    let truthy = |fieldname: &str| is_truthy(values.get(fieldname));
    let money = |fieldname: &str| match values.get(fieldname) {
        Some(DocValue::Money(money)) => Some(money.clone()),
        _ => None,
    };
    let outstanding = money("outstandingAmount");
    let grand_total = money("grandTotal");
    let open = submitted && !cancelled;
    if open && truthy("returnAgainst") {
        return "Return";
    }
    if open && truthy("isReturned") {
        return "ReturnIssued";
    }
    if open && outstanding.as_ref().is_some_and(|o| o.is_zero()) {
        return "Paid";
    }
    let equals_total = match (&outstanding, &grand_total) {
        (Some(o), Some(g)) => o.eq(g).unwrap_or(false),
        _ => false,
    };
    if open && equals_total {
        return "Unpaid";
    }
    if cancelled {
        return "Cancelled";
    }
    if open && outstanding.as_ref().is_some_and(|o| o.is_positive()) && !equals_total {
        return "PartlyPaid";
    }
    "Saved"
}

/// `Status` for a status name.
pub fn status(status: &str) -> Status {
    Status {
        label: status_text(status).to_owned(),
        color: status_color(status),
    }
}

/// The badge of a column that isn't a field.
fn badge(column: &ListColumn, schema: &Schema, values: &DocValueMap) -> Status {
    let truthy = |fieldname: &str| is_truthy(values.get(fieldname));
    match column {
        ListColumn::Enabled if truthy("isEnabled") => Status {
            label: "Enabled".to_owned(),
            color: "green",
        },
        ListColumn::Enabled => Status {
            label: "Disabled".to_owned(),
            color: "orange",
        },
        ListColumn::EnabledFor => Status {
            label: match (truthy("isSales"), truthy("isPurchase")) {
                (true, true) => "Sales and Purchase",
                (true, false) => "Sales",
                (false, true) => "Purchase",
                (false, false) => "None",
            }
            .to_owned(),
            color: "gray",
        },
        _ if schema.name == "SerialNumber" => {
            let status = match values.get("status") {
                Some(DocValue::Text(status)) => status.as_str(),
                _ => "Inactive",
            };
            Status {
                label: status.to_owned(),
                color: status_color(status),
            }
        }
        _ => self::status(doc_status(schema, values)),
    }
}

/// A row of the list.
#[derive(Debug, Clone)]
pub struct ListRow {
    pub name: String,
    pub cells: Vec<ListCell>,
}

#[derive(Debug, Clone)]
pub enum ListCell {
    Text(String),
    Status(Status),
}

/// `updateData`: the documents passing `filters` whose name or title contains `search`,
/// newest first (by `date`, then `created`).
pub fn rows(
    fyo: &Fyo,
    formatter: &Formatter,
    schema_name: &str,
    filters: &QueryFilter,
    search: &str,
) -> Result<Vec<ListRow>, Error> {
    let Some(schema) = fyo.schema(schema_name).cloned() else {
        return Ok(Vec::new());
    };
    let columns = columns(fyo, schema_name);
    let mut order_by = vec!["created".to_owned()];
    if schema.field("date").is_some() {
        order_by.insert(0, "date".to_owned());
    }
    let options = GetAllOptions {
        fields: Some(vec!["*".to_owned()]),
        filters: filters.clone(),
        order_by: Some(order_by),
        ..GetAllOptions::default()
    };
    let needle = search.trim().to_lowercase();
    let title_field = schema
        .title_field
        .clone()
        .unwrap_or_else(|| "name".to_owned());
    let mut rows = Vec::new();
    for values in fyo.db.get_all_docs(schema_name, &options)? {
        let name = match values.get("name") {
            Some(DocValue::Text(name)) => name.clone(),
            _ => continue,
        };
        if !needle.is_empty() {
            let title = match values.get(&title_field) {
                Some(DocValue::Text(title)) => title.to_lowercase(),
                _ => String::new(),
            };
            if !name.to_lowercase().contains(&needle) && !title.contains(&needle) {
                continue;
            }
        }
        let cells = columns
            .iter()
            .map(|column| match column {
                ListColumn::Field(f) => {
                    ListCell::Text(field::display(formatter, f, values.get(&f.fieldname)))
                }
                column => ListCell::Status(badge(column, &schema, &values)),
            })
            .collect();
        rows.push(ListRow { name, cells });
    }
    Ok(rows)
}

/// `routeFilters` and the sidebar's filters, by name.
pub fn route_filters(name: &str) -> QueryFilter {
    let one = |field: &str, value: &str| {
        QueryFilter::from([(field.to_owned(), Filter::Equals(value.into()))])
    };
    let any = |field: &str, values: &[&str]| {
        QueryFilter::from([(
            field.to_owned(),
            Filter::list("in", values.iter().map(|v| FilterValue::from(*v)).collect()),
        )])
    };
    match name {
        "SalesItems" => any("for", &["Sales", "Both"]),
        "PurchaseItems" => any("for", &["Purchases", "Both"]),
        "Items" => one("for", "Both"),
        "PurchasePayments" => one("referenceType", "PurchaseInvoice"),
        "SalesPayments" => one("referenceType", "SalesInvoice"),
        "Suppliers" => any("role", &["Supplier", "Both"]),
        "Customers" => any("role", &["Customer", "Both"]),
        "Party" => any("role", &["Customer", "Supplier", "Both"]),
        _ => QueryFilter::new(),
    }
}

/// `createFilters`: the values a document made from a filtered list starts with.
pub fn create_values(name: &str) -> DocValueMap {
    let (field, value) = match name {
        "SalesItems" => ("for", "Sales"),
        "PurchaseItems" => ("for", "Purchases"),
        "Items" => ("for", "Both"),
        "PurchasePayments" => ("paymentType", "Pay"),
        "SalesPayments" => ("paymentType", "Receive"),
        "Suppliers" => ("role", "Supplier"),
        "Customers" => ("role", "Customer"),
        "Party" => ("role", "Both"),
        _ => return DocValueMap::new(),
    };
    DocValueMap::from([(field.to_owned(), DocValue::Text(value.to_owned()))])
}
