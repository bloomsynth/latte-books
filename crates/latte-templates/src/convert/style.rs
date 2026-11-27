//! Tailwind classes and inline styles as Typst layout: Books' Tailwind config
//! (tailwind.config.js @ v0.36.0: its text sizes, spacing and colours.json) and the CSS
//! properties templates set in `style` and `:style`.
//!
//! Each value is a Typst expression. A class given under a condition (`:class="{ … }"`)
//! makes its value an `if`, so it can change as the document does.

use super::expr::{self, Scope};

/// Books' colours: the palettes and their shades (colors.json).
const PALETTES: [(&str, &[u16]); 14] = [
    (
        "gray",
        &[
            25, 50, 100, 200, 300, 400, 500, 600, 700, 800, 850, 875, 890, 900,
        ],
    ),
    ("red", &[50, 100, 200, 300, 400, 500, 600, 700, 800, 900]),
    ("orange", &[50, 100, 200, 300, 400, 500, 600, 700, 800, 900]),
    ("yellow", &[50, 100, 200, 300, 400, 500, 600, 700, 800, 900]),
    ("green", &[50, 100, 200, 300, 400, 500, 600, 700, 800, 900]),
    ("teal", &[50, 100, 200, 300, 400, 500, 600, 700, 800, 900]),
    ("blue", &[50, 100, 200, 300, 400, 500, 600, 700, 800, 900]),
    ("indigo", &[100, 200, 300, 400, 500, 600, 700, 800, 900]),
    ("purple", &[50, 100, 200, 300, 400, 500, 600, 700, 800, 900]),
    ("pink", &[50, 100, 200, 300, 400, 500, 600, 700, 800, 900]),
    ("violet", &[50, 100, 200, 300, 400, 500, 600, 700, 800, 900]),
    ("cyan", &[50, 100, 200, 300, 400, 500, 600, 700, 800, 900]),
    ("amber", &[50, 100, 200, 300, 400, 500, 600, 700, 800, 900]),
    ("white", &[]),
];

/// Books' text sizes, in px.
pub const TEXT_SIZES: [(&str, f64); 8] = [
    ("xs", 11.0),
    ("sm", 12.0),
    ("base", 13.0),
    ("lg", 14.0),
    ("xl", 18.0),
    ("2xl", 20.0),
    ("3xl", 24.0),
    ("4xl", 28.0),
];

/// How an element is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    Block,
    Inline,
    InlineBlock,
    Flex,
    Grid,
    Hidden,
}

/// A line height: a factor of the text size, or a length in px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineHeight {
    Factor(f64),
    Px(f64),
}

/// A value for each side: top, right, bottom, left.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sides {
    pub top: Option<String>,
    pub right: Option<String>,
    pub bottom: Option<String>,
    pub left: Option<String>,
}

