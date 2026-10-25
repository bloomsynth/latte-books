//! The Import Wizard: `src/pages/ImportWizard.vue` @ v0.36.0.
//!
//! Pick what to import, save its template, fill it in elsewhere and select it, check the
//! column each field is in, fix any cell shown in red, and import. The work is done by
//! `latte_core::importer`.
//!
//! Differences from Books:
//! - A cell is fixed by clicking it and typing in the box above the table; Books edits it
//!   in place.
//! - Whether to submit what is imported is a checkbox; Books asks once the import starts.
//! - The columns to pick are listed on the page; Books shows them in a dialog.

use crate::books::{Text as _, gray};
use crate::widgets::small_input;
use crate::widgets::{Button, check_box};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use latte_core::importer::{self, ImportResult, Importer};

use crate::Navigate;
use crate::i18n::t;
use crate::link::{LinkInput, LinkOption, LinkPicked};
use crate::session::Shared;
use crate::widgets::{error_message, info_message, page_header};

const COLUMN_WIDTH: f32 = 160.;

/// What an import did, as the wizard lists it.
struct Outcome {
    success: Vec<String>,
    failed: Vec<(String, String)>,
}

pub struct ImportWizard {
    session: Shared,
    type_input: Entity<LinkInput>,
    importer: Option<Importer>,
    file_name: Option<String>,
    columns: Vec<Entity<LinkInput>>,
    show_picker: bool,
    submit: bool,
    selected: Option<(usize, usize)>,
    cell_input: Entity<InputState>,
    outcome: Option<Outcome>,
    error: Option<String>,
    info: Option<String>,
    _subscriptions: Vec<Subscription>,
    _column_subscriptions: Vec<Subscription>,
}

impl EventEmitter<Navigate> for ImportWizard {}

