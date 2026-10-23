// templates/Business.template.html @ v0.36.0.
#import "/common.typ": *
#show: setup

#let is-sales = doc.entryType == "SalesInvoice"

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
    text(weight: "semibold", if is-sales { "Invoice" } else { "Bill" }),
    text(fill: gray800)[
      #text(weight: "semibold", doc.name) \
      #doc.date #h(sp(2)) #doc.at("time", default: "")
    ],
  )
  #v(sp(4))
  #grid(
    columns: (1fr, 2fr),
    text(weight: "semibold", if is-sales { "Customer" } else { "Supplier" }),
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

// Items
#block(width: 100%, inset: (x: sp(12), top: sp(12)))[
  #set text(size: lg)
  #let columns = item-columns((4, 4, 2, 2, 3, 3))
  #text(weight: "semibold", grid(columns: columns, ..item-headers()))
  #v(sp(4))
  #for row in rows(doc.items) [
    #block(width: 100%, inset: (y: sp(1)), text(fill: gray800, grid(columns: columns, ..item-cells(row))))
  ]
]

// Footer
#block(width: 100%, inset: (x: sp(12), y: sp(12)))[
  #set text(size: lg)
  #let total(label, value) = align(right, block(inset: (y: sp(3)))[
    #text(fill: gray800, label)
    #v(sp(2))
    #text(size: xl, value)
  ])
  #let cells = (total(t("Subtotal"), doc.netTotal),)
  #if discount-before-tax { cells.push(total(t("Discount"), doc.totalDiscount)) }
  #for tax in rows(doc.at("taxes", default: ())) { cells.push(total(tax.account, tax.amount)) }
  #if discount-after-tax { cells.push(total(t("Discount"), doc.totalDiscount)) }
  #cells.push(block(fill: brand, inset: (x: sp(4), y: sp(3)), align(right, text(fill: white)[
    #t("Grand Total")
    #v(sp(2))
    #text(size: xxl, weight: "semibold", doc.grandTotal)
  ])))
  #block(width: 100%, fill: gray100, outset: (x: sp(3)), align(right, stack(dir: ltr, spacing: sp(8), ..cells)))

  #if has(doc.at("terms", default: none)) [
    #v(sp(12))
    #text(weight: "semibold", t("Notes"))
    #v(sp(4))
    #lines(doc.terms)
  ]

  #if has(pr.amountInWords) [
    #v(sp(10))
    #align(right)[#text(weight: "semibold")[#t("Grand Total In Words"):] #h(sp(2)) #doc.grandTotalInWords]
  ]

  #if terms-and-conditions [
    #v(sp(12))
    #text(weight: "semibold", t("Terms and Conditions"))
    #v(sp(4))
    #lines(pr.termsAndConditions)
  ]
]
