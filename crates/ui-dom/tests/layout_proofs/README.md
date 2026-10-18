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
| Sign rules & numeric edges; units & sizing keywords (CSS Box 3 §4.4, CSS Sizing 3, CSS Backgrounds 3, CSS UI 4, CSS Fonts 4 §3.5, CSS Color 4 §12.2, CSS Flexbox 1 §5.4/§7.3, CSS 2.1 §10.8; CSS Values 4 §percentages/§math/§viewport-relative/§font-relative, CSS Sizing 3 `auto`/intrinsic keywords, CSS Cascade 5 CSS-wide keywords) | `style/unsupported_values.rs` | partial; classes mixed (sign rules relabeled on-spec in B1 landing (b); 1 divergent (opacity) + 1 deliberate (line-height floor) remain registered + on-spec keeps + robustness pins + unsupported unit/keyword gates) |
| Box shorthands (CSS Box 3 §4.2/§4.3 `margin`/`padding`; CSS Backgrounds 3 §4.2/§4.3 `border`/`border-width`; CSS Position 3 §6 `inset`; CSS UI 4 §4.3 `outline`) | `style/unsupported_shorthands.rs` | partial; all rows unimplemented (D6 admission-gated drop-and-count + group-initial pins) |

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
| `off_spec_opacity_out_of_range_rejected` (`style/unsupported_values.rs`) | divergent (reject-side) | CSS Color 4 §12.2: `opacity` clamps to `[0,1]` at computed time | `opacity: 1.5` / `-0.5` drop + count; spec-correct: admit + clamp (does NOT flip in B1) | `parse_opacity` (`decl_parse.rs:360-367`) rejects the 0.0..=1.0 range instead of clamping |
| `line_height_zero_floors_at_one_px` (`style/unsupported_values.rs`) | deliberate | CSS 2.1 §10.8: `line-height: 0` is legal (zero-height lines) | `line-height: 0` is admitted and floored to 1px at resolve time | the floor (`project.rs:137`, render backstop at `text_layout/mod.rs:719-722`) is load-bearing against cosmic-text's `line_height != 0` assert (`buffer.rs:254`) — an engine-dependency constraint, pinned permanently |
| Unit & sizing-keyword gates (`unsupported_percentage_lengths_drop`, `unsupported_math_functions_drop`, `unsupported_viewport_units_drop`, `unsupported_font_relative_units_drop`, `unsupported_auto_lengths_drop`, `unsupported_intrinsic_size_keywords_drop`, `unsupported_css_wide_keywords_drop` in `style/unsupported_values.rs`) | unimplemented | CSS Values 4 §percentages/§math/§viewport-relative/§font-relative; CSS Sizing 3 `auto` and intrinsic size keywords; CSS Cascade 5 CSS-wide keywords beyond `unset` | each name/grammar form drops + counts, computed values stay at group initials, no crash | the closed `DeclGrammar` table in `src/domains/style/presentation_property.rs`; admission per the D6 policy |
| Box-shorthand gates (`unsupported_margin_shorthand_drops`, `unsupported_padding_shorthand_drops`, `unsupported_border_and_border_width_shorthands_drop`, `unsupported_inset_shorthand_drops`, `unsupported_outline_shorthand_drops` in `style/unsupported_shorthands.rs`) | unimplemented | CSS Box 3 §4.2/§4.3 `margin`/`padding` shorthands; CSS Backgrounds 3 §4.2/§4.3 `border`/`border-width` shorthands; CSS Position 3 §6 `inset`; CSS UI 4 §4.3 `outline` | each shorthand name drops + counts, the corresponding longhand group fields stay at their group initial, no crash | the names are absent from the `property_shape` name table in `src/domains/style/decl_parse.rs`; admission per the D6 policy |

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
