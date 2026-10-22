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

## To fix when the app is built

### The rate box rewrites the exchange rate

- **In Books:** the exchange rate box shows the rate to two decimals and writes that back
  when you leave it, even unchanged, so 13.3333… becomes 13.33.
- **How we know:** seen in Books, 2026-10-23, comparing an invoice in EUR.
- **Plan:** the Phase 3 form keeps the rate as entered.
