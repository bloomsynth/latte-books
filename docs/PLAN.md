# Latte Books — Project Plan

Latte Books is a fast, native Rust port of the Frappe Books desktop app. It is for people who
want to keep doing their accounting on their own computer now that Frappe has ended the
Electron app ([frappe/books#1566](https://github.com/frappe/books/issues/1566)).

- **Status:** Phase 0 (started 2026-10-21)
- **Owner:** Bloomsynth
- **License:** AGPL-3.0, the same as Frappe Books
- **Not affiliated with Frappe Technologies.**

---

## 1. Goals

1. **Opens existing Frappe Books files.** A user points Latte Books at their `.books.db` and
   carries on, with no export or import step.
2. **Opens fast and feels native.** The window appears in well under a second, even on
   modest hardware.
3. **Gives the same numbers.** For the same data, every ledger entry and report matches
   Frappe Books exactly.
4. **Is an open, credible continuation.** It is a public AGPL repo that credits Frappe and the
   original contributors, keeps their translations, and states the project's origin honestly.
5. **Leaves room for paid services.** Sync, backup, bank feeds and e-invoicing can be added
   later as separate server-side services, without reworking the app.

### Not goals (for now)

- A hosted web version of the app.
- Arabic and Persian (right-to-left) layout. GPUI doesn't support it yet; see §9.
- Screen-reader accessibility beyond what GPUI provides. The old app had very little.
- The ERPNext sync integration (`ERPNextSyncQueue` and related).

---

## 2. Key decisions

| Area | Decision | Why |
|---|---|---|
| Approach | **Port** Frappe Books to Rust: reuse its schemas, data files and accounting logic | Keeps users' trust, translations and proven rules. Paid features live on servers, so owning every line isn't needed. |
| Language | Rust (stable), one Cargo workspace | Fast start, one binary, safe concurrency |
| UI | **GPUI + gpui-kit**, exact versions pinned | Native GPU rendering, fast and polished. gpui-kit provides tables, inputs and dialogs. |
| Database | SQLite through `rusqlite` (bundled) | Same engine and file as Frappe Books |
| Money | `rust_decimal`, read from and written to the existing `text` columns | Books stores Currency as text (via `pesa`); never use floats |
| Dates | `chrono` / `time` | — |
| Printing | **Typst** as a library, with Books' HTML print templates ported to Typst | Native PDF output with no browser engine; templates stay editable as text |
| Translations | Load Books' existing `translations/*.csv` as they are | 18 languages of volunteer work kept |
| Packaging | `cargo-packager` + its updater | MSI/NSIS, DMG, AppImage/deb; Flathub and Homebrew manifests by hand |
| Name | "Latte Books"; "Frappe" never appears in the product name, logo or domain | Trademark policy; "a port of Frappe Books" in descriptions is fine |

### Porting principle: stay as close to the original as possible

- **Same names:** doctypes, field names, report names, settings and menu labels match Books v0.36.0,
  so users, docs and translations all carry over.
- **Same behaviour, including quirks:** rounding, naming series, validation messages, and posting order.
  If Books does something odd but users rely on it, we keep it.
- **Same structure where Rust allows:** each Books model or report maps to a Rust module
  with a matching name, and each ported file starts with a comment linking to its source file in
  Books (for example `// Port of models/baseModels/Payment/Payment.ts @ v0.36.0`). That makes
  review and future fixes easy.
- **Every deviation is written down** in `docs/DEVIATIONS.md` with the reason
  (for example: HTML print templates → Typst; ERPNext sync not ported).
- Improvements wait until after 1.0. Until then, parity comes first.

---

## 3. Architecture

```
latte-books/
├─ crates/
│  ├─ latte-schema/   Loads Books' JSON schemas (vendored); field types, links, child tables
│  ├─ latte-db/       SQLite access, schema-driven CRUD, patches/migrations, change log
│  ├─ latte-core/     Models and business rules: posting, invoices, payments, taxes,
│  │                  numbering, inventory, reports. NO UI types here.
│  ├─ latte-i18n/     CSV translation loader, number/date/currency formatting
│  ├─ latte-print/    Typst rendering of invoices, payments and shipments
│  ├─ latte-regional/ Country modules: `in` (GST, GSTR-1/2), `ch`, …
│  ├─ latte-ui/       GPUI screens and components (depends on gpui-kit)
│  └─ latte-app/      main(): startup, window, menus, file open, updater
├─ vendor/books/      Files taken from Frappe Books (schemas, fixtures, translations,
│                     templates), with their original copyright notices
├─ tools/parity/      Scripts comparing Latte Books output with Frappe Books
└─ docs/
```

**Rules:**
- `latte-core` and everything below it have no GPUI dependency. A GPUI upgrade can only break `latte-ui`.
- **Schema-driven, as Books is.** Tables and fields come from the vendored JSON schemas, not
  hand-written Rust structs for all 70 doctypes. Typed Rust wrappers exist only where business
  logic needs them (Invoice, Payment, JournalEntry, StockMovement…).
- **Every write goes through one place in `latte-db`** and is recorded in a change log
  (a `latte_changes` table). It costs little now, and it's what sync will be built on (§7).

### Compatibility rule

- Latte Books uses the **same schema version** as the last Frappe Books release (v0.36.0).
  During beta, a file stays usable in both apps.
- v0.36.0 (2025-11-06) is the **last published release**, so that's what users' files come from.
  The `master` branch is 123 commits ahead and says 0.37.0, but that version was never released.
  It has unreleased schema changes (`BatchSeries`, `SerialNumberSeries`, and edits to
  `Item`, `InvoiceItem`, `SalesInvoice`, `LoyaltyProgram`, `POSSettings`). Vendor from the
  **`v0.36.0` tag**, not `master`. Pick up useful unreleased features later, as our own patches.
- Latte-only tables use a `latte_` prefix. Latte Books never changes Books' own tables before 1.0.
- After 1.0, any schema change ships as a patch, like Books' `backend/patches`, and the docs
  say clearly which version stops being readable by Frappe Books.

---

## 4. Parity testing (how we know the numbers are right)

Frappe Books is the **reference** for correct results:

1. Use Books' own dummy-data generator (`dummy/`) to create sample companies across several
   countries and fiscal years.
2. Run Frappe Books' reports on them (headless, through its Node tests) and save the results as JSON.
3. Run Latte Books on the same files and compare: General Ledger, Trial Balance, P&L,
   Balance Sheet, Stock Ledger, Stock Balance, GSTR-1/2.
4. Port Books' existing specs (23 `*.spec.ts` files, mostly under `models/**/tests/`) to Rust tests:
   invoices, payments, pricing rules, coupons, loyalty, batches, serial numbers, stock queue.
5. Volunteers can also check against their **real company files**, anonymised.

CI runs the parity suite on every PR. Any difference fails the build.

---

## 5. Phases

### Phase 0 — Foundations and risk spikes (2–3 weeks) — done 2026-10-22

- Repo, AGPL `LICENSE`, `NOTICE` crediting Frappe and contributors, README with origin and the
  "not affiliated" note.
- Cargo workspace skeleton and CI (fmt, clippy, tests) on Windows, macOS and Linux.
- Vendor schemas, fixtures, translations and templates from Books v0.36.0.
- **GPUI spikes. These decide whether GPUI stays:**
  1. Startup time to first usable window on a low-end Windows laptop.
     *First result (2026-10-21): 543 ms to first frame on a Ryzen 3700X + RTX 2060 desktop;
     the low-end laptop run is skipped (2026-10-22): performance problems on slow machines will
     come in as issue reports. See `spikes/gpui-startup/RESULTS.md`.*
  2. Runs on a Windows VM, Windows Remote Desktop, and a Linux VM (Vulkan).
     *Skipped (2026-10-22): few Books users run it in a VM or over Remote Desktop. We document
     that a GPU is required and act on user reports. Linux is still tried on real hardware
     before release.*
  3. An **editable invoice item grid**: tab between cells, add and remove rows,
     autocomplete for items, a total that updates live.
     *Passed (2026-10-21): all four work. With `uniform_list`, each edit reaches the screen in
     one 120 Hz frame even at 2,000 rows. Hand-tested (2026-10-22): three mouse bugs found and
     fixed; now covered by 11 UI tests. See `spikes/invoice-grid/RESULTS.md`.*
  4. Text in Hindi, Gujarati, Chinese and Korean renders correctly.
     *Passed with two known issues (2026-10-22): shaping matches Chromium and input-method
     composition works. On stock GPUI, wrapped Hindi and Gujarati text can break mid-word, and
     the cursor can stop inside a conjunct. We don't patch GPUI; both are listed as known
     issues and to be reported upstream. See `spikes/indic-cjk-text/RESULTS.md`.*
- **Exit:** spikes 1, 3 and 4 pass (spike 2 skipped), or we have a written plan B.
  *Met (2026-10-22): GPUI stays. The CI workflow is written but hasn't run yet, as there is no
  GitHub repo.*

### Phase 1 — Data layer (3–4 weeks)

- `latte-schema`: load and validate all vendored schemas. *Done 2026-10-22: a port of
  `schemas/index.ts` that combines regional, abstract, meta and custom fields as Books does,
  with the schema files embedded in the binary. 17 tests check it against every vendored
  file. Not yet compared with the output of Books' own `getSchemas`.*
- `latte-db`: open a `.books.db`, check the schema version, generic get/list/insert/update/delete,
  child tables, naming series, single values, custom fields, and the change log.
  *Reading done 2026-10-22: ports of the read half of `backend/database/core.ts` (get, getAll
  with Books' filters, single values, exists) and of `fyo/core/converter.ts` to document
  values. Files must come from Books v0.36.0, which brings their tables up to date.
  Writing done 2026-10-22: insert, update, delete, deleteAll and rename with child tables
  and single values, `#createTable`, and the converter back to database values. Each write
  runs in one transaction (Books runs each statement on its own) and logs every row it
  touches in `latte_changes`. Naming series move to Phase 2: in Books they are part of
  `Doc.sync`, not the database. Migrating a file isn't ported yet.*
- Money in and out of the `text` columns with no loss of precision. *Reading done: pesa's
  parsing, `store` and rounding are ported (`latte_db::pesa`); its arithmetic comes with
  Phase 2.*
- **Exit:** open a real Books file and print every document type with its child rows, read-only.
  *Read-only part met 2026-10-22: `cargo run -p latte-db --example dump` reads all 564
  documents in a real company file with no errors. Writing met 2026-10-22: the
  `write_test` example added a party, renamed it, added and edited a journal entry and
  changed a setting in a copy of a real file. Books v0.36.0 opened the copy, showed the
  documents, and edited and saved the journal entry. Latte then read Books' edit back.
  Books' own edits aren't in `latte_changes`, and Books signs documents with the
  signed-in user (an email), not `Admin`.*

### Phase 2 — Accounting engine (5–7 weeks)

- Port `LedgerPosting` and the rules for accounts, journal entries, sales and purchase
  invoices, payments (incl. against invoices), taxes, multi-currency, round-off,
  submit and cancel.
  *`Doc` done 2026-10-22 (`latte_core::Doc` and `Fyo`): defaults, child rows, `set`,
  formulas, validation, meta values, naming and Number Series, load, sync, submit, cancel,
  delete, rename and duplicate, with the NumberSeries and SystemSettings models. pesa's
  arithmetic is ported for it. The deviations are listed at the top of each file; the main
  ones are no document cache or observers, no links, and no CustomForm or CustomField
  models yet. Books' `Doc` doesn't check `minvalue` or `maxvalue`, so neither does this.*
  *Journal entries post 2026-10-22 (`latte_core::models`): `Transactional`,
  `LedgerPosting` (with round-off) and the Account, AccountingLedgerEntry, JournalEntry and
  JournalEntryAccount models. Submitting posts the ledger entries, cancelling reverts
  them, deleting removes them, and an entry that doesn't balance is refused. Checked
  against Books 2026-10-22: the `post_test` example submitted one journal entry and
  cancelled another in a copy of a real file, and Books v0.36.0 did the same by hand. The
  ledger entries match field for field: accounts, amounts, the midnight-UTC date, the
  null party, the reverted flags, which entry each reversal reverts and the order they are
  written in. The only difference is `createdBy`/`modifiedBy` (`Admin` against the
  signed-in user). Latte reads Books' entries back with no errors.*
  *Invoices post 2026-10-23: the Item, Party, SalesInvoice, PurchaseInvoice, their item
  rows and TaxSummary models. Rows total with item and invoice discounts, taxes come from
  the tax template, foreign currency invoices post in the company currency, and what
  doesn't balance goes to the round-off account. Submitting posts and raises what the
  party owes; cancelling reverts both. Pricing rules, coupons, loyalty points, returns,
  payments and stock transfers aren't ported and are refused where they would change the
  result. Five Books bugs in the row formulas are fixed (see DEVIATIONS.md). Checked
  against Books 2026-10-23: the `invoice_test` example submitted two sales invoices in a
  copy of a real file, one with a 10% row discount and one in EUR at a rate of 90, and
  Books v0.36.0 did the same by hand. The discounted invoice matches field for field:
  invoice, row, taxes, ledger entries and what the party owes, apart from
  `createdBy`/`modifiedBy`. The EUR invoice matched once the rate was the same: Books'
  rate box shows the rate to two decimals and writes that back when you leave it, even
  unchanged, so its row rate was 13.33 against Latte's 13.3333…. The sums from either
  rate agree. That rewrite is a Books UI bug for Phase 3 not to copy.*
  *Payments post 2026-10-23: the Payment and PaymentFor models. A payment takes its
  party, type, accounts and amount from the invoices it pays; submitting posts it and
  lowers what the invoices and the party owe, cancelling puts it back, and cancelling or
  deleting an invoice does the same to its payments. An invoice set to make its payment on
  submit does so. Write-offs are refused until their postings are settled. Four Books bugs
  are fixed (see BOOKS-BUGS.md); two of them were seen in Books. Checked against Books
  2026-10-23: the `payment_test` example paid the two invoices from the invoice check in
  a copy of a real file, and Books v0.36.0 paid its own two by Make Payment. PAY-1014
  and Books' PAY-1016 match field for field: payment, row, ledger entries and what the
  invoice and party owe, apart from `createdBy`/`modifiedBy` and the amount, which is
  each invoice's outstanding (they differ by the Books rate box bug above). Books' other
  payment, PAY-1015, was lost to the failed-cancel bug; its ledger entries, which Books
  left behind, match Latte's PAY-1013.*
  *Write-offs post 2026-10-23, corrected: the cash or bank account takes the amount less
  the write-off, the write-off account the rest (a loss when receiving, a gain when
  paying), and the party's account the whole amount. The amount alone must cover the
  invoices, a write-off doesn't excuse a partial payment, and it can't be negative or
  more than the amount. Checked against Books 2026-10-23: Books paid SINV-1023 (₹1,416)
  with ₹4 written off as PAY-1017, and the `payment_test` example paid SINV-1022 (₹1,180)
  the same way as PAY-1018. The payments match field for field apart from
  `createdBy`/`modifiedBy` and the amounts, which follow each invoice. The postings differ
  as intended: Books posted Cash ₹1,416, Debtors ₹1,416 credit and ₹4 debit, and Write
  Off ₹4 credit, leaving the customer owing ₹4 in the ledger while the invoice and party
  say ₹0; Latte posted Cash ₹1,176, Write Off ₹4 debit and Debtors ₹1,180 credit.*
  *Returns 2026-10-23: Make Return on a sales or purchase invoice makes a return of what
  is left to return; submitting it reverses the invoice's posting, takes it off what the
  party owes and marks the invoice returned, and a return can't be cancelled while its
  invoice can't be either. Make Payment on a return refunds it. Returns of invoices with
  coupons or loyalty points are refused. Seven Books bugs are fixed (see BOOKS-BUGS.md),
  among them a return posting the invoice again instead of reversing it. The
  `return_test` example makes a return in a copy of a file to compare with Books.*
- Reports: General Ledger, Trial Balance, P&L, Balance Sheet.
  *Reports 2026-10-23: the four reports are ported in `latte-core::reports`, with their
  filters, columns, rows and CSV export, and amounts and dates shown in the company's
  locale and format (`latte-i18n::format`). Nine Books bugs are fixed (see
  BOOKS-BUGS.md), among them the Trial Balance's closing columns and the To Date left out
  of the General Ledger. The `report_test` example prints a report of a file, opened
  read-only, to compare with Books.*
- Setup: create a new company from `countryInfo.json` and the charts of accounts.
  *Setup 2026-10-23: `latte_db::Database::create` makes a new file as Books does (every
  table, the settings' defaults, and the effects and log of Books' patches), and
  `latte_core::setup` sets the company up as the setup wizard does: settings, currencies,
  the chart of accounts with the bank, discount and default accounts, India's GST taxes,
  units, the Stores location and the number series. Checked against Books 2026-10-23: the
  `setup_test` example set up an Indian company, and its tables match a file Books
  v0.36.0 set up, row for row (accounts, taxes and rates, units, payment methods, patch
  log, number series, settings), apart from what that file's owner changed since. Every
  chart of accounts sets up without errors.*
- **Exit:** parity suite passes for every core document and report.

### Phase 3 — MVP desktop app (6–8 weeks)

- Shell: sidebar, list views, search, file open and recent files, settings.
- Forms: Party, Item, Sales/Purchase Invoice, Payment, Journal Entry, Chart of Accounts tree.
- Report viewer with filters and CSV export.
- Printing: port the `Basic`, `Business` and `Minimal` templates to Typst, then PDF and print.
- Translations: language switcher with the existing CSVs.
- Packaging: signed installers for Windows and macOS, AppImage/deb; auto-update.
- **Exit: public beta.** An existing Books user can open their file and do daily invoicing,
  payments and reports.

### Phase 4 — Feature parity (8–12 weeks)

- Inventory: stock movements, transfers, receipts, shipments, batches, serial numbers,
  FIFO stock queue, UOM conversions, Stock Ledger and Stock Balance.
  *Inventory 2026-10-23: InventorySettings, StockMovement, Shipment, PurchaseReceipt and
  their rows are ported in `latte_core::models::inventory`, with the stock queue, batches,
  serial numbers and unit conversions. Submitting moves stock and posts to Stock In Hand,
  Cost of Goods Sold and Stock Received But Not Billed; an invoice set to do so makes its
  shipment or receipt on submit, and the form offers Shipment or Purchase Receipt on an
  invoice, and Sales or Purchase Invoice and Return on a shipment or receipt. The Stock
  Ledger and Stock Balance reports are in `latte_core::reports`. Eight Books bugs are
  fixed (see BOOKS-BUGS.md). The barcode scanner and the inventory list filters aren't
  ported yet. Not yet checked against Books by hand.*
- India: GSTIN fields, GST types, state codes, GSTR-1 and GSTR-2 reports.
- Switzerland regional fields.
- Point of Sale: profiles, shifts, checkout, barcodes.
- Pricing rules, price lists, coupons, loyalty programs, leads, quotes.
  *Regional, pricing and sales 2026-10-23: India's Address and Party models, and the
  GSTR-1 and GSTR-2 reports with their JSON export, are in `latte_core`; Switzerland's
  fields come with the schemas. Pricing rules, price lists, coupons, loyalty programs,
  leads and quotes are ported, with Make Customer and Make Sales Quote on a lead and
  Make Sales Invoice on a quote. Eleven Books bugs are fixed (see BOOKS-BUGS.md). Loyalty
  points on returns are still refused. Not yet checked against Books by hand.*
  *Point of sale 2026-10-23: POSSettings and the shift models are ported, with
  `latte_core::pos` for opening and closing shifts, the cart's checks, and making the
  invoice, shipment and payment of a sale. The page has the Classic UI's item list,
  cart, payment and shift panels, barcode and weighed-item search, and saved sales taken
  up later. Seven Books bugs are fixed. The Modern UI, and returns, coupons and loyalty at
  the till, come after 1.0. Not yet checked against Books by hand.*
- Data import (CSV), custom fields and forms, the dashboard.
  *Import, custom fields and dashboard 2026-10-23: `latte_core::importer` and the Import
  Wizard page; CustomForm and CustomField, with a new field's column added when the file
  is opened; `latte_core::dashboard` and the Dashboard page with plain bar charts. Four
  Books bugs are fixed. Not yet checked against Books by hand. 2026-10-25: Books' charts,
  an area chart for cashflow, bars around zero for profit and loss, and a donut for the
  top expenses.*
- The Get Started page. *2026-10-23: `latte_core::views::get_started` and the page; the
  desk opens on it until every card is done or it is hidden. One Books bug is fixed. Not
  yet checked against Books by hand.*
- Create Demo. *2026-10-23: `latte_core::dummy` with Books' items, parties and logo, and
  the button in the company list. Three Books bugs are fixed.*
- GST: check one GSTR-1 JSON export, B2B rows included, with the GST portal's offline
  tool before 1.0.
- **Exit: 1.0.** Parity suite is green for everything Books v0.36.0 does, except the non-goals in §1.

### Phase 5 — Latte Cloud (after 1.0)

Paid, optional services. Each one is a **separate private server**. The app only contains a
small open-source client for it (AGPL).

1. **Cloud backup:** encrypted, scheduled uploads of the company file. Simplest, so it goes first.
2. **Sync across devices:** built on the `latte_changes` log. Start single-user, multi-device;
   multi-user editing comes later.
3. **Bank feeds:** import bank statements and match them to payments.
4. **E-invoicing and GST filing (India first):** generate IRN/e-invoices and file GSTR returns
   through our server.

Accounts, billing and the servers are a separate project. They are not in this repo.

### Dime — the AI layer (after the app is built)

Dime lets you work in Latte Books from Claude, ChatGPT or any other AI agent: "what does
Acme owe?", "invoice Acme for 10 chairs", "record their payment". The app keeps the name
Latte Books; Dime is the name of this layer (dimehq.ai, Bloomsynth's earlier AI assistant
for Tally).

- **An MCP server over `latte-core`**, shipped with the app and open source (AGPL). It
  opens the company file and offers tools to look up documents, run reports, and create,
  submit and cancel documents. You bring your own agent, so Latte pays for no models.
- **Set up in one click, like DaVinci Resolve 21.1.** Resolve's File > Setup AI Assistants
  writes an entry for its bundled `ResolveMCP.exe` into each assistant's own config
  (`~/.claude.json` for Claude Code, `~/.codex/config.toml` for Codex), and that's all the
  user does. Latte does the same:
  - The installer puts a small `latte-mcp` executable next to the app. It's built on
    `latte-core` with no UI.
  - A "Set up AI assistants" screen lists the assistants found on the computer, such as
    Claude Desktop, Claude Code and Codex, each with Connect and Disconnect. Connect adds
    the entry; Disconnect removes only Latte's entry.
  - The words "MCP", "server" and "config" don't appear in the app.
  - It works with Latte closed. It uses the company the app last opened (or the one chosen
    on that screen), and tells the assistant which company that is. Changes an assistant
    makes while the app is open show up in the app.
  - Updates and moving the app keep working: the installer and updater rewrite the entries
    that point at `latte-mcp`.
- **No shortcuts:** every write goes through the same `Doc` sync, submit and cancel
  checks as the app. An agent can't save what a person couldn't.
- **You stay in control:** writes wait for your confirmation, and drafts come before
  submitting.
- *Built 2026-10-23: the `latte-mcp` crate has the server (company info, document types
  and fields, find, get, create, update, submit, cancel and delete documents, and the
  reports) and the assistant setup for Claude Desktop, Claude Code, Codex and Cursor; the
  app's "Set up AI assistants" page connects and disconnects them, and the app points
  connected assistants at its own `latte-mcp` each time it starts. Writes are confirmed by
  the assistant's own tool approval. An assistant's writes are logged under their own
  device id, and the app shows the open page again when it sees them (see DEVIATIONS.md).
  Not yet tried with a real assistant.*
- Later: a "Changes by assistants" list in the app, and setup for VS Code (Copilot) and
  Windsurf.
- Next: an invoice in a foreign currency made by an assistant gets exchange rate 1 (Latte
  doesn't fetch rates yet, see DEVIATIONS.md), so a ₹30 item becomes €30 with no warning.
  `latte-mcp` should refuse or warn when no rate is given, and the app should be able to
  fetch today's rate as Books does.
- **Starts after Phase 3,** once the app can show what an agent did. Hosted features
  (reading receipts and invoices into entries, an assistant built into the app) are paid
  Latte Cloud services, on Bloomsynth's servers like those above.

---

## 6. Release and distribution

- GitHub Releases on every tag, with `cargo-packager` artifacts for each OS.
  *2026-10-23: the packager settings are in `crates/latte-app/Cargo.toml` (NSIS, DMG,
  deb and AppImage, with `latte-mcp` beside the app), and `.github/workflows/release.yml`
  builds them on a `v*` tag into a draft release. The app icon, a mug that is also a book,
  is in `crates/latte-app/icons` and is built into the Windows `.exe`. Unsigned. Windows
  installs for the current user. Not yet run.*
  *2026-10-24: ready for the first beta, `0.1.0-beta.1`. The workflow checks the tag
  against `Cargo.toml`, takes the notes from `CHANGELOG.md`, adds `SHA256SUMS.txt` and
  build attestations, and names the installers the same in every release for the
  README's download buttons. It never marks a release as a pre-release, since
  `releases/latest` skips those. Run by hand, it builds without releasing. Both `.exe`
  files carry a version resource (product name "Latte Books"), which SignPath requires.
  The macOS build is Apple silicon only.*
  *2026-10-24: `0.1.0-beta.1` released. Next, Windows signing: apply to SignPath
  Foundation, which only signs projects that are already released.*
- Windows: Azure Trusted Signing (or an OV certificate). macOS: Developer ID + notarization.
- Flathub manifest and Homebrew cask, maintained in their own repos.
- Website: `latte.bloomsynth.com` (subdomain, no new domain). Pages include "Coming from
  Frappe Books?" and "Open your Frappe Books file".

---

## 7. Sync-readiness (decide now, build later)

So Phase 5 doesn't need a rework:
- Every document write goes through `latte-db`, which records table, name, operation, the
  changed fields, a timestamp and a device ID in `latte_changes`.
- New documents get an ID that's unique across devices, while Books' naming series stays
  for display. *Open: in Books `name` is both the primary key and the series name
  (`SINV-1001`), so this ID needs a home outside Books' columns. Until then
  `latte_changes` records rows by `name`.*
- No direct SQL writes from UI or regional code.

---

## 8. Before going public

- [ ] Trademark search for "Latte" in software (class 9) and accounting (class 36), in India and the US.
  *Web search (2026-10-22) found no accounting or finance software named Latte. Only "LATTE
  SMART" (coffee-machine software, US, dead) came up. The official registers (USPTO, IP India)
  are still to be searched.*
- [ ] Contact Frappe: introduce the project and ask for a link from the `frappe/books` README or #1566.
  *`frappe/books` is archived (read-only), so contact goes through `frappe/frappe-books`, the
  forum or email.*
- [x] Create the GitHub repo under Bloomsynth (public, 2026-10-22).
- [ ] Short legal review of the AGPL setup and the paid-services model.

---

## 9. Risks

| Risk | Impact | Mitigation |
|---|---|---|
| GPUI API changes break the build | Medium | Pin exact versions; upgrade on purpose about once a month; keep GPUI out of the core |
| GPUI fails on VMs, remote desktop or old Linux | Some users can't run it | Spike skipped as uncommon; document requirements; act on user reports; Mesa lavapipe fallback on Linux |
| The editable grid is hard in GPUI | Invoice entry suffers | Phase 0 spike before committing |
| No RTL in GPUI | Arabic and Persian users stay on Frappe Books | Use start/end in layouts; track zed#31102 and #57237; contribute direction support upstream |
| Report numbers drift from Books | Users lose trust | Parity suite in CI |
| Users' files get damaged | Severe | Auto-backup before the first write; never change Books' tables before 1.0 |
| Print templates look different | Minor complaints | Port the three main templates carefully; let users edit them in Typst |
| One maintainer | Slow progress | Public roadmap; "good first issue" labels; welcome former Books contributors |

---

## 10. Next steps

1. ~~Create the public GitHub repo.~~ Done: github.com/bloomsynth/latte-books.
2. Report the two text issues from spike 4 upstream: line wrapping to Zed, the cursor to
   gpui-kit.
3. Phase 1 is done. In Phase 2 the `Doc` layer is ported, and journal entries and sales
   and purchase invoices, payments and returns post to the ledger, matching what Books
   posts. The four accounting reports and company setup are ported, so Phase 2 is done.
   Next: Phase 3, the app.
