// templates/Business.Payment.template.html @ v0.36.0.
#import "/common.typ": *
#show: setup

// Header
#block(width: 100%, fill: gray100, inset: (x: sp(12), y: sp(10)))[
  #grid(
    columns: (auto, 1fr),
    align: horizon,
    if has(pr.displayLogo) and has-logo { box(inset: (right: sp(4)), logo()) },
    [
      #text(size: xl, weight: "semibold", fill: brand, pr.companyName)
      #set text(size: sm, fill: gray800)
      #if has(pr.address) [\ #get(pr, "links", "address", "addressDisplay")]
      #if has(pr.gstin) [\ GSTIN: #pr.gstin]
    ],
  )

  #v(sp(8))
  #set text(size: lg)
  #grid(
    columns: (1fr, 2fr),
    text(weight: "semibold", t("Bill")),
    text(fill: gray800)[
      #text(weight: "semibold", doc.name) \
      #doc.date #h(sp(2)) #doc.at("time", default: "")
    ],
  )
  #v(sp(4))
  #grid(
    columns: (1fr, 2fr),
    text(weight: "semibold", t("party")),
    if has(doc.at("party", default: none)) {
      set text(fill: gray800)
      text(weight: "semibold", doc.party)
      if has(get(doc, "links", "party", "address")) {
        [\ #get(doc, "links", "party", "links", "address", "addressDisplay")]
      }
      if has(get(doc, "links", "party", "gstin")) [\ GSTIN: #get(doc, "links", "party", "gstin")]
    },
  )
]

#set text(size: lg)
#let field-grid(width, ..cells) = block(
  width: width,
  inset: sp(2),
  grid(columns: (2fr, 1fr, 4fr), row-gutter: sp(3), column-gutter: sp(3), ..cells),
)

// Payment details
#block(width: 100%, inset: (x: sp(12), y: sp(12)))[
  #field-grid(
    75%,
    text(weight: "bold", t("Payment Type")), [:], [#doc.paymentType],
    text(weight: "bold", t("Payment Method")), [:], [#doc.paymentMethod],
  )
]

#if rows(doc.taxes).len() > 0 [
  #block(width: 50%, inset: (x: sp(12), bottom: sp(6)))[
    #align(center, text(weight: "semibold", t("Tax Summary")))
    #set text(size: base)
    #for tax in rows(doc.taxes) [
      #block(inset: (left: sp(5)), grid(columns: (1fr, auto), tax.account, tax.amount))
    ]
    #block(inset: (left: sp(5)), grid(columns: (1fr, auto), t("Total Ex.Tax"), doc.at("subTotal", default: "")))
  ]
]

// Footer
#block(width: 100%, inset: (x: sp(12)))[
  #v(sp(5))
  #field-grid(
    100%,
    text(weight: "bold", t("Grand Total")), [:],
    [#text(weight: "bold", doc.amount) \ #doc.grandTotalInWords],
    text(weight: "bold", t("Amount Paid")), [:],
    [#text(weight: "bold", doc.amountPaid) \ #doc.at("amountPaidInWords", default: "")],
  )

  #if has(doc.at("terms", default: none)) [
    #v(sp(12))
    #text(weight: "semibold", t("Notes"))
    #v(sp(4))
    #lines(doc.terms)
  ]

  #if terms-and-conditions [
    #v(sp(12))
    #text(weight: "semibold", t("Terms and Conditions"))
    #v(sp(4))
    #lines(pr.termsAndConditions)
  ]
]
