// Port of models/baseModels/PrintTemplate.ts @ v0.36.0.
//
// The Template Builder's `formRoute` from `getListViewSettings` is the UI's: the
// PrintTemplate list opens the Template Builder.

use latte_db::DocValue;
use latte_schema::SelectOption;

use crate::fyo::model::doc::Doc;
use crate::fyo::model::helpers::is_truthy;
use crate::fyo::model::types::{DocContext, Model};

pub struct PrintTemplate;

/// `lists.type`: the schemas a template can print.
pub const TYPES: [&str; 8] = [
    "SalesInvoice",
    "SalesQuote",
    "PurchaseInvoice",
    "JournalEntry",
    "Payment",
    "Shipment",
    "PurchaseReceipt",
    "StockMovement",
];

/// Whether a PrintTemplate is the user's own: one of Books' or Latte's bundled
/// templates isn't.
pub fn is_custom(doc: &Doc) -> bool {
    is_truthy(doc.get("isCustom"))
}

impl Model for PrintTemplate {
    /// `readOnly`: a bundled template's name, type and template can't be changed.
    fn read_only(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<bool> {
        match fieldname {
            "name" | "type" | "template" => Some(!is_custom(ctx.doc)),
            _ => None,
        }
    }

    fn list(&self, fieldname: &str, ctx: Option<&DocContext<'_>>) -> Option<Vec<SelectOption>> {
        if fieldname != "type" {
            return None;
        }
        Some(
            TYPES
                .iter()
                .map(|value| SelectOption {
                    value: (*value).to_owned(),
                    label: ctx
                        .and_then(|c| c.fyo.schema(value))
                        .map_or_else(|| (*value).to_owned(), |s| s.label.clone()),
                })
                .collect(),
        )
    }

    /// `canDelete`: a bundled template (`isCustom === false`) can't be deleted.
    fn can_delete(&self, doc: &Doc) -> bool {
        !matches!(doc.get("isCustom"), Some(DocValue::Bool(false)))
    }

    /// `duplicate`: a copy is the user's own.
    fn duplicate(&self, doc: &mut Doc) {
        doc.set_value("isCustom", DocValue::Bool(true));
    }
}
