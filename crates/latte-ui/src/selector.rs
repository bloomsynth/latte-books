//! Choosing a company file: `src/pages/DatabaseSelector.vue` @ v0.36.0.
//!
//! Differences from Books:
//! - Create Demo shows its progress in the selector instead of a full-window loader.
//! - A file's × takes it off the list and leaves it on the disk. Books deletes the file.
//! - Changing the language rebuilds the screen; Books reloads the window.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::books::{self, Text as _, black, gray, white};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::{self, t};
use latte_core::dummy::setup_dummy_instance;

use crate::link::{LinkInput, LinkOption, LinkPicked};
use crate::session::{self, RecentFile};
use crate::widgets::error_message;

pub enum Selected {
    Open(PathBuf),
    New,
}

pub struct Selector {
    files: Vec<RecentFile>,
    error: Option<String>,
    language: Entity<LinkInput>,
    /// `creatingDemo`, with `creationMessage` (untranslated) and `creationPercent`.
    creating_demo: Option<(String, f64)>,
    _subscription: Subscription,
}

impl EventEmitter<Selected> for Selector {}

impl Selector {
    pub fn new(error: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // `LanguageSelector`.
        let options = latte_i18n::translation::LANGUAGES
            .iter()
            .map(|(name, _)| LinkOption {
                value: (*name).into(),
                label: (*name).into(),
            })
            .collect();
        let language = cx.new(|cx| LinkInput::new(options, "Language", window, cx));
        let current = SharedString::from(i18n::language());
        language.update(cx, |link, cx| link.set_value(Some(current), window, cx));
        let subscription = cx.subscribe_in(
            &language,
            window,
            |_: &mut Self, _, event: &LinkPicked, _, cx| {
                i18n::set_language(&event.0);
                cx.notify();
            },
        );
        Self {
            files: session::recent_files(),
            error,
            language,
            creating_demo: None,
            _subscription: subscription,
        }
    }

