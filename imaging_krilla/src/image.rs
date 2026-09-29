// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Conversion and caching of peniko images to krilla images.

use std::collections::HashMap;

use krilla::image::Image;
use peniko::{ImageAlphaType, ImageData, ImageFormat, ImageQuality};

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct ImageKey {
    blob: u64,
    width: u32,
    height: u32,
    bgra: bool,
    premultiplied: bool,
    interpolate: bool,
}

/// A document-lifetime cache of krilla images.
#[derive(Debug, Default)]
pub(crate) struct ImageCache {
    images: HashMap<ImageKey, Option<Image>>,
}

impl ImageCache {
    /// Get (or convert) the krilla image for the given image data.
    pub(crate) fn get(&mut self, image: &ImageData, quality: ImageQuality) -> Option<Image> {
        let key = ImageKey {
            blob: image.data.id(),
            width: image.width,
            height: image.height,
            bgra: image.format == ImageFormat::Bgra8,
            premultiplied: image.alpha_type == ImageAlphaType::AlphaPremultiplied,
            interpolate: quality != ImageQuality::Low,
        };
        self.images
            .entry(key)
            .or_insert_with(|| convert(image))
            .clone()
    }
}

/// Convert image data to straight-alpha RGBA8 and wrap it as a krilla image.
fn convert(image: &ImageData) -> Option<Image> {
    let expected = image.format.size_in_bytes(image.width, image.height)?;
    let data = image.data.data();
    if data.len() < expected || image.width == 0 || image.height == 0 {
        return None;
    }
    let mut rgba = data[..expected].to_vec();
    to_straight_rgba(
        &mut rgba,
        image.format == ImageFormat::Bgra8,
        image.alpha_type == ImageAlphaType::AlphaPremultiplied,
    );
    Some(Image::from_rgba8(rgba, image.width, image.height))
}

/// Swizzle BGRA to RGBA and un-premultiply in place.
pub(crate) fn to_straight_rgba(pixels: &mut [u8], bgra: bool, premultiplied: bool) {
    for px in pixels.as_chunks_mut::<4>().0 {
        if bgra {
            px.swap(0, 2);
        }
        if premultiplied {
            let a = u16::from(px[3]);
            if a == 0 {
                px[0] = 0;
                px[1] = 0;
                px[2] = 0;
            } else if a < 255 {
                for c in &mut px[..3] {
                    let v = (u16::from(*c) * 255 + a / 2) / a;
                    *c = u8::try_from(v.min(255)).unwrap_or(u8::MAX);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::to_straight_rgba;

    #[test]
    fn bgra_premultiplied_to_rgba() {
        // Half-transparent pure red, premultiplied, in BGRA order.
        let mut px = [0, 0, 128, 128];
        to_straight_rgba(&mut px, true, true);
        assert_eq!(px, [255, 0, 0, 128], "should swizzle and unpremultiply");
    }

    #[test]
    fn transparent_clears_color() {
        let mut px = [10, 20, 30, 0];
        to_straight_rgba(&mut px, false, true);
        assert_eq!(px, [0, 0, 0, 0], "fully transparent pixels have no color");
    }
}
