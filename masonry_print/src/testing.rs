// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Helpers for testing PDF output: text extraction and rasterization.
//!
//! The text extractor understands what a PDF viewer needs for copy and paste:
//! `ToUnicode` maps, `/ActualText` spans and `/ReversedChars` sections. It is intentionally
//! small and only supports the constructs written by krilla.

use std::collections::HashMap;
use std::sync::Arc;

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::{RenderCache, RenderSettings};
use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, ObjectId};
use masonry::kurbo::Affine;

/// An RGBA8 image (not premultiplied).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Raster {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pixels, row by row.
    pub data: Vec<u8>,
}

/// Rasterizes a page of a PDF with `hayro`, over a white background.
///
/// `scale` is pixels per PDF point.
pub fn rasterize(pdf: &[u8], page: usize, scale: f32) -> Raster {
    let pdf = Pdf::new(Arc::new(pdf.to_vec())).expect("valid PDF");
    let page = &pdf.pages()[page];
    let pixmap = hayro::render(
        page,
        &RenderCache::new(),
        &InterpreterSettings::default(),
        &RenderSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: masonry::peniko::Color::WHITE,
            ..RenderSettings::default()
        },
    );
    let (width, height) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
    let data = pixmap
        .take_unpremultiplied()
        .into_iter()
        .flat_map(|p| [p.r, p.g, p.b, p.a])
        .collect();
    Raster {
        width,
        height,
        data,
    }
}

/// Returns the number of pages of a PDF.
pub fn page_count(pdf: &[u8]) -> usize {
    Document::load_mem(pdf)
        .expect("valid PDF")
        .get_pages()
        .len()
}

/// Extracts the text of each page, one line per text line.
pub fn extract_text(pdf: &[u8]) -> Vec<String> {
    let doc = Document::load_mem(pdf).expect("valid PDF");
    doc.get_pages()
        .values()
        .map(|&page_id| {
            let mut extractor = Extractor::default();
            let resources = page_resources(&doc, page_id);
            let content = doc.get_page_content(page_id).unwrap_or_default();
            extractor.run(&doc, &content, &resources, Affine::IDENTITY);
            extractor.finish()
        })
        .collect()
}

fn resolve<'a>(doc: &'a Document, object: &'a Object) -> &'a Object {
    match object {
        Object::Reference(id) => doc.get_object(*id).unwrap_or(object),
        _ => object,
    }
}

fn page_resources(doc: &Document, page_id: ObjectId) -> Dictionary {
    let page = doc.get_dictionary(page_id).expect("page dictionary");
    page.get(b"Resources")
        .ok()
        .and_then(|r| resolve(doc, r).as_dict().ok())
        .cloned()
        .unwrap_or_default()
}

#[derive(Clone, Debug, Default)]
struct FontInfo {
    two_byte: bool,
    to_unicode: HashMap<u32, String>,
}

fn utf16_hex(hex: &[u8]) -> String {
    let units: Vec<u16> = hex
        .chunks(2)
        .map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)]))
        .collect();
    String::from_utf16_lossy(&units)
}

fn parse_hex_tokens(line: &str) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find('<') {
        let Some(end) = rest[start..].find('>') else {
            break;
        };
        let hex = &rest[start + 1..start + end];
        let bytes = (0..hex.len() / 2)
            .filter_map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok())
            .collect();
        out.push(bytes);
        rest = &rest[start + end + 1..];
    }
    out
}

fn code(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0, |acc, b| (acc << 8) | u32::from(*b))
}

fn parse_cmap(data: &[u8]) -> HashMap<u32, String> {
    let text = String::from_utf8_lossy(data);
    let mut map = HashMap::new();
    let mut mode = "";
    for line in text.lines() {
        let line = line.trim();
        if line.ends_with("beginbfchar") {
            mode = "char";
            continue;
        }
        if line.ends_with("beginbfrange") {
            mode = "range";
            continue;
        }
        if line.starts_with("endbf") {
            mode = "";
            continue;
        }
        let tokens = parse_hex_tokens(line);
        match (mode, tokens.as_slice()) {
            ("char", [src, dst]) => {
                map.insert(code(src), utf16_hex(dst));
            }
            ("range", [lo, hi, dst]) if !line.contains('[') => {
                let (lo, hi) = (code(lo), code(hi));
                let base = utf16_hex(dst);
                let mut chars: Vec<char> = base.chars().collect();
                for c in lo..=hi {
                    map.insert(c, chars.iter().collect());
                    if let Some(last) = chars.last_mut() {
                        *last = char::from_u32(*last as u32 + 1).unwrap_or(*last);
                    }
                }
            }
            ("range", [lo, _hi, dsts @ ..]) => {
                for (i, dst) in dsts.iter().enumerate() {
                    #[expect(clippy::cast_possible_truncation, reason = "small ranges")]
                    map.insert(code(lo) + i as u32, utf16_hex(dst));
                }
            }
            _ => {}
        }
    }
    map
}

