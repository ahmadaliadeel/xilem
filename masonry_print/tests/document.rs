// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! End-to-end tests of paginated documents.

mod common;

use std::sync::Arc;

use common::{fonts, locale, out_dir};
use lopdf::{Document, Object};
use masonry::core::{DefaultProperties, NewWidget, Widget};
use masonry::properties::LayoutDirection;
use masonry::properties::types::CrossAxisAlignment;
use masonry::widgets::{Flex, Label};
use masonry_print::testing::{extract_text, page_count, rasterize};
use masonry_print::widgets::{KeepTogether, PageBreak, Semantic, SemanticRole};
use masonry_print::{
    DocumentPart, Length, NumberSystem, PageInfo, PageSetup, PaperSize, PdfOptions, PdfStandard,
    WidgetDecorations, document_properties, paginate_to_pdf, print_property_set,
};

fn setup() -> PageSetup {
    PageSetup::new(PaperSize::A5)
        .with_header(Length::Pt(14.0))
        .with_footer(Length::Pt(14.0))
}

fn properties(setup: &PageSetup) -> Arc<DefaultProperties> {
    Arc::new(document_properties(print_property_set(), setup))
}

fn footer(
    setup: PageSetup,
    numbers: NumberSystem,
) -> WidgetDecorations<
    impl FnMut(&PageInfo) -> Option<NewWidget<dyn Widget>>,
    impl FnMut(&PageInfo) -> Option<NewWidget<dyn Widget>>,
> {
    WidgetDecorations::new(
        setup,
        properties(&setup),
        fonts(),
        |_| Some(NewWidget::new(Label::new("Test document")).erased()),
        move |page: &PageInfo| {
            let text = format!(
                "Page {} of {}",
                numbers.format(page.number),
                numbers.format(page.total)
            );
            Some(NewWidget::new(Label::new(text)).erased())
        },
    )
}

fn paragraph(i: usize) -> String {
    format!("Paragraph {i}: the quick brown fox jumps over the lazy dog, again and again.")
}

fn long_document() -> NewWidget<impl Widget> {
    let mut column = Flex::column().cross_axis_alignment(CrossAxisAlignment::Start);
    column = column.with_fixed(
        NewWidget::new(Semantic::new(
            SemanticRole::Heading(1),
            Label::new("Annual report").prepare(),
        ))
        .erased(),
    );
    for i in 0..60 {
        if i == 30 {
            column = column.with_fixed(NewWidget::new(PageBreak::new()).erased());
            column = column.with_fixed(
                NewWidget::new(Semantic::new(
                    SemanticRole::Heading(2),
                    Label::new("Second part").prepare(),
                ))
                .erased(),
            );
        }
        column = column.with_fixed(Label::new(paragraph(i)).prepare());
    }
    column = column.with_fixed(
        NewWidget::new(Semantic::new(
            SemanticRole::Link("https://linebender.org".into()),
            Label::new("linebender.org").prepare(),
        ))
        .erased(),
    );
    column = column.with_fixed(
        NewWidget::new(KeepTogether::new(
            Flex::column()
                .with_fixed(Label::new("Kept line A").prepare())
                .with_fixed(Label::new("Kept line B").prepare())
                .prepare(),
        ))
        .erased(),
    );
    column.prepare()
}

fn count_struct_types(doc: &Document) -> std::collections::HashMap<String, usize> {
    let mut counts = std::collections::HashMap::new();
    for object in doc.objects.values() {
        if let Ok(dict) = object.as_dict()
            && dict.get(b"Type").ok().and_then(|t| t.as_name().ok()) == Some(b"StructElem")
            && let Ok(Object::Name(s)) = dict.get(b"S")
        {
            *counts
                .entry(String::from_utf8_lossy(s).into_owned())
                .or_insert(0) += 1;
        }
    }
    counts
}

