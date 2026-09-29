# Masonry Print

Printing and vector PDF export for Masonry.

Masonry paints through the `imaging` abstraction. This crate replays that paint output into
[`imaging_krilla`](../imaging_krilla), which writes PDF with [krilla](https://github.com/LaurenzV/krilla),
so printed output matches what is on screen, as resolution-independent vector graphics with real,
selectable text.

- **Snapshots**: print a window, a widget or a `PrintRegion` as it appears on screen
  (`snapshot_to_pdf`).
- **Paginated documents**: lay out content at paper size and flow it across pages without cutting
  lines of text, with headers, footers and page numbers (`paginate_to_pdf`).
- **Text**: glyphs are embedded as subsetted fonts together with their source text, so text can be
  searched and copied, including right-to-left and complex scripts (Arabic, Urdu Nastaliq).
  Right-to-left runs are written with `/ActualText` in logical order.
- **Right-to-left documents**: `PageSetup::with_direction(LayoutDirection::Rtl)` mirrors layouts
  and paragraph directions; `NumberSystem` formats Arabic-Indic and Urdu digits.
- **Tagged PDF**: the structure tree is built from Masonry's accessibility tree (paragraphs,
  headings, links, figures, tables, lists), for screen readers and PDF/UA.
- **Links and bookmarks**: link widgets become clickable, headings become the outline.
- **PDF/A** (2b/2u/2a, 3b/3u/3a) and **PDF/UA-1**, validated by krilla.
- **Printing**: `os_print` sends PDFs to the operating system's printers (CUPS `lp` on Linux and
  macOS, PowerShell on Windows).

## How it works

With `RenderRoot::set_paint_annotations(true)`, Masonry wraps each widget's paint output in a
widget context, and each glyph run in a context carrying its source text and the text range of
every glyph. Backends that don't care ignore these annotations, so on-screen rendering is
unchanged. A `Capture` combines the annotated paint output with the full accessibility tree; the
PDF writer uses the widget contexts to filter content (widget snapshots, page slices) and to tag
it, and the text contexts to write searchable text.

## Tests

The tests check that extracted text (Latin, Arabic, Urdu Nastaliq, mixed bidi) is in logical
order, that PDFs rasterized with `hayro` match the on-screen `vello_cpu` rendering, that
pagination never cuts lines, and that PDF/A and PDF/UA documents validate.

## Fonts

The `noto-fonts` feature (default) bundles Noto Naskh Arabic and Noto Nastaliq Urdu, licensed
under the SIL Open Font License 1.1 (see `resources/fonts/noto`).
