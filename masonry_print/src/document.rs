// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Paginated documents.

use std::sync::Arc;

use masonry::core::{DefaultProperties, NewWidget, Widget};
use masonry::kurbo::{Affine, Rect, Size};
use masonry::peniko::Color;
use masonry::properties::{BaseDirection, ContentColor, LayoutDirection, LineBreaking};
use masonry::theme::default_property_set;
use masonry::widgets::{Flex, Grid, Label, TextArea};

use crate::capture::Capture;
use crate::fonts::FontSetup;
use crate::headless::{HeadlessOptions, HeadlessRoot};
use crate::page::PageSetup;
use crate::paginate::{PageSlice, flow_geometry, paginate};
use crate::sink::ContentFilter;
use crate::widgets::{PAGE_FLOW_CLASS, PageFlow};
use crate::writer::{ItemRole, PageItem, PdfError, PdfOptions, PdfOutput, PdfWriter};

/// Information about a page, for headers and footers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageInfo {
    /// The page number, starting at 1.
    pub number: usize,
    /// The total number of pages.
    pub total: usize,
}

/// Digits used to format numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NumberSystem {
    /// 0123456789
    #[default]
    Latin,
    /// Arabic-Indic digits, used in Arabic: ٠١٢٣٤٥٦٧٨٩
    ArabicIndic,
    /// Extended Arabic-Indic digits, used in Persian and Urdu: ۰۱۲۳۴۵۶۷۸۹
    ExtendedArabicIndic,
}

impl NumberSystem {
    /// Formats a number with these digits.
    pub fn format(self, number: usize) -> String {
        let zero = match self {
            Self::Latin => return number.to_string(),
            Self::ArabicIndic => 0x0660,
            Self::ExtendedArabicIndic => 0x06F0,
        };
        number
            .to_string()
            .chars()
            .map(|c| {
                let digit = c.to_digit(10).unwrap_or(0);
                char::from_u32(zero + digit).unwrap_or(c)
            })
            .collect()
    }
}

/// Provides page headers and footers.
pub trait PageDecorations {
    /// The header of a page, if any. It is laid out in the header band of the page.
    fn header(&mut self, page: &PageInfo) -> Option<Capture>;
    /// The footer of a page, if any. It is laid out in the footer band of the page.
    fn footer(&mut self, page: &PageInfo) -> Option<Capture>;
}

/// Text color of [`print_property_set`].
pub const PRINT_TEXT_COLOR: Color = Color::from_rgb8(0x11, 0x11, 0x11);

/// Masonry's default properties, adjusted for paper: dark text, and text that wraps.
///
/// Masonry's default theme has light text for dark backgrounds, which is unreadable on paper.
pub fn print_property_set() -> DefaultProperties {
    let mut properties = default_property_set();
    properties.insert::<Label, _>(ContentColor::new(PRINT_TEXT_COLOR));
    properties.insert::<TextArea<false>, _>(ContentColor::new(PRINT_TEXT_COLOR));
    properties.insert::<TextArea<true>, _>(ContentColor::new(PRINT_TEXT_COLOR));
    properties.insert::<Label, _>(LineBreaking::WordWrap);
    properties
}

/// Default properties adjusted for printing documents with the given page setup:
/// text wraps, and with [`LayoutDirection::Rtl`], layouts are mirrored and paragraphs are
/// right-to-left.
pub fn document_properties(
    mut properties: DefaultProperties,
    setup: &PageSetup,
) -> DefaultProperties {
    properties.insert::<Label, _>(LineBreaking::WordWrap);
    if setup.direction == LayoutDirection::Rtl {
        properties.insert::<Flex, _>(LayoutDirection::Rtl);
        properties.insert::<Grid, _>(LayoutDirection::Rtl);
        properties.insert::<Label, _>(BaseDirection::Rtl);
    }
    properties
}

/// Decorations built from Masonry widgets, laid out in their own headless roots.
pub struct WidgetDecorations<H, F> {
    setup: PageSetup,
    properties: Arc<DefaultProperties>,
    fonts: FontSetup,
    header: H,
    footer: F,
}

