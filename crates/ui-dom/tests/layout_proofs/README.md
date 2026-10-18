# layout_proofs

These tests are the sealed-module layout proofs for `ui-dom`: hermetic (no
hardware, no external services; the render world is faked at the established
noop/manual seam), asserting the module's public promises through `RootDom`
and the staged render ops surface.

Each proof should follow the same shape:

1. Build a small authored scenario through public `RootDom` APIs.
2. Run one real `RootDom::update(...)` step.
3. Assert the staged paint plans and retained layout results for the affected
   nodes.

Guidelines:

- Keep each proof focused on one layout claim.
- Proof families are table-driven where the claim admits it: one test function
  driving a table of authored scenarios and expected outcomes, so coverage is
  enumerable in review.
- The staged paint plan is the primary measure. Op counts, kinds, order, and
  geometry are the cost the crate actually pays and the exact handoff the
  render world draws; a change that moves ops is a real cost change, visible
  without a GPU. Assert plan contents (`assert_rect_op_geometry` and friends);
  retained layout assertions (`assert_node_layout`) are secondary.
- New or changed browser-like claims must name the spec section and closest WPT
  or browser test that defines the expected behavior.
- New or changed solver-facing claims must name the Gecko and Blink
  implementation files whose phase shape the `ui-dom` solver is expected to
  follow.
- If no WPT/browser test is available for a new or changed proof, state that
  explicitly in the module comment, cite the governing spec plus Gecko/Blink
  implementation files, and state the exact browser-backed subset being pinned.
  Do not use a local invariant label as a substitute for browser provenance.
- Put source references in the module or test that uses them, not only in a
  separate evidence bundle.
- Use helpers from `support.rs` instead of duplicating world setup.
- Scenario setup helpers return tuples in the exact order documented in
  `support.rs`; keep destructured variable names aligned with that order.
- Assert layout results and staged ops, not incidental runtime details.
- Known off-spec behavior is pinned by a *running* proof that asserts the
  current values and is labeled off-spec in its header: spec section, closest
  WPT oracle, Gecko/Blink implementation files, the spec-correct expected
  values, and the named production branch that causes the divergence. The
  proof must fail when the behavior moves in either direction — moving onto
  spec is the good outcome and means relabeling the proof on-spec. `#[ignore]`
  is not permitted in this tree; an ignored proof hides divergence movement
  and lets its expectations rot.

## Table conventions

Proof families that admit tabulation are table-driven:

- One `#[test]` function per claim family drives a table of cases; the table
  is the enumerable coverage and is reviewed as such.
- Every row carries a `name` used in assertion labels, so a failure names the
  row.
- Staged-ops assertions pin exact plan counts and exact op counts per kind
  per plan (counts are platform-stable; exact equality is required), plus
  indexed-op geometry through `assert_close`.
- Retained-layout assertions are secondary to staged-ops assertions.
- Rows build scenarios through `support.rs` helpers and public `RootDom`
  APIs only.

## Spec coverage map

Keyed by governing spec. Provenance legend: **full** = spec sections + WPT or
browser oracles + pinned Gecko/Blink implementation files; **partial** = spec
sections only; **crate-owned** = pins crate invariants by design.

