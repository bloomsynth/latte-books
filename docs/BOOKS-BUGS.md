# Frappe Books bugs Latte fixes

Latte Books ports Frappe Books **v0.36.0** as closely as it can (see
[PLAN.md](PLAN.md#porting-principle-stay-as-close-to-the-original-as-possible)), with one
exception: where Books has a bug, Latte doesn't copy it. Each bug found while porting is
listed here, with what it does to your books, what Latte does instead, how we know, and
the test that keeps it fixed. [DEVIATIONS.md](DEVIATIONS.md) lists every difference from
Books, these included.

**Seen in Books** means we did it in Frappe Books v0.36.0 and watched it go wrong.
**From the code** means it follows from reading Books' source; we haven't made it happen
in Books yet.

All the fixes are backwards compatible: Latte reads and writes the same files, and
what Books already saved is left as it is.

## Payments

### Cancelling a payment after a restart is refused

- **In Books:** a payment's amount is checked against what its invoices still owe on
  every save, cancelling included. Once the payment is made the invoice owes 0, so after
  a restart cancelling it fails with "Payment amount cannot exceed ₹ 0.00." Nor can the
  invoice be cancelled, as that cancels its payments.
- **What it does to your books:** a payment made by mistake can only be undone in the
  session it was made in.
- **In Latte:** the amount is checked only while the payment is a draft.
- **How we know:** seen in Books, 2026-10-23.
- **Test:** `cancelling_a_payment_puts_back_what_is_owed`.

### A failed cancel lets you delete a payment without undoing it

- **In Books:** cancelling marks the document cancelled, then saves it. If the save
  fails, the mark stays. The form takes the document for cancelled and offers Delete,
  which removes it without putting back what it paid.
- **What it does to your books:** after the refused cancel above, deleting the payment
  left its invoice marked paid, the party owing nothing, and the payment's ledger entries
  in place, pointing at a payment that no longer exists ("Not Found: Payment PAY-1015").
- **In Latte:** a submit or cancel that fails to save leaves the document exactly as it
  was.
- **How we know:** seen in Books, 2026-10-23.
- **Test:** `a_failed_cancel_leaves_the_document_submitted`.

### One payment can't pay two invoices

- **In Books:** the amount may be no more than what the *last* invoice in the payment
  owes, so a payment for two invoices is refused.
- **In Latte:** no more than what all of them owe.
- **How we know:** from the code.
- **Test:** `one_payment_pays_two_invoices`.

### A partial payment slips through until submit

- **In Books:** with partial payments off, a payment for less than what is owed should
  be refused. The check depends on a total kept by an earlier check, so a new payment can
  be saved for less and is refused only at submit.
- **In Latte:** refused on every save.
- **How we know:** from the code.
- **Test:** `a_partial_payment_needs_partial_payments_on`.

### Write-offs post the wrong way round

- **In Books:** an invoice for ₹1,000, settled with ₹990 and ₹10 written off, posts Cash
  ₹1,000, takes only ₹990 off Debtors, and credits the ₹10 to Write Off as income.
  Paying a supplier has the same mistake in reverse.
- **What it does to your books:** the cash is overstated, the customer still owes ₹10 in
  the ledger while the invoice says it is paid, and a loss shows as a gain.
- **In Latte:** Cash ₹990, Write Off ₹10 as a loss, Debtors ₹1,000. Paying a supplier,
  the write-off is a gain.
- **How we know:** seen in Books, 2026-10-23. Settling a ₹1,416 invoice with ₹4 written
  off posted Cash ₹1,416, Debtors ₹1,416 credit and ₹4 debit, and Write Off ₹4 credit.
  The ledger then had the customer owing ₹4 while the invoice and the party said ₹0.
  Books has no tests for write-offs.
- **Tests:** `a_write_off_on_a_receipt_is_a_loss_and_clears_the_invoice`,
  `a_write_off_on_a_payment_to_a_supplier_is_a_gain`.

### A write-off can stand in for part of the payment

- **In Books:** the payment must cover what is allocated to its invoices, but the
  write-off counts towards it on top of the amount, so more comes off the invoices than
  the party is credited with. And with partial payments off, any payment with a
  write-off is let through, leaving the invoice part paid.
- **In Latte:** the amount alone must cover the invoices, and a write-off doesn't
  excuse a partial payment.
- **How we know:** from the code.
- **Tests:** `the_amount_alone_covers_what_is_allocated_to_the_invoices`,
  `a_write_off_doesnt_make_a_partial_payment_whole`.

### A write-off can be negative or more than the payment

- **In Books:** not checked.
- **In Latte:** refused.
- **How we know:** from the code.
- **Test:** `a_write_off_is_no_less_than_zero_and_no_more_than_the_amount`.

## Returns

### A return posts the invoice again instead of reversing it

- **In Books:** a return's totals are negative, and Books posts them as they are on the
  swapped sides: Debtors is credited −₹354, which is a debit of ₹354, and Sales is
  debited −₹300. A return's discount, also negative, isn't posted at all; the round-off
  takes it.
- **What it does to your books:** the ledger shows the sale twice instead of cancelling
  it, while the invoices and the party say it was returned.
- **In Latte:** Debtors is credited ₹354, Sales and the taxes are debited, and the
  discount account credited, as Books' own tests expect.
- **How we know:** from the code; Books' test for returns expects the positive amounts.
- **Test:** `returning_an_invoice_reverses_its_posting_and_what_the_party_owes`.

### Cancelled returns still count as returned

- **In Books:** what is left to return takes off every return of the invoice, drafts and
  cancelled ones included, so a cancelled return's items can't be returned again.
- **In Latte:** only submitted returns count.
- **How we know:** from the code.
- **Test:** `cancelling_a_return_lets_its_items_be_returned_again`.

### An item on two rows is returned twice over

- **In Books:** each row of the return gets all that is left of its item, so an item on
  two rows is returned twice.
- **In Latte:** what is left is shared between the rows.
- **How we know:** from the code.
- **Test:** `an_item_on_two_rows_is_returned_once`.

### A return can take back more than was sold

- **In Books:** a return's quantities aren't checked against what is left to return.
- **In Latte:** refused: "Cannot return 3 of Widget: only 2 is left to return on
  SINV-1001."
- **How we know:** from the code.
- **Test:** `a_return_cant_take_back_more_than_is_left_to_return`.

### A return copies its invoice's Returned flags

- **In Books:** a return of a partly returned invoice is made as a copy of it, Returned
  flag included.
- **In Latte:** a return starts with the flags cleared.
- **How we know:** from the code.
- **Test:** `a_partial_return_leaves_the_rest_to_return`.

### Cancelling a return leaves the invoice Fully Returned

- **In Books:** submitting a return sets the invoice's Fully Returned flag; cancelling
  it doesn't clear it.
- **In Latte:** cancelling clears it.
- **How we know:** from the code.
- **Test:** `cancelling_a_return_lets_its_items_be_returned_again`.

### A refund for a return can't be made

- **In Books:** Make Payment on a return fills in its outstanding amount, which is
  negative, and a payment refuses a negative amount. When the amount is typed in, Books
  adds it to the return's outstanding amount instead of taking it off, so the return and
  the party are left owing twice the refund.
- **In Latte:** the refund is for the amount the return is owed, and clears it.
- **How we know:** from the code.
- **Tests:** `a_paid_invoice_returned_is_refunded`,
  `a_refund_whose_amount_is_typed_in_still_clears_the_return`,
  `a_purchase_return_is_refunded_by_the_supplier`.

## Reports

### The General Ledger leaves out the To Date

- **In Books:** the To Date filter is compared with the entries' stored date and time,
  so the entries made on the To Date itself are left out.
- **In Latte:** the To Date is included.
- **How we know:** from the code.
- **Test:** `the_general_ledger_lists_entries_by_voucher_with_a_closing_row`.

### The General Ledger offers shipments only with inventory off

- **In Books:** Shipment and Purchase Receipt are offered as reference types when
  inventory is off, the one time there are none; the check is the wrong way round.
- **In Latte:** they are offered when inventory is on.
- **How we know:** from the code.

### Profit And Loss with income but no expenses is empty

- **In Books:** the report shows only when there are expenses, so a company that has
  only sold sees nothing.
- **In Latte:** the income and its total are shown.
- **How we know:** from the code.
- **Test:** `profit_and_loss_with_only_income_shows_it`.

### Profit And Loss calls expenses income

- **In Books:** with expenses but no income, the total is labelled "Total Income
  (Credit)".
- **In Latte:** it is labelled "Total Expense (Debit)".
- **How we know:** from the code.

### Fiscal year periods go missing

- **In Books:** with Based On "Fiscal Year", the number of periods is worked out from
  whole years between the From Year and the To Year, so a January–December year with both
  the same shows only its last period.
- **In Latte:** the periods cover the whole span.
- **How we know:** from the code.

### The default year isn't this fiscal year

- **In Books:** the reports default to the fiscal year that starts this calendar year,
  which for an April–March year is next year's until April.
- **In Latte:** they default to the fiscal year today is in.
- **How we know:** from the code.

### The Trial Balance's closing isn't the closing

- **In Books:** the closing columns show the entries after the To Date, not the opening
  plus the period.
- **In Latte:** closing is the opening plus the period.
- **How we know:** from the code.
- **Test:** `the_trial_balance_closes_with_the_opening_plus_the_period`.

### The Trial Balance leaves out the year's last days

- **In Books:** the period ends before the To Date, and the default To Date is the day
  before the fiscal year ends, so the last two days of the year are missing.
- **In Latte:** the period includes the To Date, and the default is the whole year.
- **How we know:** from the code.
- **Test:** `the_trial_balance_closes_with_the_opening_plus_the_period`.

### The stock reports leave out the To Date

- **In Books:** the Stock Ledger and Stock Balance compare each entry's date and time with
  the start of the To Date, so the entries on the To Date are left out.
- **In Latte:** the To Date is included.
- **How we know:** from the code.
- **Test:** `the_stock_reports_value_stock_first_in_first_out`.

### CSV exports cut a digit off with no decimals

- **In Books:** with a display precision of 0, every amount loses its last digit in the
  CSV export: 123 becomes 12.
- **In Latte:** amounts are exported whole.
- **How we know:** from the code.
- **Test:** `csv_exports_drop_zero_decimals_and_quote_spaces`.

### GSTR shows twice the GST rate

- **In Books:** the GSTR-1 and GSTR-2 reports add up an invoice's tax rates twice, so an
  invoice with 18% GST shows a rate of 36.
- **What it does to your books:** the rate filed is wrong; an invoice with GST is never
  listed as nil rated, and the B2C-Large and B2C-Small JSON export claims twice the
  integrated tax.
- **In Latte:** the rates are added up once.
- **How we know:** from the code.
- **Test:** `gstr_1_lists_each_sale_with_its_gst_once`.

### GSTR mixes up the tax amounts

- **In Books:** the report sets each tax account's amount in a `switch` without `break`s,
  so an IGST account also fills in the central and state tax, a CGST account also the
  state tax, and every GST account marks the invoice nil rated, exempt and non-GST.
- **In Latte:** each account sets only its own amount.
- **How we know:** from the code.
- **Test:** `gstr_1_lists_each_sale_with_its_gst_once`.

## Invoice rows

### "Discounted Amount" shows the discount

- **In Books:** with a 10% discount on ₹2,000 the row's Discounted Amount is ₹200.
- **In Latte:** ₹1,800, the amount after the discount. The field is shown, not stored.
- **How we know:** seen in Books, 2026-10-22.
- **Test:** `discounts_come_off_before_tax_and_post_to_the_discount_account`.

### "Taxed Amount" goes stale

- **In Books:** it updates only when the rate, quantity or item changes, so changing the
  discount or the tax leaves it wrong.
- **In Latte:** it updates on those changes too. The field is shown, not stored.
- **How we know:** seen in Books, 2026-10-22 (₹2,360 for a row whose taxed amount was
  ₹2,124).
- **Test:** `discounts_come_off_before_tax_and_post_to_the_discount_account`.

### Changing the item resets the transfer unit

- **In Books:** a check written the wrong way round sends the transfer unit back to the
  item's unit every time.
- **In Latte:** kept when the new item converts to it.
- **How we know:** from the code.
- **Test:** `a_row_keeps_a_transfer_unit_the_new_item_converts_to_too`.

### Whole-number units aren't rounded

- **In Books:** the quantity should be rounded for a unit like "Unit", but Books looks
  the unit up under a field Item doesn't have, so it never is.
- **In Latte:** rounded.
- **How we know:** from the code.
- **Test:** `a_whole_number_unit_rounds_the_quantity`.

### Stock not shipped counts only the last shipment

- **In Books:** what is left to ship takes off only the last shipped row's quantity.
- **In Latte:** takes off every shipped or received row of the item.
- **How we know:** from the code.
- **Test:** `what_isnt_shipped_takes_off_every_shipped_row`.

### A discount after tax drops the taxes

- **In Books:** an invoice whose discount is applied after tax works out no taxes at all,
  though each row's taxed total includes them.
- **What it does to your books:** an invoice for ₹1,400 with 18% GST and 10% off after
  tax comes to ₹1,234.80, and posts no GST. It should be ₹1,652 less ₹165.20: ₹1,486.80,
  with ₹252 of GST.
- **In Latte:** the taxes are taken on each row's whole amount, and the discount comes
  off the taxed total.
- **How we know:** from the code.
- **Test:** `a_discount_after_tax_keeps_the_tax_and_carries_to_the_shipment`.

## Shipments and receipts

### A receipt or shipment ignores its invoice's discount

- **In Books:** a shipment or receipt made from an invoice works out its grand total with
  the invoice's discount, but reads whether the discount is an amount or a percent, and
  whether it comes after tax, from the shipment itself, which has none of those fields.
  So it always takes the percent, and a discount given as an amount is lost.
- **What it does to your books:** a receipt for an invoice of ₹1,400 less ₹100 puts
  ₹1,400 into Stock In Hand, not ₹1,300.
- **In Latte:** the invoice's own discount settings are used.
- **How we know:** from the code.
- **Test:** `a_receipt_takes_off_its_invoices_discount`.

### A row's discount after tax is lost on shipments

- **In Books:** a row discount percent after tax is taken on the row's total with its
  taxes, which Books reads from the shipment's row. That row has no taxes, so the
  discount comes to nothing.
- **In Latte:** it is taken on the invoice row's total with its taxes.
- **How we know:** from the code.
- **Test:** `a_discount_after_tax_keeps_the_tax_and_carries_to_the_shipment`.

### A shipment's cost of goods fails for some items

- **In Books:** working out what a shipment's items cost fails on an item with no stock
  ledger entries when another item has some, and on any row it has no stock queue for.
- **In Latte:** each row's cost is worked out; a row with no queue adds its amount.
- **How we know:** from the code.
- **Test:** `a_shipment_costs_an_item_with_no_stock_entries_at_its_amount`.

### A return of a shipment or receipt posts it again

- **In Books:** as with invoices, a return's amount is negative and is posted as it is
  on the swapped sides, so it posts the shipment again instead of reversing it.
- **In Latte:** the size of the amount is posted on the swapped sides.
- **How we know:** from the code.
- **Test:** `a_return_reverses_what_its_shipment_or_receipt_posted`.

### Returns of shipments and receipts have the invoices' return bugs

- **In Books:** what is left to return counts draft and cancelled returns, an item on two
  rows is returned twice, a return can take back more than was shipped, and it copies
  the Returned flag. It also copies the invoice the shipment was made from, so its grand
  total adds the invoice's taxes to its negative amounts and submitting it adds to the
  invoice's stock not shipped.
- **In Latte:** the same rules as for invoice returns, and a return leaves the invoice out.
- **How we know:** from the code.
- **Test:** `a_return_takes_back_no_more_than_was_shipped`.

### A purchase return looks for a shipment

- **In Books:** a return invoice set to make its shipment or receipt on submit always
  looks for a shipment of a sales invoice, even for a purchase return, and fails when
  there is none.
- **In Latte:** the return's shipment or receipt is made against its own invoice's.
- **How we know:** from the code.
- **Test:** `a_return_invoice_ships_against_its_invoices_shipment`.

### Serial numbers can be returned twice

- **In Books:** each row of a return takes all the serial numbers of the row it returns,
  including those already returned.
- **In Latte:** only the serial numbers not yet returned.
- **How we know:** from the code.
- **Test:** `a_serial_number_is_returned_once`.

## Pricing rules and coupons

### Every sales invoice is marked as having a pricing rule

- **In Books:** once pricing rules are on, Is Pricing Rule Applied is set on every sales
  invoice, as an empty list of rules counts as applied.
- **In Latte:** it is set only when a rule applies.
- **How we know:** from the code.
- **Test:** `with_pricing_rules_on_but_none_for_its_items_an_invoice_saves_as_usual`.

### A coupon is checked against one of the item's rules

- **In Books:** a coupon is matched with only the first pricing rule its item is in, so
  it is refused when its own rule happens to come later.
- **In Latte:** it applies if any of the item's rules is the coupon's.
- **How we know:** from the code.
- **Test:** none yet.

### A coupon at its rule's minimum amount is dropped on saving

- **In Books:** whether a coupon's rule applies to a row is judged on the row's amount,
  which the rule's own rate or discount has already lowered. A rule for 4000 and up that
  sets the rate to 800 for 5 jackets makes the amount 4000, which isn't more than the
  minimum, so saving drops the coupon the invoice was given.
- **In Latte:** it is judged on the item's own rate times the quantity, as the invoice
  itself judges it.
- **How we know:** from the code; Books' own tests never save an invoice with a coupon.
- **Test:** `a_coupon_applies_its_rule_and_is_counted_when_used`.

### Free items are added only for stock

- **In Books:** a product discount's free item is added only when there is stock of it,
  so with inventory off, or for a service, it never is.
- **In Latte:** a free item that isn't tracked as stock is added; one that is needs the
  stock.
- **How we know:** from the code.
- **Test:** `a_product_discount_adds_free_items`.

### The free item message is split

- **In Books:** the message for a free item of no quantity is written over two lines.
- **In Latte:** one line.
- **How we know:** from the code.
- **Test:** none.

### A coupon outside its dates blames its setup

- **In Books:** a coupon used before or after its dates is refused with "Valid From Date
  should be less than Valid To Date." or "Valid To Date should be greater than Valid
  From Date.", as if the coupon were set up wrong.
- **In Latte:** "Coupon X isn't valid yet on the invoice date." or "Coupon X is no longer
  valid on the invoice date."
- **How we know:** from the code.
- **Test:** `a_coupon_outside_its_dates_says_so`.

### Cancelling a return frees its coupons again

- **In Books:** submitting a return frees the coupons it used; cancelling it frees them
  again instead of using them again.
- **What it does to your books:** a coupon can be used more times than its maximum.
- **In Latte:** cancelling a return uses its coupons again.
- **How we know:** from the code.
- **Test:** none yet.

## Loyalty points

### Redeemed points are credited twice

- **In Books:** redeemed points come off the invoice's grand total, and the posting then
  credits the customer with them again. The round-off account takes the difference.
- **What it does to your books:** the customer's account shows less than they owe, and
  the round-off account holds the cost of the points.
- **In Latte:** the points come off once, and their worth is posted to the loyalty
  program's expense account.
- **How we know:** from the code.
- **Test:** `invoices_earn_and_redeem_loyalty_points`.

### Earned points outlive a cancel

- **In Books:** cancelling an invoice deletes its loyalty entry and then makes it again
  with the same points unless points were redeemed.
- **What it does to your books:** a customer keeps the points of a cancelled sale.
- **In Latte:** cancelling takes back the points the invoice earned.
- **How we know:** from the code.
- **Test:** `invoices_earn_and_redeem_loyalty_points`.

## Point of sale

### Closing a shift leaves the shortage on the counter

- **In Books:** when the cash counted at closing differs from what the shift expected,
  the difference is posted between Cash and the write-off account, the wrong way round,
  and the counter's cash account keeps it.
- **What it does to your books:** the counter never comes back to zero, and a shortage
  shows up as more cash in Cash.
- **In Latte:** the counter's cash goes to Cash, and a shortage or excess is posted
  between the counter and the write-off account, so the counter is cleared.
- **How we know:** from the code.
- **Test:** `a_shift_opens_sells_and_closes_with_the_counter_cleared`,
  `cash_over_is_written_back`.

### The stock check stops at the first untracked item

- **In Books:** before a POS sale is made, each row's quantity is checked against the
  stock, but the check returns at the first item that isn't tracked.
- **What it does to your books:** a sale can ship more than is in stock when an untracked
  item comes before a tracked one.
- **In Latte:** every tracked item is checked.
- **How we know:** from the code.
- **Test:** `the_cart_is_checked_as_books_checks_it`.

### Discounts of 1% and less aren't counted

- **In Books:** the POS adds up a row's discount only when its percent is more than 1.
- **What it does to your books:** the Discounts shown at the till leave out small
  discounts; the invoice itself is right.
- **In Latte:** any percent above zero counts.
- **How we know:** from the code.
- **Test:** `the_cart_is_checked_as_books_checks_it`.

### A shift's takings count payments more than once

- **In Books:** what a shift took in is a payment's whole amount for each POS invoice it
  pays, and draft and cancelled payments count too.
- **What it does to your books:** the expected amounts at closing are too high, so the
  shift looks short and the difference is written off.
- **In Latte:** each payment counts what it put against its POS invoices, once, and only
  submitted payments count.
- **How we know:** from the code.
- **Test:** `a_payment_of_two_sales_counts_once`.

### A negative closing amount is allowed

- **In Books:** the closing shift's check for negative amounts catches its own error and
  carries on.
- **What it does to your books:** a shift can be closed with a negative cash count.
- **In Latte:** it is refused.
- **How we know:** from the code.
- **Test:** `a_negative_closing_amount_is_refused`.

### Cash tendered over the total can't be paid

- **In Books:** the payment is made for all the cash tendered, and is then refused for
  being more than the invoice owes.
- **What it does to your books:** a customer paying with a larger note can't be put
  through; the invoice is submitted but left unpaid.
- **In Latte:** the payment is for the total, and the rest is shown as change.
- **How we know:** from the code.
- **Test:** none yet.

### Paying a submitted sale shows an error

- **In Books:** paying a sale taken up from the submitted list submits it again, which
  fails; the error is shown and the payment is then made anyway.
- **What it does to your books:** nothing, but the cashier is told the sale failed when it
  was paid.
- **In Latte:** a sale that is already submitted is only paid.
- **How we know:** from the code.
- **Test:** `a_saved_sale_is_taken_up_and_paid_later`.

## Data import

### A shorter file takes the template's columns

- **In Books:** a file's columns are matched to the fields the template had in those
  places, and columns past the end of the file stay matched to fields it hasn't got.
- **What it does to your books:** a file with fewer columns than the template is refused
  for duplicate columns, or imported as if it had columns it hasn't.
- **In Latte:** columns past the end of the file are for no field.
- **How we know:** from the code.
- **Test:** `items_are_imported_from_their_template`.

### Missing links are named with the wrong schema

- **In Books:** a link that doesn't exist is named with the schema being imported, so an
  invoice's missing customer is "(Sales Invoice, Acme)".
- **What it does to your books:** nothing, but the message points at the wrong thing.
- **In Latte:** it is named with the schema it links to: "(Party, Acme)".
- **How we know:** from the code.
- **Test:** `an_import_with_bad_values_or_links_is_refused`.

## Dashboard

### A period's first day is partly left out

- **In Books:** a dashboard period starts at the time of day it is now, not at midnight.
- **What it does to your books:** entries earlier in the day on the period's first day
  are left out of the cash flow, the expenses and the invoices.
- **In Latte:** a period starts at the start of its first day.
- **How we know:** from the code.
- **Test:** none yet.

## Custom fields

### A custom field's label clashes with itself

- **In Books:** a custom field's fieldname is looked for among all the form's fields, the
  one being edited included.
- **What it does to your books:** setting a custom field's label again to the same value
  is refused as a duplicate.
- **In Latte:** the field being edited isn't counted.
- **How we know:** from the code.
- **Test:** none yet.

## Get Started

### Making a purchase invoice doesn't tick "Create Purchase Invoice"

- **In Books:** the Get Started page marks "Create Purchase Invoice" done when there is a
  sales invoice, not a purchase invoice.
- **What it does to your books:** nothing, but the card is ticked by the wrong step, and
  making a purchase invoice doesn't tick it.
- **In Latte:** it is ticked once a purchase invoice is made.
- **How we know:** from the code.
- **Test:** `a_purchase_invoice_marks_its_card_done_and_a_sales_invoice_doesnt`.

## Create Demo

### The demo company's fiscal year ends on 1 April

- **In Books:** Create Demo gives the fiscal year's end as `04-01`, the same as its start,
  so the year runs from 1 April to 1 April the next year.
- **What it does to your books:** the demo company's year is a day long, and reports for
  the year take in the first day of the next one.
- **In Latte:** the year ends on 31 March, as the setup wizard sets it for India.
- **How we know:** from the code.
- **Test:** `the_demo_company_has_a_year_of_balanced_books`.

### A demo invoice can have the same item twice

- **In Books:** Create Demo means to skip an item already on the invoice, but compares the
  row's item name with the item itself, so it never finds one.
- **What it does to your books:** some demo invoices list one item on two rows.
- **In Latte:** an item already on the invoice is skipped.
- **How we know:** from the code.
- **Test:** `the_demo_company_has_a_year_of_balanced_books`.

### Demo payments are never written off

- **In Books:** one sale in twenty is meant to be paid with 15% written off, but Create
  Demo sets `writeOff`, which isn't a Payment field (it's `writeoff`).
- **What it does to your books:** the demo company has no write-offs to look at.
- **In Latte:** those payments write off 15%.
- **How we know:** from the code.
- **Test:** `the_demo_company_has_a_year_of_balanced_books`.

## Printing

### A payment that isn't for a Sales Invoice doesn't print

- **In Books:** a payment's printout takes its tax rows from the Sales Invoice it pays,
  and has none at all otherwise. The Payment template reads how many there are, which
  fails when there are none.
- **What it does to your books:** a payment made against a purchase invoice, or one not
  set against any invoice, can't be printed or saved as a PDF.
- **In Latte:** such a payment has an empty list of tax rows, and prints without them.
- **How we know:** from the code.
- **Test:** `a_payment_without_taxes_prints`.

## To fix when the app is built

### The rate box rewrites the exchange rate

- **In Books:** the exchange rate box shows the rate to two decimals and writes that back
  when you leave it, even unchanged, so 13.3333… becomes 13.33.
- **How we know:** seen in Books, 2026-10-23, comparing an invoice in EUR.
- **Plan:** the Phase 3 form keeps the rate as entered.
