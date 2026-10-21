# Latte Books

A fast, native desktop accounting app for Frappe Books users.

Frappe has ended the Frappe Books desktop app in favour of a web-only version
([frappe/books#1566](https://github.com/frappe/books/issues/1566)). Latte Books is a
Rust port of Frappe Books v0.36.0 that keeps the desktop app alive:

- **Opens your existing Frappe Books file.** Point it at your `.books.db` and carry on.
- **Same accounts, same numbers.** Same documents, fields, reports and translations as
  Frappe Books.
- **Native and fast.** Written in Rust with [GPUI](https://www.gpui.rs/). No Electron.
- **Free and open source** under the AGPL-3.0, like Frappe Books.

> **Status:** early development. Not usable yet. See [docs/PLAN.md](docs/PLAN.md).

Latte Books is **not affiliated with Frappe Technologies**. It is built on their work;
see [NOTICE](NOTICE).

## Layout

| Path | What it holds |
|---|---|
| `crates/latte-schema` | Loads Books' JSON schemas |
| `crates/latte-db` | SQLite access to `.books.db` files |
| `crates/latte-core` | Accounting rules and reports (no UI) |
| `crates/latte-i18n` | Translations and formatting |
| `crates/latte-print` | PDF printing with Typst |
| `crates/latte-regional` | Country modules (India GST, Switzerland) |
| `crates/latte-ui` | GPUI screens |
| `crates/latte-app` | The `latte-books` executable |
| `vendor/books` | Files copied from Frappe Books v0.36.0 |
| `docs` | Plan and list of deviations from Frappe Books |

## Building

```sh
cargo build
cargo test
cargo run -p latte-app
```

## License

AGPL-3.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