fn font_info(doc: &Document, font: &Dictionary) -> FontInfo {
    let two_byte = font
        .get(b"Subtype")
        .ok()
        .and_then(|s| s.as_name().ok())
        .is_some_and(|s| s == b"Type0");
    let to_unicode = font
        .get(b"ToUnicode")
        .ok()
        .and_then(|r| resolve(doc, r).as_stream().ok())
        .and_then(|s| {
            s.decompressed_content()
                .ok()
                .or_else(|| Some(s.content.clone()))
        })
        .map(|data| parse_cmap(&data))
        .unwrap_or_default();
    FontInfo {
        two_byte,
        to_unicode,
    }
}

fn number(object: &Object) -> f64 {
    match object {
        Object::Integer(i) => *i as f64,
        Object::Real(r) => f64::from(*r),
        _ => 0.0,
    }
}

fn matrix(operands: &[Object]) -> Affine {
    let v: Vec<f64> = operands.iter().map(number).collect();
    if v.len() == 6 {
        Affine::new([v[0], v[1], v[2], v[3], v[4], v[5]])
    } else {
        Affine::IDENTITY
    }
}

#[derive(Debug)]
enum Marked {
    ActualText(String),
    Reversed,
    Other,
}

#[derive(Default)]
struct Extractor {
    lines: Vec<(f64, String)>,
    marked: Vec<Marked>,
    suppress: usize,
    fonts: HashMap<Vec<u8>, FontInfo>,
    font: Option<Vec<u8>>,
    last_y: Option<f64>,
    /// Baseline of the current text object (glyphs of one run may have different y offsets).
    block_y: Option<f64>,
    /// Text collected inside `/ReversedChars` sections.
    reversed: Vec<String>,
}

impl Extractor {
    fn emit(&mut self, text: &str, y: f64) {
        if text.is_empty() {
            return;
        }
        if let Some(buffer) = self.reversed.last_mut() {
            buffer.push_str(text);
            return;
        }
        match self.last_y {
            Some(last) if (last - y).abs() < 2.0 => {
                self.lines.last_mut().unwrap().1.push_str(text);
            }
            _ => self.lines.push((y, text.to_string())),
        }
        self.last_y = Some(y);
    }

    fn show(&mut self, bytes: &[u8], y: f64) {
        let y = *self.block_y.get_or_insert(y);
        if self.suppress > 0 {
            return;
        }
        let Some(font) = self.font.as_ref().and_then(|f| self.fonts.get(f)) else {
            return;
        };
        let step = if font.two_byte { 2 } else { 1 };
        let text: String = bytes
            .chunks(step)
            .filter_map(|c| font.to_unicode.get(&code(c)).cloned())
            .collect();
        self.emit(&text, y);
    }