impl ImportWizard {
    pub fn new(session: Shared, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let options = {
            let s = session.borrow();
            importer::importable_schema_names(&s.fyo)
                .into_iter()
                .map(|name| LinkOption {
                    value: name.into(),
                    label: s
                        .fyo
                        .schema(name)
                        .map_or_else(|| name.to_owned(), |schema| schema.label.clone())
                        .into(),
                })
                .collect()
        };
        let type_input = cx.new(|cx| LinkInput::new(options, t("Import Type"), window, cx).small());
        let cell_input = cx.new(|cx| InputState::new(window, cx).placeholder(t("No Value")));
        let subscriptions = vec![
            cx.subscribe_in(
                &type_input,
                window,
                |this, _, event: &LinkPicked, window, cx| this.set_type(&event.0, window, cx),
            ),
            cx.subscribe_in(
                &cell_input,
                window,
                |this, state, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                        let text = state.read(cx).value().to_string();
                        this.set_cell(&text, cx);
                    }
                },
            ),
        ];
        Self {
            session,
            type_input,
            importer: None,
            file_name: None,
            columns: Vec::new(),
            show_picker: false,
            submit: false,
            selected: None,
            cell_input,
            outcome: None,
            error: None,
            info: None,
            _subscriptions: subscriptions,
            _column_subscriptions: Vec::new(),
        }
    }

    fn set_type(&mut self, schema_name: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_messages();
        self.file_name = None;
        self.selected = None;
        self.outcome = None;
        if schema_name.is_empty() {
            self.importer = None;
        } else {
            let s = self.session.borrow();
            match Importer::new(schema_name, &s.fyo) {
                Ok(importer) => self.importer = Some(importer),
                Err(e) => self.error = Some(e.to_string()),
            }
        }
        self.make_columns(window, cx);
        cx.notify();
    }

    fn clear_messages(&mut self) {
        self.error = None;
        self.info = None;
    }

    /// The fields a column can be for: the picked ones.
    fn column_options(&self) -> Vec<LinkOption> {
        let Some(importer) = &self.importer else {
            return Vec::new();
        };
        importer
            .fields
            .iter()
            .filter(|(key, _)| importer.picked.get(*key) == Some(&true))
            .map(|(key, field)| LinkOption {
                value: key.clone().into(),
                label: field.column_label().into(),
            })
            .collect()
    }

    /// A field picker for each column, set to the column's field.
    fn make_columns(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.columns.clear();
        self._column_subscriptions.clear();
        let options = self.column_options();
        let assigned = self
            .importer
            .as_ref()
            .map(|i| i.assigned.clone())
            .unwrap_or_default();
        for (index, key) in assigned.into_iter().enumerate() {
            let options = options.clone();
            let input = cx.new(|cx| LinkInput::new(options, t("Select column"), window, cx));
            input.update(cx, |link, cx| {
                link.set_value(key.map(Into::into), window, cx)
            });
            self._column_subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |this, _, event: &LinkPicked, _, cx| {
                    let key = (!event.0.is_empty()).then(|| event.0.to_string());
                    this.set_column(index, key, cx);
                },
            ));
            self.columns.push(input);
        }
    }

    fn set_column(&mut self, index: usize, key: Option<String>, cx: &mut Context<Self>) {
        let s = self.session.borrow();
        if let Some(importer) = &mut self.importer {
            importer.set_template_field(&s.fyo, index, key);
        }
        cx.notify();
    }

    fn toggle_picked(
        &mut self,
        key: &str,
        picked: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        {
            let s = self.session.borrow();
            let Some(importer) = &mut self.importer else {
                return;
            };
            importer.picked.insert(key.to_owned(), picked);
            if !picked {
                let columns: Vec<usize> = importer
                    .assigned
                    .iter()
                    .enumerate()
                    .filter(|(_, k)| k.as_deref() == Some(key))
                    .map(|(i, _)| i)
                    .collect();
                for index in columns {
                    importer.set_template_field(&s.fyo, index, None);
                }
            }
        }
        self.make_columns(window, cx);
        cx.notify();
    }

    fn save_template(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(importer) = &self.importer else {
            return;
        };
        let template = importer.csv_template();
        let label = self
            .type_input
            .read(cx)
            .value()
            .map(|v| v.to_string())
            .unwrap_or_default();
        let label = self
            .session
            .borrow()
            .fyo
            .schema(&label)
            .map_or(label, |s| s.label.clone());
        let file_name = format!("{label} {}.csv", t("Template"));
        let dir = documents_dir();
        let path = cx.prompt_for_new_path(&dir, Some(&file_name));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = path.await else {
                return;
            };
            let result = std::fs::write(&path, template);
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => this.info = Some(format!("Saved {}", path.display())),
                    Err(e) => this.error = Some(e.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn select_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t("Select File").into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let text =
                std::fs::read(&path).map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
            this.update_in(cx, |this, window, cx| {
                this.load_file(&path, text, window, cx)
            })
            .ok();
        })
        .detach();
    }

    fn load_file(
        &mut self,
        path: &std::path::Path,
        text: std::io::Result<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_messages();
        self.outcome = None;
        self.selected = None;
        let Some(schema_name) = self.importer.as_ref().map(|i| i.schema_name.clone()) else {
            return;
        };
        let text = match text {
            Ok(text) => text,
            Err(e) => {
                self.error = Some(format!("{}: {e}", t("Cannot read file")));
                cx.notify();
                return;
            }
        };
        {
            let s = self.session.borrow();
            // A new file starts from a new importer, as Books makes one when it is picked.
            let mut importer = match Importer::new(&schema_name, &s.fyo) {
                Ok(importer) => importer,
                Err(e) => {
                    self.error = Some(e.to_string());
                    return;
                }
            };
            if !importer.select_file(&s.fyo, &text) || importer.rows.is_empty() {
                self.error = Some(t("Bad import data, could not read file."));
            } else {
                self.importer = Some(importer);
                self.file_name = path.file_name().map(|n| n.to_string_lossy().to_string());
            }
        }
        self.make_columns(window, cx);
        cx.notify();
    }

    fn add_row(&mut self, cx: &mut Context<Self>) {
        if let Some(importer) = &mut self.importer {
            importer.add_row();
        }
        cx.notify();
    }

    fn remove_row(&mut self, row: usize, cx: &mut Context<Self>) {
        if let Some(importer) = &mut self.importer {
            importer.remove_row(row);
        }
        self.selected = None;
        cx.notify();
    }

    fn select_cell(
        &mut self,
        row: usize,
        column: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected = Some((row, column));
        let raw = self
            .importer
            .as_ref()
            .and_then(|i| i.rows.get(row))
            .and_then(|r| r.get(column))
            .map(|c| c.raw.clone())
            .unwrap_or_default();
        self.cell_input.update(cx, |input, cx| {
            input.set_value(raw, window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    fn set_cell(&mut self, text: &str, cx: &mut Context<Self>) {
        let Some((row, column)) = self.selected else {
            return;
        };
        let s = self.session.borrow();
        if let Some(importer) = &mut self.importer {
            let unchanged = importer
                .rows
                .get(row)
                .and_then(|r| r.get(column))
                .is_some_and(|c| c.raw == text);
            if !unchanged {
                importer.set_cell(&s.fyo, row, column, text);
            }
        }
        cx.notify();
    }

    fn import(&mut self, cx: &mut Context<Self>) {
        self.clear_messages();
        let submit = self.submit;
        let result: Result<ImportResult, String> = {
            let mut s = self.session.borrow_mut();
            let Some(importer) = &self.importer else {
                return;
            };
            match importer.check(&s.fyo) {
                Err(e) => Err(e.to_string()),
                Ok(Some(message)) => Err(format!("{}: {message}", t("Cannot Import"))),
                Ok(None) => importer
                    .import(&mut s.fyo, submit)
                    .map_err(|e| e.to_string()),
            }
        };
        match result {
            Ok(result) => {
                if let Some(importer) = &mut self.importer {
                    importer.keep_failed(&result);
                }
                self.selected = None;
                self.outcome = Some(Outcome {
                    success: result.success.clone(),
                    failed: result
                        .failed
                        .iter()
                        .map(|(name, e)| (name.clone(), e.to_string()))
                        .collect(),
                });
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }

    fn render_picker(&self, cx: &mut Context<Self>) -> Div {
        let Some(importer) = &self.importer else {
            return div();
        };
        let picked = importer.picked.values().filter(|p| **p).count();
        let theme = cx.theme().clone();
        let toggle = Button::new("pick-columns")
            .small()
            .ghost()
            .label(format!(
                "{} · {picked} {}",
                t("Pick Import Columns"),
                t("fields selected")
            ))
            .on_click(cx.listener(|this, _, _, cx| {
                this.show_picker = !this.show_picker;
                cx.notify();
            }));
        let mut picker = div().v_flex().gap_2().child(toggle);
        if self.show_picker {
            let fields: Vec<(String, String, bool, bool)> = importer
                .fields
                .iter()
                .map(|(key, field)| {
                    (
                        key.clone(),
                        field.column_label(),
                        importer.picked.get(key) == Some(&true),
                        field.field.required,
                    )
                })
                .collect();
            picker = picker.child(
                div()
                    .h_flex()
                    .flex_wrap()
                    .gap_x_4()
                    .gap_y_1()
                    .p_2()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border)
                    .children(fields.into_iter().map(|(key, label, checked, required)| {
                        let id = SharedString::from(format!("pick-{key}"));
                        div()
                            .id(ElementId::Name(id))
                            .w(px(220.))
                            .h_flex()
                            .gap_3()
                            .when(!required, |d| d.cursor_pointer())
                            .when(required, |d| d.opacity(0.6))
                            .child(check_box(checked))
                            .child(div().text_13().text_color(gray(600)).child(label))
                            .when(!required, |d| {
                                d.on_click(cx.listener(move |this, _, window, cx| {
                                    this.toggle_picked(&key, !checked, window, cx)
                                }))
                            })
                    })),
            );
        }
        picker
    }

    fn render_table(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(importer) = &self.importer else {
            return div().into_any_element();
        };
        let width = px(COLUMN_WIDTH);
        let header = div()
            .h_flex()
            .gap_1()
            .child(div().w(px(48.)).flex_none())
            .children(
                self.columns
                    .iter()
                    .map(|c| div().w(width).flex_none().child(c.clone())),
            );
        let rows = importer.rows.iter().enumerate().map(|(r, row)| {
            div()
                .h_flex()
                .gap_1()
                .child(
                    div()
                        .w(px(48.))
                        .flex_none()
                        .h_flex()
                        .gap_1()
                        .text_11()
                        .text_color(theme.muted_foreground)
                        .child(
                            Button::new(("remove-row", r))
                                .xsmall()
                                .ghost()
                                .icon("x")
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.remove_row(r, cx)),
                                ),
                        )
                        .child(format!("{}", r + 1)),
                )
                .children((0..self.columns.len()).map(|c| {
                    let cell = row.get(c);
                    let raw = cell.map(|c| c.raw.clone()).unwrap_or_default();
                    let error = cell.is_some_and(|c| c.error);
                    let selected = self.selected == Some((r, c));
                    div()
                        .id(("cell", r * 1000 + c))
                        .w(width)
                        .flex_none()
                        .h(px(28.))
                        .px_2()
                        .h_flex()
                        .rounded_sm()
                        .border_1()
                        .border_color(if selected { theme.ring } else { theme.border })
                        .when(error, |d| {
                            d.bg(theme.red.opacity(0.12)).text_color(theme.red)
                        })
                        .cursor_pointer()
                        .child(div().truncate().child(raw))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.select_cell(r, c, window, cx)
                        }))
                }))
        });
        div()
            .id("import-table")
            .v_flex()
            .flex_1()
            .gap_1()
            .px_4()
            .pb_4()
            .overflow_scroll()
            .child(header)
            .children(rows)
            .child(
                div().pt_1().child(
                    Button::new("add-row")
                        .small()
                        .ghost()
                        .label(t("Add Row"))
                        .on_click(cx.listener(|this, _, _, cx| this.add_row(cx))),
                ),
            )
            .into_any_element()
    }

    fn render_outcome(&self, cx: &App) -> Option<Div> {
        let outcome = self.outcome.as_ref()?;
        let theme = cx.theme();
        let count = |n: usize, one: &str, many: &str| {
            if n == 1 {
                format!("{n} {}", t(one))
            } else {
                format!("{n} {}", t(many))
            }
        };
        let mut panel = div()
            .v_flex()
            .gap_1()
            .mx_4()
            .mt_2()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t("Import Complete")),
            )
            .child(count(
                outcome.success.len(),
                "entry imported",
                "entries imported",
            ));
        if outcome.success.is_empty() && outcome.failed.is_empty() {
            panel = panel.child(t("No entries were imported."));
        }
        if !outcome.success.is_empty() {
            panel = panel.child(
                div()
                    .text_12()
                    .text_color(theme.muted_foreground)
                    .child(outcome.success.join(", ")),
            );
        }
        if !outcome.failed.is_empty() {
            panel = panel
                .child(div().text_color(theme.red).child(count(
                    outcome.failed.len(),
                    "entry failed",
                    "entries failed",
                )))
                .children(
                    outcome
                        .failed
                        .iter()
                        .map(|(name, e)| div().text_12().child(format!("{name}: {e}"))),
                );
        }
        Some(panel)
    }
}