impl<H, F> std::fmt::Debug for WidgetDecorations<H, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WidgetDecorations")
            .field("setup", &self.setup)
            .finish_non_exhaustive()
    }
}

impl<H, F> WidgetDecorations<H, F>
where
    H: FnMut(&PageInfo) -> Option<NewWidget<dyn Widget>>,
    F: FnMut(&PageInfo) -> Option<NewWidget<dyn Widget>>,
{
    /// Creates decorations from functions building the header and footer widget of a page.
    pub fn new(
        setup: PageSetup,
        properties: Arc<DefaultProperties>,
        fonts: FontSetup,
        header: H,
        footer: F,
    ) -> Self {
        Self {
            setup,
            properties,
            fonts,
            header,
            footer,
        }
    }

    fn capture(&self, widget: NewWidget<dyn Widget>, band: Rect) -> Capture {
        let size = self.setup.pt_to_px(band.size());
        let options = HeadlessOptions {
            size,
            scale_factor: 1.0,
            default_properties: self.properties.clone(),
            fonts: self.fonts.clone(),
        };
        HeadlessRoot::new(widget, options).capture()
    }
}

impl<H, F> PageDecorations for WidgetDecorations<H, F>
where
    H: FnMut(&PageInfo) -> Option<NewWidget<dyn Widget>>,
    F: FnMut(&PageInfo) -> Option<NewWidget<dyn Widget>>,
{
    fn header(&mut self, page: &PageInfo) -> Option<Capture> {
        let widget = (self.header)(page)?;
        Some(self.capture(widget, self.setup.header_rect_pt()))
    }

    fn footer(&mut self, page: &PageInfo) -> Option<Capture> {
        let widget = (self.footer)(page)?;
        Some(self.capture(widget, self.setup.footer_rect_pt()))
    }
}

/// No headers or footers.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoDecorations;

impl PageDecorations for NoDecorations {
    fn header(&mut self, _page: &PageInfo) -> Option<Capture> {
        None
    }

    fn footer(&mut self, _page: &PageInfo) -> Option<Capture> {
        None
    }
}

/// A part of a document.
#[derive(Debug)]
pub enum DocumentPart {
    /// Flowing content, split across as many pages as needed.
    ///
    /// Build it with [`DocumentPart::flow`].
    Flow(Capture),
    /// A single page; the content is scaled down if it doesn't fit.
    Page(Capture),
}

impl DocumentPart {
    /// Lays out `widget` as flowing content for the given page setup.
    pub fn flow(
        widget: NewWidget<impl Widget + ?Sized>,
        setup: &PageSetup,
        properties: Arc<DefaultProperties>,
        fonts: FontSetup,
    ) -> Self {
        let content = setup.pt_to_px(setup.content_rect_pt().size());
        let flow = PageFlow::new(content.width, widget);
        let mut root = HeadlessRoot::new(
            NewWidget::new(flow),
            HeadlessOptions {
                size: content,
                scale_factor: 1.0,
                default_properties: properties,
                fonts,
            },
        );
        Self::Flow(root.capture())
    }

    /// Lays out `widget` as a single page for the given page setup.
    pub fn page(
        widget: NewWidget<impl Widget + ?Sized>,
        setup: &PageSetup,
        properties: Arc<DefaultProperties>,
        fonts: FontSetup,
    ) -> Self {
        let content = setup.pt_to_px(setup.content_rect_pt().size());
        let mut root = HeadlessRoot::new(
            widget,
            HeadlessOptions {
                size: content,
                scale_factor: 1.0,
                default_properties: properties,
                fonts,
            },
        );
        Self::Page(root.capture())
    }

