//! Printing a document: `src/pages/PrintView/PrintView.vue` @ v0.36.0.
//!
//! The document is laid out with its PrintTemplate (Typst, `latte-print`) and shown as
//! page images. The template picker lists the schema's PrintTemplates; the "…" menu has
//! Print Settings, New Template, the template itself and Duplicate Template.
//!
//! Differences from Books:
//! - Print opens the PDF in the computer's PDF viewer to print from there; GPUI has no
//!   print dialog.
//! - A Books HTML template isn't printed: the page says so, and offers Convert to Typst,
//!   which makes a Typst copy of it and opens that in the Template Builder.
//! - Every page is shown, one under another; Books shows the first page's area and
//!   scrolls what overflows it.
//! - Duplicate Template copies the template's page size too.

use std::sync::Arc;

use gpui_kit::component::*;
use gpui_kit::*;
use latte_print::templates::{self, PrintTemplate};
use latte_print::{PrintError, Printout};

use crate::books::{Text as _, gray};
use crate::link::{LinkInput, LinkOption, LinkPicked};
use crate::print_parts::{self, Laid, MenuEntry};
use crate::session::Shared;
use crate::widgets::{Button, error_message, page_header};
use crate::{Nav, Navigate, i18n};
use latte_core::views::sidebar::Route;

/// `isPrintable`: whether any template prints documents of this schema.
pub fn can_print(session: &Shared, schema: &str) -> bool {
    templates::can_print(&session.borrow().fyo, schema)
}

pub struct PrintPage {
    session: Shared,
    schema: String,
    name: String,
    template_list: Vec<String>,
    template: Option<PrintTemplate>,
    picker: Entity<LinkInput>,
    printout: Option<Printout>,
    pages: Vec<(Arc<Image>, (f64, f64))>,
    errors: Vec<PrintError>,
    /// A message under the header, such as a failed conversion.
    error: Option<String>,
    /// The "…" menu is open.
    menu: bool,
    /// Which layout is the latest, so an older one finishing late is dropped.
    generation: usize,
    _subscription: Subscription,
}

impl EventEmitter<Navigate> for PrintPage {}

impl PrintPage {
    pub fn new(
        session: Shared,
        schema: &str,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (template_list, default) = {
            let mut s = session.borrow_mut();
            let list = templates::template_names(&s.fyo, schema);
            let default = s
                .fyo
                .get_doc(schema, Some(name))
                .ok()
                .and_then(|doc| templates::default_template_name(&mut s.fyo, &doc));
            (list, default)
        };
        let options = template_list
            .iter()
            .map(|n| LinkOption {
                value: n.clone().into(),
                label: n.clone().into(),
            })
            .collect();
        let picker =
            cx.new(|cx| LinkInput::new(options, i18n::t("Template Name"), window, cx).small());
        let subscription = cx.subscribe_in(
            &picker,
            window,
            |this: &mut Self, _, event: &LinkPicked, _, cx| {
                this.set_template(Some(event.0.to_string()), cx);
            },
        );
        let mut page = Self {
            session,
            schema: schema.to_owned(),
            name: name.to_owned(),
            template_list,
            template: None,
            picker,
            printout: None,
            pages: Vec::new(),
            errors: Vec::new(),
            error: None,
            menu: false,
            generation: 0,
            _subscription: subscription,
        };
        if let Some(default) = &default {
            let value = SharedString::from(default.clone());
            page.picker
                .update(cx, |link, cx| link.set_value(Some(value), window, cx));
        }
        page.set_template(default, cx);
        page
    }

    /// `onTemplateNameChange`.
    fn set_template(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        self.template = name.and_then(|name| {
            let mut s = self.session.borrow_mut();
            match templates::load(&mut s.fyo, &name) {
                Ok(t) => Some(t),
                Err(e) => {
                    self.error = Some(e.to_string());
                    None
                }
            }
        });
        self.render_document(cx);
    }

