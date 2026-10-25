//! The Import Wizard: `src/pages/ImportWizard.vue` @ v0.36.0.
//!
//! Pick what to import, save its template, fill it in elsewhere and select it, check the
//! column each field is in, fix any cell shown in red by editing it in place, and import.
//! Submit is asked in a dialog once the import starts, the columns are picked in a dialog
//! from the "…" menu, and the import ends with what was imported: Fix Failed keeps the
//! rows that failed, Show Me opens the list and Done starts again. The work is done by
//! `latte_core::importer`.
//!
//! Differences from Books:
//! - A cell is edited as text, whatever its field; Books shows the field's own control,
//!   such as a checkbox or a date picker.
//! - Fix Failed keeps the columns as they were; Books puts them back in the template's
//!   order.

use std::time::Duration;

use crate::books::{self, Text as _, gray};
use crate::widgets::{
    Button, DialogButton, Kind, check_box, control_frame, dialog, dropdown_item, loading, modal,
    popover_panel,
};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use latte_core::importer::{self, Cell, ImportResult, Importer};
use latte_core::views::field;
use latte_core::views::sidebar::Route;

use crate::i18n::t;
use crate::link::{LinkInput, LinkOption, LinkPicked};
use crate::session::Shared;
use crate::{Nav, Navigate};

/// `10rem`, a column's width, and `4rem`, the row numbers'.
const COLUMN_WIDTH: f32 = 160.;
const INDEX_WIDTH: f32 = 64.;

/// What an import did (`success`, `failed`).
struct Outcome {
    success: Vec<String>,
    failed: Vec<(String, String)>,
    /// The names in the file of what was imported (`successOldName`).
    success_old_names: Vec<String>,
}

/// The dialogs the wizard asks in.
enum Ask {
    /// "Cannot Import" or "Cannot read file", with why.
    Error(String, String),
    /// `askShouldSubmit`.
    Submit,
}

pub struct ImportWizard {
    session: Shared,
    type_input: Entity<LinkInput>,
    importer: Option<Importer>,
    /// The file selected (`file`).
    file_name: Option<String>,
    columns: Vec<Entity<LinkInput>>,
    /// `showColumnPicker`.
    show_picker: bool,
    /// The "…" menu, while open.
    menu: bool,
    /// The cell being edited, and its box.
    editing: Option<(usize, usize)>,
    cell_input: Entity<InputState>,
    ask: Option<Ask>,
    /// `isMakingEntries`: how many of how many are made.
    making: Option<(usize, usize)>,
    /// `complete`.
    outcome: Option<Outcome>,
    focus: FocusHandle,
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
        let cell_input = cx.new(|cx| InputState::new(window, cx));
        let subscriptions = vec![
            cx.subscribe_in(
                &type_input,
                window,
                |this, _, event: &LinkPicked, window, cx| this.set_type(&event.0, window, cx),
            ),
            cx.subscribe_in(
                &cell_input,
                window,
                |this, state, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                        let text = state.read(cx).value().to_string();
                        this.set_cell(&text, cx);
                        // Enter leaves the cell; the page keeps the keyboard for the shortcuts.
                        if matches!(event, InputEvent::PressEnter { .. }) {
                            this.focus.focus(window, cx);
                        }
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
            menu: false,
            editing: None,
            cell_input,
            ask: None,
            making: None,
            outcome: None,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
            _column_subscriptions: Vec::new(),
        }
    }

    /// `setImportType`: a new importer for the type, after `clear`.
    fn set_type(&mut self, schema_name: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_state();
        if !schema_name.is_empty() {
            let s = self.session.borrow();
            match Importer::new(schema_name, &s.fyo) {
                Ok(importer) => self.importer = Some(importer),
                Err(e) => self.ask = Some(Ask::Error(t("Error"), e.to_string())),
            }
        }
        self.make_columns(window, cx);
        cx.notify();
    }

