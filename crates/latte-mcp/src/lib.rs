//! Dime, the AI layer: an MCP server over `latte-core`, and the setup that connects AI
//! assistants to it (docs/PLAN.md, "Dime").
//!
//! Every write goes through the same `Doc` sync, submit and cancel checks as the app.

pub mod assistants;
pub mod server;
mod tools;
mod values;
