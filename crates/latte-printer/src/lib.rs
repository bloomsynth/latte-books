//! Print: the computer's own print dialog, as Books' `webContents.print` opens it.
//!
//! - Windows: the Print dialog (`PrintDlgW`), then each page drawn to the printer as a
//!   picture at the printer's resolution (up to 600 dpi).
//! - macOS: PDFKit prints the PDF, with the print panel as a sheet on the window.
//! - Linux: the desktop's print dialog through xdg-desktop-portal, which prints the PDF.
//!
//! When there is no dialog to show (no portal on Linux, say), the caller opens the PDF in
//! the computer's PDF viewer instead.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::print;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::print;

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod portal;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub use portal::print;

/// How a print went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Sent to the printer.
    Printed,
    /// The dialog was closed without printing.
    Cancelled,
    /// The dialog is open on the window and prints by itself (macOS's sheet).
    Shown,
    /// There is no print dialog here; the PDF should be opened to print from there.
    Unavailable(String),
    /// The printer or the dialog failed.
    Failed(String),
}

/// A page drawn for the printer: its width and height in pixels and its pixels as
/// BGRA, top row first.
pub type Bitmap = (u32, u32, Vec<u8>);

/// A document to print: its title, its PDF and how to draw its pages.
pub struct Job {
    pub title: String,
    pub pdf: Vec<u8>,
    /// Each page's width and height, in points.
    pub page_sizes: Vec<(f64, f64)>,
    /// Draws page `index` at `pixel_per_pt` pixels to a point.
    pub draw: Box<dyn Fn(usize, f64) -> Option<Bitmap> + Send>,
}

/// The pages a printer gets for a range from the dialog, 1-based and inclusive; all of
/// them when there's none.
pub fn pages(range: Option<(u16, u16)>, count: usize) -> std::ops::Range<usize> {
    match range {
        Some((from, to)) => {
            let from = usize::from(from.max(1)) - 1;
            let to = usize::from(to).min(count);
            from.min(to)..to
        }
        None => 0..count,
    }
}

/// The resolution pages are drawn at for a printer of `dpi`: its own, at most 600.
pub fn draw_dpi(dpi: i32) -> f64 {
    f64::from(dpi.clamp(72, 600))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pages_of_a_range() {
        assert_eq!(pages(None, 3), 0..3);
        assert_eq!(pages(Some((1, 3)), 3), 0..3);
        assert_eq!(pages(Some((2, 2)), 3), 1..2);
        // A range past the end prints what there is.
        assert_eq!(pages(Some((2, 9)), 3), 1..3);
        assert_eq!(pages(Some((5, 9)), 3), 3..3);
        assert_eq!(pages(Some((0, 1)), 3), 0..1);
    }

    #[test]
    fn pages_are_drawn_at_the_printers_resolution() {
        assert_eq!(draw_dpi(300), 300.0);
        assert_eq!(draw_dpi(1200), 600.0);
        assert_eq!(draw_dpi(0), 72.0);
    }
}
