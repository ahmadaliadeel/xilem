// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Rendering Xilem views to paginated PDF documents, without a window.

use std::fmt;
use std::sync::Arc;

use masonry_print::{
    Capture, DocumentPart, FontSetup, HeadlessOptions, HeadlessRoot, PageDecorations, PageInfo,
    PageSetup, PdfError, PdfOptions, PdfOutput, PdfStandard, document_properties, paginate_to_pdf,
    print_property_set,
};
use xilem::core::{ProxyError, RawProxy, SendMessage, ViewId};
use xilem::masonry::core::{DefaultProperties, NewWidget};
use xilem::tokio::runtime::{Builder, Runtime};
use xilem::{AnyWidgetView, ViewCtx, WidgetView};

/// A proxy for views built without a running app: messages are dropped.
#[derive(Debug)]
struct NoopProxy;

impl RawProxy for NoopProxy {
    fn send_message(&self, _path: Arc<[ViewId]>, message: SendMessage) -> Result<(), ProxyError> {
        Err(ProxyError::DriverFinished(message))
    }

    fn dyn_debug(&self) -> &dyn fmt::Debug {
        self
    }
}

/// Builds the widget tree of a Xilem view, without a window or app.
///
/// The widgets reflect the current `state`; the resulting tree is not updated afterwards,
/// so this is meant for printing. Views that run async tasks won't receive their results.
pub fn build_widget<State: 'static, V: WidgetView<State>>(
    state: &mut State,
    view: &V,
) -> NewWidget<V::Widget> {
    thread_local! {
        static RUNTIME: Arc<Runtime> = Arc::new(
            Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to create a Tokio runtime"),
        );
    }
    let runtime = RUNTIME.with(Arc::clone);
    let mut ctx = ViewCtx::new(Arc::new(NoopProxy), runtime);
    let (pod, _view_state) = view.build(&mut ctx, state);
    pod.new_widget
}

type ViewFn<State> = Box<dyn Fn(&mut State, &PageInfo) -> Box<AnyWidgetView<State>>>;

enum Part<State: 'static> {
    Flow(Box<AnyWidgetView<State>>),
    Page(Box<AnyWidgetView<State>>),
}

/// A paginated document made of Xilem views. Create it with [`document`].
///
/// ```no_run
/// use xilem::view::label;
/// use xilem_print::{document, PageSetup};
///
/// struct Invoice { customer: String }
///
/// let mut state = Invoice { customer: "Ayesha".into() };
/// let pdf = document::<Invoice>(PageSetup::a4())
///     .title("Invoice")
///     .lang("en")
///     .flow(label(format!("Invoice for {}", state.customer)))
///     .footer(|_, page| label(format!("Page {} of {}", page.number, page.total)))
///     .render(&mut state)
///     .unwrap();
/// pdf.save("invoice.pdf").unwrap();
/// ```
pub struct Document<State: 'static> {
    setup: PageSetup,
    parts: Vec<Part<State>>,
    header: Option<ViewFn<State>>,
    footer: Option<ViewFn<State>>,
    options: PdfOptions,
    fonts: FontSetup,
    properties: Option<Box<dyn Fn() -> DefaultProperties>>,
}

impl<State: 'static> fmt::Debug for Document<State> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Document")
            .field("setup", &self.setup)
            .field("parts", &self.parts.len())
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

/// Creates a paginated document with the given page setup.
pub fn document<State: 'static>(setup: PageSetup) -> Document<State> {
    Document {
        setup,
        parts: Vec::new(),
        header: None,
        footer: None,
        options: PdfOptions::default(),
        fonts: FontSetup::system(),
        properties: None,
    }
}