#[test]
fn long_document_paginates_without_cutting_lines() {
    let setup = setup();
    let part = DocumentPart::flow(long_document(), &setup, properties(&setup), fonts());
    let options = PdfOptions::default()
        .with_title("Annual report")
        .with_lang("en");
    let pdf = paginate_to_pdf(
        &[part],
        &setup,
        &mut footer(setup, NumberSystem::Latin),
        &options,
    )
    .unwrap();
    pdf.save(out_dir().join("long_document.pdf")).unwrap();

    let pages = extract_text(&pdf.bytes);
    assert_eq!(pages.len(), pdf.pages);
    assert_eq!(page_count(&pdf.bytes), pdf.pages);
    assert!(pdf.pages >= 3, "expected several pages, got {}", pdf.pages);

    // Every paragraph appears exactly once, in order: lines are neither lost nor duplicated
    // at page breaks. (Paragraphs wrap, so lines are joined with spaces.)
    let body: String = pages
        .iter()
        .map(|p| {
            let lines: Vec<&str> = p.lines().collect();
            lines[1..lines.len() - 1].join(" ")
        })
        .collect::<Vec<_>>()
        .join(" ");
    let body = body.replace("  ", " ");
    let mut last_index = 0;
    for i in 0..60 {
        let text = paragraph(i);
        assert_eq!(
            body.matches(&text).count(),
            1,
            "{text:?} should appear exactly once"
        );
        let position = body.find(&text).unwrap();
        assert!(position >= last_index, "paragraphs must stay in order");
        last_index = position;
    }

    // The forced page break: "Second part" starts a page (after the header line).
    let second = pages
        .iter()
        .position(|p| p.contains("Second part"))
        .unwrap();
    assert_eq!(pages[second].lines().nth(1), Some("Second part"));
    assert!(pages[second - 1].contains("Paragraph 29:"));
    assert!(!pages[second].contains("Paragraph 29:"));

    // Kept lines are on the same page.
    let kept = pages
        .iter()
        .position(|p| p.contains("Kept line A"))
        .unwrap();
    assert!(pages[kept].contains("Kept line B"));

    // Headers and footers on every page.
    for (n, page) in pages.iter().enumerate() {
        assert_eq!(page.lines().next(), Some("Test document"));
        let expected = format!("Page {} of {}", n + 1, pdf.pages);
        assert_eq!(page.lines().last(), Some(expected.as_str()));
    }

    // Structure, links and outline.
    let doc = Document::load_mem(&pdf.bytes).unwrap();
    let counts = count_struct_types(&doc);
    assert_eq!(counts.get("H1"), Some(&1), "{counts:?}");
    assert_eq!(counts.get("H2"), Some(&1), "{counts:?}");
    assert_eq!(counts.get("Link"), Some(&1), "{counts:?}");
    assert!(counts.get("P").copied().unwrap_or(0) >= 60, "{counts:?}");
    fn has_uri(object: &Object, uri: &[u8]) -> bool {
        match object {
            Object::Dictionary(dict) => dict.iter().any(|(key, value)| {
                (key == b"URI" && matches!(value, Object::String(s, _) if s == uri))
                    || has_uri(value, uri)
            }),
            Object::Array(items) => items.iter().any(|item| has_uri(item, uri)),
            _ => false,
        }
    }
    let has_uri = doc
        .objects
        .values()
        .any(|o| has_uri(o, b"https://linebender.org"));
    assert!(has_uri, "link annotation should point to the URL");
    let catalog = doc.catalog().unwrap();
    assert!(
        catalog.get(b"Outlines").is_ok(),
        "headings should create an outline"
    );
    assert!(
        catalog.get(b"StructTreeRoot").is_ok(),
        "document should be tagged"
    );
}

