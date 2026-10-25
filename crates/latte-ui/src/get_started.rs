//! Get Started: `src/pages/GetStarted.vue` @ v0.36.0. A card for each first step, with
//! Set Up and Documentation buttons shown while the pointer is over it, and a check once
//! it's done.
//!
//! Differences from Books:
//! - The cards have no pictures, only the check once done.

use crate::widgets::Button;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use latte_core::views::get_started::{self, Card, Section};

use crate::i18n::t;
use crate::session::Shared;
use crate::widgets::{error_message, page_header};
use crate::{Nav, Navigate};

pub struct GetStartedPage {
    session: Shared,
    sections: Vec<Section>,
    /// `activeCard`: the card the pointer is over.
    active: Option<&'static str>,
    error: Option<String>,
}

impl EventEmitter<Navigate> for GetStartedPage {}

impl GetStartedPage {
    pub fn new(session: Shared, _: &mut Window, _: &mut Context<Self>) -> Self {
        // `activated`.
        let error = get_started::check_for_completed_tasks(&mut session.borrow_mut().fyo)
            .err()
            .map(|e| e.to_string());
        Self {
            session,
            sections: get_started::sections(),
            active: None,
            error,
        }
    }

    fn is_completed(&self, card: &Card) -> bool {
        get_started::is_completed(&self.session.borrow().fyo, card.fieldname)
    }

    /// `handleAction`.
    fn set_up(&mut self, card: &Card, cx: &mut Context<Self>) {
        self.active = None;
        if let Err(e) = get_started::on_action(&mut self.session.borrow_mut().fyo, card) {
            self.error = Some(e.to_string());
        }
        if let Some(route) = card.action.clone() {
            cx.emit(Navigate(Nav::Route(route)));
        }
        cx.notify();
    }

    /// `handleDocumentation`.
    fn documentation(&mut self, card: &Card, cx: &mut Context<Self>) {
        if let Some(url) = card.documentation {
            cx.open_url(url);
        }
        if let Err(e) = get_started::on_documentation(&mut self.session.borrow_mut().fyo, card) {
            self.error = Some(e.to_string());
        }
        cx.notify();
    }

    fn render_card(&self, card: &Card, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = cx.theme().clone();
        let completed = self.is_completed(card);
        let active = self.active == Some(card.key) && !completed;
        let key = card.key;
        let mut buttons = div().h_flex().gap_4().mt_2();
        if card.action.is_some() {
            let card = card.clone();
            buttons = buttons.child(
                Button::new(SharedString::from(format!("set-up-{key}")))
                    .small()
                    .primary()
                    .label(t("Set Up"))
                    .on_click(cx.listener(move |this, _, _, cx| this.set_up(&card, cx))),
            );
        }
        if card.documentation.is_some() {
            let card = card.clone();
            buttons = buttons.child(
                Button::new(SharedString::from(format!("documentation-{key}")))
                    .small()
                    .label(t("Documentation"))
                    .on_click(cx.listener(move |this, _, _, cx| this.documentation(&card, cx))),
            );
        }
        div()
            .id(SharedString::from(format!("card-{key}")))
            .v_flex()
            .justify_between()
            .flex_1()
            .min_w_0()
            .h(px(160.))
            .p_4()
            .border_1()
            .border_color(theme.border)
            .rounded_lg()
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.active = Some(key);
                } else if this.active == Some(key) {
                    this.active = None;
                }
                cx.notify();
            }))
            .child(
                div()
                    .v_flex()
                    .when(completed, |d| {
                        d.child(
                            Icon::new(IconName::CircleCheck)
                                .text_color(theme.green)
                                .mb_4(),
                        )
                    })
                    .child(div().font_weight(FontWeight::MEDIUM).child(t(card.label)))
                    .child(
                        div()
                            .mt_2()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(t(card.description)),
                    ),
            )
            .when(active, |d| d.child(buttons))
    }
}

impl Render for GetStartedPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let sections: Vec<Div> = self
            .sections
            .clone()
            .iter()
            .map(|section| {
                let cards: Vec<_> = section
                    .items
                    .iter()
                    .map(|card| self.render_card(card, cx))
                    .collect();
                div()
                    .v_flex()
                    .p_4()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .child(t(section.label)),
                    )
                    .child(div().h_flex().mt_4().gap_4().children(cards))
            })
            .collect();
        div()
            .v_flex()
            .size_full()
            .child(page_header(t("Set Up Your Workspace"), cx))
            .children(self.error.clone().map(|e| error_message(e, cx)))
            .child(
                div()
                    .id("get-started")
                    .flex_1()
                    .overflow_y_scroll()
                    .children(sections),
            )
    }
}
