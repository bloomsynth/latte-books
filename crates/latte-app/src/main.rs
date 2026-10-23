//! Latte Books desktop app entry point.

// No console window behind the app on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    latte_ui::run();
}
