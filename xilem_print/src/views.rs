// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Views adding document semantics and pagination hints.

use std::marker::PhantomData;

use masonry_print::widgets::{self, SemanticRole};
use xilem::core::{MessageCtx, MessageResult, Mut, View, ViewId, ViewMarker, ViewPathTracker};
use xilem::{Pod, ViewCtx, WidgetView};

/// This is a randomly generated 32 bit number, to be able to catch bugs.
const CHILD_VIEW_ID: ViewId = ViewId::new(0x5a1d_7e3b);

macro_rules! single_child_view {
    (
        $(#[$meta:meta])*
        $view:ident, $widget:ident,
        fields { $($field:ident : $ty:ty),* },
        build($this:ident, $child:ident) $build:block,
        rebuild($self_:ident, $prev:ident, $element:ident) $rebuild:block
    ) => {
        $(#[$meta])*
        #[must_use = "View values do nothing unless provided to Xilem."]
        pub struct $view<V, State, Action = ()> {
            child: V,
            $($field: $ty,)*
            phantom: PhantomData<fn() -> (State, Action)>,
        }

        impl<V, State, Action> std::fmt::Debug for $view<V, State, Action> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct(stringify!($view)).finish_non_exhaustive()
            }
        }

        impl<V, State, Action> ViewMarker for $view<V, State, Action> {}
        impl<V, State, Action> View<State, Action, ViewCtx> for $view<V, State, Action>
        where
            State: 'static,
            Action: 'static,
            V: WidgetView<State, Action>,
        {
            type Element = Pod<widgets::$widget>;
            type ViewState = V::ViewState;

            fn build(
                &self,
                ctx: &mut ViewCtx,
                app_state: &mut State,
            ) -> (Self::Element, Self::ViewState) {
                let (child, child_state) =
                    ctx.with_id(CHILD_VIEW_ID, |ctx| self.child.build(ctx, app_state));
                let $this = self;
                let $child = child.new_widget;
                (Pod::new($build), child_state)
            }

            fn rebuild(
                &self,
                prev: &Self,
                view_state: &mut Self::ViewState,
                ctx: &mut ViewCtx,
                mut element: Mut<'_, Self::Element>,
                app_state: &mut State,
            ) {
                {
                    let $self_ = self;
                    let $prev = prev;
                    let $element = &mut element;
                    $rebuild
                }
                ctx.with_id(CHILD_VIEW_ID, |ctx| {
                    self.child.rebuild(
                        &prev.child,
                        view_state,
                        ctx,
                        widgets::$widget::child_mut(&mut element).downcast(),
                        app_state,
                    );
                });
            }

            fn teardown(
                &self,
                view_state: &mut Self::ViewState,
                ctx: &mut ViewCtx,
                mut element: Mut<'_, Self::Element>,
            ) {
                ctx.with_id(CHILD_VIEW_ID, |ctx| {
                    self.child.teardown(
                        view_state,
                        ctx,
                        widgets::$widget::child_mut(&mut element).downcast(),
                    );
                });
            }

            fn message(
                &self,
                view_state: &mut Self::ViewState,
                message: &mut MessageCtx,
                mut element: Mut<'_, Self::Element>,
                app_state: &mut State,
            ) -> MessageResult<Action> {
                match message.take_first() {
                    Some(CHILD_VIEW_ID) => self.child.message(
                        view_state,
                        message,
                        widgets::$widget::child_mut(&mut element).downcast(),
                        app_state,
                    ),
                    _ => {
                        tracing::warn!(?message, "Got unexpected id path in `{}`", stringify!($view));
                        MessageResult::Stale
                    }
                }
            }
        }
    };
}

single_child_view!(
    /// The view created by [`semantic`] and the functions built on it.
    SemanticView, Semantic,
    fields { role: SemanticRole, lang: Option<String>, alt: Option<String> },
    build(this, child) {
        let mut widget = widgets::Semantic::new(this.role.clone(), child);
        if let Some(lang) = &this.lang {
            widget = widget.with_lang(lang.clone());
        }
        if let Some(alt) = &this.alt {
            widget = widget.with_alt(alt.clone());
        }
        widget
    },
    rebuild(this, prev, element) {
        if this.role != prev.role {
            widgets::Semantic::set_role(element, this.role.clone());
        }
        if this.lang != prev.lang {
            widgets::Semantic::set_lang(element, this.lang.clone());
        }
        if this.alt != prev.alt {
            widgets::Semantic::set_alt(element, this.alt.clone());
        }
    }
);

impl<V, State, Action> SemanticView<V, State, Action> {
    /// Sets the language (BCP 47 tag, e.g. `"ur"`) of the content.
    pub fn lang(mut self, lang: impl Into<String>) -> Self {
        self.lang = Some(lang.into());
        self
    }

    /// Sets the alternative text of the content (e.g. of a figure).
    pub fn alt(mut self, alt: impl Into<String>) -> Self {
        self.alt = Some(alt.into());
        self
    }
}

/// Gives `child` a semantic role in printed documents (and for accessibility).
pub fn semantic<State: 'static, Action: 'static, V: WidgetView<State, Action>>(
    role: SemanticRole,
    child: V,
) -> SemanticView<V, State, Action> {
    SemanticView {
        child,
        role,
        lang: None,
        alt: None,
        phantom: PhantomData,
    }
}

/// A heading of the given level (1-6). Headings become PDF bookmarks.
pub fn heading<State: 'static, Action: 'static, V: WidgetView<State, Action>>(
    level: u8,
    child: V,
) -> SemanticView<V, State, Action> {
    semantic(SemanticRole::Heading(level), child)
}

