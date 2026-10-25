//! The Template Builder: `src/pages/TemplateBuilder/*` @ v0.36.0, where a PrintTemplate
//! is written and previewed. The PrintTemplate list opens it (`formRoute`).
//!
//! The preview shows a document of the template's type (Display Doc) laid out with the
//! template at the Display Scale. The editor is on the right, with Key Hints under it.
//! Ctrl+Enter applies the editor's changes, Ctrl+E toggles edit mode, Ctrl+H the hints,
//! Ctrl+= and Ctrl+- zoom, Ctrl+S saves.
//!
//! Differences from Books:
//! - Templates are Typst, not HTML; the Key Hints also list Latte's print library and
//!   its fonts. A Typst error shows its message, line and column over the preview, and
//!   is marked in the editor.
//! - A Books HTML template is shown read-only, with Convert to Typst.
//! - Select Template File and Save Template File read and write `.typ` files.
//! - The panel between the preview and the editor can't be dragged wider.

use std::collections::HashSet;
use std::sync::Arc;

use gpui_kit::component::input::{Editor, EditorState, Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use latte_core::Doc;
use latte_core::views::sidebar::Route;
use latte_db::{DocValue, Filter, GetAllOptions, Order};
use latte_print::hints::{Hint, LIBRARY_HINTS};
use latte_print::templates::{self, PRINT_SIZES, PrintTemplate, TEMPLATE_TYPES};
use latte_print::{PrintError, Printout};

use crate::books::{self, Text as _, gray, white};
use crate::i18n::t;
use crate::link::{LinkInput, LinkOption, LinkPicked};
use crate::print_parts::{self, Laid, MenuEntry};
use crate::session::Shared;
use crate::widgets::{
    Button, DialogButton, Kind as DialogKind, control_frame, dialog, field_label, modal,
    page_header_with, toast,
};
use crate::{Nav, Navigate};

/// The editor panel's width: `22rem`.
const PANEL_WIDTH: f32 = 22. * 16.;

/// A dialog over the builder.
enum Overlay {
    /// `SetType.vue`.
    SetType(Entity<LinkInput>, String),
    /// `SetPrintSize.vue`: the size picker, height and width.
    SetSize {
        size: Entity<LinkInput>,
        height: Entity<InputState>,
        width: Entity<InputState>,
    },
    /// A message, such as "No Display Entries Found".
    Message(String, String),
    /// Delete, asked first.
    Delete,
}

pub struct TemplateBuilder {
    session: Shared,
    doc: Doc,
    name_input: Option<Entity<InputState>>,
    editor: Entity<EditorState>,
    template_changed: bool,
    hints: Vec<(String, Hint)>,
    show_hints: bool,
    /// The hint rows opened, by their key path.
    open_hints: HashSet<String>,
    display_doc: Option<String>,
    display_picker: Entity<LinkInput>,
    type_picker: Entity<LinkInput>,
    scale: f32,
    scale_input: Entity<InputState>,
    panel_width: f32,
    /// The scale, panel width and sidebar before edit mode, while it is on.
    edit_mode: Option<(f32, f32)>,
    printout: Option<Printout>,
    pages: Vec<(Arc<Image>, (f64, f64))>,
    errors: Vec<PrintError>,
    error: Option<String>,
    info: Option<String>,
    menu: bool,
    overlay: Option<Overlay>,
    generation: usize,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Navigate> for TemplateBuilder {}

fn text(doc: &Doc, field: &str) -> String {
    match doc.get(field) {
        Some(DocValue::Text(t)) => t.clone(),
        _ => String::new(),
    }
}

fn type_options(session: &Shared) -> Vec<LinkOption> {
    let s = session.borrow();
    TEMPLATE_TYPES
        .iter()
        .map(|value| LinkOption {
            value: (*value).into(),
            label: s
                .fyo
                .schema(value)
                .map_or_else(|| (*value).to_owned(), |s| s.label.clone())
                .into(),
        })
        .collect()
}

impl TemplateBuilder {
    /// The PrintTemplate `name`, or a new one.
    pub fn build(session: &Shared, name: Option<&str>) -> Result<Doc, String> {
        let s = session.borrow();
        match name {
            Some(name) => {
                drop(s);
                let mut s = session.borrow_mut();
                s.fyo
                    .get_doc("PrintTemplate", Some(name))
                    .map_err(|e| e.to_string())
            }
            None => {
                templates::new_template(&s.fyo, "SalesInvoice", None).map_err(|e| e.to_string())
            }
        }
    }

    pub fn new(session: Shared, mut doc: Doc, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // `initialize`: a template to start from.
        if text(&doc, "template").is_empty() {
            doc.set_value(
                "template",
                DocValue::Text(templates::BASE_TEMPLATE.to_owned()),
            );
        }
        let custom = PrintTemplate::from_doc(&doc).is_custom;
        let source = text(&doc, "template");
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .soft_wrap(false)
                .default_value(source)
                .placeholder(t("Template"))
        });
        let mut subscriptions = vec![cx.subscribe_in(
            &editor,
            window,
            |this: &mut Self, _, event: &InputEvent, _, cx| match event {
                InputEvent::Change => {
                    if !this.template_changed {
                        this.template_changed = true;
                        cx.notify();
                    }
                }
                // Ctrl+Enter, which the editor takes for itself.
                InputEvent::Blur | InputEvent::PressEnter { secondary: true, .. } => {
                    this.apply_template(cx)
                }
                _ => {}
            },
        )];
        let name_input = doc.not_inserted().then(|| {
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t("Template Name"))
                    .default_value(if templates::is_html(&text(&doc, "template")) {
                        String::new()
                    } else {
                        let name = text(&doc, "name");
                        if session.borrow().fyo.is_temporary_name(&name, doc.schema()) {
                            String::new()
                        } else {
                            name
                        }
                    })
            });
            subscriptions.push(cx.subscribe_in(
                &input,
                window,
                |this: &mut Self, input, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Change | InputEvent::Blur) {
                        let name = input.read(cx).value().trim().to_owned();
                        this.set_field("name", DocValue::Text(name), cx);
                    }
                },
            ));
            input
        });
        let kind = text(&doc, "type");
        let type_picker = cx.new(|cx| {
            LinkInput::new(type_options(&session), t("Template Type"), window, cx).bare()
        });
        type_picker.update(cx, |p, cx| {
            p.set_value(Some(kind.clone().into()), window, cx)
        });
        subscriptions.push(cx.subscribe_in(
            &type_picker,
            window,
            |this: &mut Self, _, event: &LinkPicked, window, cx| {
                if !this.is_custom() {
                    return;
                }
                this.set_type(event.0.to_string(), window, cx);
            },
        ));
        let display_picker =
            cx.new(|cx| LinkInput::new(Vec::new(), t("Display Doc"), window, cx).bare());
        subscriptions.push(cx.subscribe_in(
            &display_picker,
            window,
            |this: &mut Self, _, event: &LinkPicked, _, cx| {
                let name = (!event.0.is_empty()).then(|| event.0.to_string());
                this.set_display_doc(name, cx);
            },
        ));
        let scale_input = cx.new(|cx| InputState::new(window, cx).default_value("0.6"));
        subscriptions.push(cx.subscribe_in(
            &scale_input,
            window,
            |this: &mut Self, input, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change | InputEvent::PressEnter { .. })
                    && let Ok(scale) = input.read(cx).value().trim().parse::<f32>()
                {
                    this.scale = scale.clamp(0.15, 10.);
                    cx.notify();
                }
            },
        ));
        let mut builder = Self {
            session,
            doc,
            name_input,
            editor,
            template_changed: false,
            hints: Vec::new(),
            show_hints: false,
            open_hints: ["doc".to_owned(), "print".to_owned()].into_iter().collect(),
            display_doc: None,
            display_picker,
            type_picker,
            scale: 0.6,
            scale_input,
            panel_width: PANEL_WIDTH,
            edit_mode: None,
            printout: None,
            pages: Vec::new(),
            errors: Vec::new(),
            error: None,
            info: None,
            menu: false,
            overlay: None,
            generation: 0,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        };
        builder
            .editor
            .update(cx, |e, cx| e.set_readonly(!custom, cx));
        builder.set_hints();
        builder.set_display_initial_doc(window, cx);
        builder.focus.focus(window, cx);
        builder
    }

    fn is_custom(&self) -> bool {
        PrintTemplate::from_doc(&self.doc).is_custom
    }

    fn template(&self) -> PrintTemplate {
        PrintTemplate::from_doc(&self.doc)
    }

    /// Sets a field of the template with `doc.set`, which runs its checks.
    fn set_field(&mut self, field: &str, value: DocValue, cx: &mut Context<Self>) {
        let result = {
            let mut s = self.session.borrow_mut();
            self.doc.set(&mut s.fyo, field, value)
        };
        if let Err(e) = result {
            self.error = Some(e.to_string());
        }
        cx.notify();
    }

    fn set_hints(&mut self) {
        let kind = text(&self.doc, "type");
        self.hints = if kind.is_empty() {
            Vec::new()
        } else {
            latte_print::hints::hints(&self.session.borrow().fyo, &kind, &t)
        };
    }

    /// `setType`.
    fn set_type(&mut self, kind: String, window: &mut Window, cx: &mut Context<Self>) {
        self.set_field("type", DocValue::Text(kind.clone()), cx);
        self.type_picker
            .update(cx, |p, cx| p.set_value(Some(kind.into()), window, cx));
        self.set_hints();
        self.set_display_initial_doc(window, cx);
    }

    /// `setDisplayInitialDoc`: the newest document of the template's type that isn't
    /// cancelled.
    fn set_display_initial_doc(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let kind = text(&self.doc, "type");
        if kind.is_empty() {
            return;
        }
        let (names, label) = {
            let s = self.session.borrow();
            let Some(schema) = s.fyo.schema(&kind) else {
                return;
            };
            let mut filters = latte_db::QueryFilter::new();
            if schema.field("cancelled").is_some() {
                filters.insert("cancelled".to_owned(), Filter::from(false));
            }
            let options = GetAllOptions {
                filters,
                order: Some(Order::Desc),
                ..GetAllOptions::default()
            };
            let names: Vec<String> = s
                .fyo
                .db
                .get_all_docs(&kind, &options)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|row| match row.get("name") {
                    Some(DocValue::Text(n)) => Some(n.clone()),
                    _ => None,
                })
                .collect();
            (names, schema.label.clone())
        };
        let options = names
            .iter()
            .map(|n| LinkOption {
                value: n.clone().into(),
                label: n.clone().into(),
            })
            .collect();
        self.display_picker
            .update(cx, |p, cx| p.set_options(options, cx));
        match names.first() {
            Some(first) => {
                let first = first.clone();
                self.display_picker.update(cx, |p, cx| {
                    p.set_value(Some(first.clone().into()), window, cx)
                });
                self.set_display_doc(Some(first), cx);
            }
            None => {
                self.display_picker
                    .update(cx, |p, cx| p.set_value(None, window, cx));
                self.set_display_doc(None, cx);
                self.overlay = Some(Overlay::Message(
                    t("No Display Entries Found"),
                    format!(
                        "{} {label} {}",
                        t("Please create a"),
                        t("entry to view Template Preview.")
                    ),
                ));
            }
        }
    }

    fn set_display_doc(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        self.display_doc = name;
        self.render_preview(cx);
    }

    /// `setTemplate`: the editor's text becomes the template (a custom one only).
    fn apply_template(&mut self, cx: &mut Context<Self>) {
        let changed = std::mem::take(&mut self.template_changed);
        if !self.is_custom() {
            return;
        }
        let value = self.editor.read(cx).value().to_string();
        if value == text(&self.doc, "template") {
            if changed {
                cx.notify();
            }
            return;
        }
        self.set_field("template", DocValue::Text(value), cx);
        self.render_preview(cx);
    }

    /// Lays out the Display Doc with the template.
    fn render_preview(&mut self, cx: &mut Context<Self>) {
        self.printout = None;
        self.pages.clear();
        self.errors.clear();
        self.generation += 1;
        self.mark_errors(cx);
        let template = self.template();
        let (Some(name), false) = (self.display_doc.clone(), template.is_html()) else {
            cx.notify();
            return;
        };
        if template.template.is_empty() {
            cx.notify();
            return;
        }
        let values = {
            let mut s = self.session.borrow_mut();
            let s = &mut *s;
            s.fyo
                .get_doc(&template.kind, Some(&name))
                .map_err(|e| PrintError::new(e.to_string()))
                .and_then(|doc| latte_print::values(&mut s.fyo, &s.formatter, &doc, &t))
        };
        let values = match values {
            Ok(v) => v,
            Err(e) => {
                self.errors = vec![e];
                cx.notify();
                return;
            }
        };
        let source = template.template.clone();
        let size = template.size();
        let task =
            print_parts::lay_out(move || latte_print::render(&source, &values, size, &t), cx);
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                match result {
                    Ok(Laid { printout, pages }) => {
                        this.printout = Some(printout);
                        this.pages = pages;
                    }
                    Err(errors) => this.errors = errors,
                }
                this.mark_errors(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Marks the template's errors in the editor.
    fn mark_errors(&mut self, cx: &mut Context<Self>) {
        use gpui_kit::base::input::Diagnostic;
        use gpui_kit::component::input::Position;
        let marks: Vec<Diagnostic> = self
            .errors
            .iter()
            .filter_map(|e| {
                let (line, column) = (e.line?, e.column?);
                let at = |c: usize| Position {
                    line: (line - 1) as u32,
                    character: (c - 1) as u32,
                };
                Some(Diagnostic::new(
                    at(column)..at(column + 1),
                    e.message.clone(),
                ))
            })
            .collect();
        self.editor.update(cx, |editor, cx| {
            if let Some(diagnostics) = editor.diagnostics_mut() {
                diagnostics.clear();
                diagnostics.extend(marks);
            }
            cx.notify();
        });
    }

    /// `sync`.
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_template(cx);
        let result = {
            let mut s = self.session.borrow_mut();
            self.doc.sync(&mut s.fyo)
        };
        match result {
            Ok(()) => {
                self.name_input = None;
                let custom = self.is_custom();
                self.editor.update(cx, |e, cx| e.set_readonly(!custom, cx));
                self.focus.focus(window, cx);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    fn set_scale(&mut self, scale: f32, window: &mut Window, cx: &mut Context<Self>) {
        self.scale = ((scale * 100.).round() / 100.).clamp(0.15, 10.);
        let value = format!("{}", self.scale);
        self.scale_input
            .update(cx, |input, cx| input.set_value(value, window, cx));
        cx.notify();
    }

    /// `toggleEditMode`: a wider editor, with the preview scaled to the rest.
    fn toggle_edit_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_custom() {
            return;
        }
        if self.display_doc.is_none() {
            self.info = Some(t("Please set a Display Doc"));
            cx.notify();
            return;
        }
        match self.edit_mode.take() {
            Some((scale, panel)) => {
                self.panel_width = panel;
                crate::desk::set_sidebar_hidden(false, cx);
                self.set_scale(scale, window, cx);
            }
            None => {
                self.edit_mode = Some((self.scale, self.panel_width));
                let half = f32::from(window.viewport_size().width) / 2.;
                self.panel_width = half.max(self.panel_width);
                crate::desk::set_sidebar_hidden(true, cx);
                let width = self.template().size().0 as f32 * 37.8;
                let target = half - 16. * 2. - 16. * 0.6;
                self.set_scale(target / width, window, cx);
                self.editor.update(cx, |e, cx| e.focus(window, cx));
            }
        }
        cx.notify();
    }

    fn save_pdf(&mut self, print: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(printout) = &self.printout else {
            return;
        };
        let pdf = match printout.pdf() {
            Ok(pdf) => pdf,
            Err(e) => {
                self.error = Some(e);
                cx.notify();
                return;
            }
        };
        let name = self.doc.name().unwrap_or_default().to_owned();
        if print {
            match print_parts::print_pdf(pdf, &name, cx) {
                Ok(m) => self.info = Some(m),
                Err(e) => self.error = Some(e),
            }
            cx.notify();
            return;
        }
        print_parts::save_pdf(pdf, &name, window, cx, |this, outcome, _| match outcome {
            Ok(m) => this.info = Some(m),
            Err(e) => this.error = Some(e),
        });
    }

    /// Select Template File: a `.typ` file's text becomes the template.
    fn select_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let read = std::fs::read_to_string(&path);
            this.update_in(cx, |this, window, cx| match read {
                Ok(source) => {
                    this.editor
                        .update(cx, |e, cx| e.set_value(source.clone(), window, cx));
                    this.set_field("template", DocValue::Text(source), cx);
                    if this.doc.not_inserted() {
                        let file = path
                            .file_name()
                            .map(|f| f.to_string_lossy().to_string())
                            .unwrap_or_default();
                        let name = file
                            .trim_end_matches(".typ")
                            .trim_end_matches(".template.html")
                            .trim_end_matches(".html")
                            .to_owned();
                        if let Some(input) = &this.name_input {
                            input.update(cx, |i, cx| i.set_value(name.clone(), window, cx));
                        }
                        this.set_field("name", DocValue::Text(name), cx);
                    }
                    this.render_preview(cx);
                }
                Err(e) => {
                    this.error = Some(e.to_string());
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Save Template File: the template as a `.typ` file.
    fn save_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.doc.name().unwrap_or_default().to_owned();
        let template = self.editor.read(cx).value().to_string();
        if name.is_empty() {
            self.info = Some(t("Print Template Name not set"));
            cx.notify();
            return;
        }
        if template.is_empty() {
            self.info = Some(t("Print Template is empty"));
            cx.notify();
            return;
        }
        let extension = if templates::is_html(&template) {
            "template.html"
        } else {
            "typ"
        };
        let dir = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(|home| std::path::PathBuf::from(home).join("Documents"))
            .unwrap_or_default();
        let path = cx.prompt_for_new_path(&dir, Some(&format!("{name}.{extension}")));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = path.await else {
                return;
            };
            let result = std::fs::write(&path, template);
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.info =
                            Some(format!("{}: {}", t("Template file saved"), path.display()))
                    }
                    Err(e) => this.error = Some(e.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Convert to Typst, for a Books HTML template.
    fn convert(&mut self, cx: &mut Context<Self>) {
        let template = self.template();
        let result = {
            let mut s = self.session.borrow_mut();
            crate::print::convert_template(&mut s.fyo, &template)
        };
        match result {
            Ok(name) => cx.emit(Navigate(Nav::Route(Route::Form {
                schema: "PrintTemplate".to_owned(),
                name: Some(name),
            }))),
            Err(e) => {
                self.error = Some(e);
                cx.notify();
            }
        }
    }

    fn open_set_type(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let kind = text(&self.doc, "type");
        let picker = cx
            .new(|cx| LinkInput::new(type_options(&self.session), t("Template Type"), window, cx));
        picker.update(cx, |p, cx| {
            p.set_value(Some(kind.clone().into()), window, cx)
        });
        self._subscriptions.push(cx.subscribe_in(
            &picker,
            window,
            |this: &mut Self, _, event: &LinkPicked, _, cx| {
                if let Some(Overlay::SetType(_, kind)) = &mut this.overlay {
                    *kind = event.0.to_string();
                }
                cx.notify();
            },
        ));
        self.overlay = Some(Overlay::SetType(picker, kind));
        cx.notify();
    }

    fn open_set_size(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (width, height) = self.template().size();
        let name = templates::size_name(width, height);
        let options = PRINT_SIZES
            .iter()
            .map(|(n, _, _)| *n)
            .chain(["Custom"])
            .map(|n| LinkOption {
                value: n.into(),
                label: n.into(),
            })
            .collect();
        let size = cx.new(|cx| LinkInput::new(options, t("Page Size"), window, cx));
        size.update(cx, |p, cx| p.set_value(Some(name.into()), window, cx));
        let number = |v: f64, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).default_value(format!("{v}")))
        };
        let height_input = number(height, window, cx);
        let width_input = number(width, window, cx);
        self._subscriptions.push(cx.subscribe_in(
            &size,
            window,
            |this: &mut Self, _, event: &LinkPicked, window, cx| {
                // `sizeChange`.
                let Some((_, w, h)) = PRINT_SIZES.iter().find(|(n, _, _)| *n == event.0.as_ref())
                else {
                    return;
                };
                if let Some(Overlay::SetSize { height, width, .. }) = &this.overlay {
                    height.update(cx, |i, cx| i.set_value(format!("{h}"), window, cx));
                    width.update(cx, |i, cx| i.set_value(format!("{w}"), window, cx));
                }
            },
        ));
        for input in [&height_input, &width_input] {
            self._subscriptions.push(cx.subscribe_in(
                input,
                window,
                |this: &mut Self, _, event: &InputEvent, window, cx| {
                    // `valueChange`: a typed size is Custom unless it is one of the sizes.
                    if !matches!(event, InputEvent::Change) {
                        return;
                    }
                    if let Some(Overlay::SetSize {
                        size,
                        height,
                        width,
                    }) = &this.overlay
                    {
                        let h = height.read(cx).value().trim().parse::<f64>().unwrap_or(0.);
                        let w = width.read(cx).value().trim().parse::<f64>().unwrap_or(0.);
                        let name = templates::size_name(w, h);
                        size.update(cx, |p, cx| p.set_value(Some(name.into()), window, cx));
                    }
                },
            ));
        }
        self.overlay = Some(Overlay::SetSize {
            size,
            height: height_input,
            width: width_input,
        });
        cx.notify();
    }

    /// Done in Set Template Type or Set Print Size.
    fn done(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.overlay.take() {
            Some(Overlay::SetType(_, kind)) => self.set_type(kind, window, cx),
            Some(Overlay::SetSize { height, width, .. }) => {
                let h = height.read(cx).value().trim().parse::<f64>();
                let w = width.read(cx).value().trim().parse::<f64>();
                if let (Ok(h), Ok(w)) = (h, w) {
                    self.set_field("width", DocValue::Number(w), cx);
                    self.set_field("height", DocValue::Number(h), cx);
                    self.render_preview(cx);
                }
            }
            _ => {}
        }
        cx.notify();
    }

    fn delete(&mut self, cx: &mut Context<Self>) {
        let result = {
            let mut s = self.session.borrow_mut();
            self.doc.delete(&mut s.fyo)
        };
        match result {
            Ok(()) => cx.emit(Navigate(Nav::Route(Route::list(
                "PrintTemplate",
                "Print Templates",
                None,
            )))),
            Err(e) => {
                self.error = Some(e.to_string());
                cx.notify();
            }
        }
    }

    /// `actions`: the document's, then Print Settings and the template's own.
    fn menu_entries(&self, cx: &mut Context<Self>) -> Vec<MenuEntry> {
        let this = cx.entity().downgrade();
        let run = move |f: fn(&mut Self, &mut Window, &mut Context<Self>)| {
            let this = this.clone();
            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                this.update(cx, |this, cx| {
                    this.menu = false;
                    f(this, window, cx);
                    cx.notify();
                })
                .ok();
            }
        };
        let custom = self.is_custom();
        let mut entries = Vec::new();
        if self.doc.inserted() {
            entries.push(MenuEntry::new(
                "Create",
                t("Duplicate"),
                run(|this, _, cx| {
                    let copy = {
                        let s = this.session.borrow();
                        this.doc.duplicate(&s.fyo)
                    };
                    match copy {
                        Ok(doc) => cx.emit(Navigate(Nav::Doc(doc))),
                        Err(e) => this.error = Some(e.to_string()),
                    }
                }),
            ));
        }
        if self.doc.can_delete() {
            entries.push(MenuEntry::new(
                "",
                t("Delete"),
                run(|this, _, _| {
                    this.overlay = Some(Overlay::Delete);
                }),
            ));
        }
        entries.push(MenuEntry::new(
            "View",
            t("Print Settings"),
            run(|_, _, cx| {
                cx.emit(Navigate(Nav::Route(Route::Settings {
                    tab: Some("PrintSettings".to_owned()),
                })))
            }),
        ));
        if custom {
            entries.push(MenuEntry::new(
                "Action",
                t("Set Template Type"),
                run(Self::open_set_type),
            ));
            entries.push(MenuEntry::new(
                "Action",
                t("Set Print Size"),
                run(Self::open_set_size),
            ));
            entries.push(MenuEntry::new(
                "Action",
                t("Select Template File"),
                run(Self::select_file),
            ));
        }
        entries.push(MenuEntry::new(
            "Action",
            t("Save Template File"),
            run(Self::save_file),
        ));
        if self.template().is_html() {
            entries.push(MenuEntry::new(
                "Create",
                t("Convert to Typst"),
                run(|this, _, cx| this.convert(cx)),
            ));
        }
        entries
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        if !(k.modifiers.control || k.modifiers.platform) {
            return;
        }
        match k.key.as_str() {
            "enter" => self.apply_template(cx),
            "e" => self.toggle_edit_mode(window, cx),
            "h" => {
                self.show_hints = !self.show_hints;
                cx.notify();
            }
            "=" | "+" => self.set_scale(self.scale + 0.1, window, cx),
            "-" => self.set_scale(self.scale - 0.1, window, cx),
            "s" => self.save(window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    /// `helperMessage`.
    fn helper_message(&self) -> Option<String> {
        if text(&self.doc, "type").is_empty() {
            return Some(t("Select a Template type"));
        }
        if self.display_doc.is_none() {
            return Some(t("Select a Display Doc to view the Template"));
        }
        if text(&self.doc, "template").is_empty() {
            return Some(t("Set a Template value to see the Print Template"));
        }
        None
    }

    fn render_header(&mut self, cx: &mut Context<Self>) -> Div {
        let title = if self.doc.inserted() {
            self.doc.name().unwrap_or_default().to_owned()
        } else {
            String::new()
        };
        let left = self.name_input.clone().map(|input| {
            div().w(px(240.)).flex_none().child(
                control_frame(false, false).child(
                    Input::new(&input)
                        .appearance(false)
                        .h(px(30.))
                        .px_2()
                        .text_18()
                        .font_weight(FontWeight::SEMIBOLD),
                ),
            )
        });
        let ready = self.printout.is_some() && self.display_doc.is_some();
        let custom = self.is_custom();
        let can_save = self.doc.not_inserted() || self.doc.dirty();
        let menu = self.menu.then(|| {
            let entries = self.menu_entries(cx);
            deferred(anchored().anchor(Anchor::TopRight).snap_to_window().child(
                print_parts::menu_panel("builder-menu", entries).on_mouse_down_out(cx.listener(
                    |this, _, _, cx| {
                        this.menu = false;
                        cx.notify();
                    },
                )),
            ))
            .with_priority(1)
        });
        page_header_with(title, div().children(left), cx).child(
            div()
                .h_flex()
                .gap_2()
                .when(ready, |d| {
                    d.child(
                        Button::new("builder-save-pdf")
                            .label(t("Save as PDF"))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.save_pdf(false, window, cx)),
                            ),
                    )
                    .child(
                        Button::new("builder-print").label(t("Print")).on_click(
                            cx.listener(|this, _, window, cx| this.save_pdf(true, window, cx)),
                        ),
                    )
                })
                .when(custom && self.display_doc.is_some(), |d| {
                    d.child(
                        Button::new("builder-edit-mode")
                            .icon("edit")
                            .tooltip(t("Toggle Edit Mode"))
                            .on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.toggle_edit_mode(window, cx)
                                }),
                            ),
                    )
                })
                .child(
                    div()
                        .relative()
                        .child(
                            Button::new("builder-more")
                                .icon("more-horizontal")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.menu = !this.menu;
                                    cx.notify();
                                })),
                        )
                        .children(menu),
                )
                .when(can_save, |d| {
                    d.child(
                        Button::new("builder-save")
                            .primary()
                            .label(t("Save"))
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    )
                }),
        )
    }

    /// `TemplateBuilderHint.vue`: keys and their labels, objects and lists folded open
    /// with a click.
    fn render_hints(
        &self,
        hints: &[(String, Hint)],
        prefix: &str,
        level: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut rows: Vec<&(String, Hint)> = hints.iter().collect();
        rows.sort_by_key(|(_, h)| !matches!(h, Hint::Value(_)));
        let container = div().v_flex().when(level > 0, |d| {
            d.ml_2().pl_2().border_l_1().border_color(gray(200))
        });
        container.children(rows.into_iter().map(|(key, hint)| {
            let path = match hint {
                Hint::Array(_) => format!("{prefix}.{key}[number]"),
                _ if prefix.is_empty() => key.clone(),
                _ => format!("{prefix}.{key}"),
            };
            let open = self.open_hints.contains(&path);
            let row = div()
                .id(SharedString::from(format!("hint-{path}")))
                .h_flex()
                .gap_2()
                .text_12()
                .text_color(gray(600))
                .whitespace_nowrap()
                .child(path.clone());
            match hint {
                Hint::Value(label) => div().child(
                    row.child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(gray(800))
                            .child(label.clone()),
                    ),
                ),
                Hint::Object(children) | Hint::Array(children) => {
                    let (badge, color) = if matches!(hint, Hint::Array(_)) {
                        ("Array", "blue")
                    } else {
                        ("Object", "pink")
                    };
                    let toggle = path.clone();
                    div()
                        .child(
                            row.cursor_pointer()
                                .child(
                                    div()
                                        .px_1()
                                        .rounded(books::RADIUS)
                                        .border_1()
                                        .border_color(white())
                                        .bg(books::color(color, 100))
                                        .text_color(books::color(color, 600))
                                        .text_11()
                                        .child(badge),
                                )
                                .child(div().ml_auto().child(books::icon(
                                    if open { "chevron-up" } else { "chevron-down" },
                                    px(16.),
                                    gray(600),
                                )))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !this.open_hints.remove(&toggle) {
                                        this.open_hints.insert(toggle.clone());
                                    }
                                    cx.notify();
                                })),
                        )
                        .when(open, |d| {
                            d.child(self.render_hints(children, &path, level + 1, cx))
                        })
                }
            }
        }))
    }

    fn render_overlay(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let close = cx.listener(|this, _, _, cx| {
            this.overlay = None;
            cx.notify();
        });
        let done = || {
            div()
                .h_flex()
                .p_4()
                .border_t_1()
                .border_color(gray(200))
                .child(
                    div().ml_auto().child(
                        Button::new("builder-done")
                            .primary()
                            .label(t("Done"))
                            .on_click(cx.listener(|this, _, window, cx| this.done(window, cx))),
                    ),
                )
        };
        let form = |title: String, body: Div| {
            div()
                .w(books::W_FORM)
                .v_flex()
                .child(
                    div()
                        .h(px(60.))
                        .h_flex()
                        .px_4()
                        .text_18()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .child(div().border_t_1().border_color(gray(200)))
                .child(body.p_4().w_full().v_flex().gap_4())
        };
        match self.overlay.as_ref()? {
            Overlay::SetType(picker, _) => {
                let content = form(
                    t("Set Template Type"),
                    div()
                        .child(
                            div()
                                .text_13()
                                .text_color(gray(900))
                                .child(t("Select the template type.")),
                        )
                        .child(
                            div()
                                .child(field_label(t("Template Type"), false, cx))
                                .child(picker.clone()),
                        ),
                )
                .child(done());
                Some(modal(content, close, window).into_any_element())
            }
            Overlay::SetSize {
                size,
                height,
                width,
            } => {
                let number = |label: String, input: &Entity<InputState>| {
                    div()
                        .flex_1()
                        .child(field_label(label, false, cx))
                        .child(crate::widgets::small_input(input))
                };
                let content = form(
                    t("Set Print Size"),
                    div()
                        .child(div().text_13().text_color(gray(900)).child(t(
                            "Select a pre-defined page size, or set a custom page size for your Print Template.",
                        )))
                        .child(
                            div()
                                .child(field_label(t("Page Size"), false, cx))
                                .child(size.clone()),
                        )
                        .child(
                            div()
                                .h_flex()
                                .gap_4()
                                .w_full()
                                .child(number(t("Height (in cm)"), height))
                                .child(number(t("Width (in cm)"), width)),
                        ),
                )
                .child(done());
                Some(modal(content, close, window).into_any_element())
            }
            Overlay::Message(title, detail) => Some(
                dialog(
                    title.clone(),
                    Some(detail.clone().into()),
                    DialogKind::Warning,
                    vec![DialogButton::new(
                        t("Okay"),
                        false,
                        cx.listener(|this, _, _, cx| {
                            this.overlay = None;
                            cx.notify();
                        }),
                    )],
                    window,
                )
                .into_any_element(),
            ),
            Overlay::Delete => Some(
                dialog(
                    format!("{} {}?", t("Delete"), self.doc.name().unwrap_or_default()),
                    Some(t("This action is permanent").into()),
                    DialogKind::Warning,
                    vec![
                        DialogButton::new(
                            t("Cancel"),
                            false,
                            cx.listener(|this, _, _, cx| {
                                this.overlay = None;
                                cx.notify();
                            }),
                        ),
                        DialogButton::new(
                            t("Yes"),
                            true,
                            cx.listener(|this, _, _, cx| {
                                this.overlay = None;
                                this.delete(cx);
                            }),
                        ),
                    ],
                    window,
                )
                .into_any_element(),
            ),
        }
    }
}

