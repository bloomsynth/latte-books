// Latte Books' print library. A print template starts with
//
//   #import "/latte.typ": *
//   #show: setup
//
// and then has what Books gives its HTML templates (`getPrintTemplatePropValues` in
// src/utils/printTemplates.ts @ v0.36.0): `doc`, the document with its values formatted,
// its child rows and its linked documents under `links`; and `print`, the Print Settings
// with the company's GSTIN or tax ID. The Template Builder's Key Hints list every key.
//
// The rest are helpers to lay a page out as Books' HTML templates do, with Tailwind's
// sizes and Books' colours (tailwind.config.js and colors.json @ v0.36.0). Books lays a
// template out in CSS pixels, so a pixel is 0.75pt here. Templates people write depend
// on these names, so they stay as they are.

#let data = json("/data.json")

/// The document, as Books passes it to a template.
#let doc = data.doc
/// The Print Settings and accounting settings, as Books passes them.
#let print = data.print
/// The page, from the template's Print Size: `(width: 21cm, height: 29.7cm)`.
#let page-size = (width: data.page.width * 1cm, height: data.page.height * 1cm)

/// Translates a word or sentence into the app's language: Books' `t`.
#let t(s) = data.t.at(s, default: s)

/// Whether a value counts as there, as JavaScript's `v-if` decides: not `none`, "",
/// `false`, 0 or an empty list.
#let has(v) = v != none and v != "" and v != false and v != 0 and v != () and v != (:)

/// `d.a.b.c`, or `none` when a key on the way isn't there: `get(doc, "links", "party")`.
#let get(d, ..path) = {
  let v = d
  for key in path.pos() {
    if type(v) == dictionary and type(key) == str {
      v = v.at(key, default: none)
    } else if type(v) == array and type(key) == int {
      v = v.at(key, default: none)
    } else {
      return none
    }
  }
  v
}

/// The rows of a table field: a list, or an empty one when there are none (Books gives
/// an empty table as "").
#let rows(v) = if type(v) == array { v } else { () }

