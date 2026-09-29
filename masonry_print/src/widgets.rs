// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Widgets that add document semantics and pagination hints.
//!
//! These are regular Masonry widgets, so they can be used on screen as well. Their effect on
//! printing comes from the accessibility information they provide, which is how
//! [`capture`](crate::capture) sees them.

use masonry::accesskit::{Node, Role};
use masonry::core::{
    AccessCtx, ChildrenIds, LayoutCtx, MeasureCtx, NewWidget, NoAction, PaintCtx, PropertiesRef,
    RegisterCtx, Widget, WidgetId, WidgetMut, WidgetPod,
};
use masonry::imaging::Painter;
use masonry::kurbo::{Axis, Point, Size};
use masonry::layout::{LenDef, LenReq, Length, SizeDef};
use tracing::{Span, trace_span};

/// Class name of [`PageBreak`] widgets in the accessibility tree.
pub const PAGE_BREAK_CLASS: &str = "masonry_print.page_break";
/// Class name of [`KeepTogether`] widgets in the accessibility tree.
pub const KEEP_TOGETHER_CLASS: &str = "masonry_print.keep_together";
/// Class name of [`PageFlow`] widgets in the accessibility tree.
pub const PAGE_FLOW_CLASS: &str = "masonry_print.page_flow";

/// The semantic role of a [`Semantic`] widget.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SemanticRole {
    /// A heading of the given level (1 to 6). Headings appear in the PDF outline.
    Heading(u8),
    /// A hyperlink to the given URL. The area of the widget is clickable in the PDF.
    Link(String),
    /// A paragraph.
    Paragraph,
    /// A figure (e.g. an image or a chart). Should have alternative text.
    Figure,
    /// A list.
    List,
    /// An item of a list.
    ListItem,
    /// A table.
    Table,
    /// A row of a table.
    TableRow,
    /// A header cell of a table.
    TableHeaderCell,
    /// A cell of a table.
    TableCell,
    /// A section of a document.
    Section,
    /// Decorative content, which is ignored by screen readers.
    Artifact,
}

impl SemanticRole {
    fn role(&self) -> Role {
        match self {
            Self::Heading(_) => Role::Heading,
            Self::Link(_) => Role::Link,
            Self::Paragraph => Role::Paragraph,
            Self::Figure => Role::Figure,
            Self::List => Role::List,
            Self::ListItem => Role::ListItem,
            Self::Table => Role::Table,
            Self::TableRow => Role::Row,
            Self::TableHeaderCell => Role::ColumnHeader,
            Self::TableCell => Role::Cell,
            Self::Section => Role::Section,
            Self::Artifact => Role::GenericContainer,
        }
    }
}

// --- MARK: SINGLE CHILD
/// Shared implementation of single-child pass-through widgets.
struct SingleChild {
    child: WidgetPod<dyn Widget>,
}

impl SingleChild {
    fn new(child: NewWidget<impl Widget + ?Sized>) -> Self {
        Self {
            child: child.erased().to_pod(),
        }
    }

    fn measure(
        &mut self,
        ctx: &mut MeasureCtx<'_>,
        axis: Axis,
        cross_length: Option<Length>,
    ) -> Length {
        ctx.redirect_measurement(&mut self.child, axis, cross_length)
    }

    fn layout(&mut self, ctx: &mut LayoutCtx<'_>, size: Size) {
        ctx.run_layout(&mut self.child, size);
        ctx.place_child(&mut self.child, Point::ORIGIN);
        ctx.derive_baselines(&self.child);
    }
}

macro_rules! single_child_widget_impl {
    ($name:ident) => {
        fn register_children(&mut self, ctx: &mut RegisterCtx<'_>) {
            ctx.register_child(&mut self.inner.child);
        }

        fn measure(
            &mut self,
            ctx: &mut MeasureCtx<'_>,
            _props: &PropertiesRef<'_>,
            axis: Axis,
            _len_req: LenReq,
            cross_length: Option<Length>,
        ) -> Length {
            self.inner.measure(ctx, axis, cross_length)
        }

        fn layout(&mut self, ctx: &mut LayoutCtx<'_>, _props: &PropertiesRef<'_>, size: Size) {
            self.inner.layout(ctx, size);
        }

        fn paint(
            &mut self,
            _ctx: &mut PaintCtx<'_>,
            _props: &PropertiesRef<'_>,
            _painter: &mut Painter<'_>,
        ) {
        }

        fn children_ids(&self) -> ChildrenIds {
            ChildrenIds::from_slice(&[self.inner.child.id()])
        }

        fn make_trace_span(&self, id: WidgetId) -> Span {
            trace_span!(stringify!($name), id = id.trace())
        }
    };
}

