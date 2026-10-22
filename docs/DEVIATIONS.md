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
| Exchange rates | An invoice in a foreign currency fetches today's rate from api.vatcomply.com | The rate is 1 until you set it, as in Books when it can't fetch | No network calls yet |
| Invoice features not ported yet | Pricing rules, coupons, loyalty points, returns, and making the payment or shipment on submit | Refused with "… isn't supported in Latte yet." Saving refuses a sales invoice with a row an enabled pricing rule is for; submitting, cancelling and deleting refuse what would need the rest | An invoice Latte saves is never one Books would have saved differently |
| Row "Discounted Amount" with a discount percent | Shows the discount (200 for 10% of 2000) | Shows the amount less the discount (1800) | Books bug. The field is shown, not stored |
| Row "Taxed Amount" | Updates only when the rate, quantity or item changes, so a discount or tax change leaves it stale | Updates when the discount or tax changes too | Books bug. The field is shown, not stored |
| Row transfer unit when the item changes | Always goes back to the item's unit (its check is the wrong way round) | Kept if the new item converts to it | Books bug |
| Whole-number units | The row quantity is never rounded (it looks for the unit in `uom`, which Item doesn't have) | Rounded when the item's unit is a whole number | Books bug |
| Row stock not shipped or received | Takes off only the last shipped row's quantity | Takes off all shipped or received rows of the item | Books bug |
| Amounts in messages | "Total Debit: ₹ 1,250.00 …", in the locale's format with the currency symbol | "Total Debit: 1250.00 …" | Number formatting isn't ported yet |