| Spec area | Pinning modules | Provenance |
|---|---|---|
| CSS 2.1 §8 box model, §8.3.1 margin collapsing | `layout/normal_flow.rs`, `layout/content_height.rs` | partial (impl files in `layout/normal_flow.rs`) |
| CSS 2.1 §9.2–9.4 block/inline formatting, §10.3/10.6 width and height | `layout/block_inline.rs`, `layout/content_height.rs`, `text/text_auto_size.rs` | full in `layout/block_inline.rs`; partial elsewhere; `text/text_auto_size.rs` carries per-test Blink/Gecko notes |
| CSS 2.1 §9.3 positioning, §10.3.7/10.6.4 absolutely positioned | `layout/fixed_layers.rs`, `layout/normal_flow.rs`, `pipeline/transitions.rs` | partial |
| CSS 2.1 §10.3.2/10.6.2 replaced elements, CSS Images 3 §5.3 | `layout/replaced_elements.rs` | full, with a declared deliberate divergence (see registry) |
| CSS 2.1 §10.7 min/max sizes, §11.1.1 overflow; CSS 2.2 §10.6.6/10.6.7 | `widgets/popup_menus.rs`, `widgets/popup_text_rows.rs` | partial in `widgets/popup_text_rows.rs`; `widgets/popup_menus.rs` is crate-owned |
| CSS Flexbox 1 | `layout/flex_layout.rs` | full; unadmitted subsets gated in `src/domains/layout/flex.rs` |
| CSS Text 4 `white-space`; CSS Text 3 wrapping and word-break | `text/white_space.rs`, `text/text_auto_size.rs` | full in `text/white_space.rs` |
| CSS Forms 1 `field-sizing`, HTML rendering widgets, CSS Sizing 3 | `layout/form_control_auto_size.rs` | full |
| CSS Transitions 1 | `pipeline/transitions.rs` | partial |
| HTML Standard "update the rendering" (pipeline ordering) | `pipeline/stylesheet_lifecycle.rs` | full (explicit no-single-WPT statement) |
| CSS Cascading and Inheritance: cascade order, specificity, inheritance transmission; CSS 2.1 §4.1.7 + css-style-attr §3 declaration error handling; CSS Fonts 4 §3.1–3.3 font-property inheritance | `style/cascade.rs` | full (module-header + per-family provenance; no divergences) |
| Sign rules & numeric edges (CSS Box 3 §4.4, CSS Sizing 3, CSS Backgrounds 3, CSS UI 4, CSS Fonts 4 §3.5, CSS Color 4 §12.2, CSS Flexbox 1 §5.4/§7.3, CSS 2.1 §10.8) | `style/unsupported_values.rs` | partial; classes mixed (8 divergent rows in the off-spec registry + on-spec keeps + robustness pins) |

## Known off-spec registry

Every known divergence from the cited specs is registered here and pinned by a
running test. Registry classes:

- **divergent** — behavior contradicts the spec; pinned at current values and
  expected to move on-spec.
- **deliberate** — a crate-owned value replacing a spec default where the
  spec's default has no meaning for our world; pinned permanently and
  re-reviewed when the spec area is revisited.
- **unimplemented** — gates named `unsupported_*`: policy-gated absence (the
  D6 admission policy in `MIGRATION.md`); the pin asserts the gate behavior
  (dropped + counted, or load error) and no crash.
- **robustness** — pins named `robustness_*`: crash-resistance over adversarial
  inputs; orthogonal to the classes above (a robustness pin also carries the
  divergent/unimplemented/on-spec class of the behavior it guards).

Off-spec proofs are named `off_spec_*` so
`cargo test --test layout_proofs off_spec` enumerates the divergent registry.
When an off-spec proof fails, the behavior moved; the same change must
classify the move: unintended regressions are reverted, intended divergence
changes update the pinned values and this registry, and arrival at spec
relabels the proof on-spec.

