// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Pixel-parity tests: each scene is rendered with `vello_cpu` and, separately, written to a PDF
//! with [`KrillaSink`] and rasterized with `hayro`. The two images must match within a small
//! anti-aliasing tolerance.

use std::path::PathBuf;
use std::sync::Arc;

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::{RenderCache, RenderSettings};
use imaging::record::{self, Glyph, Scene};
use imaging::{BlurredRoundedRect, Composite, GroupRef, Painter};
use imaging_krilla::{KrillaResources, KrillaSink, KrillaSinkOptions, KrillaSinkStats};
use imaging_vello_cpu::VelloCpuRenderer;
use krilla::Document;
use krilla::page::PageSettings;
use kurbo::{Affine, BezPath, Cap, Circle, Join, Rect, RoundedRect, Stroke};
use peniko::{
    Blob, Color, ColorStop, FontData, Gradient, ImageAlphaType, ImageBrush, ImageData, ImageFormat,
};
use skrifa::MetadataProvider as _;

const ROBOTO: &[u8] =
    include_bytes!("../../masonry_testing/resources/fonts/roboto/Roboto-Regular.ttf");

fn with_background(scene: &Scene, w: f64, h: f64) -> Scene {
    let mut full = Scene::new();
    Painter::new(&mut full).fill_rect(Rect::new(0., 0., w, h), Color::WHITE);
    full.append_transformed(scene, Affine::IDENTITY);
    full
}

fn render_vello(scene: &Scene, w: u16, h: u16) -> Vec<u8> {
    let scene = with_background(scene, f64::from(w), f64::from(h));
    let mut renderer = VelloCpuRenderer::new(w, h);
    renderer.render_scene(&scene, w, h).unwrap().data
}

fn write_pdf(scene: &Scene, w: u16, h: u16) -> (Vec<u8>, KrillaSinkStats) {
    let mut document = Document::new();
    let mut resources = KrillaResources::default();
    let stats;
    {
        let mut page =
            document.start_page_with(PageSettings::from_wh(f32::from(w), f32::from(h)).unwrap());
        let mut surface = page.surface();
        let mut sink = KrillaSink::new(
            &mut surface,
            &mut resources,
            Affine::IDENTITY,
            KrillaSinkOptions::default(),
        );
        record::replay(scene, &mut sink);
        stats = sink.finish();
        surface.finish();
        page.finish();
    }
    (document.finish().unwrap(), stats)
}

fn render_pdf(pdf: Vec<u8>) -> (Vec<u8>, u16, u16) {
    let pdf = Pdf::new(Arc::new(pdf)).unwrap();
    let page = &pdf.pages()[0];
    let pixmap = hayro::render(
        page,
        &RenderCache::new(),
        &InterpreterSettings::default(),
        &RenderSettings {
            bg_color: Color::WHITE,
            ..RenderSettings::default()
        },
    );
    let (w, h) = (pixmap.width(), pixmap.height());
    let data = pixmap
        .take_unpremultiplied()
        .into_iter()
        .flat_map(|p| [p.r, p.g, p.b, p.a])
        .collect();
    (data, w, h)
}

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/imaging_krilla_parity");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn save_png(path: &std::path::Path, data: &[u8], w: u16, h: u16) {
    let img = image::RgbaImage::from_raw(u32::from(w), u32::from(h), data.to_vec()).unwrap();
    img.save(path).unwrap();
}

