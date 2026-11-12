//! Not in Books: an anonymous count of use, sent once a day to Bloomsynth's telemetry
//! service (telemetry.bloomsynth.com), so we know how many people use Latte Books
//! and which features they use.
//!
//! A ping carries a random ID made on the first run, the app's version, the system, the
//! language and the names of the features used since the last ping, such as `pos` or
//! `doc:SalesInvoice`. Never a company's name, its documents, amounts or file paths. The
//! README's "Usage counts" lists every field.
//!
//! Nothing is sent before the first-run notice has been shown, or while Settings → System's
//! "Send anonymous usage counts" is off. Turning it off also forgets the ID and the features
//! not yet sent. A failed ping is dropped and tried again later; the app never shows it.
//!
//! A debug build doesn't send, unless `LATTE_TELEMETRY_ENDPOINT` names a service to try it
//! with, such as `http://localhost:8787/v1/latte-books/ping`.

use std::collections::BTreeSet;
use std::sync::Mutex;
use std::time::Duration;

use gpui_kit::*;
use latte_core::config::{config_value, set_config_value};
use latte_core::views::sidebar::Route;

use crate::i18n::t;
use crate::widgets::{Kind as DialogKind, Toast, show_toast};

const ENDPOINT: &str = "https://telemetry.bloomsynth.com/v1/latte-books/ping";

/// Settings → System's "Send anonymous usage counts", on unless set to "false".
const ENABLED_KEY: &str = "sendUsageCounts";
/// The random ID, 32 hex digits.
const INSTALL_ID_KEY: &str = "installId";
/// The features used since the last ping, comma-separated.
const FEATURES_KEY: &str = "usageFeatures";
/// The UTC day of the last ping, YYYY-MM-DD.
const LAST_PING_KEY: &str = "usageLastPing";
/// "true" once the first-run notice has been shown.
const NOTICE_KEY: &str = "usageNoticeShown";

/// The most features one ping carries; the service takes up to 200.
const MAX_FEATURES: usize = 200;

/// How often a running app looks whether today's ping is due.
const HOUR: Duration = Duration::from_secs(60 * 60);

/// The features used since the last ping, as kept in the config.
static FEATURES: Mutex<Option<BTreeSet<String>>> = Mutex::new(None);

fn endpoint() -> Option<String> {
    match std::env::var("LATTE_TELEMETRY_ENDPOINT") {
        Ok(endpoint) if !endpoint.is_empty() => Some(endpoint),
        _ => (!cfg!(debug_assertions)).then(|| ENDPOINT.to_owned()),
    }
}

/// Settings → System's "Send anonymous usage counts".
pub fn enabled() -> bool {
    config_value(ENABLED_KEY).as_deref() != Some("false")
}

pub fn set_enabled(on: bool, cx: &mut App) {
    set_config_value(ENABLED_KEY, if on { "true" } else { "false" });
    if !on {
        // Forget what would have been sent.
        set_config_value(INSTALL_ID_KEY, "");
        set_config_value(FEATURES_KEY, "");
        set_config_value(LAST_PING_KEY, "");
        if let Ok(mut features) = FEATURES.lock() {
            *features = Some(BTreeSet::new());
        }
    }
    cx.refresh_windows();
}

/// Whether `name` is a feature name the service takes: a lowercase word, then up to two
/// `:`-separated parts of letters, digits and `_`.
fn valid_feature(name: &str) -> bool {
    let mut parts = name.split(':');
    let Some(first) = parts.next() else {
        return false;
    };
    let word = |s: &str, max: usize| {
        !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    };
    let rest: Vec<&str> = parts.collect();
    word(first, 32)
        && first.as_bytes()[0].is_ascii_lowercase()
        && !first.bytes().any(|b| b.is_ascii_uppercase())
        && rest.len() <= 2
        && rest.iter().all(|part| word(part, 40))
}

fn with_features<R>(f: impl FnOnce(&mut BTreeSet<String>) -> R) -> Option<R> {
    let mut guard = FEATURES.lock().ok()?;
    let features = guard.get_or_insert_with(|| {
        config_value(FEATURES_KEY)
            .unwrap_or_default()
            .split(',')
            .filter(|f| valid_feature(f))
            .map(str::to_owned)
            .collect()
    });
    Some(f(features))
}

fn save_features(features: &BTreeSet<String>) {
    let joined: Vec<&str> = features.iter().map(String::as_str).collect();
    set_config_value(FEATURES_KEY, &joined.join(","));
}

/// Notes that a feature was used, for the next ping.
pub fn record(feature: &str) {
    if !enabled() || !valid_feature(feature) {
        return;
    }
    with_features(|features| {
        if features.len() < MAX_FEATURES && features.insert(feature.to_owned()) {
            save_features(features);
        }
    });
}

/// The feature a screen counts as: its kind, and the document type or report it shows.
pub fn route_feature(route: &Route) -> String {
    match route {
        Route::GetStarted => "get_started".to_owned(),
        Route::Dashboard => "dashboard".to_owned(),
        Route::List { schema, .. } | Route::Form { schema, .. } => format!("doc:{schema}"),
        Route::Report(name) | Route::FilteredReport { name, .. } => format!("report:{name}"),
        Route::ChartOfAccounts => "chart_of_accounts".to_owned(),
        Route::ImportWizard => "import_wizard".to_owned(),
        Route::Settings { tab } => match tab {
            Some(tab) => format!("settings:{tab}"),
            None => "settings".to_owned(),
        },
        Route::Pos => "pos".to_owned(),
        Route::Print { schema, .. } => format!("print:{schema}"),
        Route::ReportPrint(name) => format!("report_print:{name}"),
        Route::TemplateBuilder { .. } => "template_builder".to_owned(),
        Route::AiAssistants => "ai_assistants".to_owned(),
    }
}

