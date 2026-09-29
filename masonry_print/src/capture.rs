// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Capturing the paint output and semantics of a render root.

use std::collections::HashMap;
use std::sync::Arc;

use imaging_krilla::usvg::Tree;
use masonry::app::{RenderRoot, VisualLayerPlan};
use masonry::core::{Widget, WidgetRef};
use masonry::kurbo::{Rect, Size};
use masonry::peniko::Color;
use masonry::widgets::Svg;

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
    /// The SVG trees of `Svg` widgets, by raw widget id, so they can be printed as vectors.
    pub svgs: HashMap<u64, Arc<Tree>>,
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
    let base_layer = root.get_layer_root(0);
    let content_bounds = base_layer.ctx().bounding_box();
    let mut svgs = HashMap::new();
    collect_svgs(base_layer, &mut svgs);
    Capture {
        plan,
        access: AccessIndex::new(&tree),
        window_size,
        content_bounds,
        background: None,
        svgs,
    }
}

fn collect_svgs(widget: WidgetRef<'_, dyn Widget>, svgs: &mut HashMap<u64, Arc<Tree>>) {
    if let Some(svg) = widget.downcast::<Svg>() {
        svgs.insert(widget.id().to_raw(), svg.inner().tree().clone());
    }
    for child in widget.children() {
        collect_svgs(child, svgs);
    }
}
