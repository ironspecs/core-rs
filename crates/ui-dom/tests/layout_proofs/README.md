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
| Layout-impact property-table absences (`z-index`, `position: sticky`, `display: grid`/`inline-grid`/`table`/`inline-table`/`flow-root`/`contents`/`ruby`, `box-sizing`, `aspect-ratio`/`object-fit`/`object-position`, `float`/`clear`, `contain`/`content-visibility`/`break-inside`/`resize`/`scroll-behavior`/`scroll-snap-type`/`overscroll-behavior`/`touch-action`/`will-change`; CSS 2.1 Appendix E & §9.5, CSS Position 3 §sticky, CSS Display 3 §2, CSS UI 4 §box-sizing, CSS Sizing 4/Images 3 replaced hints, containment/fragmentation/scroll-snap/overscroll/pointer-events/will-change) | `style/unsupported_properties.rs` | partial; all rows unimplemented (D6 admission-gated drop-and-count + per-family no-effect pins) |
| Color grammar (`#abc`/`#abcd` hex shorthands, wider named colors, `currentColor`, modern `rgb()` space/slash forms; CSS Color 4 §4.2 hex + `rgb()`, §6.1 named colors, §`currentColor`) | `style/unsupported_colors.rs` | partial; classes mixed (space/comma/percent `rgb()` forms on-spec; hex3/4 + wider named colors + `currentColor` + slash-alpha unimplemented) |
| Admitted grammars whose computed values are ignored/approximated/quirky downstream (CSS Fonts 4 §3.1–3.3; CSS Text 3 §7.1 `text-align`; CSS Text 4 `white-space`; CSS Transitions 1 §2; CSS Backgrounds 3 §5.1; CSS Writing Modes 4 `direction`; pointer-events content vs stylesheet channel; CSS Color 4 §12 `color` initial; CSS 2.1 §10.8 `line-height: normal`; CSS Backgrounds 3 §4.2 `border-width` initial) | `style/admitted_gaps.rs` | partial; mixed classes (font-family/weight/style + transform transitions + range zero-radius + break-spaces + `line-height: normal` → 1.0× divergent; `text-align: justify`/`dir`-vs-`direction`/pointer-events auto-none on-spec; `font-style: oblique` + pointer-events wider values/stylesheet channel unimplemented; near-white initial `color` + zero initial border widths deliberate) |
| Selector grammar boundary & at-rules (CSS Selectors 4 §5 grouping, §6 attribute selectors, §7 structural/state pseudo-classes + `:not()`, §13 combinators + pseudo-elements; CSS Conditional 3 `@media`/`@supports`; CSS Animations 1 `@keyframes`; CSS Fonts 4 `@font-face`; CSS Cascade 5 `@import`; CSS Syntax 3 `@charset` + malformed-statement recovery) | `style/unsupported_selectors.rs` | partial; all rows unimplemented (D6 admission-gated load-error gates + block-structure failures); the F1 failed-load-atomicity row is on-spec |

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
| Layout-impact property gates (`unsupported_z_index_drops_and_dom_order_paints`, `unsupported_position_sticky_drops_to_static`, `unsupported_display_values_drop_to_initial`, `unsupported_box_sizing_drops_and_border_box_sizing_holds`, `unsupported_replaced_size_hints_drop`, `unsupported_float_and_clear_drop`, `unsupported_layout_hint_properties_drop` in `style/unsupported_properties.rs`) | unimplemented | CSS 2.1 Appendix E stacking order & §9.5 floats; CSS Position 3 §sticky; CSS Display 3 §2 display values; CSS UI 4 §box-sizing; CSS Sizing 4/Images 3 replaced-element hints; containment/fragmentation/scroll-snap/overscroll/pointer-events/will-change | each name/value drops + counts and the defined no-effect holds (DOM paint order, static/block fallback, unchanged replaced-element and block-flow sizing). The crate's single sizing model is content-box (`border_box_width = content + padding + border`); the "border-box" label in PLAN.md B6 was a doc error — `box-sizing: content-box` is a no-op naming the active model, and a `width: 100px` + `padding: 10px 10px` box has a 120px border box | names absent from the `property_shape` table (`decl_parse.rs`); `position: sticky` and `display: grid`/… drop at the value level in `presentation_property.rs`; admission per the D6 policy |
| Color-grammar gates (`unsupported_short_hex_colors_drop`, `unsupported_named_colors_drop`, `unsupported_current_color_drops`, and the slash-alpha rows in `modern_rgb_syntax_is_partially_admitted` in `style/unsupported_colors.rs`) | unimplemented | CSS Color 4 §4.2 3/4-digit hex; §6.1 full named-color set; §`currentColor`; §4.2 `rgb()` slash-alpha | each form drops + counts and the computed `color` stays at the near-white group initial `[240/255; 3, 1.0]`; the admitted named set is exactly `white`/`black`/`red`/`green`/`blue` + `transparent` (pinned by the `color: blue` CONTROL row) | `parse_css_color_rgba` (`dom_attr_parse.rs:136-151`) — hex3/4 and non-five names return `Err`; `rgb_function_tokens` (`dom_attr_parse.rs:199-204`) has no slash separator, so slash-alpha is a token-count mismatch; admission per the D6 policy |
| Selector & at-rule gates (`unsupported_combinator_selectors_fail_the_load`, `unsupported_attribute_selectors_fail_the_load`, `unsupported_pseudo_elements_fail_the_load`, `unsupported_pseudo_classes_fail_the_load`, `unsupported_selector_lists_fail_the_load`, `unsupported_at_rules_fail_the_load`, `unsupported_import_and_charset_fail_the_load`, `malformed_block_structure_fails_the_load` in `style/unsupported_selectors.rs`) | unimplemented | CSS Selectors 4 §5 grouping/§6 attribute selectors/§7 pseudo-classes/§13 combinators + pseudo-elements; CSS Conditional 3 `@media`/`@supports`; CSS Animations 1 `@keyframes`; CSS Fonts 4 `@font-face`; CSS Cascade 5 `@import`; CSS Syntax 3 `@charset` + malformed-statement recovery | each unsupported selector form, at-rule, stray `}`, and missing closing `}` fails the whole load with a typed `Err` — `InvalidSelector` for every selector/at-rule/stray-`}` row and `@import`/`@charset`; `InvalidAttributeValue` for a missing closing brace. The "stray text before a rule" premise (`garbage .a { }`) does NOT fail: `garbage` is a valid tag identifier, so it loads as a valid `garbage .a` descendant selector (honesty correction) | the closed selector grammar in `src/domains/style/mod.rs` (`parse_css_blocks`, `parse_selector`, `contains_unsupported_selector_syntax`, `split_selector_pseudo_state`); admission per the D6 policy |
| `off_spec_font_family_computed_but_unconsumed` + `off_spec_font_weight_and_style_computed_but_unconsumed` (`style/admitted_gaps.rs`) | divergent (step-5 follower) | CSS Fonts 4 §3.1/§3.2/§3.3 select the family/weight/style used for glyph rendering | the computed family (`Some("serif")`), weight (`700`), and style (`Italic`) all resolve but text intrinsic widths are identical to the default — rendering stays regular/upright | `TextFontFamilyId::UiSans` one-variant enum (`text_layout/mod.rs:36-39`), hardcoded in `TextLayoutRequest::for_text` (`text_layout_request.rs:33-48`) |
| `off_spec_transform_transitions_do_not_schedule` (`style/admitted_gaps.rs`) | divergent (documented in MIGRATION.md) | CSS Transitions 1 §2: `transition: transform` interpolates a `translateX` change | the target presents immediately at the next update; no transition is scheduled and no mid-flight value exists | `decl_value_for_groups` (`root_dom.rs:4695-4703`) returns `None` for `PresentationProperty::Transform` — the interned-text `DeclValue` cannot interpolate; the typed `[f32; 2]` pair is the admission shape |
| `off_spec_range_authored_zero_radius_falls_back_to_pill` (`style/admitted_gaps.rs`) | divergent (documented) | CSS Backgrounds 3 §5.1: an authored `border-radius: 0px` wins over a control default | the range track/thumb stage the pill default (`track_height/2`, `thumb_size/2`), not `[0.0; 4]` | `has_authored_border_radius` (`element_behaviors/paint.rs:386`) reads `any(|r| r > 0.0)`, flattening authored presence to value; fix belongs to the behavior-sheet tier (F) |
| `white_space_pre_line_and_break_spaces_consumption` (`style/admitted_gaps.rs`) | `pre-line` on-spec; `break-spaces` divergent | CSS Text 4 `white-space`: `pre-line` collapses spaces/tabs and preserves newlines; `break-spaces` preserves every trailing space | `pre-line` collapses spaces and preserves segment breaks (on-spec); `break-spaces` folds into `pre-wrap`'s `preserve_space_run_advances` transform (repeated spaces NBSP-substituted, a lone trailing space not preserved) | `text_layout_input_for_white_space` (`types/white_space.rs:58-69`) maps `BreakSpaces` to the same `preserve_space_run_advances` as `Pre`/`PreWrap` |
| `unsupported_font_style_oblique_drops` (`style/admitted_gaps.rs`) | unimplemented | CSS Fonts 4 §3.3: `font-style` admits `oblique` | `font-style: oblique` drops + counts; the computed style stays `Keyword::Normal` | the closed `FONT_STYLE_KEYWORDS` set (`presentation_property.rs:213`) is exactly normal/italic; admission per the D6 policy |
| `pointer_events_content_channel` (`style/admitted_gaps.rs`) | on-spec for `auto`/`none`; unimplemented for wider values + the stylesheet channel | pointer-events as behavior input (`hit_test.rs:331-337`) | `"none"` disables a subtree and `"auto"` re-enables (on-spec); wider values (`visiblePainted`) are stored-but-ignored (inherit); `pointer-events` in a stylesheet drops + counts with no hit-test effect | the content-side `Custom("pointer-events")` channel is deliberate (behavior input, not cascaded style); the presentation-table absence drops the stylesheet channel |
| `initial_color_is_near_white_by_design` (`style/admitted_gaps.rs`) | deliberate | CSS Color 4 §12 `color` initial `canvastext` (black in light contexts) | near-white `[240/255;3, 1.0]` (dark-theme design-system default); spec-correct: `canvastext` | `DEFAULT_TEXT_COLOR_RGBA` (`computed_color_group.rs:23`); pinned permanently, re-reviewed if the design system changes |
| `off_spec_line_height_normal_resolves_to_one_times_font_size` (`style/admitted_gaps.rs`) | divergent | CSS 2.1 §10.8 `line-height: normal` is UA-dependent (~1.2 in desktop browsers) | `1.0 × font-size`; spec-correct: font-metrics-derived normal (Blink `FontMetrics::lineSpacing`, Gecko hhea ascent+descent+lineGap) | `effective_line_height_px` (`text_layout/mod.rs:714-722`); moves on-spec when a consumer need is named |
| `initial_border_widths_are_zero_by_design` (`style/admitted_gaps.rs`) | deliberate | CSS Backgrounds 3 §4.2 `border-width` initial `medium` (invisible because `border-style` initial is `none`) | `[0.0; 4]` (width-gates-border model); spec-correct: `medium × style:none` | `ComputedBoxGroup::initial()` (`computed_box_group.rs`) — the width-gates-border model; the B11 `border-style` admission would revisit |

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