    fn clear_state(&mut self) {
        self.importer = None;
        self.file_name = None;
        self.editing = None;
        self.outcome = None;
        self.making = None;
        self.menu = false;
        self.show_picker = false;
    }

    /// `clear`: back to no type.
    fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_state();
        self.type_input
            .update(cx, |link, cx| link.set_value(None, window, cx));
        self.make_columns(window, cx);
        cx.notify();
    }

    /// `gridColumnTitleDf`'s options: the picked fields, then None.
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
            .chain([LinkOption {
                value: "".into(),
                label: t("None").into(),
            }])
            .collect()
    }

    /// `columnCount`: the picked fields until a file is selected, then the file's columns
    /// that have a place.
    fn column_count(&self) -> usize {
        let Some(importer) = &self.importer else {
            return 0;
        };
        if self.file_name.is_none() {
            return importer.picked.values().filter(|p| **p).count();
        }
        match importer.rows.first() {
            None => importer.assigned.len(),
            Some(row) => importer.assigned.len().min(row.len()),
        }
    }

    /// A column picker for each column, set to the column's field.
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
            let input =
                cx.new(|cx| LinkInput::new(options, t("Select column"), window, cx).small());
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

    /// `pickColumn`: a field picked or not; one no longer picked leaves its column, and
    /// before a file is selected the columns follow the picked fields
    /// (`reassignTemplateFields`).
    fn pick_column(
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
            if !picked
                && let Some(index) = importer
                    .assigned
                    .iter()
                    .position(|k| k.as_deref() == Some(key))
            {
                importer.set_template_field(&s.fyo, index, None);
                if importer.rows.is_empty() {
                    let picked: Vec<String> = importer
                        .picked
                        .iter()
                        .filter(|(_, p)| **p)
                        .map(|(k, _)| k.clone())
                        .collect();
                    for i in 0..importer.assigned.len() {
                        importer.set_template_field(&s.fyo, i, picked.get(i).cloned());
                    }
                }
            }
            if picked && importer.rows.is_empty() && self.file_name.is_none() {
                // A newly picked field gets the next column.
                let index = importer.picked.values().filter(|p| **p).count() - 1;
                if index >= importer.assigned.len() {
                    importer.assigned.resize(index + 1, None);
                }
                if importer.assigned[index].is_none() {
                    importer.set_template_field(&s.fyo, index, Some(key.to_owned()));
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
        // `importType + ' ' + t`Template``: the schema's name.
        let file_name = format!("{} {}.csv", importer.schema_name, t("Template"));
        let dir = documents_dir();
        let path = cx.prompt_for_new_path(&dir, Some(&file_name));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = path.await else {
                return;
            };
            let result = std::fs::write(&path, template);
            this.update(cx, |this, cx| {
                if let Err(e) = result {
                    this.ask = Some(Ask::Error(t("Error"), e.to_string()));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// `selectFile`.
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
        let Some(schema_name) = self.importer.as_ref().map(|i| i.schema_name.clone()) else {
            return;
        };
        let cannot_read = || {
            Ask::Error(
                t("Cannot read file"),
                t("Bad import data, could not read file."),
            )
        };
        let Ok(text) = text else {
            self.ask = Some(cannot_read());
            cx.notify();
            return;
        };
        {
            let s = self.session.borrow();
            // A file is read into a new importer, as Books makes one for the type.
            let mut importer = match Importer::new(&schema_name, &s.fyo) {
                Ok(importer) => importer,
                Err(e) => {
                    self.ask = Some(Ask::Error(t("Error"), e.to_string()));
                    return;
                }
            };
            if !importer.select_file(&s.fyo, &text) || importer.rows.is_empty() {
                self.ask = Some(cannot_read());
            } else {
                self.importer = Some(importer);
                self.editing = None;
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
        self.editing = None;
        cx.notify();
    }

    /// Edits a cell in place.
    fn edit_cell(
        &mut self,
        row: usize,
        column: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(importer) = &self.importer else {
            return;
        };
        let Some(cell) = importer.rows.get(row).and_then(|r| r.get(column)) else {
            return;
        };
        let text = match (self.column_field(column), cell.error) {
            (Some(f), false) => field::edit_text(&f, cell.value.as_ref()),
            _ => cell.raw.clone(),
        };
        self.editing = Some((row, column));
        self.cell_input.update(cx, |input, cx| {
            input.set_value(text, window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    fn set_cell(&mut self, text: &str, cx: &mut Context<Self>) {
        let Some((row, column)) = self.editing.take() else {
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

    /// The field of a column, if it has one.
    fn column_field(&self, column: usize) -> Option<latte_schema::Field> {
        let importer = self.importer.as_ref()?;
        let key = importer.assigned.get(column)?.as_ref()?;
        Some(importer.fields.get(key)?.field.clone())
    }

    /// `importData`: the checks, then Submit asked when the type can be submitted.
    fn import(&mut self, cx: &mut Context<Self>) {
        if self.making.is_some() || self.outcome.is_some() {
            return;
        }
        let checked = {
            let s = self.session.borrow();
            let Some(importer) = &self.importer else {
                return;
            };
            importer
                .check(&s.fyo)
                .map(|problem| (problem, importer.is_submittable(&s.fyo)))
        };
        match checked {
            Err(e) => self.ask = Some(Ask::Error(t("Cannot Import"), e.to_string())),
            Ok((Some(problem), _)) => self.ask = Some(Ask::Error(t("Cannot Import"), problem)),
            Ok((None, true)) => self.ask = Some(Ask::Submit),
            Ok((None, false)) => self.make_entries(false, cx),
        }
        cx.notify();
    }

    /// Saves the documents one by one, with the loader showing how many are made.
    fn make_entries(&mut self, submit: bool, cx: &mut Context<Self>) {
        let docs = {
            let mut s = self.session.borrow_mut();
            let Some(importer) = &self.importer else {
                return;
            };
            importer.docs(&mut s.fyo)
        };
        let docs = match docs {
            Ok(docs) => docs,
            Err(e) => {
                self.ask = Some(Ask::Error(t("Cannot Import"), e.to_string()));
                cx.notify();
                return;
            }
        };
        let total = docs.len();
        self.making = Some((0, total));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let mut result = ImportResult::default();
            for (i, doc) in docs.into_iter().enumerate() {
                let Ok(()) = this.update(cx, |this, cx| {
                    result.import_doc(&mut this.session.borrow_mut().fyo, doc, submit);
                    this.making = Some((i + 1, total));
                    cx.notify();
                }) else {
                    return;
                };
                cx.background_executor().timer(Duration::ZERO).await;
            }
            this.update(cx, |this, cx| {
                this.making = None;
                this.outcome = Some(Outcome {
                    success: result.success.clone(),
                    failed: result
                        .failed
                        .iter()
                        .map(|(name, e)| (name.clone(), e.to_string()))
                        .collect(),
                    success_old_names: result.success_old_names.clone(),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// `clearSuccessfullyImportedEntries`: only the rows that failed are left to fix.
    fn fix_failed(&mut self, cx: &mut Context<Self>) {
        if let (Some(outcome), Some(importer)) = (self.outcome.take(), &mut self.importer) {
            let result = ImportResult {
                success_old_names: outcome.success_old_names,
                ..ImportResult::default()
            };
            importer.keep_failed(&result);
        }
        self.file_name = None;
        self.editing = None;
        cx.notify();
    }

    /// `showMe`: the list of what was imported.
    fn show_me(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(schema) = self.importer.as_ref().map(|i| i.schema_name.clone()) else {
            return;
        };
        let title = self
            .session
            .borrow()
            .fyo
            .schema(&schema)
            .map_or_else(|| schema.clone(), |s| s.label.clone());
        self.clear(window, cx);
        cx.emit(Navigate(Nav::Route(Route::list(&schema, &title, None))));
    }

    /// The header's "…" (`DropdownWithActions.vue`): Select File or Change File once there
    /// are rows, Pick Import Columns, and Cancel.
    fn render_menu(&self, has_rows: bool, cx: &mut Context<Self>) -> Div {
        let button = Button::new("import-menu")
            .icon("more-horizontal")
            .tooltip(t("More"))
            .disabled(self.making.is_some())
            .on_click(cx.listener(|this, _, _, cx| {
                this.menu = !this.menu;
                cx.notify();
            }));
        let menu = self.menu.then(|| {
            let file_label = if self.file_name.is_some() {
                "Change File"
            } else {
                "Select File"
            };
            let mut items: Vec<(&'static str, bool, u8)> = Vec::new();
            if has_rows {
                items.push((file_label, false, 0));
            }
            items.push(("Pick Import Columns", false, 1));
            items.push(("Cancel", true, 2));
            deferred(
                anchored().anchor(Anchor::TopRight).snap_to_window().child(
                    popover_panel()
                        .id("import-menu-panel")
                        .occlude()
                        .mt_2()
                        .min_w(px(160.))
                        .p_1()
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.menu = false;
                            cx.notify();
                        }))
                        .children(items.into_iter().enumerate().map(
                            |(i, (label, danger, action))| {
                                let label = div()
                                    .when(danger, |d| d.text_color(books::color("red", 700)))
                                    .child(t(label));
                                dropdown_item(("import-action", i), label)
                                    .when(i > 0, |d| d.mt_1())
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.menu = false;
                                        match action {
                                            0 => this.select_file(window, cx),
                                            1 => this.show_picker = true,
                                            _ => this.clear(window, cx),
                                        }
                                        cx.notify();
                                    }))
                            },
                        )),
                ),
            )
            .with_priority(1)
        });
        div().relative().child(button).children(menu)
    }

    /// The row of column pickers and the grid of values.
    fn render_grid(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(importer) = &self.importer else {
            return div().into_any_element();
        };
        let columns = self.column_count();
        let width = px(COLUMN_WIDTH);
        // `.index-cell`.
        let index_cell = || {
            div()
                .w(px(INDEX_WIDTH))
                .flex_none()
                .h_flex()
                .justify_end()
                .pr_4()
                .self_stretch()
                .border_r_1()
                .border_color(gray(200))
                .text_color(gray(600))
        };
        let assignment = div()
            .h_flex()
            .flex_none()
            .py_4()
            .pr_4()
            .gap_4()
            .border_b_1()
            .border_r_1()
            .border_color(gray(200))
            .bg(books::white())
            .child(index_cell().my(px(-16.)).child("#"))
            .children(
                self.columns
                    .iter()
                    .take(columns)
                    .map(|c| div().w(width).flex_none().child(c.clone())),
            );
        let formatter = &self.session.borrow().formatter;
        let rows: Vec<AnyElement> = importer
            .rows
            .iter()
            .enumerate()
            .map(|(r, row)| {
                let group = SharedString::from(format!("import-row-{r}"));
                div()
                    .h_flex()
                    .gap_4()
                    .child(
                        index_cell()
                            .id(("remove-row", r))
                            .group(group.clone())
                            .cursor_pointer()
                            .child(
                                div()
                                    .group_hover(group.clone(), |d| d.invisible())
                                    .child(format!("{}", r + 1)),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .invisible()
                                    .group_hover(group.clone(), |d| d.visible())
                                    .child(books::icon("x", px(16.), gray(600))),
                            )
                            .relative()
                            .on_click(cx.listener(move |this, _, _, cx| this.remove_row(r, cx))),
                    )
                    .children((0..columns).map(|c| {
                        let cell = row.get(c).cloned().unwrap_or_default();
                        self.render_cell(r, c, &cell, formatter, cx)
                            .w(width)
                            .flex_none()
                    }))
                    .into_any_element()
            })
            .collect();
        let values = if rows.is_empty() {
            div()
                .h_flex()
                .h(px(62.5))
                .pl_4()
                .text_color(gray(700))
                .child(t("No rows added. Select a file or add rows."))
        } else {
            div()
                .v_flex()
                .flex_none()
                .py_4()
                .pr_4()
                .gap_4()
                .border_r_1()
                .border_b_1()
                .border_color(gray(200))
                .children(rows)
        };
        div()
            .id("import-grid")
            .text_13()
            .flex_1()
            .min_h_0()
            .overflow_scroll()
            .child(
                div()
                    .v_flex()
                    .w(px(INDEX_WIDTH
                        + 16.
                        + (COLUMN_WIDTH + 16.) * columns as f32))
                    .child(assignment)
                    .child(values),
            )
            .into_any_element()
    }

    /// A cell: an assigned column's value, red when it can't be read, edited in place on
    /// a click; an unassigned column's text, read-only.
    fn render_cell(
        &self,
        r: usize,
        c: usize,
        cell: &Cell,
        formatter: &latte_core::fyo::format::Formatter,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let field = self.column_field(c);
        let editing = self.editing == Some((r, c));
        let title = field_title(cell, field.as_ref(), formatter);
        let id = ("import-cell", r * 1000 + c);
        let Some(field) = field else {
            // `Data` read-only with the cell's text.
            return control_frame(true, false)
                .id(id)
                .min_h(px(30.))
                .px_2()
                .child(div().truncate().child(cell.raw.clone()))
                .tooltip(move |window, cx| Tooltip::new(title.clone()).build(window, cx));
        };
        let frame = control_frame(false, editing)
            .id(id)
            .min_h(px(30.))
            .when(cell.error, |d| d.border_color(books::color("red", 300)));
        if editing {
            return frame.child(
                Input::new(&self.cell_input)
                    .appearance(false)
                    .h(px(28.))
                    .px_2()
                    .text_13(),
            );
        }
        let text = if cell.error {
            String::new()
        } else {
            field::display(formatter, &field, cell.value.as_ref())
        };
        let numeric = matches!(
            field.fieldtype,
            Some(
                latte_schema::FieldType::Int
                    | latte_schema::FieldType::Float
                    | latte_schema::FieldType::Currency
            )
        );
        frame
            .px_2()
            .cursor_text()
            .when(numeric, |d| d.justify_end())
            .child(div().truncate().child(text))
            .tooltip(move |window, cx| Tooltip::new(title.clone()).build(window, cx))
            .on_click(cx.listener(move |this, _, window, cx| this.edit_cell(r, c, window, cx)))
    }

    /// `Pick Import Columns`: the fields by schema, three to a row, and how many are
    /// picked.
    fn render_picker(&self, cx: &mut Context<Self>) -> Option<Div> {
        let importer = self.importer.as_ref()?;
        // Each schema's fields: key, label, picked and required.
        type Fields = Vec<(String, String, bool, bool)>;
        let mut groups: Vec<(String, Fields)> = Vec::new();
        for (key, tf) in &importer.fields {
            let label = match &tf.parent_child_field {
                Some(table) => format!(
                    "{} ({})",
                    table.label.as_deref().unwrap_or(&table.fieldname),
                    tf.schema_label
                ),
                None => tf.schema_label.clone(),
            };
            let entry = (
                key.clone(),
                tf.label().to_owned(),
                importer.picked.get(key) == Some(&true),
                tf.field.required,
            );
            match groups.iter_mut().find(|(l, _)| *l == label) {
                Some((_, fields)) => fields.push(entry),
                None => groups.push((label, vec![entry])),
            }
        }
        let picked = importer.picked.values().filter(|p| **p).count();
        let hr = || div().w_full().h(px(1.)).flex_none().bg(gray(200));
        Some(
            div()
                .v_flex()
                .w(books::W_FORM)
                .child(form_header(t("Pick Import Columns")))
                .child(hr())
                .child(
                    div()
                        .id("pick-columns")
                        .max_h(px(480.))
                        .overflow_y_scroll()
                        .children(groups.into_iter().map(|(label, fields)| {
                            div()
                                .p_4()
                                .child(
                                    div()
                                        .text_12()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(gray(800))
                                        .child(label),
                                )
                                .child(
                                    div()
                                        .grid()
                                        .grid_cols(3)
                                        .mt_1()
                                        .border_1()
                                        .border_color(gray(200))
                                        .rounded(books::RADIUS)
                                        .children(fields.into_iter().map(
                                            |(key, label, checked, required)| {
                                                div()
                                                    .id(SharedString::from(format!("pick-{key}")))
                                                    .h_flex()
                                                    .px_3()
                                                    .py_2()
                                                    .gap_2()
                                                    .when(!required, |d| d.cursor_pointer())
                                                    .child(check_box(checked))
                                                    .child(
                                                        div()
                                                            .text_13()
                                                            .text_color(gray(600))
                                                            .truncate()
                                                            .child(label),
                                                    )
                                                    .when(required, |d| {
                                                        d.child(
                                                            div()
                                                                .text_color(books::color(
                                                                    "red", 600,
                                                                ))
                                                                .child("*"),
                                                        )
                                                    })
                                                    .when(!required, |d| {
                                                        d.on_click(cx.listener(
                                                            move |this, _, window, cx| {
                                                                this.pick_column(
                                                                    &key, !checked, window, cx,
                                                                )
                                                            },
                                                        ))
                                                    })
                                            },
                                        )),
                                )
                        })),
                )
                .child(hr())
                .child(
                    div()
                        .h_flex()
                        .p_4()
                        .justify_between()
                        .child(
                            div()
                                .text_12()
                                .text_color(gray(600))
                                .child(format!("{picked} {}", t("fields selected"))),
                        )
                        .child(
                            Button::new("picker-done")
                                .primary()
                                .label(t("Done"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.show_picker = false;
                                    cx.notify();
                                })),
                        ),
                ),
        )
    }

    /// `Import Complete`: what was imported and what failed.
    fn render_outcome(&self, cx: &mut Context<Self>) -> Option<Div> {
        let outcome = self.outcome.as_ref()?;
        let hr = || div().w_full().h(px(1.)).flex_none().bg(gray(200));
        let count = |n: usize, one: &str, many: &str| {
            if n == 1 {
                format!("{n} {}", t(one))
            } else {
                format!("{n} {}", t(many))
            }
        };
        let section_header = |title: String, detail: String| {
            div()
                .h_flex()
                .justify_between()
                .px_4()
                .pt_4()
                .pb_1()
                .child(
                    div()
                        .text_13()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .child(div().text_12().text_color(gray(600)).child(detail))
        };
        let success = (!outcome.success.is_empty()).then(|| {
            div()
                .v_flex()
                .child(section_header(
                    t("Success"),
                    count(outcome.success.len(), "entry imported", "entries imported"),
                ))
                .child(
                    div()
                        .id("import-success")
                        .max_h(px(160.))
                        .overflow_y_scroll()
                        .children(outcome.success.iter().enumerate().map(|(i, name)| {
                            div()
                                .h_flex()
                                .px_4()
                                .py_1()
                                .gap_4()
                                .text_13()
                                .child(
                                    div()
                                        .w_4()
                                        .flex_none()
                                        .h_flex()
                                        .justify_end()
                                        .child(format!("{}.", i + 1)),
                                )
                                .child(div().truncate().child(name.clone()))
                        })),
                )
                .child(hr())
        });
        let failed = (!outcome.failed.is_empty()).then(|| {
            div()
                .v_flex()
                .child(section_header(
                    t("Failed"),
                    count(outcome.failed.len(), "entry failed", "entries failed"),
                ))
                .child(
                    div()
                        .id("import-failed")
                        .max_h(px(160.))
                        .overflow_y_scroll()
                        .children(outcome.failed.iter().enumerate().map(|(i, (name, e))| {
                            div()
                                .h_flex()
                                .px_4()
                                .py_1()
                                .gap_4()
                                .text_13()
                                .child(
                                    div()
                                        .w_4()
                                        .flex_none()
                                        .h_flex()
                                        .justify_end()
                                        .child(format!("{}.", i + 1)),
                                )
                                .child(div().w_32().flex_none().truncate().child(name.clone()))
                                .child(div().truncate().child(e.clone()))
                        })),
                )
                .child(hr())
        });
        let nothing = outcome.success.is_empty() && outcome.failed.is_empty();
        let has_failed = !outcome.failed.is_empty();
        let show_me = !has_failed && !outcome.success.is_empty();
        Some(
            div()
                .v_flex()
                .w(books::W_FORM)
                .child(form_header(t("Import Complete")))
                .child(hr())
                .children(success)
                .children(failed)
                .when(nothing, |d| {
                    d.child(div().p_4().text_13().child(t("No entries were imported.")))
                })
                .child(
                    div()
                        .h_flex()
                        .justify_between()
                        .p_4()
                        .when(has_failed, |d| {
                            d.child(
                                Button::new("fix-failed")
                                    .label(t("Fix Failed"))
                                    .on_click(cx.listener(|this, _, _, cx| this.fix_failed(cx))),
                            )
                        })
                        .when(show_me, |d| {
                            d.child(Button::new("show-me").label(t("Show Me")).on_click(
                                cx.listener(|this, _, window, cx| this.show_me(window, cx)),
                            ))
                        })
                        .child(
                            Button::new("import-done").label(t("Done")).on_click(
                                cx.listener(|this, _, window, cx| this.clear(window, cx)),
                            ),
                        ),
                ),
        )
    }

    fn render_ask(&self, window: &Window, cx: &mut Context<Self>) -> Option<Deferred> {
        let ask = self.ask.as_ref()?;
        Some(match ask {
            Ask::Error(title, detail) => dialog(
                title.clone(),
                Some(detail.clone().into()),
                Kind::Error,
                vec![DialogButton::new(
                    t("Okay"),
                    false,
                    cx.listener(|this, _, _, cx| {
                        this.ask = None;
                        cx.notify();
                    }),
                )],
                window,
            ),
            // Books' `details` is a typo, so the dialog says no more than its title.
            Ask::Submit => dialog(
                t("Submit entries?"),
                None,
                Kind::Info,
                vec![
                    DialogButton::new(
                        t("Yes"),
                        true,
                        cx.listener(|this, _, _, cx| {
                            this.ask = None;
                            this.make_entries(true, cx);
                        }),
                    ),
                    DialogButton::new(
                        t("No"),
                        false,
                        cx.listener(|this, _, _, cx| {
                            this.ask = None;
                            this.make_entries(false, cx);
                        }),
                    )
                    .escape(),
                ],
                window,
            ),
        })
    }
}

/// `FormHeader.vue`: a modal's title, 56px high.
fn form_header(title: String) -> Div {
    div()
        .h_flex()
        .h(px(56.))
        .px_4()
        .flex_none()
        .text_18()
        .font_weight(FontWeight::SEMIBOLD)
        .child(title)
}

/// `getFieldTitle`: a cell's value, its text and whether it couldn't be read.
fn field_title(
    cell: &Cell,
    field: Option<&latte_schema::Field>,
    formatter: &latte_core::fyo::format::Formatter,
) -> SharedString {
    let mut title = Vec::new();
    if let (Some(field), Some(value)) = (field, cell.value.as_ref())
        && !matches!(value, latte_db::DocValue::Null)
    {
        title.push(format!(
            "{}: {}",
            t("Value"),
            field::display(formatter, field, Some(value))
        ));
    }
    if !cell.raw.is_empty() {
        title.push(format!("{}: {}", t("Raw Value"), cell.raw));
    }
    if cell.error {
        title.push(t("Conversion Error"));
    }
    if title.is_empty() {
        return t("No Value").into();
    }
    title.join(", ").into()
}

fn documents_dir() -> std::path::PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|home| std::path::PathBuf::from(home).join("Documents"))
        .unwrap_or_default()
}

impl Render for ImportWizard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let has_type = self.importer.is_some();
        let has_rows = self.importer.as_ref().is_some_and(|i| !i.rows.is_empty());
        let making = self.making.is_some();
        let problem = self.importer.as_ref().and_then(|i| i.error_message());
        // `ImportWizard.vue`'s header: More, Add Row and Save Template, then Import Data or
        // Select File.
        let header = crate::widgets::page_header(t("Import Wizard"), cx).child(
            div()
                .h_flex()
                .gap_2()
                .when(has_type, |d| {
                    d.child(self.render_menu(has_rows, cx))
                        .child(
                            Button::new("header-add-row")
                                .icon("plus")
                                .tooltip(t("Add Row"))
                                .disabled(making)
                                .on_click(cx.listener(|this, _, _, cx| this.add_row(cx))),
                        )
                        .child(
                            Button::new("save-template")
                                .icon("download")
                                .tooltip(t("Save Template"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.save_template(window, cx)
                                })),
                        )
                })
                .when(has_rows, |d| {
                    d.child(
                        Button::new("import")
                            .primary()
                            .label(t("Import Data"))
                            .disabled(problem.is_some() || making)
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

        // The type, and what to do next or what is wrong.
        let rows = self.importer.as_ref().map_or(0, |i| i.rows.len());
        let message = match (&problem, &self.file_name) {
            (Some(problem), _) => div()
                .ml_2()
                .text_13()
                .text_color(books::color("red", 500))
                .child(problem.clone()),
            (None, file) => {
                let mut parts = div().h_flex().ml_2().text_13().whitespace_nowrap();
                parts = match file {
                    Some(name) => parts
                        .text_color(books::pick(gray(900), books::raw_gray(25)))
                        .child(format!("{} ", t("Selected")))
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(format!("{name},")),
                        )
                        .child(format!(" {} ", t("check values and click on")))
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(t("Import Data.")),
                        ),
                    None if !has_type => parts
                        .text_color(books::pick(gray(700), books::raw_gray(200)))
                        .child(t("Set an Import Type")),
                    None => parts.text_color(books::pick(gray(700), books::raw_gray(200))),
                };
                parts.when(rows > 0, |d| {
                    let added = if rows == 1 {
                        format!(" {rows} {}", t("row added."))
                    } else {
                        format!(" {rows} {}", t("rows added."))
                    };
                    d.child(added)
                })
            }
        };

        div()
            .v_flex()
            .size_full()
            .track_focus(&self.focus)
            .child(header)
            .child(
                div()
                    .h_flex()
                    .h(books::H_PAGE_HEADER)
                    .flex_none()
                    .p_4()
                    .gap_2()
                    .border_b_1()
                    .border_color(gray(200))
                    .child(div().w(px(160.)).child(self.type_input.clone()))
                    .child(message),
            )
            .when(has_type, |d| d.child(self.render_grid(cx)))
            .children(self.making.map(|(done, total)| {
                let percent = if total == 0 {
                    0.
                } else {
                    done as f64 / total as f64
                };
                loading(
                    format!("{done} entries made out of {total}..."),
                    percent,
                    window,
                )
            }))
            .children(
                self.show_picker
                    .then(|| self.render_picker(cx))
                    .flatten()
                    .map(|picker| {
                        modal(
                            picker,
                            cx.listener(|this, _, _, cx| {
                                this.show_picker = false;
                                cx.notify();
                            }),
                            window,
                        )
                    }),
            )
            .children(self.render_outcome(cx).map(|outcome| {
                modal(
                    outcome,
                    cx.listener(|this, _, window, cx| this.clear(window, cx)),
                    window,
                )
            }))
            .children(self.render_ask(window, cx))
    }
}

impl Focusable for ImportWizard {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
