// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! A bilingual (English / Urdu) invoice, shared by the examples.

use xilem::masonry::layout::AsUnit;
use xilem::masonry::properties::{BaseDirection, LayoutDirection};
use xilem::peniko::Color;
use xilem::style::Style as _;
use xilem::view::{CrossAxisAlignment, FlexSpacer, flex_col, flex_row, label};
use xilem::{AnyWidgetView, WidgetView};
use xilem_print::{
    heading, keep_together, lang, link, table, table_cell, table_header_cell, table_row,
};

/// Font for Urdu text (bundled with `xilem_print`).
pub(crate) const URDU_FONT: &str = "Noto Nastaliq Urdu";
/// Font for Arabic text (bundled with `xilem_print`).
pub(crate) const ARABIC_FONT: &str = "Noto Naskh Arabic";
/// Text color: the invoice is designed for white paper.
pub(crate) const INK: Color = Color::from_rgb8(0x1a, 0x1a, 0x1a);
/// Link color.
const LINK: Color = Color::from_rgb8(0x1a, 0x5f, 0xb4);

/// A line item.
#[derive(Clone, Debug)]
pub(crate) struct Item {
    pub(crate) name: &'static str,
    pub(crate) name_ur: &'static str,
    pub(crate) quantity: u32,
    pub(crate) price: u32,
}

/// The data of an invoice.
#[derive(Clone, Debug)]
pub(crate) struct InvoiceData {
    pub(crate) number: u32,
    pub(crate) customer: &'static str,
    pub(crate) customer_ur: &'static str,
    pub(crate) items: Vec<Item>,
    /// Lay the invoice out right-to-left.
    pub(crate) rtl: bool,
}

impl InvoiceData {
    pub(crate) fn sample() -> Self {
        let item = |name, name_ur, quantity, price| Item {
            name,
            name_ur,
            quantity,
            price,
        };
        Self {
            number: 1024,
            customer: "Ayesha Khan",
            customer_ur: "عائشہ خان",
            items: vec![
                item("Green tea", "سبز چائے", 3, 450),
                item("Notebook", "کاپی", 10, 120),
                item("Fountain pen", "فاؤنٹین پین", 1, 2_500),
                item("Urdu dictionary", "اردو لغت", 1, 1_800),
            ],
            rtl: false,
        }
    }

    pub(crate) fn total(&self) -> u32 {
        self.items.iter().map(|i| i.quantity * i.price).sum()
    }
}

/// Formats an amount in rupees.
pub(crate) fn rupees(amount: u32) -> String {
    let digits = amount.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    format!("Rs {out}")
}

/// Directions of an invoice.
#[derive(Clone, Copy)]
struct Dirs {
    layout: LayoutDirection,
    text: BaseDirection,
}

fn text<State: 'static>(dirs: Dirs, s: impl Into<String>) -> Box<AnyWidgetView<State>> {
    label(s.into()).color(INK).prop(dirs.text).boxed()
}

fn urdu<State: 'static>(dirs: Dirs, s: impl Into<String>) -> Box<AnyWidgetView<State>> {
    lang(
        "ur",
        label(s.into()).font(URDU_FONT).color(INK).prop(dirs.text),
    )
    .boxed()
}

fn cell<State: 'static>(
    width: f64,
    header: bool,
    content: Box<AnyWidgetView<State>>,
) -> Box<AnyWidgetView<State>> {
    if header {
        table_header_cell(content).width(width.px()).boxed()
    } else {
        table_cell(content).width(width.px()).boxed()
    }
}

const WIDTHS: [f64; 5] = [170., 150., 50., 90., 100.];

fn row<State: 'static>(
    dirs: Dirs,
    header: bool,
    cells: [Box<AnyWidgetView<State>>; 5],
) -> Box<AnyWidgetView<State>> {
    let cells: Vec<_> = cells
        .into_iter()
        .zip(WIDTHS)
        .map(|(content, width)| cell(width, header, content))
        .collect();
    table_row(flex_row(cells).prop(dirs.layout)).boxed()
}

fn items_table<State: 'static>(dirs: Dirs, data: &InvoiceData) -> Box<AnyWidgetView<State>> {
    let mut rows = vec![row(
        dirs,
        true,
        [
            text(dirs, "Item"),
            urdu(dirs, "آئٹم"),
            text(dirs, "Qty"),
            text(dirs, "Price"),
            text(dirs, "Amount"),
        ],
    )];
    for item in &data.items {
        rows.push(row(
            dirs,
            false,
            [
                text(dirs, item.name),
                urdu(dirs, item.name_ur),
                text(dirs, item.quantity.to_string()),
                text(dirs, rupees(item.price)),
                text(dirs, rupees(item.quantity * item.price)),
            ],
        ));
    }
    table(
        flex_col(rows)
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .prop(dirs.layout),
    )
    .boxed()
}

fn header_section<State: 'static>(dirs: Dirs, data: &InvoiceData) -> Box<AnyWidgetView<State>> {
    let items: Vec<Box<AnyWidgetView<State>>> = vec![
        heading(
            1,
            label("Invoice").text_size(28.).color(INK).prop(dirs.text),
        )
        .boxed(),
        urdu(dirs, "رسید"),
        flex_row((
            text(dirs, format!("Invoice #{}", data.number)),
            FlexSpacer::Flex(1.),
            text(dirs, "Date: 2026-09-29"),
        ))
        .prop(dirs.layout)
        .boxed(),
        text(dirs, format!("Bill to: {}", data.customer)),
        urdu(dirs, format!("بنام: {}", data.customer_ur)),
    ];
    flex_col(items)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .prop(dirs.layout)
        .boxed()
}

fn closing_section<State: 'static>(dirs: Dirs, data: &InvoiceData) -> Box<AnyWidgetView<State>> {
    let items: Vec<Box<AnyWidgetView<State>>> = vec![
        label(format!("Total: {}", rupees(data.total())))
            .text_size(18.)
            .color(INK)
            .prop(dirs.text)
            .boxed(),
        urdu(
            dirs,
            "آپ کی خریداری کا شکریہ! یہ رسید کمپیوٹر سے تیار کی گئی ہے۔",
        ),
        lang(
            "ar",
            label("شكرًا لتسوقكم معنا")
                .font(ARABIC_FONT)
                .color(INK)
                .prop(dirs.text),
        )
        .boxed(),
        link(
            "https://github.com/linebender/xilem",
            label("Made with Xilem: github.com/linebender/xilem")
                .color(LINK)
                .prop(dirs.text),
        )
        .boxed(),
    ];
    keep_together(
        flex_col(items)
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .prop(dirs.layout),
    )
    .boxed()
}

/// The invoice. With `data.rtl`, rows are mirrored and paragraphs are right-to-left.
pub(crate) fn invoice_view<State: 'static>(data: &InvoiceData) -> Box<AnyWidgetView<State>> {
    let dirs = if data.rtl {
        Dirs {
            layout: LayoutDirection::Rtl,
            text: BaseDirection::Rtl,
        }
    } else {
        Dirs {
            layout: LayoutDirection::Ltr,
            text: BaseDirection::Auto,
        }
    };
    let sections = vec![
        header_section(dirs, data),
        items_table(dirs, data),
        closing_section(dirs, data),
    ];
    flex_col(sections)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .prop(dirs.layout)
        .padding(16.px())
        .boxed()
}
