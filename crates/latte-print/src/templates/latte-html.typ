
// Helpers for templates converted from Books' HTML templates, which read values as
// JavaScript does and lay boxes out as CSS does. (Part of /latte.typ.)

/// The page as Books prints an HTML template: as `setup`, but in Inter, Books' own font,
/// until the template sets one.
#let setup-html(body) = {
  set page(margin: 0pt, fill: white)
  set text(
    font: ("Inter", "Liberation Sans"),
    size: px(16),
    fill: rgb("#000000"),
    ..line-edges(1.5, "Inter"),
  )
  set par(spacing: 0pt, leading: 0pt, justify: false)
  set block(spacing: 0pt)
  body
}

/// A value as `{{ }}` shows it: nothing for `none`, numbers without a needless `.0`.
#let val(v) = {
  if v == none { "" } else if type(v) == str or type(v) == content { v } else if type(v) == bool {
    if v { "true" } else { "false" }
  } else if type(v) == float and calc.fract(v) == 0 and calc.abs(v) < 1e15 {
    str(int(v))
  } else if type(v) == int or type(v) == float { str(v) } else { repr(v) }
}

/// A value as a number, as JavaScript's `Number` reads it.
#let js-number(v) = {
  if type(v) == int or type(v) == float { v } else if type(v) == bool { int(v) } else if type(v) == str {
    let s = v.trim()
    if s == "" { 0 } else if s.match(regex("^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$")) != none {
      float(s)
    } else { float.nan }
  } else if v == none { 0 } else { float.nan }
}

/// `a + b`: joined as text when either is text, else added.
#let js-add(a, b) = {
  let text-like(v) = type(v) == str or type(v) == content
  if text-like(a) or text-like(b) { val(a) + val(b) } else { js-number(a) + js-number(b) }
}

/// `a == b`, where text and a number are compared as numbers.
#let js-eq(a, b) = {
  let number(v) = type(v) == int or type(v) == float
  if (number(a) and type(b) == str) or (type(a) == str and number(b)) {
    js-number(a) == js-number(b)
  } else { a == b }
}

/// `a < b` and the like: text with text, else as numbers.
#let js-compare(a, op, b) = {
  let (x, y) = if type(a) == str and type(b) == str { (a, b) } else { (js-number(a), js-number(b)) }
  if op == "<" { x < y } else if op == ">" { x > y } else if op == "<=" { x <= y } else { x >= y }
}

/// `v.length`.
#let js-len(v) = {
  if type(v) == str { v.codepoints().len() } else if type(v) == array or type(v) == dictionary {
    v.len()
  } else { 0 }
}

/// What `v-for` walks: `(value, index)` for each item of a list, `(value, key)` for an
/// object, and `n in 5` counts from 1.
#let js-each(v) = {
  if type(v) == array { v.enumerate().map(((i, x)) => (x, i)) } else if type(v) == dictionary {
    v.pairs().map(((k, x)) => (x, k))
  } else if type(v) == int { range(v).map(i => (i + 1, i)) } else if type(v) == str {
    v.clusters().enumerate().map(((i, x)) => (x, i))
  } else { () }
}

/// `v.toFixed(digits)`.
#let js-fixed(v, digits) = {
  let n = js-number(v)
  let digits = if digits == none { 0 } else { int(digits) }
  let s = str(calc.round(n, digits: digits))
  if digits > 0 {
    let (whole, fraction) = if s.contains(".") { s.split(".") } else { (s, "") }
    whole + "." + fraction + "0" * (digits - fraction.len())
  } else { s }
}

#let js-includes(v, x) = {
  if type(v) == str { v.contains(val(x)) } else if type(v) == array { v.contains(x) } else { false }
}

#let js-join(v, separator) = rows(v).map(val).join(if separator == none { "," } else { val(separator) })

