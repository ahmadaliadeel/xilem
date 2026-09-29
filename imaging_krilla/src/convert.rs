// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Conversions from kurbo/peniko types to krilla types.

use krilla::color::rgb;
use krilla::geom::{Path, PathBuilder, Transform};
use krilla::num::NormalizedF32;
use krilla::paint::{
    FillRule, LineCap, LineJoin, LinearGradient, Paint, RadialGradient, SpreadMethod, Stop,
    StrokeDash, SweepGradient,
};
use kurbo::{Affine, BezPath, PathEl};
use peniko::color::{AlphaColor, Srgb};
use peniko::{Extend, Gradient, GradientKind};

/// Convert a kurbo affine to a krilla transform.
pub(crate) fn transform(affine: Affine) -> Transform {
    let [a, b, c, d, e, f] = affine.as_coeffs();
    Transform::from_row(a as f32, b as f32, c as f32, d as f32, e as f32, f as f32)
}

/// Convert a kurbo path to a krilla path.
///
/// Returns `None` for empty or degenerate paths.
pub(crate) fn path(path: &BezPath) -> Option<Path> {
    let mut builder = PathBuilder::new();
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => builder.move_to(p.x as f32, p.y as f32),
            PathEl::LineTo(p) => builder.line_to(p.x as f32, p.y as f32),
            PathEl::QuadTo(p1, p2) => {
                builder.quad_to(p1.x as f32, p1.y as f32, p2.x as f32, p2.y as f32);
            }
            PathEl::CurveTo(p1, p2, p3) => builder.cubic_to(
                p1.x as f32,
                p1.y as f32,
                p2.x as f32,
                p2.y as f32,
                p3.x as f32,
                p3.y as f32,
            ),
            PathEl::ClosePath => builder.close(),
        }
    }
    builder.finish()
}

pub(crate) fn fill_rule(rule: peniko::Fill) -> FillRule {
    match rule {
        peniko::Fill::NonZero => FillRule::NonZero,
        peniko::Fill::EvenOdd => FillRule::EvenOdd,
    }
}

pub(crate) fn line_cap(cap: kurbo::Cap) -> LineCap {
    match cap {
        kurbo::Cap::Butt => LineCap::Butt,
        kurbo::Cap::Round => LineCap::Round,
        kurbo::Cap::Square => LineCap::Square,
    }
}

pub(crate) fn line_join(join: kurbo::Join) -> LineJoin {
    match join {
        kurbo::Join::Bevel => LineJoin::Bevel,
        kurbo::Join::Miter => LineJoin::Miter,
        kurbo::Join::Round => LineJoin::Round,
    }
}

pub(crate) fn dash(stroke: &kurbo::Stroke) -> Option<StrokeDash> {
    if stroke.dash_pattern.is_empty() {
        return None;
    }
    Some(StrokeDash {
        array: stroke.dash_pattern.iter().map(|d| *d as f32).collect(),
        offset: stroke.dash_offset as f32,
    })
}

pub(crate) fn normalized(value: f32) -> NormalizedF32 {
    NormalizedF32::new(value.clamp(0.0, 1.0)).unwrap_or(NormalizedF32::ONE)
}

/// Convert an sRGB color to a krilla color and its alpha.
pub(crate) fn color(color: AlphaColor<Srgb>) -> (rgb::Color, f32) {
    let [r, g, b, a] = color.to_rgba8().to_u8_array();
    (rgb::Color::new(r, g, b), f32::from(a) / 255.0)
}

fn spread(extend: Extend) -> SpreadMethod {
    match extend {
        Extend::Pad => SpreadMethod::Pad,
        Extend::Repeat => SpreadMethod::Repeat,
        Extend::Reflect => SpreadMethod::Reflect,
    }
}

/// Number of sub-stops used to approximate interpolation in a non-sRGB color space.
const RESAMPLE_STEPS: usize = 8;

