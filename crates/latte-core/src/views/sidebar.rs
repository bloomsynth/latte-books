// Port of src/utils/sidebarConfig.ts @ v0.36.0: the sidebar's groups and items, and which
// of them a company's settings hide.
//
// Differences from Books: there is no Setup group. Chart of Accounts and Tax Templates are
// in Common, with the other records; Import Wizard, Print Templates, Customize Form and AI
// Assistants are a Tools group; Settings is at the sidebar's foot, with Help.

use crate::fyo::Fyo;
use crate::models::helpers::single_is_truthy;
use crate::reports::gstr::is_gst_registered;

/// Where a sidebar item goes. Books' routes, as values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    GetStarted,
    Dashboard,
    /// `/list/<schema>/<title>`, with the `routeFilters` named `filters`, and the
    /// `query.filters` given as (field, operator, value), such as a party's invoices.
    List {
        schema: String,
        title: String,
        filters: Option<String>,
        conditions: Vec<(String, String, String)>,
    },
    /// `/report/<name>`.
    Report(String),
    /// `/report/<name>` with `defaultFilters`, such as a document's ledger entries.
    FilteredReport {
        name: String,
        filters: Vec<(String, String)>,
    },
    ChartOfAccounts,
    ImportWizard,
    /// `/settings`, at a tab: the settings schema it shows.
    Settings {
        tab: Option<String>,
    },
    Pos,
    /// `/edit/<schema>/<name>`; no name for a new document.
    Form {
        schema: String,
        name: Option<String>,
    },
    /// `/print/<schema>/<name>`.
    Print {
        schema: String,
        name: String,
    },
    /// `/report-print/<name>`: a report's Print View.
    ReportPrint(String),
    /// `/template-builder/<name>`: a PrintTemplate's form (`formRoute`); no name for a
    /// new one.
    TemplateBuilder {
        name: Option<String>,
    },
    /// Latte's own: the "Set up AI assistants" screen (Dime).
    AiAssistants,
}

impl Route {
    pub fn list(schema: &str, title: &str, filters: Option<&str>) -> Self {
        Self::List {
            schema: schema.to_owned(),
            title: title.to_owned(),
            filters: filters.map(str::to_owned),
            conditions: Vec::new(),
        }
    }

