// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Writing captures to PDF pages.

use std::fmt;

use imaging_krilla::krilla::action::{Action, LinkAction};
use imaging_krilla::krilla::annotation::{Annotation, LinkAnnotation, Target};
use imaging_krilla::krilla::configure::{Accessibility, Archival, ConfigurationBuilder};
use imaging_krilla::krilla::destination::XyzDestination;
use imaging_krilla::krilla::error::KrillaError;
use imaging_krilla::krilla::geom::Point;
use imaging_krilla::krilla::metadata::{DateTime, Metadata};
use imaging_krilla::krilla::outline::{Outline, OutlineNode};
use imaging_krilla::krilla::page::PageSettings;
use imaging_krilla::krilla::tagging::{ArtifactType, TagTree};
use imaging_krilla::krilla::{Document, SerializeSettings};
use imaging_krilla::{KrillaResources, KrillaSink, KrillaSinkOptions, KrillaSinkStats};
use masonry::accesskit::Role;
use masonry::imaging::Painter;
use masonry::kurbo::{Affine, Rect, Size};

use crate::access::AccessIndex;
use crate::capture::Capture;
use crate::sink::{CaptureTags, ContentFilter, SemanticSink, Tagging};
use crate::tagging::capture_structure;

/// A PDF standard to conform to.
///
/// Conformance is validated by krilla when the document is finished; violations are
/// reported as [`PdfError::Validation`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PdfStandard {
    /// A regular PDF (1.7).
    #[default]
    Plain,
    /// PDF/A-2b (archival, visual appearance preserved).
    A2b,
    /// PDF/A-2u (archival, with Unicode text).
    A2u,
    /// PDF/A-2a (archival and accessible: tagged, with Unicode text).
    A2a,
    /// PDF/A-3b.
    A3b,
    /// PDF/A-3u.
    A3u,
    /// PDF/A-3a.
    A3a,
    /// PDF/UA-1 (universal accessibility).
    Ua1,
    /// PDF/A-2a and PDF/UA-1.
    A2aUa1,
    /// PDF/A-3a and PDF/UA-1.
    A3aUa1,
}

impl PdfStandard {
    fn archival(self) -> Option<Archival> {
        Some(match self {
            Self::A2b => Archival::A2_B,
            Self::A2u => Archival::A2_U,
            Self::A2a | Self::A2aUa1 => Archival::A2_A,
            Self::A3b => Archival::A3_B,
            Self::A3u => Archival::A3_U,
            Self::A3a | Self::A3aUa1 => Archival::A3_A,
            Self::Plain | Self::Ua1 => return None,
        })
    }

    fn accessibility(self) -> Option<Accessibility> {
        matches!(self, Self::Ua1 | Self::A2aUa1 | Self::A3aUa1).then_some(Accessibility::UA1)
    }

    /// Whether the standard requires a tagged (structured) PDF.
    pub fn requires_tagging(self) -> bool {
        matches!(
            self,
            Self::A2a | Self::A3a | Self::Ua1 | Self::A2aUa1 | Self::A3aUa1
        )
    }
}

/// A creation date for document metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PdfDate {
    /// Year.
    pub year: u16,
    /// Month (1-12).
    pub month: u8,
    /// Day (1-31).
    pub day: u8,
    /// Hour (0-23), UTC.
    pub hour: u8,
    /// Minute (0-59).
    pub minute: u8,
    /// Second (0-59).
    pub second: u8,
}

/// Options for writing PDF documents.
#[derive(Clone, Debug)]
pub struct PdfOptions {
    /// Document title. Required for PDF/UA.
    pub title: Option<String>,
    /// Default language of the document (BCP 47, e.g. `"en"`, `"ar"`, `"ur"`).
    pub lang: Option<String>,
    /// Authors.
    pub authors: Vec<String>,
    /// Subject/description.
    pub subject: Option<String>,
    /// Keywords.
    pub keywords: Vec<String>,
    /// Creation date.
    pub creation_date: Option<PdfDate>,
    /// The standard to conform to.
    pub standard: PdfStandard,
    /// Whether to write a tagged (structured, accessible) PDF.
    ///
    /// Always enabled for standards that require it.
    pub tagged: bool,
    /// Whether to add a document outline (bookmarks) from headings.
    pub outline: bool,
    /// Whether to add link annotations for link widgets.
    pub links: bool,
    /// Whether to compress content streams.
    pub compress: bool,
    /// Options for the vector backend.
    pub sink: KrillaSinkOptions,
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self {
            title: None,
            lang: None,
            authors: Vec::new(),
            subject: None,
            keywords: Vec::new(),
            creation_date: None,
            standard: PdfStandard::Plain,
            tagged: true,
            outline: true,
            links: true,
            compress: true,
            sink: KrillaSinkOptions::default(),
        }
    }
}

