//! Frappe Books' look: its colours (`colors.json`), type scale and radii
//! (`tailwind.config.js`), the Inter font and its icons @ v0.36.0.
//!
//! The screens use these instead of gpui-kit's defaults, so they look as Books does. The
//! rem stays at 16px, as in a browser, so GPUI's spacing (`p_4` is 1rem) is Tailwind's.

use std::borrow::Cow;

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;

/// A colour from `colors.json`, as `0xRRGGBB`.
pub fn hex(rgb: u32) -> Hsla {
    gpui_kit::rgb(rgb).into()
}

/// `colors.black`: Books' text and primary button colour.
pub fn black() -> Hsla {
    hex(0x1E293B)
}

pub fn white() -> Hsla {
    hex(0xFFFFFF)
}

pub fn gray(shade: u16) -> Hsla {
    hex(match shade {
        25 => 0xFBFBFB,
        50 => 0xF8F8F8,
        100 => 0xF3F3F3,
        200 => 0xEDEDED,
        300 => 0xE2E2E2,
        400 => 0xC7C7C7,
        500 => 0x999999,
        600 => 0x7C7C7C,
        700 => 0x525252,
        800 => 0x383838,
        850 => 0x282828,
        875 => 0x212121,
        890 => 0x1C1C1C,
        _ => 0x171717,
    })
}

/// A shade of one of Books' named colours (`getValidColor`); unknown names are gray.
pub fn color(name: &str, shade: u16) -> Hsla {
    let shades: [u32; 9] = match name {
        "red" => [
            0xFFF0F0, 0xFCD7D7, 0xF9C6C6, 0xEB9091, 0xE03636, 0xCC2929, 0xB52A2A, 0x941F1F,
            0x6B1515,
        ],
        "orange" => [
            0xFFF1E7, 0xFCE6D5, 0xF7D6BD, 0xF0B58B, 0xE86C13, 0xD45A08, 0xBD3E0C, 0x9E3513,
            0x6B2711,
        ],
        "yellow" => [
            0xFFF7D3, 0xF7E9A8, 0xF5E171, 0xF2D14B, 0xEDBA13, 0xD1930D, 0xAB6E05, 0x8C5600,
            0x733F12,
        ],
        "green" => [
            0xE4F5E9, 0xDAF0E1, 0xCAE5D4, 0xB6DEC5, 0x59BA8B, 0x30A66D, 0x278F5E, 0x16794C,
            0x173B2C,
        ],
        "teal" => [
            0xE6F7F4, 0xBAE8E1, 0x97DED4, 0x73D1C4, 0x36BAAD, 0x0B9E92, 0x0F736B, 0x115C57,
            0x114541,
        ],
        "blue" => [
            0xEDF6FD, 0xE3F1FD, 0xC9E7FC, 0x70B6F0, 0x33A1FF, 0x007BE0, 0x0070CC, 0x005CA3,
            0x004880,
        ],
        "indigo" => [
            0xEBF4FF, 0xC3DAFE, 0xA3BFFA, 0x7F9CF5, 0x667EEA, 0x5A67D8, 0x4C51BF, 0x434190,
            0x3C366B,
        ],
        "purple" => [
            0xF9F0FF, 0xF1E5FA, 0xE9D6F5, 0xD6C1E6, 0x9C45E3, 0x8642C2, 0x6E399D, 0x5C2F83,
            0x401863,
        ],
        "pink" => [
            0xFEEEF8, 0xF8E2F0, 0xF2D4E6, 0xE9C4DA, 0xDF9EB8, 0xCF3A96, 0x9C2671, 0x801458,
            0x570F3E,
        ],
        _ => return gray(shade),
    };
    let i = (shade / 100).clamp(1, 9) as usize - 1;
    hex(shades[i])
}

/// `getBgTextColorClass`: a badge or pill's background and text, by colour name.
pub fn bg_text(name: &str) -> (Hsla, Hsla) {
    let name = match name {
        "orange" | "green" | "red" | "yellow" | "blue" | "indigo" | "pink" | "purple" | "teal" => {
            name
        }
        _ => "gray",
    };
    (color(name, 200), color(name, 700))
}

/// Books' type scale (`tailwind.config.js` `fontSize`), in place of GPUI's rem sizes.
pub trait Text: Styled + Sized {
    /// `text-xs`: 11px.
    fn text_11(self) -> Self {
        self.text_size(px(11.))
    }
    /// `text-sm`: 12px.
    fn text_12(self) -> Self {
        self.text_size(px(12.))
    }
    /// `text-base`: 13px.
    fn text_13(self) -> Self {
        self.text_size(px(13.))
    }
    /// `text-lg`: 14px.
    fn text_14(self) -> Self {
        self.text_size(px(14.))
    }
    /// `text-xl`: 18px.
    fn text_18(self) -> Self {
        self.text_size(px(18.))
    }
    /// `text-2xl`: 20px.
    fn text_20(self) -> Self {
        self.text_size(px(20.))
    }
}

impl<T: Styled> Text for T {}

