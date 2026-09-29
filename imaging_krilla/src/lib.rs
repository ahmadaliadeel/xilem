// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! A [krilla] (PDF) backend for the [`imaging`] crate.
//!
//! [`KrillaSink`] implements [`imaging::PaintSink`] on top of a [`krilla::surface::Surface`],
//! so any retained [`imaging::record::Scene`] (for example Masonry's paint output) can be
//! written to a PDF page as vector graphics:
//!
//! - fills and strokes become PDF paths (solid colors, linear/radial/sweep gradients),
//! - clips and isolated groups (opacity, blend modes, masks) map to their PDF equivalents,
//! - images are embedded losslessly,
//! - glyph runs become **real PDF text** with embedded, subsetted fonts. When the caller
//!   provides the source text for a run (see [`TextSource`]), the PDF text is selectable and
//!   searchable, including right-to-left and complex scripts such as Arabic and Urdu.
//! - content without a PDF equivalent (blurred rounded rectangles, used for box shadows)
//!   is rasterized with `vello_cpu` when the `raster-fallback` feature is enabled.
//!
//! # Example
//!
//! ```
//! use imaging::{Painter, record};
//! use imaging_krilla::{KrillaResources, KrillaSink, KrillaSinkOptions};
//! use krilla::Document;
//! use krilla::page::PageSettings;
//! use kurbo::{Affine, Rect};
//! use peniko::Color;
//!
//! let mut scene = record::Scene::new();
//! Painter::new(&mut scene).fill_rect(Rect::new(10., 10., 90., 40.), Color::from_rgb8(0x20, 0x60, 0xd0));
//!
//! let mut document = Document::new();
//! let mut resources = KrillaResources::default();
//! let mut page = document.start_page_with(PageSettings::from_wh(100.0, 50.0).unwrap());
//! let mut surface = page.surface();
//! let mut sink = KrillaSink::new(&mut surface, &mut resources, Affine::IDENTITY, KrillaSinkOptions::default());
//! record::replay(&scene, &mut sink);
//! let stats = sink.finish();
//! surface.finish();
//! page.finish();
//! let pdf: Vec<u8> = document.finish().unwrap();
//! assert!(pdf.starts_with(b"%PDF"));
//! assert_eq!(stats.unsupported.len(), 0);
//! ```
//!
//! # Feature flags
//!
//! - `raster-fallback` (default): rasterize blurred rounded rectangles with `vello_cpu`.

// LINEBENDER LINT SET - lib.rs - v3
// See https://linebender.org/wiki/canonical-lints/
#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![warn(clippy::print_stdout, clippy::print_stderr)]
#![cfg_attr(target_pointer_width = "64", warn(clippy::trivially_copy_pass_by_ref))]
// END LINEBENDER LINT SET
#![cfg_attr(docsrs, feature(doc_cfg))]
#![allow(
    clippy::cast_possible_truncation,
    reason = "krilla uses f32 coordinates, while kurbo uses f64"
)]

mod convert;
mod font;
mod image;
#[cfg(feature = "raster-fallback")]
mod raster;

use std::ops::Range;

use imaging::record::{Glyph, replay};
use imaging::{
    BlurredRoundedRect, ClipRef, Composite, FillRef, GlyphRunRef, GroupRef, MaskMode, PaintSink,
    StrokeRef,
};
use krilla::geom::{Path, Point, Size};
use krilla::mask::{Mask, MaskType};
use krilla::paint::{Fill, FillRule, Paint, Stroke};
use krilla::surface::Surface;
use krilla::tagging::{ContentTag, Identifier};
use krilla::text::{GlyphId, KrillaGlyph};
use kurbo::{Affine, BezPath, Rect};
use peniko::{BrushRef, Compose, Extend, Style};

pub use krilla;

/// Document-lifetime caches shared by all [`KrillaSink`]s writing to the same document.
///
/// Reusing one `KrillaResources` for all pages of a document means each font and image is
/// converted (and hashed by krilla) only once.
#[derive(Debug, Default)]
pub struct KrillaResources {
    fonts: font::FontCache,
    images: image::ImageCache,
}

