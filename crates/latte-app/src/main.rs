//! Latte Books desktop app entry point.
//!
//! `latte-books --print-job` lays out a print job from `latte-mcp` and exits without
//! opening a window (see `latte_print::job`).

// No console window behind the app on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args().nth(1).as_deref() == Some(latte_print::job::FLAG) {
        let code = latte_print::job::serve(std::io::stdin().lock(), std::io::stdout().lock());
        std::process::exit(code);
    }
    latte_ui::run();
}
