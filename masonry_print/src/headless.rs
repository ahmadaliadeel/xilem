// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Rendering Masonry widget trees without a window.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use masonry::app::{RenderRoot, RenderRootOptions, RenderRootSignal, WindowSizePolicy};
use masonry::core::{DefaultProperties, NewWidget, Widget, WindowEvent};
use masonry::dpi::PhysicalSize;
use masonry::kurbo::Size;

use crate::capture::{Capture, capture};
use crate::fonts::FontSetup;

/// Options for a [`HeadlessRoot`].
#[derive(Clone, Debug)]
pub struct HeadlessOptions {
    /// Size of the (virtual) window, in logical pixels.
    pub size: Size,
    /// Scale factor. Headless print roots normally use `1.0`: content is vector anyway.
    pub scale_factor: f64,
    /// Default properties (theme) of widgets.
    pub default_properties: Arc<DefaultProperties>,
    /// Fonts.
    pub fonts: FontSetup,
}

impl HeadlessOptions {
    /// Options with the given size and default properties, using system fonts.
    pub fn new(size: Size, default_properties: Arc<DefaultProperties>) -> Self {
        Self {
            size,
            scale_factor: 1.0,
            default_properties,
            fonts: FontSetup::system(),
        }
    }

    /// Sets the fonts.
    pub fn with_fonts(mut self, fonts: FontSetup) -> Self {
        self.fonts = fonts;
        self
    }
}

fn physical_size(size: Size, scale_factor: f64) -> PhysicalSize<u32> {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "window sizes are small positive numbers"
    )]
    PhysicalSize::new(
        (size.width * scale_factor).ceil().max(1.0) as u32,
        (size.height * scale_factor).ceil().max(1.0) as u32,
    )
}

/// A Masonry [`RenderRoot`] without a window, used to lay out and paint content for printing.
pub struct HeadlessRoot {
    root: RenderRoot,
    signals: Rc<RefCell<Vec<RenderRootSignal>>>,
    scale_factor: f64,
}

impl std::fmt::Debug for HeadlessRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessRoot")
            .field("size", &self.root.size())
            .finish_non_exhaustive()
    }
}

impl HeadlessRoot {
    /// Creates a render root for the given widget.
    pub fn new(widget: NewWidget<impl Widget + ?Sized>, options: HeadlessOptions) -> Self {
        let signals = Rc::new(RefCell::new(Vec::new()));
        let sink = signals.clone();
        let mut root = RenderRoot::new(
            widget,
            move |signal| sink.borrow_mut().push(signal),
            RenderRootOptions {
                default_properties: options.default_properties,
                use_system_fonts: options.fonts.system_fonts,
                size_policy: WindowSizePolicy::User,
                size: physical_size(options.size, options.scale_factor),
                scale_factor: options.scale_factor,
                test_font: None,
            },
        );
        options.fonts.apply(&mut root);
        let mut this = Self {
            root,
            signals,
            scale_factor: options.scale_factor,
        };
        this.process_signals();
        this
    }

    /// The underlying render root.
    pub fn render_root(&mut self) -> &mut RenderRoot {
        &mut self.root
    }

    /// Resizes the virtual window (in logical pixels).
    pub fn resize(&mut self, size: Size) {
        self.root
            .handle_window_event(WindowEvent::Resize(physical_size(size, self.scale_factor)));
        self.process_signals();
    }

    /// Handles signals emitted by the render root (e.g. new layers for popups).
    pub fn process_signals(&mut self) {
        loop {
            let signals = std::mem::take(&mut *self.signals.borrow_mut());
            if signals.is_empty() {
                break;
            }
            for signal in signals {
                match signal {
                    RenderRootSignal::NewLayer(_, widget, pos) => {
                        self.root.add_layer(widget, pos);
                    }
                    RenderRootSignal::RemoveLayer(id) => self.root.remove_layer(id),
                    RenderRootSignal::RepositionLayer(id, pos) => {
                        self.root.reposition_layer(id, pos);
                    }
                    _ => {}
                }
            }
        }
    }

    /// Lays out and paints the widget tree, returning its paint output and semantics.
    pub fn capture(&mut self) -> Capture {
        self.process_signals();
        let capture = capture(&mut self.root);
        self.process_signals();
        capture
    }
}