/// What to do with glyph runs that have no [`TextSource`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnmappedText {
    /// Emit real text, deriving the Unicode text of each glyph from the font's character map.
    ///
    /// This works well for simple scripts, but can't recover text for ligatures or
    /// contextual forms (e.g. Arabic).
    #[default]
    ReverseCmap,
    /// Draw the glyphs as outlines (not selectable).
    Outline,
}

/// Options for a [`KrillaSink`].
#[derive(Clone, Debug)]
pub struct KrillaSinkOptions {
    /// Tolerance used when flattening shapes (e.g. rounded rectangles) into paths.
    pub tolerance: f64,
    /// Pixels per output unit used when rasterizing content without a PDF equivalent.
    ///
    /// With PDF points as output units, the default of `300 / 72` gives 300 DPI.
    pub raster_scale: f32,
    /// How to handle glyph runs without a [`TextSource`].
    pub unmapped_text: UnmappedText,
}

impl Default for KrillaSinkOptions {
    fn default() -> Self {
        Self {
            tolerance: 0.01,
            raster_scale: 300.0 / 72.0,
            unmapped_text: UnmappedText::default(),
        }
    }
}

/// The source text of a glyph run, used to make PDF text selectable and searchable.
///
/// `glyph_ranges[i]` is the byte range in `text` of the cluster that glyph `i` belongs to,
/// in the same (visual) order as the glyphs of the run.
/// Several glyphs may share a range (e.g. a base glyph and its marks), and one glyph may
/// cover several characters (e.g. a ligature).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextSource {
    /// The text covered by the glyph run.
    pub text: String,
    /// For each glyph, the byte range of its cluster in `text`.
    pub glyph_ranges: Vec<Range<usize>>,
}

impl TextSource {
    fn is_valid_for(&self, glyph_count: usize) -> bool {
        self.glyph_ranges.len() == glyph_count
            && self.glyph_ranges.iter().all(|r| {
                r.start <= r.end
                    && r.end <= self.text.len()
                    && self.text.is_char_boundary(r.start)
                    && self.text.is_char_boundary(r.end)
            })
    }
}

/// Statistics about what a [`KrillaSink`] emitted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KrillaSinkStats {
    /// Glyph runs emitted as text with a caller-provided [`TextSource`].
    pub glyph_runs_text: usize,
    /// Glyph runs emitted as text with text derived from the font's character map.
    pub glyph_runs_unmapped: usize,
    /// Glyph runs drawn as outlines.
    pub glyph_runs_outlined: usize,
    /// Number of elements that were rasterized.
    pub raster_fallbacks: usize,
    /// Features that were encountered but could not be represented (deduplicated).
    pub unsupported: Vec<&'static str>,
}

impl KrillaSinkStats {
    fn unsupported(&mut self, what: &'static str) {
        if !self.unsupported.contains(&what) {
            tracing::warn!("imaging_krilla: unsupported feature: {what}");
            self.unsupported.push(what);
        }
    }

    /// Merge statistics from another sink into these.
    pub fn merge(&mut self, other: &Self) {
        self.glyph_runs_text += other.glyph_runs_text;
        self.glyph_runs_unmapped += other.glyph_runs_unmapped;
        self.glyph_runs_outlined += other.glyph_runs_outlined;
        self.raster_fallbacks += other.raster_fallbacks;
        for u in &other.unsupported {
            if !self.unsupported.contains(u) {
                self.unsupported.push(u);
            }
        }
    }
}

/// Clip state of one group level.
///
/// Clips are applied lazily: `desired` is the imaging clip stack, and the first `applied` of
/// them are currently pushed on the krilla surface. This lets us temporarily remove clips
/// (e.g. around tagged content boundaries) and re-apply them before the next draw.
#[derive(Default)]
struct Level {
    desired: Vec<(Path, FillRule)>,
    applied: usize,
    /// Number of surface pushes made when opening this level (group).
    pops: usize,
}

/// An [`imaging::PaintSink`] that writes to a [`krilla::surface::Surface`].
///
/// All drawing commands are transformed by `base` (e.g. to map logical pixels to PDF points
/// and to place content on the page). krilla surfaces use a y-down coordinate system with the
/// origin at the top-left of the page.
///
/// Call [`finish`](Self::finish) when done; dropping the sink also balances the surface state.
pub struct KrillaSink<'s, 'p> {
    surface: &'s mut Surface<'p>,
    resources: &'s mut KrillaResources,
    base: Affine,
    options: KrillaSinkOptions,
    levels: Vec<Level>,
    viewport: Option<Rect>,
    viewport_applied: bool,
    next_text: Option<TextSource>,
    stats: KrillaSinkStats,
    finished: bool,
}

