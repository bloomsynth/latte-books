# Changelog

What changes in each release of Latte Books. A release's notes on GitHub are its section
here.

## [Unreleased]

- **Updates:** Latte Books checks for a new version when it starts and once a day. A
  newer one shows at the foot of the sidebar, with its notes, and **Update and Restart**
  installs it and opens the app again. Each update is checked against Latte Books' signing
  key before it's installed. Turn checking off, or check now, in Settings → System. The
  .deb links to the release instead, and versions up to 0.1.0-beta.3 need this one
  installed by hand.
- AI Assistants lists **ChatGPT Codex**: ChatGPT and Codex share one setup, so Connect
  makes Latte Books available in the ChatGPT desktop app's Codex and Work chats as well as
  Codex. It shows as found when either is installed.
- GSTR-1's JSON export is the whole return for the period, as the GST portal's offline
  tool (V3.2.4) reads it: B2B, B2C-Large and B2C-Small, credit notes, nil rated and
  exempt supplies, the HSN summary (Table 12) and the documents issued (Table 13).
- GSTR-1 includes the invoices of the To Date. Frappe Books leaves them out,
  so a quarter's return was missing its last day.
- B2C-Large uses the ₹1 lakh limit in force since August 2024.
- In the JSON, an invoice has one item per rate, numbered 1, 2 and so on; the place of
  supply is the customer's; returns are credit notes; and B2C-Small is summed by place
  and rate. Each of these was wrong in Frappe Books.
- The export stops and names the row when a row has no GST tax or no HSN/SAC code.
- Settings → System and the foot of the sidebar show Latte Books' version. The Frappe
  Books version the company file is kept at is under Compatibility.
- Change the company's name in Settings → General. The sidebar, window title and recent
  files follow it.
- The Show Sidebar button is always shown, and both sidebar buttons name Shift+H.
- GSTR-1 picks a **Financial Year and a month**, or a **quarter** for companies
  in the QRMP scheme, as the GST portal does, starting at the latest period that has
  ended. Custom brings back From Date and To Date.
- GSTR-1 shows when the return is due: the 11th of the next month, or the 13th after a
  quarter. Its exports are named by the GSTIN and the period, such as
  `GSTR1_29AABCA1234A1Z5_092026.json`.
- Settings has a **GST** tab for Indian companies: the GSTIN, Return Filing (Monthly or
  Quarterly) and a link to the Tax Templates.
- **Settings** is at the foot of the sidebar, and Ctrl+, (⌘,) opens it. The Setup group
  is gone: Chart of Accounts and Tax Templates are in Common, and Import Wizard, Print
  Templates, Customize Form and AI Assistants are a new Tools group.
- New Indian companies get GST-40 and IGST-40 tax templates, for GST 2.0's 40% rate.
- AI assistants see GSTR-1 only for companies with a GSTIN.
- GSTR-2 is removed. The return hasn't been filed since 2017; input tax credit is
  claimed from the GSTR-2B the GST portal makes.
- A report wider than the window scrolls sideways to its other columns: drag the
  scrollbar under the rows, use Shift and the wheel, or swipe on a trackpad. The wheel
  alone scrolls sideways too when the rows all fit. They were cut off before.
- Printed reports keep each GSTIN and amount whole: a column is never narrower than its
  longest word, and ₹ stays on the amount's line. GSTR's "Intergrated Tax" column is
  now "Integrated Tax".
- **Reverse charge** for Indian companies: purchases on which you pay the GST, such as
  software and cloud services bought from abroad. Give the item the IGST-18-RCM tax
  template (GST-18-RCM within your state, and 5% templates for goods transport). The
  invoice is for what the supplier bills; the GST is posted as owed on IGST (RCM) and
  claimed as input tax credit on IGST. New Indian companies have the templates; for an
  existing one, Settings → GST → **Set Up Reverse Charge** adds them.
- A reverse charge purchase invoice prints as its **Self Invoice (Reverse Charge)**, in
  rupees, from the print view's template list.
- **GSTR-3B**, under GST: the period's figures to enter on the portal, from the sales
  and purchase invoices: outward supplies (3.1), reverse charge purchases (3.1(d)),
  input tax credit (4(A)), exempt purchases (5) and the reverse charge to pay in cash
  (6.1), with the return's due date. Rows with no GST tax template are listed below,
  to put right.
- GSTR-1's Reverse Chrg. column is N. Frappe Books shows Y for every customer without
  a GSTIN.
- A tax row that is taken off, such as a reverse charge's, comes off the invoice's total
  and posts on the other side of the ledger. Frappe Books adds it to the total.
- An AI assistant can give a number for a text field, such as an item's HSN/SAC code.

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
