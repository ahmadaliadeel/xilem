# Imaging Krilla

A [krilla](https://github.com/LaurenzV/krilla) (PDF) backend for the `imaging` crate.

`KrillaSink` implements `imaging::PaintSink` on top of a krilla `Surface`, so any retained
`imaging::record::Scene` (for example Masonry's paint output) can be written to a PDF page as
vector graphics:

- fills and strokes become PDF paths (solid colors, linear/radial/sweep gradients),
- clips and isolated groups (opacity, blend modes, masks) map to their PDF equivalents,
- images are embedded losslessly,
- glyph runs become real PDF text with embedded, subsetted fonts. With a `TextSource`, the text
  is selectable and searchable, including right-to-left and complex scripts (Arabic, Urdu).
- content without a PDF equivalent (blurred rounded rectangles, used for box shadows) is
  rasterized with `vello_cpu` when the `raster-fallback` feature is enabled (default).

The `tests/parity.rs` suite renders every scene with `vello_cpu` and, separately, through
krilla and the `hayro` PDF rasterizer, and asserts that the images match.
