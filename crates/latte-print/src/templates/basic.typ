// templates/Basic.template.html @ v0.36.0.
#import "/common.typ": *
#show: setup

#block(inset: (x: sp(6)), width: 100%)[
  // Invoice header
  #block(width: 100%, inset: (y: sp(6)), stroke: (bottom: border))[
    #set text(size: sm, fill: gray900)
    #grid(
      columns: (1fr, 1fr, 1fr),
      if has(pr.displayLogo) and has-logo { logo() } else {
        text(size: xl, fill: gray700, weight: "semibold", pr.companyName)
      },
      [#pr.email #v(sp(1)) #pr.phone],
      [
        #if has(pr.address) [#get(pr, "links", "address", "addressDisplay") \ ]
        #if has(pr.gstin) [GSTIN: #pr.gstin]
      ],
    )
  ]

  // Sub heading
  #v(sp(8))
  #grid(
    columns: (1fr, 1fr, 1fr),
    [
      #text(size: xxl, weight: "semibold", doc.name)
      #v(sp(2))
      #text(size: base)[#doc.date #h(sp(2)) #doc.at("time", default: "")]
    ],
    [],
    if has(doc.at("party", default: none)) {
      set align(right)
      text(size: lg, weight: "semibold", doc.party)
      let address = get(doc, "links", "party", "links", "address", "addressDisplay")
      set text(size: xs, fill: gray600)
      if has(get(doc, "links", "party", "address")) { v(sp(1)); [#address] }
      if has(get(doc, "links", "party", "gstin")) { v(sp(1)); [GSTIN: #doc.at("partyGSTIN", default: "")] }
    },
  )

  // Items
  #v(sp(8))
  #set text(size: base)
  #let columns = item-columns((5, 5, 2, 1, 3, 3))
  #block(width: 100%, stroke: (bottom: border))[
    #set text(fill: gray600)
    #grid(columns: columns, inset: (y: sp(4)), ..item-headers())
  ]
  #for row in rows(doc.items) [
    #block(width: 100%, inset: (y: sp(1)), stroke: (bottom: border))[
      #set text(fill: gray900)
      #grid(columns: columns, inset: (y: sp(4)), ..item-cells(row))
    ]
  ]

  // Footer
  #v(sp(8))
  #let total-row(label, value, top: false, bottom: true) = block(
    width: 100%,
    inset: (left: sp(2), y: sp(3)),
    stroke: (bottom: if bottom { border } else { none }, top: if top { border } else { none }),
    grid(columns: (1fr, auto), label, value),
  )
  #grid(
    columns: (1fr, 1fr),
    [
      #if has(doc.at("terms", default: none)) [
        #v(sp(2))
        #text(size: sm, fill: gray600, tracking: 0.1em, t("Notes"))
      ]
      #v(sp(4))
      #text(size: lg, lines(doc.at("terms", default: "")))
    ],
    [
      #total-row(t("Subtotal"), doc.netTotal)
      #if discount-before-tax { total-row(t("Discount"), doc.totalDiscount) }
      #for tax in rows(doc.at("taxes", default: ())) { total-row(tax.account, tax.amount, bottom: false) }
      #if discount-after-tax { total-row(t("Discount"), doc.totalDiscount, top: true, bottom: false) }
      #text(fill: green600, weight: "semibold", size: base, total-row(t("Grand Total"), doc.grandTotal, top: true, bottom: false))
    ],
  )

  #if terms-and-conditions [
    #v(sp(8))
    #block(inset: (x: sp(6)))[
      #text(size: lg, weight: "semibold", t("Terms and Conditions"))
      #v(sp(4))
      #text(size: lg, lines(pr.termsAndConditions))
    ]
  ]
]