    /// Computes the page slices of this part.
    fn slices(&self, setup: &PageSetup) -> Vec<PageSlice> {
        let content = setup.pt_to_px(setup.content_rect_pt().size());
        match self {
            Self::Flow(capture) => {
                let access = &capture.access;
                let Some(flow) = access.find_class(PAGE_FLOW_CLASS).into_iter().next() else {
                    return vec![PageSlice {
                        y0: 0.0,
                        y1: capture.content_bounds.height(),
                    }];
                };
                let bounds = access.bounds(flow).unwrap_or_default();
                // The content of the flow may be taller than the flow widget itself.
                let height = access
                    .children(flow)
                    .filter_map(|child| access.bounds(child))
                    .fold(bounds.height(), |h, b| h.max(b.y1));
                paginate(&flow_geometry(access, flow, height), content.height)
            }
            Self::Page(capture) => vec![PageSlice {
                y0: 0.0,
                y1: capture.window_size.height,
            }],
        }
    }

    fn capture(&self) -> &Capture {
        match self {
            Self::Flow(capture) | Self::Page(capture) => capture,
        }
    }
}

/// Writes a paginated document.
///
/// Each [`DocumentPart::Flow`] is split into as many pages as needed, never cutting a line of
/// text; each [`DocumentPart::Page`] becomes one page. `decorations` provides headers and
/// footers, which are marked as artifacts in tagged PDF.
pub fn paginate_to_pdf(
    parts: &[DocumentPart],
    setup: &PageSetup,
    decorations: &mut dyn PageDecorations,
    options: &PdfOptions,
) -> Result<PdfOutput, PdfError> {
    let slices: Vec<Vec<PageSlice>> = parts.iter().map(|part| part.slices(setup)).collect();
    let total: usize = slices.iter().map(Vec::len).sum();
    let page_size = setup.page_size_pt();
    let content = setup.content_rect_pt();
    let content_px = setup.pt_to_px(content.size());
    let scale = setup.pt_per_px;

    let mut writer = PdfWriter::new(options)?;
    let mut number = 0;
    for (part, slices) in parts.iter().zip(&slices) {
        let capture = part.capture();
        let id = writer.add_capture(capture);
        for slice in slices {
            number += 1;
            let info = PageInfo { number, total };
            let (transform, filter, clip) = match part {
                DocumentPart::Flow(_) => (
                    Affine::translate((content.x0, content.y0))
                        * Affine::scale(scale)
                        * Affine::translate((0.0, -slice.y0)),
                    ContentFilter::Slice {
                        y0: slice.y0,
                        y1: slice.y1,
                    },
                    Rect::new(0.0, slice.y0, content_px.width, slice.y1),
                ),
                DocumentPart::Page(capture) => {
                    let size = capture.window_size;
                    let fit = scale
                        .min(content.width() / size.width.max(1.0))
                        .min(content.height() / size.height.max(1.0));
                    (
                        Affine::translate((content.x0, content.y0)) * Affine::scale(fit),
                        ContentFilter::All,
                        Rect::from_origin_size((0.0, 0.0), size),
                    )
                }
            };
            let header = decorations.header(&info);
            let footer = decorations.footer(&info);
            let mut items = vec![PageItem {
                capture,
                role: ItemRole::Content(id),
                filter,
                transform,
                clip: Some(clip),
            }];
            for (band, decoration) in [
                (setup.header_rect_pt(), header.as_ref()),
                (setup.footer_rect_pt(), footer.as_ref()),
            ] {
                let Some(decoration) = decoration else {
                    continue;
                };
                items.push(PageItem {
                    capture: decoration,
                    role: ItemRole::Artifact,
                    filter: ContentFilter::All,
                    transform: Affine::translate((band.x0, band.y0)) * Affine::scale(scale),
                    clip: Some(Rect::from_origin_size(
                        (0.0, 0.0),
                        Size::new(band.width() / scale, band.height() / scale),
                    )),
                });
            }
            writer.add_page(page_size, &items);
        }
    }
    writer.finish()
}

#[cfg(test)]
mod tests {
    use super::NumberSystem;

    #[test]
    fn number_systems() {
        assert_eq!(NumberSystem::Latin.format(1204), "1204");
        assert_eq!(NumberSystem::ArabicIndic.format(1204), "١٢٠٤");
        assert_eq!(NumberSystem::ExtendedArabicIndic.format(1204), "۱۲۰۴");
    }
}
