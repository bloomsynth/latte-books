# Deviations from Frappe Books

Latte Books follows Frappe Books **v0.36.0** as closely as possible (see
[PLAN.md](PLAN.md#porting-principle-stay-as-close-to-the-original-as-possible)).
Every place where it behaves differently is listed here, with the reason.

| Area | Frappe Books v0.36.0 | Latte Books | Why |
|---|---|---|---|
| Print templates | HTML/Vue templates rendered by Electron | Typst templates rendered to PDF | No browser engine in a native app |
| ERPNext sync | `ERPNextSyncSettings`, `ERPNextSyncQueue`, `FetchFromERPNextQueue` | Not ported; the tables are kept untouched | Out of scope (PLAN.md §1) |
| Right-to-left layout | Arabic and Persian use RTL | Left-to-right only for now | GPUI has no RTL support yet ([zed#31102](https://github.com/zed-industries/zed/issues/31102)) |
| Change log | — | Every write Latte makes is recorded in a `latte_changes` table it adds to the file | Groundwork for sync between devices (PLAN.md §7). Books ignores the table |
| Saving | Each statement of a save runs on its own | A save, with its child rows, runs in one transaction | A failed save leaves nothing half-written |
| Older files | Books brings a file from an older version up to date when it opens it | Only files already opened by Books v0.36.0 can be opened | Migrations aren't ported yet |
| Who made a change | `createdBy` and `modifiedBy` are the signed-in user | Always `Admin` | There is no sign-in yet |
| Custom forms and fields | Custom fields can be added and edited | Custom fields in a file are read and saved, but can't be added or edited | The CustomForm and CustomField models aren't ported yet |
| Amounts in messages | "Total Debit: ₹ 1,250.00 …", in the locale's format with the currency symbol | "Total Debit: 1250.00 …" | Number formatting isn't ported yet |
