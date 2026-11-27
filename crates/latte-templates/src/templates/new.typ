#import "/latte.typ": *
#show: setup

// Edit This Code
#block(width: 100%, inset: sp(4), stroke: (bottom: border))[
  #set text(size: text-size.at("2xl"), weight: "semibold")
  #text(fill: brand, print.companyName)
  #h(1fr)
  #doc.name
]

#block(width: 100%, inset: sp(4))[
  #set text(fill: tw("gray", 600))
  Edit the code in the Template Editor on the right
  to create your own personalized custom template.
]
