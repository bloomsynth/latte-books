# Contributing to Latte Books

Thanks for helping keep the Frappe Books desktop app alive. Issues, checks against Frappe
Books and pull requests are all welcome.

## Reporting a problem

[Open an issue](https://github.com/bloomsynth/latte-books/issues/new/choose) and say:

- what you did, what you expected, and what happened instead;
- your OS, and your company's country and language;
- whether Frappe Books v0.36.0 does the same with the same file, if you can check.

Security problems are the exception: report them privately, as [SECURITY.md](SECURITY.md)
describes. Everyone taking part follows the [code of conduct](CODE_OF_CONDUCT.md).

**Never attach your real company file.** It holds your customers, suppliers and accounts.
If a file helps, make one with **Create Demo** or **New Company** and reproduce the problem
there.

## How Latte is ported

Latte ports Frappe Books **v0.36.0** as closely as it can: the same documents, fields, rules,
reports and messages, and the same file format. Before changing behaviour, read the
porting principle in [docs/PLAN.md](docs/PLAN.md).

- **A deliberate difference** from Frappe Books goes in [docs/DEVIATIONS.md](docs/DEVIATIONS.md),
  with why.
- **A Frappe Books bug** isn't copied. Fix it, add a test that fails without the fix, and
  list it in [docs/BOOKS-BUGS.md](docs/BOOKS-BUGS.md) (and in DEVIATIONS). Keep the count in
  the README current.
- **Code comments** name the Frappe Books function or file a piece ports, so the two can be
  read side by side.
- **Docs change with the code**, in the same pull request: the README, PLAN, DEVIATIONS and
  BOOKS-BUGS as they apply.

## Building and checking

You need [rustup](https://rustup.rs/); `rust-toolchain.toml` pins the Rust version. On
Windows, build from PowerShell or a Developer Command Prompt so the MSVC linker is used.

Before you open a pull request, run what CI runs:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Tests that write make their own company files in the temp folder. Example programs that
write refuse a file whose name doesn't contain "test", so they can't touch a real company by
mistake; keep that rule in any you add.

## Pull requests

- Keep one change per pull request, with a message that says what it does for someone using
  the app.
- Include tests for new rules and fixed bugs.
- Files use LF line endings.

By contributing you agree that your work is licensed under the
[AGPL-3.0](LICENSE), like the rest of Latte Books.
