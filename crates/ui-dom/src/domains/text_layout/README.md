# text_layout

Text measurement, line breaking, shaping, and caret layout for `ui-dom`, and
the single place a production `cosmic_text::Buffer` is born.

## Purpose and scope

This module owns the main-world side of the text seam: it measures text
intrinsics that feed layout, breaks and wraps lines, shapes glyph runs, and
produces caret layouts. It owns the shared `cosmic_text::Buffer` constructors
and the thread-local repo `TextLayoutContext`. It does not own the
rasterize/cache stage — glyph-mask rasterization, the swash cache, and GPU
atlas submission live in `domains/layer_pool` (see its README) — and it does
not own the resolved font/line-height computation (that is the style system's
`computed/project.rs`).

## Ownership boundaries

- `mod.rs` owns the two `Buffer` constructors (`build_text_buffer`,
  `build_wrapped_text_buffer_and_byte_map`), the wrap-policy text transforms
  (`text_layout_input_for_wrap_policy` and its byte-map sibling), the caret-map
  helpers built from a laid-out buffer, `effective_line_height_px` (the
  render-side line-height backstop), `install_text_font_catalog`, and the
  thread-local repo context. It is the ONLY place a production `Buffer` is
  born.
- `text_layout_context.rs` owns `TextLayoutContext` (the main-world
  `FontSystem` plus its operations): `measure_intrinsic`, `layout_wrapped_text`,
  `measure_wrapped_text_height`, `layout_text_caret`, `layout_text_caret_map`,
  `layout_text_caret_stop_byte_index_at_point`, and the zero-size guards.
- `text_layout_request.rs` owns `TextLayoutRequest`, the immutable input
  carrier (text, font size, line height, white-space/word-break/wrap policy,
  alignment, the resolved family list, weight, style, direction, and the
  language hint).
- `text_render.rs` owns the render-facing helpers: `build_render_text_buffer`
  (the seam's public render entry), `resolve_requested_family` +
  `installed_family_by_name` (S2's first-family-if-installed resolution),
  `preferred_repo_font_family` (language-routing), `ui_sans_repo_font_asset_paths`,
  `cosmic_wrap`, `cosmic_align`, and the embedded font asset paths.
- `font_weight_kw.rs` / `font_style_kw.rs` own the request-carrier weight/style
  keywords and their `cosmic_text` mappings; `resolved_text_style.rs` owns the
  `ResolvedTextStyle` handoff bundle the main-world sites carry into the
  request.
- `text_font_catalog.rs` owns `TextFontCatalog` (the embedded font binary
  sources); `text_layout_config.rs` owns `TextLayoutConfig` (which catalog a
  context is built from); `text_language_id.rs` owns `TextLanguageId` (the
  script-range language hint).

## The three-stage seam (Measure / Shape / Rasterize)

- **Measure** — `measure_intrinsic` derives the fragment-level intrinsic
  surface (`min_content_width_px`, `max_content_width_px`,
  `leading_fragment_width_px`, `trailing_fragment_width_px`, the boundary
  wrap-opportunity flags, `line_height_px`, and `v_min_content_px`) from a
  max-content buffer plus a min-content (width 0) buffer. No third measurement
  and no per-node cosmic pass beyond the two buffers.
- **Shape** — `layout_wrapped_text` (line breaking, `WrappedTextLayout`),
  `layout_text_caret` and `layout_text_caret_map` (caret stops per line,
  `TextCaretLayout` / `TextCaretMap`), all built from a laid-out `Buffer`.
- **Rasterize/cache** — the render world's glyph-mask and GPU submission stage,
  owned by `domains/layer_pool`. This module's only rasterizer-facing surface
  is `build_render_text_buffer` (in `text_render.rs`), the entry every render
  text-run buffer goes through.

## The Buffer rule

`build_text_buffer` and `build_wrapped_text_buffer_and_byte_map` are the ONLY
production `cosmic_text::Buffer` births. Measurement, the caret-layout path,
the render world's text-run cache, and the glyph-mask warmup path all reach a
`Buffer` through one of these two constructors — directly, or through
`text_render::build_render_text_buffer`. There is no third birth site; do not
add one.

## The FontSystem rule

There are exactly two production `cosmic_text::FontSystem`s: the main
thread-local one on `TextLayoutContext` (this module) and the render-owned one
on `TextRunRenderState` (`domains/layer_pool`). Each owns a private fontdb by
cosmic-text's design. What the seam shares — and what any future consumer-door
or catalog change unifies — is the SOURCE list (`TextFontCatalog`), never the
FontSystems themselves.

## The TextStyle handoff (text seam S2)

The style system hands the seam a resolved text style — the parsed
`font-family` list plus the resolved weight/style/direction — which
`TextLayoutRequest` carries into the shaper instead of a hardcoded family id:

- `TextLayoutRequest.family` is the RESOLVED `TextFamilyList` (self-owned
  names, `domains/style/computed`), not an arena key: the render world reads it
  from the extracted record (`TextRunSignature` → `ExtractedTextRunRecord` →
  `RenderTextRunRecord`) and has no document interner. The arena key
  (`TextFamilyListKey`) lives at the computed-group/declaration level only.
- `weight: FontWeightKw` (100–900) and `style: FontStyleKw` (`Normal|Italic`)
  map to `cosmic_text::Weight`/`Style` in `text_layout_attrs`.
- `direction` reuses the text group's `effective_text_direction()`; cosmic-text
  0.18.2 has no paragraph-direction setter, so the attrs auto-detect bidi.
- `text_layout_attrs(request)` resolves family as the requested first
  family-if-installed (ASCII case-insensitive against the four embedded faces),
  else today's language-routing (`preferred_repo_font_family`). This is the
  S4 policy table's rule-1 seed; S4 generalizes to generics, named paths, and
  per-script tests.
- The `TextRunSignature` GAINS `family`/`weight`/`style` (so styled runs
  distinguish) and KEEPS `font_family_id` (Role B: the render-world
  font-registration grouping key) until S3 removes it.

## The zero-size doctrine`font-size: 0` is CSS-legal (CSS Fonts 4 §3.5), but cosmic-text asserts a
non-zero font size when it builds a `Buffer`. Every entry point therefore
guards `font_size_px == 0.0` and returns an honest zero-size answer without
building a buffer: `measure_intrinsic` returns zero widths (keeping the
computed `line_height_px`), `layout_wrapped_text` and
`measure_wrapped_text_height` return zero-width lines at the line count the
white-space policy implies, and the caret paths coincide at the origin (or the
text start for point lookup). The guard and its line-count policy live in
`text_layout_context.rs` (`zero_size_line_count`, `zero_size_caret_map`).

## Invariants, rules, and decisions

- All work happens through `Buffer` instances born from the two constructors
  above; no consumer constructs a `cosmic_text::Buffer` directly.
- The line-height 1px floor is two-layered: the style system floors at resolve
  time (`computed/project.rs`), and this module's `effective_line_height_px`
  plus the two `Buffer` constructors floor again as the render backstop
  (load-bearing against cosmic-text's `line_height != 0` assert).
- The two caret-map entry points `text_caret_map` and
  `text_caret_stop_byte_index_at_point` are unwired: click-to-place caret
  positioning is not wired into any production path, and keyboard-only
  selection is today's editing model. They (and their context-method
  implementations) keep their dead-code allowances; their fate is deferred to
  the editing epic, not decided here.
- Text inputs are boundary-shaped through the white-space/word-break/wrap
  policy transforms; the `keep-all` joiner transform is the one that can
  produce owned (`Cow::Owned`) input plus a byte map back to the original text.
