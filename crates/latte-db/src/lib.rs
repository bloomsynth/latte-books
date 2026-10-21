//! SQLite access for Frappe Books company files (`.books.db`).
//!
//! Port of `backend/database/` @ v0.36.0. Every write goes through this crate and is
//! recorded in the `latte_changes` table (see docs/PLAN.md §7).
