// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Tests of rendering Xilem views to PDF documents.

use std::sync::Arc;

use masonry_print::testing::extract_text;
use xilem::masonry::properties::LayoutDirection;
use xilem::peniko::Blob;
use xilem::view::{flex_col, flex_row, label};
use xilem::{WidgetView, kurbo::Size};
use xilem_print::{
    FontFallback, FontSetup, Length, PageSetup, PdfStandard, SnapshotOptions, capture_view,
    document, heading, link, page_break, table, table_cell, table_header_cell, table_row,
};

fn fonts() -> FontSetup {
    FontSetup::only([Blob::new(Arc::new(masonry_testing::ROBOTO))])
        .with_fallback(FontFallback::new(*b"Latn", &["Roboto"]))
        .with_fallback(FontFallback::new(*b"Zyyy", &["Roboto"]))
        .with_noto_arabic()
}

struct Invoice {
    customer: String,
    items: Vec<(String, u32)>,
}

fn invoice() -> Invoice {
    Invoice {
        customer: "Ayesha Khan".into(),
        items: vec![("Tea".into(), 3), ("چائے".into(), 5), ("Books".into(), 12)],
    }
}

fn items_table(state: &Invoice) -> impl WidgetView<Invoice> + use<> {
    let rows: Vec<_> = state
        .items
        .iter()
        .map(|(name, price)| {
            table_row(flex_row((
                table_cell(label(name.clone())),
                table_cell(label(price.to_string())),
            )))
        })
        .collect();
    table(flex_col((
        table_row(flex_row((
            table_header_cell(label("Item")),
            table_header_cell(label("Price")),
        ))),
        flex_col(rows),
    )))
}

#[test]
fn invoice_document() {
    let mut state = invoice();
    let body = flex_col((
        heading(1, label("Invoice")),
        label(format!("Customer: {}", state.customer)),
        items_table(&state),
        label("شکریہ! یہ رسید آپ کے لیے ہے۔").font("Noto Nastaliq Urdu"),
        link("https://linebender.org", label("linebender.org")),
        page_break(),
        heading(2, label("Terms")),
        label("Payment is due within 30 days."),
    ));
    let pdf = document::<Invoice>(PageSetup::a4().with_footer(Length::Pt(14.0)))
        .title("Invoice")
        .lang("en")
        .fonts(fonts())
        .flow(body)
        .footer(|state: &mut Invoice, page| {
            label(format!(
                "{} · Page {} of {}",
                state.customer, page.number, page.total
            ))
        })
        .standard(PdfStandard::A2aUa1)
        .render(&mut state)
        .unwrap();
    let out =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/xilem_print_tests");
    std::fs::create_dir_all(&out).unwrap();
    pdf.save(out.join("invoice.pdf")).unwrap();

    assert_eq!(pdf.pages, 2);
    assert_eq!(pdf.stats.glyph_runs_unmapped, 0);
    let pages = extract_text(&pdf.bytes);
    assert!(
        pages[0].starts_with("Invoice\nCustomer: Ayesha Khan"),
        "{}",
        pages[0]
    );
    assert!(pages[0].contains("چائے"), "{}", pages[0]);
    assert!(
        pages[0].contains("شکریہ! یہ رسید آپ کے لیے ہے۔"),
        "{}",
        pages[0]
    );
    assert!(
        pages[0].ends_with("Ayesha Khan · Page 1 of 2"),
        "{}",
        pages[0]
    );
    assert!(pages[1].starts_with("Terms"), "{}", pages[1]);
}

#[test]
fn rtl_document_from_views() {
    let mut state = invoice();
    let pdf = document::<Invoice>(PageSetup::a4().with_direction(LayoutDirection::Rtl))
        .lang("ur")
        .fonts(fonts())
        .flow(flex_col((
            label("رسید").font("Noto Nastaliq Urdu"),
            items_table(&state),
        )))
        .render(&mut state)
        .unwrap();
    let text = &extract_text(&pdf.bytes)[0];
    assert!(text.starts_with("رسید"), "{text}");
}

#[test]
fn capture_view_snapshot() {
    let mut state = invoice();
    let capture = capture_view(
        &mut state,
        flex_col((label("Snapshot of a view"), items_table(&invoice()))),
        Size::new(300.0, 160.0),
        masonry_print::print_property_set(),
        fonts(),
    );
    let pdf = masonry_print::write_snapshot(&capture, &SnapshotOptions::default()).unwrap();
    let text = &extract_text(&pdf.bytes)[0];
    assert!(text.starts_with("Snapshot of a view\nItem"), "{text}");
}

#[test]
#[ignore = "manual inspection"]
fn render_invoice_png() {
    let out =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/xilem_print_tests");
    let bytes = std::fs::read(out.join("invoice.pdf")).unwrap();
    let r = masonry_print::testing::rasterize(&bytes, 0, 1.2);
    let img = image::RgbaImage::from_raw(r.width, r.height, r.data).unwrap();
    img.save(out.join("invoice.page0.png")).unwrap();
}
