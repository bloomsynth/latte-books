# Changelog

What changes in each release of Latte Books. A release's notes on GitHub are its section
here.

## [Unreleased]

## [0.1.0-beta.3] - 2026-10-26

Latte now looks and works like Frappe Books on every screen.

### Looks like Frappe Books

- Frappe Books' colours, font, icons and layout on every screen, and **dark mode**
  (System Settings).
- **Search** (Ctrl+K, ⌘K on macOS) with recent items and filters, and a **Shortcuts** window
  listing every keyboard shortcut.
- Messages that close by themselves and offer a next step, such as **Open Folder** after
  an export.

### Forms

- Edit a table row's other fields in a panel beside the form, and see **Linked Entries**
  (Ctrl+L).
- Link fields search as you type, with **Create** to add a new record without leaving
  the form.
- **Make Payment** opens over the invoice and marks it paid when you save.
- Frappe Books' checks before saving, submitting and deleting, such as a warning when an
  item is short of stock.
- Set the **exchange rate** on an invoice in another currency.
- A calendar for dates, a colour picker, attachments, and a barcode box for scanners.

### Printing

- Templates are written in Typst and edited in the **Template Builder**, with a live
  preview. Printouts use fonts that come with Latte, so they look the same on every
  computer.
- New templates for shipments and POS receipts.
- **Convert to Typst** turns a template you made in Frappe Books into one you can edit in
  Latte. The original is kept for Frappe Books.
- **Print** opens your computer's print dialog.
- Print reports, and export any report as JSON.

### Point of sale

- The Modern layout as well as the Classic one, item pictures and groups.
- Coupons, loyalty points, price lists, returns and item enquiries at the till.
- Change a row's rate, discount or quantity at the till, with an on-screen keypad.

### Also

- Lists: pick items and create a quote or invoice from them; filter any list.
- The Import Wizard edits cells in place and lets you fix the rows that failed.
- On Windows, the installer updates an older Latte Books in place, without asking whether
  to uninstall it first.
- Fixed: a new payment could default to the Cash In Hand or Bank Accounts group on a fast
  computer, leaving it out of the cash account and the Cashflow chart.
- Fixed: removing a coupon from an invoice could leave its discounted rate on the row.
- 70 Frappe Books bugs fixed in all, each listed in
  [docs/BOOKS-BUGS.md](https://github.com/bloomsynth/latte-books/blob/main/docs/BOOKS-BUGS.md).

### Installing

The installers aren't signed yet, so your system warns you the first time:

- **Windows** (`.exe`): SmartScreen says "Windows protected your PC". Choose **More info**,
  then **Run anyway**. Latte installs for your user only, without admin rights.
- **macOS** (`.dmg`, Apple Silicon): drag Latte Books to Applications, then right-click it
  and choose **Open**. If macOS says the app is damaged, run
  `xattr -cr "/Applications/Latte Books.app"` in Terminal.
- **Linux**: install the `.deb`, or make the `.AppImage` executable and run it.

Check a download against `SHA256SUMS.txt`, or that it was built from this repository with
`gh attestation verify <file> --repo bloomsynth/latte-books`.

### Known issues

- Frappe Books' own HTML templates don't print directly: convert them to Typst first.
- Exchange rates aren't fetched: set the rate on an invoice in another currency yourself.
- Arabic and Persian are shown left to right.

## [0.1.0-beta.2] - 2026-10-25

- The dashboard draws Frappe Books' charts: cash in and out as shaded areas, profit and
  loss as bars above and below zero, and the top expenses as a donut with the total in
  the middle.

### Installing

The installers aren't signed yet, so your system warns you the first time:

- **Windows** (`.exe`): SmartScreen says "Windows protected your PC". Choose **More info**,
  then **Run anyway**. Latte installs for your user only, without admin rights.
- **macOS** (`.dmg`, Apple Silicon): drag Latte Books to Applications, then right-click it
  and choose **Open**. If macOS says the app is damaged, run
  `xattr -cr "/Applications/Latte Books.app"` in Terminal.
- **Linux**: install the `.deb`, or make the `.AppImage` executable and run it.

Check a download against `SHA256SUMS.txt`, or that it was built from this repository with
`gh attestation verify <file> --repo bloomsynth/latte-books`.

## [0.1.0-beta.1] - 2026-10-24

The first beta: Frappe Books v0.36.0, ported to Rust as a native desktop app.

**Try it on a copy of your company file**, and keep a backup. Latte reads and writes the
same `.books.db` file as Frappe Books, which can still open it afterwards.

### What's in it

- Everything in Frappe Books v0.36.0: sales and purchases, quotes, payments, journal
  entries, returns, inventory with shipments and receipts, point of sale, pricing rules,
  coupons and loyalty points, data import, custom fields, the dashboard, and reports from
  the General Ledger to the Balance Sheet.
- India GST and Switzerland country modules, with GSTR-1 and GSTR-2.
- Printing to PDF with Frappe Books' templates.
- 18 languages, from Frappe Books' community translations.
- 64 Frappe Books bugs fixed, each listed in
  [docs/BOOKS-BUGS.md](https://github.com/bloomsynth/latte-books/blob/main/docs/BOOKS-BUGS.md).
- **Create Demo**, a company with a year of made-up sales, purchases and payments.
- **Dime**: Claude, Codex or Cursor can look things up and draft documents in your books,
  set up in one click from **Set up AI assistants**.

### Installing

The installers aren't signed yet, so your system warns you the first time:

- **Windows** (`.exe`): SmartScreen says "Windows protected your PC". Choose **More info**,
  then **Run anyway**. Latte installs for your user only, without admin rights.
- **macOS** (`.dmg`, Apple Silicon): drag Latte Books to Applications, then right-click it
  and choose **Open**. If macOS says the app is damaged, run
  `xattr -cr "/Applications/Latte Books.app"` in Terminal.
- **Linux**: install the `.deb`, or make the `.AppImage` executable and run it.

Check a download against `SHA256SUMS.txt`, or that it was built from this repository with
`gh attestation verify <file> --repo bloomsynth/latte-books`.

### Known issues

- Arabic and Persian are shown left to right.
- In Hindi and Gujarati, wrapped text can break inside a word.
- Exchange rates aren't fetched: set the rate on an invoice in another currency yourself.
- A GPU is required. Virtual machines and Remote Desktop are untested.
- Not ported: ERPNext sync.
