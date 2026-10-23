//! Choosing a company file: `src/pages/DatabaseSelector.vue` @ v0.36.0.
//!
//! Differences from Books:
//! - Create Demo shows its progress in the selector instead of a full-window loader.
//! - "Remove" takes a file off the list and leaves it on the disk. Books deletes the file.
//! - Changing the language rebuilds the screen; Books reloads the window.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::component::button::*;
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

    fn row(&self, i: usize, file: &RecentFile, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
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
        div()
            .id(("file", i))
            .h_flex()
            .w_full()
            .px_3()
            .py_2()
            .gap_3()
            .rounded_md()
            .cursor_pointer()
            .hover(|d| d.bg(theme.list_hover))
            .on_click(cx.listener(move |_, _, _, cx| cx.emit(Selected::Open(path.clone()))))
            .child(
                div()
                    .size(px(28.))
                    .flex_none()
                    .rounded_md()
                    .bg(theme.muted)
                    .h_flex()
                    .justify_center()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(name.chars().next().unwrap_or('?').to_string()),
            )
            .child(
                div()
                    .v_flex()
                    .flex_1()
                    .overflow_hidden()
                    .child(div().font_weight(FontWeight::MEDIUM).child(name))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .truncate()
                            .child(file.path.display().to_string()),
                    ),
            )
            .child(
                Button::new(("forget", i))
                    .ghost()
                    .xsmall()
                    .label(t("Remove"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        session::forget_file(&forget);
                        this.files = session::recent_files();
                        cx.notify();
                    })),
            )
    }
}

impl Render for Selector {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let action = |id: &'static str, title: &str, detail: &str| {
            div()
                .id(id)
                .v_flex()
                .px_3()
                .py_2()
                .rounded_md()
                .cursor_pointer()
                .hover(|d| d.bg(theme.list_hover))
                .child(div().font_weight(FontWeight::MEDIUM).child(t(title)))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(t(detail)),
                )
        };
        let rows: Vec<AnyElement> = self
            .files
            .clone()
            .iter()
            .enumerate()
            .map(|(i, file)| self.row(i, file, cx).into_any_element())
            .collect();
        div().size_full().h_flex().justify_center().child(
            div()
                .v_flex()
                .w(px(600.))
                .max_h(px(640.))
                .border_1()
                .border_color(theme.border)
                .rounded_lg()
                .child(
                    div()
                        .v_flex()
                        .p_4()
                        .border_b_1()
                        .border_color(theme.border)
                        .child(
                            div()
                                .text_xl()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(t("Welcome to Latte Books")),
                        )
                        .child(div().text_color(theme.muted_foreground).child(t(
                            "Create a new company or select an existing one from your computer",
                        ))),
                )
                .children(self.error.clone().map(|e| error_message(e, cx)))
                .child(
                    div()
                        .v_flex()
                        .p_2()
                        .border_b_1()
                        .border_color(theme.border)
                        .child(
                            action(
                                "new-company",
                                "New Company",
                                "Create a new company and store it on your computer",
                            )
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(Selected::New))),
                        )
                        .child(
                            action(
                                "existing-company",
                                "Existing Company",
                                "Load an existing company from your computer",
                            )
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_existing(window, cx)),
                            ),
                        )
                        .when(self.files.is_empty(), |d| {
                            d.child(
                                action(
                                    "create-demo",
                                    "Create Demo",
                                    "Create a demo company to try out Latte Books",
                                )
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.create_demo(window, cx)),
                                ),
                            )
                        }),
                )
                .child(match &self.creating_demo {
                    // `Loading`.
                    Some((message, percent)) => div()
                        .id("files")
                        .v_flex()
                        .p_4()
                        .gap_2()
                        .flex_1()
                        .child(div().text_sm().child(t(message)))
                        .child(
                            div()
                                .h(px(6.))
                                .w_full()
                                .rounded_full()
                                .bg(theme.muted)
                                .child(
                                    div()
                                        .h_full()
                                        .rounded_full()
                                        .bg(theme.primary)
                                        .w(relative(percent.clamp(0.0, 1.0) as f32)),
                                ),
                        ),
                    None => div()
                        .id("files")
                        .v_flex()
                        .p_2()
                        .flex_1()
                        .overflow_y_scroll()
                        .children(rows),
                })
                .child(
                    div()
                        .h_flex()
                        .justify_between()
                        .p_2()
                        .border_t_1()
                        .border_color(theme.border)
                        .child(div().w(px(200.)).child(self.language.clone()))
                        .when(!self.files.is_empty(), |d| {
                            let label = if self.creating_demo.is_some() {
                                "Please Wait"
                            } else {
                                "Create Demo"
                            };
                            d.child(
                                Button::new("create-demo")
                                    .small()
                                    .label(t(label))
                                    .disabled(self.creating_demo.is_some())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.create_demo(window, cx)
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
