// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Capturing the paint output and semantics of a render root.

use masonry::app::{RenderRoot, VisualLayerPlan};
use masonry::kurbo::{Rect, Size};
use masonry::peniko::Color;

use crate::access::AccessIndex;

/// The paint output and semantics of a widget tree at one point in time.
///
/// All coordinates are logical pixels in the window's coordinate space.
#[derive(Debug)]
pub struct Capture {
    /// The annotated paint output.
    pub plan: VisualLayerPlan,
    /// The accessibility tree.
    pub access: AccessIndex,
    /// The size of the window, in logical pixels.
    pub window_size: Size,
    /// The bounds of the base layer's root widget (its full content), in logical pixels.
    pub content_bounds: Rect,
    /// Background color painted behind the content, if any.
    pub background: Option<Color>,
}

impl Capture {
    /// Sets the background color.
    pub fn with_background(mut self, color: Option<Color>) -> Self {
        self.background = color;
        self
    }
}

/// Captures the current state of a render root, for printing.
///
/// This runs the rewrite and paint passes with
/// [paint annotations](RenderRoot::set_paint_annotations) enabled, and builds the full
/// accessibility tree. The annotation setting of the root is restored afterwards.
pub fn capture(root: &mut RenderRoot) -> Capture {
    let annotations = root.paint_annotations();
    root.set_paint_annotations(true);
    let (plan, _) = root.redraw();
    let tree = root.full_access_tree();
    root.set_paint_annotations(annotations);

    let physical = root.size();
    let scale = root.scale_factor();
    let window_size = Size::new(
        f64::from(physical.width) / scale,
        f64::from(physical.height) / scale,
    );
    let content_bounds = root.get_layer_root(0).ctx().bounding_box();
    Capture {
        plan,
        access: AccessIndex::new(&tree),
        window_size,
        content_bounds,
        background: None,
    }
}
