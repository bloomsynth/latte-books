// Business - Payment: templates/Business.Payment.template.html @ v0.36.0, in Typst.
#import "/latte.typ": *
#show: setup

#let gray800 = tw("gray", 800)

// Header
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
  #grid(
    columns: (1fr, 2fr),
    text(weight: "semibold", t("Bill")),
    text(fill: gray800)[
      #block(text(weight: "semibold", doc.name))
      #block[#doc.date #h(sp(2)) #doc.at("time", default: "")]
    ],
  )
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

#set text(size: text-size.lg)

// `grid grid-cols-7 grid-rows-N p-2 gap-3`: label, colon and value, with every one of
// the N rows as high as the highest, as CSS sizes `1fr` rows.
#let field-grid(width, count, ..fields) = layout(size => {
  let fields = fields.pos()
  let inner = size.width * (width / 100%) - 2 * sp(2)
  let column = (inner - 6 * sp(3)) / 7
  let height = calc.max(..fields.map(((label, value)) => calc.max(
    measure(block(width: 2 * column + sp(3), text(weight: "bold", label))).height,
    measure(block(width: 4 * column + 3 * sp(3), value)).height,
  )))
  block(width: width, inset: sp(2), grid(
    columns: (1fr,) * 7,
    rows: (height,) * count,
    gutter: sp(3),
    ..fields
      .map(((label, value)) => (
        grid.cell(colspan: 2, text(weight: "bold", label)),
        [:],
        grid.cell(colspan: 4, value),
      ))
      .flatten(),
  ))
})

// Payment Details
#block(width: 100%, inset: (x: sp(12), y: sp(12)))[
  #field-grid(
    75%,
    2,
    (t("Payment Type"), [#doc.paymentType]),
    (t("Payment Method"), [#doc.paymentMethod]),
  )
]

#if rows(doc.at("taxes", default: ())).len() > 0 {
  block(width: 50%, inset: (x: sp(12), bottom: sp(6)))[
    #v(sp(2))
    #align(center, block(text(weight: "semibold", t("Tax Summary"))))
    #set text(size: text-size.base)
    #for tax in rows(doc.taxes) {
      block(width: 100%, inset: (left: sp(5)))[#tax.account #h(1fr) #tax.amount]
    }
    #block(width: 100%, inset: (left: sp(5)))[#t("Total Ex.Tax") #h(1fr) #doc.at("subTotal", default: "")]
  ]
}

// Invoice Footer
#block(width: 100%, inset: (x: sp(12)))[
  #v(sp(5))
  #field-grid(
    100%,
    5,
    (t("Grand Total"), [#block(text(weight: "bold", doc.amount))#block(doc.grandTotalInWords)]),
    (
      t("Amount Paid"),
      [#block(text(weight: "bold", doc.amountPaid))#block(doc.at("amountPaidInWords", default: ""))],
    ),
  )

  // Invoice Terms
  #if has(doc.at("terms", default: none)) {
    v(sp(12))
    block(text(weight: "semibold", t("Notes")))
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
