//! A list's Export Wizard: `src/components/ExportWizard.vue` @ v0.36.0, in a modal.
//!
//! Pick CSV or JSON, whether the list's filters apply, how many rows, and the fields;
//! the export is made by `latte_core::export`.

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use latte_core::export::{self, ExportField, ExportTableField};
use latte_db::QueryFilter;
use latte_schema::FieldType;

use crate::books::{Text as _, black, gray};
use crate::i18n::t;
use crate::link::{LinkInput, LinkOption, LinkPicked};
use crate::session::Shared;
use crate::widgets::{Button, check_box, control_frame};

/// The wizard was closed, with the file it exported to, if it did.
pub struct ExportClosed(pub Option<std::path::PathBuf>);

pub struct ExportWizard {
    session: Shared,
    schema: String,
    title: String,
    filters: QueryFilter,
    use_list_filters: bool,
    json: bool,
    format: Entity<LinkInput>,
    limit: Entity<InputState>,
    fields: Vec<ExportField>,
    table_fields: Vec<ExportTableField>,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ExportClosed> for ExportWizard {}

impl ExportWizard {
    pub fn new(
        session: Shared,
        schema: &str,
        title: &str,
        filters: QueryFilter,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (fields, table_fields) = {
            let s = session.borrow();
            let schema_fields = s
                .fyo
                .schema(schema)
                .map(|s| s.fields.clone())
                .unwrap_or_default();
            (
                export::export_fields(&schema_fields, &[]),
                export::export_table_fields(&s.fyo, &schema_fields),
            )
        };
        let options = vec![
            LinkOption {
                value: "json".into(),
                label: "JSON".into(),
            },
            LinkOption {
                value: "csv".into(),
                label: "CSV".into(),
            },
        ];
        let format = cx.new(|cx| LinkInput::new(options, t("Export Format"), window, cx));
        format.update(cx, |input, cx| {
            input.set_value(Some("csv".into()), window, cx)
        });
        let limit = cx.new(|cx| InputState::new(window, cx).placeholder(t("Limit number of rows")));
        let subscriptions = vec![
            cx.subscribe_in(&format, window, |this, _, event: &LinkPicked, _, cx| {
                this.json = event.0.as_ref() == "json";
                cx.notify();
            }),
            cx.subscribe_in(&limit, window, |_, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
        ];
        Self {
            session,
            schema: schema.to_owned(),
            title: title.to_owned(),
            filters,
            use_list_filters: true,
            json: false,
            format,
            limit,
            fields,
            table_fields,
            error: None,
            _subscriptions: subscriptions,
        }
    }

    /// `filteredTableFields`: the tables whose field is exported.
    fn exported_tables(&self) -> Vec<&ExportTableField> {
        self.table_fields
            .iter()
            .filter(|t| {
                self.fields
                    .iter()
                    .any(|f| f.fieldname == t.fieldname && f.export)
            })
            .collect()
    }

    /// `numSelected`.
    fn selected(&self) -> usize {
        let tables: usize = self
            .exported_tables()
            .iter()
            .map(|t| t.fields.iter().filter(|f| f.export).count())
            .sum();
        tables
            + self
                .fields
                .iter()
                .filter(|f| f.export && f.fieldtype != Some(FieldType::Table))
                .count()
    }

    /// `exportData` and `saveExportData`.
    fn export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let limit = self.limit.read(cx).value().trim().parse::<u64>().ok();
        let filters = if self.use_list_filters {
            self.filters.clone()
        } else {
            QueryFilter::new()
        };
        let data = {
            let s = self.session.borrow();
            let export = if self.json {
                export::json_export
            } else {
                export::csv_export
            };
            export(
                &s.fyo,
                &self.schema,
                &self.fields,
                &self.table_fields,
                limit,
                &filters,
            )
        };
        let data = match data {
            Ok(data) => data,
            Err(e) => {
                self.error = Some(e.to_string());
                cx.notify();
                return;
            }
        };
        // `getFileName`: the title in lower case, dashed, and today's date.
        let extension = if self.json { "json" } else { "csv" };
        let file_name = format!(
            "{}_{}.{extension}",
            self.title.to_lowercase().replace(char::is_whitespace, "-"),
            jiff::Zoned::now().date()
        );
        let dir = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(|home| std::path::PathBuf::from(home).join("Documents"))
            .unwrap_or_default();
        let path = cx.prompt_for_new_path(&dir, Some(&file_name));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = path.await else {
                return;
            };
            let result = std::fs::write(&path, data);
            this.update(cx, |this, cx| match result {
                Ok(()) => cx.emit(ExportClosed(Some(path.clone()))),
                Err(e) => {
                    this.error = Some(e.to_string());
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// A field's checkbox (`Check.vue` with its label on the right).
    fn check(
        &self,
        id: SharedString,
        field: &ExportField,
        table: Option<String>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let fieldname = field.fieldname.clone();
        let checked = field.export;
        div()
            .id(id)
            .h_flex()
            .px_3()
            .py_2()
            .cursor_pointer()
            .child(check_box(checked))
            .child(
                div()
                    .ml_3()
                    .text_12()
                    .text_color(gray(600))
                    .when(field.fieldtype == Some(FieldType::Table), |d| {
                        d.font_weight(FontWeight::SEMIBOLD)
                    })
                    .child(field.label.clone()),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                let fields = match &table {
                    None => Some(&mut this.fields),
                    Some(table) => this
                        .table_fields
                        .iter_mut()
                        .find(|t| &t.fieldname == table)
                        .map(|t| &mut t.fields),
                };
                if let Some(f) =
                    fields.and_then(|f| f.iter_mut().find(|f| f.fieldname == fieldname))
                {
                    f.export = !checked;
                }
                cx.notify();
            }))
    }
}

impl Render for ExportWizard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let schema_label = {
            let s = self.session.borrow();
            s.fyo
                .schema(&self.schema)
                .map(|s| s.label.clone())
                .unwrap_or_else(|| self.schema.clone())
        };
        let hr = || div().w_full().h(px(1.)).flex_none().bg(gray(200));
        let section_title = |title: String| {
            div()
                .text_12()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(gray(800))
                .child(title)
        };
        let main_checks: Vec<Stateful<Div>> = self
            .fields
            .clone()
            .iter()
            .enumerate()
            .map(|(i, f)| self.check(format!("export-field-{i}").into(), f, None, cx))
            .collect();
        let tables: Vec<(String, String, Vec<ExportField>)> = self
            .exported_tables()
            .iter()
            .map(|t| {
                let label = self
                    .session
                    .borrow()
                    .fyo
                    .schema(&t.target)
                    .map(|s| s.label.clone())
                    .unwrap_or_else(|| t.target.clone());
                (t.fieldname.clone(), label, t.fields.clone())
            })
            .collect();
        let table_sections: Vec<AnyElement> = tables
            .into_iter()
            .map(|(table, label, fields)| {
                let checks: Vec<Stateful<Div>> = fields
                    .iter()
                    .enumerate()
                    .map(|(i, f)| {
                        self.check(
                            format!("export-{table}-{i}").into(),
                            f,
                            Some(table.clone()),
                            cx,
                        )
                    })
                    .collect();
                div()
                    .p_4()
                    .child(section_title(label))
                    .child(
                        div()
                            .mt_1()
                            .grid()
                            .grid_cols(3)
                            .border_1()
                            .border_color(gray(200))
                            .rounded(crate::books::RADIUS)
                            .children(checks),
                    )
                    .into_any_element()
            })
            .collect();
        let focused = self.limit.focus_handle(cx).is_focused(window);
        let use_filters = self.use_list_filters;

        div()
            .v_flex()
            .w(crate::books::W_FORM)
            .text_13()
            // `FormHeader.vue`: the list's name and "Export Wizard".
            .child(
                div()
                    .h_flex()
                    .h(px(56.))
                    .px_4()
                    .justify_between()
                    .text_18()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(div().text_color(black()).child(self.title.clone()))
                    .child(div().text_color(gray(600)).child(t("Export Wizard"))),
            )
            .child(hr())
            .child(
                div()
                    .grid()
                    .grid_cols(3)
                    .p_4()
                    .gap_4()
                    .when(!self.filters.is_empty(), |d| {
                        d.child(
                            div()
                                .id("use-list-filters")
                                .h_flex()
                                .justify_between()
                                .min_h(px(34.))
                                .px_3()
                                .rounded(crate::books::RADIUS)
                                .border_1()
                                .border_color(gray(200))
                                .bg(gray(25))
                                .cursor_pointer()
                                .child(div().text_color(gray(600)).child(t("Use List Filters")))
                                .child(check_box(use_filters))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.use_list_filters = !use_filters;
                                    cx.notify();
                                })),
                        )
                    })
                    .child(self.format.clone())
                    .child(
                        control_frame(false, focused).child(
                            gpui_kit::component::input::Input::new(&self.limit)
                                .appearance(false)
                                .h(px(32.))
                                .px_3()
                                .text_13(),
                        ),
                    ),
            )
            .child(hr())
            .child(
                div()
                    .id("export-fields")
                    .max_h(px(320.))
                    .overflow_y_scroll()
                    .child(
                        div().p_4().child(section_title(schema_label)).child(
                            div()
                                .mt_1()
                                .grid()
                                .grid_cols(3)
                                .border_1()
                                .border_color(gray(200))
                                .rounded(crate::books::RADIUS)
                                .children(main_checks),
                        ),
                    )
                    .children(table_sections),
            )
            .child(hr())
            .children(self.error.clone().map(|e| {
                div()
                    .px_4()
                    .pt_2()
                    .text_12()
                    .text_color(crate::books::color("red", 600))
                    .child(e)
            }))
            .child(
                div()
                    .h_flex()
                    .p_4()
                    .justify_between()
                    .child(div().text_12().text_color(gray(600)).child(format!(
                        "{} {}",
                        self.selected(),
                        t("fields selected")
                    )))
                    .child(
                        Button::new("export-data")
                            .primary()
                            .label(t("Export"))
                            .on_click(cx.listener(|this, _, window, cx| this.export(window, cx))),
                    ),
            )
    }
}
