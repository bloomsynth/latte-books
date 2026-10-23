// Port of models/inventory/ @ v0.36.0: stock movements, shipments and purchase receipts,
// the stock ledger entries they make, and the point of sale's models.
//
// Not ported yet: the list view settings and actions of Batch, Location, SerialNumber,
// StockLedgerEntry, Shipment and PurchaseReceipt, which the UI needs.

pub mod helpers;
pub mod inventory_settings;
pub mod point_of_sale;
pub mod stock_manager;
pub mod stock_movement;
pub mod stock_movement_item;
pub mod stock_queue;
pub mod stock_transfer;
pub mod stock_transfer_item;
pub mod transfer;
pub mod transfer_item;