impl PdfOptions {
    /// Sets the title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets the language.
    pub fn with_lang(mut self, lang: impl Into<String>) -> Self {
        self.lang = Some(lang.into());
        self
    }

    /// Sets the standard.
    pub fn with_standard(mut self, standard: PdfStandard) -> Self {
        self.standard = standard;
        self
    }

    fn is_tagged(&self) -> bool {
        self.tagged || self.standard.requires_tagging()
    }
}

/// Errors when writing PDF documents.
#[derive(Debug)]
pub enum PdfError {
    /// The options are invalid (e.g. incompatible standards).
    Config(String),
    /// The document doesn't conform to the requested standard.
    Validation(Vec<String>),
    /// Another error while writing the PDF.
    Krilla(String),
    /// An I/O error.
    Io(std::io::Error),
}

impl fmt::Display for PdfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(e) => write!(f, "invalid PDF configuration: {e}"),
            Self::Validation(errors) => {
                write!(f, "PDF validation failed: {}", errors.join("; "))
            }
            Self::Krilla(e) => write!(f, "PDF export failed: {e}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for PdfError {}

impl From<std::io::Error> for PdfError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<KrillaError> for PdfError {
    fn from(e: KrillaError) -> Self {
        match e {
            KrillaError::Validation(errors) => Self::Validation(
                errors
                    .into_iter()
                    .map(|(error, _)| format!("{error:?}"))
                    .collect(),
            ),
            other => Self::Krilla(format!("{other:?}")),
        }
    }
}

/// A finished PDF document.
#[derive(Clone, Debug)]
pub struct PdfOutput {
    /// The PDF file contents.
    pub bytes: Vec<u8>,
    /// Number of pages.
    pub pages: usize,
    /// Statistics about the emitted content.
    pub stats: KrillaSinkStats,
}

impl PdfOutput {
    /// Writes the PDF to a file.
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> Result<(), PdfError> {
        std::fs::write(path, &self.bytes)?;
        Ok(())
    }
}

/// Identifies a capture registered with [`PdfWriter::add_capture`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureId(usize);

/// How the content of a [`PageItem`] is tagged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemRole {
    /// Real content of the document, tagged according to its accessibility tree.
    Content(CaptureId),
    /// Decoration such as page headers, footers or page numbers.
    Artifact,
}

/// Content placed on a page.
#[derive(Debug)]
pub struct PageItem<'a> {
    /// The capture to draw.
    pub capture: &'a Capture,
    /// How the content is tagged.
    pub role: ItemRole,
    /// Which content of the capture to draw.
    pub filter: ContentFilter,
    /// Transform from capture coordinates (logical pixels) to page coordinates (points).
    pub transform: Affine,
    /// Clip rectangle in capture coordinates.
    pub clip: Option<Rect>,
}

#[derive(Debug)]
struct OutlineEntry {
    level: u32,
    title: String,
    page: usize,
    point: Point,
}

struct CaptureEntry {
    access: AccessIndex,
    tags: CaptureTags,
}

/// Writes captures to a PDF document, page by page.
pub struct PdfWriter {
    document: Document,
    resources: KrillaResources,
    options: PdfOptions,
    tagged: bool,
    captures: Vec<CaptureEntry>,
    outline: Vec<OutlineEntry>,
    pages: usize,
    stats: KrillaSinkStats,
}

impl fmt::Debug for PdfWriter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PdfWriter")
            .field("options", &self.options)
            .field("pages", &self.pages)
            .finish_non_exhaustive()
    }
}

impl PdfWriter {
    /// Creates a writer.
    pub fn new(options: &PdfOptions) -> Result<Self, PdfError> {
        let mut builder = ConfigurationBuilder::new();
        if let Some(archival) = options.standard.archival() {
            builder = builder.with_archival_validator(archival);
        }
        if let Some(accessibility) = options.standard.accessibility() {
            builder = builder.with_accessibility_validator(accessibility);
        }
        let configuration = builder
            .finish()
            .map_err(|e| PdfError::Config(format!("{e:?}")))?;
        let tagged = options.is_tagged();
        let settings = SerializeSettings {
            compress_content_streams: options.compress,
            configuration,
            enable_tagging: tagged,
            ..SerializeSettings::default()
        };
        Ok(Self {
            document: Document::new_with(settings),
            resources: KrillaResources::default(),
            options: options.clone(),
            tagged,
            captures: Vec::new(),
            outline: Vec::new(),
            pages: 0,
            stats: KrillaSinkStats::default(),
        })
    }

