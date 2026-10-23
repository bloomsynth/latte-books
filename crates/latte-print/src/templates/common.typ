// Shared by the templates: the values, Books' Tailwind sizes and colours
// (tailwind.config.js and colors.json @ v0.36.0), and small helpers.
//
// Books lays a template out as HTML 21cm wide, so a CSS pixel is 0.75pt here.

#let data = json("/data.json")
#let doc = data.doc
#let pr = data.print
#let has-logo = data.hasLogo
#let t(s) = data.t.at(s, default: s)

#let px(n) = n * 0.75pt
#let sp(n) = px(n * 4) // Tailwind spacing: 1 = 4px

// fontSize in tailwind.config.js.
#let xs = px(11)
#let sm = px(12)
#let base = px(13)
#let lg = px(14)
#let xl = px(18)
#let xxl = px(20)

// colors.json.
#let gray100 = rgb("#F3F3F3")
#let gray200 = rgb("#EDEDED")
#let gray600 = rgb("#7C7C7C")
#let gray700 = rgb("#525252")
#let gray800 = rgb("#383838")
#let gray900 = rgb("#171717")
#let green600 = rgb("#30A66D")
#let border = 1pt * 0.75 + gray200

#let brand = if pr.color != "" { rgb(pr.color) } else { rgb("#112B42") }

// A value the template shows with `v-if`: JavaScript truthiness.
#let has(v) = v != none and v != "" and v != false and v != 0 and v != ()

// `a.b.c` that is `none` when a link isn't there.
#let get(d, ..path) = {
  let v = d
  for key in path.pos() {
    if type(v) != dictionary { return none }
    v = v.at(key, default: none)
  }
  v
}

// `whitespace-pre-line`.
#let lines(s) = if s == none { [] } else { s.split("\n").map(l => [#l]).join(linebreak()) }

#let rows(v) = if type(v) == array { v } else { () }

#let logo(height: px(48)) = box(height: height, image("/logo", height: height, fit: "contain"))

#let setup(body) = {
  set page(paper: "a4", margin: (top: 0pt, bottom: px(24), x: 0pt))
  set text(
    font: (if has(pr.font) { (pr.font,) } else { () }) + ("Inter", "Arial", "Libertinus Serif"),
    size: px(16),
    fill: black,
    top-edge: "ascender",
    bottom-edge: "descender",
  )
  set par(spacing: 0pt, leading: px(6))
  body
}

// The items table shared by the invoice templates: `widths` are the Tailwind
// fractions of Item, Description, HSN/SAC, Quantity, Rate and Amount.
#let item-columns(widths) = {
  let (item, description, hsn, quantity, rate, amount) = widths
  let columns = (item * 1fr,)
  if has(doc.at("description", default: none)) { columns.push(description * 1fr) }
  if doc.showHSN { columns.push(hsn * 1fr) }
  columns + (quantity * 1fr, rate * 1fr, amount * 1fr)
}

#let item-cells(row) = {
  let cells = ([#row.item],)
  if has(doc.at("description", default: none)) { cells.push([#row.description]) }
  if doc.showHSN { cells.push(align(right)[#row.hsnCode]) }
  cells + (align(right)[#row.quantity], align(right)[#row.rate], align(right)[#row.amount])
}

#let item-headers() = {
  let cells = ([#t("Item")],)
  if has(doc.at("description", default: none)) { cells.push([#t("Description")]) }
  if doc.showHSN { cells.push(align(right)[#t("HSN/SAC")]) }
  cells + (align(right)[#t("Quantity")], align(right)[#t("Rate")], align(right)[#t("Amount")])
}

#let discount-before-tax = has(doc.at("totalDiscount", default: none)) and not has(doc.at("discountAfterTax", default: none))
#let discount-after-tax = has(doc.at("totalDiscount", default: none)) and has(doc.at("discountAfterTax", default: none))
#let terms-and-conditions = has(pr.displaytermsandconditions) and has(pr.termsAndConditions)
