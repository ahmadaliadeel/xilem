// Copyright 2018 the Xilem Authors and the Druid Authors
// SPDX-License-Identifier: Apache-2.0

//! Support for text display and rendering
//!
//! There are three kinds of text commonly needed:
//!  1) Non interactive text (e.g. a button's label)
//!  2) Selectable text (e.g. a paragraph of content)
//!  3) Editable text (e.g. a search bar)
//!
//! All of these have the same set of global styling options, and can contain rich text

/// A reference counted string slice.
///
/// This is a data-friendly way to represent strings in Masonry. Unlike `String`
/// it cannot be mutated, but unlike `String` it can be cheaply cloned.
pub type ArcStr = std::sync::Arc<str>;

/// The Parley [`Brush`] used within Masonry.
///
/// This enables updating of brush details without performing relayouts;
/// the inner values are indexes into the `brushes` argument to [`render_text()`].
///
/// [`Brush`]: parley::Brush
#[derive(Clone, PartialEq, Default, Debug)]
pub struct BrushIndex(pub usize);

/// A style property specialised for use within Masonry.
pub type StyleProperty = parley::StyleProperty<'static, BrushIndex>;

/// A set of styles specialised for use within Masonry.
pub type StyleSet = parley::StyleSet<BrushIndex>;

use std::ops::Range;

use accesskit::{TextDecoration, TextDecorationStyle};
use kurbo::{Affine, Line, Stroke};
use parley::{GlyphRun, Layout, PositionedLayoutItem, Run, Style};
use peniko::{Brush, Fill};
use smallvec::SmallVec;

use crate::imaging::{ContextRef, PaintSink, Painter, record::Glyph};

/// A function that renders laid out glyphs through imaging's [`Painter`].
///
/// The `BrushIndex` values of the runs are indices into `brushes`.
pub fn render_text(
    painter: &mut Painter<'_, impl PaintSink + ?Sized>,
    transform: Affine,
    layout: &Layout<BrushIndex>,
    brushes: &[Brush],
    // TODO: Should this be part of `BrushIndex` (i.e. `brushes`)?
    hint: bool,
) {
    render_text_with_source(painter, transform, layout, brushes, hint, None);
}

/// The name of the [context annotation](ContextRef) that [`render_text_with_source`] wraps
/// around each glyph run.
///
/// Its value is a string, which [`GlyphRunSource::decode`] turns back into a [`GlyphRunSource`].
pub const GLYPH_RUN_SOURCE_CONTEXT: &str = "masonry.glyph_run_source";

/// The source text of a glyph run, with the text range of each glyph.
///
/// This is attached to glyph runs as a [context annotation](GLYPH_RUN_SOURCE_CONTEXT) by
/// [`render_text_with_source`], so that backends which output text (e.g. PDF) can make text
/// selectable and searchable.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GlyphRunSource {
    /// The text covered by the glyph run, in logical order.
    pub text: String,
    /// For each glyph of the run (in drawing order), the byte range in `text`
    /// of the cluster the glyph belongs to.
    pub glyph_ranges: Vec<Range<usize>>,
}

impl GlyphRunSource {
    const VERSION: &str = "v1";

    /// Encode this source as a string, for use as a context annotation value.
    pub fn encode(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::with_capacity(self.text.len() + self.glyph_ranges.len() * 6 + 4);
        out.push_str(Self::VERSION);
        out.push(';');
        for (i, range) in self.glyph_ranges.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let _ = write!(out, "{}:{}", range.start, range.end);
        }
        out.push(';');
        out.push_str(&self.text);
        out
    }

    /// Decode a source encoded with [`encode`](Self::encode).
    pub fn decode(encoded: &str) -> Option<Self> {
        let mut parts = encoded.splitn(3, ';');
        if parts.next()? != Self::VERSION {
            return None;
        }
        let ranges = parts.next()?;
        let text = parts.next()?.to_string();
        let mut glyph_ranges = Vec::new();
        if !ranges.is_empty() {
            for range in ranges.split(',') {
                let (start, end) = range.split_once(':')?;
                let (start, end) = (start.parse().ok()?, end.parse().ok()?);
                if start > end || end > text.len() {
                    return None;
                }
                glyph_ranges.push(start..end);
            }
        }
        Some(Self { text, glyph_ranges })
    }
}

