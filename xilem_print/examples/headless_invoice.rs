// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Writes invoice PDFs without opening a window.
//!
//! ```sh
//! cargo run -p xilem_print --example headless_invoice -- [output directory]
//! ```
//!
//! This writes:
//! - `invoice_en.pdf`: a left-to-right A4 invoice,
//! - `invoice_ur_rtl.pdf`: the same invoice laid out right-to-left, with Urdu page numbers,
//!   conforming to PDF/A-2a and PDF/UA-1.

#![expect(clippy::print_stdout, reason = "command line example")]

#[path = "shared/invoice.rs"]
mod invoice;

use std::error::Error;
use std::path::PathBuf;

use invoice::{InvoiceData, URDU_FONT, invoice_view};
use xilem::WidgetView as _;
use xilem::masonry::properties::{BaseDirection, LayoutDirection};
use xilem::view::label;
use xilem_print::{FontSetup, Length, NumberSystem, PageSetup, PdfStandard, document};

fn main() -> Result<(), Box<dyn Error>> {
    let out_dir = std::env::args()
        .nth(1)
        .map_or_else(std::env::temp_dir, PathBuf::from);
    std::fs::create_dir_all(&out_dir)?;

    let mut data = InvoiceData::sample();
    for (file, rtl, standard) in [
        ("invoice_en.pdf", false, PdfStandard::Plain),
        ("invoice_ur_rtl.pdf", true, PdfStandard::A2aUa1),
    ] {
        data.rtl = rtl;
        let direction = if rtl {
            LayoutDirection::Rtl
        } else {
            LayoutDirection::Ltr
        };
        let setup = PageSetup::a4()
            .with_direction(direction)
            // Nastaliq needs tall lines.
            .with_footer(Length::Pt(if rtl { 36.0 } else { 16.0 }));
        let pdf = document::<InvoiceData>(setup)
            .title(format!("Invoice {}", data.number))
            .lang(if rtl { "ur" } else { "en" })
            .fonts(FontSetup::system().with_noto_arabic())
            .flow(invoice_view(&data))
            .footer(move |_, page| {
                if rtl {
                    let digits = NumberSystem::ExtendedArabicIndic;
                    label(format!(
                        "صفحہ {} از {}",
                        digits.format(page.number),
                        digits.format(page.total)
                    ))
                    .font(URDU_FONT)
                    .prop(BaseDirection::Rtl)
                } else {
                    label(format!("Page {} of {}", page.number, page.total))
                        .prop(BaseDirection::Auto)
                }
            })
            .standard(standard)
            .render(&mut data)?;
        let path = out_dir.join(file);
        pdf.save(&path)?;
        println!(
            "Wrote {} ({} page(s), {} bytes, {} text runs)",
            path.display(),
            pdf.pages,
            pdf.bytes.len(),
            pdf.stats.glyph_runs_text
        );
    }
    Ok(())
}
