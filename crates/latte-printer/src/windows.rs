//! Windows: `PrintDlgW`, then each page as a picture through GDI.

use windows::Win32::Foundation::{GlobalFree, HGLOBAL, HWND};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteDC, GetDeviceCaps, HDC, LOGPIXELSX,
    LOGPIXELSY, PHYSICALOFFSETX, PHYSICALOFFSETY, SRCCOPY, StretchDIBits,
};
use windows::Win32::Storage::Xps::{AbortDoc, DOCINFOW, EndDoc, EndPage, StartDocW, StartPage};
use windows::Win32::UI::Controls::Dialogs::{
    CommDlgExtendedError, PD_NOSELECTION, PD_PAGENUMS, PD_RETURNDC, PD_USEDEVMODECOPIESANDCOLLATE,
    PRINTDLGW, PrintDlgW,
};
use windows::core::PCWSTR;

use crate::{Job, Outcome, draw_dpi, pages};

/// Shows the Print dialog over the window `owner` (an `HWND`) and prints the job's
/// pages. Blocks until the dialog closes and the pages are sent, so call it off the
/// window's thread.
pub fn print(owner: isize, job: Job) -> Outcome {
    let count = job.page_sizes.len();
    let max = u16::try_from(count).unwrap_or(u16::MAX).max(1);
    let mut dialog = PRINTDLGW {
        lStructSize: std::mem::size_of::<PRINTDLGW>() as u32,
        hwndOwner: HWND(owner as *mut _),
        Flags: PD_RETURNDC | PD_USEDEVMODECOPIESANDCOLLATE | PD_NOSELECTION,
        nFromPage: 1,
        nToPage: max,
        nMinPage: 1,
        nMaxPage: max,
        nCopies: 1,
        ..Default::default()
    };
    // SAFETY: `dialog` is a valid PRINTDLGW with its size set.
    let chosen = unsafe { PrintDlgW(&mut dialog) }.as_bool();
    free(dialog.hDevMode);
    free(dialog.hDevNames);
    if !chosen {
        // SAFETY: no arguments; reads the dialog's last error.
        let error = unsafe { CommDlgExtendedError() };
        return if error.0 == 0 {
            Outcome::Cancelled
        } else {
            Outcome::Failed(format!("Print dialog error {}", error.0))
        };
    }
    let hdc = dialog.hDC;
    let range =
        (dialog.Flags & PD_PAGENUMS == PD_PAGENUMS).then_some((dialog.nFromPage, dialog.nToPage));
    let outcome = print_pages(hdc, &job, pages(range, count));
    // SAFETY: the dialog made this DC for us.
    unsafe {
        let _ = DeleteDC(hdc);
    }
    outcome
}

fn free(memory: HGLOBAL) {
    if !memory.is_invalid() {
        // SAFETY: the dialog allocated it with GlobalAlloc and we no longer use it.
        unsafe {
            let _ = GlobalFree(Some(memory));
        }
    }
}

fn print_pages(hdc: HDC, job: &Job, range: std::ops::Range<usize>) -> Outcome {
    let title: Vec<u16> = job.title.encode_utf16().chain(Some(0)).collect();
    let info = DOCINFOW {
        cbSize: std::mem::size_of::<DOCINFOW>() as i32,
        lpszDocName: PCWSTR(title.as_ptr()),
        ..Default::default()
    };
    // SAFETY: `hdc` is the printer's DC and `info` lives for the call.
    if unsafe { StartDocW(hdc, &info) } <= 0 {
        return Outcome::Failed("The printer refused the document".into());
    }
    // SAFETY: reading the printer DC's capabilities.
    let (dpi_x, dpi_y, offset_x, offset_y) = unsafe {
        (
            GetDeviceCaps(Some(hdc), LOGPIXELSX),
            GetDeviceCaps(Some(hdc), LOGPIXELSY),
            GetDeviceCaps(Some(hdc), PHYSICALOFFSETX),
            GetDeviceCaps(Some(hdc), PHYSICALOFFSETY),
        )
    };
    let scale = draw_dpi(dpi_x) / 72.0;
    for index in range {
        let Some((width, height, bgra)) = (job.draw)(index, scale) else {
            continue;
        };
        let (page_w, page_h) = job.page_sizes[index];
        // The page at its own size, from the paper's corner: the printable area starts
        // `offset` device pixels in.
        let dest_w = (page_w * f64::from(dpi_x) / 72.0).round() as i32;
        let dest_h = (page_h * f64::from(dpi_y) / 72.0).round() as i32;
        let header = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            // Negative: the rows run top to bottom.
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let bitmap = BITMAPINFO {
            bmiHeader: header,
            ..Default::default()
        };
        // SAFETY: `bgra` holds width × height 32-bit pixels as `bitmap` describes.
        let drawn = unsafe {
            StartPage(hdc) > 0 && {
                let lines = StretchDIBits(
                    hdc,
                    -offset_x,
                    -offset_y,
                    dest_w,
                    dest_h,
                    0,
                    0,
                    width as i32,
                    height as i32,
                    Some(bgra.as_ptr().cast()),
                    &bitmap,
                    DIB_RGB_COLORS,
                    SRCCOPY,
                );
                EndPage(hdc) > 0 && lines != 0
            }
        };
        if !drawn {
            // SAFETY: the document was started on this DC.
            unsafe {
                AbortDoc(hdc);
            }
            return Outcome::Failed("The printer stopped while printing".into());
        }
    }
    // SAFETY: the document was started on this DC.
    if unsafe { EndDoc(hdc) } <= 0 {
        return Outcome::Failed("The printer didn't finish the document".into());
    }
    Outcome::Printed
}
