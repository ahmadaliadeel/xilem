// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Printing what is on screen: snapshots of windows and widgets.

use masonry::app::RenderRoot;
use masonry::core::WidgetId;
use masonry::kurbo::{Affine, Rect, Size, Vec2};
use masonry::peniko::Color;

use crate::capture::{Capture, capture};
use crate::page::{Length, PT_PER_PX, PageSetup};
use crate::sink::ContentFilter;
use crate::writer::{ItemRole, PageItem, PdfError, PdfOptions, PdfOutput, PdfWriter};

/// What part of a window to print.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnapshotTarget {
    /// The whole window.
    Window,
    /// A widget and its descendants.
    Widget(WidgetId),
    /// The widget with the given AccessKit author id (see `PrintRegion`).
    Region(String),
}

/// How a snapshot is placed on the page.
#[derive(Clone, Debug, PartialEq)]
pub enum SnapshotPage {
    /// The page has the size of the content (at [`PT_PER_PX`]), plus a margin.
    FitContent {
        /// Margin around the content.
        margin: Length,
    },
    /// The content is scaled down (if needed) to fit the content area of a paper page,
    /// and centered horizontally.
    Paper(PageSetup),
}

/// Options for [`snapshot_to_pdf`].
#[derive(Clone, Debug)]
pub struct SnapshotOptions {
    /// What to print.
    pub target: SnapshotTarget,
    /// Page placement.
    pub page: SnapshotPage,
    /// Background color behind the content.
    pub background: Option<Color>,
    /// PDF options.
    pub pdf: PdfOptions,
}

impl Default for SnapshotOptions {
    fn default() -> Self {
        Self {
            target: SnapshotTarget::Window,
            page: SnapshotPage::FitContent {
                margin: Length::ZERO,
            },
            background: None,
            pdf: PdfOptions::default(),
        }
    }
}

/// Prints the current content of a render root (e.g. a window) to a single-page PDF.
///
/// The output is vector graphics with real text, matching what is on screen.
pub fn snapshot_to_pdf(
    root: &mut RenderRoot,
    options: &SnapshotOptions,
) -> Result<PdfOutput, PdfError> {
    let capture = capture(root).with_background(options.background);
    write_snapshot(&capture, options)
}

/// Writes a previously taken capture as a single-page PDF.
///
/// This lets you [`capture`](crate::capture) on the UI thread and encode the PDF elsewhere.
pub fn write_snapshot(capture: &Capture, options: &SnapshotOptions) -> Result<PdfOutput, PdfError> {
    let (region, filter) = match &options.target {
        SnapshotTarget::Window => (
            Rect::from_origin_size((0.0, 0.0), capture.window_size),
            ContentFilter::All,
        ),
        SnapshotTarget::Widget(id) => {
            let raw = id.to_raw();
            let bounds = capture
                .access
                .bounds(raw)
                .ok_or_else(|| PdfError::Config(format!("widget {id:?} not found")))?;
            (bounds, ContentFilter::Subtree(raw))
        }
        SnapshotTarget::Region(key) => {
            let id = capture
                .access
                .find_author_id(key)
                .ok_or_else(|| PdfError::Config(format!("print region {key:?} not found")))?;
            let bounds = capture.access.bounds(id).unwrap_or_default();
            (bounds, ContentFilter::Subtree(id))
        }
    };

    let (page_size, transform) = place(region, &options.page);
    let mut writer = PdfWriter::new(&options.pdf)?;
    let id = writer.add_capture(capture);
    writer.add_page(
        page_size,
        &[PageItem {
            capture,
            role: ItemRole::Content(id),
            filter,
            transform,
            clip: Some(region),
        }],
    );
    writer.finish()
}

/// Computes the page size and the transform from capture coordinates to the page.
fn place(region: Rect, page: &SnapshotPage) -> (Size, Affine) {
    match page {
        SnapshotPage::FitContent { margin } => {
            let margin = margin.to_pt(PT_PER_PX);
            let size = Size::new(
                region.width() * PT_PER_PX + 2.0 * margin,
                region.height() * PT_PER_PX + 2.0 * margin,
            );
            let transform = Affine::translate((margin, margin))
                * Affine::scale(PT_PER_PX)
                * Affine::translate(-region.origin().to_vec2());
            (size, transform)
        }
        SnapshotPage::Paper(setup) => {
            let content = setup.content_rect_pt();
            let scale = setup
                .pt_per_px
                .min(content.width() / region.width().max(1.0))
                .min(content.height() / region.height().max(1.0));
            let width = region.width() * scale;
            let offset = Vec2::new(content.x0 + (content.width() - width) / 2.0, content.y0);
            let transform = Affine::translate(offset)
                * Affine::scale(scale)
                * Affine::translate(-region.origin().to_vec2());
            (setup.page_size_pt(), transform)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_content_placement() {
        let (size, transform) = place(
            Rect::new(10.0, 20.0, 110.0, 70.0),
            &SnapshotPage::FitContent {
                margin: Length::Pt(5.0),
            },
        );
        assert_eq!(size, Size::new(85.0, 47.5));
        assert_eq!(
            transform * masonry::kurbo::Point::new(10.0, 20.0),
            (5.0, 5.0).into()
        );
    }

    #[test]
    fn paper_scales_down_wide_content() {
        let setup = PageSetup::a4();
        let (_, transform) = place(
            Rect::new(0.0, 0.0, 2000.0, 100.0),
            &SnapshotPage::Paper(setup),
        );
        let content = setup.content_rect_pt();
        let right = transform * masonry::kurbo::Point::new(2000.0, 0.0);
        assert!(
            (right.x - content.x1).abs() < 1e-6,
            "{right:?} vs {content:?}"
        );
    }
}