#let js-slice(v, start, ..end) = {
  let end = end.pos().at(0, default: none)
  let parts = if type(v) == str { v.clusters() } else if type(v) == array { v } else { () }
  let n = parts.len()
  let clamp(i) = if i < 0 { calc.max(0, n + i) } else { calc.min(i, n) }
  let from = clamp(int(js-number(start)))
  let to = if end == none { n } else { clamp(int(js-number(end))) }
  let sliced = if from < to { parts.slice(from, to) } else { () }
  if type(v) == str { sliced.join("") } else { sliced }
}

/// `t` with values in it, as Books' t`Pay ${x}` is: `tf("Pay ${0}", x)`.
#let tf(s, ..args) = {
  let out = t(s)
  for (i, a) in args.pos().enumerate() { out = out.replace("${" + str(i) + "}", val(a)) }
  out
}

/// A CSS colour value: `#fff`, `rgb(0, 0, 0)`, or a colour.
#let css-color(v) = {
  if type(v) == color { return v }
  if type(v) != str { return none }
  let v = v.trim()
  if v.match(regex("^#([0-9a-fA-F]{3,4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$")) != none { return rgb(v) }
  let m = v.match(regex("^rgba?\(\s*(\d+)[\s,]+(\d+)[\s,]+(\d+)"))
  if m != none { return rgb(..m.captures.map(int)) }
  let named = (black: "#000000", white: "#FFFFFF", red: "#FF0000", green: "#008000", blue: "#0000FF")
  if lower(v) in named { rgb(named.at(lower(v))) } else { none }
}

/// A CSS length value: a number of pixels, `12px`, `1rem`, `2cm`, `50%`.
#let css-length(v) = {
  if type(v) == length or type(v) == ratio or type(v) == relative { return v }
  if type(v) == int or type(v) == float { return px(v) }
  if type(v) != str { return auto }
  let m = v.trim().match(regex("^(-?[\d.]+)\s*(px|rem|em|pt|cm|mm|in|%)?$"))
  if m == none { return auto }
  let n = float(m.captures.at(0))
  let unit = m.captures.at(1)
  if unit == none or unit == "px" { px(n) } else if unit == "rem" { px(n * 16) } else if unit == "em" {
    n * 1em
  } else if unit == "pt" { n * 1pt } else if unit == "cm" { n * 1cm } else if unit == "mm" {
    n * 1mm
  } else if unit == "in" { n * 1in } else { n * 1% }
}

/// A side of a border that a condition turns on: `none` when it's off.
#let css-stroke(width, paint, dash: none) = if width == none { none } else {
  (thickness: width, paint: paint, dash: dash)
}

/// An `<img>` from the document's values, sized as its classes say.
#let css-img(src, height: auto, width: auto, max-width: none, fit: "stretch") = context {
  let fit = if width == auto or height == auto { "contain" } else { fit }
  let shown = picture(src, height: height, width: width, fit: fit)
  if shown == none { return }
  if max-width != none and width == auto and measure(shown).width > max-width {
    box(width: max-width, height: height, picture(src, width: max-width, height: height, fit: "contain"))
  } else { box(shown) }
}