macro_rules! debug_impl {
    ($name:ident { $($field:ident),* }) => {
        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct(stringify!($name))
                    $(.field(stringify!($field), &self.$field))*
                    .finish_non_exhaustive()
            }
        }
    };
}

debug_impl!(Semantic { role, lang, alt });
debug_impl!(KeepTogether {});
debug_impl!(PrintRegion { key });
debug_impl!(PageFlow {
    width,
    content_height
});

// --- MARK: SEMANTIC
/// A pass-through container which gives its child a semantic role, language or
/// alternative text.
///
/// In printed documents, this determines the structure of tagged PDF (headings, links,
/// figures, tables, lists), the document outline, and clickable links.
pub struct Semantic {
    inner: SingleChild,
    role: SemanticRole,
    lang: Option<String>,
    alt: Option<String>,
}

impl Semantic {
    /// Creates a new semantic container.
    pub fn new(role: SemanticRole, child: NewWidget<impl Widget + ?Sized>) -> Self {
        Self {
            inner: SingleChild::new(child),
            role,
            lang: None,
            alt: None,
        }
    }

    /// Sets the language (BCP 47 tag, e.g. `"ar"`) of the content.
    pub fn with_lang(mut self, lang: impl Into<String>) -> Self {
        self.lang = Some(lang.into());
        self
    }

    /// Sets the alternative text (e.g. of a figure).
    pub fn with_alt(mut self, alt: impl Into<String>) -> Self {
        self.alt = Some(alt.into());
        self
    }

    /// Returns the role.
    pub fn role(&self) -> &SemanticRole {
        &self.role
    }

    /// Changes the role.
    pub fn set_role(this: &mut WidgetMut<'_, Self>, role: SemanticRole) {
        this.widget.role = role;
        this.ctx.request_accessibility_update();
    }

    /// Changes the language.
    pub fn set_lang(this: &mut WidgetMut<'_, Self>, lang: Option<String>) {
        this.widget.lang = lang;
        this.ctx.request_accessibility_update();
    }

    /// Changes the alternative text.
    pub fn set_alt(this: &mut WidgetMut<'_, Self>, alt: Option<String>) {
        this.widget.alt = alt;
        this.ctx.request_accessibility_update();
    }

    /// Replaces the child.
    pub fn set_child(this: &mut WidgetMut<'_, Self>, child: NewWidget<impl Widget + ?Sized>) {
        let old = std::mem::replace(&mut this.widget.inner.child, child.erased().to_pod());
        this.ctx.remove_child(old);
    }

    /// Returns a mutable reference to the child.
    pub fn child_mut<'t>(this: &'t mut WidgetMut<'_, Self>) -> WidgetMut<'t, dyn Widget> {
        this.ctx.get_mut(&mut this.widget.inner.child)
    }
}

impl Widget for Semantic {
    type Action = NoAction;

    single_child_widget_impl!(Semantic);

    fn accessibility_role(&self) -> Role {
        self.role.role()
    }

    fn accessibility(
        &mut self,
        _ctx: &mut AccessCtx<'_>,
        _props: &PropertiesRef<'_>,
        node: &mut Node,
    ) {
        match &self.role {
            SemanticRole::Heading(level) => node.set_level(usize::from((*level).clamp(1, 6))),
            SemanticRole::Link(url) => node.set_url(url.clone()),
            SemanticRole::Artifact => node.set_hidden(),
            _ => {}
        }
        if let Some(lang) = &self.lang {
            node.set_language(lang.clone());
        }
        if let Some(alt) = &self.alt {
            node.set_description(alt.clone());
        }
    }
}

