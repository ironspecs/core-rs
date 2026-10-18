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
| CSS 2.1 §8 box model, §8.3.1 margin collapsing | `layout/normal_flow.rs`, `layout/content_height.rs` | partial (impl files in `layout/normal_flow.rs`); two registered divergent slices: `off_spec_parent_boundary_negative_margins_clamp` (parent-boundary `.max(0.0)` clamp) and `off_spec_empty_block_negative_margins_collapse_to_max_not_most_negative` (empty-block `max` self-collapse) |
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
| Sign rules & numeric edges; units & sizing keywords (CSS Box 3 §4.4, CSS Sizing 3, CSS Backgrounds 3, CSS UI 4, CSS Fonts 4 §3.5, CSS Color 4 §12.2, CSS Flexbox 1 §5.4/§7.3, CSS 2.1 §10.8; CSS Values 4 §percentages/§math/§viewport-relative/§font-relative, CSS Sizing 3 `auto`/intrinsic keywords, CSS Cascade 5 CSS-wide keywords) | `style/unsupported_values.rs` | partial; classes mixed (sign rules relabeled on-spec in B1 landing (b), the opacity clamp moved on-spec in PR 1 item 5, and the percentage/`auto` families moved on-spec in PR 1 item 7 via the computed/used `BoxLength` split; 1 deliberate (line-height floor) remains registered + on-spec keeps + robustness pins + the remaining unit/keyword gates). The percentage-height indefinite-basis resolution moved on-spec in PR 17 (the four `percentage_*_against_indefinite_basis`/`percentage_height_bases_resolve_when_definite` pins); `flex-basis` percentages and relative top/bottom percentages remain unimplemented |
| Box shorthands (CSS Backgrounds 3 §4.2/§4.3 `border`/`border-width`; CSS Position 3 §6 `inset`; CSS UI 4 §4.3 `outline`); other shorthands & overflow axes (CSS Fonts 4 §font, CSS Backgrounds 3 §background, CSS Cascade 5 §all, CSS Overflow 3 §overflow-x/§overflow-y) | `style/unsupported_shorthands.rs` | partial; remaining rows unimplemented (D6 admission-gated drop-and-count + group-initial pins); the CSS Box 3 §4.2/§4.3 `margin`/`padding` rows moved on-spec in PR 1 item 1 (`margin_shorthand_expands_to_longhands`, `padding_shorthand_expands_to_longhands`) |
| Layout-impact property-table absences (`z-index`, `position: sticky`, `display: grid`/`inline-grid`/`table`/`inline-table`/`flow-root`/`contents`/`ruby`, `aspect-ratio`/`object-fit`/`object-position`, `float`/`clear`, `contain`/`content-visibility`/`break-inside`/`resize`/`scroll-behavior`/`scroll-snap-type`/`overscroll-behavior`/`touch-action`/`will-change`; CSS 2.1 Appendix E & §9.5, CSS Position 3 §sticky, CSS Display 3 §2, CSS Sizing 4/Images 3 replaced hints, containment/fragmentation/scroll-snap/overscroll/pointer-events/will-change) + text/effects property tail (`text-decoration` shorthand+longhands, `box-shadow`, `text-transform`/`text-indent`/`text-shadow`/`text-overflow`/`letter-spacing`/`word-spacing`/`line-clamp`/`-webkit-line-clamp`/`hyphens`, `vertical-align`, `writing-mode`/`unicode-bidi`, `visibility`, `cursor`/`user-select`, `list-style*`, `filter`/`mix-blend-mode`/`backdrop-filter`, `clip-path`/`mask*`, `image-rendering`, `border-style`, table-*, column-*, `orphans`/`widows`, `animation*`, the value-function family, `content`/`quotes`/`counter-*`; CSS Text Decoration 4 §2, CSS Backgrounds 3 §7.1, CSS Text 4, CSS Overflow 3/4, CSS Inline Layout 3, CSS Writing Modes 4, CSS UI 4, CSS Lists 3, Filter Effects 1/2, Compositing 1, CSS Masking 1, CSS Images 3/4, CSS 2.1 §17/§13.3, CSS Multi-column 1, CSS Fragmentation 3, CSS Animations 1, CSS Values 4/5, CSS Content 3) | `style/unsupported_properties.rs` | partial; all rows unimplemented (D6 admission-gated drop-and-count + per-family no-effect pins); the `box-sizing` row moved on-spec in PR 1 item 3 (`box_sizing_border_box_subtracts_insets`), and one registered divergent slice remains: `off_spec_box_sizing_min_max_sizes_clamp_in_content_box_terms` (min/max clamps interpret the specified size in content-box terms regardless of `box-sizing`, CSS UI 4 §3.1) |
| Color grammar (`#abc`/`#abcd` hex shorthands, wider named colors, `currentColor`, modern `rgb()` space/slash forms; CSS Color 4 §4.2 hex + `rgb()`, §6.1 named colors, §`currentColor`) | `style/unsupported_colors.rs` | partial; classes mixed (space/comma/percent `rgb()` forms on-spec; hex3/4 + wider named colors + `currentColor` + slash-alpha unimplemented) |
| Admitted grammars whose computed values are ignored/approximated/quirky downstream (CSS Fonts 4 §3.1–3.3; CSS Text 3 §7.1 `text-align`; CSS Text 4 `white-space`; CSS Transitions 1 §2; CSS Backgrounds 3 §5.1; CSS Writing Modes 4 `direction`; pointer-events content vs stylesheet channel; CSS Color 4 §12 `color` initial; CSS 2.1 §10.8 `line-height: normal`; CSS Backgrounds 3 §4.2 `border-width` initial) | `style/admitted_gaps.rs` | partial; mixed classes (font-family/weight/style + range zero-radius + break-spaces + `line-height: normal` → 1.0× divergent; `text-align: justify`/`dir`-vs-`direction`/pointer-events auto-none on-spec; `font-style: oblique` + pointer-events wider values/stylesheet channel unimplemented; near-white initial `color` + zero initial border widths deliberate; transform transitions moved on-spec in PR 1 item 6 (`transform_transitions_interpolate_translate`), with the `none` ↔ translate identity interpolation a documented follow-up pinned by `transform_none_to_translate_transition_steps`) |
| Selector grammar boundary & at-rules (CSS Selectors 4 §5 grouping, §6 attribute selectors, §7 structural/state pseudo-classes + `:not()`, §13 combinators + pseudo-elements; CSS Conditional 3 `@media`/`@supports`; CSS Animations 1 `@keyframes`; CSS Fonts 4 `@font-face`; CSS Cascade 5 `@import`; CSS Syntax 3 `@charset` + malformed-statement recovery) | `style/unsupported_selectors.rs` | partial; all rows unimplemented (D6 admission-gated load-error gates + block-structure failures); the F1 failed-load-atomicity row is on-spec |
| Cascade tiers (`!important` important tier, custom properties/`var()`, `@layer` cascade layers, the animation tier, the documented UA-sheet workaround; CSS Cascade 5 §6.1/§6.4.1/§6.4.2, CSS Custom Properties 1, CSS Animations 1) | `style/cascade_tiers.rs` | partial; `!important` admitted (F stage 1, PR 1 item 4); custom-property/`var()`/`@layer`/`animation` rows unimplemented (D6 admission-gated, F stages 3/4); the UA-sheet workaround row on-spec |

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
| `layer_bounds_clip_spillover_by_design` (`layout/normal_flow.rs`) | deliberate | CSS 2.1 §9.2/§9.4: a browser paints content pulled outside an element's box by negative margins | content spilling outside a clipped layer's bounds (negative margins, `overflow: visible` children, transforms) is clipped at the layer's border box via the composite `clip_world`; the escape hatch is an unclipped (Normal) layer, pinned by `normal_layer_paints_spillover_escape_hatch` | bounded layer surfaces ARE the render cache (scroll, animation, targeted repaint); spillover is a layering decision, never a surface-expansion decision (the 2026-10-18 decision, PLAN.md decision log) |
| 16:9 placeholder intrinsic ratio (`layout/replaced_elements.rs`, declared in module header) | deliberate | CSS Images 3 §5.3 default object size 300×150 applies to images with no intrinsic dimensions | unloaded `img` seeds a 16:9 placeholder ratio; the §10.3.2/10.6.2 transfer math stays on-spec | crate-owned placeholder policy for never-loaded images |
| Flex subsets beyond the active solver's coverage | unimplemented | CSS Flexbox 1 areas the active solver (`resolve_active_flex_container`, `src/domains/layout/flex.rs`) does not admit | the boundary is held two ways: the closed declaration grammar rejects unsupported value shapes at parse (fail-closed, `decl_parse.rs`), and the one explicit support gate is `flex_container_content_based_auto_min_is_supported` (`flex.rs:1436`, content-based auto-min only for horizontal non-reversed no-wrap containers, exercised by the `measured_item_record_*`/`resolve_active_flex_container_*` unit tests in the same file) | the active-solver subset is the coverage pinned by `layout/flex_layout.rs` batches 3–7; anything beyond it is admission-gated per the D6 policy |
| Cascade model: no built-in UA stylesheet, no cascade layers | unimplemented | CSS Cascade 5 §6: origin layers | precedence is exactly `{ tier, origin_priority, inline, specificity, order }` (the `inline` flag sorts above any selector per css-style-attr §4), pinned by `style/cascade.rs` families 1–3; the `tier` field projects `ImportantAuthor > NormalAuthor > NormalUa` and `!important` is admitted (F stage 1, PR 1 item 4) | `domains/style` precedence model; the present model is pinned, the absent features await a redesign decision |
| `line_height_zero_floors_at_one_px` (`style/unsupported_values.rs`) | deliberate | CSS 2.1 §10.8: `line-height: 0` is legal (zero-height lines) | `line-height: 0` is admitted and floored to 1px at resolve time | the floor (`project.rs:137`, render backstop at `text_layout/mod.rs:719-722`) is load-bearing against cosmic-text's `line_height != 0` assert (`buffer.rs:254`) — an engine-dependency constraint, pinned permanently |
| `off_spec_parent_boundary_negative_margins_clamp` (`layout/normal_flow.rs`) | divergent | CSS 2.1 §8.3.1: a first in-flow child's top margin (last in-flow child's bottom margin) adjoins the parent's and collapses by the same three-branch rule as siblings — a negative child margin passes through the boundary (parent 0 + child −20 → −20) | a negative first-child top / last-child bottom margin is clamped to 0 at the parent boundary; the empty-intermediary propagation applies the same clamp | `collapsed_parent_top_margin_px` / `collapsed_parent_bottom_margin_px` (`flow.rs`) keep the pre-item-2 non-negative scoping (`.max(0.0)`); admitting the three-branch rule at the boundary is the same shape as PR 1 item 2, deferred per D6 (no named consumer need) |
| `off_spec_empty_block_negative_margins_collapse_to_max_not_most_negative` (`layout/normal_flow.rs`) | divergent | CSS 2.1 §8.3.1: an empty block's own adjoining margins collapse by the three-branch rule — two negative margins collapse to the most negative (−10, −5 → −10) | the self-collapse uses plain `max` (−10, −5 → −5) | the empty-block path in `layout_flow_size` (`flow.rs`) predates PR 1 item 2's three-branch rule; admission deferred per D6 |
| `off_spec_box_sizing_min_max_sizes_clamp_in_content_box_terms` (`style/unsupported_properties.rs`) | divergent | CSS UI 4 §3.1: `box-sizing` applies to `min-width`/`max-width`/`min-height`/`max-height` — under `border-box`, min/max constrain the border box | min/max clamps interpret the specified size in content-box terms regardless of `box-sizing` (pinned values in the proof) | the sizing clamps (`flow.rs`, named in the proof header) read the specified min/max directly; interpreting them per `box-sizing` is a named admission, deferred per D6 |
| Robustness safety net (`robustness.rs` — 8 pins over adversarial inputs: empty/whitespace, block-structure abuse, comments, 10k-rule/200-declaration/64-deep-selector volume, extreme values, typed-writer abuse, encoding, non-ASCII importance-marker tail; plus 8 `robustness_*` pins in `style/unsupported_values.rs` covering numeric edges and the zero-font-size family) | robustness | CSS 2.1 §4.1.7 and css-syntax-3 error handling; AGENTS.md "Panic and boundary policy" | every adversarial input returns a typed `Err` or a counted drop — never a panic (verified across both files) | the typed-error boundary and closed grammars; crashes land as boundary-hardening fixes per PLAN.md's B12 rule (two landed: font-size: 0, caret path) |
| Unit & sizing-keyword gates (`unsupported_math_functions_drop`, `unsupported_viewport_units_drop`, `unsupported_font_relative_units_drop`, `unsupported_intrinsic_size_keywords_drop`, `unsupported_css_wide_keywords_drop`, `padding_auto_is_rejected` in `style/unsupported_values.rs`) | unimplemented | CSS Values 4 §math/§viewport-relative/§font-relative; CSS Sizing 3 intrinsic size keywords; CSS Cascade 5 CSS-wide keywords beyond `unset`; `auto` invalid for padding/border/font-size | each name/grammar form drops + counts, computed values stay at group initials, no crash. The `unsupported_percentage_lengths_drop` and `unsupported_auto_lengths_drop` gates left this row in PR 1 item 7 — percentages and the admitted `auto` keywords are pinned on-spec by `percentage_lengths_resolve_against_their_bases` and `auto_lengths_are_admitted_and_resolve` | the closed `DeclGrammar` table + `AUTO_ADMISSIBLE` set in `src/domains/style/`; admission per the D6 policy |
| `unsupported_line_width_percentages_drop` (`style/unsupported_values.rs`) | unimplemented (spec-correct drop) | CSS Backgrounds 3 §4.3 `border-*-width` and CSS UI 4 §4.1 `outline-width` take `<line-width>` (a `<length>` only, no percentages); CSS UI 4 §4.3 `outline-offset` takes `<length>` (no percentages) | `border-top-width: 10%` / `outline-width: 10%` / `outline-offset: 10%` each drop + count and the computed field stays at its 0.0 group initial. The drop IS spec-correct for these families — the row is classed unimplemented only in that the percentage form stays unadmitted, not because the drop diverges from spec | the `LineWidth`/`SignedLineWidth` grammars (`presentation_property.rs`) admit no percentage arm; the previous `NonNegativeLength`/`Length` assignment admitted `<f32>%`, which the f32 resolver (`resolve_length`, `project.rs`) could not resolve and silently left at the initial — a silent no-op closed by shutting the grammar (reject-and-report) |
| `unsupported_border_radius_percentage_drops` (`style/unsupported_values.rs`) | unimplemented | CSS Backgrounds 3 §5.1 `border-*-radius` takes `<length-percentage>` (elliptical per-corner horizontal/vertical resolution) | `border-radius: 50%` / `border-top-left-radius: 50%` drop + count and the radius stays `[0.0; 4]`; the circular-corner model (`radius: [f32; 4]`, one radius per corner) cannot represent the elliptical resolution CSS requires (50% on a non-square box computes different horizontal and vertical radii per corner), so the percentage is rejected at admission exactly like `gap` percentages | the four radius longhands use the `LineWidth` grammar (no percentages); admission gated on a corner-pair (elliptical) model per the D6 policy |
| Flex item auto margins (`flex_item_auto_margin_computes_to_zero_for_now` in `style/unsupported_values.rs`; `column_flex_block_item_cross_auto_margins_absorb_free_space` in `layout/flex_layout.rs`) | unimplemented | CSS Flexbox 1 §8.1: a flex item's `margin: auto` absorbs free space in its axis (both axes) | main-axis auto margins compute to 0 (the flex placement context gives the item its own resolved content size, so the block §10.3.3 free-space solver sees zero); a block-level item in a COLUMN container gets cross-axis auto-margin absorption through the block solver — accidental, §8.1-aligned for the pinned case; full §8.1 semantics (auto margins vs align-self, main-axis absorption) await the named D6 admission | deferred in PR 1 item 7 (S5 gate); named in `main_axis_start_margin` (`src/domains/layout/flex.rs`) |
| `off_spec_anonymous_inline_runs_are_not_merged` (`layout/flex_layout.rs`) | divergent | CSS Flexbox 1 §4: a contiguous run of inline-level children wraps into ONE anonymous flex item; whitespace-only runs are discarded | each DOM child becomes its own flex item: two adjacent text nodes (and a span + adjacent text) get a column-gap between them instead of merging into one item; a whitespace-only text node between blocks is painted as an item and adds a gap (spec: discarded, one gap between the blocks) | `collect_measured_flex_items_in_dom_order` (`flex.rs:406`) assigns `FlexItemSource::BlockItem` per child (`flex.rs:421`); `FlexItemSource::AnonymousInlineGroup` (`flex_item_source.rs:16`) is dead — fix belongs to the engine-mapped anonymous-item model (PR 3 mapping doc's named input) |
| `off_spec_abspos_and_display_none_children_occupy_flex_slots` (`layout/flex_layout.rs`) | divergent | CSS Flexbox 1 §4.1/§9.2 + CSS 2.1 §9.2: an absolutely-positioned child is not a flex item; `display: none` generates no box | an abs-pos or `display: none` middle child between two items adds a gap adjacency, pushing the trailing item to 50px (spec: 40px, one gap); an abs-pos-only child flattens to 0 width at its static position (spec: laid out at its static position with its used size) | `measure_tree_walker_node_without_layout_write` (`flex.rs:541`) measures abs-pos/none children to `FlowSize::default()`, but `collect_measured_flex_items_in_dom_order` (`flex.rs:406`) still collects every DOM child as a `FlexItemSource::BlockItem`, so the zero-size slot still occupies a line position and adds a gap adjacency |
| `off_spec_inflexible_items_skip_hypothetical_clamps` (`layout/flex_layout.rs`) | divergent | CSS Flexbox 1 §9.7 step 3: an inflexible item freezes at its hypothetical main size (clamp(base, min, max) floored by the automatic minimum) | inflexible items keep their unclamped flex base at the flex level: a 10px-base/20px-min-width item paints 20px wide but pushes the next item to 10px (spec: 20px); a 50px-base/20px-max-width item paints 20px but pushes the next to 50px (spec: 20px); the content-based automatic minimum is skipped for a visible inflexible item (base 10px stays 10px) | `initial_flex_line_item_resolution_states` (`flex.rs:2082`) seeds `resolved_main_size_px = flex_base_size_px`; the all-frozen early return (`flex.rs:2040`) exits before `freeze_flex_line_main_size_violations` (`flex.rs:2185`) applies the clamps |
| `off_spec_hypothetical_main_size_not_floored_by_auto_min` (`layout/flex_layout.rs`) | divergent | CSS Flexbox 1 §4.5/§9.2: the hypothetical main size is the flex base clamped by min/max and floored by the automatic minimum; line breaking consumes the floored size | a visible item with a small explicit base keeps its base as the hypothetical: a wrapping container keeps it and its sibling on one line (spec: the ~140.5px automatic minimum breaks the item onto its own line); an inflexible visible item stays at its base (spec: floored at the content-based minimum) | `clamp_main_axis_size` (`flex.rs:1473`) clamps by min/max only; `hypothetical_main_size_px` (`flex.rs:487`) never floors by `automatic_min_main_size_px` — the §9.2 line-break side of the C4 root cause |
| `off_spec_flex_container_ignores_min_max_main_cross_sizes` (`layout/flex_layout.rs`) | divergent | CSS Flexbox 1 §9.2 + CSS 2.1 §10.7: the container's main/cross size used for line breaking, justify, and align distribution is the USED (min/max-clamped) size | the container paints its clamped height but the vertical axis reads the raw authored height: a column container with min-height 100/height 50 distributes only 30px of free space (second item at 40px, spec 90px); max-height 50/height 100 pushes the item to 90px (spec 40px); a column-wrap min-height breaks each 40px item into its own column (spec: two per column); a row min-height 100/height 50 stretches the item to 50px (spec 100px) | `entered_container_main_size` (`flex.rs:1506`) and `entered_container_cross_size` (`flex.rs:1513`) read the raw `entered.style.height_px` for the vertical axis, bypassing the block-layout min/max clamp |
| Box-shorthand gates (`unsupported_border_and_border_width_shorthands_drop`, `unsupported_inset_shorthand_drops`, `unsupported_outline_shorthand_drops` in `style/unsupported_shorthands.rs`) | unimplemented | CSS Backgrounds 3 §4.2/§4.3 `border`/`border-width` shorthands; CSS Position 3 §6 `inset`; CSS UI 4 §4.3 `outline` | each shorthand name drops + counts, the corresponding longhand group fields stay at their group initial, no crash | the names are absent from the `property_shape` name table in `src/domains/style/decl_parse.rs`; admission per the D6 policy |
| Other-shorthand & overflow-axis gates (`unsupported_font_shorthand_drops`, `unsupported_background_shorthand_drops`, `unsupported_all_shorthand_drops`, `unsupported_overflow_axes_and_two_token_drop` in `style/unsupported_shorthands.rs`) | unimplemented | CSS Fonts 4 §font; CSS Backgrounds 3 §background; CSS Cascade 5 §all; CSS Overflow 3 §overflow-x/§overflow-y | each shorthand name (and the two-token `overflow: scroll auto` value form) drops + counts, the corresponding longhand group fields stay at their group initial, no crash. The two-token `overflow` form drops as ONE declaration (the admitted keyword grammar matches the whole value string). CONTROL: the single-token admitted `overflow: scroll` still applies | the names are absent from the `property_shape` name table in `src/domains/style/decl_parse.rs`; the two-token `overflow` form fails the closed `Keywords` grammar; admission per the D6 policy |
| Layout-impact property gates (`unsupported_z_index_drops_and_dom_order_paints`, `unsupported_position_sticky_drops_to_static`, `unsupported_display_values_drop_to_initial`, `unsupported_replaced_size_hints_drop`, `unsupported_float_and_clear_drop`, `unsupported_layout_hint_properties_drop` in `style/unsupported_properties.rs`) | unimplemented | CSS 2.1 Appendix E stacking order & §9.5 floats; CSS Position 3 §sticky; CSS Display 3 §2 display values; CSS Sizing 4/Images 3 replaced-element hints; containment/fragmentation/scroll-snap/overscroll/pointer-events/will-change | each name/value drops + counts and the defined no-effect holds (DOM paint order, static/block fallback, unchanged replaced-element and block-flow sizing). The `box-sizing` gate left this row in PR 1 item 3 — the property is admitted and its relationship is pinned on-spec by `box_sizing_border_box_subtracts_insets` | names absent from the `property_shape` table (`decl_parse.rs`); `position: sticky` and `display: grid`/… drop at the value level in `presentation_property.rs`; admission per the D6 policy |
| Text/effects property-tail gates (`unsupported_text_decoration_family_drops`, `unsupported_box_shadow_drops`, `unsupported_property_tail_drops`, `unsupported_value_functions_drop`, `unsupported_content_and_counter_properties_drop` in `style/unsupported_properties.rs`) | unimplemented | CSS Text Decoration 4 §2; CSS Backgrounds 3 §7.1; CSS Text 4; CSS Overflow 3/4; CSS Inline Layout 3; CSS Writing Modes 4; CSS UI 4; CSS Lists 3; Filter Effects 1/2; Compositing 1; CSS Masking 1; CSS Images 3/4; CSS 2.1 §17/§13.3; CSS Multi-column 1; CSS Fragmentation 3; CSS Animations 1; CSS Values 4/5; CSS Content 3 | every tail-table name drops + counts at name lookup, inert and crash-free; the value-function family drops at the value grammar on an ADMITTED property (`width`), proven by the `width: 10px` CONTROL which applies; `text-decoration`/`box-shadow` carry a no-effect control — the staged op count equals the unstyled baseline, no decoration/shadow op appears | names absent from the `property_shape` table (`decl_parse.rs`); the value-function rows fail the closed `DeclGrammar`; admission per the D6 policy |
| Color-grammar gates (`unsupported_short_hex_colors_drop`, `unsupported_named_colors_drop`, `unsupported_current_color_drops`, and the slash-alpha rows in `modern_rgb_syntax_is_partially_admitted` in `style/unsupported_colors.rs`) | unimplemented | CSS Color 4 §4.2 3/4-digit hex; §6.1 full named-color set; §`currentColor`; §4.2 `rgb()` slash-alpha | each form drops + counts and the computed `color` stays at the near-white group initial `[240/255; 3, 1.0]`; the admitted named set is exactly `white`/`black`/`red`/`green`/`blue` + `transparent` (pinned by the `color: blue` CONTROL row) | `parse_css_color_rgba` (`dom_attr_parse.rs:139-154`) — hex3/4 and non-five names return `Err`; `rgb_function_tokens` (`dom_attr_parse.rs:202-207`) has no slash separator, so slash-alpha is a token-count mismatch; admission per the D6 policy |
| Selector & at-rule gates (`unsupported_combinator_selectors_fail_the_load`, `unsupported_attribute_selectors_fail_the_load`, `unsupported_pseudo_elements_fail_the_load`, `unsupported_pseudo_classes_fail_the_load`, `unsupported_selector_lists_fail_the_load`, `unsupported_at_rules_fail_the_load`, `unsupported_import_and_charset_fail_the_load`, `malformed_block_structure_fails_the_load` in `style/unsupported_selectors.rs`) | unimplemented | CSS Selectors 4 §5 grouping/§6 attribute selectors/§7 pseudo-classes/§13 combinators + pseudo-elements; CSS Conditional 3 `@media`/`@supports`; CSS Animations 1 `@keyframes`; CSS Fonts 4 `@font-face`; CSS Cascade 5 `@import`; CSS Syntax 3 `@charset` + malformed-statement recovery | each unsupported selector form, at-rule, stray `}`, and missing closing `}` fails the whole load with a typed `Err` — `InvalidSelector` for every selector/at-rule/stray-`}` row and `@import`/`@charset`; `InvalidAttributeValue` for a missing closing brace. The "stray text before a rule" premise (`garbage .a { }`) does NOT fail: `garbage` is a valid tag identifier, so it loads as a valid `garbage .a` descendant selector (honesty correction) | the closed selector grammar in `src/domains/style/mod.rs` (`parse_css_blocks`, `parse_selector`, `contains_unsupported_selector_syntax`, `split_selector_pseudo_state`); admission per the D6 policy |
| Cascade-tier gates (`unsupported_layer_at_rule_fails_the_load`, `unsupported_custom_properties_and_var_drop`, `unsupported_animation_declarations_drop` in `style/cascade_tiers.rs`) | unimplemented | CSS Cascade 5 §6.4.2 `@layer` (F stage 4); CSS Custom Properties 1 §2/§3 `--*`/`var()`; CSS Animations 1 §3 `animation` (F stage 3) | `@layer` fails the whole load with `Err(DomError::InvalidSelector)` (the at-rule prelude is treated as a selector, the nested `}` becomes a stray `}`); `--brand` drops at name lookup and `var(--brand)` drops at the value grammar (two drops, no apply); `animation` drops at name lookup. The two `!important` gates left this row in PR 1 item 4 — importance is admitted and pinned on-spec | the `Precedence` record (`decl_value.rs`) carries no layer/animation tier and the `property_shape` name table (`decl_parse.rs`) lacks `--*`/`animation`; admission per the D6 policy / F stages |
| `ua_sheet_workaround_low_priority_origin_loses_to_author_and_beats_behavior` (`style/cascade_tiers.rs`) | on-spec | CSS Cascade 5 §6.4.1 origin order; the documented "model a UA sheet as a low-priority origin" path | a `StyleRuleOrigin::new("ua", -100)` sheet loses to a `priority: 0` author sheet at equal specificity (`width` 10px vs 20px → 20px), and beats the behavior sheet (which folds at `i32::MIN`) — a hovered input's `#445566` from the `-100` sheet wins over the `with_default_field_state_styles` hover `#191d26` | `behavior_precedence` (`winners_fold.rs:263-270`) folds the behavior sheet at `i32::MIN`, below every origin — pinned so it cannot silently break |
| `off_spec_font_family_computed_but_unconsumed` + `off_spec_font_weight_and_style_computed_but_unconsumed` (`style/admitted_gaps.rs`) | divergent (step-5 follower) | CSS Fonts 4 §3.1/§3.2/§3.3 select the family/weight/style used for glyph rendering | the computed family (`Some("Source Han Sans")`), weight (`700`), and style (`Italic`) all resolve but text intrinsic widths are identical to the default — rendering stays regular/upright | `TextFontFamilyId::UiSans` one-variant enum (`text_layout/mod.rs:36-39`), hardcoded in `TextLayoutRequest::for_text` (`text_layout_request.rs:33-48`) |
| `off_spec_range_authored_zero_radius_falls_back_to_pill` (`style/admitted_gaps.rs`) | divergent (documented) | CSS Backgrounds 3 §5.1: an authored `border-radius: 0px` wins over a control default | the range track/thumb stage the pill default (`track_height/2`, `thumb_size/2`), not `[0.0; 4]` | `has_authored_border_radius` (`element_behaviors/paint.rs:386`) reads `any(|r| r > 0.0)`, flattening authored presence to value; fix belongs to the behavior-sheet tier (F) |
| `off_spec_break_spaces_approximated_as_pre_wrap` (`style/admitted_gaps.rs`) | divergent | CSS Text 4 `white-space`: `break-spaces` preserves every trailing space | `break-spaces` folds into `pre-wrap`'s `preserve_space_run_advances` transform (repeated spaces NBSP-substituted, a lone trailing space not preserved) | `text_layout_input_for_white_space` (`types/white_space.rs:58-69`) maps `BreakSpaces` to the same `preserve_space_run_advances` as `Pre`/`PreWrap` |
| `unsupported_font_style_oblique_drops` (`style/admitted_gaps.rs`) | unimplemented | CSS Fonts 4 §3.3: `font-style` admits `oblique` | `font-style: oblique` drops + counts; the computed style stays `Keyword::Normal` | the closed `FONT_STYLE_KEYWORDS` set (`presentation_property.rs:213`) is exactly normal/italic; admission per the D6 policy |
| `pointer_events_content_channel` (`style/admitted_gaps.rs`) | on-spec for `auto`/`none`; unimplemented for wider values + the stylesheet channel | pointer-events as behavior input (`hit_test.rs:331-337`) | `"none"` disables a subtree and `"auto"` re-enables (on-spec); wider values (`visiblePainted`) are stored-but-ignored (inherit); `pointer-events` in a stylesheet drops + counts with no hit-test effect | the content-side `Custom("pointer-events")` channel is deliberate (behavior input, not cascaded style); the presentation-table absence drops the stylesheet channel |
| `initial_color_is_near_white_by_design` (`style/admitted_gaps.rs`) | deliberate | CSS Color 4 §12 `color` initial `canvastext` (black in light contexts) | near-white `[240/255;3, 1.0]` (dark-theme design-system default); spec-correct: `canvastext` | `DEFAULT_TEXT_COLOR_RGBA` (`computed_color_group.rs:23`); pinned permanently, re-reviewed if the design system changes |
| `off_spec_line_height_normal_resolves_to_one_times_font_size` (`style/admitted_gaps.rs`) | divergent | CSS 2.1 §10.8 `line-height: normal` is UA-dependent (~1.2 in desktop browsers) | `1.0 × font-size`; spec-correct: font-metrics-derived normal (Blink `FontMetrics::lineSpacing`, Gecko hhea ascent+descent+lineGap) | `effective_line_height_px` (`text_layout/mod.rs:714-722`); moves on-spec when a consumer need is named |
| `initial_border_widths_are_zero_by_design` (`style/admitted_gaps.rs`) | deliberate | CSS Backgrounds 3 §4.2 `border-width` initial `medium` (invisible because `border-style` initial is `none`) | `[0.0; 4]` (width-gates-border model); spec-correct: `medium × style:none` | `ComputedBoxGroup::initial()` (`computed_box_group.rs`) — the width-gates-border model; the B11 `border-style` admission would revisit |
| `percentage_authored_transitions_step_at_completion_for_now` (`pipeline/transitions.rs`) | unimplemented | CSS Transitions 1 interpolates length-percentage endpoints against the containing block | `width: 50%` → `width: 100px` on a 200px container steps: mid-flight presents the from-shape's used value (100.0) and the target is taken only at completion — no interpolation | the scheduler's `interpolate_value` (`src/types/dom_transition.rs`) only lerps `Px → Px`; `DeclLength::Percent` carries no used basis at interpolation time. Deferred per D6: schedule-time basis resolution — resolve percentage endpoints against the containing-block width at schedule time and lerp the px pair |
| `flex_basis_percentage_drops` (`style/unsupported_values.rs`) | unimplemented | CSS Flexbox 1 §7.3.3 `flex-basis` takes `<content> | <'width'>`, where `<'width'>` is `<length-percentage>`, so `flex-basis: 50%` is valid | `flex-basis: 50%` drops + counts and the flex basis stays at its `Auto` initial, no crash | the closed `FlexBasis` grammar admits only `auto | content | 0 | <px> | <em>` (`parse_flex_basis_length`, `decl_parse.rs:606-612`); admission per the D6 policy |
| `relative_top_percentage_defers_to_offset_time` (`style/unsupported_values.rs`) | unimplemented | CSS 2.1 §9.4.2/§9.4.3 relative `top`/`bottom` percentages resolve against the laid-out containing-block height | `top: 50%` on a relative element under an auto-height containing block defers to no offset (the box stays at its normal-flow position) | `node_style` resolves `top`/`bottom` against the node_style-time height basis (`None` under an auto-height cb); offset-time resolution against the laid-out containing block is a follow-up |

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
- Spec-surface enumeration (Run A): table-driven divergence-probe families
  pinning actual flex behavior against CSS Flexbox 1 cells, registering
  divergent outcomes in the registry. Landing so far:
  `off_spec_anonymous_inline_runs_are_not_merged` (§4 anonymous items),
  `off_spec_abspos_and_display_none_children_occupy_flex_slots` (§4.1/§9.2),
  `off_spec_inflexible_items_skip_hypothetical_clamps` (§9.7 step 3),
  `off_spec_hypothetical_main_size_not_floored_by_auto_min` (§4.5/§9.2),
  `off_spec_flex_container_ignores_min_max_main_cross_sizes` (§9.2/§10.7);
  one suspected divergence resolved on-spec (no registry row):
  `cross_axis_min_max_clamps_apply_via_block_measure_and_relayout` (§8.3/§9.2/§9.4,
  F6 false positive).

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
