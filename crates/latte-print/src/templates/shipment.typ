// Business - Shipment: templates/Business.Shipment.template.html @ v0.36.0, in Typst.
#import "/latte.typ": *
#show: setup

#let gray800 = tw("gray", 800)

// Invoice Header
#block(width: 100%, fill: tw("gray", 100), inset: (x: sp(12), y: sp(10)))[
  // Company Details
  #grid(
    columns: (auto, 1fr),
    align: horizon,
    if show-logo { pad(right: sp(4), logo()) },
    [
      #block(text(size: text-size.xl, weight: "semibold", fill: brand, print.companyName))
      #set text(size: text-size.sm, fill: gray800)
      #if has(print.address) { block(get(print, "links", "address", "addressDisplay")) }
      #if has(print.gstin) { block[GSTIN: #print.gstin] }
    ],
  )

  // Sub Heading Section
  #v(sp(8))
  #set text(size: text-size.lg)
  // Doc Details
  #grid(
    columns: (1fr, 2fr),
    text(weight: "semibold", t("Bill")),
    text(fill: gray800)[
      #block(text(weight: "semibold", doc.name))
      #block[#doc.date #h(sp(2)) #doc.at("time", default: "")]
    ],
  )
  // Party Details
  #v(sp(4))
  #grid(
    columns: (1fr, 2fr),
    text(weight: "semibold", t("party")),
    if has(doc.party) {
      set text(fill: gray800)
      block(text(weight: "semibold", doc.party))
      if has(get(doc, "links", "party", "address")) {
        block(get(doc, "links", "party", "links", "address", "addressDisplay"))
      }
      if has(get(doc, "links", "party", "gstin")) {
        block[GSTIN: #get(doc, "links", "party", "gstin")]
      }
    },
  )
]

// Items Table: w-1/2, w-3/12 and w-1/4 add up to more than the row, so they share
// it in proportion, as CSS's flex-shrink does.
#let columns = (6fr,) + if doc.showHSN { (3fr,) } else { () } + (3fr, 3fr, 3fr, 3fr)
#let cells(item, hsn, from, quantity, rate, amount) = (
  (item,)
    + if doc.showHSN { (align(right, hsn),) } else { () }
    + (align(right, from), align(right, quantity), align(right, rate), align(right, amount))
)
#v(sp(5))
#pad(x: sp(16))[
  // Heading Row
  #block(width: 100%, stroke: (bottom: border), text(weight: "semibold", grid(
    columns: columns,
    inset: (y: sp(1)),
    ..cells(t("Item"), t("HSN/SAC"), t("From"), t("Quantity"), t("Rate"), t("Amount")),
  )))
  // Body Rows
  #for row in rows(doc.items) {
    block(width: 100%, inset: (y: sp(1)), stroke: (bottom: border), text(
      fill: tw("gray", 900),
      grid(
        columns: columns,
        inset: (y: sp(1)),
        ..cells(row.item, row.at("hsnCode", default: ""), row.at("location", default: ""),
          row.quantity, row.rate, row.amount),
      ),
    ))
  }
]

// Footer
#block(width: 100%, inset: (x: sp(12), top: sp(10)))[
  #set text(size: text-size.lg)
  // Totaled Amounts
  #pad(x: -sp(3), block(width: 100%, fill: tw("gray", 100), grid(
    columns: (1fr, auto, auto),
    column-gutter: sp(8),
    align: (horizon, horizon, top),
    [],
    doc.grandTotalInWords,
    // Grand Total
    block(fill: brand, inset: (x: sp(4), y: sp(3)), align(right, text(fill: white)[
      #block(t("Grand Total"))
      #v(sp(2))
      #block(text(size: text-size.at("2xl"), weight: "semibold", doc.grandTotal))
    ])),
  )))

  // Invoice Terms
  #if has(doc.at("terms", default: none)) {
    v(sp(12))
    block(text(weight: "semibold", "Notes"))
    v(sp(4))
    block(lines(doc.terms))
  }

  // Terms and Conditions
  #if has(print.displaytermsandconditions) and has(print.termsAndConditions) {
    v(sp(12))
    block(text(weight: "semibold", t("Terms and Conditions")))
    v(sp(4))
    block(lines("\n" + print.termsAndConditions))
  }
]