// --- MARK: KEEP TOGETHER
/// A pass-through container whose content is never split across pages, if it fits on a page.
pub struct KeepTogether {
    inner: SingleChild,
}

impl KeepTogether {
    /// Creates a new container.
    pub fn new(child: NewWidget<impl Widget + ?Sized>) -> Self {
        Self {
            inner: SingleChild::new(child),
        }
    }

    /// Replaces the child.
    pub fn set_child(this: &mut WidgetMut<'_, Self>, child: NewWidget<impl Widget + ?Sized>) {
        let old = std::mem::replace(&mut this.widget.inner.child, child.erased().to_pod());
        this.ctx.remove_child(old);
    }

    /// Returns a mutable reference to the child.
    pub fn child_mut<'t>(this: &'t mut WidgetMut<'_, Self>) -> WidgetMut<'t, dyn Widget> {
        this.ctx.get_mut(&mut this.widget.inner.child)
    }
}

impl Widget for KeepTogether {
    type Action = NoAction;

    single_child_widget_impl!(KeepTogether);

    fn accessibility_role(&self) -> Role {
        Role::GenericContainer
    }

    fn accessibility(
        &mut self,
        _ctx: &mut AccessCtx<'_>,
        _props: &PropertiesRef<'_>,
        node: &mut Node,
    ) {
        node.set_class_name(KEEP_TOGETHER_CLASS);
    }
}

// --- MARK: PRINT REGION
/// A pass-through container that marks a region of the UI which can be printed on its own,
/// with [`SnapshotTarget::Region`](crate::SnapshotTarget::Region).
pub struct PrintRegion {
    inner: SingleChild,
    key: String,
}

impl PrintRegion {
    /// Creates a new print region with the given key.
    pub fn new(key: impl Into<String>, child: NewWidget<impl Widget + ?Sized>) -> Self {
        Self {
            inner: SingleChild::new(child),
            key: key.into(),
        }
    }

    /// Changes the key.
    pub fn set_key(this: &mut WidgetMut<'_, Self>, key: impl Into<String>) {
        this.widget.key = key.into();
        this.ctx.request_accessibility_update();
    }

    /// Replaces the child.
    pub fn set_child(this: &mut WidgetMut<'_, Self>, child: NewWidget<impl Widget + ?Sized>) {
        let old = std::mem::replace(&mut this.widget.inner.child, child.erased().to_pod());
        this.ctx.remove_child(old);
    }

    /// Returns a mutable reference to the child.
    pub fn child_mut<'t>(this: &'t mut WidgetMut<'_, Self>) -> WidgetMut<'t, dyn Widget> {
        this.ctx.get_mut(&mut this.widget.inner.child)
    }
}

impl Widget for PrintRegion {
    type Action = NoAction;

    single_child_widget_impl!(PrintRegion);

    fn accessibility_role(&self) -> Role {
        Role::GenericContainer
    }

    fn accessibility(
        &mut self,
        _ctx: &mut AccessCtx<'_>,
        _props: &PropertiesRef<'_>,
        node: &mut Node,
    ) {
        node.set_author_id(self.key.clone());
    }
}

// --- MARK: PAGE BREAK
/// Forces a page break in a [`PageFlow`]. It has no size.
#[derive(Debug, Default)]
pub struct PageBreak;

impl PageBreak {
    /// Creates a page break.
    pub fn new() -> Self {
        Self
    }
}

impl Widget for PageBreak {
    type Action = NoAction;

