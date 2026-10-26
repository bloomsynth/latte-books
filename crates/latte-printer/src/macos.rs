//! macOS: PDFKit's print operation, run as a sheet on the window.

use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2::{AnyThread as _, MainThreadMarker};
use objc2_app_kit::{NSPrintInfo, NSPrintOperation, NSView};
use objc2_foundation::{NSData, NSString};
use objc2_pdf_kit::{PDFDocument, PDFPrintScalingMode};

use crate::{Job, Outcome};

thread_local! {
    /// The operation the sheet is running, kept until the next print.
    static RUNNING: RefCell<Option<Retained<NSPrintOperation>>> = const { RefCell::new(None) };
}

/// Shows the print panel as a sheet on the window whose content view is `ns_view` and
/// prints the job's PDF from it. Returns at once: the sheet prints by itself.
pub fn print(ns_view: NonNull<c_void>, job: Job) -> Outcome {
    let Some(mtm) = MainThreadMarker::new() else {
        return Outcome::Failed("Print must start on the main thread".into());
    };
    // SAFETY: AppKit's window handle is the window's NSView, alive while it's shown.
    let view: &NSView = unsafe { ns_view.cast().as_ref() };
    let Some(window) = view.window() else {
        return Outcome::Unavailable("The view has no window".into());
    };
    let data = NSData::with_bytes(&job.pdf);
    // SAFETY: `data` is a PDF file.
    let Some(document) = (unsafe { PDFDocument::initWithData(PDFDocument::alloc(), &data) }) else {
        return Outcome::Failed("PDFKit couldn't read the PDF".into());
    };
    let info = NSPrintInfo::sharedPrintInfo();
    // SAFETY: valid print info; the page size comes from the PDF.
    let Some(operation) = (unsafe {
        document.printOperationForPrintInfo_scalingMode_autoRotate(
            Some(&info),
            PDFPrintScalingMode::PageScaleNone,
            true,
            mtm,
        )
    }) else {
        return Outcome::Failed("PDFKit couldn't print the PDF".into());
    };
    operation.setJobTitle(Some(&NSString::from_str(&job.title)));
    // SAFETY: a sheet on our window with no delegate to call back.
    unsafe {
        operation.runOperationModalForWindow_delegate_didRunSelector_contextInfo(
            &window,
            None,
            None,
            std::ptr::null_mut(),
        );
    }
    RUNNING.with(|running| *running.borrow_mut() = Some(operation));
    Outcome::Shown
}
