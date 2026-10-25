//! `src/components/Controls/AttachImage.vue` @ v0.36.0 for the screens outside forms: a
//! 135px square with the picture, or a camera; on hover, upload or clear.

use std::sync::Arc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::books::{self, gray};

/// A picture stored as `data:image/png;base64,…`.
pub fn image_from_data_url(url: &str) -> Option<Image> {
    use base64::Engine as _;
    let (mime, data) = url.strip_prefix("data:")?.split_once(";base64,")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .ok()?;
    Some(Image::from_bytes(ImageFormat::from_mime_type(mime)?, bytes))
}

/// A picture file as a data URL, as Books stores it (`mime_types`).
pub fn data_url(path: &std::path::Path) -> Result<String, String> {
    use base64::Engine as _;
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let mime = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => return Err(format!("{} isn't a picture Latte can show", path.display())),
    };
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

/// The control: `image` shown, and `on_click` for its upload or clear button (clear when
/// there is a picture).
pub fn attach_image(
    id: &'static str,
    image: Option<Arc<Image>>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let has_value = image.is_some();
    div()
        .id(id)
        .group(id)
        .relative()
        .size(px(135.))
        .flex()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .bg(books::pick(books::white(), books::raw_gray(900)))
        .border_1()
        .border_color(gray(200))
        .rounded(books::RADIUS)
        .map(|d| match image {
            Some(image) => d.child(img(image).size_full().object_fit(ObjectFit::Contain)),
            None => d.child(
                svg()
                    .path("books/camera.svg")
                    .w_6()
                    .h(px(21.))
                    .text_color(books::pick(books::raw_gray(300), books::raw_gray(600))),
            ),
        })
        .child(
            div()
                .absolute()
                .inset_0()
                .invisible()
                .group_hover(id, |d| d.visible())
                .flex()
                .items_end()
                .justify_center()
                .bg(hsla(0., 0., 0., 0.2))
                .child(
                    div()
                        .id(SharedString::from(format!("{id}-button")))
                        .mb_1()
                        .p(px(2.))
                        .rounded(books::RADIUS)
                        .bg(books::pick(books::raw_gray(100), books::raw_gray(800)))
                        .cursor_pointer()
                        .child(books::icon(
                            if has_value { "x" } else { "upload" },
                            px(16.),
                            gray(600),
                        ))
                        .on_click(on_click),
                ),
        )
}
