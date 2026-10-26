# Deviations from Frappe Books

Latte Books follows Frappe Books **v0.36.0** as closely as possible (see
[PLAN.md](PLAN.md#porting-principle-stay-as-close-to-the-original-as-possible)).
Every place where it behaves differently is listed here, with the reason. The Books bugs
Latte fixes are also described one by one in [BOOKS-BUGS.md](BOOKS-BUGS.md).

| Area | Frappe Books v0.36.0 | Latte Books | Why |
|---|---|---|---|
| Print templates | PrintTemplates are HTML with Vue syntax and Tailwind classes, laid out by Electron | PrintTemplates are Typst, laid out to PDF by Latte with its print library (`/latte.typ`), which gives them Books' `doc` and `print` values. The bundled templates are Typst ports of Books' six designs, under Books' names | No browser engine in a native app; Typst prints the same on every device |
| Books HTML print templates | Printed | A Books HTML template in a company file is listed, but not printed: Print View and the Template Builder say it is a Frappe Books HTML template and offer Convert to Typst, which makes a custom Typst copy, "<name> (Typst)", with `TODO(convert)` notes where the HTML couldn't be followed; the HTML template is kept, read-only, for Books | The conversion is done once, best effort, and the Typst is then the user's to edit |
| Bundled templates in a file Books also opens | Books writes its templates again when its files are newer than the rows | Latte writes its Typst templates over Books' bundled (not custom) rows whenever they aren't Latte's, marked as modified in 2000, so Books puts its HTML back when it opens the file | Each app prints with its own bundled templates; custom ones are left alone by both |
| Print fonts | The Print Settings font (Arial, Times New Roman or Courier) from the computer; Inter where a template sets none | The bundled Liberation Sans, Serif and Mono, which have those fonts' metrics, and the bundled Inter; never the computer's fonts | The same printout on every device |
| ERPNext sync | `ERPNextSyncSettings`, `ERPNextSyncQueue`, `FetchFromERPNextQueue` | Not ported; the tables are kept untouched | Out of scope (PLAN.md §1) |
| Right-to-left layout | Arabic and Persian use RTL | Left-to-right only for now | GPUI has no RTL support yet ([zed#31102](https://github.com/zed-industries/zed/issues/31102)) |
| Change log | — | Every write Latte makes is recorded in a `latte_changes` table it adds to the file | Groundwork for sync between devices (PLAN.md §7). Books ignores the table |
| Saving | Each statement of a save runs on its own | A save, with its child rows, runs in one transaction | A failed save leaves nothing half-written |
| Older files | Books brings a file from an older version up to date when it opens it | Only files already opened by Books v0.36.0 can be opened | Migrations aren't ported yet |
| Who made a change | `createdBy` and `modifiedBy` are the signed-in user | Always `Admin` | There is no sign-in yet |
| Custom forms and fields | Customize Form is a page of its own, and a new field appears straight away | Customize Form is the CustomForm list and form; saving or deleting one opens the file again by itself, which adds the new fields and their columns | The schemas are built when a file is opened; reopening is how Books makes the column too |
| A custom field's label set again to the same value | Refused as a duplicate of itself | Allowed | Books bug |
| Exchange rates | An invoice in a foreign currency fetches today's rate from api.vatcomply.com | The rate is 1 until it is set in the header's exchange rate control, as in Books when it can't fetch | No network calls yet |
| Exchange rate box | Shows the rate to two decimals and writes that back on leaving the box, even unchanged | Shows every digit; leaving it unchanged keeps the rate | Books bug |
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
| Search's recent documents | Kept by their schema's label; picking one opens `/list/<label>`, which is no list. A recent item picked again keeps only its label and opens nothing | A recent document opens the document; a recent item picked again keeps what it opens | Books bug |
| Date controls | The browser's date input, typed in the computer's date format, with the browser's calendar | Typed as YYYY-MM-DD (and HH:MM), with a calendar drawn like Chromium's | No browser engine in a native app |
| Color control's custom colour | An `<input type="color">` that opens the system's colour picker | A "Custom Hex" box for `#RRGGBB` under the swatches | GPUI has no colour dialog |
| Short stock before submitting | An item with exactly enough stock is listed as short, "(0)" | Only items with less stock than their quantity | Books bug |
| Scanning a barcode into a Stock Movement | Says the item was added, but adds no row (`StockMovement.addItem` only reads the item) | Adds a row, or one more of it | Books bug |
| Print | Chromium's print dialog, with its own preview | The computer's print dialog: Windows' Print dialog, then each page sent as a picture at the printer's resolution (up to 600 dpi); macOS's print panel on the PDF; on Linux the desktop's dialog through xdg-desktop-portal. Without a portal, the PDF opens in the PDF viewer to print from there | No browser engine; the system's own dialogs instead |
| Print View pages | The first page's area is shown, scrolling what overflows it | Every page is shown, one under another, up to 30; the PDF has them all | The printout is laid out as pages; a long ledger stays quick to show |
| Duplicate Template | Copies the type and the template, not the page size, so a copy of the POS receipt prints on A4 | Copies the page size too | Books bug |
| Template Builder | The editor highlights Vue and completes keys from the Key Hints; the panel can be dragged wider; Set Template Type's dialog is titled "Set Print Size"; Select and Save Template File use `.template.html` | No highlighting or completion; the panel's width is fixed but for edit mode; the dialog is titled "Set Template Type"; `.typ` files; the Key Hints also list `date`, `grandTotalInWords` and the other values Books adds, and Latte's print library and fonts; a Typst error is shown with its line and column, and marked in the editor | Typst, not HTML; the dialog title is a Books bug |
| Report JSON export | `softwareName` is "Frappe Books" | "Latte Books", with Latte's version | It is Latte that made the file |
| Dates shown | In the computer's time zone, so west of UTC a ledger entry shows the day before | The date it is stored for | Books bug for anyone west of UTC |
| Creating a company file where one exists | Deletes the file and starts again | Refused | A typo shouldn't cost anyone their books |
| Printing a payment that isn't for a Sales Invoice | Fails: its tax rows are missing, and the Payment template reads how many there are | Prints, with no tax rows | Books bug |
| Default print template in a new company | `checkSingleLinks` drops Defaults' print templates before the templates are made, so the newest template of the schema is used, after an error dialog for a missing one | The same template, without the dialog | The dialog has nothing to act on |
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
| Data import columns | A file's columns take the fields the template had in those places, even past the end of the file | Columns past the end of the file are for no field | Books bug |
| Data import missing links | Named with the schema being imported: "(Sales Invoice, Acme)" | Named with the schema linked to: "(Party, Acme)" | Books bug |
| Import Wizard cells | Edited in place with the field's own control, such as a checkbox or a date picker; Fix Failed puts the columns back in the template's order | Edited in place as text; Fix Failed keeps the columns as they were | Simpler in GPUI; the same values are imported |
| Import Wizard rows added | "1 rows added.", "2 row added." | "1 row added.", "2 rows added." | Books bug (it tests for 2 rows) |
| Dashboard period start | The time of day it is now, so the first day's earlier entries are left out | The start of the first day | Books bug |
| POS shift closing | Posts the difference between Cash and the write-off account, wrong way round, and leaves it on the counter | Counter cleared to Cash; the difference between the counter and the write-off account | Books bug |
| POS stock check | Stops at the first untracked item | Checks every tracked item | Books bug |
| POS discounts shown | Only percents above 1 | Any percent above 0 | Books bug |
| POS shift takings | A payment's whole amount once per POS invoice it pays; drafts and cancelled payments count | What each submitted payment put against its POS invoices, once | Books bug |
| POS negative closing amount | Allowed (the check swallows its error) | Refused | Books bug |
| POS cash over the total | The payment is for all the cash and is refused | The payment is for the total; the rest shown as change | Books bug |
| POS item search | A Link dropdown matched fuzzily | A Link dropdown matched by what is typed anywhere in the name, as Latte's other Link fields | Simpler; the same items are found |
| POS paying a submitted sale | Submits it again, shows the error, then pays | Pays it | Books bug |
| POS cash refund for a return | Pays it to the counter cash account from the counter cash account, which the payment refuses | Pays it from the counter cash account | Books bug |
| POS quantity refused at the till | The row and the keypad keep the refused quantity | Put back as it was, as Books' quick quantity does | Books bug |
| POS quantity checked | Against the first row of the item, which for a batched item can be another batch's | Against the row changed | Books bug |
| POS keypad quantity | Takes zero or less on a sale | Refused, as the row and the quick quantity refuse it | Books bug |
| POS row quantity and discounts | Changing a row's quantity clears the discount a pricing rule gives for it when the row had no discount of its own | The discount is kept as it was, and a pricing rule's is left alone | Books bug |
| POS loyalty points Cancel | Leaves the points typed redeemed | Takes them back | Books bug |
| POS Return modal Cancel | Opens the Saved Invoice modal and leaves the Return modal open | Closes it | Books bug |
| POS Save and Continue | Leaves for the Sales Invoice List whether or not the sale saved; the error is lost | Leaves once the sale is saved | Books bug |
| POS Shift+L | Needs price lists on | Needs the loyalty program on, as its quick action does | Books bug |
| POS applied coupons badge | Blank once a coupon is taken off | The number of coupons on the sale; none at 0 | Books bug |
| POS quick action names | A label above the icon on hover | A tooltip | Simpler in GPUI |
| POS messages | Toasts stack | One message at a time, the latest | Simpler; the same messages |
| Get Started "Create Purchase Invoice" | Ticked once there is a sales invoice | Ticked once there is a purchase invoice | Books bug |
| Create Demo fiscal year | 1 April to 1 April | 1 April to 31 March | Books bug |
| Create Demo items | An invoice can list an item twice | Each item once | Books bug |
| Create Demo write-offs | None (it sets a field Payment doesn't have) | One sale in twenty paid with 15% written off | Books bug |
| Dark mode | Each component has its own `dark:` shades, and the change shows once the window is reloaded | Every screen maps each light gray to the dark shade Books pairs it with most; the sidebar, page headers, buttons, dialogs, menus, toasts, search, lists and dashboard use their components' own. Saving the setting changes the look at once | One palette for every screen; Settings take effect without a reload |
| Top Expenses legend | Its colour squares are blank: the colours are given as an object Vue reads as class names | Coloured as the donut's slices | Books bug |
| Shortcuts while typing | Shift+Backspace goes back even while typing in a field | Shift+Backspace and Shift+H do nothing while typing in a field | A capital H or a Shift+Backspace in a field shouldn't leave the page |
| Report and Report Issue | Open a new issue on frappe/books with the error, the version, platform, path, language, country and the settings' on/off values | Open one on Latte Books' repository with the error, version, platform and language | Latte's issues are its own |
| Shortcuts window | "Applicable anywhere in Frappe Books" | "Applicable anywhere in Latte Books" | The app's name |
| Changes from another device | Shown when a page is opened again | Every two seconds the app looks for changes another device, such as an AI assistant, wrote, and shows the page again; a form shows them only when they are to its document and it has no changes of its own; the till, Settings and the import wizard aren't shown again | Dime (PLAN.md): an assistant works in the file while the app is open |
| AI assistants | — | "Set up AI assistants" connects Claude Desktop, Claude Code, Codex or Cursor, which work in the books through `latte-mcp` with the same checks as the app | Dime (PLAN.md); Books doesn't touch the assistants' files |
