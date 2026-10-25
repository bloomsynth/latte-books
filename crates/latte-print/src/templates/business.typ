// Business: templates/Business.template.html @ v0.36.0, in Typst.
#import "/latte.typ": *
#show: setup

#let sales = doc.entryType == "SalesInvoice"
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
    text(weight: "semibold", if sales { "Invoice" } else { "Bill" }),
    text(fill: gray800)[
      #block(text(weight: "semibold", doc.name))
      #block[#doc.date #h(sp(2)) #doc.at("time", default: "")]
    ],
  )
  // Party Details
  #v(sp(4))
  #grid(
    columns: (1fr, 2fr),
    text(weight: "semibold", if sales { "Customer" } else { "Supplier" }),
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
#block(width: 100%, inset: (x: sp(12), top: sp(12)))[
  #set text(size: text-size.lg)
  // Heading Row
  #block(text(weight: "semibold", grid(
    columns: columns,
    ..cells(t("Item"), t("Description"), t("HSN/SAC"), t("Quantity"), t("Rate"), t("Amount")),
  )))
  #v(sp(4))
  // Body Rows
  #for row in rows(doc.items) {
    block(width: 100%, inset: (y: sp(1)), text(fill: gray800, grid(
      columns: columns,
      ..cells(row.item, row.at("description", default: ""), row.at("hsnCode", default: ""),
        row.quantity, row.rate, row.amount),
    )))
  }
]

// Invoice Footer
#block(width: 100%, inset: (x: sp(12), y: sp(12)))[
  #set text(size: text-size.lg)
  // Totaled Amounts
  #let total(label, value) = block(inset: (y: sp(3)), align(right)[
    #block(text(fill: gray800, label))
    #v(sp(2))
    #block(text(size: text-size.xl, value))
  ])
  #let discount = has(doc.at("totalDiscount", default: none))
  #let after-tax = has(doc.at("discountAfterTax", default: none))
  #let totals = (total(t("Subtotal"), doc.netTotal),)
  #if discount and not after-tax { totals.push(total(t("Discount"), doc.totalDiscount)) }
  #for tax in rows(doc.at("taxes", default: ())) { totals.push(total(tax.account, tax.amount)) }
  #if discount and after-tax { totals.push(total(t("Discount"), doc.totalDiscount)) }
  #totals.push(block(fill: brand, inset: (x: sp(4), y: sp(3)), align(right, text(fill: white)[
    #block(t("Grand Total"))
    #v(sp(2))
    #block(text(size: text-size.at("2xl"), weight: "semibold", doc.grandTotal))
  ])))
  #pad(x: -sp(3), block(width: 100%, fill: tw("gray", 100), align(right, stack(
    dir: ltr,
    spacing: sp(8),
    ..totals,
  ))))

  // Invoice Terms
  #if has(doc.at("terms", default: none)) {
    v(sp(12))
    block(text(size: text-size.lg, weight: "semibold", t("Notes")))
    v(sp(4))
    block(lines(doc.terms))
  }

  #if has(print.amountInWords) {
    v(sp(10))
    align(right, block[
      #text(weight: "semibold")[#t("Grand Total In Words"):]#h(sp(2))#doc.grandTotalInWords
    ])
  }

  // Terms and Conditions
  #if has(print.displaytermsandconditions) and has(print.termsAndConditions) {
    v(sp(12))
    block(text(size: text-size.lg, weight: "semibold", t("Terms and Conditions")))
    v(sp(4))
    block(lines("\n" + print.termsAndConditions))
  }
]