impl Sides {
    fn set(&mut self, sides: &str, value: &str, cond: Option<&str>, default: &str) {
        let put = |slot: &mut Option<String>| {
            *slot = Some(conditional(slot.take(), value, cond, default));
        };
        for side in sides.chars() {
            match side {
                't' => put(&mut self.top),
                'r' => put(&mut self.right),
                'b' => put(&mut self.bottom),
                'l' => put(&mut self.left),
                _ => {}
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.top.is_none() && self.right.is_none() && self.bottom.is_none() && self.left.is_none()
    }

    /// As a Typst dictionary of the sides set: `(top: …, left: …)`.
    pub fn dict(&self) -> Option<String> {
        let parts: Vec<String> = [
            ("top", &self.top),
            ("right", &self.right),
            ("bottom", &self.bottom),
            ("left", &self.left),
        ]
        .iter()
        .filter_map(|(k, v)| v.as_ref().map(|v| format!("{k}: {v}")))
        .collect();
        (!parts.is_empty()).then(|| format!("({})", parts.join(", ")))
    }
}

/// A value that is `value` when `cond` holds, else what it was (or `default`).
fn conditional(old: Option<String>, value: &str, cond: Option<&str>, default: &str) -> String {
    match cond {
        None => value.to_owned(),
        Some(cond) => format!(
            "(if has({cond}) {{ {value} }} else {{ {} }})",
            old.as_deref().unwrap_or(default)
        ),
    }
}

/// An element's style.
#[derive(Debug, Clone, Default)]
pub struct Style {
    pub display: Option<Display>,
    /// Conditions under which the element is hidden.
    pub hidden_if: Vec<String>,
    pub padding: Sides,
    pub margin: Sides,
    pub margin_left_auto: bool,
    pub margin_right_auto: bool,
    pub width: Option<String>,
    pub height: Option<String>,
    pub max_width: Option<String>,
    pub min_height: Option<String>,
    pub fill: Option<String>,
    pub border: Sides,
    pub border_color: Option<String>,
    pub border_dashed: bool,
    pub radius: Option<String>,
    /// `row` or `col`.
    pub direction: Option<&'static str>,
    pub justify: Option<&'static str>,
    pub align_items: Option<&'static str>,
    pub align_self: Option<&'static str>,
    pub gap_x: Option<String>,
    pub gap_y: Option<String>,
    pub wrap: bool,
    pub grow: Option<String>,
    pub shrink: Option<String>,
    pub basis: Option<String>,
    pub grid_cols: Option<String>,
    pub grid_rows: Option<String>,
    pub col_span: Option<String>,
    pub col_start: Option<String>,
    pub row_start: Option<String>,
    pub row_span: Option<String>,
    pub space_x: Option<String>,
    pub space_y: Option<String>,
    /// `set text` arguments, each perhaps under a condition.
    pub text: Vec<(Option<String>, &'static str, String)>,
    /// Text alignment, perhaps under a condition.
    pub align: Vec<(Option<String>, &'static str)>,
    pub transform: Option<&'static str>,
    pub whitespace: Option<&'static str>,
    pub line_height: Option<LineHeight>,
    /// The font, as a Typst expression for a family name.
    pub font_family: Option<String>,
    /// The text size in px, when it is known.
    pub font_size_px: Option<f64>,
    pub decoration: Vec<&'static str>,
    pub object_fit: Option<&'static str>,
    /// What wasn't understood, for the notes.
    pub unknown: Vec<String>,
}

/// A length in Tailwind's spacing scale: `4` is 16px.
fn spacing(v: &str) -> Option<String> {
    match v {
        "0" => Some("0pt".to_owned()),
        "px" => Some("px(1)".to_owned()),
        _ => {
            if let Some(arbitrary) = v.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
                return css_length(arbitrary);
            }
            let n: f64 = v.parse().ok()?;
            Some(format!("sp({})", trim_number(n)))
        }
    }
}

fn trim_number(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        let s = format!("{n:.4}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

/// A width or height: `1/3`, `full`, `auto`, a spacing value.
fn size(v: &str) -> Option<String> {
    if let Some((a, b)) = v.split_once('/') {
        let (a, b): (f64, f64) = (a.parse().ok()?, b.parse().ok()?);
        return Some(format!("{}%", trim_number(a / b * 100.0)));
    }
    match v {
        "full" | "screen" => Some("100%".to_owned()),
        "auto" | "min" | "max" | "fit" => Some("auto".to_owned()),
        _ => spacing(v),
    }
}

/// `max-w-*`: Tailwind's sizes and Books' own (`max-w-32` is 8rem).
fn max_width(v: &str) -> Option<String> {
    let rem = match v {
        "xs" => 20.0,
        "sm" => 24.0,
        "md" => 28.0,
        "lg" => 32.0,
        "xl" => 36.0,
        "2xl" => 42.0,
        "3xl" => 48.0,
        "4xl" => 56.0,
        "5xl" => 64.0,
        "6xl" => 72.0,
        "7xl" => 80.0,
        "full" | "none" => return Some("100%".to_owned()),
        _ => return size(v),
    };
    Some(format!("px({})", trim_number(rem * 16.0)))
}

/// A CSS length: `12px`, `1.5rem`, `2cm`, `50%`, `0`.
pub fn css_length(v: &str) -> Option<String> {
    let v = v.trim();
    if v == "0" {
        return Some("0pt".to_owned());
    }
    let split = v
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .unwrap_or(v.len());
    let (number, unit) = v.split_at(split);
    let n: f64 = number.parse().ok()?;
    let n = trim_number(n);
    Some(match unit.trim() {
        "px" | "" => format!("px({n})"),
        "rem" => format!("px({n} * 16)"),
        "em" => format!("{n}em"),
        "pt" => format!("{n}pt"),
        "cm" => format!("{n}cm"),
        "mm" => format!("{n}mm"),
        "in" => format!("{n}in"),
        "%" => format!("{n}%"),
        _ => return None,
    })
}

/// A colour class's name, such as `gray-600`, as Typst.
fn color(v: &str) -> Option<String> {
    if let Some(arbitrary) = v.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
        return css_color(arbitrary);
    }
    match v {
        "white" => return Some("rgb(\"#FFFFFF\")".to_owned()),
        "black" => return Some("tw(\"black\")".to_owned()),
        "transparent" => return Some("none".to_owned()),
        _ => {}
    }
    let (name, shade) = v.rsplit_once('-')?;
    let shade: u16 = shade.parse().ok()?;
    PALETTES
        .iter()
        .find(|(p, shades)| *p == name && shades.contains(&shade))
        .map(|_| format!("tw(\"{name}\", {shade})"))
}

/// A CSS colour value: `#fff`, `rgb(…)`, a few names.
pub fn css_color(v: &str) -> Option<String> {
    let v = v.trim();
    if v.starts_with('#') {
        return Some(format!("rgb({})", expr::string(v)));
    }
    if let Some(args) = v
        .strip_prefix("rgba(")
        .or_else(|| v.strip_prefix("rgb("))
        .and_then(|a| a.strip_suffix(')'))
    {
        let parts: Vec<&str> = args
            .split([',', ' ', '/'])
            .filter(|p| !p.is_empty())
            .collect();
        if parts.len() >= 3 {
            return Some(format!("rgb({}, {}, {})", parts[0], parts[1], parts[2]));
        }
    }
    let named = match v.to_lowercase().as_str() {
        "black" => "#000000",
        "white" => "#FFFFFF",
        "red" => "#FF0000",
        "green" => "#008000",
        "blue" => "#0000FF",
        "gray" | "grey" => "#808080",
        "transparent" => return Some("none".to_owned()),
        _ => return None,
    };
    Some(format!("rgb(\"{named}\")"))
}

/// Classes with no effect on a printout, which are left out without a note.
fn no_op(class: &str) -> bool {
    const PREFIXES: [&str; 22] = [
        "cursor-",
        "select-",
        "transition",
        "duration-",
        "ease-",
        "shadow",
        "overflow-",
        "z-",
        "outline",
        "ring",
        "pointer-events",
        "resize",
        "appearance",
        "box-",
        "scroll",
        "no-scrollbar",
        "custom-scroll",
        "placeholder-",
        "opacity-",
        "bg-opacity-",
        "text-opacity-",
        "border-opacity-",
    ];
    const EXACT: [&str; 13] = [
        "relative",
        "static",
        "antialiased",
        "border-solid",
        "object-center",
        "isolate",
        "visible",
        "invisible",
        "sr-only",
        "truncate",
        "break-words",
        "break-all",
        "container",
    ];
    EXACT.contains(&class) || PREFIXES.iter().any(|p| class.starts_with(p))
}

impl Style {
    /// Adds a class, perhaps under a condition.
    pub fn class(&mut self, class: &str, cond: Option<&str>) {
        let class = class.trim().trim_start_matches('!');
        if class.is_empty() {
            return;
        }
        // Variants: what is shown on a printed page.
        let class = match class.rsplit_once(':') {
            Some((variants, base)) => {
                let shown = variants
                    .split(':')
                    .all(|v| matches!(v, "print" | "sm" | "md" | "ltr"));
                if !shown {
                    return;
                }
                base
            }
            None => class,
        };
        if !self.apply(class, cond) && !no_op(class) {
            self.unknown.push(format!("class `{class}`"));
        }
    }

    fn text_arg(&mut self, cond: Option<&str>, name: &'static str, value: String) {
        self.text.push((cond.map(str::to_owned), name, value));
    }

    /// Applies a class; false for one that isn't known.
    fn apply(&mut self, class: &str, cond: Option<&str>) -> bool {
        let (negative, class) = match class.strip_prefix('-') {
            Some(c) => (true, c),
            None => (false, class),
        };
        let neg = |v: String| if negative { format!("-{v}") } else { v };

        // Display.
        let display = match class {
            "block" => Some(Display::Block),
            "inline" => Some(Display::Inline),
            "inline-block" => Some(Display::InlineBlock),
            "flex" | "inline-flex" => Some(Display::Flex),
            "grid" | "inline-grid" => Some(Display::Grid),
            "hidden" => Some(Display::Hidden),
            "contents" | "table" | "table-row" | "table-cell" | "flow-root" => return true,
            _ => None,
        };
        if let Some(display) = display {
            match cond {
                None => self.display = Some(display),
                Some(c) if display == Display::Hidden => self.hidden_if.push(c.to_owned()),
                Some(_) => self
                    .unknown
                    .push(format!("class `{class}` under a condition")),
            }
            return true;
        }

        // Flex and grid.
        match class {
            "flex-row" => self.direction = Some("row"),
            "flex-col" => self.direction = Some("col"),
            "flex-row-reverse" | "flex-col-reverse" => return false,
            "flex-wrap" => self.wrap = true,
            "flex-nowrap" => self.wrap = false,
            "flex-1" => {
                self.grow = Some("1".to_owned());
                self.basis = Some("0pt".to_owned());
            }
            "flex-auto" => self.grow = Some("1".to_owned()),
            "flex-initial" => self.grow = Some("0".to_owned()),
            "flex-none" => {
                self.grow = Some("0".to_owned());
                self.shrink = Some("0".to_owned());
            }
            "flex-grow" | "grow" => self.grow = Some("1".to_owned()),
            "flex-grow-0" | "grow-0" => self.grow = Some("0".to_owned()),
            "flex-shrink" | "shrink" => self.shrink = Some("1".to_owned()),
            "flex-shrink-0" | "shrink-0" => self.shrink = Some("0".to_owned()),
            "justify-start" | "justify-normal" => self.justify = Some("start"),
            "justify-end" => self.justify = Some("end"),
            "justify-center" => self.justify = Some("center"),
            "justify-between" => self.justify = Some("between"),
            "justify-around" => self.justify = Some("around"),
            "justify-evenly" => self.justify = Some("evenly"),
            "items-start" => self.align_items = Some("start"),
            "items-end" => self.align_items = Some("end"),
            "items-center" => self.align_items = Some("center"),
            "items-baseline" => self.align_items = Some("start"),
            "items-stretch" => self.align_items = Some("stretch"),
            "self-start" => self.align_self = Some("start"),
            "self-end" => self.align_self = Some("end"),
            "self-center" => self.align_self = Some("center"),
            "self-stretch" | "self-auto" => self.align_self = Some("stretch"),
            "col-span-full" => self.col_span = Some("-1".to_owned()),
            "uppercase" => self.transform = Some("upper"),
            "lowercase" => self.transform = Some("lower"),
            "capitalize" => self.transform = Some("capitalize"),
            "normal-case" => self.transform = None,
            "italic" => self.text_arg(cond, "style", "\"italic\"".to_owned()),
            "not-italic" => self.text_arg(cond, "style", "\"normal\"".to_owned()),
            "underline" => self.decoration.push("underline"),
            "line-through" => self.decoration.push("strike"),
            "no-underline" => self.decoration.clear(),
            "whitespace-pre-line" => self.whitespace = Some("pre-line"),
            "whitespace-pre" | "whitespace-pre-wrap" => self.whitespace = Some("pre"),
            "whitespace-nowrap" => self.whitespace = Some("nowrap"),
            "whitespace-normal" => self.whitespace = None,
            "text-left" | "text-start" => self.align.push((cond.map(str::to_owned), "left")),
            "text-right" | "text-end" => self.align.push((cond.map(str::to_owned), "right")),
            "text-center" => self.align.push((cond.map(str::to_owned), "center")),
            "text-justify" => self.text_arg(cond, "justify", "true".to_owned()),
            "object-contain" => self.object_fit = Some("contain"),
            "object-cover" => self.object_fit = Some("cover"),
            "object-fill" => self.object_fit = Some("stretch"),
            "border-dashed" | "border-dotted" => self.border_dashed = true,
            "mx-auto" => {
                self.margin_left_auto = true;
                self.margin_right_auto = true;
            }
            "ml-auto" | "ms-auto" => self.margin_left_auto = true,
            "mr-auto" | "me-auto" => self.margin_right_auto = true,
            _ => return self.apply_valued(class, cond, &neg),
        }
        true
    }

    /// The classes with a value: `p-4`, `text-gray-600`, `w-1/3`, ….
    fn apply_valued(
        &mut self,
        class: &str,
        cond: Option<&str>,
        neg: &dyn Fn(String) -> String,
    ) -> bool {
        // Spacing: padding and margin.
        let spacing_classes: [(&str, &str, bool); 18] = [
            ("p-", "trbl", false),
            ("px-", "rl", false),
            ("py-", "tb", false),
            ("pt-", "t", false),
            ("pr-", "r", false),
            ("pb-", "b", false),
            ("pl-", "l", false),
            ("ps-", "l", false),
            ("pe-", "r", false),
            ("m-", "trbl", true),
            ("mx-", "rl", true),
            ("my-", "tb", true),
            ("mt-", "t", true),
            ("mr-", "r", true),
            ("mb-", "b", true),
            ("ml-", "l", true),
            ("ms-", "l", true),
            ("me-", "r", true),
        ];
        for (prefix, sides, margin) in spacing_classes {
            if let Some(v) = class.strip_prefix(prefix) {
                let Some(value) = spacing(v) else {
                    return false;
                };
                let value = neg(value);
                if margin {
                    self.margin.set(sides, &value, cond, "0pt");
                } else {
                    self.padding.set(sides, &value, cond, "0pt");
                }
                return true;
            }
        }
        for (prefix, x) in [("space-x-", true), ("space-y-", false)] {
            if let Some(v) = class.strip_prefix(prefix) {
                let Some(value) = spacing(v) else {
                    return false;
                };
                if x {
                    self.space_x = Some(value);
                } else {
                    self.space_y = Some(value);
                }
                return true;
            }
        }
        for (prefix, x, y) in [
            ("gap-x-", true, false),
            ("gap-y-", false, true),
            ("gap-", true, true),
        ] {
            if let Some(v) = class.strip_prefix(prefix) {
                let Some(value) = spacing(v) else {
                    return false;
                };
                if x {
                    self.gap_x = Some(value.clone());
                }
                if y {
                    self.gap_y = Some(value);
                }
                return true;
            }
        }

        // Sizing.
        if let Some(v) = class.strip_prefix("w-") {
            return self.set_size(v, cond, true);
        }
        if let Some(v) = class.strip_prefix("h-") {
            return self.set_size(v, cond, false);
        }
        if let Some(v) = class.strip_prefix("max-w-") {
            self.max_width = max_width(v);
            return self.max_width.is_some();
        }
        if let Some(v) = class.strip_prefix("min-h-") {
            self.min_height = size(v);
            return self.min_height.is_some();
        }
        if class.starts_with("min-w-") || class.starts_with("max-h-") {
            return true;
        }

        // Grid.
        for (prefix, slot) in [
            ("grid-cols-", 0),
            ("grid-rows-", 1),
            ("col-span-", 2),
            ("col-start-", 3),
            ("row-start-", 4),
            ("row-span-", 5),
        ] {
            if let Some(v) = class.strip_prefix(prefix) {
                let Ok(n) = v.parse::<u32>() else {
                    return false;
                };
                let n = Some(n.to_string());
                match slot {
                    0 => self.grid_cols = n,
                    1 => self.grid_rows = n,
                    2 => self.col_span = n,
                    3 => self.col_start = n,
                    4 => self.row_start = n,
                    _ => self.row_span = n,
                }
                return true;
            }
        }
        if class.starts_with("col-end-")
            || class.starts_with("row-end-")
            || class.starts_with("grid-flow-")
        {
            return true;
        }

        // Text.
        if let Some(v) = class.strip_prefix("text-") {
            if let Some((_, px)) = TEXT_SIZES.iter().find(|(name, _)| *name == v) {
                let value = if v.starts_with(|c: char| c.is_ascii_digit()) {
                    format!("text-size.at(\"{v}\")")
                } else {
                    format!("text-size.{v}")
                };
                self.text_arg(cond, "size", value);
                if cond.is_none() {
                    self.font_size_px = Some(*px);
                }
                return true;
            }
            if let Some(c) = color(v) {
                self.text_arg(cond, "fill", c);
                return true;
            }
            return false;
        }
        if let Some(v) = class.strip_prefix("font-") {
            let weight = match v {
                "thin" => 100,
                "extralight" => 200,
                "light" => 300,
                "normal" => 400,
                "medium" => 500,
                "semibold" => 600,
                "bold" => 700,
                "extrabold" => 800,
                "black" => 900,
                "sans" => {
                    self.font_family = Some("\"Inter\"".to_owned());
                    return true;
                }
                "serif" => {
                    self.font_family = Some("\"Liberation Serif\"".to_owned());
                    return true;
                }
                "mono" => {
                    self.font_family = Some("\"Liberation Mono\"".to_owned());
                    return true;
                }
                _ => return false,
            };
            self.text_arg(cond, "weight", weight.to_string());
            return true;
        }
        if let Some(v) = class.strip_prefix("tracking-") {
            let em = match v {
                "tighter" => "-0.05em",
                "tight" => "-0.025em",
                "normal" => "0em",
                "wide" => "0.025em",
                "wider" => "0.05em",
                "widest" => "0.1em",
                _ => return false,
            };
            self.text_arg(cond, "tracking", em.to_owned());
            return true;
        }
        if let Some(v) = class.strip_prefix("leading-") {
            self.line_height = Some(match v {
                "none" => LineHeight::Factor(1.0),
                "tight" => LineHeight::Factor(1.25),
                "snug" => LineHeight::Factor(1.375),
                "normal" => LineHeight::Factor(1.5),
                "relaxed" => LineHeight::Factor(1.625),
                "loose" => LineHeight::Factor(2.0),
                _ => match v.parse::<f64>() {
                    Ok(n) => LineHeight::Px(n * 4.0),
                    Err(_) => return false,
                },
            });
            return true;
        }

        // Backgrounds and borders.
        if let Some(v) = class.strip_prefix("bg-") {
            let Some(c) = color(v) else { return false };
            self.fill = Some(conditional(self.fill.take(), &c, cond, "none"));
            return true;
        }
        if class == "rounded" || class.starts_with("rounded-") {
            // `rounded-t-lg` rounds the top corners; all of them are rounded here.
            let v = class.strip_prefix("rounded").unwrap_or("");
            let v = v.trim_start_matches('-').rsplit('-').next().unwrap_or("");
            let px = match v {
                "" | "t" | "b" | "l" | "r" | "s" | "e" => 5.0,
                "none" => 0.0,
                "sm" => 4.0,
                "md" => 6.0,
                "lg" => 8.0,
                "xl" => 12.0,
                "2xl" => 16.0,
                "3xl" => 24.0,
                "full" => 9999.0,
                _ => return false,
            };
            self.radius = Some(format!("px({})", trim_number(px)));
            return true;
        }
        if class == "border" || class.starts_with("border-") {
            return self.apply_border(class.strip_prefix("border").unwrap_or(""), cond);
        }
        false
    }

    fn set_size(&mut self, v: &str, cond: Option<&str>, width: bool) -> bool {
        let Some(value) = size(v) else { return false };
        let slot = if width {
            &mut self.width
        } else {
            &mut self.height
        };
        *slot = Some(conditional(slot.take(), &value, cond, "auto"));
        true
    }

    /// `border`, `border-t`, `border-b-2`, `border-gray-200`, `border-0`.
    fn apply_border(&mut self, rest: &str, cond: Option<&str>) -> bool {
        let rest = rest.trim_start_matches('-');
        let (sides, width) = match rest.split_once('-') {
            Some((s, w)) if ["t", "r", "b", "l", "x", "y", "s", "e"].contains(&s) => (s, w),
            _ if ["t", "r", "b", "l", "x", "y", "s", "e"].contains(&rest) => (rest, ""),
            _ => ("", rest),
        };
        let sides = match sides {
            "" => "trbl",
            "x" => "rl",
            "y" => "tb",
            "s" => "l",
            "e" => "r",
            s => s,
        };
        let width = match width {
            "" => Some(1.0),
            "0" => Some(0.0),
            "2" => Some(2.0),
            "4" => Some(4.0),
            "8" => Some(8.0),
            _ => None,
        };
        match width {
            Some(w) => {
                let value = if w == 0.0 {
                    "none".to_owned()
                } else {
                    format!("px({})", trim_number(w))
                };
                self.border.set(sides, &value, cond, "none");
                true
            }
            None => match color(rest) {
                Some(c) => {
                    self.border_color = Some(c);
                    true
                }
                None => false,
            },
        }
    }

    /// Adds a static `style` attribute: `color: red; padding: 4px`.
    pub fn css(&mut self, style: &str) {
        for declaration in style.split(';') {
            let Some((property, value)) = declaration.split_once(':') else {
                continue;
            };
            let property = property.trim().to_lowercase();
            let value = value.trim();
            let typst = match property.as_str() {
                "color" | "background-color" | "background" | "border-color" => css_color(value),
                "font-family" => Some(expr::string(
                    value
                        .split(',')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .trim_matches(['\'', '"']),
                )),
                "font-weight" => match value {
                    "bold" => Some("700".to_owned()),
                    "normal" => Some("400".to_owned()),
                    v if v.parse::<u16>().is_ok() => Some(v.to_owned()),
                    _ => None,
                },
                "text-align" | "text-transform" | "white-space" | "display" => {
                    Some(value.to_owned())
                }
                "line-height" => Some(value.to_owned()),
                _ => css_length(value),
            };
            match typst {
                Some(v) => self.property(&property, &v, None),
                None => self.unknown.push(format!("style `{}`", declaration.trim())),
            }
        }
    }

    /// Adds a `:style` binding: `{ color: print.color }`.
    pub fn bound_css(&mut self, binding: &str, scope: &Scope, notes: &mut Vec<String>) {
        let parsed = expr::parse(binding);
        let Ok(expr::Expr::Object(entries)) = parsed else {
            notes.push(format!(":style=\"{binding}\" isn't converted"));
            return;
        };
        for (key, value) in entries {
            let property = kebab(&key);
            let v = expr::to_typst(&value, scope, notes);
            let typst = match property.as_str() {
                "color" | "background-color" | "background" | "border-color" => {
                    format!("css-color({v})")
                }
                "font-family" => v.clone(),
                "font-weight" => format!("js-number({v})"),
                "text-align" | "text-transform" | "white-space" | "display" | "line-height" => {
                    notes.push(format!(":style's {property} isn't converted"));
                    continue;
                }
                _ => format!("css-length({v})"),
            };
            self.property(&property, &typst, Some(&v));
        }
    }

    /// Sets a CSS property to a Typst value. `bound` is the expression of a `:style`
    /// value, for the properties that read it as it is.
    fn property(&mut self, property: &str, v: &str, bound: Option<&str>) {
        match property {
            "color" => self.text_arg(None, "fill", v.to_owned()),
            "background-color" | "background" => self.fill = Some(v.to_owned()),
            "border-color" => self.border_color = Some(v.to_owned()),
            "font-family" => {
                self.font_family = Some(match bound {
                    Some(b) => format!("font-family({b})"),
                    None => format!("font-family({v})"),
                })
            }
            "font-size" => {
                self.text_arg(None, "size", v.to_owned());
                if let Some(px) = v.strip_prefix("px(").and_then(|n| n.strip_suffix(')')) {
                    self.font_size_px = px.parse().ok();
                }
            }
            "font-weight" => self.text_arg(None, "weight", v.to_owned()),
            "letter-spacing" => self.text_arg(None, "tracking", v.to_owned()),
            "text-align" => match v {
                "right" | "end" => self.align.push((None, "right")),
                "center" => self.align.push((None, "center")),
                _ => self.align.push((None, "left")),
            },
            "text-transform" => {
                self.transform = match v {
                    "uppercase" => Some("upper"),
                    "lowercase" => Some("lower"),
                    "capitalize" => Some("capitalize"),
                    _ => None,
                }
            }
            "white-space" => {
                self.whitespace = match v {
                    "pre-line" => Some("pre-line"),
                    "pre" | "pre-wrap" => Some("pre"),
                    "nowrap" => Some("nowrap"),
                    _ => None,
                }
            }
            "display" => {
                self.display = match v {
                    "none" => Some(Display::Hidden),
                    "flex" | "inline-flex" => Some(Display::Flex),
                    "grid" => Some(Display::Grid),
                    "inline" => Some(Display::Inline),
                    "inline-block" => Some(Display::InlineBlock),
                    _ => Some(Display::Block),
                }
            }
            "line-height" => {
                if let Ok(n) = v.parse::<f64>() {
                    self.line_height = Some(LineHeight::Factor(n));
                } else if let Some(px) = v.strip_suffix("px").and_then(|n| n.parse().ok()) {
                    self.line_height = Some(LineHeight::Px(px));
                }
            }
            "width" => self.width = Some(v.to_owned()),
            "height" => self.height = Some(v.to_owned()),
            "max-width" => self.max_width = Some(v.to_owned()),
            "min-height" => self.min_height = Some(v.to_owned()),
            "padding" | "margin" => {
                let sides = if property == "padding" {
                    &mut self.padding
                } else {
                    &mut self.margin
                };
                sides.set("trbl", v, None, "0pt");
            }
            p if p.starts_with("padding-") || p.starts_with("margin-") => {
                let (kind, side) = p.split_once('-').unwrap_or((p, ""));
                let side = match side {
                    "top" => "t",
                    "right" => "r",
                    "bottom" => "b",
                    "left" => "l",
                    _ => return,
                };
                let sides = if kind == "padding" {
                    &mut self.padding
                } else {
                    &mut self.margin
                };
                sides.set(side, v, None, "0pt");
            }
            "border-radius" => self.radius = Some(v.to_owned()),
            _ => self.unknown.push(format!("style `{property}`")),
        }
    }
}

/// `backgroundColor` as `background-color`.
fn kebab(key: &str) -> String {
    let mut out = String::new();
    for c in key.chars() {
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(classes: &str) -> Style {
        let mut s = Style::default();
        for c in classes.split_whitespace() {
            s.class(c, None);
        }
        s
    }

    #[test]
    fn spacing_and_sizes() {
        let s = style("px-12 py-10 -mx-3 mt-0.5 w-1/3 h-12 max-w-32");
        assert_eq!(s.padding.left.as_deref(), Some("sp(12)"));
        assert_eq!(s.padding.top.as_deref(), Some("sp(10)"));
        assert_eq!(s.margin.left.as_deref(), Some("-sp(3)"));
        assert_eq!(s.margin.top.as_deref(), Some("sp(0.5)"));
        assert_eq!(s.width.as_deref(), Some("33.3333%"));
        assert_eq!(s.height.as_deref(), Some("sp(12)"));
        assert_eq!(s.max_width.as_deref(), Some("sp(32)"));
        assert!(s.unknown.is_empty());
    }

    #[test]
    fn text_classes() {
        let s = style("text-lg font-semibold text-gray-800 uppercase tracking-widest text-right");
        assert_eq!(s.font_size_px, Some(14.0));
        assert!(s.text.contains(&(None, "size", "text-size.lg".to_owned())));
        assert!(s.text.contains(&(None, "weight", "600".to_owned())));
        assert!(
            s.text
                .contains(&(None, "fill", "tw(\"gray\", 800)".to_owned()))
        );
        assert!(s.text.contains(&(None, "tracking", "0.1em".to_owned())));
        assert_eq!(s.transform, Some("upper"));
        assert_eq!(s.align, vec![(None, "right")]);
        let s = style("text-2xl");
        assert!(
            s.text
                .contains(&(None, "size", "text-size.at(\"2xl\")".to_owned()))
        );
    }

    #[test]
    fn layout_and_borders() {
        let s = style(
            "flex justify-between items-center gap-2 border-b border-t-2 border-black bg-gray-100 rounded-md",
        );
        assert_eq!(s.display, Some(Display::Flex));
        assert_eq!(s.justify, Some("between"));
        assert_eq!(s.align_items, Some("center"));
        assert_eq!(s.gap_x.as_deref(), Some("sp(2)"));
        assert_eq!(s.border.bottom.as_deref(), Some("px(1)"));
        assert_eq!(s.border.top.as_deref(), Some("px(2)"));
        assert_eq!(s.border_color.as_deref(), Some("tw(\"black\")"));
        assert_eq!(s.fill.as_deref(), Some("tw(\"gray\", 100)"));
        assert_eq!(s.radius.as_deref(), Some("px(6)"));
        let s = style("grid grid-cols-7 col-span-4 col-start-3 row-start-2");
        assert_eq!(s.grid_cols.as_deref(), Some("7"));
        assert_eq!(s.col_span.as_deref(), Some("4"));
    }

    #[test]
    fn variants_and_unknown_classes() {
        let s = style("dark:bg-gray-900 hover:underline md:p-2 lg:p-8 cursor-pointer wobble");
        assert_eq!(s.fill, None);
        assert!(s.decoration.is_empty());
        assert_eq!(s.padding.top.as_deref(), Some("sp(2)"));
        assert_eq!(s.unknown, vec!["class `wobble`".to_owned()]);
    }

    #[test]
    fn conditional_classes() {
        let mut s = Style::default();
        s.class("p-2", None);
        s.class("p-4", Some("x"));
        s.class("font-bold", Some("y"));
        s.class("hidden", Some("z"));
        assert_eq!(
            s.padding.top.as_deref(),
            Some("(if has(x) { sp(4) } else { sp(2) })")
        );
        assert!(
            s.text
                .contains(&(Some("y".to_owned()), "weight", "700".to_owned()))
        );
        assert_eq!(s.hidden_if, vec!["z".to_owned()]);
    }

    #[test]
    fn inline_styles() {
        let mut s = Style::default();
        s.css("color: #ff0000; min-height: 2rem; padding-left: 4px; font-family: 'Courier New', monospace");
        assert!(
            s.text
                .contains(&(None, "fill", "rgb(\"#ff0000\")".to_owned()))
        );
        assert_eq!(s.min_height.as_deref(), Some("px(2 * 16)"));
        assert_eq!(s.padding.left.as_deref(), Some("px(4)"));
        assert_eq!(
            s.font_family.as_deref(),
            Some("font-family(\"Courier New\")")
        );
        let mut notes = Vec::new();
        let mut s = Style::default();
        s.bound_css(
            "{ 'font-family': print.font, backgroundColor: print.color }",
            &Scope::default(),
            &mut notes,
        );
        assert!(notes.is_empty());
        assert_eq!(
            s.font_family.as_deref(),
            Some("font-family(get(print, \"font\"))")
        );
        assert_eq!(s.fill.as_deref(), Some("css-color(get(print, \"color\"))"));
    }
}
