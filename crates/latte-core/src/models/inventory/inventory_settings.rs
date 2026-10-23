// Port of models/inventory/InventorySettings.ts @ v0.36.0.

use latte_db::{Filter, QueryFilter};

use crate::fyo::model::helpers::is_truthy;
use crate::fyo::model::types::{DocContext, Model};
use crate::models::helpers::{query, single_is_truthy};

pub struct InventorySettings;

impl Model for InventorySettings {
    fn link_filters(&self, fieldname: &str, _ctx: &DocContext<'_>) -> Option<QueryFilter> {
        let account_type = match fieldname {
            "stockInHand" => "Stock",
            "stockReceivedButNotBilled" => "Stock Received But Not Billed",
            "costOfGoodsSold" => "Cost of Goods Sold",
            _ => return None,
        };
        Some(query([
            ("isGroup", Filter::from(false)),
            ("accountType", Filter::from(account_type)),
        ]))
    }

    /// A feature can't be turned off once it's on, nor the point of sale while a shift is
    /// open.
    fn read_only(&self, fieldname: &str, ctx: &DocContext<'_>) -> Option<bool> {
        match fieldname {
            "enableBarcodes"
            | "enableBatches"
            | "enableSerialNumber"
            | "enableUomConversions"
            | "enableStockReturns" => Some(is_truthy(ctx.doc.get(fieldname))),
            "enablePointOfSale" => Some(single_is_truthy(ctx.fyo, "POSSettings", "isShiftOpen")),
            _ => None,
        }
    }
}
