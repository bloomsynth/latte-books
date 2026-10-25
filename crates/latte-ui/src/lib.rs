//! GPUI screens and components for Latte Books.
//!
//! The only crate (with `latte-app`) allowed to depend on GPUI and gpui-kit. What a screen
//! shows is worked out in `latte_core::views`, so it can be tested without a window.

mod app;
mod assistants_page;
mod attach_image;
mod books;
mod coa;
mod dashboard;
mod date_picker;
mod desk;
mod export_wizard;
mod filter_dropdown;
mod form;
mod get_started;
mod i18n;
mod import_wizard;
mod link;
mod list_page;
mod placeholder;
mod pos_link;
mod pos_page;
mod pos_parts;
mod print;
mod report_page;
mod search_bar;
mod selector;
mod session;
mod settings;
mod setup_wizard;
mod shortcuts;
mod widgets;

use gpui_kit::*;
use latte_core::Doc;
use latte_core::views::sidebar::Route;

pub use session::{config_dir, recent_files};

/// Where a screen asks the desk to go: a route, or a document not yet saved (a payment
/// made from an invoice, a copy).
#[derive(Clone)]
pub enum Nav {
    Route(Route),
    Doc(Doc),
    /// The company opened again, then the route: a saved Customize Form changes the
    /// schemas, which are built when a file is opened.
    Reopen(Route),
}

/// A screen's request to go somewhere.
pub struct Navigate(pub Nav);

/// Opens the main window and runs the app until it is closed.
pub fn run() {
    // Connected assistants follow the app when it is updated or moved.
    if let Some(server) = latte_mcp::assistants::server_path() {
        latte_mcp::assistants::update_entries(&server);
    }
    gpui_kit::application()
        .with_assets(books::Assets)
        .run(|cx: &mut App| {
            i18n::load();
            gpui_kit::init(cx);
            books::init(cx);
            let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Latte Books".into()),
                    ..Default::default()
                }),
                window_min_size: Some(size(px(900.), px(600.))),
                // Matches the installed `latte-books.desktop`, so Linux shows the app's icon.
                app_id: Some("latte-books".into()),
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| app::LatteApp::new(window, cx))
            })
            .ok();
            cx.activate(true);
        });
}
