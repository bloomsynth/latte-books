//! Printing a document: `src/pages/PrintView/PrintView.vue` @ v0.36.0.
//!
//! The document is laid out with Typst (`latte-print`) and shown as page images.
//!
//! Differences from Books:
//! - "Save as PDF" is the only way out; Books also opens the system print dialog. Print
//!   the saved PDF instead.
//! - There is no template editor; the templates are Typst ports of Books' own.

use std::sync::Arc;

use crate::widgets::Button;
use gpui_kit::component::*;
use gpui_kit::*;
use latte_print::{Printout, Template};

use crate::link::{LinkInput, LinkOption, LinkPicked};
use crate::session::Shared;
use crate::widgets::{error_message, info_message, page_header};
use crate::{Nav, Navigate, i18n};
use latte_core::views::sidebar::Route;

/// Whether Latte has a print template for documents of this schema.
pub fn can_print(schema: &str) -> bool {
    latte_print::can_print(schema)
}

/// Pixels per point for the preview.
const PREVIEW_SCALE: f64 = 1.5;

pub struct PrintPage {
    session: Shared,
    schema: String,
    name: String,
    templates: Vec<Template>,
    template: Option<Template>,
    picker: Entity<LinkInput>,
    printout: Option<Printout>,
    pages: Vec<Arc<Image>>,
    error: Option<String>,
    info: Option<String>,
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
        let (templates, template) = {
            let s = session.borrow();
            (
                latte_print::templates(schema),
                latte_print::default_template(&s.fyo, schema),
            )
        };
        let options = templates
            .iter()
            .map(|t| LinkOption {
                value: t.name.clone().into(),
                label: t.name.clone().into(),
            })
            .collect();
        let picker = cx.new(|cx| LinkInput::new(options, "Template", window, cx));
        let current = template
            .as_ref()
            .map(|t| SharedString::from(t.name.clone()));
        picker.update(cx, |link, cx| link.set_value(current, window, cx));
        let subscription = cx.subscribe_in(
            &picker,
            window,
            |this: &mut Self, _, event: &LinkPicked, _, cx| {
                this.template = this.templates.iter().find(|t| t.name == *event.0).cloned();
                this.render_document(cx);
            },
        );
        let mut page = Self {
            session,
            schema: schema.to_owned(),
            name: name.to_owned(),
            templates,
            template,
            picker,
            printout: None,
            pages: Vec::new(),
            error: None,
            info: None,
            _subscription: subscription,
        };
        page.render_document(cx);
        page
    }

    /// Lays out the document with the chosen template.
    fn render_document(&mut self, cx: &mut Context<Self>) {
        self.printout = None;
        self.pages.clear();
        self.info = None;
        let Some(template) = self.template.clone() else {
            self.error = Some("Pick a template".to_owned());
            cx.notify();
            return;
        };
        let result = {
            let mut s = self.session.borrow_mut();
            let s = &mut *s;
            s.fyo
                .get_doc(&self.schema, Some(&self.name))
                .map_err(|e| e.to_string())
                .and_then(|doc| {
                    latte_print::print(&mut s.fyo, &s.formatter, &doc, &template, &i18n::t)
                })
        };
        let result = result.and_then(|printout| {
            let pages = printout.png_pages(PREVIEW_SCALE)?;
            Ok((printout, pages))
        });
        match result {
            Ok((printout, pages)) => {
                self.pages = pages
                    .into_iter()
                    .map(|png| Arc::new(Image::from_bytes(ImageFormat::Png, png)))
                    .collect();
                self.printout = Some(printout);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }

    fn save_pdf(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
        let file_name = format!("{}.pdf", self.name);
        let dir = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(|home| std::path::PathBuf::from(home).join("Documents"))
            .unwrap_or_default();
        let path = cx.prompt_for_new_path(&dir, Some(&file_name));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = path.await else {
                return;
            };
            let result = std::fs::write(&path, pdf);
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
}

impl Render for PrintPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let back = Route::Form {
            schema: self.schema.clone(),
            name: Some(self.name.clone()),
        };
        let header = page_header(format!("{} - {}", i18n::t("Print"), self.name), cx).child(
            div()
                .h_flex()
                .gap_2()
                .child(div().w(px(260.)).child(self.picker.clone()))
                .child(
                    Button::new("back-to-doc")
                        .small()
                        .ghost()
                        .label(i18n::t("Open Document"))
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.emit(Navigate(Nav::Route(back.clone())))
                        })),
                )
                .child(
                    Button::new("save-pdf")
                        .small()
                        .primary()
                        .label(i18n::t("Save as PDF"))
                        .disabled(self.printout.is_none())
                        .on_click(cx.listener(|this, _, window, cx| this.save_pdf(window, cx))),
                ),
        );
        let pages = self.pages.iter().map(|page| {
            // A4 at the preview scale, shown at half its pixels.
            div()
                .flex_none()
                .w(px(595.0 * PREVIEW_SCALE as f32 / 1.5))
                .h(px(842.0 * PREVIEW_SCALE as f32 / 1.5))
                .bg(gpui_kit::white())
                .border_1()
                .border_color(theme.border)
                .shadow_md()
                .child(img(page.clone()).size_full())
        });
        div()
            .v_flex()
            .size_full()
            .child(header)
            .children(self.error.clone().map(|e| error_message(e, cx)))
            .children(self.info.clone().map(|i| info_message(i, cx)))
            .child(
                div()
                    .id("pages")
                    .flex_1()
                    .overflow_y_scroll()
                    .bg(theme.muted)
                    .child(div().v_flex().items_center().gap_4().py_6().children(pages)),
            )
    }
}
