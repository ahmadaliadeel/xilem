# Fork notes: printing and PDF export

This fork of [linebender/xilem](https://github.com/linebender/xilem) adds vector PDF export and
printing. Most of the work lives in new crates, so syncing with upstream stays cheap.

## New crates

- `imaging_krilla`: a [krilla](https://github.com/LaurenzV/krilla) backend for `imaging`
  (`KrillaSink`, an `imaging::PaintSink` writing PDF). It doesn't depend on Masonry and could be
  upstreamed on its own.
- `masonry_print`: snapshots, pagination, tagged PDF, PDF/A, bundled Arabic/Urdu fonts, OS printing.
- `xilem_print`: Xilem views and APIs for documents and printing running apps.

## Changes to upstream crates

They are small, opt-in, and don't change on-screen behavior:

- `masonry_core`
  - `RenderRoot::set_paint_annotations`, plus the widget contexts in `passes/paint.rs`.
  - `render_text_with_source` and `GlyphRunSource` in `core/text.rs`.
  - `RenderRoot::full_access_tree`, `edit_font_collection` and `scale_factor`.
  - `PaintCtx::paint_annotations_enabled`.
- `masonry`
  - `Label` and `TextArea` pass their text to `render_text_with_source`.
  - New `LayoutDirection` and `BaseDirection` properties (`properties/direction.rs`). `Flex` and `Grid` mirror their layout for `LayoutDirection::Rtl`, and `Label` supports `BaseDirection`.
- `xilem`
  - `Xilem::run_in_with` (wraps the app driver).
  - `MasonryDriver::update_state`.

## Syncing with upstream

```sh
git remote add upstream https://github.com/linebender/xilem.git   # once
git config rerere.enabled true                                     # remember conflict resolutions
git fetch upstream
git merge upstream/main          # merge, don't rebase, if others use this fork
cargo test --workspace           # screenshot tests must pass unchanged
git tag pdf-sync-$(date +%Y-%m-%d)
```

Conflicts, if any, are expected in the files listed above. Check `cargo tree -d` after syncing:
krilla and parley may use different versions of `skrifa`/`read-fonts`. Fonts cross between them
as bytes, so this works, but it duplicates crates.

To use the fork from another workspace, pin a tag:

```toml
[patch.crates-io]
xilem = { git = "https://github.com/ahmadaliadeel/xilem", tag = "pdf-sync-YYYY-MM-DD" }
masonry = { git = "https://github.com/ahmadaliadeel/xilem", tag = "pdf-sync-YYYY-MM-DD" }
```
