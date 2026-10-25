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
| Custom forms and fields | Customize Form is a page of its own, and a new field appears straight away | Customize Form is the CustomForm list and form; saving or deleting one opens the file again by itself, which adds the new fields and their columns | The schemas are built when a file is opened; reopening is how Books makes the column too |
| A custom field's label set again to the same value | Refused as a duplicate of itself | Allowed | Books bug |
| Exchange rates | An invoice in a foreign currency fetches today's rate from api.vatcomply.com | The rate is 1 until you set it, as in Books when it can't fetch | No network calls yet |
| Returning an invoice with loyalty points | The return changes the invoice's loyalty entry by its own points, whichever way they went | Refused with "Returning an invoice with loyalty points isn't supported in Latte yet." | Books' result isn't clearly right; to settle with real cases |
| Pricing rule applied flag | Set on every sales invoice once pricing rules are on | Set only when a rule applies | Books bug |
| A coupon for an item in several pricing rules | Checked against the first rule the item is in only | Applies whatever the order of the item's rules | Books bug |
| Free items that aren't stock | Added only when there is stock, so never with inventory off or for a service | Added | Books bug |
| Free item of no quantity message | Split over two lines | One line | Books bug |
| Whether a rule or coupon applies to a row | Judged on the row's amount, which the rule's own rate or discount has lowered, so on saving a coupon at its rule's minimum amount is dropped | Judged on the item's rate times the quantity, as the invoice does | Books bug |
| A coupon outside its dates | "Valid From Date should be less than Valid To Date." | "Coupon X isn't valid yet on the invoice date." or "… is no longer valid …" | Books bug: the message blames the coupon's setup |
| Cancelling a return with coupons | Frees the coupons a second time | Uses them again | Books bug |
| Redeemed loyalty points | Taken off the grand total, and the customer credited with them again; the round-off account balances it | Taken off once, and posted to the program's expense account | Books bug |
| Cancelling an invoice that earned loyalty points | The points are made again and kept | Taken back | Books bug |
| An invoice from a quote | Copies the quote's name, meta fields and row names; the database sets the parent fields again on saving | Starts as a new invoice with new rows | Same result, without carrying the quote's names |
| GSTR rate | The taxes' rates added up twice (18% GST shows 36) | Added once | Books bug: also doubles the B2C export's integrated tax |
| GSTR tax amounts | An IGST account also sets the central and state tax, a CGST one the state tax, and every GST account marks the invoice nil rated, exempt and non-GST | Each account sets its own | Books bug (a `switch` without `break`s) |
| Payment write-offs | The whole amount posts to the cash or bank account; the write-off goes against the party's account and to the write-off account the other way round | The cash or bank account takes the amount less the write-off, and the write-off account the rest: a loss when receiving, a gain when paying | Books bug: the cash is overstated, the party still owes the write-off in the ledger, and a loss shows as a gain |
| A payment short of its invoices, made up by a write-off | The amount and write-off together must cover what is allocated to the invoices; with partial payments off, any payment with a write-off is let through | The amount alone must cover them, and a write-off doesn't excuse a partial payment | Books bug: more comes off the invoices than the party is credited with |
| A negative write-off, or one more than the amount | Not checked | Refused | Books bug |
| A submit or cancel that fails to save | The document keeps the `submitted` or `cancelled` mark in memory. After a failed cancel the form offers Delete, which removes the document without reverting what it posted | The document goes back to how it was before the submit or cancel | Books bug, seen 2026-10-23: a deleted payment left its invoice paid and its ledger entries behind |
| Cancelling a payment | Checks the amount against what its invoices owe now, so after a restart (0 owed) it refuses: "Payment amount cannot exceed ₹ 0.00." Cancelling the invoice fails the same way | The amount is checked only while the payment is a draft | Books bug, seen 2026-10-23 |
| A payment for several invoices | The amount may be no more than the last invoice's outstanding | No more than the sum of all of them | Books bug |
| A new payment's cash or bank account | The first account of that type, newest first, groups included | The first that isn't a group | Books bug: a group made in the same millisecond as its accounts can come first, and the payment's account fields don't offer groups |
| A partial payment with partial payments off | A new payment for less can be saved; it is refused only at submit | Refused on every save | Books bug: the check depends on a total cached by an earlier check |
| Return postings | Posts the return's negative totals on the swapped sides, which posts the invoice again; a return's discount isn't posted | Posts their size on the swapped sides, discount included, so the return reverses its invoice | Books bug. Books' own tests expect the positive amounts |
| What is left to return | Takes off every return of the invoice, drafts and cancelled ones included; each row gets all that is left of its item | Takes off submitted returns only; what is left is shared between rows of the same item | Books bug: a cancelled return's items can't be returned again, and an item on two rows is returned twice |
| A return of more than is left | Not checked | Refused | Books bug |
| A new return's Returned flags | Copied from the invoice | Cleared | Books bug |
| Cancelling a return | Leaves the invoice Fully Returned | Clears Fully Returned | Books bug |
| Refunding a return | Make Payment fills in the return's negative outstanding amount, which the payment refuses; a typed-in amount is added to what the return owes instead of taken off | The refund is for what the return is owed, and clears it | Books bug |
| General Ledger To Date | Entries on the To Date are left out | Included | Books bug |
| General Ledger reference types | Shipment and Purchase Receipt are offered when inventory is off | Offered when it is on | Books bug |
| Profit And Loss with only income | Shows nothing | Shows the income and its total | Books bug |
| Profit And Loss with only expenses | The total is labelled "Total Income (Credit)" | "Total Expense (Debit)" | Books bug |
| Fiscal year periods in reports | Counted in whole years between From Year and To Year, so periods go missing | The whole span from the From Year's start to the To Year's end | Books bug |
| Reports' default year | The fiscal year starting this calendar year | The fiscal year today is in | Books bug |
| Trial Balance closing | The entries after the To Date | The opening plus the period | Books bug |
| Trial Balance period | Ends before the To Date; the default To Date is the day before the year ends | Includes the To Date; the default is the whole fiscal year | Books bug |
| Trial Balance entries before 0001 or after 9999 | Stops with "invalid entry in trial balance" | Left out | Can't happen in practice; Latte doesn't stop the report for it |
| CSV export with display precision 0 | Cuts the last digit off every amount | Amounts exported whole | Books bug |
| Report totals | Summed as floats | Summed as money | No drift on long ledgers; what is shown is the same |
| Report refresh | Reports watch the ledger and refresh themselves | The app runs a report again | Simpler; the same rows |
| Report JSON export | Offered for every report | Offered for GSTR-1 and GSTR-2 only | CSV first; the GST portal needs GSTR's JSON |
| Dates shown | In the computer's time zone, so west of UTC a ledger entry shows the day before | The date it is stored for | Books bug for anyone west of UTC |
| Creating a company file where one exists | Deletes the file and starts again | Refused | A typo shouldn't cost anyone their books |
| Printing a payment that isn't for a Sales Invoice | Fails: its tax rows are missing, and the Payment template reads how many there are | Prints, with no tax rows | Books bug |
| Print templates in a new company | The Basic, Business and Minimal templates are added as PrintTemplate documents | Not added | Latte prints with its own Typst templates |
| Account names in a new company | Translated into the app's language | In English | Books looks some of them up by their English names, such as a party's Debtors or Creditors, which a translated chart doesn't have |
| Row "Discounted Amount" with a discount percent | Shows the discount (200 for 10% of 2000) | Shows the amount less the discount (1800) | Books bug. The field is shown, not stored |
| Row "Taxed Amount" | Updates only when the rate, quantity or item changes, so a discount or tax change leaves it stale | Updates when the discount or tax changes too | Books bug. The field is shown, not stored |
| Row transfer unit when the item changes | Always goes back to the item's unit (its check is the wrong way round) | Kept if the new item converts to it | Books bug |
| Whole-number units | The row quantity is never rounded (it looks for the unit in `uom`, which Item doesn't have) | Rounded when the item's unit is a whole number | Books bug |
| Row stock not shipped or received | Takes off only the last shipped row's quantity | Takes off all shipped or received rows of the item | Books bug |
| Shipment or receipt discount | Reads the invoice's discount settings from the shipment, which has none, so takes the percent | Uses the invoice's own settings | Books bug: a discount given as an amount is lost |
| Invoice taxes with the discount after tax | None: every tax is left out | Taken on each row's whole amount | Books bug |
| Shipment row discount after tax | Taken on the shipment row's taxed total, which it doesn't have | Taken on the invoice row's | Books bug |
| Shipment cost of goods | Fails on an item without stock entries beside one with them, and on a row without a stock queue | Worked out for every row | Books bug |
| Shipment and receipt returns | Post the negative amount on the swapped sides; count draft and cancelled returns; let a return take back more than was shipped; copy the Returned flag and the invoice link; return serial numbers already returned | Reverse the posting; the same return rules as invoices; leave out the invoice link and returned serial numbers | Books bugs |
| A return invoice's shipment or receipt | Always looks for a sales invoice's shipment, and fails when there's none | Made against its own invoice's shipment or receipt | Books bug |
| Stock Ledger and Stock Balance To Date | Entries on the To Date are left out | Included | Books bug |
| Inventory not ported yet | The barcode scanner (`addItem`) and the inventory list filters | Not ported yet | Phase 4 |
| Data import columns | A file's columns take the fields the template had in those places, even past the end of the file | Columns past the end of the file are for no field | Books bug |
| Data import missing links | Named with the schema being imported: "(Sales Invoice, Acme)" | Named with the schema linked to: "(Party, Acme)" | Books bug |
| Import Wizard | Cells are edited in place; Submit is asked in a dialog; the column picker is a dialog | A clicked cell is edited in a box above the table; "Submit entries?" is a checkbox; the column picker is on the page | Simpler in GPUI; the same import |
| Dashboard period start | The time of day it is now, so the first day's earlier entries are left out | The start of the first day | Books bug |
| Dashboard period picker | A period picker on each chart | One period picker for the page, in the header | Simpler; the same numbers |
| POS shift closing | Posts the difference between Cash and the write-off account, wrong way round, and leaves it on the counter | Counter cleared to Cash; the difference between the counter and the write-off account | Books bug |
| POS stock check | Stops at the first untracked item | Checks every tracked item | Books bug |
| POS discounts shown | Only percents above 1 | Any percent above 0 | Books bug |
| POS shift takings | A payment's whole amount once per POS invoice it pays; drafts and cancelled payments count | What each submitted payment put against its POS invoices, once | Books bug |
| POS negative closing amount | Allowed (the check swallows its error) | Refused | Books bug |
| POS cash over the total | The payment is for all the cash and is refused | The payment is for the total; the rest shown as change | Books bug |
| POS screens | Open shift, close shift and payment are dialogs | Panels on the page | Simpler in GPUI |
| POS saved sales | Saved and submitted sales are listed in a dialog | Listed in a panel beside the cart | Simpler in GPUI |
| POS paying a submitted sale | Submits it again, shows the error, then pays | Pays it | Books bug |
| POS not ported yet | The Modern UI, item groups, keyboard shortcuts, returns, coupons, loyalty points, price lists, and editing a row's rate or discount at the till | Not ported yet | The Modern UI, and returns, coupons and loyalty at the till, come after 1.0 |
| Get Started cards | A picture on each card | The card's name and text, with a check once done | Latte has no pictures for them yet |
| Get Started "Create Purchase Invoice" | Ticked once there is a sales invoice | Ticked once there is a purchase invoice | Books bug |
| Create Demo progress | A full-window loader | A message and bar in the company list | Simpler in GPUI |
| Create Demo fiscal year | 1 April to 1 April | 1 April to 31 March | Books bug |
| Create Demo items | An invoice can list an item twice | Each item once | Books bug |
| Create Demo write-offs | None (it sets a field Payment doesn't have) | One sale in twenty paid with 15% written off | Books bug |
| Amounts in messages | "Total Debit: ₹ 1,250.00 …", in the locale's format with the currency symbol | "Total Debit: 1250.00 …" | Number formatting isn't ported yet |
| Changes from another device | Shown when a page is opened again | Every two seconds the app looks for changes another device, such as an AI assistant, wrote, and shows the page again; a form shows them only when they are to its document and it has no changes of its own; the till, Settings and the import wizard aren't shown again | Dime (PLAN.md): an assistant works in the file while the app is open |
| AI assistants | — | "Set up AI assistants" connects Claude Desktop, Claude Code, Codex or Cursor, which work in the books through `latte-mcp` with the same checks as the app | Dime (PLAN.md); Books doesn't touch the assistants' files |
