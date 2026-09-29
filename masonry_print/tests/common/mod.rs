// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Shared helpers for `masonry_print` integration tests.

#![allow(dead_code, unreachable_pub, reason = "shared test helpers")]

use std::path::PathBuf;
use std::sync::Arc;

use imaging_vello_cpu::VelloCpuRenderer;
use masonry::core::StyleProperty;
use masonry::imaging::Painter;
use masonry::imaging::record::Scene;
use masonry::kurbo::Rect;
use masonry::parley::fontique::Language;
use masonry::peniko::{Blob, Color};
use masonry_print::testing::{Raster, rasterize};
use masonry_print::{Capture, FontFallback, FontSetup};

/// Fonts for tests: Roboto for Latin, bundled Noto fonts for Arabic and Urdu.
pub fn fonts() -> FontSetup {
    FontSetup::only([Blob::new(Arc::new(masonry_testing::ROBOTO))])
        .with_fallback(FontFallback::new(*b"Latn", &["Roboto"]))
        .with_fallback(FontFallback::new(*b"Zyyy", &["Roboto"]))
        .with_noto_arabic()
}

/// A locale style property.
pub fn locale(tag: &str) -> StyleProperty {
    StyleProperty::Locale(Some(Language::parse(tag).expect("valid language tag")))
}

/// Directory for test output.
pub fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/masonry_print_tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Renders a capture with `vello_cpu`, like Masonry does on screen.
pub fn render_capture(capture: &Capture, background: Color) -> Raster {
    let size = capture.window_size;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "test"
    )]
    let (w, h) = (size.width.ceil() as u16, size.height.ceil() as u16);
    let mut scene = Scene::new();
    Painter::new(&mut scene).fill_rect(Rect::new(0., 0., f64::from(w), f64::from(h)), background);
    for layer in &capture.plan.layers {
        if let masonry::app::VisualLayerKind::Scene(layer_scene) = &layer.kind {
            scene.append_transformed(layer_scene, layer.transform);
        }
    }
    let image = VelloCpuRenderer::new(w, h)
        .render_scene(&scene, w, h)
        .unwrap();
    Raster {
        width: image.width,
        height: image.height,
        data: image.data,
    }
}

pub fn save_png(raster: &Raster, name: &str) {
    let img = image::RgbaImage::from_raw(raster.width, raster.height, raster.data.clone()).unwrap();
    img.save(out_dir().join(name)).unwrap();
}

/// Compares two rasters; returns (fraction of pixels differing by more than 24, mean abs error).
pub fn compare(expected: &Raster, actual: &Raster) -> (f64, f64) {
    assert_eq!(
        (expected.width, expected.height),
        (actual.width, actual.height),
        "raster size mismatch"
    );
    let mut bad = 0_usize;
    let mut total = 0_u64;
    for (e, a) in expected.data.chunks(4).zip(actual.data.chunks(4)) {
        let d = (0..3).map(|c| e[c].abs_diff(a[c])).max().unwrap_or(0);
        total += u64::from(d);
        if d > 24 {
            bad += 1;
        }
    }
    let pixels = (expected.data.len() / 4) as f64;
    (bad as f64 / pixels, total as f64 / pixels)
}

/// Asserts that a single-page PDF made from `capture` (at 0.75pt/px, no margins) looks like
/// the on-screen rendering.
pub fn assert_pixel_parity(name: &str, capture: &Capture, pdf: &[u8], background: Color) {
    let expected = render_capture(capture, background);
    let actual = rasterize(pdf, 0, 1.0 / 0.75);
    save_png(&expected, &format!("{name}.screen.png"));
    save_png(&actual, &format!("{name}.pdf.png"));
    let (bad, mae) = compare(&expected, &actual);
    assert!(
        bad < 0.01 && mae < 1.5,
        "{name}: PDF differs from screen rendering: {:.3}% pixels off, MAE {mae:.3}",
        bad * 100.0
    );
}