impl std::fmt::Debug for KrillaSink<'_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KrillaSink")
            .field("base", &self.base)
            .field("options", &self.options)
            .field("group_depth", &self.group_depth())
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

impl<'s, 'p> KrillaSink<'s, 'p> {
    /// Create a sink drawing into `surface`, with all content transformed by `base`.
    pub fn new(
        surface: &'s mut Surface<'p>,
        resources: &'s mut KrillaResources,
        base: Affine,
        options: KrillaSinkOptions,
    ) -> Self {
        Self {
            surface,
            resources,
            base,
            options,
            levels: vec![Level::default()],
            viewport: None,
            viewport_applied: false,
            next_text: None,
            stats: KrillaSinkStats::default(),
            finished: false,
        }
    }

    /// The transform applied to all incoming content.
    pub fn base_transform(&self) -> Affine {
        self.base
    }

    /// Set the transform applied to subsequent content.
    pub fn set_base_transform(&mut self, base: Affine) {
        self.base = base;
    }

    /// The number of currently open isolated groups.
    pub fn group_depth(&self) -> usize {
        self.levels.len() - 1
    }

    /// Set the source text for the next glyph run.
    ///
    /// It is used only if its glyph count matches the next glyph run, and is discarded after it.
    pub fn set_next_text_source(&mut self, source: Option<TextSource>) {
        self.next_text = source;
    }

    /// Draw a glyph run with the given source text.
    pub fn glyph_run_with_text(
        &mut self,
        draw: GlyphRunRef<'_>,
        glyphs: &mut dyn Iterator<Item = Glyph>,
        source: TextSource,
    ) {
        self.next_text = Some(source);
        self.glyph_run(draw, glyphs);
    }

    /// Set an additional clip rectangle (in input coordinates) applied to top-level content.
    ///
    /// This is used for example to restrict content to a page's content area.
    /// It only affects content outside of isolated groups; a group is clipped by the
    /// viewport that is active when the group is opened.
    pub fn set_viewport_clip(&mut self, clip: Option<Rect>) {
        if clip == self.viewport {
            return;
        }
        if self.viewport_applied && self.levels.len() == 1 {
            self.surface.pop();
            self.viewport_applied = false;
        }
        self.viewport = clip;
    }

    /// Start a tagged content section (see [`krilla::tagging`]).
    ///
    /// Returns `None` inside isolated groups, where tags can't be started.
    pub fn start_tagged(&mut self, tag: ContentTag<'_>) -> Option<Identifier> {
        if self.levels.len() != 1 {
            return None;
        }
        self.flush_clips();
        Some(self.surface.start_tagged(tag))
    }

    /// End the tagged content section started with [`start_tagged`](Self::start_tagged).
    pub fn end_tagged(&mut self) {
        if self.levels.len() != 1 {
            return;
        }
        self.flush_clips();
        self.surface.end_tagged();
    }

    /// Set the location reported in krilla validation errors for subsequent content.
    pub fn set_location(&mut self, location: Option<std::num::NonZeroU64>) {
        match location {
            Some(loc) => self.surface.set_location(loc),
            None => self.surface.reset_location(),
        }
    }

    /// Statistics about the content emitted so far.
    pub fn stats(&self) -> &KrillaSinkStats {
        &self.stats
    }

    /// Balance all surface state and return statistics.
    pub fn finish(mut self) -> KrillaSinkStats {
        self.balance();
        std::mem::take(&mut self.stats)
    }

    fn balance(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        while self.levels.len() > 1 {
            self.pop_group();
        }
        self.flush_clips();
    }

    // --- MARK: CLIPS