/// `rounded`: 5px.
pub const RADIUS: Pixels = px(5.);
/// `rounded-md`: 6px.
pub const RADIUS_MD: Pixels = px(6.);
/// `rounded-lg`: 8px.
pub const RADIUS_LG: Pixels = px(8.);

/// `--h-row-largest`, a page header's height.
pub const H_PAGE_HEADER: Pixels = px(64.);
/// `--w-sidebar`.
pub const W_SIDEBAR: Pixels = px(224.);
/// `--w-form`.
pub const W_FORM: Pixels = px(600.);

/// `shadow-lg`.
pub fn shadow_lg() -> Vec<BoxShadow> {
    vec![
        BoxShadow {
            color: hsla(0., 0., 0., 0.1),
            offset: point(px(0.), px(10.)),
            blur_radius: px(15.),
            spread_radius: px(-3.),
            inset: false,
        },
        BoxShadow {
            color: hsla(0., 0., 0., 0.05),
            offset: point(px(0.), px(4.)),
            blur_radius: px(6.),
            spread_radius: px(-2.),
            inset: false,
        },
    ]
}

/// Inter, which Books bundles; Latte bundles the same font.
const FONTS: [&[u8]; 4] = [
    include_bytes!("../fonts/Inter-Regular.otf"),
    include_bytes!("../fonts/Inter-Medium.otf"),
    include_bytes!("../fonts/Inter-SemiBold.otf"),
    include_bytes!("../fonts/Inter-Bold.otf"),
];

/// Loads Inter and sets gpui-kit's theme to Books' colours, so its inputs, popovers and
/// scrollbars match the screens drawn here.
pub fn init(cx: &mut App) {
    cx.text_system()
        .add_fonts(FONTS.iter().map(|f| Cow::Borrowed(*f)).collect())
        .ok();
    Theme::change(ThemeMode::Light, None, cx);
    let theme = Theme::global_mut(cx);
    theme.font_family = "Inter".into();
    theme.font_size = px(16.);
    theme.radius = RADIUS;
    theme.radius_lg = RADIUS_LG;
    theme.shadow = false;
    theme.focus_ring = false;
    let c = &mut theme.colors;
    c.background = white();
    c.foreground = gray(900);
    c.border = gray(200);
    c.input = gray(200);
    c.ring = gray(300);
    c.caret = gray(900);
    c.selection = color("blue", 300).opacity(0.6);
    c.muted = gray(50);
    c.muted_foreground = gray(600);
    c.accent = gray(100);
    c.accent_foreground = gray(900);
    c.primary = black();
    c.primary_hover = black();
    c.primary_active = black();
    c.primary_foreground = white();
    c.secondary = gray(200);
    c.secondary_hover = gray(200);
    c.secondary_active = gray(300);
    c.secondary_foreground = gray(700);
    c.popover = white();
    c.popover_foreground = gray(900);
    c.list = white();
    c.list_hover = gray(100);
    c.list_active = gray(100);
    c.list_active_border = gray(100);
    c.list_even = white();
    c.list_head = white();
    c.table = white();
    c.table_head = white();
    c.table_head_foreground = gray(600);
    c.table_hover = gray(50);
    c.table_active = gray(100);
    c.table_active_border = gray(100);
    c.table_even = white();
    c.table_row_border = gray(200);
    c.tab_bar = white();
    c.tab_active = white();
    c.tab_active_foreground = gray(900);
    c.tab_foreground = gray(700);
    c.scrollbar = white().opacity(0.);
    c.scrollbar_thumb = gray(100);
    c.scrollbar_thumb_hover = gray(200);
    c.sidebar = gray(25);
    c.sidebar_border = gray(200);
    c.sidebar_foreground = gray(700);
    c.danger = color("red", 500);
    c.danger_foreground = white();
    c.success = color("green", 600);
    c.success_foreground = white();
    c.warning = color("orange", 500);
    c.info = color("blue", 500);
    c.red = color("red", 500);
    c.green = color("green", 600);
    c.yellow = color("yellow", 500);
    c.blue = color("blue", 500);
    c.cyan = color("teal", 500);
    c.switch = gray(300);
    c.progress_bar = black();
    c.skeleton = gray(100);
}

/// The app's assets: Books' icons, then gpui-kit's.
pub struct Assets;

macro_rules! sidebar {
    ($name:literal, $state:literal) => {
        (
            concat!("books/sidebar/", $name, "-", $state, ".svg"),
            include_bytes!(concat!("../icons/sidebar/", $name, "-", $state, ".svg")),
        )
    };
}

macro_rules! feather {
    ($name:literal) => {
        (
            concat!("feather/", $name, ".svg"),
            include_bytes!(concat!("../icons/feather/", $name, ".svg")),
        )
    };
}

