//! Phase 0 spike 1: how long from launch until the first frame of a window is painted?
//!
//! Opens a window shaped like the Frappe Books desk (sidebar + invoice list), then prints
//! timings measured from the start of `main`. With `--exit`, it quits after the first frame
//! so a script can time many launches (see `measure.ps1`).

use std::time::Instant;

use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;

// Sidebar groups as in Books v0.36.0 (src/utils/sidebarConfig.ts).
const SIDEBAR: &[&str] = &[
    "Get Started",
    "Dashboard",
    "Sales",
    "Purchases",
    "Common",
    "Reports",
    "Inventory",
    "POS",
    "Setup",
];

struct Desk {
    started: Instant,
    exit_after_first_frame: bool,
    first_render_logged: bool,
}

impl Render for Desk {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.first_render_logged {
            self.first_render_logged = true;
            let started = self.started;
            let exit = self.exit_after_first_frame;
            eprintln!("first render:  {:>7.1} ms", ms(started));
            window.on_next_frame(move |_, cx| {
                println!("first frame:   {:>7.1} ms", ms(started));
                if exit {
                    cx.quit();
                }
            });
        }

        let theme = cx.theme();
        let sidebar = div()
            .v_flex()
            .w(px(200.))
            .h_full()
            .p_2()
            .gap_1()
            .bg(theme.sidebar)
            .child(div().p_2().text_lg().child("Latte Books"))
            .children(
                SIDEBAR
                    .iter()
                    .map(|label| div().px_2().py_1().child(*label)),
            );

        let rows = (1..=40).map(|i| {
            div()
                .h_flex()
                .gap_4()
                .px_4()
                .py_1()
                .border_b_1()
                .border_color(theme.border)
                .child(div().w(px(140.)).child(format!("SINV-{i:04}")))
                .child(div().w(px(200.)).child(format!("Customer {i}")))
                .child(div().w(px(120.)).child("2026-10-21"))
                .child(div().w(px(120.)).child(format!("₹ {}.00", i * 1250)))
        });

        let main = div()
            .v_flex()
            .flex_1()
            .h_full()
            .child(
                div()
                    .h_flex()
                    .justify_between()
                    .p_4()
                    .child(div().text_xl().child("Sales Invoice"))
                    .child(Button::new("new").primary().label("+ New")),
            )
            .children(rows);

        div()
            .h_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(sidebar)
            .child(main)
    }
}

fn ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

fn main() {
    let started = Instant::now();
    let exit_after_first_frame = std::env::args().any(|a| a == "--exit");

    let app = gpui_kit::application();
    eprintln!("platform:      {:>7.1} ms", ms(started));
    app.run(move |cx| {
        eprintln!("app ready:     {:>7.1} ms", ms(started));
        gpui_kit::init(cx);

        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, move |_, cx| {
            cx.new(|_| Desk {
                started,
                exit_after_first_frame,
                first_render_logged: false,
            })
        })
        .expect("failed to open window");
        eprintln!("window opened: {:>7.1} ms", ms(started));
        cx.activate(true);
    });
}
