// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! A paint sink that interprets Masonry's paint annotations while writing PDF content.

use std::collections::HashMap;

use imaging_krilla::krilla::tagging::{Artifact, ArtifactType, ContentTag, Identifier, SpanTag};
use imaging_krilla::{KrillaSink, KrillaSinkStats, TextSource};
use masonry::accesskit::Role;
use masonry::core::{GLYPH_RUN_SOURCE_CONTEXT, GlyphRunSource};
use masonry::imaging::record::Glyph;
use masonry::imaging::{
    BlurredRoundedRect, ClipRef, ContextKindRef, ContextRef, ContextValueRef, FillRef, GlyphRunRef,
    GroupRef, PaintSink, StrokeRef,
};
use masonry::kurbo::{Rect, Shape as _};
use masonry::peniko::BrushRef;

use crate::access::AccessIndex;

/// Which content of a capture to draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ContentFilter {
    /// Everything.
    All,
    /// Only the given widget (by raw id) and its descendants.
    Subtree(u64),
    /// A horizontal band of the capture, used for pagination.
    ///
    /// Non-text content is drawn if it intersects the band (and clipped to it by the page),
    /// text is drawn only if its baseline is in `y0..y1`, so lines are never cut.
    Slice {
        /// Top of the band.
        y0: f64,
        /// Bottom of the band.
        y1: f64,
    },
}

/// The semantic role of a node for tagging purposes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Class {
    /// A structure element that owns all content drawn by it and its descendants.
    Owner,
    /// A text element; owns its text unless an ancestor is an [`Class::Owner`].
    Text,
    /// A grouping structure element (table, list, ...).
    Container,
    /// No semantics of its own.
    Transparent,
}

pub(crate) fn classify(access: &AccessIndex, id: u64) -> Class {
    let Some(node) = access.node(id) else {
        return Class::Transparent;
    };
    match node.role() {
        Role::Heading | Role::Link | Role::Paragraph => Class::Owner,
        Role::Image | Role::Figure => {
            // Images without alternative text are decorative (artifacts).
            let has_alt = node
                .description()
                .or(node.label())
                .is_some_and(|alt| !alt.trim().is_empty());
            if has_alt && !node.is_hidden() {
                Class::Owner
            } else {
                Class::Transparent
            }
        }
        Role::Label | Role::TextInput | Role::MultilineTextInput | Role::Document => Class::Text,
        Role::Table
        | Role::Row
        | Role::Cell
        | Role::ColumnHeader
        | Role::RowHeader
        | Role::List
        | Role::ListItem
        | Role::Section => Class::Container,
        _ => Class::Transparent,
    }
}

/// Marked content identifiers collected for the structure elements of one capture.
#[derive(Debug, Default)]
pub(crate) struct CaptureTags {
    pub(crate) leaves: HashMap<u64, Vec<Identifier>>,
}

/// How the content drawn by a [`SemanticSink`] is tagged.
pub(crate) enum Tagging<'t> {
    /// No tagging (untagged PDF).
    None,
    /// Tag content according to the accessibility tree.
    Content(&'t mut CaptureTags),
    /// Mark all content as an artifact (e.g. page headers and footers).
    Artifact(ArtifactType),
}

#[derive(Clone, Debug, PartialEq)]
enum TagKey {
    Content { owner: u64, lang: Option<String> },
    Artifact,
}

#[derive(Debug)]
enum Ctx {
    Widget(u64),
    Text(Option<GlyphRunSource>),
    Other,
}

/// Wraps a [`KrillaSink`], interpreting widget and text source annotations to
/// filter content, make text selectable, and tag the PDF.
pub(crate) struct SemanticSink<'a, 't, 's, 'p> {
    inner: KrillaSink<'s, 'p>,
    access: &'a AccessIndex,
    filter: ContentFilter,
    tagging: Tagging<'t>,
    contexts: Vec<Ctx>,
    current_tag: Option<TagKey>,
    /// Transform from capture coordinates to the layer being replayed is applied by the plan,
    /// so draw transforms are in capture coordinates.
    tolerance: f64,
}

