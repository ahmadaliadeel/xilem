// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

#![doc(
    html_logo_url = "https://raw.githubusercontent.com/linebender/xilem/main/docs/assets/xilem-logo.svg"
)]
//! Printing and PDF export for Xilem apps.
//!
//! This crate builds on [`masonry_print`] to print Xilem views as vector PDF, with real,
//! selectable text (including right-to-left scripts such as Arabic and Urdu), tagged PDF
//! for accessibility, links, bookmarks and PDF/A. It provides:
//!
//! - **Documents**: [`document`] lays out Xilem views at paper size, splitting flowing
//!   content across pages without cutting lines, with headers and footers:
//!
//!   ```no_run
//!   use xilem::view::{flex_col, label};
//!   use xilem_print::{document, heading, page_break, PageSetup};
//!
//!   struct State;
//!   let pdf = document::<State>(PageSetup::a4())
//!       .title("Report")
//!       .lang("en")
//!       .flow(flex_col((
//!           heading(1, label("Report")),
//!           label("First chapter"),
//!           page_break(),
//!           label("Second chapter"),
//!       )))
//!       .footer(|_, page| label(format!("Page {} of {}", page.number, page.total)))
//!       .render(&mut State)
//!       .unwrap();
//!   pdf.save("report.pdf").unwrap();
//!   ```
//!
//! - **Window snapshots**: print a running app's window, a widget, or a [`print_region`], as it
//!   looks on screen. Start the app with [`run_with_printing`] and call [`request_print`] from
//!   a callback.
//! - **Semantic views**: [`heading`], [`link`], [`paragraph`], [`figure`], [`table`], [`list`],
//!   [`lang`], ... give content structure in tagged PDF, create bookmarks and clickable links.
//! - **Printing**: [`os_print`] sends PDF files to the operating system's printers.
//!
//! Right-to-left documents are supported with
//! [`PageSetup::with_direction`]`(LayoutDirection::Rtl)`, which mirrors layouts and paragraph
//! directions.

// LINEBENDER LINT SET - lib.rs - v3
// See https://linebender.org/wiki/canonical-lints/
#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![warn(clippy::print_stdout, clippy::print_stderr)]
#![cfg_attr(target_pointer_width = "64", warn(clippy::trivially_copy_pass_by_ref))]
// END LINEBENDER LINT SET
#![cfg_attr(docsrs, feature(doc_cfg))]

mod document;
mod live;
mod views;

pub use document::{Document, build_widget, capture_view, document};
pub use live::{
    PrintJob, PrintJobError, PrintOutput, PrintResult, PrintingDriver, SnapshotBackground,
    execute_print_job, request_print, run_with_printing,
};
pub use views::{
    KeepTogetherView, PageBreakView, PrintRegionView, SemanticView, artifact, figure, heading,
    keep_together, lang, link, list, list_item, page_break, paragraph, print_region, semantic,
    table, table_cell, table_header_cell, table_row,
};

pub use masonry_print;
pub use masonry_print::os_print;
pub use masonry_print::widgets::SemanticRole;
pub use masonry_print::{
    FontFallback, FontSetup, Length, Margins, NumberSystem, Orientation, PageInfo, PageSetup,
    PaperSize, PdfOptions, PdfOutput, PdfStandard, SnapshotOptions, SnapshotPage, SnapshotTarget,
};