/// A flex row (`display: flex`): `items` are `(basis, width, grow, shrink, ml, mr,
/// ml-auto, mr-auto, self, body: w => …)`, sized as CSS sizes flex items: each starts
/// at its basis or width (or what it needs), shares out what space is left by `grow`,
/// and shrinks in proportion when there's too little.
#let css-flex(items, gap: 0pt, justify: "start", align: "stretch", height: auto) = layout(size => {
  let items = items.filter(i => i != none)
  if items.len() == 0 { return }
  let full = size.width
  let to-length(b) = {
    if type(b) == ratio { full * (b / 100%) } else if type(b) == relative {
      b.length.to-absolute() + full * (b.ratio / 100%)
    } else if type(b) == length { b.to-absolute() } else { none }
  }
  let bases = items.map(i => {
    let b = to-length(i.at("basis", default: i.at("width", default: auto)))
    // What it needs, with a hair more so that laying it out again doesn't wrap it.
    if b == none { measure((i.body)(auto)).width + 0.1pt } else { b }
  })
  let margins = items.map(i => i.at("ml", default: 0pt) + i.at("mr", default: 0pt))
  let n = items.len()
  let free = full - bases.sum() - margins.sum() - gap * (n - 1)
  let widths = bases
  if free > 0pt {
    let grows = items.map(i => i.at("grow", default: 0))
    let total = grows.sum()
    if total > 0 {
      widths = bases.zip(grows).map(((b, g)) => b + free * g / total)
      free = 0pt
    }
  } else if free < 0pt {
    let weights = items.zip(bases).map(((i, b)) => i.at("shrink", default: 1) * (b / 1pt))
    let total = weights.sum()
    if total > 0 {
      widths = bases.zip(weights).map(((b, s)) => calc.max(0pt, b + free * s / total))
      free = 0pt
    }
  }
  let space = calc.max(free, 0pt)
  let autos = items.map(i => (i.at("ml-auto", default: false), i.at("mr-auto", default: false)))
  let auto-count = autos.map(((l, r)) => int(l) + int(r)).sum()
  let lead = 0pt
  let between = gap
  let before = autos.map(_ => 0pt)
  let after = autos.map(_ => 0pt)
  if auto-count > 0 {
    let share = space / auto-count
    before = autos.map(((l, r)) => if l { share } else { 0pt })
    after = autos.map(((l, r)) => if r { share } else { 0pt })
  } else if justify == "end" { lead = space } else if justify == "center" { lead = space / 2 } else if (
    justify == "between" and n > 1
  ) { between = gap + space / (n - 1) } else if justify == "around" {
    lead = space / n / 2
    between = gap + space / n
  } else if justify == "evenly" {
    lead = space / (n + 1)
    between = gap + space / (n + 1)
  }
  // Items are as high as the row unless they're aligned otherwise, as CSS stretches them.
  let row = items.zip(widths).map(((item, w)) => measure(block(width: w, (item.body)(w))).height)
  let row = calc.max(..row, if height == auto { 0pt } else { height })
  let columns = ()
  let cells = ()
  if lead > 0pt {
    columns.push(lead)
    cells.push([])
  }
  for (k, item) in items.enumerate() {
    if k > 0 and between != 0pt {
      columns.push(between)
      cells.push([])
    }
    let ml = item.at("ml", default: 0pt) + before.at(k)
    let mr = item.at("mr", default: 0pt) + after.at(k)
    let w = widths.at(k)
    columns.push(calc.max(0pt, w + ml + mr))
    let a = item.at("self", default: align)
    let v = if a == "center" { horizon } else if a == "end" { bottom } else { top }
    let body = if a == "stretch" { (item.body)(w, h: row) } else { (item.body)(w) }
    cells.push(grid.cell(align: v, pad(left: ml, right: mr, body)))
  }
  grid(columns: columns, rows: if height == auto { auto } else { row }, ..cells)
})

/// The height inside a box of `height` with `padding` above and below it.
#let css-inner(height, padding) = if height == auto { auto } else { height - padding }

/// A flex column (`flex-col`): the items one under another, `gap` apart, stretched or
/// aligned as `align` says.
#let css-flex-col(items, gap: 0pt, align: "stretch") = layout(size => {
  let items = items.filter(i => i != none)
  for (k, item) in items.enumerate() {
    if k > 0 and gap != 0pt { v(gap) }
    let a = item.at("self", default: align)
    let ml = item.at("ml", default: 0pt)
    let mr = item.at("mr", default: 0pt)
    let width = item.at("width", default: if a == "stretch" { size.width - ml - mr } else { auto })
    let body = pad(left: ml, right: mr, (item.body)(width))
    if a == "stretch" { body } else {
      std.align(if a == "center" { center } else if a == "end" { right } else { left }, body)
    }
  }
})

