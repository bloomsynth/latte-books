//! Help: the documentation page for what is shown, `docsPathMap` of `src/utils/misc.ts`
//! and the pages that set `docsPathRef` @ v0.36.0. Help (and F1) opens
//! `https://docs.frappe.io/<path>`.

use crate::views::sidebar::Route;

/// Where Books' documentation is.
pub const DOCS_URL: &str = "https://docs.frappe.io/";

/// `docsPathMap`.
pub fn docs_path_map(key: &str) -> Option<&'static str> {
    Some(match key {
        "Dashboard" => "books/dashboard",
        "Reports" => "books/reports",
        "GeneralLedger" => "books/general-ledger",
        "ProfitAndLoss" => "books/profit-and-loss",
        "BalanceSheet" => "books/balance-sheet",
        "TrialBalance" => "books/trial-balance",
        "SalesInvoice" => "books/sales-invoices",
        "PurchaseInvoice" => "books/purchase-invoices",
        "Payment" => "books/payments",
        "JournalEntry" => "books/journal-entries",
        "StockMovement" => "books/stock-movement",
        "Shipment" => "books/shipment",
        "PurchaseReceipt" => "books/purchase-receipt",
        "StockLedger" => "books/stock-ledger",
        "StockBalance" => "books/stock-balance",
        "Batch" => "books/batches",
        "Entries" => "books/books",
        "Party" => "books/party",
        "Item" => "books/items",
        "Tax" => "books/taxes",
        "PrintTemplate" => "books/print-templates",
        "Search" => "books/quick-search",
        "NumberSeries" => "books/number-series",
        "ImportWizard" => "books/import-wizard",
        "Settings" => "books/settings",
        "ChartOfAccounts" => "books/chart-of-accounts",
        _ => return None,
    })
}

/// A report's class name (`reports/index.ts`), from its route name.
fn report_class(name: &str) -> &str {
    match name {
        "general-ledger" => "GeneralLedger",
        "profit-and-loss" => "ProfitAndLoss",
        "balance-sheet" => "BalanceSheet",
        "trial-balance" => "TrialBalance",
        "stock-ledger" => "StockLedger",
        "stock-balance" => "StockBalance",
        "gstr-1" => "GSTR1",
        "gstr-2" => "GSTR2",
        name => name,
    }
}

/// `docsPathRef` while a route is shown: a list's schema or Entries, a form's schema,
/// a report or Reports, and the pages Books gives a path; nothing elsewhere.
pub fn docs_path(route: &Route) -> &'static str {
    let path = match route {
        Route::Dashboard => docs_path_map("Dashboard"),
        Route::List { schema, .. } => docs_path_map(schema).or(docs_path_map("Entries")),
        Route::Form { schema, .. } => docs_path_map(schema),
        Route::Report(name) | Route::FilteredReport { name, .. } => {
            docs_path_map(report_class(name)).or(docs_path_map("Reports"))
        }
        Route::ChartOfAccounts => docs_path_map("ChartOfAccounts"),
        Route::ImportWizard => docs_path_map("ImportWizard"),
        Route::Settings { .. } => docs_path_map("Settings"),
        Route::TemplateBuilder { .. } => docs_path_map("PrintTemplate"),
        Route::GetStarted
        | Route::Pos
        | Route::Print { .. }
        | Route::ReportPrint(_)
        | Route::AiAssistants => None,
    };
    path.unwrap_or("")
}

/// `openDocumentation`: the page Help opens for a route.
pub fn docs_url(route: &Route) -> String {
    format!("{DOCS_URL}{}", docs_path(route))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_page_opens_its_documentation() {
        assert_eq!(
            docs_url(&Route::Dashboard),
            "https://docs.frappe.io/books/dashboard"
        );
        assert_eq!(
            docs_path(&Route::list("SalesInvoice", "Sales Invoices", None)),
            "books/sales-invoices"
        );
        // A list without a page of its own is under Entries.
        assert_eq!(
            docs_path(&Route::list("Account", "Accounts", None)),
            "books/books"
        );
        // A form without one has none.
        let form = |schema: &str| Route::Form {
            schema: schema.to_owned(),
            name: None,
        };
        assert_eq!(docs_path(&form("Item")), "books/items");
        assert_eq!(docs_path(&form("Account")), "");
        assert_eq!(
            docs_path(&Route::Report("trial-balance".into())),
            "books/trial-balance"
        );
        assert_eq!(docs_path(&Route::Report("gstr-1".into())), "books/reports");
        assert_eq!(docs_path(&Route::Settings { tab: None }), "books/settings");
        assert_eq!(docs_url(&Route::GetStarted), "https://docs.frappe.io/");
    }
}
