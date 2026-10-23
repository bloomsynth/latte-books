// templates/Minimal.template.html @ v0.36.0.
#import "/common.typ": *
#show: setup

#let is-sales = doc.entryType == "SalesInvoice"
#let label(body) = text(size: sm, weight: "semibold", fill: gray800, tracking: 0.1em, upper(body))

// Header
#block(width: 100%, inset: (x: sp(12), y: sp(10)), stroke: (bottom: border))[
  #grid(
    columns: (auto, 1fr, auto),
    align: horizon,
    if has(pr.displayLogo) and has-logo { box(inset: (right: sp(4)), logo()) },
    [
      #text(size: xl, weight: "semibold", fill: brand, pr.companyName) \
      #doc.date #h(sp(2)) #doc.at("time", default: "")
    ],
    align(right)[
      #text(size: xl, weight: "semibold", fill: brand, doc.entryLabel) \
      #doc.name
    ],
  )
]

// Party and company
#block(width: 100%, inset: (x: sp(12), y: sp(10)), stroke: (bottom: border))[
  #set text(size: lg)
  #set par(leading: px(10))
  #grid(
    columns: (1fr, 1fr),
    [
      #label(if is-sales { "To" } else { "From" })
      #v(sp(4))
      #doc.at("party", default: "")
      #if has(get(doc, "links", "party", "address")) [
        #v(sp(2))
        #get(doc, "links", "party", "links", "address", "addressDisplay")
      ]
      #if has(get(doc, "links", "party", "gstin")) [
        #v(sp(2))
        GSTIN: #get(doc, "links", "party", "gstin")
      ]
    ],
    block(inset: (left: sp(8)))[
      #label(if is-sales { "From" } else { "To" })
      #v(sp(4))
      #pr.companyName
      #if has(pr.address) [
        #v(sp(2))
        #get(pr, "links", "address", "addressDisplay")
      ]
      #if has(pr.gstin) [
        #v(sp(2))
        GSTIN: #pr.gstin
      ]
    ],
  )
]

// Items
#block(width: 100%, inset: (x: sp(12), y: sp(10)), stroke: (bottom: border))[
  #let columns = item-columns((4, 4, 2, 2, 3, 3))
  #grid(columns: columns, ..item-headers().map(label))
  #v(sp(4))
  #set text(size: lg)
  #for row in rows(doc.items) [
    #block(width: 100%, inset: (y: sp(1)), grid(columns: columns, ..item-cells(row)))
  ]
]

// Footer
#block(width: 100%, inset: (x: sp(12), y: sp(10)))[
  #let total-row(name, value) = block(
    width: 100%,
    inset: (left: sp(2), y: sp(1)),
    grid(columns: (1fr, auto), name, value),
  )
  #grid(
    columns: (1fr, 1fr),
    if has(doc.at("terms", default: none)) [
      #label(t("Notes"))
      #v(sp(4))
      #text(size: lg, lines(doc.terms))
    ],
    [
      #set text(size: lg)
      #total-row(t("Subtotal"), doc.netTotal)
      #if discount-before-tax { total-row(t("Discount"), doc.totalDiscount) }
      #for tax in rows(doc.at("taxes", default: ())) { total-row(tax.account, tax.amount) }
      #if discount-after-tax { total-row(t("Discount"), doc.totalDiscount) }
      #text(weight: "semibold", fill: brand, total-row(t("Grand Total"), doc.grandTotal))
    ],
  )
]

#if terms-and-conditions [
  #block(width: 100%, inset: (x: sp(12), y: sp(10)))[
    #label(t("Terms and Conditions"))
    #v(sp(4))
    #text(size: lg, lines(pr.termsAndConditions))
  ]
]