/// A CSS grid (`grid grid-cols-N`): `cells` are `(col-span, col-start, row-start,
/// row-span, body: w => …)`, placed as CSS places them. With `rows`, every row is as
/// high as the highest, as `grid-rows-N`'s `1fr` rows are.
#let css-grid(cells, cols: none, rows: none, gap-x: 0pt, gap-y: 0pt) = layout(size => {
  let cells = cells.filter(c => c != none)
  let n = if cols == none { 1 } else { cols }
  let taken = (:)
  let key(r, c) = str(r) + "," + str(c)
  let fits(taken, r, c, span) = c >= 0 and c + span <= n and range(span).all(k => key(r, c + k) not in taken)
  let placed = ()
  let (row, col) = (0, 0)
  for cell in cells {
    let span = cell.at("col-span", default: 1)
    if span < 1 or span > n { span = n }
    let rspan = calc.max(1, cell.at("row-span", default: 1))
    let x = cell.at("col-start", default: none)
    let y = cell.at("row-start", default: none)
    if x != none { x = calc.min(calc.max(x - 1, 0), n - 1) }
    if y != none { y = calc.max(y - 1, 0) }
    let pos = none
    if x != none and y != none {
      // CSS lets placed items overlap; here an earlier item in the way is cut short.
      for (j, p) in placed.enumerate() {
        if p.pos.at(0) == y and p.pos.at(1) < x and p.pos.at(1) + p.span > x {
          for k in range(x, p.pos.at(1) + p.span) { let _ = taken.remove(key(y, k), default: none) }
          placed.at(j).span = x - p.pos.at(1)
        }
      }
      let s = calc.min(span, n - x)
      while s > 1 and not fits(taken, y, x, s) { s -= 1 }
      if fits(taken, y, x, s) {
        pos = (y, x)
        span = s
      }
    } else if y != none {
      for c in range(n) { if pos == none and fits(taken, y, c, span) { pos = (y, c) } }
    } else if x != none {
      let s = calc.min(span, n - x)
      let r = row
      while pos == none and r < row + 500 {
        if fits(taken, r, x, s) {
          pos = (r, x)
          span = s
        }
        r += 1
      }
    }
    if pos == none {
      let (r, c) = (row, col)
      while pos == none {
        if c + span > n {
          r += 1
          c = 0
        } else if fits(taken, r, c, span) { pos = (r, c) } else { c += 1 }
      }
      (row, col) = (pos.at(0), pos.at(1) + span)
    }
    for k in range(span) {
      for rr in range(rspan) { taken.insert(key(pos.at(0) + rr, pos.at(1) + k), true) }
    }
    placed.push((pos: pos, span: span, rspan: rspan, body: cell.body))
  }
  let column = (size.width - gap-x * (n - 1)) / n
  let heights = auto
  if rows != none and placed.len() > 0 {
    let h = calc.max(0pt, ..placed.map(p => measure(block(
      width: column * p.span + gap-x * (p.span - 1),
      (p.body)(column * p.span + gap-x * (p.span - 1)),
    )).height / p.rspan))
    let count = calc.max(rows, ..placed.map(p => p.pos.at(0) + p.rspan))
    heights = (h,) * count
  }
  grid(
    columns: (1fr,) * n,
    rows: heights,
    column-gutter: gap-x,
    row-gutter: gap-y,
    ..placed.map(p => grid.cell(
      x: p.pos.at(1),
      y: p.pos.at(0),
      colspan: p.span,
      rowspan: p.rspan,
      (p.body)(column * p.span + gap-x * (p.span - 1)),
    )),
  )
})

/// A `<table>`: `rows` are `(fill, cells: ((colspan, body: w => …), …))`.
#let css-table(rows, columns: 1, full: false) = {
  let cells = ()
  for row in rows.filter(r => r != none) {
    let used = 0
    for cell in row.cells {
      let span = calc.max(1, calc.min(cell.at("colspan", default: 1), columns - used))
      if used < columns {
        cells.push(grid.cell(
          colspan: span,
          fill: row.fill,
          align: cell.at("align", default: left),
          (cell.body)(auto),
        ))
        used += span
      }
    }
    for _ in range(columns - used) { cells.push(grid.cell(fill: row.fill, [])) }
  }
  grid(columns: if full { (1fr,) * columns } else { (auto,) * columns }, ..cells)
}
