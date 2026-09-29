// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Printing the windows of a running Xilem app.

use std::any::Any;
use std::cell::RefCell;
use std::fmt;
use std::path::PathBuf;

use masonry_print::os_print::{PrintError, PrintOptions, PrintReport, print_pdf, temp_pdf_path};
use masonry_print::{
    PdfError, PdfOptions, SnapshotOptions, SnapshotPage, SnapshotTarget, snapshot_to_pdf,
};
use masonry_winit::app::{AppDriver, DriverCtx, MasonryState, WgpuContext};
use xilem::masonry::app::RenderRoot;
use xilem::masonry::core::{ErasedAction, WidgetId};
use xilem::peniko::Color;
use xilem::winit::error::EventLoopError;
use xilem::{AppState, EventLoopBuilder, MasonryDriver, WindowId, WindowView, Xilem};

/// What to do with the PDF of a print job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrintOutput {
    /// Save the PDF to a file.
    Save(PathBuf),
    /// Send the PDF to a printer (via a temporary file).
    Print(PrintOptions),
    /// Save the PDF to a file, and print it.
    SaveAndPrint(PathBuf, PrintOptions),
}

/// Background painted behind a window snapshot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SnapshotBackground {
    /// The window's base color (as on screen).
    Window,
    /// A custom color.
    Color(Color),
    /// None (transparent, i.e. white paper).
    None,
}

/// The result of a successful print job.
#[derive(Clone, Debug)]
pub struct PrintResult {
    /// Where the PDF was written.
    pub path: PathBuf,
    /// Number of pages.
    pub pages: usize,
    /// The print report, if the job was printed.
    pub print: Option<PrintReport>,
}

/// Errors of print jobs.
#[derive(Debug)]
pub enum PrintJobError {
    /// Creating the PDF failed.
    Pdf(PdfError),
    /// Printing failed.
    Print(PrintError),
    /// Writing the file failed.
    Io(std::io::Error),
}

impl fmt::Display for PrintJobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pdf(e) => e.fmt(f),
            Self::Print(e) => e.fmt(f),
            Self::Io(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for PrintJobError {}

type OnDone<State> = Box<dyn FnOnce(&mut State, Result<PrintResult, PrintJobError>)>;

/// A request to print a window (or part of it) of the running app.
///
/// Submit it with [`request_print`] from any callback; it runs right after the callback,
/// if the app was started with [`run_with_printing`].
pub struct PrintJob<State> {
    /// The window, or `None` for the window in which the action happened.
    pub window: Option<WindowId>,
    /// What to print.
    pub target: SnapshotTarget,
    /// Page placement.
    pub page: SnapshotPage,
    /// Background.
    pub background: SnapshotBackground,
    /// PDF options.
    pub pdf: PdfOptions,
    /// What to do with the PDF.
    pub output: PrintOutput,
    /// Called with the result, with access to the app state.
    pub on_done: Option<OnDone<State>>,
}

impl<State> fmt::Debug for PrintJob<State> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrintJob")
            .field("window", &self.window)
            .field("target", &self.target)
            .field("page", &self.page)
            .field("output", &self.output)
            .finish_non_exhaustive()
    }
}

impl<State> PrintJob<State> {
    /// A job printing the whole window, sized to its content, saved to a temporary file.
    pub fn window() -> Self {
        Self {
            window: None,
            target: SnapshotTarget::Window,
            page: SnapshotPage::FitContent {
                margin: masonry_print::Length::ZERO,
            },
            background: SnapshotBackground::Window,
            pdf: PdfOptions::default(),
            output: PrintOutput::Save(temp_pdf_path("xilem-window")),
            on_done: None,
        }
    }

    /// Prints the given part of the window.
    pub fn target(mut self, target: SnapshotTarget) -> Self {
        self.target = target;
        self
    }

    /// Sets the page placement.
    pub fn page(mut self, page: SnapshotPage) -> Self {
        self.page = page;
        self
    }

    /// Sets the background.
    pub fn background(mut self, background: SnapshotBackground) -> Self {
        self.background = background;
        self
    }

    /// Sets the PDF options.
    pub fn pdf(mut self, pdf: PdfOptions) -> Self {
        self.pdf = pdf;
        self
    }

    /// Sets what to do with the PDF.
    pub fn output(mut self, output: PrintOutput) -> Self {
        self.output = output;
        self
    }

    /// Sets the callback receiving the result.
    pub fn on_done(
        mut self,
        on_done: impl FnOnce(&mut State, Result<PrintResult, PrintJobError>) + 'static,
    ) -> Self {
        self.on_done = Some(Box::new(on_done));
        self
    }
}

thread_local! {
    static JOBS: RefCell<Vec<Box<dyn Any>>> = const { RefCell::new(Vec::new()) };
}

