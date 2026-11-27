// Basic: templates/Basic.template.html @ v0.36.0, in Typst.
#import "/latte.typ": *
#show: setup

#show: body => pad(x: sp(6), body)

// Invoice Header
#block(width: 100%, inset: (y: sp(6)), stroke: (bottom: border))[
  #set text(size: text-size.sm, fill: tw("gray", 900))
  #grid(
    columns: (1fr, 1fr, 1fr),
    // Company Logo & Name
    if show-logo { logo() } else {
      text(size: text-size.xl, fill: tw("gray", 700), weight: "semibold", print.companyName)
    },
    // Company Contacts
    [
      #block[#print.email]
      #v(sp(1))
      #block[#print.phone]
    ],
    // Company Address & GSTIN
    [
      #if has(print.address) { block(get(print, "links", "address", "addressDisplay")) }
      #if has(print.gstin) { block[GSTIN: #print.gstin] }
    ],
  )
]

// Sub Heading Section
#v(sp(8))
#grid(
  columns: (1fr, 1fr, 1fr),
  // Invoice Details
  [
    #block(text(size: text-size.at("2xl"), weight: "semibold", doc.name))
    #set text(size: text-size.base)
    #block(inset: (y: sp(2)))[#doc.date #h(sp(2)) #doc.at("time", default: "")]
  ],
  [],
  // Party Details
  if has(doc.party) {
    set align(right)
    block(inset: (y: sp(1)), text(size: text-size.lg, weight: "semibold", doc.party))
    set text(size: text-size.xs, fill: tw("gray", 600))
    if has(get(doc, "links", "party", "address")) {
      v(sp(1))
      block(get(doc, "links", "party", "links", "address", "addressDisplay"))
    }
    if has(get(doc, "links", "party", "gstin")) {
      v(sp(1))
      block[GSTIN: #doc.at("partyGSTIN", default: "")]
    }
  },
)

// Items Table
#let description = has(doc.at("description", default: none))
#let columns = (
  (5fr,)
    + if description { (5fr,) } else { () }
    + if doc.showHSN { (2fr,) } else { () }
    + (1fr, 3fr, 3fr)
)
#let cells(item, desc, hsn, quantity, rate, amount) = (
  (item,)
    + if description { (desc,) } else { () }
    + if doc.showHSN { (align(right, hsn),) } else { () }
    + (align(right, quantity), align(right, rate), align(right, amount))
)
#v(sp(8))
#block(width: 100%)[
  #set text(size: text-size.base)
  // Heading Row
  #block(width: 100%, stroke: (bottom: border), text(fill: tw("gray", 600), grid(
    columns: columns,
    inset: (y: sp(4)),
    ..cells(t("Item"), t("Description"), t("HSN/SAC"), t("Quantity"), t("Rate"), t("Amount")),
  )))
  // Body Rows
  #for row in rows(doc.items) {
    block(width: 100%, inset: (y: sp(1)), stroke: (bottom: border), text(
      fill: tw("gray", 900),
      grid(
        columns: columns,
        inset: (y: sp(4)),
        ..cells(row.item, row.at("description", default: ""), row.at("hsnCode", default: ""),
          row.quantity, row.rate, row.amount),
      ),
    ))
  }
]

// Invoice Footer
#v(sp(8))
#grid(
  columns: (1fr, 1fr),
  // Invoice Terms
  {
    if has(doc.at("terms", default: none)) {
      v(sp(2))
      block(text(size: text-size.sm, tracking: 0.1em, fill: tw("gray", 600), t("Notes")))
    }
    v(sp(4))
    block(text(size: text-size.lg, lines(doc.at("terms", default: ""))))
  },
  // Totaled Amounts
  {
    set text(size: text-size.base)
    let line(label, value, top: false, bottom: true) = block(
      width: 100%,
      inset: (left: sp(2), y: sp(3)),
      stroke: (bottom: if bottom { border }, top: if top { border }),
      [#label #h(1fr) #value],
    )
    let discount = has(doc.at("totalDiscount", default: none))
    let after-tax = has(doc.at("discountAfterTax", default: none))
    line(t("Subtotal"), doc.netTotal)
    if discount and not after-tax { line(t("Discount"), doc.totalDiscount) }
    for tax in rows(doc.at("taxes", default: ())) { line(tax.account, tax.amount, bottom: false) }
    if discount and after-tax { line(t("Discount"), doc.totalDiscount, top: true, bottom: false) }
    text(fill: tw("green", 600), weight: "semibold", line(
      t("Grand Total"),
      doc.grandTotal,
      top: true,
      bottom: false,
    ))
  },
)

// Terms and Conditions
#if has(print.displaytermsandconditions) and has(print.termsAndConditions) {
  v(sp(8))
  pad(x: sp(6))[
    #block(text(size: text-size.lg, weight: "semibold", t("Terms and Conditions")))
    #v(sp(4))
    #block(text(size: text-size.lg, lines("\n" + print.termsAndConditions)))
  ]
}
