// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Paper sizes, margins and other page setup.

use masonry::kurbo::{Rect, Size};
use masonry::properties::LayoutDirection;

/// Default conversion factor from logical pixels to PDF points.
///
/// This follows the CSS convention of 96 logical pixels per inch (72 points per inch),
/// so a 16px label prints at 12pt.
pub const PT_PER_PX: f64 = 0.75;

/// A length, used for paper sizes and margins.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    /// PDF points (1/72 inch).
    Pt(f64),
    /// Millimeters.
    Mm(f64),
    /// Inches.
    In(f64),
    /// Logical pixels (see [`PageSetup::pt_per_px`]).
    Px(f64),
}

impl Length {
    /// Zero length.
    pub const ZERO: Self = Self::Pt(0.0);

    /// Converts this length to PDF points.
    pub fn to_pt(self, pt_per_px: f64) -> f64 {
        match self {
            Self::Pt(v) => v,
            Self::Mm(v) => v * 72.0 / 25.4,
            Self::In(v) => v * 72.0,
            Self::Px(v) => v * pt_per_px,
        }
    }

    /// Converts this length to logical pixels.
    pub fn to_px(self, pt_per_px: f64) -> f64 {
        self.to_pt(pt_per_px) / pt_per_px
    }
}

/// A paper size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PaperSize {
    /// ISO A3 (297 × 420 mm).
    A3,
    /// ISO A4 (210 × 297 mm).
    A4,
    /// ISO A5 (148 × 210 mm).
    A5,
    /// US Letter (8.5 × 11 in).
    Letter,
    /// US Legal (8.5 × 14 in).
    Legal,
    /// Tabloid (11 × 17 in).
    Tabloid,
    /// A custom size (in portrait orientation).
    Custom {
        /// Width of the paper.
        width: Length,
        /// Height of the paper.
        height: Length,
    },
}

impl PaperSize {
    /// The size of the paper in portrait orientation, in points.
    pub fn size_pt(self, pt_per_px: f64) -> Size {
        let (w, h) = match self {
            Self::A3 => (Length::Mm(297.0), Length::Mm(420.0)),
            Self::A4 => (Length::Mm(210.0), Length::Mm(297.0)),
            Self::A5 => (Length::Mm(148.0), Length::Mm(210.0)),
            Self::Letter => (Length::In(8.5), Length::In(11.0)),
            Self::Legal => (Length::In(8.5), Length::In(14.0)),
            Self::Tabloid => (Length::In(11.0), Length::In(17.0)),
            Self::Custom { width, height } => (width, height),
        };
        Size::new(w.to_pt(pt_per_px), h.to_pt(pt_per_px))
    }

    /// The name of the paper size as understood by CUPS (`-o media=...`), if any.
    pub fn cups_media_name(self) -> Option<&'static str> {
        Some(match self {
            Self::A3 => "A3",
            Self::A4 => "A4",
            Self::A5 => "A5",
            Self::Letter => "Letter",
            Self::Legal => "Legal",
            Self::Tabloid => "Tabloid",
            Self::Custom { .. } => return None,
        })
    }
}

/// Page orientation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Orientation {
    /// Taller than wide.
    #[default]
    Portrait,
    /// Wider than tall.
    Landscape,
}

/// Page margins.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Margins {
    /// Top margin.
    pub top: Length,
    /// Right margin.
    pub right: Length,
    /// Bottom margin.
    pub bottom: Length,
    /// Left margin.
    pub left: Length,
}

impl Margins {
    /// The same margin on all sides.
    pub const fn uniform(length: Length) -> Self {
        Self {
            top: length,
            right: length,
            bottom: length,
            left: length,
        }
    }

    /// No margins.
    pub const ZERO: Self = Self::uniform(Length::ZERO);
}

/// Everything about the layout of printed pages.
///
/// A page consists of (from top to bottom, inside the margins) an optional header band,
/// the content area, and an optional footer band, separated by `band_gap`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageSetup {
    /// Paper size.
    pub paper: PaperSize,
    /// Orientation.
    pub orientation: Orientation,
    /// Margins around the printable area.
    pub margins: Margins,
    /// Height of the header band (zero for none).
    pub header_height: Length,
    /// Height of the footer band (zero for none).
    pub footer_height: Length,
    /// Gap between the header/footer bands and the content area.
    pub band_gap: Length,
    /// Conversion factor from logical pixels to points (default [`PT_PER_PX`]).
    pub pt_per_px: f64,
    /// Layout direction of pages. With [`LayoutDirection::Rtl`], documents are laid out
    /// right-to-left (mirrored rows, right-aligned paragraphs).
    pub direction: LayoutDirection,
}

impl Default for PageSetup {
    fn default() -> Self {
        Self::a4()
    }
}

