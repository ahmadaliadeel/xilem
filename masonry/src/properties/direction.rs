// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

use std::any::TypeId;

use crate::core::{Property, UpdateCtx};

/// The inline direction in which a container places its children.
///
/// This is the equivalent of the CSS `direction` property for layout:
/// with [`Rtl`](Self::Rtl), [`Flex`](crate::widgets::Flex) rows place their first child
/// on the right, and "start" alignments (main and cross axis) refer to the right edge.
/// [`Grid`](crate::widgets::Grid) places column 0 on the right.
///
/// It is used to lay out user interfaces and documents in right-to-left languages,
/// such as Arabic, Hebrew, Persian and Urdu.
/// Text direction is controlled separately by [`BaseDirection`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LayoutDirection {
    /// Left to right (the default).
    #[default]
    Ltr,
    /// Right to left: layouts are mirrored horizontally.
    Rtl,
}

impl Property for LayoutDirection {
    fn static_default() -> &'static Self {
        &Self::Ltr
    }
}

impl LayoutDirection {
    /// Helper function to be called in [`Widget::property_changed`](crate::core::Widget::property_changed).
    pub fn prop_changed(ctx: &mut UpdateCtx<'_>, property_type: TypeId) {
        if property_type != TypeId::of::<Self>() {
            return;
        }
        ctx.request_layout();
    }

    /// Returns `true` for [`LayoutDirection::Rtl`].
    pub fn is_rtl(self) -> bool {
        self == Self::Rtl
    }
}

/// The base (paragraph) direction of text.
///
/// By default ([`Auto`](Self::Auto)), the direction of a paragraph is determined by its first
/// strong character, following the Unicode Bidirectional Algorithm. For example,
/// `"PDF فائل"` is laid out left-to-right because it starts with a Latin letter.
///
/// Setting [`Rtl`](Self::Rtl) or [`Ltr`](Self::Ltr) forces the paragraph direction, which also
/// determines what [`TextAlign::Start`](crate::TextAlign::Start) and
/// [`TextAlign::End`](crate::TextAlign::End) mean.
///
/// This is supported by [`Label`](crate::widgets::Label).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BaseDirection {
    /// Determine the direction from the text content (the default).
    #[default]
    Auto,
    /// Force left-to-right paragraphs.
    Ltr,
    /// Force right-to-left paragraphs.
    Rtl,
}

impl Property for BaseDirection {
    fn static_default() -> &'static Self {
        &Self::Auto
    }
}

impl BaseDirection {
    /// Helper function to be called in [`Widget::property_changed`](crate::core::Widget::property_changed).
    pub fn prop_changed(ctx: &mut UpdateCtx<'_>, property_type: TypeId) {
        if property_type != TypeId::of::<Self>() {
            return;
        }
        ctx.request_layout();
    }

    /// The invisible Unicode direction mark that forces this direction when placed at the
    /// start of a paragraph, if any.
    pub fn direction_mark(self) -> Option<char> {
        match self {
            Self::Auto => None,
            // LEFT-TO-RIGHT MARK
            Self::Ltr => Some('\u{200E}'),
            // RIGHT-TO-LEFT MARK
            Self::Rtl => Some('\u{200F}'),
        }
    }
}
