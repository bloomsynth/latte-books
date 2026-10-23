<img src="crates/latte-app/icons/icon.png" alt="Latte Books icon: a brown mug shaped like a book, with steam, on a caramel tile" width="96">

# Latte Books

**A fast, native desktop accounting app for Frappe Books users.**

Frappe has retired the Frappe Books desktop app
([frappe/books#1566](https://github.com/frappe/books/issues/1566)) and replaced it with a new
Frappe Books built on the Frappe Framework, used from a browser. Latte Books keeps the desktop
app alive: it is a Rust port of Frappe Books v0.36.0 that works on your own computer, with
your company in a single file.

> **Status: early development. Not usable yet.** Latte can read and write Frappe Books files,
> and the accounting engine is under way: journal entries, sales and purchase invoices and
> payments against them post to the ledger, and what each party owes is kept up to date.
> There is no
> app to use yet. Follow along in [docs/PLAN.md](docs/PLAN.md).

Latte Books is **not affiliated with Frappe Technologies**. It is built on their work; see
[NOTICE](NOTICE).

## What it will do

- **Open your existing Frappe Books file.** Point it at your `.books.db` and carry on. No
  export, no import.
- **Give the same numbers.** The same documents, fields, reports and rules as Frappe Books
  v0.36.0. Every ledger entry and report is checked against Frappe Books itself.
- **Without Frappe Books' bugs.** Where Frappe Books gets something wrong, Latte fixes it
  rather than copying it. So far that's 61 bugs, from write-offs posted the wrong way
  round to payments that can't be cancelled; see [docs/BOOKS-BUGS.md](docs/BOOKS-BUGS.md).
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
| 2 | Accounting engine and reports, checked against Frappe Books | **Done** |
| 3 | Desktop app: invoices, payments, reports, printing. **Public beta.** | Built; being tested |
| 4 | Everything else in Frappe Books v0.36.0: inventory, GST, POS and more. **1.0.** | Mostly built; being tested |

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

To run the app:

```sh
cargo run -p latte-app
```

It isn't ready for real books yet: try it on a copy of your company file.

To see the data layer read a company file (it opens the file read-only):

```sh
cargo run -p latte-db --example dump -- "path/to/company.books.db"
```

To print a report of a company file, also read-only, or to set up a new company as
Frappe Books' setup wizard does:

```sh
cargo run -p latte-core --example report_test -- "path/to/company.books.db" trial-balance
cargo run -p latte-core --example setup_test -- "path/to/new test.books.db" India "Acme" "Acme Bank"
```

To make installers (not signed yet) with
[cargo-packager](https://github.com/crabnebula-dev/cargo-packager), on each OS:

```sh
cargo install cargo-packager --locked
cargo packager --release -p latte-app
```

They land in `target/packages`. Pushing a `v*` tag builds them on GitHub and attaches them to a
draft release.

### AI assistants

**Set up AI assistants** in the app's sidebar connects Claude Desktop, Claude Code, Codex
or Cursor to your books. The assistant runs `latte-mcp`, which is built beside the app and
works in the company you opened last (or the one chosen on that screen). It can look up
documents, run reports, and save, submit and cancel documents, with the same checks as the
app. To try it from a build, `cargo build` first so `latte-mcp` is next to `latte-books`.

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
| `crates/latte-app` | The `latte-books` executable, and its installer settings |
| `crates/latte-mcp` | Dime: the `latte-mcp` executable AI assistants run, and their setup |
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
