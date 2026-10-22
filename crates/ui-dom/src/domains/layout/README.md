# layout

DOM-style layout for `ui-dom`: the single `TreeWalker` boundary that measures
intrinsic sizes, resolves block flow, flex, and grid containers, and places
boxes — feeding paint and hit-testing their retained geometry.

## Purpose and scope

This module is the one layout engine in `ui-dom`. It owns the traversal
orchestration, the intrinsic measurement record, the block/flow solver, the
flex solver, and the grid solver. It does not own style resolution (the
computed groups come from `domains/style`), text shaping (`domains/text_layout`
measures text intrinsics), or paint (which consumes the resolved boxes and the
text-run/icon/render ops the layout pass stages).

## Ownership boundaries

- `mod.rs` owns the public layout seam and the solver/record re-exports; it
  must stay small and boundary-focused and never grow a second walker.
- `walker.rs` owns the single shared DFS traversal: dirty-tree queries, the
  relative-offset bridge, and pass entry points. Property meaning lives in
  `style.rs`, never in traversal helpers.
- `style.rs` owns the computed-group → layout input interpretation (`NodeStyle`,
  display/position/overflow mapping, content-size property style).
- `flow.rs` owns the block/flow solver: entering/entering-node layout,
  content-box arithmetic, margin collapse, auto content sizing.
- `flex.rs` + the `flex_*` sibling files own the flex solver and its lanes
  (item/order/line/resolution) and run-formation helpers.
- `grid.rs` + the `grid_*` sibling files own the grid solver, its track/item
  lanes, and the `CellMap` occupancy model.
- `measurement_record.rs` owns the per-node `MeasurementRecord` and its
  equality-gated write path; `measurement_flags.rs` owns the flag set.
- `dom_layout_resolver.rs` / `identity_dom_layout_resolver.rs` own the resolver
  seam the walker consumes.

## The fold doctrine

Layout is two folds per update, gated by dirty bits (the 2026-10-19 layout-pass
doctrine):

1. **One bottom-up intrinsic fold** — every dirty node writes its retained
   `MeasurementRecord` once per dirty pass (block world at `exit_node`, flex
   subtrees at the measure fold), carrying subtree-computed content sizes the
   solvers read in the same pass. Record writes are equality-gated and a pass
   OUTPUT — they never mark style/layout/paint dirty (the phantom-dirty lesson).
2. **One top-down resolution/placement fold** — container used sizes flow down,
   subtrees whose cross size depends on the resolved main size are flagged in
   the up fold and re-folded once at the resolved width (the engines'
   `CanMainSizeInfluenceCrossSize` semantics).

A reflow must not allocate: records are node-inline `Copy` values; solver state
lives in `RootDom`-owned scratch lanes at high-water mark; neighbor access only
through arena links and the visitor's carried context. No free recursion outside
the two folds.

## The three solvers

- **Flow (block)** — `flow.rs`: the block formatting context. Content-box
  sizing, the three-branch margin-collapse rule (PR 1 item 2), auto content
  height/width, and the flow-size epilogue.
- **Flex** — `flex.rs` + lanes: the engines' flex semantics on doctrine-
  conforming data. Item formation (anonymous runs, abs-pos/`display:none`
  exclusion, blockification), the auto-min floor by fold, §9.7 resolution in
  the engines' exact shape (seed at the floored hypothetical, one-class freeze
  by signed total), cross-after-main, and §8.1 auto-margin absorption. The lane
  discipline (item/order/line/resolution) replaced the former per-container
  `Vec`/`HashMap` re-joins.
- **Grid** — `grid.rs` + lanes + `CellMap`: track sizing in the engines'
  corroborated phases, row-major auto-placement over the order lane, spans and
  named lines, `grid-template-areas`, auto-fill/fit, and the placement
  longhands. Percent tracks, `fr` expansion, and the capacity gates are
  admitted per the grid mapping (`GRID_MAPPING.md`).

## Invariants, rules, and decisions

- One `TreeWalker`; no second walker and no wrapper-specific subtree recursion.
- Layout is dirty-gated; work happens on visit (visitor, not batch).
- Zero allocation in steady state: records inline, scratch lanes at high-water
  mark, no per-element/per-container collection in the solve.
- The layer-spillover decision (PLAN.md): layers clip; spillover is a layering
  decision, never surface expansion.
- Percentage resolution reads the containing-block basis at the point of use
  (never an `f32` basis); flex/grid bases come from the down-fold context.
