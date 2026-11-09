//! Not in Books: updates from GitHub Releases, with nothing else to host.
//!
//! Each release carries `latest.json`, which names the version and, for each system, the
//! file to update with and its signature (`.github/workflows/release.yml`). The app reads
//! it from `releases/latest/download/latest.json`, which always points at the newest
//! release, when it starts and then once a day, unless Settings → System turns that off.
//!
//! A newer version shows at the foot of the sidebar. Updating downloads the file from the
//! same release, checks its signature against the key built in here, and installs it:
//! Windows runs the new installer, which reopens the app; macOS swaps the app bundle and
//! Linux the AppImage, then the app opens again. A .deb can't update itself, so it gets a
//! link to the release instead.
//!
//! A debug build doesn't check, unless `LATTE_UPDATE_ENDPOINT` names a `latest.json` to
//! try it with.

use std::sync::Arc;
use std::time::Duration;

use cargo_packager_updater::semver::Version;
use cargo_packager_updater::{Config, Update, UpdaterBuilder};
use gpui_kit::*;

use crate::i18n::t;
use crate::widgets::{DialogButton, Kind as DialogKind, Toast, dialog, show_toast};

/// The newest release's `latest.json`.
const ENDPOINT: &str =
    "https://github.com/bloomsynth/latte-books/releases/latest/download/latest.json";

/// The newest release's page, for a .deb to download from.
const RELEASES: &str = "https://github.com/bloomsynth/latte-books/releases/latest";

/// The public half of the key releases are signed with (minisign, base64). The secret half
/// is the release workflow's `CARGO_PACKAGER_SIGN_PRIVATE_KEY`.
const PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDA5NzRFNDRFNkQxNEQxNDIKUldSQzBSUnRUdVIwQ2V5eHZSdTFNTCthd1lSaXFOdmlkMnRKNTM3WStuUS9UWlNWRDNXaFIrU3oK";

/// The config key for Settings → System's "Check for updates", on unless set to "false".
const ENABLED_KEY: &str = "checkForUpdates";

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// Where updating stands.
#[derive(Clone, Default)]
pub enum State {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(Arc<Update>),
    Installing,
    Failed(String),
}

#[derive(Default)]
struct Updates {
    state: State,
    /// The update dialog is open.
    dialog: bool,
}

impl Global for Updates {}

/// The `latest.json` to check: GitHub's in a release build, or `LATTE_UPDATE_ENDPOINT`.
fn endpoint() -> Option<String> {
    match std::env::var("LATTE_UPDATE_ENDPOINT") {
        Ok(endpoint) if !endpoint.is_empty() => Some(endpoint),
        _ => (!cfg!(debug_assertions)).then(|| ENDPOINT.to_owned()),
    }
}

/// Whether an update can install itself: on Windows and macOS, and on Linux from the
/// AppImage. A .deb is its package manager's to update.
pub fn can_install() -> bool {
    cfg!(any(windows, target_os = "macos")) || std::env::var_os("APPIMAGE").is_some()
}

/// Settings → System's "Check for updates".
pub fn enabled() -> bool {
    latte_core::config::config_value(ENABLED_KEY).as_deref() != Some("false")
}

pub fn set_enabled(on: bool, cx: &mut App) {
    latte_core::config::set_config_value(ENABLED_KEY, if on { "true" } else { "false" });
    if on && matches!(state(cx), State::Idle) {
        check_now(cx);
    }
    cx.refresh_windows();
}

pub fn state(cx: &App) -> State {
    cx.try_global::<Updates>()
        .map(|u| u.state.clone())
        .unwrap_or_default()
}

fn set_state(state: State, cx: &mut App) {
    cx.default_global::<Updates>().state = state;
    cx.refresh_windows();
}

/// Checks when the app starts and then once a day, while "Check for updates" is on.
pub fn init(cx: &mut App) {
    cx.set_global(Updates::default());
    if endpoint().is_none() {
        return;
    }
    cx.spawn(async move |cx| {
        loop {
            if enabled() {
                check(cx).await;
            }
            cx.background_executor().timer(DAY).await;
        }
    })
    .detach();
}

/// Settings → System's Check Now.
pub fn check_now(cx: &mut App) {
    if endpoint().is_none() {
        set_state(
            State::Failed(t(
                "This copy of Latte Books was built from source, so it doesn't update itself.",
            )),
            cx,
        );
        return;
    }
    cx.spawn(async move |cx| check(cx).await).detach();
}

async fn check(cx: &mut AsyncApp) {
    let Some(endpoint) = endpoint() else {
        return;
    };
    if matches!(
        cx.update(|cx| state(cx)),
        State::Checking | State::Installing
    ) {
        return;
    }
    cx.update(|cx| set_state(State::Checking, cx));
    let result = cx
        .background_executor()
        .spawn(async move { find_update(&endpoint) })
        .await;
    cx.update(|cx| {
        set_state(
            match result {
                Ok(Some(update)) => State::Available(Arc::new(update)),
                Ok(None) => State::UpToDate,
                Err(e) => State::Failed(format!("{}: {e}", t("Couldn't check for updates"))),
            },
            cx,
        )
    });
}

