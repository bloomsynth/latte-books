// Self Invoice: not in Books. The tax invoice the company issues to itself for a purchase
// under the reverse charge, as section 31(3)(f) of the CGST Act asks of a buyer from a
// supplier who isn't registered, such as one abroad. It is the purchase invoice's own
// values in rupees (`doc.reverseCharge`); the supplier's bill is kept with it.
#import "/latte.typ": *
#show: setup

#let charge = doc.at("reverseCharge", default: none)
#let heading(body) = block(text(
  size: text-size.sm,
  weight: "semibold",
  tracking: 0.1em,
  fill: tw("gray", 800),
  upper(body),
))
#let detail(body, space: sp(1)) = {
  v(space)
  block(line-height(1.5, text(size: text-size.base, fill: tw("black"), body)))
}

#if charge == none {
  block(inset: sp(12), text(size: text-size.lg)[
    #t("This purchase invoice has no row under the reverse charge, so it has no self-invoice.")
  ])
} else {
  // Header
  block(width: 100%, inset: (x: sp(12), y: sp(8)), stroke: (bottom: border))[
    #grid(
      columns: (auto, 1fr, auto),
      align: horizon,
      if show-logo { pad(right: sp(4), logo()) },
      [
        #block(text(size: text-size.xl, weight: "semibold", fill: brand, print.companyName))
        #if has(print.gstin) { block[GSTIN: #print.gstin] }
      ],
      align(right)[
        #block(text(size: text-size.xl, weight: "semibold", fill: brand, t("Self Invoice")))
        #block(doc.name)
        #block(doc.date)
      ],
    )
    #v(sp(2))
    #text(size: text-size.sm, fill: tw("gray", 600), t(
      "Tax invoice issued by the recipient under section 31(3)(f) of the CGST Act, for a supply on which tax is payable under the reverse charge.",
    ))
  ]

  // Supplier and recipient
  block(width: 100%, inset: (x: sp(12), y: sp(8)), stroke: (bottom: border))[
    #grid(
      columns: (1fr, 1fr),
      [
        #heading(t("Supplier"))
        #detail(doc.party, space: sp(3))
        #if has(get(doc, "links", "party", "address")) {
          detail(get(doc, "links", "party", "links", "address", "addressDisplay"))
        }
        #detail(if charge.import {
          t("Not registered under GST (outside India)")
        } else {
          t("Not registered under GST")
        })
      ],
      pad(left: sp(8))[
        #heading(t("Recipient"))
        #detail(print.companyName, space: sp(3))
        #if has(print.address) { detail(get(print, "links", "address", "addressDisplay")) }
        #if has(print.gstin) { detail[GSTIN: #print.gstin] }
        #if has(charge.placeOfSupply) { detail[#t("Place of Supply"): #charge.placeOfSupply] }
        #detail[#t("Tax payable under reverse charge"): #t("Yes")]
      ],
    )
  ]

  // Rows
  let integrated = charge.integrated
  let columns = if integrated { (4fr, 2fr, 3fr, 1.5fr, 3fr) } else { (4fr, 2fr, 3fr, 1.5fr, 3fr, 3fr) }
  let cells(item, hsn, value, rate, ..taxes) = (
    (item, hsn, align(right, value), align(right, rate)) + taxes.pos().map(v => align(right, v))
  )
  block(width: 100%, inset: (x: sp(12), y: sp(8)), stroke: (bottom: border))[
    #block(text(size: text-size.sm, tracking: 0.1em, weight: "semibold", fill: tw("gray", 800), grid(
      columns: columns,
      ..if integrated {
        cells(..(t("Item"), t("HSN/SAC"), t("Taxable Value"), t("Rate"), t("Integrated Tax")).map(upper))
      } else {
        cells(..(t("Item"), t("HSN/SAC"), t("Taxable Value"), t("Rate"), t("Central Tax"), t("State Tax")).map(upper))
      },
    )))
    #v(sp(3))
    #for row in rows(charge.rows) {
      block(width: 100%, inset: (y: sp(1)), text(size: text-size.base, grid(
        columns: columns,
        ..if integrated {
          cells(row.item, row.hsnCode, row.value, row.rate, row.igst)
        } else {
          cells(row.item, row.hsnCode, row.value, row.rate, row.cgst, row.sgst)
        },
      )))
    }
  ]

  // Totals and the supplier's bill
  block(width: 100%, inset: (x: sp(12), y: sp(8)))[
    #grid(
      columns: (1fr, 1fr),
      if has(doc.at("terms", default: none)) {
        heading(t("Supplier's Invoice and Notes"))
        v(sp(3))
        block(text(size: text-size.base, lines(doc.terms)))
      },
      {
        set text(size: text-size.base)
        let line(label, value) = block(width: 100%, inset: (left: sp(2), y: sp(1)))[
          #label #h(1fr) #value
        ]
        line(t("Taxable Value"), charge.value)
        if integrated {
          line(t("Integrated Tax"), charge.igst)
        } else {
          line(t("Central Tax"), charge.cgst)
          line(t("State Tax"), charge.sgst)
        }
        text(weight: "semibold", fill: brand, line(t("Tax Payable"), charge.tax))
      },
    )
  ]

  // Signature
  block(width: 100%, inset: (x: sp(12), y: sp(8)))[
    #align(right)[
      #block(text(size: text-size.base)[#t("For") #print.companyName])
      #v(sp(12))
      #block(text(size: text-size.sm, fill: tw("gray", 600), t("Authorised Signatory")))
    ]
  ]
}
