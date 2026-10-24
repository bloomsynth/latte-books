# Security policy

Latte Books keeps a company's accounts in a file on the user's own computer, and Dime's
`latte-mcp` server lets AI assistants read and write that file. A security problem here can
expose or change someone's books, so please report it privately.

## Reporting a vulnerability

**Don't open a public issue.** Use
[Report a vulnerability](https://github.com/bloomsynth/latte-books/security/advisories/new)
on the Security tab instead. Say what you found, how to reproduce it, and what it lets
someone do.

You'll get a reply within a week. Once a fix is out, the advisory is published with credit
to you, unless you'd rather not be named.

**Never attach a real company file**, yours or anyone's. Reproduce the problem in a company
made with **Create Demo** or **New Company**.

## What's in scope

- Reading or changing a company file without going through Latte's rules, for example
  through `latte-mcp` or a crafted `.books.db`, import file or print template.
- Anything that runs code or reaches the network from a file the user opens.
- The installers and the release workflow.

Bugs in Frappe Books itself are out of scope unless Latte inherits them.

## Supported versions

Latte Books is in beta. Fixes go into the latest release and `main`.
