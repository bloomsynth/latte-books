// Port of models/baseModels/Account/Account.ts @ v0.36.0.
//
// Not ported yet, as the UI needs them: `filters`, `getListViewSettings` and
// `getTreeSettings`.

use latte_db::DocValue;

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::helpers::is_truthy;
use crate::fyo::model::types::{DocContext, Formula, Model};

pub struct Account;

/// `isDebit`: asset and expense accounts.
pub fn is_debit(doc: &Doc) -> bool {
    matches!(
        doc.get("rootType"),
        Some(DocValue::Text(root_type)) if root_type == "Asset" || root_type == "Expense"
    )
}

/// `isCredit`.
pub fn is_credit(doc: &Doc) -> bool {
    !is_debit(doc)
}

impl Model for Account {
    fn required(&self, fieldname: &str, ctx: &DocContext<'_>) -> bool {
        // Root accounts have no parent; they are made while setting up.
        fieldname == "parentAccount"
            && is_truthy(
                ctx.fyo
                    .get_value("AccountingSettings", None, "setupComplete")
                    .as_ref(),
            )
    }

    fn default(&self, fieldname: &str, _doc: &Doc, _fyo: &Fyo) -> Option<DocValue> {
        match fieldname {
            "lft" | "rgt" => Some(DocValue::Number(0.0)),
            _ => None,
        }
    }

    fn before_sync(&self, doc: &mut Doc, fyo: &mut Fyo) -> Result<(), Error> {
        let parent_account = match doc.get("parentAccount") {
            Some(DocValue::Text(parent)) if !parent.is_empty() => parent.clone(),
            _ => return Ok(()),
        };
        if is_truthy(doc.get("accountType")) {
            return Ok(());
        }

        let account = fyo.db.get_doc("Account", &parent_account)?;
        let account_type = account
            .get("accountType")
            .cloned()
            .unwrap_or(DocValue::Null);
        doc.set_value("accountType", account_type);
        Ok(())
    }

    fn formulas(&self) -> Vec<Formula> {
        vec![Formula {
            fieldname: "rootType",
            depends_on: None,
            formula: |ctx, _| {
                let Some(DocValue::Text(parent)) = ctx.doc.get("parentAccount") else {
                    return Ok(None);
                };
                if parent.is_empty() {
                    return Ok(None);
                }
                Ok(ctx
                    .fyo
                    .get_value("Account", Some(parent), "rootType")
                    .map(Into::into))
            },
        }]
    }

    fn read_only(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<bool> {
        let inserted = ctx.doc.inserted();
        match fieldname {
            "rootType" | "parentAccount" | "isGroup" => Some(inserted),
            "accountType" => Some(is_truthy(ctx.doc.get("accountType")) && inserted),
            _ => None,
        }
    }
}