    fn clip_path(&self, clip: &ClipRef<'_>) -> (Path, FillRule) {
        let tolerance = self.options.tolerance;
        let (bez, rule) = match clip {
            ClipRef::Fill {
                transform,
                shape,
                fill_rule,
            } => {
                let mut bez = shape.clone().to_path(tolerance);
                bez.apply_affine(self.base * *transform);
                (bez, convert::fill_rule(*fill_rule))
            }
            ClipRef::Stroke {
                transform,
                shape,
                stroke,
            } => {
                let path = shape.clone().to_path(tolerance);
                let mut bez = kurbo::stroke(path, stroke, &kurbo::StrokeOpts::default(), tolerance);
                bez.apply_affine(self.base * *transform);
                (bez, FillRule::NonZero)
            }
        };
        (convert::path(&bez).unwrap_or_else(empty_clip), rule)
    }

    /// Apply all desired clips (and the viewport) of the current level to the surface.
    fn sync_clips(&mut self) {
        let top_level = self.levels.len() == 1;
        let level = self.levels.last_mut().unwrap();
        if level.applied < level.desired.len() {
            if top_level && self.viewport_applied {
                // The viewport clip must stay innermost.
                self.surface.pop();
                self.viewport_applied = false;
            }
            for (path, rule) in &level.desired[level.applied..] {
                self.surface.push_clip_path(path, rule);
            }
            level.applied = level.desired.len();
        }
        if top_level
            && !self.viewport_applied
            && let Some(viewport) = self.viewport
        {
            let mut bez = BezPath::from_vec(Vec::new());
            bez.extend(kurbo::Shape::path_elements(&viewport, 0.1));
            bez.apply_affine(self.base);
            let path = convert::path(&bez).unwrap_or_else(empty_clip);
            self.surface.push_clip_path(&path, &FillRule::NonZero);
            self.viewport_applied = true;
        }
    }

    /// Remove all clips of the current level from the surface (they stay desired).
    fn flush_clips(&mut self) {
        if self.levels.len() == 1 && self.viewport_applied {
            self.surface.pop();
            self.viewport_applied = false;
        }
        let level = self.levels.last_mut().unwrap();
        while level.applied > 0 {
            self.surface.pop();
            level.applied -= 1;
        }
    }

    // --- MARK: PAINT

    fn push_blend(&mut self, composite: Composite) -> bool {
        if composite.blend.compose != Compose::SrcOver {
            self.stats.unsupported("compose modes other than SrcOver");
        }
        if let Some(mode) = convert::blend_mode(composite.blend.mix) {
            self.surface.push_blend_mode(mode);
            true
        } else {
            false
        }
    }

    /// Convert a non-image brush to a paint and opacity.
    fn paint(&mut self, brush: &BrushRef<'_>, brush_transform: Option<Affine>) -> (Paint, f32) {
        match brush {
            BrushRef::Solid(color) => {
                let (color, alpha) = convert::color(*color);
                (color.into(), alpha)
            }
            BrushRef::Gradient(gradient) => (
                convert::gradient(gradient, brush_transform.unwrap_or_default(), 1.0),
                1.0,
            ),
            BrushRef::Image(_) => {
                self.stats.unsupported("image brushes on strokes or text");
                (krilla::color::rgb::Color::new(128, 128, 128).into(), 1.0)
            }
        }
    }

    fn draw_path(
        &mut self,
        transform: Affine,
        path: &Path,
        fill: Option<Fill>,
        stroke: Option<Stroke>,
    ) {
        self.surface.push_transform(&convert::transform(transform));
        self.surface.set_fill(fill);
        self.surface.set_stroke(stroke);
        self.surface.draw_path(path);
        self.surface.pop();
    }

