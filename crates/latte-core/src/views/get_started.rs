//! The Get Started page's cards and checks: `src/utils/getStartedConfig.ts` and the
//! script of `src/pages/GetStarted.vue` @ v0.36.0.

use latte_db::{DocValue, Filter, GetAllOptions, QueryFilter};

use crate::views::sidebar::Route;
use crate::{Error, Fyo};

/// `GetStartedConfigItem['items'][number]`.
#[derive(Debug, Clone)]
pub struct Card {
    pub key: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    /// The GetStarted field that marks it done.
    pub fieldname: &'static str,
    /// What its Set Up button opens.
    pub action: Option<Route>,
    pub documentation: Option<&'static str>,
}

/// `GetStartedConfigItem`.
#[derive(Debug, Clone)]
pub struct Section {
    pub label: &'static str,
    pub items: Vec<Card>,
}

fn card(
    key: &'static str,
    label: &'static str,
    description: &'static str,
    fieldname: &'static str,
    action: Option<Route>,
    documentation: Option<&'static str>,
) -> Card {
    Card {
        key,
        label,
        description,
        fieldname,
        action,
        documentation,
    }
}

/// `getGetStartedConfig`. The labels are English; the page translates them.
pub fn sections() -> Vec<Section> {
    // `openSettings`: Settings at a tab.
    let settings = |tab: &str| {
        Some(Route::Settings {
            tab: Some(tab.to_owned()),
        })
    };
    let list = |schema: &str, title: &str, filters: Option<&str>| {
        Some(Route::list(schema, title, filters))
    };
    vec![
        Section {
            label: "Organisation",
            items: vec![
                card(
                    "General",
                    "General",
                    "Set up your company information, email, country and fiscal year",
                    "companySetup",
                    settings("AccountingSettings"),
                    None,
                ),
                card(
                    "Print",
                    "Print",
                    "Customize your invoices by adding a logo and address details",
                    "printSetup",
                    settings("PrintSettings"),
                    None,
                ),
                card(
                    "System",
                    "System",
                    "Setup system defaults like date format and display precision",
                    "systemSetup",
                    settings("SystemSettings"),
                    None,
                ),
            ],
        },
        Section {
            label: "Accounts",
            items: vec![
                card(
                    "Review Accounts",
                    "Review Accounts",
                    "Review your chart of accounts, add any account or tax heads as needed",
                    "chartOfAccountsReviewed",
                    Some(Route::ChartOfAccounts),
                    Some("https://docs.frappe.io/books/chart-of-accounts"),
                ),
                card(
                    "Opening Balances",
                    "Opening Balances",
                    "Set up your opening balances before performing any accounting entries",
                    "openingBalanceChecked",
                    None,
                    Some("https://docs.frappe.io/books/setup-opening-balances"),
                ),
                card(
                    "Add Taxes",
                    "Add Taxes",
                    "Set up your tax templates for your sales or purchase transactions",
                    "taxesAdded",
                    list("Tax", "Taxes", None),
                    Some("https://docs.frappe.io/books/create-initial-entries#add-taxes"),
                ),
            ],
        },
        Section {
            label: "Sales",
            items: vec![
                card(
                    "Add Sales Items",
                    "Add Items",
                    "Add products or services that you sell to your customers",
                    "salesItemCreated",
                    list("Item", "Sales Items", Some(r#"{"for":"Sales"}"#)),
                    Some("https://docs.frappe.io/books/create-initial-entries#add-sales-items"),
                ),
                card(
                    "Add Customers",
                    "Add Customers",
                    "Add a few customers to create your first sales invoice",
                    "customerCreated",
                    list("Party", "Customers", Some(r#"{"role":"Customer"}"#)),
                    Some("https://docs.frappe.io/books/create-initial-entries#add-customers"),
                ),
                card(
                    "Create Sales Invoice",
                    "Create Sales Invoice",
                    "Create your first sales invoice for the created customer",
                    "invoiceCreated",
                    list("SalesInvoice", "Sales Invoices", None),
                    Some("https://docs.frappe.io/books/sales-invoices"),
                ),
            ],
        },
        Section {
            label: "Purchase",
            items: vec![
                card(
                    "Add Purchase Items",
                    "Add Items",
                    "Add products or services that you buy from your suppliers",
                    "purchaseItemCreated",
                    list("Item", "Purchase Items", Some(r#"{"for":"Purchases"}"#)),
                    None,
                ),
                card(
                    "Add Suppliers",
                    "Add Suppliers",
                    "Add a few suppliers to create your first purchase invoice",
                    "supplierCreated",
                    list("Party", "Suppliers", Some(r#"{"role":"Supplier"}"#)),
                    None,
                ),
                card(
                    "Create Purchase Invoice",
                    "Create Purchase Invoice",
                    "Create your first purchase invoice from the created supplier",
                    "billCreated",
                    list("PurchaseInvoice", "Purchase Invoices", None),
                    Some(
                        "https://docs.frappe.io/books/purchase-invoices#creating-purchase-invoices",
                    ),
                ),
            ],
        },
    ]
}

/// `isCompleted`.
pub fn is_completed(fyo: &Fyo, fieldname: &str) -> bool {
    crate::fyo::model::helpers::is_truthy(fyo.get_value("GetStarted", None, fieldname).as_ref())
}

/// `updateChecks`: sets GetStarted's checks and saves it.
pub fn update_checks(fyo: &mut Fyo, checks: &[(&str, bool)]) -> Result<(), Error> {
    if checks.is_empty() {
        return Ok(());
    }
    let mut doc = fyo.get_doc("GetStarted", None)?;
    for (fieldname, value) in checks {
        doc.set(fyo, fieldname, DocValue::Bool(*value))?;
    }
    doc.sync(fyo)
}

/// `handleAction`'s checks: what pressing a card's Set Up marks done.
pub fn on_action(fyo: &mut Fyo, card: &Card) -> Result<(), Error> {
    let fieldname = match card.key {
        "Print" | "General" | "System" | "Review Accounts" | "Add Taxes" => card.fieldname,
        _ => return Ok(()),
    };
    update_checks(fyo, &[(fieldname, true)])
}

/// `handleDocumentation`'s checks: reading the opening balances guide marks it done.
pub fn on_documentation(fyo: &mut Fyo, card: &Card) -> Result<(), Error> {
    if card.key == "Opening Balances" {
        update_checks(fyo, &[("openingBalanceChecked", true)])?;
    }
    Ok(())
}

fn exists(fyo: &Fyo, schema_name: &str, filter: Option<(&str, &str)>) -> Result<bool, Error> {
    let filters: QueryFilter = filter
        .map(|(field, value)| (field.to_owned(), Filter::from(value)))
        .into_iter()
        .collect();
    let options = GetAllOptions {
        filters,
        limit: Some(1),
        ..GetAllOptions::default()
    };
    Ok(!fyo.db.get_all_docs(schema_name, &options)?.is_empty())
}

/// `checkIsOnboardingComplete`: when every check is done, onboarding is marked complete
/// and Get Started is hidden from the sidebar.
fn check_is_onboarding_complete(fyo: &mut Fyo) -> Result<bool, Error> {
    if is_completed(fyo, "onboardingComplete") {
        return Ok(true);
    }
    let Some(schema) = fyo.schema("GetStarted") else {
        return Ok(false);
    };
    let complete = schema
        .fields
        .iter()
        .filter(|f| f.fieldname != "onboardingComplete")
        .all(|f| is_completed(fyo, &f.fieldname));
    if complete {
        update_checks(fyo, &[("onboardingComplete", true)])?;
        let mut settings = fyo.get_doc("SystemSettings", None)?;
        settings.set(fyo, "hideGetStarted", DocValue::Bool(true))?;
        settings.sync(fyo)?;
    }
    Ok(complete)
}

/// `checkForCompletedTasks`: marks the cards whose documents have been made.
///
/// Difference from Books: "Create Purchase Invoice" is done once a purchase invoice is
/// made. Books looks for a sales invoice (a Books bug).
pub fn check_for_completed_tasks(fyo: &mut Fyo) -> Result<(), Error> {
    if check_is_onboarding_complete(fyo)? {
        return Ok(());
    }
    let checks = [
        ("salesItemCreated", "Item", Some(("for", "Sales"))),
        ("purchaseItemCreated", "Item", Some(("for", "Purchases"))),
        ("invoiceCreated", "SalesInvoice", None::<(&str, &str)>),
        ("customerCreated", "Party", Some(("role", "Customer"))),
        ("billCreated", "PurchaseInvoice", None),
        ("supplierCreated", "Party", Some(("role", "Supplier"))),
    ];
    let mut to_update = Vec::new();
    for (fieldname, schema_name, filter) in checks {
        if !is_completed(fyo, fieldname) {
            to_update.push((fieldname, exists(fyo, schema_name, filter)?));
        }
    }
    // Books sets the unmade ones to false again, which changes nothing.
    to_update.retain(|(_, made)| *made);
    update_checks(fyo, &to_update)
}

#[cfg(test)]
mod tests {
    use latte_db::DocValue;

    use super::*;
    use crate::setup_tests::{TempFile, indian_company};
    use crate::tests::{map, text};

    #[test]
    fn a_purchase_invoice_marks_its_card_done_and_a_sales_invoice_doesnt() {
        let file = TempFile::new("get started");
        let mut fyo = indian_company(&file);
        check_for_completed_tasks(&mut fyo).unwrap();
        assert!(!is_completed(&fyo, "supplierCreated"));

        let values = map([("name", text("Ink Co")), ("role", text("Supplier"))]);
        fyo.get_new_doc("Party", values)
            .unwrap()
            .sync(&mut fyo)
            .unwrap();
        let values = map([
            ("name", text("Ink")),
            ("for", text("Purchases")),
            ("rate", DocValue::Number(50.0)),
        ]);
        fyo.get_new_doc("Item", values)
            .unwrap()
            .sync(&mut fyo)
            .unwrap();
        check_for_completed_tasks(&mut fyo).unwrap();
        assert!(is_completed(&fyo, "supplierCreated"));
        assert!(is_completed(&fyo, "purchaseItemCreated"));
        assert!(!is_completed(&fyo, "salesItemCreated"));
        assert!(!is_completed(&fyo, "billCreated"));

        let values = map([("party", text("Ink Co")), ("account", text("Creditors"))]);
        let mut bill = fyo.get_new_doc("PurchaseInvoice", values).unwrap();
        let row = map([("item", text("Ink")), ("quantity", DocValue::Number(2.0))]);
        bill.append(&mut fyo, "items", row).unwrap();
        bill.sync(&mut fyo).unwrap();
        check_for_completed_tasks(&mut fyo).unwrap();
        assert!(is_completed(&fyo, "billCreated"));
        assert!(!is_completed(&fyo, "invoiceCreated"));
    }

    #[test]
    fn doing_every_card_hides_get_started() {
        let file = TempFile::new("get started done");
        let mut fyo = indian_company(&file);
        let fields: Vec<String> = fyo
            .schema("GetStarted")
            .unwrap()
            .fields
            .iter()
            .map(|f| f.fieldname.clone())
            .filter(|f| f != "onboardingComplete")
            .collect();
        let checks: Vec<(&str, bool)> = fields.iter().map(|f| (f.as_str(), true)).collect();
        update_checks(&mut fyo, &checks).unwrap();
        check_for_completed_tasks(&mut fyo).unwrap();
        assert!(is_completed(&fyo, "onboardingComplete"));
        assert!(crate::fyo::model::helpers::is_truthy(
            fyo.get_value("SystemSettings", None, "hideGetStarted")
                .as_ref()
        ));
    }
}
