//! Linux: the desktop's print dialog through xdg-desktop-portal (GNOME's, KDE's…),
//! which prints the PDF itself.

use std::io::Write as _;
use std::os::fd::AsFd as _;

use ashpd::WindowIdentifier;
use ashpd::desktop::ResponseError;
use ashpd::desktop::print::{PageSetup, PreparePrintOptions, PrintOptions, PrintProxy, Settings};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use crate::{Job, Outcome};

/// Asks the portal for the print dialog over the window and prints the job's PDF with
/// what was chosen there. `Unavailable` when there's no portal to ask.
pub async fn print(
    window: Option<RawWindowHandle>,
    display: Option<RawDisplayHandle>,
    job: Job,
) -> Outcome {
    let parent = match window {
        Some(window) => WindowIdentifier::from_raw_handle(&window, display.as_ref()).await,
        None => None,
    };
    let proxy = match PrintProxy::new().await {
        Ok(proxy) => proxy,
        Err(e) => return Outcome::Unavailable(e.to_string()),
    };
    let request = proxy
        .prepare_print(
            parent.as_ref(),
            &job.title,
            Settings::default(),
            PageSetup::default(),
            PreparePrintOptions::default().set_modal(true),
        )
        .await;
    let prepared = match request.map(|r| r.response()) {
        Ok(Ok(prepared)) => prepared,
        Ok(Err(ashpd::Error::Response(ResponseError::Cancelled))) => return Outcome::Cancelled,
        Ok(Err(e)) => return Outcome::Failed(e.to_string()),
        // No print portal, or no backend for it.
        Err(e) => return Outcome::Unavailable(e.to_string()),
    };
    let file = match pdf_file(&job.pdf) {
        Ok(file) => file,
        Err(e) => return Outcome::Failed(e.to_string()),
    };
    let printed = proxy
        .print(
            parent.as_ref(),
            &job.title,
            &file.as_fd(),
            PrintOptions::default()
                .set_token(prepared.token)
                .set_modal(true),
        )
        .await
        .and_then(|r| r.response());
    match printed {
        Ok(()) => Outcome::Printed,
        Err(ashpd::Error::Response(ResponseError::Cancelled)) => Outcome::Cancelled,
        Err(e) => Outcome::Failed(e.to_string()),
    }
}

/// The PDF in a temporary file, open for reading: the portal takes a file descriptor.
fn pdf_file(pdf: &[u8]) -> std::io::Result<std::fs::File> {
    let dir = std::env::temp_dir().join("latte-books-print");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("print-{}.pdf", std::process::id()));
    std::fs::File::create(&path)?.write_all(pdf)?;
    std::fs::File::open(&path)
}