impl<'a, 't, 's, 'p> SemanticSink<'a, 't, 's, 'p> {
    pub(crate) fn new(
        inner: KrillaSink<'s, 'p>,
        access: &'a AccessIndex,
        filter: ContentFilter,
        tagging: Tagging<'t>,
    ) -> Self {
        Self {
            inner,
            access,
            filter,
            tagging,
            contexts: Vec::new(),
            current_tag: None,
            tolerance: 0.1,
        }
    }

    pub(crate) fn finish(mut self) -> KrillaSinkStats {
        if self.current_tag.take().is_some() {
            self.inner.end_tagged();
        }
        self.inner.finish()
    }

    fn widgets(&self) -> impl DoubleEndedIterator<Item = u64> + '_ {
        self.contexts.iter().filter_map(|ctx| match ctx {
            Ctx::Widget(id) => Some(*id),
            _ => None,
        })
    }

    fn in_subtree(&self) -> bool {
        match self.filter {
            ContentFilter::Subtree(target) => self.widgets().any(|id| id == target),
            _ => true,
        }
    }

    /// Whether non-text content with the given bounds (in capture coordinates) is drawn.
    fn draws_bounds(&self, bounds: Rect) -> bool {
        if !self.in_subtree() {
            return false;
        }
        match self.filter {
            ContentFilter::Slice { y0, y1 } => bounds.y1 > y0 && bounds.y0 < y1,
            _ => true,
        }
    }

    /// The structure element owning content drawn at the current point.
    fn owner(&self, is_text: bool) -> Option<u64> {
        let mut text_owner = None;
        for id in self.widgets() {
            match classify(self.access, id) {
                Class::Owner => return Some(id),
                Class::Text => text_owner = Some(id),
                _ => {}
            }
        }
        if text_owner.is_none() && is_text {
            // Text drawn by a widget without text semantics: attribute it to that widget.
            return self.widgets().next_back();
        }
        text_owner
    }

    fn tag_key(&self, is_text: bool, is_image: bool) -> Option<TagKey> {
        match &self.tagging {
            Tagging::None => None,
            Tagging::Artifact(_) => Some(TagKey::Artifact),
            Tagging::Content(_) => {
                let owner = self.owner(is_text);
                let owner = owner.filter(|owner| {
                    is_text
                        || is_image
                            && matches!(self.access.role(*owner), Some(Role::Image | Role::Figure))
                });
                Some(match owner {
                    Some(owner) => TagKey::Content {
                        owner,
                        lang: self.access.language(owner).map(str::to_string),
                    },
                    None => TagKey::Artifact,
                })
            }
        }
    }

    /// Makes sure the right tag is open before drawing.
    fn ensure_tag(&mut self, key: Option<TagKey>) {
        if self.inner.group_depth() > 0 || key == self.current_tag {
            return;
        }
        if self.current_tag.take().is_some() {
            self.inner.end_tagged();
        }
        let Some(key) = key else {
            return;
        };
        let identifier = match &key {
            TagKey::Artifact => {
                let kind = match &self.tagging {
                    Tagging::Artifact(kind) => *kind,
                    _ => ArtifactType::Layout,
                };
                self.inner
                    .start_tagged(ContentTag::Artifact(Artifact::new(kind, None)));
                None
            }
            TagKey::Content { owner, lang } => {
                let tag = if matches!(self.access.role(*owner), Some(Role::Image | Role::Figure)) {
                    ContentTag::Other
                } else {
                    ContentTag::Span(SpanTag::empty().with_lang(lang.as_deref()))
                };
                self.inner.start_tagged(tag).map(|id| (*owner, id))
            }
        };
        if let (Some((owner, id)), Tagging::Content(tags)) = (identifier, &mut self.tagging) {
            tags.leaves.entry(owner).or_default().push(id);
        }
        self.current_tag = Some(key);
    }

    fn text_source(&self) -> Option<TextSource> {
        self.contexts.iter().rev().find_map(|ctx| match ctx {
            Ctx::Text(Some(source)) => Some(TextSource {
                text: source.text.clone(),
                glyph_ranges: source.glyph_ranges.clone(),
            }),
            _ => None,
        })
    }
}