/// Whether `c` is an invisible bidi formatting character.
///
/// Clusters consisting only of these are left out of [`GlyphRunSource`]s, so that
/// direction marks (e.g. those added to force a base direction) don't end up in
/// copied text.
fn is_bidi_control(c: char) -> bool {
    matches!(
        c,
        '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
}

/// Removes leading and trailing bidi control characters from a text range.
fn trim_bidi_controls(text: &str, mut range: Range<usize>) -> Range<usize> {
    let Some(slice) = text.get(range.clone()) else {
        return range;
    };
    let trimmed_start = slice.trim_start_matches(is_bidi_control);
    range.start += slice.len() - trimmed_start.len();
    let trimmed = trimmed_start.trim_end_matches(is_bidi_control);
    range.end = range.start + trimmed.len();
    range
}

/// Compute the text range of each glyph of `run`, in visual order.
///
/// Glyphs of a cluster get the text range of that cluster. Clusters without glyphs
/// (ligature continuations, default ignorables) are merged into the preceding cluster.
fn run_glyph_ranges(run: &Run<'_, BrushIndex>, text: &str) -> Vec<Range<usize>> {
    let clusters: Vec<_> = run.clusters().collect();
    // Text range of each logical cluster, after merging glyph-less clusters.
    let mut ranges: Vec<Option<Range<usize>>> = vec![None; clusters.len()];
    let mut owner: Option<usize> = None;
    let mut pending_start: Option<usize> = None;
    for (i, cluster) in clusters.iter().enumerate() {
        let range = trim_bidi_controls(text, cluster.text_range());
        let has_glyphs = cluster.glyphs().next().is_some();
        if has_glyphs && range.is_empty() {
            // Shapers render bidi controls as invisible glyphs; they get no text.
            ranges[i] = Some(range);
        } else if has_glyphs {
            let start = pending_start.take().unwrap_or(range.start).min(range.start);
            ranges[i] = Some(start..range.end);
            owner = Some(i);
        } else if text
            .get(range.clone())
            .is_some_and(|s| s.chars().all(is_bidi_control))
        {
            // Drop direction marks.
        } else if let Some(owner) = owner {
            if let Some(owner_range) = &mut ranges[owner] {
                owner_range.end = owner_range.end.max(range.end);
            }
        } else {
            pending_start.get_or_insert(range.start);
        }
    }

    let mut glyph_ranges = Vec::new();
    for visual_index in 0..clusters.len() {
        let Some(logical) = run.visual_to_logical(visual_index) else {
            continue;
        };
        let Some(range) = &ranges[logical] else {
            continue;
        };
        for _ in clusters[logical].glyphs() {
            glyph_ranges.push(range.clone());
        }
    }
    glyph_ranges
}

/// Like [`render_text`], but annotates each glyph run with its source text.
///
/// If `source` is `Some`, it must be the text the `layout` was built from.
/// Each glyph run is then wrapped in a [context annotation](ContextRef) named
/// [`GLYPH_RUN_SOURCE_CONTEXT`] whose value is an [encoded](GlyphRunSource::encode)
/// [`GlyphRunSource`]. Backends that don't care about text ignore it, so this doesn't
/// change the rendered output.
pub fn render_text_with_source(
    painter: &mut Painter<'_, impl PaintSink + ?Sized>,
    transform: Affine,
    layout: &Layout<BrushIndex>,
    brushes: &[Brush],
    hint: bool,
    source: Option<&str>,
) {
    for line in layout.lines() {
        // Glyph runs are slices of the glyphs of a (per-line) run, split by style.
        // Track which run we are in and how many of its glyphs we have seen so far.
        let mut run_cursor: Option<(usize, usize, Vec<Range<usize>>)> = None;
        let mut glyph_runs: SmallVec<[(usize, GlyphRun<'_, BrushIndex>, Option<String>); 4]> =
            SmallVec::new();
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };
            let mut logical_start = 0;
            let annotation = source.and_then(|text| {
                let run = glyph_run.run();
                let glyph_count = glyph_run.glyphs().count();
                let (_, offset, ranges) = match &mut run_cursor {
                    Some(cursor) if cursor.0 == run.index() => cursor,
                    cursor => cursor.insert((run.index(), 0, run_glyph_ranges(run, text))),
                };
                let ranges = ranges.get(*offset..*offset + glyph_count)?;
                *offset += glyph_count;
                // Glyphs without text (e.g. bidi controls) have empty ranges and don't
                // extend the text of the run.
                let text_ranges = || ranges.iter().filter(|r| !r.is_empty());
                let start = text_ranges()
                    .map(|r| r.start)
                    .min()
                    .unwrap_or_else(|| ranges.first().map_or(0, |r| r.start));
                let end = text_ranges().map(|r| r.end).max().unwrap_or(start);
                logical_start = start;
                let len = end - start;
                let rebase = |offset: usize| offset.saturating_sub(start).min(len);
                Some(
                    GlyphRunSource {
                        text: text.get(start..end)?.to_string(),
                        glyph_ranges: ranges
                            .iter()
                            .map(|r| rebase(r.start)..rebase(r.end))
                            .collect(),
                    }
                    .encode(),
                )
            });
            glyph_runs.push((logical_start, glyph_run, annotation));
        }
        if source.is_some() {
            // Glyph runs are positioned independently, so drawing them in logical (text)
            // order instead of visual order renders the same, but makes the order of the
            // drawing commands match the reading order (e.g. for text extraction in PDFs).
            glyph_runs.sort_by_key(|(logical_start, _, _)| *logical_start);
        }
        for (_, glyph_run, annotation) in &glyph_runs {
            draw_glyph_run(
                painter,
                transform,
                glyph_run,
                brushes,
                hint,
                annotation.as_deref(),
            );
        }
    }
}

fn draw_glyph_run(
    painter: &mut Painter<'_, impl PaintSink + ?Sized>,
    transform: Affine,
    glyph_run: &GlyphRun<'_, BrushIndex>,
    brushes: &[Brush],
    hint: bool,
    annotation: Option<&str>,
) {
    let style = glyph_run.style();
    // We draw underlines under the text, then the strikethrough on top, following:
    // https://drafts.csswg.org/css-text-decor/#painting-order
    if let Some(underline) = &style.underline {
        let underline_brush = &brushes[underline.brush.0];
        let run_metrics = glyph_run.run().metrics();
        let offset = match underline.offset {
            Some(offset) => offset,
            None => run_metrics.underline_offset,
        };
        let width = match underline.size {
            Some(size) => size,
            None => run_metrics.underline_size,
        };
        // The `offset` is the distance from the baseline to the top of the underline
        // so we move the line down by half the width
        // Remember that we are using a y-down coordinate system
        // If there's a custom width, because this is an underline, we want the custom
        // width to go down from the default expectation
        let y = glyph_run.baseline() - offset + width / 2.;

        let line = Line::new(
            (glyph_run.offset() as f64, y as f64),
            ((glyph_run.offset() + glyph_run.advance()) as f64, y as f64),
        );
        painter
            .stroke(line, &Stroke::new(width.into()), underline_brush)
            .transform(transform)
            .draw();
    }
    let mut x = glyph_run.offset();
    let y = glyph_run.baseline();
    let run = glyph_run.run();
    let font = run.font();
    let font_size = run.font_size();
    let synthesis = run.synthesis();
    let glyph_xform = synthesis
        .skew()
        .map(|angle| Affine::skew(angle.to_radians().tan() as f64, 0.0));
    let coords = run.normalized_coords();
    let brush = &brushes[style.brush.0];
    let glyphs: SmallVec<[Glyph; 16]> = glyph_run
        .glyphs()
        .map(|glyph| {
            let gx = x + glyph.x;
            let gy = y + glyph.y;
            x += glyph.advance;
            Glyph {
                id: glyph.id,
                x: gx,
                y: gy,
            }
        })
        .collect();
    if let Some(annotation) = annotation {
        painter.push_context_ref(ContextRef::named_str(
            GLYPH_RUN_SOURCE_CONTEXT,
            annotation,
            None,
        ));
    }
    painter
        .glyphs(font, brush)
        .hint(hint)
        .transform(transform)
        .glyph_transform(glyph_xform)
        .font_size(font_size)
        .normalized_coords(coords)
        .draw(&peniko::Style::Fill(Fill::NonZero), &glyphs);
    if annotation.is_some() {
        painter.pop_context();
    }

    if let Some(strikethrough) = &style.strikethrough {
        let strikethrough_brush = &brushes[strikethrough.brush.0];
        let run_metrics = glyph_run.run().metrics();
        let offset = match strikethrough.offset {
            Some(offset) => offset,
            None => run_metrics.strikethrough_offset,
        };
        let width = match strikethrough.size {
            Some(size) => size,
            None => run_metrics.strikethrough_size,
        };
        // The `offset` is the distance from the baseline to the *top* of the strikethrough
        // so we calculate the middle y-position of the strikethrough based on the font's
        // standard strikethrough width.
        // Remember that we are using a y-down coordinate system
        let y = glyph_run.baseline() - offset + run_metrics.strikethrough_size / 2.;

        let line = Line::new(
            (glyph_run.offset() as f64, y as f64),
            ((glyph_run.offset() + glyph_run.advance()) as f64, y as f64),
        );
        painter
            .stroke(line, &Stroke::new(width.into()), strikethrough_brush)
            .transform(transform)
            .draw();
    }
}
fn to_accesskit_color(brush: &Brush) -> Option<accesskit::Color> {
    if let Brush::Solid(color) = brush {
        let rgba = color.to_rgba8();
        Some(accesskit::Color {
            red: rgba.r,
            green: rgba.g,
            blue: rgba.b,
            alpha: rgba.a,
        })
    } else {
        None
    }
}

/// Sets AccessKit text properties from the brush(es) for the given style.
///
/// The `BrushIndex` values of the runs are indices into `brushes`.
pub fn set_accesskit_brush_properties(
    node: &mut accesskit::Node,
    style: &Style<BrushIndex>,
    brushes: &[Brush],
) {
    if let Some(color) = to_accesskit_color(&brushes[style.brush.0]) {
        node.set_foreground_color(color);
    }
    if let Some(deco) = &style.underline
        && let Some(color) = to_accesskit_color(&brushes[deco.brush.0])
    {
        node.set_underline(TextDecoration {
            style: TextDecorationStyle::Solid,
            color,
        });
    }
    if let Some(deco) = &style.strikethrough
        && let Some(color) = to_accesskit_color(&brushes[deco.brush.0])
    {
        node.set_strikethrough(TextDecoration {
            style: TextDecorationStyle::Solid,
            color,
        });
    }
}