    fn fill_image(&mut self, draw: &FillRef<'_>, image: peniko::ImageBrushRef<'_>) {
        let data = image.image;
        let Some(krilla_image) = self.resources.images.get(data, image.sampler.quality) else {
            self.stats.unsupported("invalid image data");
            return;
        };
        if image.sampler.x_extend != Extend::Pad || image.sampler.y_extend != Extend::Pad {
            self.stats.unsupported("repeating image brushes");
        }
        let Some(size) = Size::from_wh(data.width as f32, data.height as f32) else {
            return;
        };
        let image_rect = Rect::new(0.0, 0.0, f64::from(data.width), f64::from(data.height));
        let covers_exactly = draw.brush_transform.is_none()
            && matches!(draw.shape, imaging::GeometryRef::Rect(r) if r == image_rect);

        self.surface
            .push_transform(&convert::transform(self.base * draw.transform));
        let mut pops = 1;
        if !covers_exactly {
            let bez = draw.shape.clone().to_path(self.options.tolerance);
            if let Some(path) = convert::path(&bez) {
                self.surface
                    .push_clip_path(&path, &convert::fill_rule(draw.fill_rule));
                pops += 1;
            }
        }
        if let Some(brush_transform) = draw.brush_transform {
            self.surface
                .push_transform(&convert::transform(brush_transform));
            pops += 1;
        }
        let opacity = image.sampler.alpha * draw.composite.alpha;
        if opacity < 1.0 {
            self.surface.push_opacity(convert::normalized(opacity));
            pops += 1;
        }
        self.surface.draw_image(krilla_image, size);
        for _ in 0..pops {
            self.surface.pop();
        }
    }

    fn draw_glyphs(
        &mut self,
        draw: &GlyphRunRef<'_>,
        glyphs: &[Glyph],
        source: Option<TextSource>,
    ) {
        let font_size = draw.font_size;
        if glyphs.is_empty() || font_size <= 0.0 {
            return;
        }
        let Some(font) = self.resources.fonts.get(draw.font, draw.normalized_coords) else {
            self.stats.unsupported("fonts that krilla can't load");
            return;
        };

        let (paint, alpha) = self.paint(&draw.brush, draw.brush_transform);
        let opacity = convert::normalized(alpha * draw.composite.alpha);
        let (mut fill, mut stroke) = match draw.style {
            Style::Fill(rule) => (
                Some(Fill {
                    paint: paint.clone(),
                    opacity,
                    rule: convert::fill_rule(*rule),
                }),
                None,
            ),
            Style::Stroke(s) => (
                None,
                Some(Stroke {
                    paint: paint.clone(),
                    width: s.width as f32,
                    miter_limit: s.miter_limit as f32,
                    line_cap: convert::line_cap(s.start_cap),
                    line_join: convert::line_join(s.join),
                    opacity,
                    dash: convert::dash(s),
                }),
            ),
        };
        let embolden = draw.font_embolden.x.max(draw.font_embolden.y);
        if embolden > 0.0 && stroke.is_none() {
            stroke = Some(Stroke {
                paint,
                width: (embolden * 2.0) as f32,
                opacity,
                ..Stroke::default()
            });
        }
        if fill.is_none() && stroke.is_none() {
            fill = Some(Fill::default());
        }

        // Determine the text of each glyph.
        let outlined = source.is_none() && self.options.unmapped_text == UnmappedText::Outline;
        let (text, ranges) = match source {
            Some(source) => {
                self.stats.glyph_runs_text += 1;
                (source.text, source.glyph_ranges)
            }
            None => {
                if outlined {
                    self.stats.glyph_runs_outlined += 1;
                } else {
                    self.stats.glyph_runs_unmapped += 1;
                }
                let cmap = self.resources.fonts.reverse_cmap(draw.font);
                let mut text = String::new();
                let mut ranges = Vec::with_capacity(glyphs.len());
                for glyph in glyphs {
                    let start = text.len();
                    if let Some(ch) = cmap.get(&glyph.id) {
                        text.push(*ch);
                    }
                    ranges.push(start..text.len());
                }
                (text, ranges)
            }
        };

        let run_transform = self.base * draw.transform;
        let glyph_transform = draw.glyph_transform.unwrap_or_default();
        let [a, b, _, _, e, f] = glyph_transform.as_coeffs();
        let horizontal_preserving = b.abs() < 1e-9 && (a - 1.0).abs() < 1e-9 && e == 0. && f == 0.;

        self.surface.set_fill(fill);
        self.surface.set_stroke(stroke);
        if horizontal_preserving {
            let origin = glyphs[0];
            let y0 = f64::from(origin.y);
            let conjugated =
                Affine::translate((0.0, y0)) * glyph_transform * Affine::translate((0.0, -y0));
            let mut krilla_glyphs = Vec::with_capacity(glyphs.len());
            for (i, glyph) in glyphs.iter().enumerate() {
                let x_advance = match glyphs.get(i + 1) {
                    Some(next) => (next.x - glyph.x) / font_size,
                    None => {
                        font::advance(draw.font, draw.normalized_coords, glyph.id).unwrap_or(0.0)
                    }
                };
                krilla_glyphs.push(KrillaGlyph::new(
                    GlyphId::new(glyph.id),
                    x_advance,
                    0.0,
                    (origin.y - glyph.y) / font_size,
                    0.0,
                    ranges[i].clone(),
                    None,
                ));
            }
            self.surface
                .push_transform(&convert::transform(run_transform * conjugated));
            self.surface.draw_glyphs(
                Point::from_xy(origin.x, origin.y),
                &krilla_glyphs,
                font,
                &text,
                font_size,
                outlined,
            );
            self.surface.pop();
        } else {
            // General per-glyph transform: draw glyphs one by one.
            for (i, glyph) in glyphs.iter().enumerate() {
                let t = run_transform
                    * Affine::translate((f64::from(glyph.x), f64::from(glyph.y)))
                    * glyph_transform;
                let advance =
                    font::advance(draw.font, draw.normalized_coords, glyph.id).unwrap_or(0.0);
                let krilla_glyph = KrillaGlyph::new(
                    GlyphId::new(glyph.id),
                    advance,
                    0.0,
                    0.0,
                    0.0,
                    ranges[i].clone(),
                    None,
                );
                self.surface.push_transform(&convert::transform(t));
                self.surface.draw_glyphs(
                    Point::from_xy(0.0, 0.0),
                    &[krilla_glyph],
                    font.clone(),
                    &text,
                    font_size,
                    outlined,
                );
                self.surface.pop();
            }
        }
        self.surface.set_fill(None);
        self.surface.set_stroke(None);
    }

