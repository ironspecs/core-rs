# TEXT_SEAM_DESIGN.md — PR 4: the text seam, proven open (complete design)

Status: `COMPLETE DESIGN — no open questions; transcription is
mechanical`. Date: 2026-10-22 (v3, same day). Author: the architect.
Authority: PLAN.md "PR 4 — the text seam" (lines 650–708: the
three-stage seam, the fallback policy table, the TextStyle handoff,
the four-door proof gate, the kill criterion, the context-scoped
catalog ruling, the line-height floor boundary); the text-domain
survey of 2026-10-22 (call-site map, consumption graves, catalog
reality).

**v3 changelog (S4 STOP adjudication, 2026-10-22).** The S4
transcription STOPped on D10(c): door-2 assertion (a) (non-zero
glyph ids) is unobservable through the public pipeline — glyph ids
exist only inside crate-internal shaping, and both escapes (a
test-only public accessor; a crate-internal proof) are foreclosed.
Adjudicated, WITHOUT relaxing the gate's intent:

- **Door-2 (a) is re-expressed as (c), which is strictly stronger.**
  The contextual-shaping assertion (a ligatured pair measuring
  differently than the sum of its isolated characters) is impossible
  under notdef passthrough — uniform advance, no GSUB engagement. It
  publicly proves everything (a) proxied. Door 2's public proof is
  (b) + (c): measure-stage, layout-proofs pipeline.
- **The demotion becomes countable — a production observability fix,
  not test-only visibility.** The survey found the debug-rect
  demotion is SILENT today: a consumer with missing coverage gets
  debug rects with no signal. `LayerPoolMetricName` gains
  `TextRunGlyphCoverageDemotion` (enum-indexed, zero-allocation —
  the doctrine's designed surface; it justifies the demotion
  policy's production existence and gives S4's named `NoCoverage`
  path its observable counterpart). Pinned by a crate-internal
  `layer_pool` test — the render world is extraction-isolated and
  exercised crate-internally BY PRECEDENT (the door-1 FontSystem #2
  registration test landed the same way); the counter itself is
  public production surface.
- **The seam's resolve vocabulary widens to public**:
  `TextLanguageId` and `TextFamilyList` become `pub` (they are the
  policy table's input types; the resolve API is public seam surface
  by design). `ResolutionPath` is `pub + Debug`. The resolved family
  is carried as `FamilyOwned` (the non-allocating sibling of
  `CosmicFamily`).
- **Kill criterion (c) as re-expressed**: door 2 is achievable —
  (b)+(c) publicly, the demotion counter crate-internally. The
  criterion's substance (no test-only visibility, honest public
  proof) is intact; the gate is NOT abandoned.

**v2 changelog (S2 STOP adjudication, 2026-10-22).** The S2
transcription STOPped: `TextFontFamilyId` carries TWO roles and the
design named only one. Adjudicated:

- **The enum survives S2; its death belongs to S3.** Role A (the
  request-carrier family) is S2's target; Role B (the render-world
  font-registration grouping key — `font_registration_state.rs`,
  `layer_pool_render_executor.rs`, the render records, the warmup
  key) is the catalog's concern. S2's `TextRunSignature` GAINS
  family/weight/style AND KEEPS `font_family_id` for Role B; S3's
  registration model replaces and removes it. (The enum lives inline
  in `text_layout/mod.rs` — "its file is removed" was a slip.)
- **Style reaches the render world via the extraction pipeline, not
  via `ComputedFontGroup`.** The render world has no `DomNode`:
  main-world run-emission sites (which HAVE the computed style) write
  the signature's new fields; the signature → `ExtractedTextRunRecord`
  → `RenderTextRunRecord` flow carries them; the two render-world
  request-construction sites read the RECORD's fields. D3's "every
  construction site reads the resolved `ComputedFontGroup`" is
  corrected to: main-world sites read the computed style; render-world
  sites read the record.