    /// Lays out the document with the chosen template.
    fn render_document(&mut self, cx: &mut Context<Self>) {
        self.printout = None;
        self.pages.clear();
        self.errors.clear();
        self.generation += 1;
        let Some(template) = self.template.clone() else {
            cx.notify();
            return;
        };
        if template.is_html() {
            cx.notify();
            return;
        }
        let values = {
            let mut s = self.session.borrow_mut();
            let s = &mut *s;
            s.fyo
                .get_doc(&self.schema, Some(&self.name))
                .map_err(|e| PrintError::new(e.to_string()))
                .and_then(|doc| latte_print::values(&mut s.fyo, &s.formatter, &doc, &i18n::t))
        };
        let values = match values {
            Ok(values) => values,
            Err(e) => {
                self.errors = vec![e];
                cx.notify();
                return;
            }
        };
        let source = template.template.clone();
        let size = template.size();
        let task = print_parts::lay_out(
            move || latte_print::render(&source, &values, size, &i18n::t),
            cx,
        );
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
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn save_pdf(&mut self, print: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(printout) = &self.printout {
            print_parts::output(printout.pdf(), print, &self.name, window, cx);
        }
    }

    /// Convert to Typst: a custom Typst copy of a Books HTML template, saved and opened
    /// in the Template Builder. The HTML template is left as it is.
    fn convert(&mut self, cx: &mut Context<Self>) {
        let Some(template) = self.template.clone() else {
            return;
        };
        let result = {
            let mut s = self.session.borrow_mut();
            convert_template(&mut s.fyo, &template)
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

    /// `actions`: the "…" menu.
    fn menu_entries(&self, cx: &mut Context<Self>) -> Vec<MenuEntry> {
        let entity = cx.entity().downgrade();
        let go = move |nav: Nav| {
            let entity = entity.clone();
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                let nav = nav.clone();
                entity
                    .update(cx, |this, cx| {
                        this.menu = false;
                        cx.emit(Navigate(nav));
                    })
                    .ok();
            }
        };
        let mut entries = vec![MenuEntry::new(
            "View",
            i18n::t("Print Settings"),
            go(Nav::Route(Route::Settings {
                tab: Some("PrintSettings".to_owned()),
            })),
        )];
        let new_doc = {
            let s = self.session.borrow();
            templates::new_template(&s.fyo, &self.schema, None).ok()
        };
        if let Some(doc) = new_doc {
            entries.push(MenuEntry::new(
                "Create",
                i18n::t("New Template"),
                go(Nav::Doc(doc)),
            ));
        }
        if let Some(template) = &self.template {
            entries.push(MenuEntry::new(
                "View",
                template.name.clone(),
                go(Nav::Route(Route::Form {
                    schema: "PrintTemplate".to_owned(),
                    name: Some(template.name.clone()),
                })),
            ));
            let copy = {
                let s = self.session.borrow();
                duplicate_template(&s.fyo, template).ok()
            };
            if let Some(doc) = copy {
                entries.push(MenuEntry::new(
                    "Create",
                    i18n::t("Duplicate Template"),
                    go(Nav::Doc(doc)),
                ));
            }
            if template.is_html() {
                let entity = cx.entity().downgrade();
                entries.push(MenuEntry::new(
                    "Create",
                    i18n::t("Convert to Typst"),
                    move |_, _, cx| {
                        entity
                            .update(cx, |this, cx| {
                                this.menu = false;
                                this.convert(cx);
                            })
                            .ok();
                    },
                ));
            }
        }
        entries
    }

    /// `helperMessage`.
    fn helper_message(&self) -> Option<String> {
        if self.template_list.is_empty() {
            let label = self
                .session
                .borrow()
                .fyo
                .schema(&self.schema)
                .map_or_else(|| self.schema.clone(), |s| s.label.clone());
            return Some(format!(
                "{} {label}",
                i18n::t("No Print Templates not found for entry type")
            ));
        }
        if self.template.is_none() {
            return Some(i18n::t("Please select a Print Template"));
        }
        None
    }
}

/// Duplicate Template: a new custom template of the same type with the same source and
/// page size.
pub fn duplicate_template(
    fyo: &latte_core::Fyo,
    template: &PrintTemplate,
) -> Result<latte_core::Doc, latte_core::Error> {
    let mut doc = templates::new_template(fyo, &template.kind, Some(&template.template))?;
    if template.width > 0.0 {
        doc.set_value("width", latte_db::DocValue::Number(template.width));
    }
    if template.height > 0.0 {
        doc.set_value("height", latte_db::DocValue::Number(template.height));
    }
    Ok(doc)
}

/// Converts a Books HTML template to a new custom Typst template, "<name> (Typst)",
/// saved; its name.
pub fn convert_template(
    fyo: &mut latte_core::Fyo,
    template: &PrintTemplate,
) -> Result<String, String> {
    let conversion = latte_print::convert::html_to_typst(&template.template, &template.name);
    let base = format!("{} (Typst)", template.name);
    let mut name = base.clone();
    let mut n = 2;
    while fyo
        .db
        .exists("PrintTemplate", Some(&name))
        .map_err(|e| e.to_string())?
    {
        name = format!("{base} {n}");
        n += 1;
    }
    let copy = PrintTemplate {
        template: conversion.source,
        ..template.clone()
    };
    let mut doc = duplicate_template(fyo, &copy).map_err(|e| e.to_string())?;
    doc.set_value("name", latte_db::DocValue::Text(name.clone()));
    doc.sync(fyo).map_err(|e| e.to_string())?;
    Ok(name)
}

impl Render for PrintPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let menu = self.menu.then(|| {
            let entries = self.menu_entries(cx);
            deferred(anchored().anchor(Anchor::TopRight).snap_to_window().child(
                print_parts::menu_panel("print-menu", entries).on_mouse_down_out(cx.listener(
                    |this, _, _, cx| {
                        this.menu = false;
                        cx.notify();
                    },
                )),
            ))
            .with_priority(1)
        });
        let ready = self.printout.is_some();
        let header = page_header(i18n::t("Print View"), cx).child(
            div()
                .h_flex()
                .gap_2()
                .when(!self.template_list.is_empty(), |d| {
                    d.child(div().w(px(160.)).child(self.picker.clone()))
                })
                .child(
                    div()
                        .relative()
                        .child(
                            Button::new("print-more")
                                .icon("more-horizontal")
                                .tooltip(i18n::t("More"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.menu = !this.menu;
                                    cx.notify();
                                })),
                        )
                        .children(menu),
                )
                .child(
                    Button::new("save-pdf")
                        .primary()
                        .label(i18n::t("Save as PDF"))
                        .disabled(!ready)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.save_pdf(false, window, cx)),
                        ),
                )
                .child(
                    Button::new("print")
                        .primary()
                        .label(i18n::t("Print"))
                        .disabled(!ready)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.save_pdf(true, window, cx)),
                        ),
                ),
        );
        // `setScale`: the page fits the window, never larger than itself.
        let width = self.template.as_ref().map_or(21.0, |t| t.size().0);
        let available = f32::from(window.viewport_size().width) - 32. - 224.;
        let scale = print_parts::fit_scale(width, available);
        let body: AnyElement =
            if let Some(template) = self.template.as_ref().filter(|t| t.is_html()) {
                html_notice(
                    &template.name,
                    cx.listener(|this, _, _, cx| this.convert(cx)),
                )
                .into_any_element()
            } else if !self.errors.is_empty() {
                let source = self.template.as_ref().map_or("", |t| t.template.as_str());
                div()
                    .w(px(width as f32 * 37.8 * scale))
                    .mx_auto()
                    .border_1()
                    .border_color(gray(200))
                    .child(print_parts::errors_view(&self.errors, source))
                    .into_any_element()
            } else {
                print_parts::pages_view(&self.pages, scale).into_any_element()
            };
        div()
            .v_flex()
            .size_full()
            .bg(gray(25))
            .text_13()
            .child(header)
            .children(self.error.clone().map(|e| error_message(e, cx)))
            .child(
                div()
                    .id("pages")
                    .flex_1()
                    .overflow_y_scroll()
                    .p_4()
                    .children(
                        self.helper_message()
                            .map(|m| div().text_12().text_color(gray(700)).child(m)),
                    )
                    .child(body),
            )
    }
}

/// What a Books HTML template shows in place of its printout.
pub fn html_notice(
    name: &str,
    on_convert: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Div {
    div()
        .v_flex()
        .gap_3()
        .max_w(px(480.))
        .mx_auto()
        .mt_8()
        .p_4()
        .rounded(crate::books::RADIUS_LG)
        .border_1()
        .border_color(gray(200))
        .bg(crate::books::white())
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(i18n::t("This is a Frappe Books HTML template")),
        )
        .child(div().text_12().text_color(gray(700)).child(format!(
            "{} \"{name}\" {}",
            i18n::t("Latte prints with Typst templates. Convert"),
            i18n::t(
                "to Typst to make a copy you can print and edit; the HTML template stays as it is for Frappe Books."
            )
        )))
        .child(
            div().h_flex().child(
                Button::new("convert-to-typst")
                    .primary()
                    .label(i18n::t("Convert to Typst"))
                    .on_click(on_convert),
            ),
        )
}

use gpui_kit::prelude::FluentBuilder as _;
