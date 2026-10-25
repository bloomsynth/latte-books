//! What the Print View, the Template Builder and the report's Print View share: the
//! pages of a printout drawn at a scale (`PrintContainer.vue` and `ScaledContainer.vue`
//! @ v0.36.0), a template's errors, the grouped "…" menu (`DropdownWithActions.vue`),
//! and Save as PDF and Print (`getPathAndMakePDF`).
//!
//! GPUI has no print dialog, so Print writes the PDF to the temp folder and opens it in
//! the computer's PDF viewer, to print from there (see docs/DEVIATIONS.md).

use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::component::*;
use gpui_kit::*;
use latte_print::{PrintError, Printout};

use crate::books::{self, Text as _, gray};
use crate::i18n::t;
use crate::widgets::{Kind, Toast, dropdown_item, popover_panel, show_toast};

/// A laid out printout: its PDF-ready document and its pages as images, with their
/// sizes in points.
pub struct Laid {
    pub printout: Printout,
    pub pages: Vec<(Arc<Image>, (f64, f64))>,
}

/// Pixels per point of the page images: sharp at 2× zoom.
const PIXELS_PER_PT: f64 = 2.0;

/// Lays out a printout away from the window and its pages as images.
pub fn lay_out(
    job: impl FnOnce() -> Result<Printout, Vec<PrintError>> + Send + 'static,
    cx: &App,
) -> Task<Result<Laid, Vec<PrintError>>> {
    cx.background_executor().spawn(async move {
        let printout = job()?;
        let pngs = printout
            .png_pages(PIXELS_PER_PT)
            .map_err(|e| vec![PrintError::new(e)])?;
        let pages = pngs
            .into_iter()
            .zip(printout.page_sizes())
            .map(|(png, size)| (Arc::new(Image::from_bytes(ImageFormat::Png, png)), size))
            .collect();
        Ok(Laid { printout, pages })
    })
}

/// A point in CSS pixels, as Books' preview sizes a page (`37.8` px to the cm).
const PX_PER_PT: f32 = 96.0 / 72.0;

/// The pages, one under another, `scale` times their size: `ScaledContainer.vue` with
/// `mx-auto shadow-lg border`.
pub fn pages_view(pages: &[(Arc<Image>, (f64, f64))], scale: f32) -> Div {
    div()
        .v_flex()
        .items_center()
        .gap_4()
        .children(pages.iter().map(|(image, (w, h))| {
            div()
                .flex_none()
                .w(px(*w as f32 * PX_PER_PT * scale))
                .h(px(*h as f32 * PX_PER_PT * scale))
                // Paper is white in dark mode too.
                .bg(books::hex(0xFFFFFF))
                .border_1()
                .border_color(gray(200))
                .shadow(books::shadow_lg())
                .child(img(image.clone()).size_full())
        }))
}

/// The scale at which a page `width_cm` wide fits `available` pixels, no more than 1:
/// `setScale`.
pub fn fit_scale(width_cm: f64, available: f32) -> f32 {
    let width = width_cm as f32 * 37.8;
    if width <= 0.0 {
        return 1.0;
    }
    (available / width).clamp(0.1, 1.0)
}

/// `PrintContainer.vue`'s error: a red panel with the title, each message with where it
/// is in the template, and the lines around it.
pub fn errors_view(errors: &[PrintError], source: &str) -> Div {
    let lines: Vec<&str> = source.lines().collect();
    div()
        .v_flex()
        .w_full()
        .gap_4()
        .pb_4()
        .bg(books::color("red", 100))
        .text_20()
        .text_color(gray(900))
        .child(
            div()
                .p_4()
                .border_b_1()
                .border_color(books::color("red", 200))
                .text_size(px(28.))
                .font_weight(FontWeight::BOLD)
                .text_color(books::color("red", 500))
                .child(t("Template Compilation Error")),
        )
        .children(errors.iter().map(|error| {
            let place = match (error.line, error.column) {
                (Some(line), Some(column)) => format!("{line}:{column}  "),
                _ => String::new(),
            };
            let frame = error.line.map(|line| {
                let from = line.saturating_sub(3).max(1);
                let to = (line + 2).min(lines.len());
                let mut out = Vec::new();
                for n in from..=to {
                    let text = lines.get(n - 1).copied().unwrap_or("");
                    out.push(format!("{n:>4} | {text}"));
                    if n == line {
                        let column = error.column.unwrap_or(1);
                        out.push(format!("     | {}^", " ".repeat(column.saturating_sub(1))));
                    }
                }
                out.join("\n")
            });
            div()
                .v_flex()
                .px_4()
                .gap_2()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("{place}{}", error.message)),
                )
                .children(error.hints.iter().map(|h| {
                    div()
                        .text_14()
                        .text_color(gray(700))
                        .child(format!("{}: {h}", t("Hint")))
                }))
                .children(frame.map(|frame| {
                    div()
                        .font_family("monospace")
                        .text_14()
                        .text_color(gray(700))
                        .whitespace_nowrap()
                        .children(frame.lines().map(|l| div().child(l.to_owned())))
                }))
        }))
}

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// An entry of a "…" menu: its group, label and what it does.
pub struct MenuEntry {
    pub group: SharedString,
    pub label: SharedString,
    pub on_click: ClickHandler,
}

