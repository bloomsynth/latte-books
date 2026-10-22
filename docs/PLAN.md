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

### Phase 0 — Foundations and risk spikes (2–3 weeks)

- Repo, AGPL `LICENSE`, `NOTICE` crediting Frappe and contributors, README with origin and the
  "not affiliated" note.
- Cargo workspace skeleton and CI (fmt, clippy, tests) on Windows, macOS and Linux.
- Vendor schemas, fixtures, translations and templates from Books v0.36.0.
- **GPUI spikes. These decide whether GPUI stays:**
  1. Startup time to first usable window on a low-end Windows laptop.
     *First result (2026-10-21): 543 ms to first frame on a Ryzen 3700X + RTX 2060 desktop;
     low-end laptop still to test. See `spikes/gpui-startup/RESULTS.md`.*
  2. Runs on a Windows VM, Windows Remote Desktop, and a Linux VM (Vulkan).
  3. An **editable invoice item grid**: tab between cells, add and remove rows,
     autocomplete for items, a total that updates live.
     *Passed (2026-10-21): all four work, covered by 8 UI tests. With `uniform_list`, each edit
     reaches the screen in one 120 Hz frame even at 2,000 rows. See `spikes/invoice-grid/RESULTS.md`.*
  4. Text in Hindi, Gujarati, Chinese and Korean renders correctly.
- **Exit:** all four spikes pass, or we have a written plan B.

### Phase 1 — Data layer (3–4 weeks)

- `latte-schema`: load and validate all vendored schemas.
- `latte-db`: open a `.books.db`, check the schema version, generic get/list/insert/update/delete,
  child tables, naming series, single values, custom fields, and the change log.
- Money in and out of the `text` columns with no loss of precision.
- **Exit:** open a real Books file and print every document type with its child rows, read-only.

### Phase 2 — Accounting engine (5–7 weeks)

- Port `LedgerPosting` and the rules for accounts, journal entries, sales and purchase
  invoices, payments (incl. against invoices), taxes, multi-currency, round-off,
  submit and cancel.
- Reports: General Ledger, Trial Balance, P&L, Balance Sheet.
- Setup: create a new company from `countryInfo.json` and the charts of accounts.
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
- India: GSTIN fields, GST types, state codes, GSTR-1 and GSTR-2 reports.
- Switzerland regional fields.
- Point of Sale: profiles, shifts, checkout, barcodes.
- Pricing rules, price lists, coupons, loyalty programs, leads, quotes.
- Data import (CSV), custom fields and forms, the dashboard.
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

---

## 6. Release and distribution

- GitHub Releases on every tag, with `cargo-packager` artifacts for each OS.
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
  for display.
- No direct SQL writes from UI or regional code.

---

## 8. Before going public

- [ ] Trademark search for "Latte" in software (class 9) and accounting (class 36), in India and the US.
- [ ] Contact Frappe: introduce the project and ask for a link from the `frappe/books` README or #1566.
- [ ] Create the GitHub repo under Bloomsynth.
- [ ] Short legal review of the AGPL setup and the paid-services model.

---

## 9. Risks

| Risk | Impact | Mitigation |
|---|---|---|
| GPUI API changes break the build | Medium | Pin exact versions; upgrade on purpose about once a month; keep GPUI out of the core |
| GPUI fails on VMs, remote desktop or old Linux | Some users can't run it | Phase 0 spike; document requirements; Mesa lavapipe fallback on Linux |
| The editable grid is hard in GPUI | Invoice entry suffers | Phase 0 spike before committing |
| No RTL in GPUI | Arabic and Persian users stay on Frappe Books | Use start/end in layouts; track zed#31102 and #57237; contribute direction support upstream |
| Report numbers drift from Books | Users lose trust | Parity suite in CI |
| Users' files get damaged | Severe | Auto-backup before the first write; never change Books' tables before 1.0 |
| Print templates look different | Minor complaints | Port the three main templates carefully; let users edit them in Typst |
| One maintainer | Slow progress | Public roadmap; "good first issue" labels; welcome former Books contributors |

---

## 10. Next steps

1. Create the Cargo workspace, LICENSE, NOTICE and README (Phase 0).
2. Vendor schemas, fixtures, translations and templates from Books v0.36.0.
3. Start the GPUI startup and editable-grid spikes.
