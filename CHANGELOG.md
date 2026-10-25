# Changelog

What changes in each release of Latte Books. A release's notes on GitHub are its section
here.

## [Unreleased]

- On Windows, the installer updates an older Latte Books in place, without asking whether
  to uninstall it first.

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