/// A hyperlink. In PDF documents, the area of `child` is clickable.
pub fn link<State: 'static, Action: 'static, V: WidgetView<State, Action>>(
    url: impl Into<String>,
    child: V,
) -> SemanticView<V, State, Action> {
    semantic(SemanticRole::Link(url.into()), child)
}

/// A paragraph.
pub fn paragraph<State: 'static, Action: 'static, V: WidgetView<State, Action>>(
    child: V,
) -> SemanticView<V, State, Action> {
    semantic(SemanticRole::Paragraph, child)
}

/// A figure, with alternative text for screen readers.
pub fn figure<State: 'static, Action: 'static, V: WidgetView<State, Action>>(
    alt: impl Into<String>,
    child: V,
) -> SemanticView<V, State, Action> {
    semantic(SemanticRole::Figure, child).alt(alt)
}

/// Content in the given language (BCP 47 tag, e.g. `"ar"` or `"ur"`).
pub fn lang<State: 'static, Action: 'static, V: WidgetView<State, Action>>(
    tag: impl Into<String>,
    child: V,
) -> SemanticView<V, State, Action> {
    semantic(SemanticRole::Section, child).lang(tag)
}

/// A table.
pub fn table<State: 'static, Action: 'static, V: WidgetView<State, Action>>(
    child: V,
) -> SemanticView<V, State, Action> {
    semantic(SemanticRole::Table, child)
}

/// A row of a table.
pub fn table_row<State: 'static, Action: 'static, V: WidgetView<State, Action>>(
    child: V,
) -> SemanticView<V, State, Action> {
    semantic(SemanticRole::TableRow, child)
}

/// A header cell of a table.
pub fn table_header_cell<State: 'static, Action: 'static, V: WidgetView<State, Action>>(
    child: V,
) -> SemanticView<V, State, Action> {
    semantic(SemanticRole::TableHeaderCell, child)
}

/// A cell of a table.
pub fn table_cell<State: 'static, Action: 'static, V: WidgetView<State, Action>>(
    child: V,
) -> SemanticView<V, State, Action> {
    semantic(SemanticRole::TableCell, child)
}

/// A list.
pub fn list<State: 'static, Action: 'static, V: WidgetView<State, Action>>(
    child: V,
) -> SemanticView<V, State, Action> {
    semantic(SemanticRole::List, child)
}

/// An item of a list.
pub fn list_item<State: 'static, Action: 'static, V: WidgetView<State, Action>>(
    child: V,
) -> SemanticView<V, State, Action> {
    semantic(SemanticRole::ListItem, child)
}

/// Decorative content, ignored by screen readers.
pub fn artifact<State: 'static, Action: 'static, V: WidgetView<State, Action>>(
    child: V,
) -> SemanticView<V, State, Action> {
    semantic(SemanticRole::Artifact, child)
}

single_child_view!(
    /// The view created by [`keep_together`].
    KeepTogetherView, KeepTogether,
    fields {},
    build(_this, child) { widgets::KeepTogether::new(child) },
    rebuild(_this, _prev, _element) {}
);

/// Keeps `child` on one page in paginated documents (if it fits on a page).
pub fn keep_together<State: 'static, Action: 'static, V: WidgetView<State, Action>>(
    child: V,
) -> KeepTogetherView<V, State, Action> {
    KeepTogetherView {
        child,
        phantom: PhantomData,
    }
}

single_child_view!(
    /// The view created by [`print_region`].
    PrintRegionView, PrintRegion,
    fields { key: String },
    build(this, child) { widgets::PrintRegion::new(this.key.clone(), child) },
    rebuild(this, prev, element) {
        if this.key != prev.key {
            widgets::PrintRegion::set_key(element, this.key.clone());
        }
    }
);

/// Marks a region of the UI that can be printed on its own
/// (see [`SnapshotTarget::Region`](masonry_print::SnapshotTarget::Region)).
pub fn print_region<State: 'static, Action: 'static, V: WidgetView<State, Action>>(
    key: impl Into<String>,
    child: V,
) -> PrintRegionView<V, State, Action> {
    PrintRegionView {
        child,
        key: key.into(),
        phantom: PhantomData,
    }
}

/// The view created by [`page_break`].
#[derive(Debug, Clone, Copy)]
#[must_use = "View values do nothing unless provided to Xilem."]
pub struct PageBreakView;

/// Forces a page break in a paginated document.
pub fn page_break() -> PageBreakView {
    PageBreakView
}

impl ViewMarker for PageBreakView {}
impl<State: 'static, Action> View<State, Action, ViewCtx> for PageBreakView {
    type Element = Pod<widgets::PageBreak>;
    type ViewState = ();

    fn build(&self, _ctx: &mut ViewCtx, _: &mut State) -> (Self::Element, Self::ViewState) {
        (Pod::new(widgets::PageBreak::new()), ())
    }

    fn rebuild(
        &self,
        _prev: &Self,
        (): &mut Self::ViewState,
        _ctx: &mut ViewCtx,
        _element: Mut<'_, Self::Element>,
        _: &mut State,
    ) {
    }

    fn teardown(
        &self,
        (): &mut Self::ViewState,
        _ctx: &mut ViewCtx,
        _element: Mut<'_, Self::Element>,
    ) {
    }

    fn message(
        &self,
        (): &mut Self::ViewState,
        message: &mut MessageCtx,
        _element: Mut<'_, Self::Element>,
        _: &mut State,
    ) -> MessageResult<Action> {
        tracing::warn!(?message, "Got unexpected message in `PageBreakView`");
        MessageResult::Stale
    }
}