/// Text with its line breaks kept, as CSS's `whitespace-pre-line` shows it: a line break
/// at the start is a blank line, one at the end is nothing.
#let lines(s) = if s == none { [] } else {
  let parts = str(s).split("\n").map(l => l.trim())
  if parts.len() > 1 and parts.last() == "" { parts = parts.slice(0, -1) }
  parts.map(l => if l == "" { [#sym.zws] } else { [#l] }).join(linebreak())
}

/// A CSS pixel.
#let px(n) = n * 0.75pt
/// Tailwind spacing: 1 is 4px, so `sp(4)` is `p-4`'s 16px.
#let sp(n) = px(n * 4)

/// Books' text sizes (tailwind.config.js), as `text-xs` … `text-4xl` name them:
/// `text-size.lg`, `text-size.at("2xl")`.
#let text-size = (
  xs: px(11),
  sm: px(12),
  base: px(13),
  lg: px(14),
  xl: px(18),
  "2xl": px(20),
  "3xl": px(24),
  "4xl": px(28),
)

/// Books' colours (colors.json): `colors.gray.at("600")`, or `tw("gray", 600)`.
#let colors = (
  black: rgb("#1E293B"),
  white: rgb("#FFFFFF"),
  gray: ("25": rgb("#FBFBFB"), "50": rgb("#F8F8F8"), "100": rgb("#F3F3F3"), "200": rgb("#EDEDED"), "300": rgb("#E2E2E2"), "400": rgb("#C7C7C7"), "500": rgb("#999999"), "600": rgb("#7C7C7C"), "700": rgb("#525252"), "800": rgb("#383838"), "850": rgb("#282828"), "875": rgb("#212121"), "890": rgb("#1C1C1C"), "900": rgb("#171717")),
  red: ("50": rgb("#FFF7F7"), "100": rgb("#FFF0F0"), "200": rgb("#FCD7D7"), "300": rgb("#F9C6C6"), "400": rgb("#EB9091"), "500": rgb("#E03636"), "600": rgb("#CC2929"), "700": rgb("#B52A2A"), "800": rgb("#941F1F"), "900": rgb("#6B1515")),
  orange: ("50": rgb("#FFF9F5"), "100": rgb("#FFF1E7"), "200": rgb("#FCE6D5"), "300": rgb("#F7D6BD"), "400": rgb("#F0B58B"), "500": rgb("#E86C13"), "600": rgb("#D45A08"), "700": rgb("#BD3E0C"), "800": rgb("#9E3513"), "900": rgb("#6B2711")),
  yellow: ("50": rgb("#FFFCEF"), "100": rgb("#FFF7D3"), "200": rgb("#F7E9A8"), "300": rgb("#F5E171"), "400": rgb("#F2D14B"), "500": rgb("#EDBA13"), "600": rgb("#D1930D"), "700": rgb("#AB6E05"), "800": rgb("#8C5600"), "900": rgb("#733F12")),
  green: ("50": rgb("#F3FCF5"), "100": rgb("#E4F5E9"), "200": rgb("#DAF0E1"), "300": rgb("#CAE5D4"), "400": rgb("#B6DEC5"), "500": rgb("#59BA8B"), "600": rgb("#30A66D"), "700": rgb("#278F5E"), "800": rgb("#16794C"), "900": rgb("#173B2C")),
  teal: ("50": rgb("#F0FDFA"), "100": rgb("#E6F7F4"), "200": rgb("#BAE8E1"), "300": rgb("#97DED4"), "400": rgb("#73D1C4"), "500": rgb("#36BAAD"), "600": rgb("#0B9E92"), "700": rgb("#0F736B"), "800": rgb("#115C57"), "900": rgb("#114541")),
  blue: ("50": rgb("#F7FBFD"), "100": rgb("#EDF6FD"), "200": rgb("#E3F1FD"), "300": rgb("#C9E7FC"), "400": rgb("#70B6F0"), "500": rgb("#33A1FF"), "600": rgb("#007BE0"), "700": rgb("#0070CC"), "800": rgb("#005CA3"), "900": rgb("#004880")),
  indigo: ("100": rgb("#ebf4ff"), "200": rgb("#c3dafe"), "300": rgb("#a3bffa"), "400": rgb("#7f9cf5"), "500": rgb("#667eea"), "600": rgb("#5a67d8"), "700": rgb("#4c51bf"), "800": rgb("#434190"), "900": rgb("#3c366b")),
  purple: ("50": rgb("#FDFAFF"), "100": rgb("#F9F0FF"), "200": rgb("#F1E5FA"), "300": rgb("#E9D6F5"), "400": rgb("#D6C1E6"), "500": rgb("#9C45E3"), "600": rgb("#8642C2"), "700": rgb("#6E399D"), "800": rgb("#5C2F83"), "900": rgb("#401863")),
  pink: ("50": rgb("#FFF7FC"), "100": rgb("#FEEEF8"), "200": rgb("#F8E2F0"), "300": rgb("#F2D4E6"), "400": rgb("#E9C4DA"), "500": rgb("#DF9EB8"), "600": rgb("#CF3A96"), "700": rgb("#9C2671"), "800": rgb("#801458"), "900": rgb("#570F3E")),
  violet: ("50": rgb("#FBFAFF"), "100": rgb("#F5F2FF"), "200": rgb("#E5E1FA"), "300": rgb("#DAD2F7"), "400": rgb("#BDB1F0"), "500": rgb("#6846E3"), "600": rgb("#5F46C7"), "700": rgb("#4F3DA1"), "800": rgb("#392980"), "900": rgb("#251959")),
  cyan: ("50": rgb("#F5FBFC"), "100": rgb("#E0F8FF"), "200": rgb("#B3ECFC"), "300": rgb("#94E6FF"), "400": rgb("#6BD3F2"), "500": rgb("#34BAE3"), "600": rgb("#32A4C7"), "700": rgb("#267A94"), "800": rgb("#125C73"), "900": rgb("#164759")),
  amber: ("50": rgb("#FDFAED"), "100": rgb("#FCF3CF"), "200": rgb("#F7E28D"), "300": rgb("#F5D261"), "400": rgb("#F2BE3A"), "500": rgb("#E79913"), "600": rgb("#DB7706"), "700": rgb("#B35309"), "800": rgb("#91400D"), "900": rgb("#763813")),
)