impl Render for TemplateBuilder {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = self.render_header(cx);
        let template = self.template();
        let custom = template.is_custom;
        let source = template.template.clone();
        let width = template.size().0 as f32;

        // The preview, or what is missing for one.
        let preview: AnyElement = if template.is_html() {
            crate::print::html_notice(
                &template.name,
                cx.listener(|this, _, _, cx| this.convert(cx)),
            )
            .into_any_element()
        } else if let Some(message) = self.helper_message() {
            div()
                .p_4()
                .text_12()
                .text_color(gray(700))
                .child(message)
                .into_any_element()
        } else if !self.errors.is_empty() {
            div()
                .p_4()
                .child(
                    div()
                        .w(px(width * 37.8 * self.scale))
                        .mx_auto()
                        .border_1()
                        .border_color(gray(200))
                        .child(print_parts::errors_view(&self.errors, &source)),
                )
                .into_any_element()
        } else {
            div()
                .p_4()
                .child(print_parts::pages_view(&self.pages, self.scale))
                .into_any_element()
        };

        // The bar under the preview: type, display doc and scale.
        let bar_cell = |child: AnyElement, title: String| {
            div()
                .id(SharedString::from(format!("bar-{title}")))
                .w(px(160.))
                .flex_none()
                .h_full()
                .h_flex()
                .px_1()
                .border_r_1()
                .border_color(gray(200))
                .child(child)
        };
        let bottom_bar = div()
            .h_flex()
            .w_full()
            .flex_none()
            .h(px(36.))
            .bg(white())
            .border_t_1()
            .border_color(gray(200))
            .child(bar_cell(
                self.type_picker.clone().into_any_element(),
                t("Template Type"),
            ))
            .when(!text(&self.doc, "type").is_empty(), |d| {
                d.child(bar_cell(
                    self.display_picker.clone().into_any_element(),
                    t("Display Doc"),
                ))
            })
            .when(self.printout.is_some() || !self.errors.is_empty(), |d| {
                d.child(
                    div()
                        .ml_auto()
                        .h_flex()
                        .gap_2()
                        .px_2()
                        .w(px(144.))
                        .flex_none()
                        .justify_between()
                        .child(
                            div()
                                .text_12()
                                .text_color(gray(600))
                                .child(t("Display Scale")),
                        )
                        .child(
                            div().w(px(40.)).child(
                                Input::new(&self.scale_input)
                                    .appearance(false)
                                    .text_13()
                                    .text_color(gray(800)),
                            ),
                        ),
                )
            });