    #[cfg(feature = "raster-fallback")]
    fn rasterize_blurred_rounded_rect(&mut self, draw: BlurredRoundedRect) {
        let transform = self.base * draw.transform;
        if let Some((image, rect)) =
            raster::blurred_rounded_rect(&draw, transform, self.options.raster_scale)
        {
            self.stats.raster_fallbacks += 1;
            let Some(size) = Size::from_wh(rect.width() as f32, rect.height() as f32) else {
                return;
            };
            self.surface
                .push_transform(&convert::transform(Affine::translate(
                    rect.origin().to_vec2(),
                )));
            self.surface.draw_image(image, size);
            self.surface.pop();
        }
    }
}

fn empty_clip() -> Path {
    // A degenerate clip far away from any content, which clips everything.
    let mut builder = krilla::geom::PathBuilder::new();
    builder.move_to(-1.0e6, -1.0e6);
    builder.line_to(-1.0e6 + 0.001, -1.0e6);
    builder.line_to(-1.0e6 + 0.001, -1.0e6 + 0.001);
    builder.close();
    builder.finish().expect("non-degenerate path")
}

impl Drop for KrillaSink<'_, '_> {
    fn drop(&mut self) {
        self.balance();
    }
}

impl PaintSink for KrillaSink<'_, '_> {
    fn push_clip(&mut self, clip: ClipRef<'_>) {
        let entry = self.clip_path(&clip);
        self.levels.last_mut().unwrap().desired.push(entry);
    }

    fn pop_clip(&mut self) {
        let top_level = self.levels.len() == 1;
        let level = self.levels.last_mut().unwrap();
        if level.desired.is_empty() {
            return;
        }
        if level.applied == level.desired.len() {
            if top_level && self.viewport_applied {
                self.surface.pop();
                self.viewport_applied = false;
            }
            self.surface.pop();
            level.applied -= 1;
        }
        level.desired.pop();
    }

