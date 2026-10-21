# Vendored from Frappe Books

These files are copied unchanged from [frappe/books](https://github.com/frappe/books)
at tag **v0.36.0** (commit `2f028f42be70a1dbd45bd4c6483d57df2fc64805`).

Copyright (C) Frappe Technologies Pvt. Ltd. and the Frappe Books contributors.
Licensed under the AGPL-3.0 (see [LICENSE](LICENSE)).

| Folder | From | Contents |
|---|---|---|
| `schemas/` | `schemas/**/*.json`, `schemas/README.md` | Doctype schemas (app, inventory, core, meta, regional, tests) |
| `fixtures/` | `fixtures/` | `countryInfo.json` and verified charts of accounts |
| `translations/` | `translations/` | Community translation CSVs |
| `templates/` | `templates/` | HTML print templates (reference for the Typst ports) |
| `dummy/` | `dummy/items.json`, `dummy/parties.json` | Sample data used by the dummy-data generator |

**Do not edit these files.** Latte-specific changes go in the crates, and any
behaviour difference goes in `docs/DEVIATIONS.md`. To update, copy the same paths
from a new tag and record the new commit here.