    /// The number of pages written so far.
    pub fn page_count(&self) -> usize {
        self.pages
    }

    /// Registers a capture as document content, so it is included in the structure tree.
    ///
    /// Captures are included in the structure tree in the order they are added.
    pub fn add_capture(&mut self, capture: &Capture) -> CaptureId {
        self.captures.push(CaptureEntry {
            access: capture.access.clone(),
            tags: CaptureTags::default(),
        });
        CaptureId(self.captures.len() - 1)
    }

    /// Adds a page of the given size (in points) with the given content.
    pub fn add_page(&mut self, size_pt: Size, items: &[PageItem<'_>]) {
        let page_index = self.pages;
        self.pages += 1;
        #[expect(clippy::cast_possible_truncation, reason = "page sizes fit in f32")]
        let settings = PageSettings::from_wh(size_pt.width as f32, size_pt.height as f32)
            .unwrap_or_else(|| PageSettings::from_wh(595.0, 842.0).unwrap());
        let mut page = self.document.start_page_with(settings);
        let mut links = Vec::new();
        {
            let mut surface = page.surface();
            for item in items {
                let capture = item.capture;
                let tagging = match item.role {
                    _ if !self.tagged => Tagging::None,
                    ItemRole::Content(CaptureId(index)) => {
                        Tagging::Content(&mut self.captures[index].tags)
                    }
                    ItemRole::Artifact => Tagging::Artifact(ArtifactType::PaginationOther),
                };
                let mut sink = KrillaSink::new(
                    &mut surface,
                    &mut self.resources,
                    item.transform,
                    self.options.sink.clone(),
                );
                sink.set_viewport_clip(item.clip);
                let mut semantic = SemanticSink::new(sink, &capture.access, item.filter, tagging);
                if let Some(background) = capture.background {
                    let area = item.clip.unwrap_or(capture.content_bounds);
                    Painter::new(&mut semantic).fill_rect(area, background);
                }
                capture.plan.replay_into(&mut semantic);
                let stats = semantic.finish();
                self.stats.merge(&stats);

                if let ItemRole::Content(CaptureId(index)) = item.role {
                    let visible = |bounds: Rect| match item.filter {
                        ContentFilter::Slice { y0, y1 } => bounds.y0 < y1 && bounds.y1 > y0,
                        _ => item.clip.is_none_or(|clip| clip.overlaps(bounds)),
                    };
                    collect_links_and_headings(
                        &capture.access,
                        item,
                        &visible,
                        index,
                        page_index,
                        &mut links,
                        &mut self.outline,
                    );
                }
            }
        }

        if self.options.links {
            for link in links {
                let Some(rect) = to_krilla_rect(link.rect) else {
                    continue;
                };
                let annotation = Annotation::new_link(
                    LinkAnnotation::new(
                        rect,
                        Target::Action(Action::Link(LinkAction::new(link.url))),
                    ),
                    Some(link.alt),
                );
                if self.tagged {
                    let id = page.add_tagged_annotation(annotation);
                    self.captures[link.capture]
                        .tags
                        .leaves
                        .entry(link.node)
                        .or_default()
                        .push(id);
                } else {
                    page.add_annotation(annotation);
                }
            }
        }
        page.finish();
    }

    /// Finishes the document.
    pub fn finish(mut self) -> Result<PdfOutput, PdfError> {
        let mut metadata = Metadata::new();
        if let Some(title) = &self.options.title {
            metadata = metadata.title(title.clone());
        }
        if let Some(lang) = &self.options.lang {
            metadata = metadata.language(lang.clone());
        }
        if !self.options.authors.is_empty() {
            metadata = metadata.authors(self.options.authors.clone());
        }
        if let Some(subject) = &self.options.subject {
            metadata = metadata.description(subject.clone());
        }
        if !self.options.keywords.is_empty() {
            metadata = metadata.keywords(self.options.keywords.clone());
        }
        if let Some(date) = self.options.creation_date {
            metadata = metadata.creation_date(
                DateTime::new(date.year)
                    .month(date.month)
                    .day(date.day)
                    .hour(date.hour)
                    .minute(date.minute)
                    .second(date.second)
                    .utc_offset_hour(0)
                    .utc_offset_minute(0),
            );
        }
        metadata = metadata.creator("Xilem (masonry_print)".to_string());
        self.document.set_metadata(metadata);

        if self.tagged {
            let mut tree = TagTree::new().with_lang(self.options.lang.clone());
            for entry in std::mem::take(&mut self.captures) {
                for node in capture_structure(&entry.access, entry.tags) {
                    tree.push(node);
                }
            }
            self.document.set_tag_tree(tree);
        }

        if self.options.outline && !self.outline.is_empty() {
            self.document.set_outline(build_outline(&self.outline));
        }

        let bytes = self.document.finish()?;
        Ok(PdfOutput {
            bytes,
            pages: self.pages,
            stats: self.stats,
        })
    }
}

struct PendingLink {
    capture: usize,
    node: u64,
    rect: Rect,
    url: String,
    alt: String,
}

fn to_krilla_rect(rect: Rect) -> Option<imaging_krilla::krilla::geom::Rect> {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "page coordinates fit in f32"
    )]
    imaging_krilla::krilla::geom::Rect::from_ltrb(
        rect.x0 as f32,
        rect.y0 as f32,
        rect.x1 as f32,
        rect.y1 as f32,
    )
}