impl PaintSink for SemanticSink<'_, '_, '_, '_> {
    fn push_context(&mut self, context: ContextRef<'_>) {
        let ctx = match (context.kind, context.value) {
            (ContextKindRef::Widget, ContextValueRef::U64(id)) => Ctx::Widget(id),
            (ContextKindRef::Named(GLYPH_RUN_SOURCE_CONTEXT), ContextValueRef::Str(value)) => {
                Ctx::Text(GlyphRunSource::decode(value))
            }
            _ => Ctx::Other,
        };
        self.contexts.push(ctx);
    }

    fn pop_context(&mut self) {
        self.contexts.pop();
    }

    fn push_clip(&mut self, clip: ClipRef<'_>) {
        self.inner.push_clip(clip);
    }

    fn pop_clip(&mut self) {
        self.inner.pop_clip();
    }

    fn push_group(&mut self, group: GroupRef<'_>) {
        if self.inner.group_depth() == 0 {
            let key = self.tag_key(true, false);
            self.ensure_tag(key);
        }
        self.inner.push_group(group);
    }

    fn pop_group(&mut self) {
        self.inner.pop_group();
    }

    fn fill(&mut self, draw: FillRef<'_>) {
        let bounds = draw
            .transform
            .transform_rect_bbox(draw.shape.clone().to_path(self.tolerance).bounding_box());
        if !self.draws_bounds(bounds) {
            return;
        }
        let is_image = matches!(draw.brush, BrushRef::Image(_));
        let key = self.tag_key(false, is_image);
        self.ensure_tag(key);
        self.inner.fill(draw);
    }

    fn stroke(&mut self, draw: StrokeRef<'_>) {
        let half = draw.stroke.width / 2.0;
        let bounds = draw.transform.transform_rect_bbox(
            draw.shape
                .clone()
                .to_path(self.tolerance)
                .bounding_box()
                .inflate(half, half),
        );
        if !self.draws_bounds(bounds) {
            return;
        }
        let key = self.tag_key(false, false);
        self.ensure_tag(key);
        self.inner.stroke(draw);
    }

    fn glyph_run(&mut self, draw: GlyphRunRef<'_>, glyphs: &mut dyn Iterator<Item = Glyph>) {
        let glyphs: Vec<Glyph> = glyphs.collect();
        let Some(first) = glyphs.first() else {
            return;
        };
        if !self.in_subtree() {
            return;
        }
        if let ContentFilter::Slice { y0, y1 } = self.filter {
            let baseline =
                draw.transform * masonry::kurbo::Point::new(f64::from(first.x), f64::from(first.y));
            if baseline.y < y0 || baseline.y >= y1 {
                return;
            }
        }
        let key = self.tag_key(true, false);
        self.ensure_tag(key);
        let source = self.text_source();
        self.inner.set_next_text_source(source);
        // Text owned by this page is never clipped by the page slice, so ink that extends
        // slightly beyond the line box (e.g. Nastaliq descenders) is not cut.
        let viewport = matches!(self.filter, ContentFilter::Slice { .. })
            .then(|| self.inner.take_viewport_clip())
            .flatten();
        self.inner.glyph_run(draw, &mut glyphs.into_iter());
        if viewport.is_some() {
            self.inner.set_viewport_clip(viewport);
        }
    }

    fn blurred_rounded_rect(&mut self, draw: BlurredRoundedRect) {
        let pad = draw.std_dev * 3.0;
        let bounds = draw
            .transform
            .transform_rect_bbox(draw.rect.inflate(pad, pad));
        if !self.draws_bounds(bounds) {
            return;
        }
        let key = self.tag_key(false, false);
        self.ensure_tag(key);
        self.inner.blurred_rounded_rect(draw);
    }
}