    fn open_existing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t("Open").into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update(cx, |_, cx| cx.emit(Selected::Open(path))).ok();
        })
        .detach();
    }

    /// `createDemo` and `startDummyInstanceSetup`: asks where to save the demo company,
    /// fills it with a year of documents (Books' `baseCount` of 100) and opens it.
    fn create_demo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.creating_demo.is_some() {
            return;
        }
        let dir = demo_dir();
        let path = cx.prompt_for_new_path(&dir, Some("demo.books.db"));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = path.await else {
                return;
            };
            if path.exists() {
                this.update(cx, |this, cx| {
                    this.error = Some(format!("{} is already there", path.display()));
                    cx.notify();
                })
                .ok();
                return;
            }
            let progress = Arc::new(Mutex::new(("Setting Up Instance".to_owned(), -1.0)));
            let Ok(()) = this.update(cx, |this, cx| {
                this.error = None;
                this.creating_demo = Some(("Setting Up Instance".to_owned(), -1.0));
                cx.notify();
            }) else {
                return;
            };

            let result = Arc::new(Mutex::new(None));
            cx.background_executor()
                .spawn({
                    let path = path.clone();
                    let progress = progress.clone();
                    let result = result.clone();
                    async move {
                        let mut rng = fastrand::Rng::new();
                        let device = latte_core::config::device_id();
                        let mut notifier = |stage: &str, percent: f64| {
                            if let Ok(mut progress) = progress.lock() {
                                *progress = (stage.to_owned(), percent);
                            }
                        };
                        let done =
                            setup_dummy_instance(&path, 1, 100, &device, &mut rng, &mut notifier)
                                .map(|_| ())
                                .map_err(|e| e.to_string());
                        if let Ok(mut result) = result.lock() {
                            *result = Some(done);
                        }
                    }
                })
                .detach();

            // Shows how far along it is until it's done, then opens it.
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                let current = progress.lock().map(|p| p.clone()).ok();
                let done = result.lock().ok().and_then(|mut r| r.take());
                let updated = this.update(cx, |this, cx| {
                    match done {
                        None => {
                            if let (Some(creating), Some(current)) =
                                (this.creating_demo.as_mut(), current)
                            {
                                *creating = current;
                            }
                        }
                        Some(done) => {
                            this.creating_demo = None;
                            match done {
                                Ok(()) => cx.emit(Selected::Open(path.clone())),
                                Err(e) => this.error = Some(e),
                            }
                        }
                    }
                    cx.notify();
                    this.creating_demo.is_some()
                });
                if !matches!(updated, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// A recent file: its number, name, when it changed and where it is.
    fn row(&self, i: usize, file: &RecentFile, cx: &mut Context<Self>) -> impl IntoElement {
        let path = file.path.clone();
        let forget = file.path.clone();
        let name = if file.company_name.is_empty() {
            file.path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        } else {
            file.company_name.clone()
        };
        let modified = std::fs::metadata(&file.path)
            .and_then(|m| m.modified())
            .ok()
            .map(relative_time)
            .unwrap_or_default();
        let shown_path = truncate_path(&file.path.display().to_string());
        div()
            .id(("file", i))
            .h_flex()
            .h_16()
            .px_4()
            .gap_4()
            .cursor_pointer()
            .hover(|d| d.bg(gray(50)))
            .on_click(cx.listener(move |_, _, _, cx| cx.emit(Selected::Open(path.clone()))))
            .child(
                div()
                    .size_8()
                    .flex_none()
                    .rounded_full()
                    .bg(gray(200))
                    .h_flex()
                    .justify_center()
                    .text_13()
                    .text_color(gray(500))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child((i + 1).to_string()),
            )
            .child(
                div()
                    .v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .h_flex()
                            .justify_between()
                            .child(div().font_weight(FontWeight::MEDIUM).child(name))
                            .child(
                                div()
                                    .text_12()
                                    .text_color(gray(600))
                                    .whitespace_nowrap()
                                    .child(modified),
                            ),
                    )
                    .child(
                        div()
                            .text_12()
                            .text_color(gray(600))
                            .truncate()
                            .child(shown_path),
                    ),
            )
            .child(
                div()
                    .id(("forget", i))
                    .group("forget")
                    .ml_auto()
                    .size_8()
                    .flex_none()
                    .rounded_full()
                    .h_flex()
                    .justify_center()
                    .hover(|d| d.bg(books::color("red", 200)))
                    .child(
                        books::icon("x", px(16.), gray(600))
                            .group_hover("forget", |s| s.text_color(books::color("red", 400))),
                    )
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(t("Remove from the list"))
                            .build(window, cx)
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        session::forget_file(&forget);
                        this.files = session::recent_files();
                        cx.notify();
                    })),
            )
    }
}

/// `truncate`: the end of a long path.
fn truncate_path(path: &str) -> String {
    let count = path.chars().count();
    if count < 72 {
        return path.to_owned();
    }
    let tail: String = path.chars().skip(count - 72).collect();
    format!("...{tail}")
}

/// `DateTime.toRelative()`: "3 days ago".
fn relative_time(time: std::time::SystemTime) -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(time)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (n, unit) = match seconds {
        s if s < 60 => (s, "second"),
        s if s < 3_600 => (s / 60, "minute"),
        s if s < 86_400 => (s / 3_600, "hour"),
        s if s < 2_592_000 => (s / 86_400, "day"),
        s if s < 31_536_000 => (s / 2_592_000, "month"),
        s => (s / 31_536_000, "year"),
    };
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}

/// A round icon of the selector's actions: blue for new, green for existing, pink for the
/// demo.
fn action_icon(color: &str, icon: &str, size: f32) -> Div {
    div()
        .size_8()
        .flex_none()
        .rounded_full()
        .bg(books::color(color, 500))
        .h_flex()
        .justify_center()
        .child(books::icon(icon, px(size), white()))
}

