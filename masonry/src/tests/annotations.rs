// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Tests for paint annotations (widget and text source contexts) and RTL layout support.

use std::sync::Arc;

use crate::app::{RenderRoot, RenderRootOptions, VisualLayerKind, WindowSizePolicy};
use crate::core::{
    GLYPH_RUN_SOURCE_CONTEXT, GlyphRunSource, NewWidget, PaintLayerMode, Widget, WidgetTag,
};
use crate::dpi::PhysicalSize;
use crate::imaging::record::{Glyph, Scene};
use crate::imaging::{
    BlurredRoundedRect, ClipRef, ContextKindRef, ContextRef, ContextValueRef, FillRef, GlyphRunRef,
    GroupRef, PaintSink, StrokeRef,
};
use crate::layout::{AsUnit, Length};
use crate::palette::css::{BLUE, RED};
use crate::peniko::Blob;
use crate::properties::{BaseDirection, Dimensions, LayoutDirection};
use crate::testing::{ModularWidget, ROBOTO, TestHarness};
use crate::theme::test_property_set;
use crate::widgets::{Flex, Grid, GridParams, Label, SizedBox};

fn create_render_root(root_widget: NewWidget<impl Widget>, width: u32) -> RenderRoot {
    create_render_root_with_font(root_widget, width, Blob::new(Arc::new(ROBOTO)))
}

fn create_render_root_with_font(
    root_widget: NewWidget<impl Widget>,
    width: u32,
    font: Blob<u8>,
) -> RenderRoot {
    RenderRoot::new(
        root_widget,
        |_| {},
        RenderRootOptions {
            default_properties: Arc::new(test_property_set()),
            use_system_fonts: false,
            size_policy: WindowSizePolicy::User,
            size: PhysicalSize::new(width, 40),
            scale_factor: 1.0,
            test_font: Some(font),
        },
    )
}

/// What a [`Recorder`] saw for one glyph run.
#[derive(Debug)]
struct RecordedRun {
    source: Option<GlyphRunSource>,
    widgets: Vec<u64>,
    glyphs: Vec<Glyph>,
}

/// A sink which records drawing commands without contexts, and glyph runs with their contexts.
#[derive(Default)]
struct Recorder {
    scene: Scene,
    contexts: Vec<(Option<u64>, Option<String>)>,
    runs: Vec<RecordedRun>,
}

impl PaintSink for Recorder {
    fn push_context(&mut self, context: ContextRef<'_>) {
        let entry = match (context.kind, context.value) {
            (ContextKindRef::Widget, ContextValueRef::U64(id)) => (Some(id), None),
            (ContextKindRef::Named(GLYPH_RUN_SOURCE_CONTEXT), ContextValueRef::Str(s)) => {
                (None, Some(s.to_string()))
            }
            _ => (None, None),
        };
        self.contexts.push(entry);
    }
    fn pop_context(&mut self) {
        self.contexts.pop().expect("unbalanced contexts");
    }
    fn push_clip(&mut self, clip: ClipRef<'_>) {
        PaintSink::push_clip(&mut self.scene, clip);
    }
    fn pop_clip(&mut self) {
        PaintSink::pop_clip(&mut self.scene);
    }
    fn push_group(&mut self, group: GroupRef<'_>) {
        PaintSink::push_group(&mut self.scene, group);
    }
    fn pop_group(&mut self) {
        PaintSink::pop_group(&mut self.scene);
    }
    fn fill(&mut self, draw: FillRef<'_>) {
        PaintSink::fill(&mut self.scene, draw);
    }
    fn stroke(&mut self, draw: StrokeRef<'_>) {
        PaintSink::stroke(&mut self.scene, draw);
    }
    fn glyph_run(&mut self, draw: GlyphRunRef<'_>, glyphs: &mut dyn Iterator<Item = Glyph>) {
        let glyphs: Vec<Glyph> = glyphs.collect();
        self.runs.push(RecordedRun {
            source: self
                .contexts
                .iter()
                .rev()
                .find_map(|(_, s)| s.as_deref())
                .and_then(GlyphRunSource::decode),
            widgets: self.contexts.iter().filter_map(|(id, _)| *id).collect(),
            glyphs: glyphs.clone(),
        });
        PaintSink::glyph_run(&mut self.scene, draw, &mut glyphs.into_iter());
    }
    fn blurred_rounded_rect(&mut self, draw: BlurredRoundedRect) {
        PaintSink::blurred_rounded_rect(&mut self.scene, draw);
    }
}

fn record(root: &mut RenderRoot) -> Recorder {
    let (plan, _) = root.redraw();
    for layer in &plan.layers {
        if let VisualLayerKind::Scene(scene) = &layer.kind {
            scene.validate().expect("annotated scenes must be valid");
        }
    }
    let mut recorder = Recorder::default();
    plan.replay_into(&mut recorder);
    assert!(recorder.contexts.is_empty(), "contexts must be balanced");
    recorder
}

#[test]
fn glyph_run_source_roundtrip() {
    let source = GlyphRunSource {
        text: "a;b:c,d".into(),
        glyph_ranges: vec![0..1, 1..3, 3..7, 3..7],
    };
    assert_eq!(GlyphRunSource::decode(&source.encode()), Some(source));
    assert_eq!(GlyphRunSource::decode("v0;;x"), None);
    assert_eq!(GlyphRunSource::decode("v1;0:9;short"), None);
}