| Proof or gate | Class | Spec claim | Current (pinned) vs spec-correct | Root cause |
|---|---|---|---|---|
| `off_spec_adjacent_negative_block_margins_collapse_to_most_negative` (`layout/normal_flow.rs`) | divergent | CSS 2.1 §8.3.1 final paragraph: adjoining negative margins collapse to the most negative | sums −10px and −5px to −15px; spec: −10px | `collapsed_sibling_margin_overlap_px` clamps the collapse overlap to ≥ 0 in `src/domains/layout/flow.rs` |
| 16:9 placeholder intrinsic ratio (`layout/replaced_elements.rs`, declared in module header) | deliberate | CSS Images 3 §5.3 default object size 300×150 applies to images with no intrinsic dimensions | unloaded `img` seeds a 16:9 placeholder ratio; the §10.3.2/10.6.2 transfer math stays on-spec | crate-owned placeholder policy for never-loaded images |
| Flex subsets beyond batches 3–7 | unimplemented | CSS Flexbox 1 areas the active solver does not admit | admission gated by `active_flex_gate_*` unit tests in `src/domains/layout/flex.rs`; fallback is the placeholder solver | active solver subset; see flex coverage map |
| Cascade model: no `!important`, no built-in UA stylesheet, no cascade layers | unimplemented | CSS Cascade 5 §6: important declarations, origin layers | precedence is exactly `{ origin_priority, inline, specificity, order }` (the `inline` flag sorts above any selector per css-style-attr §4), pinned by `style/cascade.rs` families 1–3 | `domains/style` precedence model; the present model is pinned, the absent features await a redesign decision |
| `off_spec_negative_padding_accepted` (`style/unsupported_values.rs`) | divergent | CSS Box 3 §4.4: `padding-*` is `<length [0,∞]>` | `.x { padding-left: -10px }` loads with `dropped == 0`, `padding[3] == -10.0`; spec-correct: drop + count | `parse_length` (`decl_parse.rs:319-321`) is sign-agnostic; `Length` grammar admits negatives |
| `off_spec_negative_box_sizes_accepted` (`style/unsupported_values.rs`) | divergent | CSS Sizing 3 §3.1/§3.3/§3.4: `width`/`height`/`min-*`/`max-*` are non-negative | each of the six at `-1px` loads with `dropped == 0`, computed `Some(-1.0)`; spec-correct: drop + count | the six box-size longhands share the sign-agnostic `Length` grammar |
| `off_spec_negative_border_and_outline_widths_accepted` (`style/unsupported_values.rs`) | divergent | CSS Backgrounds 3 §4.3 / CSS UI 4 §4.3: `<line-width>` non-negative | `border-top-width: -2px`, `outline-width: -2px` load with `dropped == 0`; spec-correct: drop + count | `Length` grammar admits negative border/outline widths |
| `off_spec_negative_shorthand_lengths_accepted` (`style/unsupported_values.rs`) | divergent | CSS Backgrounds 3 §5.1 / CSS Box Alignment 3 §4.4: radius/gap non-negative | `border-radius: -4px`, `gap: -4px`, `border-top: -2px` all load with `dropped == 0`; spec-correct: drop + count | expansion helpers bypass the grammar layer (F2): `expand_border_radius` (:660), `expand_gap` (:703), `expand_border_side` (:930) call sign-agnostic `parse_length` |
| `off_spec_negative_font_size_accepted` (`style/unsupported_values.rs`) | divergent | CSS Fonts 4 §3.5: `font-size` non-negative | `font-size: -10px` loads with `dropped == 0`, `size_px == -10.0` (crash-adjacent); spec-correct: drop + count | `LengthNoUnset` grammar sign-agnostic; `resolve_own_font_size_px` (`winners_fold.rs:324`) stores px verbatim |
| `off_spec_font_size_unset_rejected` (`style/unsupported_values.rs`) | divergent | CSS Cascade 5 §4.3: `unset` is CSS-wide (≈ inherit for inherited `font-size`) | `font-size: unset` drops + counts (`dropped == 1`); spec-correct: admit, resolve to inherited seed | `parse_length_no_unset` (`decl_parse.rs:323-328`) returns `None` for `AuthoredLength::Unset` |
| `off_spec_font_size_zero_panics` (`style/unsupported_values.rs`) | divergent | CSS Fonts 4 §3.5 admits zero font sizes; boundary data must never panic (AGENTS.md) | `font-size: 0` is admitted, then measuring/shaping the text PANICS in cosmic-text (`assert_ne!(font_size, 0.0)`, `buffer.rs:688`), pinned via `should_panic`; spec-correct: zero-width measure, renders nothing | the zero value flows verbatim from the font group into `CosmicMetrics::new` (`text_layout/mod.rs`) with no zero guard; relabels to `robustness_zero_font_size_renders_nothing` with landing (b)'s hardening |
| `off_spec_opacity_out_of_range_rejected` (`style/unsupported_values.rs`) | divergent (reject-side) | CSS Color 4 §12.2: `opacity` clamps to `[0,1]` at computed time | `opacity: 1.5` / `-0.5` drop + count; spec-correct: admit + clamp (does NOT flip in B1) | `parse_opacity` (`decl_parse.rs:360-367`) rejects the 0.0..=1.0 range instead of clamping |
| `off_spec_line_height_zero_floored_at_one_px` (`style/unsupported_values.rs`) | divergent (render-floor) | CSS 2.1 §10.8: `line-height: 0` is legal (zero-height lines) | `line-height: 0` admitted, floored to 1px (`text_layout/mod.rs:719-722`); spec-correct: render nothing (does NOT flip in B1) | `line_height.max(1.0)` in `resolve_font_field` (`project.rs:137`) and `effective_line_height_px` (`text_layout/mod.rs:720`); floor is load-bearing against cosmic-text's `line_height != 0` assert |

