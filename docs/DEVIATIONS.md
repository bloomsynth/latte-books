# Deviations from Frappe Books

Latte Books follows Frappe Books **v0.36.0** as closely as possible (see
[PLAN.md](PLAN.md#porting-principle-stay-as-close-to-the-original-as-possible)).
Every place where it behaves differently is listed here, with the reason. The Books bugs
Latte fixes are also described one by one in [BOOKS-BUGS.md](BOOKS-BUGS.md).

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
| Invoice features not ported yet | Pricing rules, coupons, loyalty points, returns, and making the shipment on submit | Refused with "… isn't supported in Latte yet." Saving refuses a sales invoice with a row an enabled pricing rule is for; submitting, cancelling and deleting refuse what would need the rest | An invoice Latte saves is never one Books would have saved differently |
| Payment write-offs | The whole amount posts to the cash or bank account; the write-off goes against the party's account and to the write-off account the other way round | The cash or bank account takes the amount less the write-off, and the write-off account the rest: a loss when receiving, a gain when paying | Books bug: the cash is overstated, the party still owes the write-off in the ledger, and a loss shows as a gain |
| A payment short of its invoices, made up by a write-off | The amount and write-off together must cover what is allocated to the invoices; with partial payments off, any payment with a write-off is let through | The amount alone must cover them, and a write-off doesn't excuse a partial payment | Books bug: more comes off the invoices than the party is credited with |
| A negative write-off, or one more than the amount | Not checked | Refused | Books bug |
| A submit or cancel that fails to save | The document keeps the `submitted` or `cancelled` mark in memory. After a failed cancel the form offers Delete, which removes the document without reverting what it posted | The document goes back to how it was before the submit or cancel | Books bug, seen 2026-10-23: a deleted payment left its invoice paid and its ledger entries behind |
| Cancelling a payment | Checks the amount against what its invoices owe now, so after a restart (0 owed) it refuses: "Payment amount cannot exceed ₹ 0.00." Cancelling the invoice fails the same way | The amount is checked only while the payment is a draft | Books bug, seen 2026-10-23 |
| A payment for several invoices | The amount may be no more than the last invoice's outstanding | No more than the sum of all of them | Books bug |
| A partial payment with partial payments off | A new payment for less can be saved; it is refused only at submit | Refused on every save | Books bug: the check depends on a total cached by an earlier check |
| Row "Discounted Amount" with a discount percent | Shows the discount (200 for 10% of 2000) | Shows the amount less the discount (1800) | Books bug. The field is shown, not stored |
| Row "Taxed Amount" | Updates only when the rate, quantity or item changes, so a discount or tax change leaves it stale | Updates when the discount or tax changes too | Books bug. The field is shown, not stored |
| Row transfer unit when the item changes | Always goes back to the item's unit (its check is the wrong way round) | Kept if the new item converts to it | Books bug |
| Whole-number units | The row quantity is never rounded (it looks for the unit in `uom`, which Item doesn't have) | Rounded when the item's unit is a whole number | Books bug |
| Row stock not shipped or received | Takes off only the last shipped row's quantity | Takes off all shipped or received rows of the item | Books bug |
| Amounts in messages | "Total Debit: ₹ 1,250.00 …", in the locale's format with the currency symbol | "Total Debit: 1250.00 …" | Number formatting isn't ported yet |
