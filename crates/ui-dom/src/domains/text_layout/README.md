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
  render-side line-height backstop), `install_text_font_catalog_sources`, and the
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
  (the seam's public render entry), `ui_sans_repo_font_asset_paths`,
  `cosmic_wrap`, `cosmic_align`, and the embedded font asset paths. Family
  resolution no longer lives here — it moved to `font_fallback_policy.rs`
  (text seam S4).
- `font_fallback_policy.rs` owns the WHOLE family-resolution policy — the
  fallback table (`resolve`), the render-world face-id resolution
  (`resolve_face_id`), the Thai wrap override (`effective_cosmic_wrap_mode`),
  and the glyph-coverage demotion naming. It is the ONLY place a family is
  resolved; no other file may resolve one.
- `font_weight_kw.rs` / `font_style_kw.rs` own the request-carrier weight/style
  keywords and their `cosmic_text` mappings; `resolved_text_style.rs` owns the
  `ResolvedTextStyle` handoff bundle the main-world sites carry into the
  request.
- `text_font_catalog.rs` owns `TextFontCatalog` (the shared source list of
  embedded + consumer-installed faces, the revision counter, and the public
  `install_face` door); `font_face_id.rs` owns `FontFaceId` (the catalog face
  identity); `font_install_report.rs` owns the install report types;
  `text_layout_config.rs` owns `TextLayoutConfig` (which catalog a context is
  built from); `text_language_id.rs` owns `TextLanguageId` (the script-range
  language hint).

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
- `text_layout_attrs(family, request)` only maps the shaper attrs (weight →
  cosmic `Weight`, style → cosmic `Style`) from the request and the
  already-resolved `FamilyOwned`. Family RESOLUTION happens upstream, in the
  two `Buffer` constructors via `font_fallback_policy::resolve` (the S4 policy
  table's four rules), never in `text_layout_attrs`.
- The `TextRunSignature` carries `family`/`weight`/`style` (so styled runs
  distinguish). The render-world font-registration key is now `FontFaceId`
  (text seam S3): the one-variant `TextFontFamilyId` placeholder enum is
  removed, and the render world resolves a run's face id from its family list
  (`font_fallback_policy::resolve_face_id`).

### The family-list grammar (text seam S2, D3 + R1/R4)

`font-family` parses in `decl_parse.rs` (`parse_family_list`) and serializes in
`style/computed/text_family_list.rs` (`to_css_string`):

- comma-separated items, split QUOTE-AWARE — a comma inside a quoted string is
  name content, not a separator (text seam R1; the pre-R1 `split(',')` dropped
  `"A, B"`);
- each item is a quoted string (single or double; the matching pair is
  stripped) or an unquoted custom-ident sequence; the bare generic keywords
  (`serif`/`sans-serif`/`monospace`/`system-ui`) parse to `Generic(_)`;
- empty items, empty lists, unterminated quotes, names containing BOTH quote
  kinds, and over-capacity lists (the 16-item cap, text seam R2) all drop +
  count;
- `to_css_string` is the canonical wire form and round-trips every parseable
  list: the empty name quotes as `""` (bare drops on replay), quoting prefers
  double quotes and switches to SINGLE quotes when the name contains `"`, and
  the both-quotes rejection keeps the admitted set round-trip-closed (text seam
  R4).

## The catalog and the consumer door (text seam S3)

`TextFontCatalog` is the single source of truth for "which faces exist". It is
context-scoped and shared: a catalog clone wraps one `Arc` of shared state
(`sources`, family names, a revision counter), so a face installed through any
clone is visible to every other clone — the main-world thread-local context and
the render world both read the same state.

- **The source list** — `FaceSource = Embedded(&'static [u8]) |
  ConsumerInstalled { bytes: Arc<Vec<u8>>, id: FontFaceId }`. The four embedded
  vendor faces are the initial list (ids `0..4`); consumer faces receive
  freshly allocated `FontFaceId`s.
- **The door** — `TextFontCatalog::install_face(bytes) -> FontInstallReport`
  parses the bytes with fontdb at install; a malformed buffer is a `Rejected`
  report, never a panic (boundary data). Family names are read from the face's
  name table so the fallback table can route to it by name.
- **Both worlds** — the main-world `TextLayoutContext` installs every catalog
  source at birth and re-installs (sources only) when the catalog revision
  changes, checked cheaply at seam entry. The render world's
  `UiDomTextFontCatalog` resource enumerates the same list: embedded handles
  plus the consumer faces the extract pipeline mints and registers into
  FontSystem #2.

## The fallback policy table (text seam S4)

`font_fallback_policy.rs` owns the whole family-resolution policy — the ONLY
place a family is resolved. `resolve(requested, script, catalog)` walks four
rules in order and returns a `ResolvedFace { family, policy }`:

1. **Requested names** — each named family in the request, ASCII
   case-insensitive, first installed match wins → `ResolutionPath::RequestedName`.
2. **Generic mapping** — the first generic keyword maps through the static
   table (every generic routes to the guaranteed-final face) →
   `ResolutionPath::GenericMapped`. The vendor directory holds real Plex Mono
   faces but they are NOT embedded; a consumer wanting real monospace installs
   one via the S3 door (registry note).
3. **Script coverage** — J/SC/K text routes to the Source Han faces,
   English/Latin to IBM Plex Sans (absorbs the pre-S4 language routing) →
   `ResolutionPath::ScriptCoverage`.
4. **No-face outcome** — Thai has no installed face that covers it, so
   resolution returns the guaranteed-final face (IBM Plex Sans is ALWAYS
   installed) and records `ResolutionPath::NoCoverage`.

The Thai `Word → WordOrGlyph` wrap override lives here too
(`effective_cosmic_wrap_mode`) as the named locale policy beside the
no-coverage reality it names, and the glyph-id-0 debug-rect demotion is NAMED
here as the render answer to `NoCoverage` — counted by
`LayerPoolMetricName::TextRunGlyphCoverageDemotion` (the zero-allocation counter
that makes the demotion policy countable rather than silent).

The resolve API is public seam surface: `TextLanguageId`, `TextFamilyList`,
`ResolutionPath` (`pub + Debug`), and `ResolvedFace` are `pub`; the resolved
family is carried as `cosmic_text::FamilyOwned` (the non-allocating sibling of
`cosmic_text::Family`). The `ResolutionPath` is read via the seam's public
`resolve` API in tests, never emitted.

## RTL and vertical honesty (text seam S5)

**RTL — auto-detect is the pinned truth.** cosmic-text 0.18.2 exposes no
paragraph-direction setter, so `text_layout_attrs` never passes a direction to
the shaper: bidi is auto-detected from the text, and an Arabic-leading string
shapes RTL with no direction hint (pinned by `door_3_rtl_runs_carry_rtl_flags`).
The request's `direction` field therefore governs only start/end alignment
(`text_align_for_css_value`), never shaping. The caret helpers in `mod.rs`
(`glyph_left/right_boundary_byte_index`, `glyph_left/right_boundary_x_px`) flip
caret-stop coordinates on `run.rtl`, and the caret map emits stops in visual
order for mixed LTR/RTL text — Latin stops left-to-right in logical order, then
RTL-segment stops continuing left-to-right with reversed (decreasing) byte
indices (pinned by `door_3_caret_map_orders_visual_runs`).

The run-level `rtl` flag and the caret map are `pub(crate)`-only, so both door-3
pins land crate-internally (door-1/door-2 precedent); the door's public proof is
the observable behavior — the direction property governs start/end alignment —
pinned publicly in `tests/layout_proofs/text/rtl.rs`.

**Named boundaries (caret model).** Two deliberate-deferred boundaries are
registered in the layout-proof registry, not silently absent:

- the caret MOVEMENT model is scalar-offset/LTR-first — arrow-key motion steps
  scalar offsets in logical order, with no visual-order (bidi) traversal;
  `keyboard.rs` owns editing, and the visual-order movement model is out of the
  seam's v1 scope;
- point-hit-testing for caret placement is unwired —
  `text_caret_map`/`text_caret_stop_byte_index_at_point` are the dead entries
  S1 noted; click-to-place caret positioning is not wired into any production
  path, and keyboard-only selection is today's editing model.

**Vertical writing — named boundary.** `writing-mode` remains unadmitted (the
existing `unsupported_property_tail_drops` gate pin stands exactly); vertical
writing is out of the seam's v1 scope, registered as the gate's allowed honest
outcome.

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