impl Render for Selector {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.creating_demo.is_some();
        let action = |id: &'static str, icon: Div, title: &str, detail: &str| {
            div()
                .id(id)
                .h_flex()
                .h_16()
                .px_4()
                .gap_4()
                .when(!busy, |d| d.cursor_pointer().hover(|d| d.bg(gray(50))))
                .child(icon)
                .child(
                    div()
                        .v_flex()
                        .child(div().font_weight(FontWeight::MEDIUM).child(t(title)))
                        .child(div().text_12().text_color(gray(600)).child(t(detail))),
                )
        };
        let rows: Vec<AnyElement> = self
            .files
            .clone()
            .iter()
            .enumerate()
            .map(|(i, file)| self.row(i, file, cx).into_any_element())
            .collect();
        let has_files = !self.files.is_empty();
        let hr = || div().w_full().h(px(1.)).flex_none().bg(gray(200));
        div()
            .size_full()
            .h_flex()
            .justify_center()
            .bg(gray(25))
            .child(
                div()
                    .v_flex()
                    .relative()
                    .w(books::W_FORM)
                    .h(px(700.))
                    .bg(white())
                    .border_1()
                    .border_color(gray(200))
                    .rounded(books::RADIUS_LG)
                    .shadow(books::shadow_lg())
                    .text_color(gray(900))
                    .child(
                        div()
                            .v_flex()
                            .p_4()
                            .child(
                                div()
                                    .text_20()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(black())
                                    .child(t("Welcome to Latte Books")),
                            )
                            .child(div().text_13().text_color(gray(600)).child(t(
                                "Create a new company or select an existing one from your computer",
                            ))),
                    )
                    .child(hr())
                    .children(self.error.clone().map(|e| error_message(e, cx)))
                    .child(
                        action(
                            "new-company",
                            action_icon("blue", "plus", 20.),
                            "New Company",
                            "Create a new company and store it on your computer",
                        )
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(Selected::New))),
                    )
                    .child(
                        action(
                            "existing-company",
                            action_icon("green", "upload", 16.),
                            "Existing Company",
                            "Load an existing company from your computer",
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.open_existing(window, cx)),
                        ),
                    )
                    .when(!has_files, |d| {
                        d.child(
                            action(
                                "create-demo",
                                action_icon("pink", "monitor", 16.),
                                "Create Demo",
                                "Create a demo company to try out Latte Books",
                            )
                            .on_click(
                                cx.listener(|this, _, window, cx| this.create_demo(window, cx)),
                            ),
                        )
                    })
                    .child(hr())
                    .child(match &self.creating_demo {
                        // `Loading`.
                        Some((message, percent)) => div()
                            .id("files")
                            .v_flex()
                            .p_4()
                            .gap_2()
                            .child(div().text_12().child(t(message)))
                            .child(
                                div().h(px(6.)).w_full().rounded_full().bg(gray(100)).child(
                                    div()
                                        .h_full()
                                        .rounded_full()
                                        .bg(gray(800))
                                        .w(relative(percent.clamp(0.0, 1.0) as f32)),
                                ),
                            ),
                        None => div()
                            .id("files")
                            .v_flex()
                            .max_h(px(340.))
                            .overflow_y_scroll()
                            .children(rows),
                    })
                    .when(has_files, |d| d.child(hr()))
                    // The language and Create Demo, at the foot of the card.
                    .child(
                        div()
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .h_flex()
                            .justify_between()
                            .p_4()
                            .text_12()
                            .when(!busy, |d| {
                                d.child(div().w(px(160.)).child(self.language.clone()))
                            })
                            .when(busy, |d| d.child(div()))
                            .when(has_files, |d| {
                                let label = if busy { "Please Wait" } else { "Create Demo" };
                                d.child(
                                    div()
                                        .id("create-demo")
                                        .h_flex()
                                        .h_8()
                                        .px_4()
                                        .rounded(books::RADIUS)
                                        .bg(gray(100))
                                        .when(!busy, |d| {
                                            d.cursor_pointer().hover(|d| d.bg(gray(200)))
                                        })
                                        .child(t(label))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            if this.creating_demo.is_none() {
                                                this.create_demo(window, cx)
                                            }
                                        })),
                                )
                            }),
                    ),
            )
    }
}

/// Where the demo company is offered to be saved: `Documents\Latte Books`.
fn demo_dir() -> PathBuf {
    let dir = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|home| PathBuf::from(home).join("Documents").join("Latte Books"))
        .unwrap_or_default();
    let _ = std::fs::create_dir_all(&dir);
    dir
}
