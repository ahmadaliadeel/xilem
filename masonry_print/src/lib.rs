// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

#![doc(
    html_logo_url = "https://raw.githubusercontent.com/linebender/xilem/main/docs/assets/masonry-logo.svg"
)]
//! Printing and vector PDF export for Masonry.
//!
//! Masonry paints through the [`imaging`](masonry::imaging) abstraction. This crate replays
//! that paint output into [`imaging_krilla`], which writes PDF, so printed output matches what
//! is on screen, as resolution-independent vector graphics with real, selectable text.
//!
//! Features:
//!
//! - **Snapshots**: print a window, a widget or a [`PrintRegion`](widgets::PrintRegion) as it
//!   appears on screen ([`snapshot_to_pdf`]).
//! - **Paginated documents**: lay out content at paper size, flow it across pages without
//!   cutting lines of text, with headers, footers and page numbers ([`paginate_to_pdf`]).
//! - **Text**: glyphs are embedded as subsetted fonts, with their source text, so text can be
//!   searched and copied, including right-to-left and complex scripts (Arabic, Urdu Nastaliq).
//!   Bundled Noto fonts for Arabic and Urdu are available with the `noto-fonts` feature.
//! - **Tagged PDF**: the structure tree is built from Masonry's accessibility tree
//!   (paragraphs, headings, links, figures, tables, lists), for screen readers and PDF/UA.
//! - **Links and bookmarks**: link widgets become clickable, headings become the outline.
//! - **PDF/A**: archival standards, validated by krilla ([`PdfStandard`]).
//! - **Printing**: hand the PDF to the operating system's print system ([`os_print`]).
//!
//! # Feature flags
//!
//! - `noto-fonts` (default): bundle Noto Naskh Arabic and Noto Nastaliq Urdu.

// LINEBENDER LINT SET - lib.rs - v3
// See https://linebender.org/wiki/canonical-lints/
#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![warn(clippy::print_stdout, clippy::print_stderr)]
#![cfg_attr(target_pointer_width = "64", warn(clippy::trivially_copy_pass_by_ref))]
// END LINEBENDER LINT SET
#![cfg_attr(docsrs, feature(doc_cfg))]

mod access;
mod capture;
mod fonts;
mod headless;
mod page;
mod sink;
mod snapshot;
mod tagging;
#[cfg(feature = "testing")]
pub mod testing;
mod writer;

pub use access::{AccessIndex, TextLine};
pub use capture::{Capture, capture};
#[cfg(feature = "noto-fonts")]
pub use fonts::noto;
pub use fonts::{FontFallback, FontSetup};
pub use headless::{HeadlessOptions, HeadlessRoot};
pub use page::{Length, Margins, Orientation, PT_PER_PX, PageSetup, PaperSize};
pub use sink::ContentFilter;
pub use snapshot::{
    SnapshotOptions, SnapshotPage, SnapshotTarget, snapshot_to_pdf, write_snapshot,
};
pub use writer::{
    CaptureId, ItemRole, PageItem, PdfDate, PdfError, PdfOptions, PdfOutput, PdfStandard, PdfWriter,
};

pub use imaging_krilla;
