//! Set up AI assistants (Dime): connects the assistants on this computer to Latte Books,
//! as DaVinci Resolve 21.1's File > Setup AI Assistants does. Not in Books.

use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;

use latte_mcp::assistants::{self, Assistant};

use crate::Navigate;
use crate::i18n::t;
use crate::session::{self, Shared};
use crate::widgets::{error_message, field_label, info_message, page_header};

pub struct AssistantsPage {
    assistants: Vec<Assistant>,
    error: Option<String>,
    info: Option<String>,
}

impl EventEmitter<Navigate> for AssistantsPage {}

impl AssistantsPage {
    pub fn new(_: Shared, _: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            assistants: assistants::all(),
            error: None,
            info: None,
        }
    }

    fn connect(&mut self, idx: usize, connect: bool, cx: &mut Context<Self>) {
        self.error = None;
        self.info = None;
        let Some(assistant) = self.assistants.get(idx) else {
            return;
        };
        let result = if connect {
            match assistants::server_path() {
                Some(server) => assistant.connect(&server),
                None => Err(t(MISSING)),
            }
        } else {
            assistant.disconnect()
        };
        let name = assistant.kind.name();
        match result {
            Ok(()) if connect => {
                self.info = Some(format!(
                    "{name} {} {}",
                    t("is connected."),
                    t(assistant.kind.after_connecting())
                ))
            }
            Ok(()) => self.info = Some(format!("{name} {}", t("is disconnected."))),
            Err(e) => self.error = Some(e),
        }
        self.assistants = assistants::all();
        cx.notify();
    }

    fn choose_company(&mut self, path: Option<String>, cx: &mut Context<Self>) {
        session::set_config_value("assistantFilePath", path.as_deref().unwrap_or(""));
        cx.notify();
    }

    fn render_company(&self, cx: &mut Context<Self>) -> Div {
        let chosen = session::config_value("assistantFilePath").filter(|p| !p.is_empty());
        let mut choices: Vec<(Option<String>, String)> =
            vec![(None, t("The company I opened last"))];
        choices.extend(session::recent_files().into_iter().map(|file| {
            let path = file.path.to_string_lossy().to_string();
            let label = if file.company_name.is_empty() {
                path.clone()
            } else {
                file.company_name
            };
            (Some(path), label)
        }));
        div()
            .v_flex()
            .gap_2()
            .child(field_label(t("Assistants work in"), false, cx))
            .child(div().h_flex().flex_wrap().gap_2().children(
                choices.into_iter().enumerate().map(|(i, (path, label))| {
                    let selected = path == chosen;
                    let button = Button::new(("company", i)).small().label(label);
                    let button = if selected {
                        button.primary()
                    } else {
                        button.ghost()
                    };
                    button.on_click(
                        cx.listener(move |this, _, _, cx| this.choose_company(path.clone(), cx)),
                    )
                }),
            ))
    }

    fn render_assistant(&self, idx: usize, assistant: &Assistant, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let connected = assistant.is_connected();
        let status = if connected {
            t("Connected")
        } else if assistant.found {
            t("Not connected")
        } else {
            t("Not found on this computer")
        };
        let button = if connected {
            Button::new(("disconnect", idx))
                .small()
                .label(t("Disconnect"))
                .on_click(cx.listener(move |this, _, _, cx| this.connect(idx, false, cx)))
        } else {
            Button::new(("connect", idx))
                .small()
                .primary()
                .label(t("Connect"))
                .on_click(cx.listener(move |this, _, _, cx| this.connect(idx, true, cx)))
        };
        div()
            .h_flex()
            .gap_4()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex_1()
                    .v_flex()
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .child(assistant.kind.name()),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(if connected {
                                theme.green
                            } else {
                                theme.muted_foreground
                            })
                            .child(status),
                    ),
            )
            .child(button)
    }
}

const MISSING: &str =
    "The part of Latte Books that assistants talk to is missing. Reinstall Latte Books.";

impl Render for AssistantsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let rows: Vec<Div> = self
            .assistants
            .clone()
            .iter()
            .enumerate()
            .map(|(i, a)| self.render_assistant(i, a, cx))
            .collect();
        div()
            .v_flex()
            .size_full()
            .child(page_header(t("Set up AI assistants"), cx))
            .children(self.error.clone().map(|e| error_message(e, cx)))
            .children(self.info.clone().map(|m| info_message(m, cx)))
            .children(
                assistants::server_path()
                    .is_none()
                    .then(|| error_message(t(MISSING), cx)),
            )
            .child(
                div()
                    .id("assistants")
                    .v_flex()
                    .flex_1()
                    .gap_4()
                    .p_4()
                    .max_w(px(640.))
                    .overflow_y_scroll()
                    .child(div().text_color(theme.muted_foreground).child(t(
                        "Work in your books from an AI assistant: ask what a customer owes, \
                         run a report, or have it draft an invoice. Assistants save drafts; \
                         they ask you before they change anything, and before submitting.",
                    )))
                    .child(self.render_company(cx))
                    .children(rows),
            )
    }
}