/// Today's day in UTC, YYYY-MM-DD.
fn today() -> String {
    jiff::Timestamp::now().to_string()[..10].to_owned()
}

fn install_id() -> String {
    match config_value(INSTALL_ID_KEY) {
        Some(id) if id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()) => id,
        _ => {
            let id = format!("{:032x}", fastrand::u128(..));
            set_config_value(INSTALL_ID_KEY, &id);
            id
        }
    }
}

/// The app's language as the service takes it: letters, `_` and `-`, up to 16.
fn locale() -> Option<String> {
    let language = crate::i18n::language().replace(' ', "_");
    (!language.is_empty()
        && language.len() <= 16
        && language
            .bytes()
            .all(|b| b.is_ascii_alphabetic() || b == b'_' || b == b'-'))
    .then_some(language)
}

/// The ping's body.
fn body(features: &BTreeSet<String>) -> serde_json::Value {
    let mut features = features.clone();
    // The AI assistants connected, such as `assistant:ClaudeDesktop`.
    for assistant in latte_mcp::assistants::all() {
        if assistant.is_connected() && features.len() < MAX_FEATURES {
            features.insert(format!("assistant:{:?}", assistant.kind));
        }
    }
    let mut body = serde_json::json!({
        "install_id": install_id(),
        "version": latte_core::VERSION,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "features": features,
    });
    if let Some(locale) = locale() {
        body["locale"] = locale.into();
    }
    body
}

fn send(endpoint: &str, body: &serde_json::Value) -> Result<(), String> {
    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?
        .post(endpoint)
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .map_err(|e| e.to_string())?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(response.status().to_string())
    }
}

/// Sends today's ping, if it's due.
async fn ping_if_due(endpoint: &str, cx: &mut AsyncApp) {
    let day = today();
    if !enabled()
        || config_value(NOTICE_KEY).as_deref() != Some("true")
        || config_value(LAST_PING_KEY).as_deref() == Some(day.as_str())
    {
        return;
    }
    let Some(sent) = with_features(|features| features.clone()) else {
        return;
    };
    let endpoint = endpoint.to_owned();
    let ok = cx
        .background_executor()
        .spawn(async move { send(&endpoint, &body(&sent)).map(|()| sent) })
        .await;
    if let Ok(sent) = ok {
        set_config_value(LAST_PING_KEY, &day);
        // Features used while it was sending stay for the next ping.
        with_features(|features| {
            features.retain(|f| !sent.contains(f));
            save_features(features);
        });
    }
}

/// Pings when the app starts, then looks every hour whether a new day's ping is due.
pub fn init(cx: &mut App) {
    let Some(endpoint) = endpoint() else {
        return;
    };
    cx.spawn(async move |cx| {
        loop {
            ping_if_due(&endpoint, cx).await;
            cx.background_executor().timer(HOUR).await;
        }
    })
    .detach();
}

/// The first-run notice: says what's sent, once, with a way to Settings to turn it off.
/// No ping is sent before it has been shown.
pub fn show_notice_once(window: &mut Window, cx: &mut App) {
    if config_value(NOTICE_KEY).as_deref() == Some("true") || !enabled() {
        return;
    }
    set_config_value(NOTICE_KEY, "true");
    show_toast(
        Toast::new(
            t("Latte Books sends an anonymous count of its use once a day: its version, your system and the features you open. Never your company's data."),
            DialogKind::Info,
        )
        .action(t("Settings"), |window, cx| {
            crate::desk::navigate(
                Route::Settings {
                    tab: Some("SystemSettings".to_owned()),
                },
                window,
                cx,
            )
        }),
        window,
        cx,
    );
}

#[cfg(test)]
mod tests {
    use super::{Route, route_feature, valid_feature};

    #[test]
    fn feature_names_follow_the_services_pattern() {
        for ok in [
            "pos",
            "doc:SalesInvoice",
            "export:GSTR1:json",
            "settings:SystemSettings",
            "assistant:ClaudeDesktop",
        ] {
            assert!(valid_feature(ok), "{ok}");
        }
        for bad in [
            "",
            "Pos",
            "doc:",
            "doc:Sales Invoice",
            "a:b:c:d",
            "export:GSTR-1",
            "doc:Acme Pvt Ltd",
        ] {
            assert!(!valid_feature(bad), "{bad}");
        }
    }

    #[test]
    fn every_route_is_a_valid_feature() {
        let routes = [
            Route::GetStarted,
            Route::Dashboard,
            Route::list("SalesInvoice", "Sales Invoices", None),
            Route::Report("GSTR1".to_owned()),
            Route::ChartOfAccounts,
            Route::ImportWizard,
            Route::Settings { tab: None },
            Route::Settings {
                tab: Some("SystemSettings".to_owned()),
            },
            Route::Pos,
            Route::Print {
                schema: "SalesInvoice".to_owned(),
                name: "SINV-1001".to_owned(),
            },
            Route::ReportPrint("ProfitAndLoss".to_owned()),
            Route::TemplateBuilder { name: None },
            Route::AiAssistants,
        ];
        for route in routes {
            let feature = route_feature(&route);
            assert!(valid_feature(&feature), "{feature}");
        }
    }
}