/// Books' two-tone sidebar icons (`src/components/Icons/18`, `24/general.vue`), drawn in
/// the colours `Icons/base.vue` gives them, Get Started's cards', the chart of accounts'
/// (`getIconComponent`),
/// and the controls' own icons.
static OWN: &[(&str, &[u8])] = &[
    (
        "books/check-on.svg",
        include_bytes!("../icons/check-on.svg"),
    ),
    (
        "books/check-off.svg",
        include_bytes!("../icons/check-off.svg"),
    ),
    (
        "books/selector.svg",
        include_bytes!("../icons/selector.svg"),
    ),
    (
        "books/coa/assets.svg",
        include_bytes!("../icons/coa/assets.svg"),
    ),
    (
        "books/coa/expenses.svg",
        include_bytes!("../icons/coa/expenses.svg"),
    ),
    (
        "books/coa/income.svg",
        include_bytes!("../icons/coa/income.svg"),
    ),
    (
        "books/coa/liabilities.svg",
        include_bytes!("../icons/coa/liabilities.svg"),
    ),
    (
        "books/coa/leaf.svg",
        include_bytes!("../icons/coa/leaf.svg"),
    ),
    (
        "books/coa/folder.svg",
        include_bytes!("../icons/coa/folder.svg"),
    ),
    (
        "books/get-started/customer.svg",
        include_bytes!("../icons/get-started/customer.svg"),
    ),
    (
        "books/get-started/general.svg",
        include_bytes!("../icons/get-started/general.svg"),
    ),
    (
        "books/get-started/green-check.svg",
        include_bytes!("../icons/get-started/green-check.svg"),
    ),
    (
        "books/get-started/invoice.svg",
        include_bytes!("../icons/get-started/invoice.svg"),
    ),
    (
        "books/get-started/item.svg",
        include_bytes!("../icons/get-started/item.svg"),
    ),
    (
        "books/get-started/opening-ac.svg",
        include_bytes!("../icons/get-started/opening-ac.svg"),
    ),
    (
        "books/get-started/percentage.svg",
        include_bytes!("../icons/get-started/percentage.svg"),
    ),
    (
        "books/get-started/purchase-invoice.svg",
        include_bytes!("../icons/get-started/purchase-invoice.svg"),
    ),
    (
        "books/get-started/review-ac.svg",
        include_bytes!("../icons/get-started/review-ac.svg"),
    ),
    (
        "books/get-started/sales-invoice.svg",
        include_bytes!("../icons/get-started/sales-invoice.svg"),
    ),
    (
        "books/get-started/supplier.svg",
        include_bytes!("../icons/get-started/supplier.svg"),
    ),
    (
        "books/get-started/system.svg",
        include_bytes!("../icons/get-started/system.svg"),
    ),
    sidebar!("common-entries", "active"),
    sidebar!("common-entries", "inactive"),
    sidebar!("dashboard", "active"),
    sidebar!("dashboard", "inactive"),
    sidebar!("general", "active"),
    sidebar!("general", "inactive"),
    sidebar!("gst", "active"),
    sidebar!("gst", "inactive"),
    sidebar!("inventory", "active"),
    sidebar!("inventory", "inactive"),
    sidebar!("pos", "active"),
    sidebar!("pos", "inactive"),
    sidebar!("purchase", "active"),
    sidebar!("purchase", "inactive"),
    sidebar!("reports", "active"),
    sidebar!("reports", "inactive"),
    sidebar!("sales", "active"),
    sidebar!("sales", "inactive"),
    sidebar!("settings", "active"),
    sidebar!("settings", "inactive"),
    feather!("alert-circle"),
    feather!("alert-triangle"),
    feather!("arrow-down"),
    feather!("arrow-left"),
    feather!("arrow-up"),
    feather!("calendar"),
    feather!("check"),
    feather!("check-circle"),
    feather!("chevron-down"),
    feather!("chevron-left"),
    feather!("chevron-right"),
    feather!("chevron-up"),
    feather!("chevrons-left"),
    feather!("chevrons-right"),
    feather!("command"),
    feather!("database"),
    feather!("download"),
    feather!("edit"),
    feather!("external-link"),
    feather!("eye"),
    feather!("file"),
    feather!("filter"),
    feather!("flag"),
    feather!("grid"),
    feather!("help-circle"),
    feather!("info"),
    feather!("link"),
    feather!("list"),
    feather!("maximize"),
    feather!("minimize"),
    feather!("minus"),
    feather!("monitor"),
    feather!("more-horizontal"),
    feather!("paperclip"),
    feather!("plus"),
    feather!("printer"),
    feather!("refresh-cw"),
    feather!("search"),
    feather!("settings"),
    feather!("square"),
    feather!("trash-2"),
    feather!("upload"),
    feather!("x"),
    feather!("x-circle"),
];

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = OWN.iter().find(|(p, _)| *p == path) {
            return Ok(Some(Cow::Borrowed(*bytes)));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut list = gpui_kit::assets::Assets.list(path)?;
        list.extend(
            OWN.iter()
                .filter(|(p, _)| p.starts_with(path))
                .map(|(p, _)| SharedString::from(*p)),
        );
        Ok(list)
    }
}

/// A feather icon (`feather-icon`). GPUI draws an SVG in its own colour, not its
/// parent's text colour.
pub fn icon(name: &str, size: Pixels, color: Hsla) -> Svg {
    svg()
        .path(SharedString::from(format!("feather/{name}.svg")))
        .size(size)
        .flex_none()
        .text_color(color)
}