    fn push_group(&mut self, group: GroupRef<'_>) {
        self.sync_clips();
        let mut pops = 0;
        if let Some(clip) = &group.clip {
            let (path, rule) = self.clip_path(clip);
            self.surface.push_clip_path(&path, &rule);
            pops += 1;
        }
        if !group.filters.is_empty() {
            self.stats.unsupported("group filters");
        }
        if let Some(mask) = &group.mask {
            let mut builder = self.surface.stream_builder();
            {
                let mut mask_surface = builder.surface();
                let mut sink = KrillaSink::new(
                    &mut mask_surface,
                    self.resources,
                    self.base * mask.transform,
                    self.options.clone(),
                );
                replay(mask.mask.scene, &mut sink);
                let stats = sink.finish();
                self.stats.merge(&stats);
                mask_surface.finish();
            }
            let stream = builder.finish();
            let mask_type = match mask.mask.mode {
                MaskMode::Alpha => MaskType::Alpha,
                MaskMode::Luminance => MaskType::Luminosity,
            };
            self.surface.push_mask(Mask::new(stream, mask_type));
            pops += 1;
        }
        let blended = self.push_blend(group.composite);
        if blended {
            pops += 1;
        }
        if group.composite.alpha < 1.0 {
            self.surface
                .push_opacity(convert::normalized(group.composite.alpha));
            pops += 1;
        } else if blended || group.mask.is_some() {
            self.surface.push_isolated();
            pops += 1;
        }
        self.levels.push(Level {
            pops,
            ..Level::default()
        });
    }

    fn pop_group(&mut self) {
        if self.levels.len() == 1 {
            return;
        }
        self.flush_clips();
        let level = self.levels.pop().unwrap();
        for _ in 0..level.pops {
            self.surface.pop();
        }
    }

    fn fill(&mut self, draw: FillRef<'_>) {
        self.sync_clips();
        let blended = self.push_blend(draw.composite);
        if let BrushRef::Image(image) = &draw.brush {
            let image = *image;
            self.fill_image(&draw, image);
        } else {
            let bez = draw.shape.clone().to_path(self.options.tolerance);
            if let Some(path) = convert::path(&bez) {
                let (paint, alpha) = self.paint(&draw.brush, draw.brush_transform);
                let fill = Fill {
                    paint,
                    opacity: convert::normalized(alpha * draw.composite.alpha),
                    rule: convert::fill_rule(draw.fill_rule),
                };
                self.draw_path(self.base * draw.transform, &path, Some(fill), None);
            }
        }
        if blended {
            self.surface.pop();
        }
    }

    fn stroke(&mut self, draw: StrokeRef<'_>) {
        self.sync_clips();
        let blended = self.push_blend(draw.composite);
        let (paint, alpha) = self.paint(&draw.brush, draw.brush_transform);
        let opacity = convert::normalized(alpha * draw.composite.alpha);
        let bez = draw.shape.clone().to_path(self.options.tolerance);
        let transform = self.base * draw.transform;
        let style = draw.stroke;
        if style.start_cap != style.end_cap {
            // PDF has a single line cap; outline the stroke instead.
            let outline = kurbo::stroke(
                bez,
                style,
                &kurbo::StrokeOpts::default(),
                self.options.tolerance,
            );
            if let Some(path) = convert::path(&outline) {
                let fill = Fill {
                    paint,
                    opacity,
                    rule: FillRule::NonZero,
                };
                self.draw_path(transform, &path, Some(fill), None);
            }
        } else if let Some(path) = convert::path(&bez) {
            let stroke = Stroke {
                paint,
                width: style.width as f32,
                miter_limit: style.miter_limit as f32,
                line_cap: convert::line_cap(style.start_cap),
                line_join: convert::line_join(style.join),
                opacity,
                dash: convert::dash(style),
            };
            self.draw_path(transform, &path, None, Some(stroke));
        }
        if blended {
            self.surface.pop();
        }
    }

    fn glyph_run(&mut self, draw: GlyphRunRef<'_>, glyphs: &mut dyn Iterator<Item = Glyph>) {
        let glyphs: Vec<Glyph> = glyphs.collect();
        let source = self
            .next_text
            .take()
            .filter(|source| source.is_valid_for(glyphs.len()));
        self.sync_clips();
        let blended = self.push_blend(draw.composite);
        self.draw_glyphs(&draw, &glyphs, source);
        if blended {
            self.surface.pop();
        }
    }

    fn blurred_rounded_rect(&mut self, draw: BlurredRoundedRect) {
        self.sync_clips();
        let blended = self.push_blend(draw.composite);
        #[cfg(feature = "raster-fallback")]
        self.rasterize_blurred_rounded_rect(draw);
        #[cfg(not(feature = "raster-fallback"))]
        {
            let _ = draw;
            self.stats.unsupported("blurred rounded rectangles");
        }
        if blended {
            self.surface.pop();
        }
    }
}