    fn run(&mut self, doc: &Document, content: &[u8], resources: &Dictionary, base: Affine) {
        let Ok(content) = Content::decode(content) else {
            return;
        };
        let fonts = resources
            .get(b"Font")
            .ok()
            .and_then(|f| resolve(doc, f).as_dict().ok())
            .cloned()
            .unwrap_or_default();
        for (name, font) in fonts.iter() {
            if let Ok(font) = resolve(doc, font).as_dict() {
                self.fonts.insert(name.clone(), font_info(doc, font));
            }
        }
        let xobjects = resources
            .get(b"XObject")
            .ok()
            .and_then(|x| resolve(doc, x).as_dict().ok())
            .cloned()
            .unwrap_or_default();

        let mut ctm = base;
        let mut stack = Vec::new();
        let mut text_matrix = Affine::IDENTITY;
        for op in &content.operations {
            let operands = &op.operands;
            match op.operator.as_str() {
                "q" => stack.push(ctm),
                "Q" => ctm = stack.pop().unwrap_or(base),
                "cm" => ctm *= matrix(operands),
                "BT" => {
                    text_matrix = Affine::IDENTITY;
                    self.block_y = None;
                }
                "Tm" => text_matrix = matrix(operands),
                "Td" | "TD" => {
                    let (tx, ty) = (number(&operands[0]), number(&operands[1]));
                    text_matrix *= Affine::translate((tx, ty));
                }
                "Tf" => {
                    self.font = operands
                        .first()
                        .and_then(|n| n.as_name().ok())
                        .map(<[u8]>::to_vec);
                }
                "Tj" | "'" | "\"" => {
                    let y = (ctm * text_matrix).translation().y;
                    if let Some(Object::String(bytes, _)) = operands.last() {
                        self.show(bytes, y);
                    }
                }
                "TJ" => {
                    let y = (ctm * text_matrix).translation().y;
                    if let Some(Object::Array(items)) = operands.first() {
                        for item in items {
                            if let Object::String(bytes, _) = item {
                                self.show(bytes, y);
                            }
                        }
                    }
                }
                "BDC" => {
                    let props = operands.get(1).map(|p| resolve(doc, p));
                    let actual = props
                        .and_then(|p| p.as_dict().ok())
                        .and_then(|d| d.get(b"ActualText").ok())
                        .and_then(|t| match t {
                            Object::String(bytes, _) => Some(decode_text_string(bytes)),
                            _ => None,
                        });
                    match actual {
                        Some(text) => {
                            self.suppress += 1;
                            self.marked.push(Marked::ActualText(text));
                        }
                        None => self.marked.push(Marked::Other),
                    }
                }
                "BMC" => {
                    let reversed = operands
                        .first()
                        .and_then(|n| n.as_name().ok())
                        .is_some_and(|n| n == b"ReversedChars");
                    if reversed {
                        self.reversed.push(String::new());
                        self.marked.push(Marked::Reversed);
                    } else {
                        self.marked.push(Marked::Other);
                    }
                }
                "EMC" => match self.marked.pop() {
                    Some(Marked::ActualText(text)) => {
                        self.suppress -= 1;
                        let y = self
                            .block_y
                            .unwrap_or_else(|| (ctm * text_matrix).translation().y);
                        if self.suppress == 0 {
                            self.emit(&text, y);
                        }
                    }
                    Some(Marked::Reversed) => {
                        let text: String = self
                            .reversed
                            .pop()
                            .unwrap_or_default()
                            .chars()
                            .rev()
                            .collect();
                        let y = self
                            .block_y
                            .unwrap_or_else(|| (ctm * text_matrix).translation().y);
                        self.emit(&text, y);
                    }
                    _ => {}
                },
                "Do" => {
                    let Some(name) = operands.first().and_then(|n| n.as_name().ok()) else {
                        continue;
                    };
                    let Some(stream) = xobjects
                        .get(name)
                        .ok()
                        .and_then(|x| resolve(doc, x).as_stream().ok())
                    else {
                        continue;
                    };
                    let is_form = stream
                        .dict
                        .get(b"Subtype")
                        .ok()
                        .and_then(|s| s.as_name().ok())
                        .is_some_and(|s| s == b"Form");
                    if !is_form {
                        continue;
                    }
                    let form_matrix = stream
                        .dict
                        .get(b"Matrix")
                        .ok()
                        .and_then(|m| m.as_array().ok())
                        .map(|m| matrix(m))
                        .unwrap_or_default();
                    let form_resources = stream
                        .dict
                        .get(b"Resources")
                        .ok()
                        .and_then(|r| resolve(doc, r).as_dict().ok())
                        .cloned()
                        .unwrap_or_else(|| resources.clone());
                    let data = stream
                        .decompressed_content()
                        .unwrap_or_else(|_| stream.content.clone());
                    let saved_font = self.font.clone();
                    self.run(doc, &data, &form_resources, ctm * form_matrix);
                    self.font = saved_font;
                }
                _ => {}
            }
        }
    }

    fn finish(mut self) -> String {
        // PDF user space is y-up: sort lines top to bottom.
        self.lines.sort_by(|a, b| b.0.total_cmp(&a.0));
        self.lines
            .into_iter()
            .map(|(_, line)| line)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn decode_text_string(bytes: &[u8]) -> String {
    if let Some(utf16) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        utf16_hex(utf16)
    } else {
        bytes.iter().map(|b| char::from(*b)).collect()
    }
}