#[test]
fn pdf_a_and_ua_validate() {
    let setup = setup();
    let content = Flex::column()
        .with_fixed(
            NewWidget::new(Semantic::new(
                SemanticRole::Heading(1),
                Label::new("Archive").prepare(),
            ))
            .erased(),
        )
        .with_fixed(Label::new("Archived and accessible text.").prepare())
        .with_fixed(
            NewWidget::new(
                Semantic::new(
                    SemanticRole::Paragraph,
                    Label::new("یہ اردو متن ہے")
                        .with_style(locale("ur"))
                        .prepare(),
                )
                .with_lang("ur"),
            )
            .erased(),
        )
        .prepare();
    let part = DocumentPart::flow(content, &setup, properties(&setup), fonts());
    for standard in [
        PdfStandard::A2b,
        PdfStandard::A2a,
        PdfStandard::Ua1,
        PdfStandard::A2aUa1,
    ] {
        let options = PdfOptions::default()
            .with_title("Archive")
            .with_lang("en")
            .with_standard(standard);
        let pdf = paginate_to_pdf(
            std::slice::from_ref(&part),
            &setup,
            &mut footer(setup, NumberSystem::Latin),
            &options,
        )
        .unwrap_or_else(|e| panic!("{standard:?}: {e}"));
        pdf.save(out_dir().join(format!("archive_{standard:?}.pdf")))
            .unwrap();
        let xmp = String::from_utf8_lossy(&pdf.bytes);
        if standard != PdfStandard::Ua1 {
            assert!(
                xmp.contains("pdfaid:part"),
                "{standard:?} should declare PDF/A"
            );
        }
        if matches!(standard, PdfStandard::Ua1 | PdfStandard::A2aUa1) {
            assert!(
                xmp.contains("pdfuaid:part"),
                "{standard:?} should declare PDF/UA"
            );
        }
    }
}

#[test]
fn rtl_document_is_mirrored_with_urdu_page_numbers() {
    let setup = setup().with_direction(LayoutDirection::Rtl);
    let content = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_fixed(Label::new("رسید").with_style(locale("ur")).prepare())
        .with_fixed(
            Label::new("PDF فائل تیار ہے")
                .with_style(locale("ur"))
                .prepare(),
        )
        .prepare();
    let part = DocumentPart::flow(content, &setup, properties(&setup), fonts());
    let pdf = paginate_to_pdf(
        &[part],
        &setup,
        &mut footer(setup, NumberSystem::ExtendedArabicIndic),
        &PdfOptions::default().with_lang("ur"),
    )
    .unwrap();
    pdf.save(out_dir().join("rtl_document.pdf")).unwrap();

    let text = &extract_text(&pdf.bytes)[0];
    assert!(text.contains("رسید"), "{text}");
    assert!(text.contains("۱"), "page numbers use Urdu digits: {text}");

    // With a right-to-left layout, content starts at the right margin: the leftmost ink of the
    // content area is far from the left margin.
    let raster = rasterize(&pdf.bytes, 0, 1.0);
    let content = setup.content_rect_pt();
    let (mut min_x, mut max_x) = (u32::MAX, 0);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "test"
    )]
    let (y0, y1) = (content.y0 as u32, (content.y0 + 60.0) as u32);
    for y in y0..y1 {
        for x in 0..raster.width {
            let i = ((y * raster.width + x) * 4) as usize;
            if raster.data[i] < 128 {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
            }
        }
    }
    assert!(
        f64::from(max_x) > content.x1 - 4.0,
        "RTL content should reach the right margin ({max_x} vs {})",
        content.x1
    );
    assert!(
        f64::from(min_x) > content.x0 + content.width() / 3.0,
        "RTL content should not start at the left margin ({min_x})"
    );
}

#[test]
#[ignore = "manual inspection"]
fn render_samples() {
    for name in ["rtl_document", "long_document"] {
        let bytes = std::fs::read(out_dir().join(format!("{name}.pdf"))).unwrap();
        common::save_png(&rasterize(&bytes, 0, 1.5), &format!("{name}.page0.png"));
    }
}
