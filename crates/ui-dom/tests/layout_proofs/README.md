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

Popup coverage map:

- `popup_menus.rs`: synthetic fixed-row popup clamp math.
- `popup_text_rows.rs`: browser-style text-row popup retained layout for below-max,
  over-max, live growth, `absolute` positioning, wrapped rows, partial-shrink,
  and empty-shrink behavior.
- `text_auto_size.rs`: row-level text auto sizing and wrapped-text growth.

Flex coverage map:

- `flex_layout.rs`: spec-backed browser flex proofs grouped by staged solver
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

- `block_inline.rs`: block stacking, inline line building, block-after-inline
  line breaks, content-box origins under padding, inline shrink-wrap, inline
  sibling participation, and inline-block text chip shrink-wrap.
- `content_height.rs`: auto-height accumulation, empty collapse, child removal,
  and flow participation rules.
- `fixed_layers.rs`: the same block/inline invariants when the containing block
  comes from a fixed-position layer root.
- `normal_flow.rs`: positioning-scheme participation and scroll-content-height
  invariants.
- `white_space.rs`: CSS Text-backed `white-space` collapse, preservation, and
  wrapping behavior.
- `stylesheet_lifecycle.rs`: stylesheet load/attach/append/import orderings
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
