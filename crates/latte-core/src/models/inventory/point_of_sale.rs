// Port of models/inventory/Point of Sale/ @ v0.36.0: POSSettings, POSOpeningShift,
// POSClosingShift and ClosingAmounts. The rest of the point of sale, in
// src/utils/pos.ts and src/pages/POS/, is in `crate::pos`.

use latte_db::pesa::Money;
use latte_db::{DocValue, Filter, QueryFilter};

use crate::Error;
use crate::fyo::Fyo;
use crate::fyo::model::doc::Doc;
use crate::fyo::model::helpers::is_truthy;
use crate::fyo::model::types::{DocContext, Formula, FormulaValue, Model};
use crate::models::helpers::{money, number, query, single_is_truthy};

pub struct PosSettings;

impl Model for PosSettings {
    fn link_filters(&self, fieldname: &str, _ctx: &DocContext<'_>) -> Option<QueryFilter> {
        match fieldname {
            "cashAccount" => Some(query([
                ("rootType", Filter::from("Asset")),
                ("accountType", Filter::from("Cash")),
                ("isGroup", Filter::from(false)),
            ])),
            "defaultAccount" => Some(query([
                ("isGroup", Filter::from(false)),
                ("accountType", Filter::from("Receivable")),
            ])),
            _ => None,
        }
    }

    fn hidden(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<bool> {
        let barcodes = single_is_truthy(ctx.fyo, "InventorySettings", "enableBarcodes");
        match fieldname {
            "weightEnabledBarcode" => Some(!barcodes),
            "checkDigits" | "itemCodeDigits" | "itemWeightDigits" => {
                Some(!barcodes || !is_truthy(ctx.doc.get("weightEnabledBarcode")))
            }
            "itemVisibility" => Some(!single_is_truthy(
                ctx.fyo,
                "AccountingSettings",
                "enablePointOfSaleWithOutInventory",
            )),
            _ => None,
        }
    }
}

/// `openingCashAmount` and `closingCashAmount`: the sum of each denomination times its
/// count in a shift's cash table.
pub fn cash_amount(doc: &Doc, table: &str, fyo: &Fyo) -> Result<Money, Error> {
    let mut amount = fyo.pesa().zero();
    for row in doc.table(table) {
        let denomination = money(row, "denomination").unwrap_or_else(|| fyo.pesa().zero());
        let count = number(row, "count").unwrap_or(0.0);
        amount = amount.add(&denomination.mul(count)?)?;
    }
    Ok(amount)
}

pub struct ClosingAmounts;

impl Model for ClosingAmounts {
    fn formulas(&self) -> Vec<Formula> {
        vec![Formula {
            fieldname: "differenceAmount",
            depends_on: None,
            formula: |ctx, _| {
                let difference = match (
                    money(ctx.doc, "closingAmount"),
                    money(ctx.doc, "expectedAmount"),
                ) {
                    (Some(closing), Some(expected)) => closing.sub(&expected)?,
                    _ => ctx.fyo.pesa().zero(),
                };
                Ok(Some(FormulaValue::Value(DocValue::Money(difference))))
            },
        }]
    }
}
