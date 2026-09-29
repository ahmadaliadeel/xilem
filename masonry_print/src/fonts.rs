// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Font configuration for headless (print) rendering.

use masonry::app::RenderRoot;
use masonry::parley::fontique::{FallbackKey, Script};
use masonry::peniko::Blob;

/// A fallback rule: text in `script` (optionally only for `locale`) uses `families`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontFallback {
    /// ISO 15924 script tag, e.g. `*b"Arab"` or `*b"Latn"`.
    pub script: [u8; 4],
    /// BCP 47 language tag, e.g. `"ur"`. `None` applies to all languages.
    pub locale: Option<String>,
    /// Font family names, in order of preference.
    pub families: Vec<String>,
}

impl FontFallback {
    /// A fallback for a script, for all languages.
    pub fn new(script: [u8; 4], families: &[&str]) -> Self {
        Self {
            script,
            locale: None,
            families: families.iter().map(|f| (*f).to_string()).collect(),
        }
    }

    /// Restrict this fallback to a language.
    pub fn for_locale(mut self, locale: &str) -> Self {
        self.locale = Some(locale.to_string());
        self
    }
}

/// Fonts available to a headless render root.
#[derive(Clone, Debug, Default)]
pub struct FontSetup {
    /// Whether system fonts are available.
    ///
    /// Disable this (and provide `fonts`) for reproducible output.
    pub system_fonts: bool,
    /// Additional font files (TrueType/OpenType data, or collections).
    pub fonts: Vec<Blob<u8>>,
    /// Script fallbacks, applied after registering `fonts`.
    pub fallbacks: Vec<FontFallback>,
}

impl FontSetup {
    /// Use system fonts, with no additional fonts.
    pub fn system() -> Self {
        Self {
            system_fonts: true,
            ..Self::default()
        }
    }

    /// Only use the given fonts.
    pub fn only(fonts: impl IntoIterator<Item = Blob<u8>>) -> Self {
        Self {
            system_fonts: false,
            fonts: fonts.into_iter().collect(),
            fallbacks: Vec::new(),
        }
    }

    /// Adds a font.
    pub fn with_font(mut self, font: Blob<u8>) -> Self {
        self.fonts.push(font);
        self
    }

    /// Adds a fallback rule.
    pub fn with_fallback(mut self, fallback: FontFallback) -> Self {
        self.fallbacks.push(fallback);
        self
    }

    /// Adds the bundled Noto Naskh Arabic and Noto Nastaliq Urdu fonts, with fallbacks so
    /// Arabic-script text uses Naskh, and Urdu (`ur`) text uses Nastaliq.
    #[cfg(feature = "noto-fonts")]
    pub fn with_noto_arabic(self) -> Self {
        self.with_font(noto::naskh_arabic())
            .with_font(noto::nastaliq_urdu())
            .with_fallback(FontFallback::new(*b"Arab", &[noto::NASKH_ARABIC_FAMILY]))
            .with_fallback(
                FontFallback::new(*b"Arab", &[noto::NASTALIQ_URDU_FAMILY]).for_locale("ur"),
            )
    }

    /// Registers the fonts and fallbacks with a render root.
    pub fn apply(&self, root: &mut RenderRoot) {
        for font in &self.fonts {
            root.register_fonts(font.clone());
        }
        if self.fallbacks.is_empty() {
            return;
        }
        root.edit_font_collection(|collection| {
            for fallback in &self.fallbacks {
                let families: Vec<_> = fallback
                    .families
                    .iter()
                    .filter_map(|name| collection.family_id(name))
                    .collect();
                if families.is_empty() {
                    tracing::warn!("no font families found for fallback {fallback:?}");
                    continue;
                }
                let script = Script::from_bytes(fallback.script);
                let key: FallbackKey = match &fallback.locale {
                    Some(locale) => (script, locale.as_str()).into(),
                    None => FallbackKey::new(script, None),
                };
                collection.append_fallbacks(key, families.into_iter());
            }
        });
    }
}

/// Bundled Noto fonts for Arabic and Urdu (SIL Open Font License 1.1).
///
/// The license texts are in `resources/fonts/noto` of this crate.
#[cfg(feature = "noto-fonts")]
pub mod noto {
    use std::sync::Arc;

    use masonry::peniko::Blob;

    /// Family name of [`naskh_arabic`].
    pub const NASKH_ARABIC_FAMILY: &str = "Noto Naskh Arabic";
    /// Family name of [`nastaliq_urdu`].
    pub const NASTALIQ_URDU_FAMILY: &str = "Noto Nastaliq Urdu";

    /// Noto Naskh Arabic (variable weight) font data.
    pub const NASKH_ARABIC: &[u8] =
        include_bytes!("../resources/fonts/noto/NotoNaskhArabic-VF.ttf");
    /// Noto Nastaliq Urdu (variable weight) font data.
    pub const NASTALIQ_URDU: &[u8] =
        include_bytes!("../resources/fonts/noto/NotoNastaliqUrdu-VF.ttf");

    /// Noto Naskh Arabic, a Naskh-style font for Arabic, Persian and Urdu.
    pub fn naskh_arabic() -> Blob<u8> {
        Blob::new(Arc::new(NASKH_ARABIC))
    }

    /// Noto Nastaliq Urdu, a Nastaliq-style font, the traditional style for Urdu.
    pub fn nastaliq_urdu() -> Blob<u8> {
        Blob::new(Arc::new(NASTALIQ_URDU))
    }
}