fn collect_links_and_headings(
    access: &AccessIndex,
    item: &PageItem<'_>,
    visible: &dyn Fn(Rect) -> bool,
    capture: usize,
    page: usize,
    links: &mut Vec<PendingLink>,
    outline: &mut Vec<OutlineEntry>,
) {
    let in_subtree = |mut id: u64| match item.filter {
        ContentFilter::Subtree(target) => loop {
            if id == target {
                break true;
            }
            match access.parent(id) {
                Some(parent) => id = parent,
                None => break false,
            }
        },
        _ => true,
    };
    for id in access.depth_first() {
        let Some(node) = access.node(id) else {
            continue;
        };
        let role = node.role();
        if !matches!(role, Role::Link | Role::Heading) || access.is_hidden(id) || !in_subtree(id) {
            continue;
        }
        let Some(bounds) = access.bounds(id) else {
            continue;
        };
        match role {
            Role::Link => {
                let Some(url) = node.url() else {
                    continue;
                };
                if !visible(bounds) {
                    continue;
                }
                let clipped = match (item.filter, item.clip) {
                    (ContentFilter::Slice { y0, y1 }, _) => {
                        Rect::new(bounds.x0, bounds.y0.max(y0), bounds.x1, bounds.y1.min(y1))
                    }
                    (_, Some(clip)) => bounds.intersect(clip),
                    _ => bounds,
                };
                let text = access.text_content(id);
                links.push(PendingLink {
                    capture,
                    node: id,
                    rect: item.transform.transform_rect_bbox(clipped),
                    url: url.to_string(),
                    alt: node.label().map(str::to_string).unwrap_or(text),
                });
            }
            Role::Heading => {
                // A heading belongs to the page where its top is.
                let owns = match item.filter {
                    ContentFilter::Slice { y0, y1 } => bounds.y0 >= y0 && bounds.y0 < y1,
                    _ => visible(bounds),
                };
                if !owns {
                    continue;
                }
                let title = node
                    .label()
                    .map(str::to_string)
                    .unwrap_or_else(|| access.text_content(id));
                let top_left = item.transform * bounds.origin();
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "page coordinates fit in f32"
                )]
                outline.push(OutlineEntry {
                    level: u32::try_from(node.level().unwrap_or(1)).unwrap_or(1).max(1),
                    title: title.trim().to_string(),
                    page,
                    point: Point::from_xy(top_left.x as f32, top_left.y as f32),
                });
            }
            _ => {}
        }
    }
}

fn build_outline(entries: &[OutlineEntry]) -> Outline {
    fn build_children(entries: &[OutlineEntry], index: &mut usize, level: u32) -> Vec<OutlineNode> {
        let mut nodes = Vec::new();
        while *index < entries.len() && entries[*index].level >= level {
            let entry = &entries[*index];
            *index += 1;
            let mut node = OutlineNode::new(
                entry.title.clone(),
                XyzDestination::new(entry.page, entry.point),
            );
            for child in build_children(entries, index, entry.level + 1) {
                node.push_child(child);
            }
            nodes.push(node);
        }
        nodes
    }
    let mut outline = Outline::new();
    let mut index = 0;
    while index < entries.len() {
        let level = entries[index].level;
        for node in build_children(entries, &mut index, level) {
            outline.push_child(node);
        }
    }
    outline
}