    fn register_children(&mut self, _ctx: &mut RegisterCtx<'_>) {}

    fn measure(
        &mut self,
        _ctx: &mut MeasureCtx<'_>,
        _props: &PropertiesRef<'_>,
        _axis: Axis,
        _len_req: LenReq,
        _cross_length: Option<Length>,
    ) -> Length {
        Length::ZERO
    }

    fn layout(&mut self, _ctx: &mut LayoutCtx<'_>, _props: &PropertiesRef<'_>, _size: Size) {}

    fn paint(
        &mut self,
        _ctx: &mut PaintCtx<'_>,
        _props: &PropertiesRef<'_>,
        _painter: &mut Painter<'_>,
    ) {
    }

    fn accessibility_role(&self) -> Role {
        Role::GenericContainer
    }

    fn accessibility(
        &mut self,
        _ctx: &mut AccessCtx<'_>,
        _props: &PropertiesRef<'_>,
        node: &mut Node,
    ) {
        node.set_class_name(PAGE_BREAK_CLASS);
    }

    fn children_ids(&self) -> ChildrenIds {
        ChildrenIds::new()
    }

    fn make_trace_span(&self, id: WidgetId) -> Span {
        trace_span!("PageBreak", id = id.trace())
    }
}

// --- MARK: PAGE FLOW
/// The root of flowing document content: lays out its child at a fixed width, with as much
/// height as it needs, so it can then be split into pages.
pub struct PageFlow {
    inner: SingleChild,
    width: f64,
    content_height: f64,
}

impl PageFlow {
    /// Creates a flow of the given width (in logical pixels).
    pub fn new(width: f64, child: NewWidget<impl Widget + ?Sized>) -> Self {
        Self {
            inner: SingleChild::new(child),
            width,
            content_height: 0.0,
        }
    }

    /// The height of the laid out content.
    pub fn content_height(&self) -> f64 {
        self.content_height
    }

    /// Changes the width.
    pub fn set_width(this: &mut WidgetMut<'_, Self>, width: f64) {
        this.widget.width = width;
        this.ctx.request_layout();
    }

    /// Replaces the child.
    pub fn set_child(this: &mut WidgetMut<'_, Self>, child: NewWidget<impl Widget + ?Sized>) {
        let old = std::mem::replace(&mut this.widget.inner.child, child.erased().to_pod());
        this.ctx.remove_child(old);
    }

    /// Returns a mutable reference to the child.
    pub fn child_mut<'t>(this: &'t mut WidgetMut<'_, Self>) -> WidgetMut<'t, dyn Widget> {
        this.ctx.get_mut(&mut this.widget.inner.child)
    }
}

impl Widget for PageFlow {
    type Action = NoAction;

    fn register_children(&mut self, ctx: &mut RegisterCtx<'_>) {
        ctx.register_child(&mut self.inner.child);
    }

    fn measure(
        &mut self,
        ctx: &mut MeasureCtx<'_>,
        _props: &PropertiesRef<'_>,
        axis: Axis,
        _len_req: LenReq,
        _cross_length: Option<Length>,
    ) -> Length {
        match axis {
            Axis::Horizontal => Length::px(self.width),
            Axis::Vertical => ctx.compute_length(
                &mut self.inner.child,
                LenReq::MaxContent.into(),
                masonry::layout::LayoutSize::maybe(Axis::Horizontal, Some(Length::px(self.width))),
                Axis::Vertical,
                Some(Length::px(self.width)),
            ),
        }
    }

    fn layout(&mut self, ctx: &mut LayoutCtx<'_>, _props: &PropertiesRef<'_>, size: Size) {
        let auto_size = SizeDef::new(LenDef::Fixed(Length::px(self.width)), LenDef::MaxContent);
        let child_size = ctx.compute_size(&mut self.inner.child, auto_size, size.into());
        let child_size = Size::new(self.width, child_size.height);
        self.content_height = child_size.height;
        ctx.run_layout(&mut self.inner.child, child_size);
        ctx.place_child(&mut self.inner.child, Point::ORIGIN);
        ctx.derive_baselines(&self.inner.child);
    }

    fn paint(
        &mut self,
        _ctx: &mut PaintCtx<'_>,
        _props: &PropertiesRef<'_>,
        _painter: &mut Painter<'_>,
    ) {
    }

    fn accessibility_role(&self) -> Role {
        Role::GenericContainer
    }

    fn accessibility(
        &mut self,
        _ctx: &mut AccessCtx<'_>,
        _props: &PropertiesRef<'_>,
        node: &mut Node,
    ) {
        node.set_class_name(PAGE_FLOW_CLASS);
    }

    fn children_ids(&self) -> ChildrenIds {
        ChildrenIds::from_slice(&[self.inner.child.id()])
    }

    fn make_trace_span(&self, id: WidgetId) -> Span {
        trace_span!("PageFlow", id = id.trace())
    }
}
