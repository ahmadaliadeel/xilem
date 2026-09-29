// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! End-to-end tests of printing on-screen content (snapshots).

mod common;

use std::sync::Arc;

use common::{assert_pixel_parity, fonts, locale, out_dir};
use masonry::core::{NewWidget, Widget, WidgetTag};
use masonry::kurbo::Size;
use masonry::peniko::Color;
use masonry::properties::{BaseDirection, LineBreaking};
use masonry::theme::default_property_set;
use masonry::widgets::{Flex, Label};
use masonry_print::testing::extract_text;
use masonry_print::{
    HeadlessOptions, HeadlessRoot, PdfOptions, SnapshotOptions, SnapshotTarget, capture,
    write_snapshot,
};

const ARABIC: &str = "مرحبا بالعالم";
const URDU: &str = "یہ اردو متن ہے";
const MIXED: &str = "Invoice رقم 123";

fn headless(widget: NewWidget<impl Widget>, size: Size) -> HeadlessRoot {
    let mut props = default_property_set();
    props.insert::<Label, _>(LineBreaking::WordWrap);
    HeadlessRoot::new(
        widget,
        HeadlessOptions::new(size, Arc::new(props)).with_fonts(fonts()),
    )
}

fn multilingual() -> NewWidget<impl Widget> {
    Flex::column()
        .with_fixed(Label::new("Hello, printed world!").prepare())
        .with_fixed(Label::new(ARABIC).with_style(locale("ar")).prepare())
        .with_fixed(Label::new(URDU).with_style(locale("ur")).prepare())
        .with_fixed(Label::new(MIXED).prepare())
        .prepare()
}

#[test]
fn window_snapshot_has_real_multilingual_text() {
    let mut root = headless(multilingual(), Size::new(360., 200.));
    let background = Color::from_rgb8(0x20, 0x22, 0x28);
    let capture = capture(root.render_root()).with_background(Some(background));
    let options = SnapshotOptions {
        pdf: PdfOptions::default().with_title("Snapshot").with_lang("en"),
        ..SnapshotOptions::default()
    };
    let pdf = write_snapshot(&capture, &options).unwrap();
    pdf.save(out_dir().join("window_snapshot.pdf")).unwrap();

    assert_eq!(pdf.pages, 1);
    assert_eq!(
        pdf.stats.glyph_runs_unmapped, 0,
        "all text should carry its source"
    );
    assert_eq!(
        pdf.stats.glyph_runs_outlined, 0,
        "text must not be outlined"
    );
    assert!(pdf.stats.glyph_runs_text >= 4);

    let text = &extract_text(&pdf.bytes)[0];
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "Hello, printed world!");
    assert_eq!(lines[1], ARABIC, "Arabic must extract in logical order");
    assert_eq!(lines[2], URDU, "Urdu must extract in logical order");
    assert_eq!(
        lines[3], MIXED,
        "mixed bidi text must extract in logical order"
    );

    assert_pixel_parity("window_snapshot", &capture, &pdf.bytes, background);
}

#[test]
fn widget_snapshot_only_prints_that_widget() {
    let tag = WidgetTag::named("urdu");
    let widget = Flex::column()
        .with_fixed(Label::new("Not printed").prepare())
        .with_fixed(
            NewWidget::new(Label::new(URDU).with_style(locale("ur")))
                .with_tag(tag)
                .with_props(BaseDirection::Rtl),
        )
        .prepare();
    let mut root = headless(widget, Size::new(300., 120.));
    let id = root.render_root().get_widget_with_tag(tag).unwrap().id();
    let capture = capture(root.render_root());
    let options = SnapshotOptions {
        target: SnapshotTarget::Widget(id),
        pdf: PdfOptions {
            compress: false,
            ..PdfOptions::default()
        },
        ..SnapshotOptions::default()
    };
    let pdf = write_snapshot(&capture, &options).unwrap();
    pdf.save(out_dir().join("widget_snapshot.pdf")).unwrap();
    let text = &extract_text(&pdf.bytes)[0];
    assert_eq!(text, URDU);
}

#[test]
fn scrolled_out_content_is_not_printed() {
    use masonry::kurbo::Point;
    use masonry::widgets::Portal;

    let tag = WidgetTag::named("portal");
    let mut column = Flex::column();
    for i in 0..30 {
        column = column.with_fixed(Label::new(format!("Row {i}")).prepare());
    }
    let portal = NewWidget::new(Portal::new(column.prepare())).with_tag(tag);
    let mut root = headless(portal, Size::new(200., 100.));
    root.render_root().edit_widget_with_tag(tag, |mut portal| {
        Portal::set_viewport_pos(&mut portal, Point::new(0., 300.));
    });
    let capture = capture(root.render_root());
    let pdf = write_snapshot(&capture, &SnapshotOptions::default()).unwrap();
    let text = &extract_text(&pdf.bytes)[0];
    assert!(
        !text.contains("Row 0\n"),
        "scrolled out rows must not be in the PDF: {text}"
    );
    assert!(
        !text.contains("Row 29"),
        "rows below the viewport must not be in the PDF: {text}"
    );
    assert!(text.lines().count() < 10, "only visible rows: {text}");
}

#[test]
fn svg_widgets_are_printed_as_vectors() {
    use imaging_krilla::usvg;
    use masonry::layout::AsUnit;
    use masonry::properties::Dimensions;
    use masonry::widgets::Svg;

    let xml = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
        <circle cx="50" cy="50" r="40" fill="#2060d0"/>
        <rect x="10" y="10" width="30" height="20" fill="#d03030"/>
    </svg>"##;
    let tree = usvg::Tree::from_str(xml, &usvg::Options::default()).unwrap();
    let svg = Svg::new(Arc::new(tree))
        .prepare()
        .with_props(Dimensions::fixed(200.px(), 200.px()));
    let mut root = headless(
        Flex::column().with_fixed(svg).prepare(),
        Size::new(220., 220.),
    );
    let background = Color::WHITE;
    let capture = capture(root.render_root()).with_background(Some(background));
    assert_eq!(capture.svgs.len(), 1);
    let pdf = write_snapshot(&capture, &SnapshotOptions::default()).unwrap();
    pdf.save(out_dir().join("svg_snapshot.pdf")).unwrap();
    let has_image = pdf
        .bytes
        .windows(b"/Subtype/Image".len())
        .any(|w| w == b"/Subtype/Image");
    assert!(
        !has_image,
        "the SVG should be drawn as vector paths, not as an image"
    );
    assert_pixel_parity("svg_snapshot", &capture, &pdf.bytes, background);
}

#[test]
#[ignore = "queries the system's printers"]
fn list_system_printers() {
    let printers = masonry_print::os_print::list_printers().unwrap();
    for printer in &printers {
        eprintln!(
            "{} {}",
            if printer.is_default { "*" } else { " " },
            printer.name
        );
    }
    assert!(!printers.is_empty(), "expected at least one printer");
}
