# Latte Books

**A fast, native desktop accounting app for Frappe Books users.**

Frappe has retired the Frappe Books desktop app
([frappe/books#1566](https://github.com/frappe/books/issues/1566)) and replaced it with a new
Frappe Books built on the Frappe Framework, used from a browser. Latte Books keeps the desktop
app alive: it is a Rust port of Frappe Books v0.36.0 that works on your own computer, with
your company in a single file.

> **Status: early development. Not usable yet.** Latte can read and write Frappe Books files,
> and the accounting engine is under way: journal entries and sales and purchase invoices
> post to the ledger, and what each party owes is kept up to date. There is no
> app to use yet. Follow along in [docs/PLAN.md](docs/PLAN.md).

Latte Books is **not affiliated with Frappe Technologies**. It is built on their work; see
[NOTICE](NOTICE).

## What it will do

- **Open your existing Frappe Books file.** Point it at your `.books.db` and carry on. No
  export, no import.
- **Give the same numbers.** The same documents, fields, reports and rules as Frappe Books
  v0.36.0. Every ledger entry and report is checked against Frappe Books itself.
- **Speak your language.** All 18 of Frappe Books' community translations, kept as they are.
- **Feel native and fast.** Written in Rust with [GPUI](https://www.gpui.rs/), the UI framework
  behind the Zed editor. No Electron, no browser engine.
- **Stay free and open source** under the AGPL-3.0, like Frappe Books.

Changes from Frappe Books are kept to a minimum, and every one is listed in
[docs/DEVIATIONS.md](docs/DEVIATIONS.md).

## Where it stands

| Phase | What | Status |
|---|---|---|
| 0 | Foundations, and prototypes to test GPUI | **Done** |
| 1 | Data layer: read and write `.books.db` files | **Done** |
| 2 | Accounting engine and reports, checked against Frappe Books | In progress |
| 3 | Desktop app: invoices, payments, reports, printing. **Public beta.** | Planned |
| 4 | Everything else in Frappe Books v0.36.0: inventory, GST, POS and more. **1.0.** | Planned |

What the Phase 0 prototypes showed:

- **Startup:** the first window appears in about half a second
  ([results](spikes/gpui-startup/RESULTS.md)).
- **Invoice item grid:** typing, Tab between cells, item autocomplete and live totals. Every
  edit reaches the screen within one frame, even with 2,000 rows
  ([results](spikes/invoice-grid/RESULTS.md)).
- **Hindi, Gujarati, Chinese and Korean:** text renders correctly and input methods work. There
  are two known issues with Hindi and Gujarati, listed below
  ([results](spikes/indic-cjk-text/RESULTS.md)).

## Known issues

- **Arabic and Persian** are shown left to right. GPUI doesn't support right-to-left layout yet.
- **Hindi and Gujarati:** text that wraps onto several lines can break in the middle of a word,
  and in text fields the arrow keys can stop inside a conjunct. Both come from GPUI and
  gpui-kit, and will be reported there.
- **A GPU is required.** Running in a virtual machine or over Remote Desktop is untested.

## Building

You need [Rust](https://rustup.rs/) (stable).

```sh
cargo build
cargo test
```

The app itself doesn't do anything yet. To see the data layer read a company file (it
opens the file read-only):

```sh
cargo run -p latte-db --example dump -- "path/to/company.books.db"
```

To try the prototypes:

```sh
cd spikes
cargo run --release -p invoice-grid
cargo run --release -p indic-cjk-text
```

### Layout

| Path | What it holds |
|---|---|
| `crates/latte-schema` | Loads Frappe Books' JSON schemas |
| `crates/latte-db` | SQLite access to `.books.db` files |
| `crates/latte-core` | Accounting rules and reports (no UI) |
| `crates/latte-i18n` | Translations and formatting |
| `crates/latte-print` | PDF printing with Typst |
| `crates/latte-regional` | Country modules (India GST, Switzerland) |
| `crates/latte-ui` | GPUI screens |
| `crates/latte-app` | The `latte-books` executable |
| `vendor/books` | Files copied unchanged from Frappe Books v0.36.0 |
| `spikes` | Phase 0 prototypes and their results |
| `docs` | The plan and the list of deviations from Frappe Books |

## Feedback

Frappe Books users are especially welcome. If you rely on the desktop app, open an
[issue](https://github.com/bloomsynth/latte-books/issues) and say what you use it for, which
country and language, and what you'd miss most. That shapes what gets built first.

## License

AGPL-3.0, the same as Frappe Books. See [LICENSE](LICENSE) and [NOTICE](NOTICE).

Latte Books is built on Frappe Books by Frappe Technologies and its contributors, including
the volunteers who translated it into 18 languages.