#[test]
fn annotations_do_not_change_drawing() {
    let tree = || {
        let isolated = NewWidget::new(
            ModularWidget::new(())
                .measure_fn(|_, _, _, _, _, _| 20.px())
                .paint_fn(|_, ctx, _, scene| {
                    ctx.set_paint_layer_mode(PaintLayerMode::IsolatedScene);
                    scene.fill(ctx.content_box(), BLUE).draw();
                }),
        );
        let leading = NewWidget::new(
            ModularWidget::new(())
                .measure_fn(|_, _, _, _, _, _| 20.px())
                .paint_fn(|_, ctx, _, scene| {
                    scene.fill(ctx.content_box(), RED).draw();
                }),
        );
        Flex::row()
            .with_fixed(leading)
            .with_fixed(isolated)
            .with_fixed(Label::new("Text").prepare())
            .prepare()
    };

    // Share the font, so that glyph runs reference the same blob.
    let font = Blob::new(Arc::new(ROBOTO));
    let mut plain = create_render_root_with_font(tree(), 120, font.clone());
    let plain = record(&mut plain);
    let mut annotated_root = create_render_root_with_font(tree(), 120, font);
    annotated_root.set_paint_annotations(true);
    let annotated = record(&mut annotated_root);

    assert!(
        plain.scene == annotated.scene,
        "annotations must not change drawing commands"
    );
    assert!(plain.runs.iter().all(|run| run.widgets.is_empty()));
    assert!(annotated.runs.iter().all(|run| !run.widgets.is_empty()));

    // Turning annotations off again removes them.
    annotated_root.set_paint_annotations(false);
    let off = record(&mut annotated_root);
    assert!(off.runs.iter().all(|run| run.widgets.is_empty()));
}

#[test]
fn label_glyph_runs_carry_source_text() {
    let tag = WidgetTag::named("label");
    let label = NewWidget::new(Label::new("Hello, world")).with_tag(tag);
    let mut root = create_render_root(Flex::column().with_fixed(label).prepare(), 200);
    root.set_paint_annotations(true);
    let recorder = record(&mut root);

    let label_id = root.get_widget_with_tag(tag).unwrap().id().to_raw();
    let run = &recorder.runs[0];
    let source = run.source.as_ref().expect("glyph run should have a source");
    assert_eq!(source.text, "Hello, world");
    assert_eq!(source.glyph_ranges.len(), run.glyphs.len());
    assert_eq!(source.glyph_ranges[0], 0..1);
    assert_eq!(run.widgets.last(), Some(&label_id));
}

#[test]
fn base_direction_rtl_aligns_to_the_right_without_marks_in_text() {
    let run_start = |direction: BaseDirection| {
        let label = Label::new("abc")
            .prepare()
            .with_props((direction, Dimensions::fixed(200.px(), Length::px(20.))));
        let mut root = create_render_root(Flex::column().with_fixed(label).prepare(), 200);
        root.set_paint_annotations(true);
        let recorder = record(&mut root);
        let run = recorder
            .runs
            .iter()
            .find(|run| !run.glyphs.is_empty())
            .unwrap();
        let source = run.source.clone().unwrap();
        (run.glyphs[0].x, source)
    };

    let (ltr_x, ltr_source) = run_start(BaseDirection::Auto);
    let (rtl_x, rtl_source) = run_start(BaseDirection::Rtl);
    assert!(ltr_x < 10.0, "LTR text starts at the left, got {ltr_x}");
    assert!(rtl_x > 150.0, "RTL paragraph is right aligned, got {rtl_x}");
    assert_eq!(ltr_source.text, "abc");
    assert_eq!(
        rtl_source.text, "abc",
        "direction marks must not be in the text"
    );
}

#[test]
fn flex_and_grid_mirror_in_rtl() {
    let first = WidgetTag::named("first");
    let cell = WidgetTag::named("cell");
    let child = |tag| {
        NewWidget::new(SizedBox::empty())
            .with_tag(tag)
            .with_props(Dimensions::fixed(20.px(), 20.px()))
    };
    let first_x = |direction: LayoutDirection| {
        let row = Flex::row()
            .with_fixed(child(first))
            .with_fixed(
                NewWidget::new(SizedBox::empty()).with_props(Dimensions::fixed(20.px(), 20.px())),
            )
            .prepare()
            .with_props((direction, Dimensions::fixed(100.px(), 20.px())));
        let harness = TestHarness::create_with_size(test_property_set(), row, (100, 20));
        harness.get_widget(first).ctx().bounding_box().x0
    };
    assert_eq!(first_x(LayoutDirection::Ltr), 0.0);
    assert_eq!(first_x(LayoutDirection::Rtl), 80.0);

    let cell_x = |direction: LayoutDirection| {
        let grid = Grid::with_dimensions(2, 1)
            .with(child(cell), GridParams::pos(0, 0))
            .prepare()
            .with_props((direction, Dimensions::fixed(100.px(), 20.px())));
        let harness = TestHarness::create_with_size(test_property_set(), grid, (100, 20));
        harness.get_widget(cell).ctx().bounding_box().x0
    };
    assert_eq!(cell_x(LayoutDirection::Ltr), 0.0);
    // Each of the two columns is 50 wide, so column 0 starts at 50 when mirrored.
    assert_eq!(cell_x(LayoutDirection::Rtl), 50.0);
}
