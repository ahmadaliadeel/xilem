// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Conversion and caching of fonts from `peniko::FontData` to `krilla::text::Font`.

use std::collections::HashMap;
use std::sync::Arc;

use krilla::Data;
use krilla::text::{Font, Tag};
use peniko::FontData;
use skrifa::raw::TableProvider as _;
use skrifa::raw::types::F2Dot14;
use skrifa::{FontRef, MetadataProvider as _};

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct FontKey {
    blob: u64,
    index: u32,
    coords: Vec<i16>,
}

/// A document-lifetime cache of krilla fonts.
#[derive(Default)]
pub(crate) struct FontCache {
    fonts: HashMap<FontKey, Option<Font>>,
    reverse_cmaps: HashMap<(u64, u32), Arc<HashMap<u32, char>>>,
}

/// Wrapper so that a peniko blob can be handed to krilla without copying the bytes.
struct BlobData(peniko::Blob<u8>);

impl AsRef<[u8]> for BlobData {
    fn as_ref(&self) -> &[u8] {
        self.0.data()
    }
}

impl std::fmt::Debug for FontCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontCache")
            .field("fonts", &self.fonts.len())
            .finish_non_exhaustive()
    }
}

impl FontCache {
    /// Get (or create) the krilla font for the given font data and variation coordinates.
    pub(crate) fn get(&mut self, font: &FontData, coords: &[i16]) -> Option<Font> {
        let key = FontKey {
            blob: font.data.id(),
            index: font.index,
            coords: coords.to_vec(),
        };
        self.fonts
            .entry(key)
            .or_insert_with(|| {
                let data: Arc<dyn AsRef<[u8]> + Send + Sync> =
                    Arc::new(BlobData(font.data.clone()));
                let data = Data::from(data);
                if coords.iter().all(|c| *c == 0) {
                    Font::new(data, font.index)
                } else {
                    let design = denormalize_coords(font, coords);
                    Font::new_variable(data, font.index, &design)
                }
            })
            .clone()
    }

    /// Get the reverse character map for a font.
    pub(crate) fn reverse_cmap(&mut self, font: &FontData) -> Arc<HashMap<u32, char>> {
        self.reverse_cmaps
            .entry((font.data.id(), font.index))
            .or_insert_with(|| Arc::new(build_reverse_cmap(font)))
            .clone()
    }
}

fn build_reverse_cmap(font: &FontData) -> HashMap<u32, char> {
    let mut map = HashMap::new();
    let Ok(font_ref) = FontRef::from_index(font.data.data(), font.index) else {
        return map;
    };
    for (codepoint, glyph) in font_ref.charmap().mappings() {
        let Some(ch) = char::from_u32(codepoint) else {
            continue;
        };
        // Prefer the lowest codepoint for glyphs mapped from several characters,
        // which usually picks the "canonical" (non-compatibility) character.
        map.entry(glyph.to_u32())
            .and_modify(|existing: &mut char| {
                if ch < *existing {
                    *existing = ch;
                }
            })
            .or_insert(ch);
    }
    map
}

/// Horizontal advance of a glyph in font units per em (i.e. divided by units per em).
pub(crate) fn advance(font: &FontData, coords: &[i16], glyph_id: u32) -> Option<f32> {
    let font_ref = FontRef::from_index(font.data.data(), font.index).ok()?;
    let coords: Vec<F2Dot14> = coords.iter().map(|c| F2Dot14::from_bits(*c)).collect();
    let metrics = font_ref.glyph_metrics(
        skrifa::instance::Size::unscaled(),
        skrifa::instance::LocationRef::new(&coords),
    );
    let upem = f32::from(font_ref.head().ok()?.units_per_em());
    metrics
        .advance_width(skrifa::GlyphId::new(glyph_id))
        .map(|a| a / upem)
}

/// Convert normalized (avar-mapped, `F2Dot14`) coordinates to design-space values per axis.
///
/// This inverts the `avar` segment maps and then the default normalization.
pub(crate) fn denormalize_coords(font: &FontData, coords: &[i16]) -> Vec<(Tag, f32)> {
    let Ok(font_ref) = FontRef::from_index(font.data.data(), font.index) else {
        return Vec::new();
    };
    let avar = font_ref.avar().ok();
    let segment_maps: Vec<Vec<(f32, f32)>> = avar
        .map(|avar| {
            avar.axis_segment_maps()
                .iter()
                .map(|maps| {
                    maps.map(|maps| {
                        maps.axis_value_maps()
                            .iter()
                            .map(|m| (m.from_coordinate().to_f32(), m.to_coordinate().to_f32()))
                            .collect()
                    })
                    .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default();

    font_ref
        .axes()
        .iter()
        .enumerate()
        .map(|(i, axis)| {
            let normalized = coords
                .get(i)
                .map(|c| F2Dot14::from_bits(*c).to_f32())
                .unwrap_or(0.0);
            let default_normalized = match segment_maps.get(i) {
                Some(map) if map.len() >= 2 => invert_segment_map(map, normalized),
                _ => normalized,
            };
            let (min, def, max) = (axis.min_value(), axis.default_value(), axis.max_value());
            let design = if default_normalized < 0.0 {
                def + default_normalized * (def - min)
            } else {
                def + default_normalized * (max - def)
            };
            let tag = axis.tag().to_be_bytes();
            (Tag::new(&tag), design)
        })
        .collect()
}

/// Invert a piecewise linear avar segment map (from -> to) at `value` (a "to" coordinate).
fn invert_segment_map(map: &[(f32, f32)], value: f32) -> f32 {
    for pair in map.windows(2) {
        let (from0, to0) = pair[0];
        let (from1, to1) = pair[1];
        if value >= to0 && value <= to1 {
            if (to1 - to0).abs() < f32::EPSILON {
                return from0;
            }
            let t = (value - to0) / (to1 - to0);
            return from0 + t * (from1 - from0);
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::invert_segment_map;

    #[test]
    fn segment_map_inversion() {
        // A map that compresses the upper half: from 0.5 -> to 0.8.
        let map = [(-1.0, -1.0), (0.0, 0.0), (0.5, 0.8), (1.0, 1.0)];
        assert!(
            (invert_segment_map(&map, 0.8) - 0.5).abs() < 1e-6,
            "0.8 should map back to 0.5"
        );
        assert!(
            (invert_segment_map(&map, 0.9) - 0.75).abs() < 1e-6,
            "0.9 should map back to 0.75"
        );
        assert!(
            (invert_segment_map(&map, -0.5) + 0.5).abs() < 1e-6,
            "identity segment"
        );
    }
}