Popup coverage map:

- `widgets/popup_menus.rs`: synthetic fixed-row popup clamp math.
- `widgets/popup_text_rows.rs`: browser-style text-row popup retained layout for below-max,
  over-max, live growth, `absolute` positioning, wrapped rows, partial-shrink,
  and empty-shrink behavior.
- `text/text_auto_size.rs`: row-level text auto sizing and wrapped-text growth.

Flex coverage map:

- `layout/flex_layout.rs`: spec-backed browser flex proofs grouped by staged solver
  batches.
- Batch 3: positive free space distribution and negative free space
  distribution. These proofs are active.
- Batch 4: order-modified visual order, row-reverse, column axis behavior, and
  column-reverse are active.
- Batch 5: row wrap, wrap-reverse, and shorthand-driven column wrapping are active.
- Batch 6: main-axis alignment, cross-axis alignment, align-self override, and
  gaps between items and lines are active.
- Batch 7: automatic minimum main size is active for the current supported
  horizontal no-wrap subset and the explicit-basis vertical block-size slice.
- Remaining unsupported flex subset cases are pinned by the active-solver gate
  tests in `crates/ui-dom/src/domains/layout/flex.rs`.

Normal-flow coverage map:

- `layout/block_inline.rs`: block stacking, inline line building, block-after-inline
  line breaks, content-box origins under padding, inline shrink-wrap, inline
  sibling participation, and inline-block text chip shrink-wrap.
- `layout/content_height.rs`: auto-height accumulation, empty collapse, child removal,
  and flow participation rules.
- `layout/fixed_layers.rs`: the same block/inline invariants when the containing block
  comes from a fixed-position layer root.
- `layout/normal_flow.rs`: positioning-scheme participation and scroll-content-height
  invariants.
- `text/white_space.rs`: CSS Text-backed `white-space` collapse, preservation, and
  wrapping behavior.
- `pipeline/stylesheet_lifecycle.rs`: stylesheet load/attach/append/import orderings
  must all present styled content on the first update, and unloaded
  stylesheets must stay unapplied.

Known boundary split:

- `layout_proofs/` proves retained layout, including relayout after content removal.
- `src/domains/composite/mod.rs` tests must still prove clipped-layer
  presentation size, because rendered popup height can diverge from retained
  layout if that seam goes stale.
- Metric and allocation pins do not live in `layout_proofs/`; they live in
  `tests/perf_metric.rs` (work-site and node-store counters) and
  `tests/perf_allocation.rs` (allocator windows).
- Pointer and interaction coverage does not live in `layout_proofs/`.
  - Attached event targeting: `src/domains/visitor/event_target.rs`
  - Hover and active pseudo-state targeting: `src/domains/visitor/pseudo_state.rs`
  - Click activation suppression: `src/types/element_behaviors/activation.rs`
  - Focus repair on attr mutation: `src/types/root_dom.rs`
  - The next actionable gaps are documented beside those tests, not in a separate tracker.