/// Render `scene` both ways and assert the images match.
fn assert_parity(name: &str, scene: &Scene, w: u16, h: u16) -> KrillaSinkStats {
    let expected = render_vello(scene, w, h);
    let (pdf, stats) = write_pdf(scene, w, h);
    let dir = out_dir();
    std::fs::write(dir.join(format!("{name}.pdf")), &pdf).unwrap();
    let (actual, aw, ah) = render_pdf(pdf);
    assert_eq!((aw, ah), (w, h), "{name}: PDF page size mismatch");

    let mut bad = 0_usize;
    let mut total_err = 0_u64;
    let mut diff = vec![0_u8; expected.len()];
    for (i, (e, a)) in expected.chunks(4).zip(actual.chunks(4)).enumerate() {
        let d = (0..3).map(|c| e[c].abs_diff(a[c])).max().unwrap_or(0);
        total_err += u64::from(d);
        if d > 24 {
            bad += 1;
        }
        diff[i * 4..i * 4 + 4].copy_from_slice(&[d.saturating_mul(4), 0, 0, 255]);
    }
    let pixels = expected.len() / 4;
    let bad_ratio = bad as f64 / pixels as f64;
    let mae = total_err as f64 / pixels as f64;
    save_png(&dir.join(format!("{name}.expected.png")), &expected, w, h);
    save_png(&dir.join(format!("{name}.actual.png")), &actual, w, h);
    save_png(&dir.join(format!("{name}.diff.png")), &diff, w, h);
    if bad_ratio > 0.005 || mae > 1.0 {
        panic!(
            "{name}: PDF rendering differs from vello_cpu: {:.3}% pixels off, MAE {mae:.3} (see {})",
            bad_ratio * 100.0,
            dir.display()
        );
    }
    stats
}

#[test]
fn shapes_and_strokes() {
    let mut scene = Scene::new();
    let mut p = Painter::new(&mut scene);
    p.fill_rect(
        Rect::new(10., 10., 90., 60.),
        Color::from_rgb8(0x20, 0x60, 0xd0),
    );
    p.fill(
        RoundedRect::new(110., 10., 190., 60., 12.),
        Color::from_rgba8(0xd0, 0x30, 0x30, 0xc0),
    )
    .draw();
    p.fill(
        Circle::new((250., 40.), 28.),
        Color::from_rgb8(0x30, 0xa0, 0x40),
    )
    .draw();
    let mut path = BezPath::new();
    path.move_to((20., 90.));
    path.line_to((80., 150.));
    path.quad_to((120., 80.), (160., 150.));
    path.curve_to((200., 80.), (240., 200.), (280., 90.));
    p.stroke(
        &path,
        &Stroke::new(6.).with_join(Join::Round).with_caps(Cap::Round),
        Color::BLACK,
    )
    .draw();
    p.stroke(
        Rect::new(20., 170., 280., 190.),
        &Stroke::new(3.).with_dashes(0., [10., 5.]),
        Color::from_rgb8(0x80, 0x20, 0x80),
    )
    .transform(Affine::rotate_about(0.05, kurbo::Point::new(150., 180.)))
    .draw();
    let stats = assert_parity("shapes_and_strokes", &scene, 300, 200);
    assert!(stats.unsupported.is_empty(), "{:?}", stats.unsupported);
}

#[test]
fn gradients() {
    let mut scene = Scene::new();
    let mut p = Painter::new(&mut scene);
    let stops = [
        ColorStop::from((0.0, Color::from_rgb8(0xff, 0x40, 0x40))),
        ColorStop::from((1.0, Color::from_rgb8(0x40, 0x40, 0xff))),
    ];
    let linear = Gradient::new_linear((10., 0.), (140., 0.)).with_stops(stops.as_slice());
    p.fill(Rect::new(10., 10., 140., 90.), &linear).draw();
    let radial = Gradient::new_radial((225., 50.), 45.).with_stops(stops.as_slice());
    p.fill(Rect::new(160., 10., 290., 90.), &radial).draw();
    assert_parity("gradients", &scene, 300, 100);
}

#[test]
fn clips_and_groups() {
    let mut scene = Scene::new();
    let mut p = Painter::new(&mut scene);
    p.with_fill_clip(RoundedRect::new(10., 10., 150., 110., 20.), |p| {
        p.fill_rect(
            Rect::new(0., 0., 300., 200.),
            Color::from_rgb8(0xe0, 0xa0, 0x20),
        );
        p.with_fill_clip(
            kurbo::Shape::to_path(&Circle::new((80., 60.), 30.), 0.01),
            |p| {
                p.fill_rect(
                    Rect::new(0., 0., 300., 200.),
                    Color::from_rgb8(0x20, 0x20, 0x80),
                );
            },
        );
    });
    p.with_group(
        GroupRef::new().with_composite(Composite::new(peniko::BlendMode::default(), 0.5)),
        |p| {
            p.fill_rect(
                Rect::new(170., 20., 250., 100.),
                Color::from_rgb8(0xc0, 0x00, 0x00),
            );
            p.fill_rect(
                Rect::new(210., 60., 290., 140.),
                Color::from_rgb8(0x00, 0x00, 0xc0),
            );
        },
    );
    assert_parity("clips_and_groups", &scene, 300, 150);
}

