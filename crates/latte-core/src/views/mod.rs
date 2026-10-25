//! What the app's screens show, without any UI types: the sidebar, list views and how a
//! form lays out, shows and reads its fields. `latte-ui` draws these; Dime reuses them.

pub mod field;
pub mod form;
#[cfg(test)]
mod form_tests;
pub mod get_started;
pub mod list;
pub mod search;
pub mod sidebar;
