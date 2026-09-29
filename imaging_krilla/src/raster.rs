// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Rasterization of content without a PDF equivalent.

use imaging::record::Scene;
use imaging::{BlurredRoundedRect, Composite, PaintSink as _};
use imaging_vello_cpu::VelloCpuRenderer;
use krilla::image::Image;
use kurbo::{Affine, Rect};

/// Maximum width/height of a rasterized element, in pixels.
const MAX_RASTER_SIZE: f64 = 4096.0;

/// Rasterize a blurred rounded rectangle.
///
/// `transform` maps the rectangle to output coordinates. Returns the image and the output-space
/// rectangle it should be drawn into.
pub(crate) fn blurred_rounded_rect(
    draw: &BlurredRoundedRect,
    transform: Affine,
    scale: f32,
) -> Option<(Image, Rect)> {
    // A gaussian is visually zero beyond three standard deviations.
    let pad = draw.std_dev * 3.0 + 1.0;
    let bounds = transform
        .transform_rect_bbox(draw.rect.inflate(pad, pad))
        .expand();
    if !(bounds.width() > 0.0 && bounds.height() > 0.0) || !bounds.is_finite() {
        return None;
    }
    let scale = f64::from(scale);
    let width = (bounds.width() * scale).ceil().clamp(1.0, MAX_RASTER_SIZE);
    let height = (bounds.height() * scale).ceil().clamp(1.0, MAX_RASTER_SIZE);
    let to_pixels = Affine::scale_non_uniform(width / bounds.width(), height / bounds.height())
        * Affine::translate(-bounds.origin().to_vec2());

    let mut scene = Scene::new();
    scene.blurred_rounded_rect(BlurredRoundedRect {
        transform: to_pixels * transform,
        // Blending is applied when the image is drawn.
        composite: Composite::new(peniko::BlendMode::default(), draw.composite.alpha),
        ..*draw
    });
    #[expect(
        clippy::cast_possible_truncation,
        reason = "width and height are clamped to MAX_RASTER_SIZE"
    )]
    let (w, h) = (width as u16, height as u16);
    let mut renderer = VelloCpuRenderer::new(w, h);
    let image = renderer.render_scene(&scene, w, h).ok()?;
    Some((
        Image::from_rgba8(image.data, image.width, image.height),
        bounds,
    ))
}
