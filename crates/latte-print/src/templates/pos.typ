// Business-POS: templates/Business-POS.template.html @ v0.36.0, in Typst. The receipt
// is as wide as Print Settings' POS Print Width. It sets no font, so Books prints it in
// Inter, the font of its screens.
#import "/latte.typ": *
#show: setup
#set text(font: ("Inter", "Liberation Sans"), ..line-edges(1.5, "Inter"))

#let black = tw("black")
#let rule = px(1) + black
#let between(left, right) = block(width: 100%)[#left #h(1fr) #right]

#pad(x: px(4) + px(2), top: px(4) + px(2), bottom: px(4) + sp(5))[
  // Invoice Header
  #v(sp(4))
  #align(center)[
    // Company Logo & Name
    #if show-logo {
      block(width: 100%, inset: (y: sp(1)), picture(print.logo, height: sp(18) - 2 * sp(1), fit: "contain"))
    }
    #block(text(size: text-size.xl, weight: "semibold", fill: brand, print.companyName))
    // Company Address
    #if has(print.address) {
      block(width: 100% * 11 / 12, text(size: text-size.lg, get(print, "links", "address", "addressDisplay")))
    }
    // Company Contacts
    #v(sp(1))
    #block(text(size: text-size.base, print.phone))
    #block(text(size: text-size.base, print.email))

    // Invoice Details
    #v(sp(1))
    #block(text(size: text-size.xl, weight: "semibold", "Invoice"))
  ]

  #block(width: 100%, inset: sp(2), text(size: text-size.xs, grid(
    columns: (1fr,) * 7,
    grid.cell(colspan: 2, text(weight: "bold", "Invoice No")), [:], grid.cell(colspan: 4, doc.name),
    grid.cell(colspan: 2, text(weight: "bold", "Customer")), [:], grid.cell(colspan: 4, doc.party),
    grid.cell(colspan: 2, text(weight: "bold", "Date")), [:], grid.cell(colspan: 4, doc.date),
  )))

  // Items Table
  #v(sp(1))
  #pad(x: sp(2))[
    #set text(size: text-size.sm)
    #let columns = (px(16), 40fr, 100fr / 12, 25fr, 25fr)
    // Heading Row
    #block(width: 100%, stroke: (top: rule, bottom: rule), text(weight: "bold", grid(
      columns: columns,
      inset: (y: sp(1)),
      t("SI"), t("Item"), t("Qty"), align(right, t("Price")), align(right, t("Amount")),
    )))
    // Body Rows
    #for (index, row) in rows(doc.items).enumerate() {
      block(width: 100%, inset: (y: sp(1)), text(fill: tw("gray", 900), grid(
        columns: columns,
        inset: (y: sp(1)),
        str(index + 1), row.item, row.quantity, align(right, row.rate), align(right, row.amount),
      )))
    }

    // Totaled Amounts
    #block(width: 100%, inset: (y: sp(1)), stroke: (top: rule, bottom: rule), between("Total", doc.netTotal))

    // Grand Total
    #v(sp(3))
    #grid(
      columns: (auto, 1fr, auto),
      text(size: text-size.lg, weight: "semibold", "Net Total To Pay"),
      [],
      text(size: text-size.xl, weight: "semibold", doc.grandTotal),
    )
  ]

  // Tax Breakdown
  #if rows(doc.at("taxes", default: ())).len() > 0 {
    pad(x: sp(2))[
      #v(sp(2))
      #block(width: 100%, stroke: (top: rule), align(center, text(size: text-size.lg, weight: "semibold", "Tax Summary")))
      #set text(size: text-size.base)
      #pad(left: sp(5), between("Total Ex.Tax", doc.at("subTotal", default: "")))
      #for tax in rows(doc.taxes) { pad(left: sp(5), between(tax.account, tax.amount)) }
    ]
  }

  // Discounts & Payments
  #pad(x: sp(1))[
    #v(sp(2))
    #block(width: 100%, stroke: (bottom: rule))[
      #set text(size: text-size.sm)
      #grid(columns: (1fr,) * 3, t("Payment"), align(right, t("Tendered")), align(right, t("Balance")))
      #block(width: 100%, stroke: (top: rule), text(size: text-size.xs, grid(
        columns: (1fr,) * 3,
        t("Discount"),
        align(right, text(weight: "medium", if has(doc.at("totalDiscount", default: none)) {
          doc.totalDiscount
        } else { "00.00" })),
      )))
      #for row in rows(doc.at("paymentDetails", default: ())) {
        text(size: text-size.xs, grid(
          columns: (1fr,) * 3,
          row.paymentMethod,
          align(right, text(weight: "medium", row.amount)),
          align(right, text(weight: "medium", row.outstandingAmount)),
        ))
      }
    ]
  ]

  #v(sp(3))
  #align(center, block(inset: (top: sp(1), bottom: sp(5)), text(size: text-size.lg, "***** Thank You Visit Again *****")))
]