        // The editor panel: the editor, "Ctrl+Enter to apply changes", Key Hints.
        let hints_open = self.show_hints;
        let hints = div()
            .flex_none()
            .border_t_1()
            .border_color(gray(200))
            .child(
                div()
                    .id("hints-toggle")
                    .h_flex()
                    .justify_between()
                    .p_2()
                    .cursor_pointer()
                    .child(
                        div()
                            .text_13()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(gray(900))
                            .child(t("Key Hints")),
                    )
                    .child(books::icon(
                        if hints_open {
                            "chevron-up"
                        } else {
                            "chevron-down"
                        },
                        px(16.),
                        gray(600),
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_hints = !this.show_hints;
                        cx.notify();
                    })),
            )
            .when(hints_open, |d| {
                let max_h = f32::from(window.viewport_size().height) * 0.3;
                let tree = self.render_hints(&self.hints.clone(), "", 0, cx);
                d.child(
                    div()
                        .id("hints")
                        .max_h(px(max_h))
                        .overflow_y_scroll()
                        .p_2()
                        .border_t_1()
                        .border_color(gray(200))
                        .child(tree)
                        .child(
                            div()
                                .mt_3()
                                .mb_1()
                                .text_11()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(gray(700))
                                .child(t("LATTE'S PRINT LIBRARY (/latte.typ)").to_uppercase()),
                        )
                        .children(LIBRARY_HINTS.iter().map(|(key, label)| {
                            div()
                                .h_flex()
                                .gap_2()
                                .text_12()
                                .text_color(gray(600))
                                .child(div().whitespace_nowrap().child(*key))
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(gray(800))
                                        .child(t(label)),
                                )
                        })),
                )
            });
        let apply_hint = self.template_changed.then(|| {
            div()
                .h_flex()
                .gap_2()
                .p_2()
                .text_12()
                .text_color(gray(600))
                .border_t_1()
                .border_color(gray(200))
                .child(print_parts::shortcut(&[print_parts::ctrl(), "Enter"]))
                .child(t(" to apply changes"))
        });
        let panel = div()
            .w(px(self.panel_width))
            .flex_none()
            .h_full()
            .v_flex()
            .border_l_1()
            .border_color(gray(200))
            .bg(white())
            .child(
                div().flex_1().min_h_0().child(
                    Editor::new(&self.editor)
                        .appearance(false)
                        .readonly(!custom)
                        .h_full()
                        .text_13(),
                ),
            )
            .children(apply_hint)
            .child(hints);

        let overlay = self.render_overlay(window, cx);
        let toast = self.info.clone().map(|message| {
            toast(
                message,
                crate::widgets::Kind::Success,
                cx.listener(|this, _, _, cx| {
                    this.info = None;
                    cx.notify();
                }),
                window,
            )
        });
        let error = self.error.clone().map(|e| {
            dialog(
                t("Error"),
                Some(e.into()),
                DialogKind::Error,
                vec![DialogButton::new(
                    t("Okay"),
                    false,
                    cx.listener(|this, _, _, cx| {
                        this.error = None;
                        cx.notify();
                    }),
                )],
                window,
            )
        });

        div()
            .id("template-builder")
            .track_focus(&self.focus)
            .v_flex()
            .size_full()
            .text_13()
            .capture_key_down(cx.listener(Self::on_key))
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .h_flex()
                    .bg(gray(50))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .v_flex()
                            .child(
                                div()
                                    .id("builder-preview")
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_scroll()
                                    .child(preview),
                            )
                            .child(bottom_bar),
                    )
                    .child(panel),
            )
            .children(toast)
            .children(error)
            .children(overlay)
    }
}