impl MenuEntry {
    pub fn new(
        group: &str,
        label: impl Into<SharedString>,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            group: if group.is_empty() {
                SharedString::default()
            } else {
                t(group).into()
            },
            label: label.into(),
            on_click: Box::new(on_click),
        }
    }
}

/// `Dropdown.vue`'s panel: the entries without a group, then each group, by name, under
/// its heading.
pub fn menu_panel(id: &'static str, entries: Vec<MenuEntry>) -> Stateful<Div> {
    let mut groups: Vec<SharedString> = entries
        .iter()
        .map(|e| e.group.clone())
        .filter(|g| !g.is_empty())
        .collect();
    groups.sort();
    groups.dedup();
    let mut ordered: Vec<(Option<SharedString>, MenuEntry)> = Vec::new();
    let mut entries: Vec<Option<MenuEntry>> = entries.into_iter().map(Some).collect();
    for slot in entries.iter_mut() {
        if slot.as_ref().is_some_and(|e| e.group.is_empty()) {
            ordered.push((None, slot.take().expect("an entry")));
        }
    }
    for group in groups {
        let mut first = true;
        for slot in entries.iter_mut() {
            if slot.as_ref().is_some_and(|e| e.group == group) {
                let heading = first.then(|| group.clone());
                first = false;
                ordered.push((heading, slot.take().expect("an entry")));
            }
        }
    }
    popover_panel()
        .id(id)
        .occlude()
        .mt_2()
        .min_w(px(160.))
        .p_1()
        .text_12()
        .children(
            ordered
                .into_iter()
                .enumerate()
                .flat_map(|(i, (heading, entry))| {
                    let heading = heading.map(|h| {
                        div()
                            .px_2()
                            .pt_3()
                            .pb_1()
                            .text_11()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(gray(700))
                            .child(h.to_uppercase())
                            .into_any_element()
                    });
                    let on_click = entry.on_click;
                    let item = dropdown_item((id, i), entry.label)
                        .on_click(move |e, w, cx| on_click(e, w, cx))
                        .into_any_element();
                    heading.into_iter().chain(std::iter::once(item))
                }),
        )
}

/// The folder Save as PDF starts in: Documents.
fn documents() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|home| PathBuf::from(home).join("Documents"))
        .unwrap_or_default()
}

/// Save as PDF (`print` false): asks where and writes the PDF; Print: writes it to the
/// temp folder and opens it with the computer's PDF viewer, whose print dialog prints it.
/// Either way a toast says how it went (`showExportInFolder`).
pub fn output(
    pdf: Result<Vec<u8>, String>,
    print: bool,
    name: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let pdf = match pdf {
        Ok(pdf) => pdf,
        Err(e) => {
            show_toast(Toast::new(e, Kind::Error), window, cx);
            return;
        }
    };
    let file_name = format!("{}.pdf", sanitize(name));
    if print {
        let dir = std::env::temp_dir().join("latte-books-print");
        let path = dir.join(&file_name);
        let toast = match std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, pdf)) {
            Ok(()) => {
                cx.open_with_system(&path);
                Toast::new(t("Opened in your PDF viewer to print"), Kind::Success).short()
            }
            Err(e) => Toast::new(format!("{}: {e}", t("Print Failed")), Kind::Error),
        };
        show_toast(toast, window, cx);
        return;
    }
    let path = cx.prompt_for_new_path(&documents(), Some(&file_name));
    window
        .spawn(cx, async move |cx| {
            let Ok(Ok(Some(path))) = path.await else {
                return;
            };
            let toast = match std::fs::write(&path, pdf) {
                Ok(()) => Toast::new(t("Save as PDF Successful"), Kind::Success).open_folder(&path),
                Err(e) => Toast::new(format!("{}: {e}", t("Export Failed")), Kind::Error),
            };
            cx.update(|window, cx| show_toast(toast, window, cx)).ok();
        })
        .detach();
}

/// A file name from a document's name.
fn sanitize(name: &str) -> String {
    let name: String = name
        .chars()
        .map(|c| if "/\\:*?\"<>|".contains(c) { '-' } else { c })
        .collect();
    if name.trim().is_empty() {
        t("Entry")
    } else {
        name
    }
}

/// `ShortcutKeys.vue` with `simple`: a key combination as Books shows it.
pub fn shortcut(keys: &[&str]) -> Div {
    div().h_flex().gap_1().children(keys.iter().map(|k| {
        div()
            .px_1()
            .rounded(books::RADIUS)
            .border_1()
            .border_color(gray(200))
            .text_11()
            .child((*k).to_owned())
    }))
}

/// `Ctrl` or `⌘`, as the platform names it.
pub fn ctrl() -> &'static str {
    if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl"
    }
}
