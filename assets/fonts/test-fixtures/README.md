# Test fixtures — fonts

One OFL Arabic face for the `ui-dom` text-seam proof gate (TEXT_SEAM_DESIGN.md
D7). This directory is NOT part of the runtime font catalog: the face is never
`include_bytes!`-embedded into the crate.

## The face

- **Face**: Noto Naskh Arabic, Regular (static)
- **File**: `NotoNaskhArabic-Regular.ttf`
- **Provenance**: `https://raw.githubusercontent.com/notofonts/noto-fonts/main/hinted/ttf/NotoNaskhArabic/NotoNaskhArabic-Regular.ttf`
- **Upstream repo**: https://github.com/notofonts/noto-fonts (commit pinned by the
  file content at acquisition, 2026-10-23)
- **License**: SIL Open Font License 1.1 — see `LICENSE` in this directory
  (copied verbatim from the same `notofonts/noto-fonts` repository)
- **Size**: 178,388 bytes (static TrueType outline; `00 01 00 00` magic)
- **Version/date**: acquired 2026-10-23; the face is the hinted static Regular
  build from the `notofonts/noto-fonts` repository's `hinted/ttf` tree

The family name read from the face's name table is `Noto Naskh Arabic` (weight
Regular).

## The rule

This face is loaded **from disk by the proofs** through the consumer API —
`TextFontCatalog::install_face(&bytes)` — never embedded. The proofs read it via
`env!("CARGO_MANIFEST_DIR")`-relative paths
(`crates/ui-dom/../../assets/fonts/test-fixtures/NotoNaskhArabic-Regular.ttf`).
That is the whole point of the consumer door: a face that arrives as bytes at
runtime, not as a compile-time `include_bytes!`.

Do not add a `const ... = include_bytes!(...)` for this file anywhere. If the
proofs need the bytes, they read them from disk.