/// Requests printing a window of the running app.
///
/// Call this from a view callback; the job runs as soon as the callback returns.
/// The app must be started with [`run_with_printing`].
pub fn request_print<State: 'static>(job: PrintJob<State>) {
    JOBS.with(|jobs| jobs.borrow_mut().push(Box::new(job)));
}

/// Runs a print job on a render root.
///
/// This is what [`PrintingDriver`] does for each requested job; it is public for testing and
/// for custom drivers.
pub fn execute_print_job<State>(
    root: &mut RenderRoot,
    base_color: Color,
    job: &PrintJob<State>,
) -> Result<PrintResult, PrintJobError> {
    let options = SnapshotOptions {
        target: job.target.clone(),
        page: job.page.clone(),
        background: match job.background {
            SnapshotBackground::Window => Some(base_color),
            SnapshotBackground::Color(color) => Some(color),
            SnapshotBackground::None => None,
        },
        pdf: job.pdf.clone(),
    };
    let pdf = snapshot_to_pdf(root, &options).map_err(PrintJobError::Pdf)?;
    let (path, print) = match &job.output {
        PrintOutput::Save(path) => (path.clone(), None),
        PrintOutput::Print(options) => (temp_pdf_path("xilem-print"), Some(options)),
        PrintOutput::SaveAndPrint(path, options) => (path.clone(), Some(options)),
    };
    std::fs::write(&path, &pdf.bytes).map_err(PrintJobError::Io)?;
    let print = match print {
        Some(options) => Some(print_pdf(&path, options).map_err(PrintJobError::Print)?),
        None => None,
    };
    Ok(PrintResult {
        path,
        pages: pdf.pages,
        print,
    })
}

/// An app driver adding printing to a Xilem app. See [`run_with_printing`].
pub struct PrintingDriver<State: 'static, Logic> {
    inner: MasonryDriver<State, Logic>,
}

impl<State: 'static, Logic> fmt::Debug for PrintingDriver<State, Logic> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrintingDriver").finish_non_exhaustive()
    }
}

impl<State, Logic, WindowIter> PrintingDriver<State, Logic>
where
    State: AppState + 'static,
    Logic: FnMut(&mut State) -> WindowIter + 'static,
    WindowIter: Iterator<Item = WindowView<State>>,
{
    /// Wraps the driver of a Xilem app.
    pub fn new(inner: MasonryDriver<State, Logic>) -> Self {
        Self { inner }
    }

    fn run_jobs(&mut self, window_id: WindowId, ctx: &mut DriverCtx<'_>) {
        let jobs = JOBS.with(|jobs| std::mem::take(&mut *jobs.borrow_mut()));
        for job in jobs {
            let Ok(mut job) = job.downcast::<PrintJob<State>>() else {
                tracing::error!("print job for a different app state type");
                continue;
            };
            let window = job.window.unwrap_or(window_id);
            let base_color = *ctx.window(window).base_color();
            let result = execute_print_job(ctx.render_root(window), base_color, &job);
            if let Err(error) = &result {
                tracing::warn!("print job failed: {error}");
            }
            if let Some(on_done) = job.on_done.take() {
                self.inner.update_state(ctx, |state| on_done(state, result));
            }
        }
    }
}

impl<State, Logic, WindowIter> AppDriver for PrintingDriver<State, Logic>
where
    State: AppState + 'static,
    Logic: FnMut(&mut State) -> WindowIter + 'static,
    WindowIter: Iterator<Item = WindowView<State>>,
{
    fn on_action(
        &mut self,
        window_id: WindowId,
        ctx: &mut DriverCtx<'_>,
        widget_id: WidgetId,
        action: ErasedAction,
    ) {
        self.inner.on_action(window_id, ctx, widget_id, action);
        self.run_jobs(window_id, ctx);
    }

    fn on_async_action(
        &mut self,
        window_id: WindowId,
        ctx: &mut DriverCtx<'_>,
        action: ErasedAction,
    ) {
        self.inner.on_async_action(window_id, ctx, action);
        self.run_jobs(window_id, ctx);
    }

    fn on_start(&mut self, state: &mut MasonryState) {
        self.inner.on_start(state);
    }

    fn on_close_requested(&mut self, window_id: WindowId, ctx: &mut DriverCtx<'_>) {
        self.inner.on_close_requested(window_id, ctx);
    }

    fn on_wgpu_ready(&mut self, wgpu: &WgpuContext<'_>) {
        self.inner.on_wgpu_ready(wgpu);
    }
}

/// Runs a Xilem app with printing support: jobs submitted with [`request_print`] are run
/// after each action.
pub fn run_with_printing<State, Logic, WindowIter>(
    app: Xilem<State, Logic>,
    event_loop: EventLoopBuilder,
) -> Result<(), EventLoopError>
where
    State: AppState + 'static,
    Logic: FnMut(&mut State) -> WindowIter + 'static,
    WindowIter: Iterator<Item = WindowView<State>>,
{
    app.run_in_with(event_loop, PrintingDriver::new)
}