impl PageSetup {
    /// A4 portrait, 20mm margins.
    pub fn a4() -> Self {
        Self::new(PaperSize::A4)
    }

    /// US Letter portrait, 0.75in margins.
    pub fn letter() -> Self {
        Self {
            margins: Margins::uniform(Length::In(0.75)),
            ..Self::new(PaperSize::Letter)
        }
    }

    /// The given paper size in portrait orientation, 20mm margins, no header or footer.
    pub fn new(paper: PaperSize) -> Self {
        Self {
            paper,
            orientation: Orientation::Portrait,
            margins: Margins::uniform(Length::Mm(20.0)),
            header_height: Length::ZERO,
            footer_height: Length::ZERO,
            band_gap: Length::Mm(4.0),
            pt_per_px: PT_PER_PX,
            direction: LayoutDirection::Ltr,
        }
    }

    /// Sets the orientation.
    pub fn with_orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Sets the margins.
    pub fn with_margins(mut self, margins: Margins) -> Self {
        self.margins = margins;
        self
    }

    /// Sets the header height.
    pub fn with_header(mut self, height: Length) -> Self {
        self.header_height = height;
        self
    }

    /// Sets the footer height.
    pub fn with_footer(mut self, height: Length) -> Self {
        self.footer_height = height;
        self
    }

    /// Sets the layout direction.
    pub fn with_direction(mut self, direction: LayoutDirection) -> Self {
        self.direction = direction;
        self
    }

    fn pt(&self, length: Length) -> f64 {
        length.to_pt(self.pt_per_px)
    }

    /// The page size in points, taking orientation into account.
    pub fn page_size_pt(&self) -> Size {
        let size = self.paper.size_pt(self.pt_per_px);
        match self.orientation {
            Orientation::Portrait => size,
            Orientation::Landscape => Size::new(size.height, size.width),
        }
    }

    /// The area inside the margins, in points.
    pub fn printable_rect_pt(&self) -> Rect {
        let size = self.page_size_pt();
        Rect::new(
            self.pt(self.margins.left),
            self.pt(self.margins.top),
            size.width - self.pt(self.margins.right),
            size.height - self.pt(self.margins.bottom),
        )
    }

    fn gap_if(&self, height: Length) -> f64 {
        if self.pt(height) > 0.0 {
            self.pt(height) + self.pt(self.band_gap)
        } else {
            0.0
        }
    }

    /// The content area (between header and footer), in points.
    pub fn content_rect_pt(&self) -> Rect {
        let printable = self.printable_rect_pt();
        Rect::new(
            printable.x0,
            printable.y0 + self.gap_if(self.header_height),
            printable.x1,
            printable.y1 - self.gap_if(self.footer_height),
        )
    }

    /// The header band, in points.
    pub fn header_rect_pt(&self) -> Rect {
        let printable = self.printable_rect_pt();
        Rect::new(
            printable.x0,
            printable.y0,
            printable.x1,
            printable.y0 + self.pt(self.header_height),
        )
    }

    /// The footer band, in points.
    pub fn footer_rect_pt(&self) -> Rect {
        let printable = self.printable_rect_pt();
        Rect::new(
            printable.x0,
            printable.y1 - self.pt(self.footer_height),
            printable.x1,
            printable.y1,
        )
    }

    /// Converts a size in points to logical pixels.
    pub fn pt_to_px(&self, size: Size) -> Size {
        Size::new(size.width / self.pt_per_px, size.height / self.pt_per_px)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_dimensions() {
        let setup = PageSetup::a4();
        let size = setup.page_size_pt();
        assert!((size.width - 595.2756).abs() < 1e-3, "{size:?}");
        assert!((size.height - 841.8898).abs() < 1e-3, "{size:?}");
        let landscape = setup
            .with_orientation(Orientation::Landscape)
            .page_size_pt();
        assert_eq!(landscape.width, size.height);
        // A4 content width in CSS pixels (210mm - 40mm margins).
        let px = setup.pt_to_px(setup.content_rect_pt().size());
        assert!((px.width - 642.52).abs() < 0.01, "{px:?}");
    }

    #[test]
    fn bands_shrink_content() {
        let setup = PageSetup::a4()
            .with_header(Length::Pt(20.0))
            .with_footer(Length::Pt(10.0));
        let printable = setup.printable_rect_pt();
        let content = setup.content_rect_pt();
        let gap = Length::Mm(4.0).to_pt(PT_PER_PX);
        assert!((content.y0 - (printable.y0 + 20.0 + gap)).abs() < 1e-9);
        assert!((content.y1 - (printable.y1 - 10.0 - gap)).abs() < 1e-9);
        assert!((setup.header_rect_pt().height() - 20.0).abs() < 1e-9);
        assert_eq!(setup.footer_rect_pt().y1, printable.y1);
    }
}
