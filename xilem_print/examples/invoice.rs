// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! An invoice editor that can print its window, part of its window, or a paginated
//! A4 document, as vector PDF, and send it to a printer.
//!
//! ```sh
//! cargo run -p xilem_print --example invoice
//! ```
//!
//! PDFs are written to the temporary directory; their paths are shown in the window.

#[path = "shared/invoice.rs"]
mod invoice;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use invoice::{InvoiceData, invoice_view};
use xilem::masonry::layout::AsUnit;
use xilem::masonry::properties::LayoutDirection;
use xilem::peniko::Color;
use xilem::style::Style as _;
use xilem::view::{CrossAxisAlignment, checkbox, flex_col, flex_row, label, portal, text_button};
use xilem::winit::error::EventLoopError;
use xilem::{EventLoop, WidgetView, WindowOptions, Xilem};
use xilem_print::masonry_print::{PdfError, noto};
use xilem_print::os_print::{PrintOptions, print_pdf_bytes};
use xilem_print::{
    FontSetup, Length, PageSetup, PdfOptions, PdfOutput, PrintJob, PrintJobError, PrintOutput,
    PrintResult, SnapshotPage, SnapshotTarget, document, print_region, request_print,
    run_with_printing,
};

struct AppData {
    invoice: InvoiceData,
    /// Status line. Shared with print jobs, which report their results asynchronously.
    status: Rc<RefCell<String>>,
}

fn output(name: &str) -> PathBuf {
    std::env::temp_dir().join(name)
}

fn report(result: Result<PrintResult, PrintJobError>) -> String {
    match result {
        Ok(result) => format!("Saved {} ({} page(s))", result.path.display(), result.pages),
        Err(error) => format!("Error: {error}"),
    }
}

/// Renders the invoice as a paginated A4 document.
fn render_document(data: &mut AppData) -> Result<PdfOutput, PdfError> {
    let direction = if data.invoice.rtl {
        LayoutDirection::Rtl
    } else {
        LayoutDirection::Ltr
    };
    let setup = PageSetup::a4()
        .with_direction(direction)
        .with_footer(Length::Pt(16.0));
    let invoice = data.invoice.clone();
    document::<InvoiceData>(setup)
        .title(format!("Invoice {}", invoice.number))
        .lang(if invoice.rtl { "ur" } else { "en" })
        .fonts(FontSetup::system().with_noto_arabic())
        .flow(invoice_view(&invoice))
        .footer(|_, page| label(format!("Page {} of {}", page.number, page.total)))
        .pdf_options(PdfOptions {
            title: Some(format!("Invoice {}", invoice.number)),
            ..PdfOptions::default()
        })
        .render(&mut data.invoice)
}

fn app_logic(data: &mut AppData) -> impl WidgetView<AppData> + use<> {
    let toolbar = flex_row((
        text_button("Save window as PDF", |data: &mut AppData| {
            let status = data.status.clone();
            request_print(
                PrintJob::window()
                    .output(PrintOutput::Save(output("xilem_invoice_window.pdf")))
                    .on_done(move |result| *status.borrow_mut() = report(result)),
            );
        }),
        text_button("Save invoice area as PDF", |data: &mut AppData| {
            let status = data.status.clone();
            request_print(
                PrintJob::window()
                    .target(SnapshotTarget::Region("invoice".into()))
                    .page(SnapshotPage::Paper(PageSetup::a4()))
                    .output(PrintOutput::Save(output("xilem_invoice_area.pdf")))
                    .on_done(move |result| *status.borrow_mut() = report(result)),
            );
        }),
        text_button("Export A4 document", |data: &mut AppData| {
            let message = match render_document(data) {
                Ok(pdf) => {
                    let path = output("xilem_invoice_document.pdf");
                    match pdf.save(&path) {
                        Ok(()) => format!("Saved {} ({} page(s))", path.display(), pdf.pages),
                        Err(error) => format!("Error: {error}"),
                    }
                }
                Err(error) => format!("Error: {error}"),
            };
            *data.status.borrow_mut() = message;
        }),
        text_button("Print…", |data: &mut AppData| {
            let message = match render_document(data) {
                Ok(pdf) => match print_pdf_bytes(&pdf.bytes, &PrintOptions::default()) {
                    Ok(report) => format!(
                        "Sent to the printer{}",
                        report
                            .job_id
                            .map(|id| format!(" (job {id})"))
                            .unwrap_or_default()
                    ),
                    Err(error) => format!("Printing failed: {error}"),
                },
                Err(error) => format!("Error: {error}"),
            };
            *data.status.borrow_mut() = message;
        }),
        checkbox(
            "Right-to-left",
            data.invoice.rtl,
            |data: &mut AppData, checked| data.invoice.rtl = checked,
        ),
    ))
    .gap(8.px());

    let status = data.status.borrow().clone();
    flex_col((
        toolbar,
        label(status),
        portal(print_region(
            "invoice",
            flex_col((invoice_view(&data.invoice),))
                .background_color(Color::WHITE)
                .border(Color::from_rgb8(0xcc, 0xcc, 0xcc), 1.px()),
        )),
    ))
    .cross_axis_alignment(CrossAxisAlignment::Start)
    .padding(12.px())
}

fn main() -> Result<(), EventLoopError> {
    let data = AppData {
        invoice: InvoiceData::sample(),
        status: Rc::new(RefCell::new(
            "Save or print the invoice. Files are written to the temporary directory.".into(),
        )),
    };
    let app = Xilem::new_simple(data, app_logic, WindowOptions::new("Invoice printing"))
        .with_font(noto::naskh_arabic())
        .with_font(noto::nastaliq_urdu());
    run_with_printing(app, EventLoop::with_user_event())
}
