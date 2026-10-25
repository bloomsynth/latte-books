// Minimal: templates/Minimal.template.html @ v0.36.0, in Typst.
#import "/latte.typ": *
#show: setup

#let sales = doc.entryType == "SalesInvoice"
#let heading(body) = block(text(
  size: text-size.sm,
  weight: "semibold",
  tracking: 0.1em,
  fill: tw("gray", 800),
  upper(body),
))
#let detail(body, space: sp(2)) = {
  v(space)
  block(line-height(1.625, text(size: text-size.lg, fill: tw("black"), body)))
}

// Invoice Header
#block(width: 100%, inset: (x: sp(12), y: sp(10)), stroke: (bottom: border))[
  #grid(
    columns: (auto, auto, 1fr, auto),
    align: horizon,
    // Left Section
    if show-logo { pad(right: sp(4), logo()) },
    [
      #block(text(size: text-size.xl, weight: "semibold", fill: brand, print.companyName))
      #block[#doc.date #h(sp(2)) #doc.at("time", default: "")]
    ],
    [],
    // Right Section
    align(right)[
      #block(text(size: text-size.xl, weight: "semibold", fill: brand, doc.entryLabel))
      #block(doc.name)
    ],
  )
]

// Party && Company Details
#block(width: 100%, inset: (x: sp(12), y: sp(10)), stroke: (bottom: border))[
  #grid(
    columns: (1fr, 1fr),
    // Party Details
    [
      #heading(if sales { "To" } else { "From" })
      #detail(doc.party, space: sp(4))
      #if has(get(doc, "links", "party", "address")) {
        detail(get(doc, "links", "party", "links", "address", "addressDisplay"))
      }
      #if has(get(doc, "links", "party", "gstin")) {
        detail[GSTIN: #get(doc, "links", "party", "gstin")]
      }
    ],
    // Company Details
    pad(left: sp(8))[
      #heading(if sales { "From" } else { "To" })
      #detail(print.companyName, space: sp(4))
      #if has(print.address) { detail(get(print, "links", "address", "addressDisplay")) }
      #if has(print.gstin) { detail[GSTIN: #print.gstin] }
    ],
  )
]

// Items Table
#let description = has(doc.at("description", default: none))
#let columns = (
  (4fr,)
    + if description { (4fr,) } else { () }
    + if doc.showHSN { (2fr,) } else { () }
    + (2fr, 3fr, 3fr)
)
#let cells(item, desc, hsn, quantity, rate, amount) = (
  (item,)
    + if description { (desc,) } else { () }
    + if doc.showHSN { (align(right, hsn),) } else { () }
    + (align(right, quantity), align(right, rate), align(right, amount))
)
#block(width: 100%, inset: (x: sp(12), y: sp(10)), stroke: (bottom: border))[
  // Heading Row
  #block(text(size: text-size.sm, tracking: 0.1em, weight: "semibold", fill: tw("gray", 800), grid(
    columns: columns,
    ..cells(..(t("Item"), t("Description"), t("HSN/SAC"), t("Quantity"), t("Rate"), t("Amount")).map(upper)),
  )))
  #v(sp(4))
  // Body Rows
  #for row in rows(doc.items) {
    block(width: 100%, inset: (y: sp(1)), text(size: text-size.lg, grid(
      columns: columns,
      ..cells(row.item, row.at("description", default: ""), row.at("hsnCode", default: ""),
        row.quantity, row.rate, row.amount),
    )))
  }
]

// Invoice Footer
#block(width: 100%, inset: (x: sp(12), y: sp(10)))[
  #grid(
    columns: (1fr, 1fr),
    // Invoice Terms
    if has(doc.at("terms", default: none)) {
      heading(t("Notes"))
      v(sp(4))
      block(text(size: text-size.lg, lines(doc.terms)))
    },
    // Totaled Amounts
    {
      set text(size: text-size.lg)
      let line(label, value) = block(width: 100%, inset: (left: sp(2), y: sp(1)))[
        #label #h(1fr) #value
      ]
      let discount = has(doc.at("totalDiscount", default: none))
      let after-tax = has(doc.at("discountAfterTax", default: none))
      line(t("Subtotal"), doc.netTotal)
      if discount and not after-tax { line(t("Discount"), doc.totalDiscount) }
      for tax in rows(doc.at("taxes", default: ())) { line(tax.account, tax.amount) }
      if discount and after-tax { line(t("Discount"), doc.totalDiscount) }
      text(weight: "semibold", fill: brand, line(t("Grand Total"), doc.grandTotal))
    },
  )
]

// Terms and Conditions
#if has(print.displaytermsandconditions) and has(print.termsAndConditions) {
  block(width: 100%, inset: (x: sp(12), y: sp(10)))[
    #heading(t("Terms and Conditions"))
    #v(sp(4))
    #block(text(size: text-size.lg, lines("\n" + print.termsAndConditions)))
  ]
}