    /// A list filtered as `query.filters` filters it: `[(field, operator, value)]`.
    pub fn filtered_list(schema: &str, title: &str, conditions: &[(&str, &str, &str)]) -> Self {
        Self::List {
            schema: schema.to_owned(),
            title: title.to_owned(),
            filters: None,
            conditions: conditions
                .iter()
                .map(|(f, o, v)| ((*f).to_owned(), (*o).to_owned(), (*v).to_owned()))
                .collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SidebarItem {
    pub label: String,
    pub route: Route,
}

#[derive(Debug, Clone)]
pub struct SidebarGroup {
    pub label: String,
    pub icon: &'static str,
    pub route: Route,
    pub items: Vec<SidebarItem>,
}

fn item(label: &str, route: Route) -> SidebarItem {
    SidebarItem {
        label: label.to_owned(),
        route,
    }
}

fn group(label: &str, icon: &'static str, route: Route, items: Vec<SidebarItem>) -> SidebarGroup {
    SidebarGroup {
        label: label.to_owned(),
        icon,
        route,
        items,
    }
}

/// `getSidebarConfig`: the sidebar for the open company, without what its settings hide.
pub fn sidebar(fyo: &Fyo) -> Vec<SidebarGroup> {
    let setting = |schema: &str, field: &str| single_is_truthy(fyo, schema, field);
    let accounting = |field: &str| setting("AccountingSettings", field);
    let mut groups = Vec::new();

    if !setting("SystemSettings", "hideGetStarted") {
        groups.push(group("Get Started", "general", Route::GetStarted, vec![]));
    }
    groups.push(group("Dashboard", "dashboard", Route::Dashboard, vec![]));

    let mut sales = vec![
        item(
            "Sales Quotes",
            Route::list("SalesQuote", "Sales Quotes", None),
        ),
        item(
            "Sales Invoices",
            Route::list("SalesInvoice", "Sales Invoices", None),
        ),
        item(
            "Sales Payments",
            Route::list("Payment", "Sales Payments", Some("SalesPayments")),
        ),
        item(
            "Customers",
            Route::list("Party", "Customers", Some("Customers")),
        ),
        item(
            "Sales Items",
            Route::list("Item", "Sales Items", Some("SalesItems")),
        ),
    ];
    for (label, schema, field) in [
        ("Loyalty Program", "LoyaltyProgram", "enableLoyaltyProgram"),
        ("Lead", "Lead", "enableLead"),
        ("Pricing Rule", "PricingRule", "enablePricingRule"),
        ("Coupon Code", "CouponCode", "enableCouponCode"),
    ] {
        if accounting(field) {
            sales.push(item(label, Route::list(schema, label, None)));
        }
    }
    groups.push(group(
        "Sales",
        "sales",
        Route::list("SalesInvoice", "Sales Invoices", None),
        sales,
    ));

    groups.push(group(
        "Purchases",
        "purchase",
        Route::list("PurchaseInvoice", "Purchase Invoices", None),
        vec![
            item(
                "Purchase Invoices",
                Route::list("PurchaseInvoice", "Purchase Invoices", None),
            ),
            item(
                "Purchase Payments",
                Route::list("Payment", "Purchase Payments", Some("PurchasePayments")),
            ),
            item(
                "Suppliers",
                Route::list("Party", "Suppliers", Some("Suppliers")),
            ),
            item(
                "Purchase Items",
                Route::list("Item", "Purchase Items", Some("PurchaseItems")),
            ),
        ],
    ));

    let mut common = vec![
        item(
            "Journal Entry",
            Route::list("JournalEntry", "Journal Entry", None),
        ),
        item("Party", Route::list("Party", "Party", Some("Party"))),
        item("Items", Route::list("Item", "Items", Some("Items"))),
    ];
    if accounting("enablePriceList") {
        common.push(item(
            "Price List",
            Route::list("PriceList", "Price List", None),
        ));
    }
    common.push(item("Chart of Accounts", Route::ChartOfAccounts));
    common.push(item(
        "Tax Templates",
        Route::list("Tax", "Tax Templates", None),
    ));
    groups.push(group(
        "Common",
        "common-entries",
        Route::list("JournalEntry", "Journal Entry", None),
        common,
    ));

    let report = |label: &str, name: &str| item(label, Route::Report(name.to_owned()));
    groups.push(group(
        "Reports",
        "reports",
        Route::Report("general-ledger".to_owned()),
        vec![
            report("General Ledger", "general-ledger"),
            report("Profit And Loss", "profit-and-loss"),
            report("Balance Sheet", "balance-sheet"),
            report("Trial Balance", "trial-balance"),
        ],
    ));

    if accounting("enableInventory") {
        groups.push(group(
            "Inventory",
            "inventory",
            Route::list("StockMovement", "Stock Movement", None),
            vec![
                item(
                    "Stock Movement",
                    Route::list("StockMovement", "Stock Movement", None),
                ),
                item("Shipment", Route::list("Shipment", "Shipment", None)),
                item(
                    "Purchase Receipt",
                    Route::list("PurchaseReceipt", "Purchase Receipt", None),
                ),
                report("Stock Ledger", "stock-ledger"),
                report("Stock Balance", "stock-balance"),
            ],
        ));
    }

    if setting("InventorySettings", "enablePointOfSale") {
        groups.push(group("POS", "pos", Route::Pos, vec![]));
    }

    if is_gst_registered(fyo) {
        groups.push(group(
            "GST",
            "gst",
            Route::Report("gstr-1".to_owned()),
            vec![report("GSTR1", "gstr-1"), report("GSTR3B", "gstr-3b")],
        ));
    }

    let mut tools = vec![
        item("Import Wizard", Route::ImportWizard),
        item(
            "Print Templates",
            Route::list("PrintTemplate", "Print Templates", None),
        ),
    ];
    if accounting("enableFormCustomization") {
        tools.push(item(
            "Customize Form",
            Route::list("CustomForm", "Customize Form", None),
        ));
    }
    // Latte's own (Dime): connect AI assistants to this company.
    tools.push(item("AI Assistants", Route::AiAssistants));
    groups.push(group("Tools", "settings", Route::ImportWizard, tools));
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup_tests::{TempFile, indian_company};
    use crate::tests::{map, text};

    fn items<'a>(groups: &'a [SidebarGroup], label: &str) -> Vec<&'a str> {
        groups
            .iter()
            .find(|g| g.label == label)
            .map(|g| g.items.iter().map(|i| i.label.as_str()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn setup_is_in_common_and_tools_and_settings_is_at_the_foot() {
        let file = TempFile::new("sidebar");
        let mut fyo = indian_company(&file);
        let groups = sidebar(&fyo);
        let labels: Vec<&str> = groups.iter().map(|g| g.label.as_str()).collect();
        assert!(!labels.contains(&"Setup"));
        assert_eq!(labels.last(), Some(&"Tools"));
        assert_eq!(
            items(&groups, "Common"),
            [
                "Journal Entry",
                "Party",
                "Items",
                "Chart of Accounts",
                "Tax Templates"
            ]
        );
        assert_eq!(
            items(&groups, "Tools"),
            ["Import Wizard", "Print Templates", "AI Assistants"]
        );
        let settings = groups
            .iter()
            .flat_map(|g| &g.items)
            .any(|i| matches!(i.route, Route::Settings { .. }));
        assert!(!settings);

        // The GST returns, once the company has a GSTIN.
        assert!(!labels.contains(&"GST"));
        fyo.db
            .update(
                "AccountingSettings",
                &map([("gstin", text("29AABCA1234A1Z5"))]),
            )
            .unwrap();
        assert_eq!(items(&sidebar(&fyo), "GST"), ["GSTR1", "GSTR3B"]);
    }
}