fn documents_dir() -> std::path::PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|home| std::path::PathBuf::from(home).join("Documents"))
        .unwrap_or_default()
}

impl Render for ImportWizard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let has_type = self.importer.is_some();
        let has_rows = self.importer.as_ref().is_some_and(|i| !i.rows.is_empty());
        let submittable = {
            let s = self.session.borrow();
            self.importer
                .as_ref()
                .is_some_and(|i| i.is_submittable(&s.fyo))
        };
        // `ImportWizard.vue`'s header: Add Row and Save Template as icons, then Select
        // File or Import Data.
        let header = page_header(t("Import Wizard"), cx).child(
            div()
                .h_flex()
                .gap_2()
                .when(has_type, |d| {
                    d.child(
                        Button::new("header-add-row")
                            .icon("plus")
                            .tooltip(t("Add Row"))
                            .on_click(cx.listener(|this, _, _, cx| this.add_row(cx))),
                    )
                    .child(
                        Button::new("save-template")
                            .icon("download")
                            .tooltip(t("Save Template"))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.save_template(window, cx)),
                            ),
                    )
                })
                .when(has_rows, |d| {
                    d.child(
                        Button::new("import")
                            .primary()
                            .label(t("Import Data"))
                            .on_click(cx.listener(|this, _, _, cx| this.import(cx))),
                    )
                })
                .when(has_type && !has_rows, |d| {
                    d.child(
                        Button::new("select-file")
                            .primary()
                            .label(t("Select File"))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.select_file(window, cx)),
                            ),
                    )
                }),
        );

        let problems = self.importer.as_ref().and_then(|i| i.error_message());
        let summary = self.importer.as_ref().map(|i| {
            let n = i.rows.len();
            if n == 0 {
                t("No rows added. Select a file or add rows.")
            } else if n == 1 {
                format!("{n} {}", t("row added."))
            } else {
                format!("{n} {}", t("rows added."))
            }
        });

        div()
            .v_flex()
            .size_full()
            .child(header)
            .children(self.error.clone().map(|e| error_message(e, cx)))
            .children(self.info.clone().map(|m| info_message(m, cx)))
            .children(problems.map(|m| error_message(m, cx)))
            .children(self.render_outcome(cx))
            .child(
                div()
                    .v_flex()
                    .gap_3()
                    .p_4()
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .child(div().w(px(160.)).child(self.type_input.clone()))
                            .child(div().ml_2().text_13().map(|d| {
                                match &self.file_name {
                                    Some(name) => d
                                        .text_color(gray(900))
                                        .child(format!("{} {name}", t("Selected")))
                                        .children(summary.clone().map(|s| format!(" {s}"))),
                                    None if has_type => {
                                        d.text_color(gray(700)).children(summary.clone())
                                    }
                                    None => d.text_color(gray(700)).child(t("Set an Import Type")),
                                }
                            }))
                            .when(submittable, |d| {
                                d.child(
                                    div()
                                        .id("submit-on-import")
                                        .ml_auto()
                                        .h_flex()
                                        .gap_3()
                                        .cursor_pointer()
                                        .child(check_box(self.submit))
                                        .child(
                                            div()
                                                .text_13()
                                                .text_color(gray(600))
                                                .child(t("Submit entries?")),
                                        )
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.submit = !this.submit;
                                            cx.notify();
                                        })),
                                )
                            }),
                    )
                    .child(self.render_picker(cx))
                    .when(self.selected.is_some(), |d| {
                        let (r, c) = self.selected.unwrap_or_default();
                        let column = self
                            .columns
                            .get(c)
                            .and_then(|input| input.read(cx).value().cloned())
                            .and_then(|key| {
                                self.importer
                                    .as_ref()
                                    .and_then(|i| i.fields.get(key.as_ref()))
                                    .map(|f| f.column_label())
                            })
                            .unwrap_or_else(|| t("None"));
                        d.child(
                            div()
                                .h_flex()
                                .gap_2()
                                .child(
                                    div()
                                        .text_12()
                                        .text_color(theme.muted_foreground)
                                        .child(format!("{} {}, {column}", t("Row"), r + 1)),
                                )
                                .child(div().w(px(320.)).child(small_input(&self.cell_input))),
                        )
                    }),
            )
            .when(has_type, |d| d.child(self.render_table(cx)))
    }
}