#[test]
fn images() {
    let (w, h) = (16_u32, 16_u32);
    let mut data = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let on = (x / 4 + y / 4) % 2 == 0;
            data.extend_from_slice(if on {
                &[255, 0, 0, 255]
            } else {
                &[0, 0, 255, 255]
            });
        }
    }
    let image = ImageData {
        data: Blob::new(Arc::new(data)),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width: w,
        height: h,
    };
    let brush = ImageBrush::new(image).with_quality(peniko::ImageQuality::Low);
    let mut scene = Scene::new();
    let mut p = Painter::new(&mut scene);
    p.draw_image(&brush, Affine::translate((20., 20.)) * Affine::scale(4.0));
    assert_parity("images", &scene, 120, 120);
}

#[test]
fn box_shadow_is_rasterized() {
    let mut scene = Scene::new();
    Painter::new(&mut scene).blurred_rounded_rect(BlurredRoundedRect {
        transform: Affine::IDENTITY,
        rect: Rect::new(40., 30., 160., 90.),
        color: Color::from_rgba8(0, 0, 0, 160),
        radius: 8.,
        std_dev: 6.,
        composite: Composite::default(),
    });
    let stats = assert_parity("box_shadow", &scene, 200, 120);
    assert_eq!(
        stats.raster_fallbacks, 1,
        "shadow should use the raster fallback"
    );
}

/// Lay out `text` with simple left-to-right cmap/hmtx "shaping".
fn simple_glyphs(font: &skrifa::FontRef<'_>, text: &str, size: f32, x: f32, y: f32) -> Vec<Glyph> {
    let charmap = font.charmap();
    let metrics = font.glyph_metrics(
        skrifa::instance::Size::new(size),
        skrifa::instance::LocationRef::default(),
    );
    let mut pen = x;
    text.chars()
        .map(|c| {
            let gid = charmap.map(c).unwrap_or_default();
            let g = Glyph {
                id: gid.to_u32(),
                x: pen,
                y,
            };
            pen += metrics.advance_width(gid).unwrap_or_default();
            g
        })
        .collect()
}

#[test]
fn text_is_real_text() {
    let font_data = FontData::new(Blob::new(Arc::new(ROBOTO.to_vec())), 0);
    let font_ref = skrifa::FontRef::new(ROBOTO).unwrap();
    let glyphs = simple_glyphs(&font_ref, "Hello, PDF world!", 24.0, 10.0, 40.0);
    let italic = simple_glyphs(&font_ref, "Synthetic italic", 18.0, 10.0, 80.0);
    let mut scene = Scene::new();
    let mut p = Painter::new(&mut scene);
    p.glyphs(&font_data, Color::BLACK)
        .font_size(24.0)
        .draw(&peniko::Style::Fill(peniko::Fill::NonZero), glyphs);
    p.glyphs(&font_data, Color::from_rgb8(0x20, 0x40, 0xa0))
        .font_size(18.0)
        .glyph_transform(Some(Affine::skew(0.25, 0.0)))
        .draw(&peniko::Style::Fill(peniko::Fill::NonZero), italic);
    let stats = assert_parity("text", &scene, 260, 100);
    assert_eq!(stats.glyph_runs_unmapped, 2, "text should use reverse cmap");
    assert_eq!(stats.glyph_runs_outlined, 0, "text must not be outlined");
    let (pdf, _) = write_pdf(&scene, 260, 100);
    assert!(
        pdf.windows(b"/FontFile2".len()).any(|w| w == b"/FontFile2"),
        "font should be embedded as TrueType"
    );
    assert!(
        pdf.windows(b"/ToUnicode".len()).any(|w| w == b"/ToUnicode"),
        "font should have a ToUnicode map"
    );
}