/// A colour of Books' palette: `tw("gray", 600)`, `tw("black")`.
#let tw(name, ..shade) = {
  let c = colors.at(name, default: none)
  if type(c) == dictionary {
    c.at(str(shade.pos().at(0, default: 500)), default: rgb("#000000"))
  } else if c == none { rgb("#000000") } else { c }
}

/// Tailwind's default border: 1px of gray 200.
#let border = px(1) + colors.gray.at("200")

/// The Print Settings colour, or Books' default.
#let brand = {
  let c = print.at("color", default: none)
  if type(c) == str and c.starts-with("#") and (c.len() == 4 or c.len() == 7) {
    rgb(c)
  } else { rgb("#112B42") }
}

/// The bundled font for a font name. The fonts Books' Print Settings offer are printed
/// with the Liberation fonts, which have their metrics.
#let font-names = (
  "Arial": "Liberation Sans",
  "Helvetica": "Liberation Sans",
  "sans-serif": "Liberation Sans",
  "Times New Roman": "Liberation Serif",
  "Times": "Liberation Serif",
  "serif": "Liberation Serif",
  "Courier": "Liberation Mono",
  "Courier New": "Liberation Mono",
  "monospace": "Liberation Mono",
)
#let font-family(name) = if type(name) != str or name == "" { "Liberation Sans" } else {
  font-names.at(name, default: name)
}

// Ascender and descender, in em, to place lines as CSS's line height does.
#let font-metrics = (
  "liberation sans": (0.905, 0.212),
  "liberation serif": (0.891, 0.216),
  "liberation mono": (0.833, 0.300),
  "inter": (0.969, 0.242),
)

/// The text edges that make each line `height` times the text size, as CSS's
/// `line-height` does: `set text(..line-edges(1.5, "Liberation Sans"))`.
#let line-edges(height, family) = {
  let (a, d) = font-metrics.at(lower(family), default: (0.905, 0.212))
  let half = (height - a - d) / 2
  (top-edge: (a + half) * 1em, bottom-edge: -(d + half) * 1em)
}

/// `body` with lines `height` times the text size: `leading-relaxed` is 1.625.
#let line-height(height, body) = context {
  let f = text.font
  let family = if type(f) == array { f.at(0, default: "Liberation Sans") } else { f }
  if type(family) == dictionary { family = family.at("name", default: "Liberation Sans") }
  set text(..line-edges(height, str(family)))
  body
}

/// An image the document has as a `data:` URL, such as `print.logo`:
/// `picture(print.logo, height: px(48))`. Nothing when there's no such image.
#let picture(src, ..args) = {
  let path = data.images.at(str(src), default: none)
  if path != none { image(path, ..args) }
}

/// Whether the logo is to be shown: Display Logo is on and there is one.
#let show-logo = (
  has(print.at("displayLogo", default: none)) and str(print.at("logo", default: "")) in data.images
)

/// The company's logo, `h-12 max-w-32 object-contain` as Books' templates show it.
#let logo(height: px(48), max-width: px(128)) = context {
  let shown = picture(print.at("logo", default: ""), height: height)
  if shown == none { return }
  if measure(shown).width > max-width {
    box(width: max-width, height: height, picture(
      print.logo,
      width: max-width,
      height: height,
      fit: "contain",
    ))
  } else {
    box(shown)
  }
}

/// The page as Books prints it: no margin, the Print Settings font at 16px, lines 1.5
/// times the text size, and no space between paragraphs or blocks but what the
/// template asks for.
#let setup(body) = {
  let family = font-family(print.at("font", default: none))
  set page(margin: 0pt, fill: white)
  set text(
    font: (family, "Liberation Sans", "Inter"),
    size: px(16),
    fill: rgb("#000000"),
    ..line-edges(1.5, family),
  )
  set par(spacing: 0pt, leading: 0pt, justify: false)
  set block(spacing: 0pt)
  body
}
