// Tests of the importer.

use latte_db::DocValue;

use super::{Importer, parse_csv};
use crate::setup_tests::{TempFile, indian_company};
use crate::tests::{map, text};

#[test]
fn csv_is_read_as_books_reads_it() {
    assert_eq!(
        parse_csv("a,\"b, c\",\"say \"\"hi\"\"\"\r\n1,\"two\r\nlines\",3"),
        vec![
            vec!["a", "b, c", "say \"hi\""],
            vec!["1", "two\r\nlines", "3"],
        ]
    );
    // Without CRLFs, the lines are split on LF.
    assert_eq!(
        parse_csv("a,b\n1,2\n"),
        vec![vec!["a", "b"], vec!["1", "2"]]
    );
}

#[test]
fn items_are_imported_from_their_template() {
    let file = TempFile::new("import-items");
    let mut fyo = indian_company(&file);
    let importer = Importer::new("Item", &fyo).unwrap();
    let template = importer.csv_template();
    let rows = parse_csv(&template);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[2][0], "Item.name");

    // The template's three rows, with only some of its columns, then two items.
    let csv = [
        "Item,Item,Item,Item,Item,Item,Item",
        "Item Name,Rate,Type,For,Sales Acc.,Purchase Acc.,Unit Type",
        "Item.name,Item.rate,Item.itemType,Item.for,Item.incomeAccount,Item.expenseAccount,Item.unit",
        "Bolt,12.5,Product,Both,Sales,Cost of Goods Sold,Unit",
        "Installation,500,Service,Sales,Service,Cost of Goods Sold,Unit",
    ]
    .join("\r\n");
    let mut importer = Importer::new("Item", &fyo).unwrap();
    assert!(importer.select_file(&fyo, &csv));
    assert_eq!(importer.rows.len(), 2);
    assert_eq!(importer.assigned.len(), 7);
    assert_eq!(importer.check(&fyo).unwrap(), None);

    let result = importer.import(&mut fyo, false).unwrap();
    assert!(result.failed.is_empty(), "{:?}", result.failed);
    assert_eq!(result.success, vec!["Bolt", "Installation"]);
    let bolt = fyo.get_doc("Item", Some("Bolt")).unwrap();
    assert_eq!(
        bolt.get("rate"),
        Some(&DocValue::Money(fyo.db.pesa().from_f64(12.5).unwrap()))
    );
    assert_eq!(
        fyo.get_doc("Item", Some("Installation"))
            .unwrap()
            .get("itemType"),
        Some(&text("Service"))
    );
}

#[test]
fn an_import_with_bad_values_or_links_is_refused() {
    let file = TempFile::new("import-checks");
    let fyo = indian_company(&file);

    // No row of keys: the columns are picked by hand.
    let mut importer = Importer::new("Item", &fyo).unwrap();
    importer.select_file(
        &fyo,
        "Bolt,abc,Gadget,Both,Sales,Cost of Goods Sold\r\n\
         Nut,2,Product,Both,Sales,Cost of Goods Sold",
    );
    assert_eq!(importer.assigned, vec![None; 6]);
    assert_eq!(
        importer.error_message().unwrap(),
        "Required fields not selected: Item Name, Purpose, Sales Acc., Purchase Acc."
    );
    let keys = [
        "Item.name",
        "Item.rate",
        "Item.itemType",
        "Item.for",
        "Item.incomeAccount",
        "Item.expenseAccount",
    ];
    for (i, key) in keys.into_iter().enumerate() {
        importer.set_template_field(&fyo, i, Some(key.into()));
    }
    assert_eq!(
        importer.check(&fyo).unwrap().unwrap(),
        "Following cells have errors: (Row 1, Rate), (Row 1, Type)."
    );
    importer.set_cell(&fyo, 0, 1, "1");
    importer.set_cell(&fyo, 0, 2, "Product");
    assert_eq!(importer.check(&fyo).unwrap(), None);

    // The same column twice.
    importer.set_template_field(&fyo, 2, Some("Item.rate".into()));
    assert_eq!(
        importer.error_message().unwrap(),
        "Duplicate columns found: Rate"
    );

    // An invoice for a customer and an item that don't exist.
    let mut importer = Importer::new("SalesInvoice", &fyo).unwrap();
    let csv = [
        "SalesInvoice.name,SalesInvoice.numberSeries,SalesInvoice.date,SalesInvoice.party,\
         SalesInvoice.account,SalesInvoiceItem.item,SalesInvoiceItem.quantity",
        "SINV-9001,SINV-,2026-10-01,Nobody,Debtors,Widget,1",
    ]
    .join("\r\n");
    importer.select_file(&fyo, &csv);
    assert_eq!(
        importer.check(&fyo).unwrap().unwrap(),
        "Following links do not exist: (Party, Nobody), (Item, Widget)."
    );
}

#[test]
fn an_invoice_is_imported_with_its_rows_and_submitted() {
    let file = TempFile::new("import-invoice");
    let mut fyo = indian_company(&file);
    let values = map([("name", text("Widget")), ("rate", DocValue::Number(100.0))]);
    fyo.get_new_doc("Item", values)
        .unwrap()
        .sync(&mut fyo)
        .unwrap();
    let values = map([("name", text("Acme")), ("role", text("Customer"))]);
    fyo.get_new_doc("Party", values)
        .unwrap()
        .sync(&mut fyo)
        .unwrap();

    let mut importer = Importer::new("SalesInvoice", &fyo).unwrap();
    assert!(importer.has_child_tables);
    // Two rows of one invoice, then a second invoice.
    let csv = [
        "SalesInvoice.name,SalesInvoice.numberSeries,SalesInvoice.date,SalesInvoice.party,\
         SalesInvoice.account,SalesInvoiceItem.item,SalesInvoiceItem.quantity,\
         SalesInvoiceItem.rate",
        "SINV-9001,SINV-,2026-10-01,Acme,Debtors,Widget,1,100",
        "SINV-9001,SINV-,2026-10-01,Acme,Debtors,Widget,2,90",
        "SINV-9002,SINV-,2026-10-02,Acme,Debtors,Widget,3,100",
    ]
    .join("\r\n");
    importer.select_file(&fyo, &csv);
    assert_eq!(importer.check(&fyo).unwrap(), None);
    let result = importer.import(&mut fyo, true).unwrap();
    assert!(result.failed.is_empty(), "{:?}", result.failed);
    // The file's names only group the rows; each invoice is named by its series.
    assert_eq!(result.success, vec!["SINV-1001", "SINV-1002"]);
    assert_eq!(result.success_old_names, vec!["SINV-9001", "SINV-9002"]);

    let invoice = fyo.get_doc("SalesInvoice", Some("SINV-1001")).unwrap();
    assert!(invoice.is_submitted());
    assert_eq!(invoice.table("items").len(), 2);
    assert_eq!(
        invoice.get("grandTotal"),
        Some(&DocValue::Money(fyo.db.pesa().from_f64(280.0).unwrap()))
    );

    // Rows whose invoice was made are cleared.
    importer.keep_failed(&result);
    assert!(importer.rows.is_empty());
}