- **S2's attrs family resolution** is exactly: requested list's first
  family-if-installed (the S4 table's rule-1 seed), else today's
  language-routing. S4 generalizes this into the full policy table
  (generics, named paths, per-script tests).
- **Direction**: reuses the existing `effective_text_direction()`
  (direction is a text-group field, not font-group). cosmic-text
  0.18.2's `Buffer` exposes NO paragraph-direction setter (verified)
  — auto-detect is the pinned truth; S5's RTL proofs assert the
  auto-detect behavior.
- **Type names**: `TextFamilyList` + `TextFamilyListKey`
  (arena-interned structured list, mirroring `GridTrackList`/
  `GridTrackListKey`); `FontWeightKw` (100–900 → `cosmic_text::
  Weight`); `FontStyleKw` (`Normal | Italic` → `cosmic_text::Style`;
  oblique stays dropped at parse).

This document decides everything for PR 4. Anything unanswered is a
document DEFECT — stop and return to the architect. **No pushes**
except when the user asks (the 2026-10-22 directive).

## D0. PR 4 in one paragraph

Text production is formalized as a sealed three-stage seam —
**Measure** (intrinsic sizing feeding layout), **Shape** (line
breaking, glyph runs, caret maps feeding paint and editing),
**Rasterize/cache** (glyph masks and atlas feeding the GPU) — whose
ONE catalog (context-scoped, per the settled ruling) feeds every
FontSystem, whose fallback policy is a single tested TABLE (family
list → catalog query → generic mapping → guaranteed-final face), and
inside which every cosmic `Buffer` is born (the two existing
constructors are codified as the only births). The style system's
resolved font-family/weight/style stop dying at the request boundary:
a parsed `TextStyle` flows from `ComputedFontGroup` through
`TextLayoutRequest` to `Attrs` and run identity, flipping the two
`off_spec_font_*` rows on-spec. The seam is proven OPEN by four doors
before merge: consumer-supplied faces, a complex script (Arabic),
RTL, and vertical writing (path or named boundary). If the interface
cannot express the doors honestly, we do not make the change — the
fallback is today's seam, documented (the kill criterion).

## D1. The five steps (sequenced landings, each full-gate)

| Step | Lands | Doors |
|---|---|---|
| **S1 — seam formalization** | The module boundary codified (the two `Buffer` births named as the only births; file comments; the missing `text_layout` README; stale citations fixed; the dead-code flags cleaned; test-buffer paths re-homed through the seam's public builders). NO behavior change (structural-migration rules: byte-identical moves). | — |
| **S2 — TextStyle** | Family-list parsing (comma-split, unquote, generic keywords); `TextLayoutRequest` carries the style; every construction site plumbs the computed group; `Attrs` consumes family/weight/style/language; run identity (`TextRunSignature`) gains the style; inherited-context accessors. The two `off_spec_font_*` rows flip on-spec. | — |
| **S3 — one catalog + the consumer door** | `TextFontCatalog` owns the source list (embedded + consumer-installed); `install_face(bytes) -> FaceInstallReport` is the consumer API; both FontSystems load the same catalog; the render-world asset path routes through it. | **Door 1** |
| **S4 — the fallback policy table** | One file: family list → catalog query → generic mapping → guaranteed-final face; absorbs `preferred_repo_font_family`, the Thai wrap override, and the no-face outcome as NAMED policy. Per-script pins. | **Door 2** |
| **S5 — RTL + vertical honesty** | RTL shaping/caret proofs (or named boundaries); the vertical-writing named boundary; registry updates. | **Doors 3, 4** |

The steps are ordered by dependency: S2's consumption needs S1's
codified boundary to land cleanly; S3's door 1 must precede S4's
door 2 (the gate's own rule: the bundled faces cannot cover Arabic);
S5 is honesty work that depends on nothing but the pins.

## D2. The seam boundary (S1, exact)

**The Buffer rule.** `build_text_buffer` and
`build_wrapped_text_buffer_and_byte_map` (`domains/text_layout/
mod.rs:204-231, 288-319`) are codified as the ONLY production
`cosmic_text::Buffer` births — this is ALREADY true in production
(the survey verified it); S1 makes it a stated, lint-visible
invariant (file-level comments; the README). The four test-only
direct constructions in `layer_pool/mod.rs` tests re-home through
`build_render_text_buffer` (the seam's public render entry) — tests
use the public surface, no test-only constructors (doctrine).

**The FontSystem rule.** There are exactly two production FontSystems
(main thread-local, render-owned) — each owns a private fontdb by
cosmic's design; what S3 unifies is their SOURCE list (the catalog),
not the FontSystems. S1 documents this.

**Docs owed in S1** (required-docs rule): create
`crates/ui-dom/src/domains/text_layout/README.md` (purpose, the
three-stage seam, the Buffer/FontSystem rules, the zero-size
doctrine, ownership boundaries) and
`crates/ui-dom/src/domains/layer_pool/README.md` (the rasterize/cache
stage's side of the seam). Fix the stale line-height-floor citations
(registry `README.md:123`, `unsupported_values.rs:476`,
`admitted_gaps.rs:168` → the floor's current homes). Remove the stale
`#[cfg_attr(not(test), allow(dead_code))]` on
`layout_wrapped_text`/`WrappedTextLayout`/`TextCaretMap` (they are
production-live via `paint.rs:479`). The two genuinely-dead caret
entries (`text_caret_map`, `text_caret_stop_byte_index_at_point`) get
a named note in the README (unwired click-to-place; keyboard-only
selection is today's model) — they are NOT deleted in S1 (their fate
is an editing-epic question, out of this PR's scope).

## D3. TextStyle (S2, exact)

**The family-list grammar** (`decl_parse.rs`, replacing the verbatim
`DeclGrammar::Text` for `font-family`): comma-separated items; each
item is a quoted string (single or double; the quote pair is
stripped — the recorded quoted-family caveat) or an unquoted
custom-ident sequence; the generic keywords `serif`, `sans-serif`,
`monospace`, `system-ui` map to `Generic(_)` variants; empty items,
empty lists, and unterminated quotes drop + count. The parsed list is
interned per document (the existing `computed_texts` interner path,
extended to carry the structured list — the wire serializes back per
CSS serialization rules: quote family names containing whitespace or
punctuation, emit generics bare).

**The request carrier** (`text_layout_request.rs`): `TextLayoutRequest`
gains `family: TextFamilyListKey`, `weight: FontWeightKw`,
`style: FontStyleKw`, `direction: DirectionKw` — replacing the
request's hardcoded `TextFontFamilyId::UiSans` (Role A). **The enum
SURVIVES S2** as the render-world registration grouping key (Role B);
S3 removes it. Main-world construction sites (`paint/mod.rs:624-631`,
`flow.rs:1556-1575`, `element_behaviors/paint.rs:168-172, 218-224,
278-288` — verify current lines) read the resolved `ComputedFontGroup`
via the node's computed style; the two render-world sites
(`layer_pool_text_render.rs:777-788`,
`text_run_render_state.rs:127-138`) read the extracted RECORD's
fields (the signature → record pipeline — the render world has no
`DomNode`). The inherited context reuses its existing
`effective_text_direction()` and gains `effective_font_family/
weight/style` beside the size/height pair.

**Consumption**: `text_layout_attrs` (`mod.rs:767-772`) becomes
`text_layout_attrs(request)`: family → the request's first family if
installed, else today's language-routing (the S4 table's rule-1 seed;
S4 generalizes); weight → `cosmic_text::Weight`; style →
`cosmic_text::Style` (oblique remains dropped at parse — the closed
set stands; registry untouched); direction → auto-detect (cosmic
0.18.2 has no paragraph-direction setter — verified; S5's RTL proofs
assert the auto-detect behavior).

**Run identity**: `TextRunSignature` GAINS `family/weight/style`
fields AND KEEPS `font_family_id` (Role B, until S3). Styled runs
distinguish; unstyled runs compare as today.

**The dirty-propagation consequence**: once family/weight/style are
measure inputs, a change re-measures with different inputs; where the
installed faces make the measurement differ, `intrinsic_changed`
becomes true and layout/paint dirty propagates — the silent no-op
dies. Where the faces make NO difference (e.g. weight against a
single static face), the measurement legitimately doesn't change and
no dirty fires — that is correct, and the pin asserts the REQUEST
surface (signature + attrs observability) rather than a phantom
dirty.

**The flips**: `off_spec_font_family_computed_but_unconsumed` and
`off_spec_font_weight_and_style_computed_but_unconsumed` flip
on-spec: the pins are rewritten to assert consumption (a styled
request carries the parsed family/weight/style; `Attrs` reflects
them; the signature distinguishes them). Both registry rows move to
the fixed-in-PR-4 note. `unsupported_font_style_oblique_drops` stays
(closed grammar).

## D4. The catalog and the consumer door (S3, exact)

**One catalog**: `TextFontCatalog` owns `sources: Vec<FaceSource>`
where `FaceSource = Embedded(&'static [u8]) | ConsumerInstalled {
bytes: Arc<Vec<u8>>, id: FontFaceId }`. The four embedded faces are
the initial list. `FontFaceId` is a narrow newtype (the /types
advice).

**The consumer API** (the public seam surface):
`TextFontCatalog::install_face(bytes: &[u8]) -> FontInstallReport`
where `FontInstallReport = Installed { id: FontFaceId, family_names:
ArrayVec<String,4> } | Rejected(FontRejectReason)` — the bytes are
parsed by fontdb at install (a malformed font is a REJECTED report,
never a panic — boundary data). Family names are read from the face's
name table so the fallback table can route to it by name.

**Both worlds load the same catalog**: the main-world FontSystem
installs every catalog source at context birth and re-installs when
the catalog's revision changes (a catalog revision counter; the
context checks it at seam entry — cheap, no per-call reload). The
render world: a consumer-installed face is registered as a Bevy asset
through the existing `UiDomFontAssetLoader` path — the extraction
pipeline already dedups and registers into FontSystem #2; S3 wires
the consumer face INTO that pipeline (the catalog is the single
source of truth for "which faces exist"; the render world's
`UiDomTextFontCatalog` resource enumerates the same list, embedded +
consumer).

**Context-scoped stands** (the settled ruling): the catalog lives on
`TextLayoutConfig`, shared across documents; no document-scoped
variant is introduced.

**Door 1 proof**: the test fixture (D7) is installed via
`install_face` in the proof itself — not embedded — and a string in
the fixture's family measures/rasterizes through the public pipeline
with the fixture's metrics (the run's resolved family is the
fixture's; the measurement differs from the same string in the
default face; the raster path produces non-debug output).

## D5. The fallback policy table (S4, exact)

One file, `domains/text_layout/font_fallback_policy.rs`, owning the
whole policy — the ONLY place family resolution happens:

```text
resolve(requested: &TextFamilyList, script: TextLanguageId, catalog)
    -> ResolvedFace { family: CosmicFamily, policy: ResolutionPath }
```

1. **Requested families in order**: exact (ASCII case-insensitive)
   name match against the catalog's installed family names — first
   match wins. `policy: RequestedName`.
2. **Generic mapping** (static table): `sans-serif` → the catalog's
   sans face (IBM Plex Sans); `serif`/`monospace`/`system-ui` → the
   same guaranteed-final face WITH the path recorded
   (`policy: GenericMapped`) — the vendor directory holds Plex Mono
   faces but they are NOT embedded; a consumer wanting real
   monospace installs one via the D4 door (registry note).
3. **Script coverage** (absorbs `preferred_repo_font_family` and the
   language-hint routing): J/SC/K text routes to the Source Han
   faces as today (`policy: ScriptCoverage`); the per-script pins
   move with the table.
4. **No-face outcome** (absorbs the Thai reality): when no installed
   face covers the script, resolution still returns the
   guaranteed-final face (IBM Plex Sans is ALWAYS installed — the
   invariant the table's tests assert) and records
   `policy: NoCoverage`; the raster demotion (glyph-id-0 → debug
   rect) stays the honest render answer and is now NAMED policy in
   the table's file, with the Thai wrap override
   (`Word → WordOrGlyph`) moved INTO the policy file as the same
   named locale decision.

Every `ResolutionPath` is observable (the resolved attrs carry it in
debug form for proofs; no metric — the path is read via the seam's
public resolve API in tests, not emission).

**Door 2 proof** (depends on door 1; v3 shape): the Arabic fixture
installed via the consumer API; an Arabic string through the public
pipeline: (a) the measured width differs from the same string
measured against the default face (the fixture's metrics are in
use); (b) a contextual-shaping assertion — a two-character sequence
whose ligatured/contextual forms change advance widths (the
fixture's lam-alef if present, verified empirically) measures
differently than the sum of the isolated characters — proving real
shaping, which notdef passthrough CANNOT produce (uniform advance,
no GSUB engagement; strictly stronger than the v1 glyph-id
assertion). **Plus** the `LayerPoolMetricName::
TextRunGlyphCoverageDemotion` counter (v3): the demotion policy's
production observability — pinned crate-internally in `layer_pool`
(the render world is extraction-isolated by architecture, exercised
crate-internally by the door-1 precedent), two-sided (a covered run
does not increment it; an uncovered-script run does).

## D6. RTL and vertical honesty (S5, exact)

**Door 3 — RTL**: `direction: rtl` + Arabic text (the door-2 fixture)
through the public pipeline: (a) the shaped runs carry `rtl` flags
(bidi shaping works — cosmic's auto-detect or the S2 direction
plumbing, whichever the implementation lands, pinned at the true
one); (b) the caret map orders stops per visual runs (the existing
per-run rtl coordinate flips) — pinned with a two-run mixed
LTR/RTL string; (c) the NAMED BOUNDARY, registered: the caret
MOVEMENT model is scalar-offset/LTR-first (no visual-order arrow-key
model; `keyboard.rs` owns editing) and point-hit-testing for caret
placement is unwired — both registered as named boundaries in the
registry, class deliberate-deferred, not silently absent.

**Door 4 — vertical writing**: the named boundary, honestly
documented: `writing-mode` remains unadmitted (the existing gate pin
`unsupported_property_tail_drops` stands); the registry gains the row
naming vertical writing as out of the seam's v1 scope. This IS the
gate's allowed outcome ("an explicitly named boundary … the proof
documents which, honestly").

## D7. The fixture (exact)

`assets/fonts/test-fixtures/` gains ONE OFL Arabic face (chosen:
**Noto Naskh Arabic**, OFL 1.1, regular weight) plus its LICENSE
file. It is NOT embedded (`include_bytes`) anywhere — it is loaded
from disk BY THE PROOFS through the D4 consumer API (that is the
door's point). The fixture directory's README names the provenance
(URL, version, license). If the face's licensing or size proves
dishonest for a foundation repo (binary-weight concern), the
fallback is Amiri (OFL) — either way ONE face, chosen before S3
starts, and the choice recorded in the fixture README.

## D8. The pin table (exact, per step)

| Step | Pins |
|---|---|
| S1 | The full suite green UNCHANGED (structural migration); `cargo xtask lint` (the new READMEs satisfy the required-docs rule); the re-homed test constructions assert through the public builders |
| S2 | `font_family_list_parses_quoted_and_generic` (comma-split, unquote, generics; malformed drops); `font_family_weight_style_flow_to_the_request` (styled node → request carries family/weight/style; signature distinguishes styled/unstyled runs); `font_weight_maps_to_cosmic_attrs` (700 vs 400 in the request → distinct `Attrs` weight — observable via the seam's public surface); the two `off_spec_font_*` rows REWRITTEN on-spec (consumption asserted); the quoted-family canonical round-trip pin UPDATED (parsed serialization replaces verbatim interning) |
| S3 | `install_face_accepts_a_valid_font` (report Installed, family names read); `install_face_rejects_garbage_bytes` (Rejected, no panic); `consumer_face_renders_through_the_public_pipeline` (Door 1: measure + raster assert the fixture's metrics/family; the render-world registration observed); `catalog_revision_reloads_the_main_world_context` (install → next seam entry sees the face) |
| S4 | `fallback_requested_name_wins` (installed name resolves `RequestedName`); `fallback_generic_maps` (sans-serif → IBM Plex; monospace → final face, `GenericMapped`); `fallback_script_coverage_routes_cjk` (the per-script pins, moved); `fallback_no_coverage_is_named` (Thai: `NoCoverage` + the named wrap policy); `fallback_final_face_is_always_installed` (the invariant); `door_2_arabic_shapes_with_real_glyphs` (v3: the width-differs + contextual-ligature assertions through the public pipeline); `glyph_coverage_demotion_counts` (v3: the counter, two-sided, crate-internal `layer_pool`) |
| S5 | `door_3_rtl_runs_carry_rtl_flags`; `door_3_caret_map_orders_visual_runs`; the two caret-model boundary registry rows; the `writing-mode` gate pin UNMOVED + the vertical boundary row |

Perf/alloc pins re-pin ONLY with documented cause (S2's request
plumbing and S3's catalog checks are the likely movers; the shaping
baseline windows are owned by this PR per FLEX_MAPPING.md — any move
is recorded as a PR-4 entry, never bundled).

## D9. Explicitly untouched (named)

The line-height 1px floor stays in `project.rs` (permanent pin does
not move; the seam owns only the render-side backstops). Oblique
stays dropped (closed grammar). The two dead caret entries are noted,
not deleted. `break-spaces`, `line-height: normal`, the range-pill,
and box-sizing rows are untouched. The popup/options migration is
PR 5's. Vertical writing is a named boundary, not a path.

## D10. The kill criterion (operationalized)

Before S3 starts, the D4 interface is written down as PUBLIC
SIGNATURES (this document's D4 is that writing). If during S3/S4 any
of these proves false, the step STOPs and the PR abandons to today's
seam, documented: (a) `install_face` can route a consumer face to
BOTH worlds through the public API; (b) the policy table can express
an arbitrary requested family name (no narrowing of what today's
verbatim interning could request); (c) door 2's shaping assertions
are achievable through the public pipeline without test-only
visibility. The fallback on abandonment: today's seam plus this
document's D1–D2 docs, with the kill reason recorded in PLAN.md.

## D11. File map (headline; per-step maps ride each step's brief)

| Surface | Steps |
|---|---|
| `domains/text_layout/` (mod, context, request, render, catalog, config, language, + NEW `font_fallback_policy.rs`, + NEW `README.md`) | S1–S5 |
| `domains/layer_pool/` (text render, render state, registration, + NEW `README.md`) | S1, S3 |
| `domains/style/` (decl_parse family grammar, presentation_property, computed font group, project accessors, inherited context) | S2 |
| `text_layout_request.rs`, `text_run_signature.rs`, the six request construction sites | S2 |
| `src/ui_dom_text_font_catalog.rs`, `bevy/ui_dom_font_asset_loader.rs`, extraction pipeline | S3 |
| `assets/fonts/test-fixtures/` + README + LICENSE | S3 |
| `tests/layout_proofs/` (admitted_gaps flips, text/, style/, registry README) | S2–S5 |
| `tests/perf_allocation.rs`, `tests/perf_metric.rs` | CONDITIONAL, documented cause |

## D12. The transcription contract (all steps)

Implement THIS DOCUMENT exactly. STOP conditions: (1) any case the
document does not answer (name it); (2) any existing pin moves except
the D8-named flips/rewrites/moves; (3) a perf pin moves without a
documented cause; (4) any push (the user's directive: pushes only
when asked); (5) any test-only visibility or mock introduced (the
seam's observability is public-API-only). Each step is its own
commit(s), full gate green per commit, on the `text/seam` branch.