/// A newer version than this one, if `latest.json` names one. A release without
/// `latest.json`, such as one made before the updater, has none.
fn find_update(endpoint: &str) -> Result<Option<Update>, String> {
    let current: Version = latte_core::VERSION.parse().map_err(|e| format!("{e}"))?;
    let config = Config {
        endpoints: vec![endpoint.parse().map_err(|e| format!("{e}"))?],
        pubkey: PUBKEY.to_owned(),
        windows: None,
    };
    let updater = UpdaterBuilder::new(current, config)
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    match updater.check() {
        Ok(update) => Ok(update),
        Err(cargo_packager_updater::Error::ReleaseNotFound) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Opens the dialog that offers the update.
pub fn open_dialog(cx: &mut App) {
    cx.default_global::<Updates>().dialog = true;
    cx.refresh_windows();
}

fn close_dialog(cx: &mut App) {
    cx.default_global::<Updates>().dialog = false;
    cx.refresh_windows();
}

/// Downloads the update, checks its signature and installs it, then opens the app again.
fn install(update: Arc<Update>, window: &mut Window, cx: &mut App) {
    close_dialog(cx);
    set_state(State::Installing, cx);
    window
        .spawn(cx, async move |cx| {
            let task_update = update.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    // On Windows this starts the new installer and ends the app; the
                    // installer opens it again.
                    task_update
                        .download_and_install()
                        .map_err(|e| e.to_string())
                })
                .await;
            cx.update(|window, cx| match result {
                Ok(()) => {
                    if let Some(appimage) = std::env::var_os("APPIMAGE") {
                        cx.set_restart_path(appimage.into());
                    }
                    cx.restart();
                }
                Err(e) => {
                    set_state(State::Available(update), cx);
                    show_toast(
                        Toast::new(format!("{}: {e}", t("Couldn't update")), DialogKind::Error),
                        window,
                        cx,
                    );
                }
            })
            .ok();
        })
        .detach();
}

/// The update dialog, drawn by the window's root while it is open.
pub fn dialog_element(window: &Window, cx: &App) -> Option<AnyElement> {
    if !cx.try_global::<Updates>().is_some_and(|u| u.dialog) {
        return None;
    }
    let State::Available(update) = state(cx) else {
        return None;
    };
    let title = format!("{} {}", t("Latte Books"), update.version);
    let mut detail = format!("{} {}.", t("Version"), update.version);
    detail = match update
        .body
        .as_deref()
        .map(str::trim)
        .filter(|b| !b.is_empty())
    {
        Some(notes) => {
            // The notes are the changelog's markdown; the dialog shows plain text.
            let notes: String = notes
                .replace("**", "")
                .replace('`', "")
                .chars()
                .take(600)
                .collect();
            format!("{detail}\n\n{notes}")
        }
        None => detail,
    };
    let later = DialogButton::new(t("Later"), false, |_, _, cx| close_dialog(cx)).escape();
    let buttons = if can_install() {
        detail.push_str("\n\n");
        detail.push_str(&t(
            "Latte Books will close, update and open again. Save what you're working on first.",
        ));
        let update = update.clone();
        vec![
            later,
            DialogButton::new(t("Update and Restart"), true, move |_, window, cx| {
                install(update.clone(), window, cx)
            }),
        ]
    } else {
        detail.push_str("\n\n");
        detail.push_str(&t("Download it from the release page and install it."));
        vec![
            later,
            DialogButton::new(t("Download"), true, |_, _, cx| {
                close_dialog(cx);
                cx.open_url(RELEASES);
            }),
        ]
    };
    Some(
        dialog(
            format!("{title} {}", t("is available")),
            Some(detail.into()),
            DialogKind::Info,
            buttons,
            window,
        )
        .into_any_element(),
    )
}

/// What the sidebar's foot says under the version: an update to install, or one under way.
pub fn sidebar_notice(cx: &App) -> Option<(String, bool)> {
    match state(cx) {
        State::Available(update) => Some((format!("{} v{}", t("Update to"), update.version), true)),
        State::Installing => Some((t("Updating…"), false)),
        _ => None,
    }
}

/// Where updating stands, for Settings → System.
pub fn status_text(cx: &App) -> Option<String> {
    match state(cx) {
        State::Idle => None,
        State::Checking => Some(t("Checking for updates…")),
        State::UpToDate => Some(format!(
            "{} {} {}",
            t("Latte Books"),
            latte_core::VERSION,
            t("is the latest version.")
        )),
        State::Available(update) => Some(format!(
            "{} {} {}",
            t("Latte Books"),
            update.version,
            t("is available.")
        )),
        State::Installing => Some(t("Updating…")),
        State::Failed(e) => Some(e),
    }
}
