# Xilem Print

Printing and vector PDF export for Xilem apps, built on [`masonry_print`](../masonry_print).

```rust,no_run
use xilem::view::{flex_col, label};
use xilem_print::{document, heading, page_break, PageSetup};

struct State;
let pdf = document::<State>(PageSetup::a4())
    .title("Report")
    .lang("en")
    .flow(flex_col((
        heading(1, label("Report")),
        label("First chapter"),
        page_break(),
        label("Second chapter"),
    )))
    .footer(|_, page| label(format!("Page {} of {}", page.number, page.total)))
    .render(&mut State)
    .unwrap();
pdf.save("report.pdf").unwrap();
```

- `document(...)`: paginated documents from Xilem views, with headers and footers.
- `run_with_printing` + `request_print(PrintJob::window())`: print a running app's window, a
  widget or a `print_region`, as it looks on screen.
- Semantic views: `heading`, `link`, `paragraph`, `figure`, `table`, `table_row`, `table_cell`,
  `list`, `list_item`, `lang`, `artifact`, and `keep_together`, `page_break`, `print_region`.
- `os_print`: send PDFs to the operating system's printers.

Output is vector PDF with real text (including Arabic and Urdu, copyable in logical order),
tagged for accessibility, with links and bookmarks, optionally conforming to PDF/A and PDF/UA.

## Examples

```sh
# Writes an English and a right-to-left Urdu invoice (PDF/A-2a + PDF/UA-1) to a directory.
cargo run -p xilem_print --example headless_invoice -- out

# An app that saves its window, part of its window or an A4 document as PDF, and prints.
cargo run -p xilem_print --example invoice
```