impl<State: 'static> Document<State> {
    /// Adds flowing content, which is split across as many pages as needed.
    ///
    /// Page breaks are only placed between lines of text and between widgets
    /// (see also [`keep_together`](crate::keep_together) and [`page_break`](crate::page_break)).
    pub fn flow(mut self, view: impl WidgetView<State>) -> Self {
        self.parts.push(Part::Flow(view.boxed()));
        self
    }

    /// Adds a single page. Its content is scaled down if it doesn't fit.
    pub fn page(mut self, view: impl WidgetView<State>) -> Self {
        self.parts.push(Part::Page(view.boxed()));
        self
    }

    /// Sets the page header, built for each page.
    pub fn header<V: WidgetView<State>>(
        mut self,
        header: impl Fn(&mut State, &PageInfo) -> V + 'static,
    ) -> Self {
        self.header = Some(Box::new(move |state, page| header(state, page).boxed()));
        self
    }

    /// Sets the page footer, built for each page.
    pub fn footer<V: WidgetView<State>>(
        mut self,
        footer: impl Fn(&mut State, &PageInfo) -> V + 'static,
    ) -> Self {
        self.footer = Some(Box::new(move |state, page| footer(state, page).boxed()));
        self
    }

    /// Sets the document title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.options.title = Some(title.into());
        self
    }

    /// Sets the document language (BCP 47 tag, e.g. `"en"`, `"ar"`, `"ur"`).
    pub fn lang(mut self, lang: impl Into<String>) -> Self {
        self.options.lang = Some(lang.into());
        self
    }

    /// Sets the PDF standard to conform to (e.g. PDF/A or PDF/UA).
    pub fn standard(mut self, standard: PdfStandard) -> Self {
        self.options.standard = standard;
        self
    }

    /// Sets all PDF options.
    pub fn pdf_options(mut self, options: PdfOptions) -> Self {
        self.options = options;
        self
    }

    /// Sets the fonts used to lay out the document.
    ///
    /// The default uses system fonts.
    pub fn fonts(mut self, fonts: FontSetup) -> Self {
        self.fonts = fonts;
        self
    }

    /// Sets the default properties (theme) of widgets, as a function creating them.
    ///
    /// The default is [`print_property_set`]: Masonry's theme with dark text for paper.
    pub fn properties(mut self, properties: impl Fn() -> DefaultProperties + 'static) -> Self {
        self.properties = Some(Box::new(properties));
        self
    }

    fn default_properties(&self) -> Arc<DefaultProperties> {
        let base = self
            .properties
            .as_ref()
            .map_or_else(print_property_set, |properties| properties());
        Arc::new(document_properties(base, &self.setup))
    }

    /// Lays out the document with the current `state` and writes it as PDF.
    pub fn render(&self, state: &mut State) -> Result<PdfOutput, PdfError> {
        let properties = self.default_properties();
        let parts: Vec<DocumentPart> = self
            .parts
            .iter()
            .map(|part| match part {
                Part::Flow(view) => DocumentPart::flow(
                    build_widget(state, view),
                    &self.setup,
                    properties.clone(),
                    self.fonts.clone(),
                ),
                Part::Page(view) => DocumentPart::page(
                    build_widget(state, view),
                    &self.setup,
                    properties.clone(),
                    self.fonts.clone(),
                ),
            })
            .collect();
        let mut decorations = ViewDecorations {
            document: self,
            state,
            properties,
        };
        paginate_to_pdf(&parts, &self.setup, &mut decorations, &self.options)
    }
}

struct ViewDecorations<'a, State: 'static> {
    document: &'a Document<State>,
    state: &'a mut State,
    properties: Arc<DefaultProperties>,
}

impl<State: 'static> ViewDecorations<'_, State> {
    fn capture(&mut self, view: &ViewFn<State>, page: PageInfo, height_pt: f64) -> Capture {
        let view = view(self.state, &page);
        let widget = build_widget(self.state, &view);
        let setup = &self.document.setup;
        let width_pt = setup.content_rect_pt().width();
        let size = setup.pt_to_px(xilem::kurbo::Size::new(width_pt, height_pt));
        let options = HeadlessOptions {
            size,
            scale_factor: 1.0,
            default_properties: self.properties.clone(),
            fonts: self.document.fonts.clone(),
        };
        HeadlessRoot::new(widget, options).capture()
    }
}

impl<State: 'static> PageDecorations for ViewDecorations<'_, State> {
    fn header(&mut self, page: &PageInfo) -> Option<Capture> {
        let header = self.document.header.as_ref()?;
        let height = self.document.setup.header_rect_pt().height();
        Some(self.capture(header, *page, height))
    }

    fn footer(&mut self, page: &PageInfo) -> Option<Capture> {
        let footer = self.document.footer.as_ref()?;
        let height = self.document.setup.footer_rect_pt().height();
        Some(self.capture(footer, *page, height))
    }
}

/// Lays out a single view at the given size and returns its capture, e.g. to print it with
/// [`masonry_print::write_snapshot`].
pub fn capture_view<State: 'static>(
    state: &mut State,
    view: impl WidgetView<State>,
    size: xilem::kurbo::Size,
    properties: DefaultProperties,
    fonts: FontSetup,
) -> Capture {
    let widget = build_widget(state, &view);
    HeadlessRoot::new(
        widget,
        HeadlessOptions {
            size,
            scale_factor: 1.0,
            default_properties: Arc::new(properties),
            fonts,
        },
    )
    .capture()
}