fn stops(gradient: &Gradient, alpha: f32) -> Vec<Stop> {
    use peniko::color::ColorSpaceTag;
    let needs_resample = gradient.interpolation_cs != ColorSpaceTag::Srgb;
    let mut out = Vec::with_capacity(gradient.stops.len());
    let mut push = |offset: f32, c: AlphaColor<Srgb>| {
        let (color, a) = color(c);
        out.push(Stop {
            offset: normalized(offset),
            color: color.into(),
            opacity: normalized(a * alpha),
        });
    };
    for (i, stop) in gradient.stops.iter().enumerate() {
        if needs_resample && i > 0 {
            // Approximate interpolation in the requested color space by adding intermediate
            // stops, which PDF then interpolates linearly in sRGB.
            let prev = &gradient.stops[i - 1];
            for step in 1..RESAMPLE_STEPS {
                let t = step as f32 / RESAMPLE_STEPS as f32;
                let c = prev.color.interpolate(
                    stop.color,
                    gradient.interpolation_cs,
                    gradient.hue_direction,
                );
                let offset = prev.offset + (stop.offset - prev.offset) * t;
                push(offset, c.eval(t).to_alpha_color::<Srgb>());
            }
        }
        push(stop.offset, stop.color.to_alpha_color::<Srgb>());
    }
    out
}

/// Convert a peniko gradient to a krilla paint.
///
/// `brush_transform` maps gradient space to the local space of the draw.
pub(crate) fn gradient(gradient: &Gradient, brush_transform: Affine, alpha: f32) -> Paint {
    let spread_method = spread(gradient.extend);
    let stops = stops(gradient, alpha);
    let transform = transform(brush_transform);
    match gradient.kind {
        GradientKind::Linear(pos) => LinearGradient {
            x1: pos.start.x as f32,
            y1: pos.start.y as f32,
            x2: pos.end.x as f32,
            y2: pos.end.y as f32,
            transform,
            spread_method,
            stops,
            anti_alias: true,
        }
        .into(),
        GradientKind::Radial(pos) => RadialGradient {
            fx: pos.start_center.x as f32,
            fy: pos.start_center.y as f32,
            fr: pos.start_radius,
            cx: pos.end_center.x as f32,
            cy: pos.end_center.y as f32,
            cr: pos.end_radius,
            transform,
            spread_method,
            stops,
            anti_alias: true,
        }
        .into(),
        GradientKind::Sweep(pos) => SweepGradient {
            cx: pos.center.x as f32,
            cy: pos.center.y as f32,
            start_angle: pos.start_angle.to_degrees(),
            end_angle: pos.end_angle.to_degrees(),
            transform,
            spread_method,
            stops,
            anti_alias: true,
        }
        .into(),
    }
}

pub(crate) fn blend_mode(mix: peniko::Mix) -> Option<krilla::blend::BlendMode> {
    use krilla::blend::BlendMode as B;
    use peniko::Mix;
    Some(match mix {
        Mix::Normal => return None,
        Mix::Multiply => B::Multiply,
        Mix::Screen => B::Screen,
        Mix::Overlay => B::Overlay,
        Mix::Darken => B::Darken,
        Mix::Lighten => B::Lighten,
        Mix::ColorDodge => B::ColorDodge,
        Mix::ColorBurn => B::ColorBurn,
        Mix::HardLight => B::HardLight,
        Mix::SoftLight => B::SoftLight,
        Mix::Difference => B::Difference,
        Mix::Exclusion => B::Exclusion,
        Mix::Hue => B::Hue,
        Mix::Saturation => B::Saturation,
        Mix::Color => B::Color,
        Mix::Luminosity => B::Luminosity,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_roundtrip() {
        let a = Affine::new([1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        let t = transform(a);
        assert_eq!(
            [t.sx(), t.ky(), t.kx(), t.sy(), t.tx(), t.ty()],
            [1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            "coefficients should map in PDF matrix order"
        );
    }

    #[test]
    fn empty_path_is_none() {
        assert!(path(&BezPath::new()).is_none(), "empty paths are skipped");
    }
}
