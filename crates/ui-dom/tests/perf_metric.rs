//! Performance contract tests for the `ui-dom` retained pipeline.
//!
//! These tests pin the exact `metrics()` / `history()` / `node_metrics()`
//! shape of one warmed update per scenario. They exist so that correctness
//! fixes can prove zero performance degradation: any change that makes a
//! scenario do more pipeline work (extra style/layout/paint phases, extra
//! node-store traffic, new allocation work sites) breaks a pin and must be
//! justified in the test comment when the pin is updated.
//!
//! Contract rules:
//!
//! - A steady-state update with no DOM edits must stay root-metric silent.
//!   This is the executable form of the crate README's cheap steady-state
//!   rule.
//! - Node-store `Get`/`GetMut` counts are the traversal-work proxy; they must
//!   not grow without a correctness reason recorded beside the pin.
//! - `append_then_update` is pinned to its post style-dirty propagation fix
//!   shape: the insert path arms the layer root's style phase, so one append
//!   update pays one style recompute. The parity proof that this recompute
//!   costs exactly what the attr-triggered path already pays for the same
//!   tree lives in
//!   `insert_style_phase_deltas_match_attr_triggered_style_phase`.

#[allow(dead_code)]
#[path = "layout_proofs/support.rs"]
mod support;

use bevy::prelude::World;
use ui_dom::{
    DomNodeSlotKey, DomPointerState, DomPointerUpdate, DomTag, RootDom, RootDomMetricName,
    RootDomMetricName::*, RootDomNodesMetricName,
};

/// Builds the shared chip scenario: one `.chip` div under the document root
/// with a `:hover` rule in the theme stylesheet, attached and warmed by one
/// update. Returns `(world, root, chip)`.
fn chip_tree_setup() -> (World, RootDom, DomNodeSlotKey) {
    let (mut world, mut root) = support::attached_gpu_root_dom_default_viewport_setup();
    support::assert_load_root_stylesheet(
        &root,
        support::theme_stylesheet_origin(),
        ".chip { width: 100px; height: 40px; } .chip:hover { width: 120px; }",
    );
    let document_root = root.document_node().0;
    let chip = support::assert_append_div_to(&mut root, document_root);
    support::assert_set_attr_str(&mut root, chip, "class", "chip");
    support::assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    (world, root, chip)
}

fn pointer_at(x_px: f32, y_px: f32) -> DomPointerUpdate {
    DomPointerUpdate::Set(DomPointerState {
        x_px,
        y_px,
        primary_button_down: false,
        scroll_delta_y_px: 0.0,
    })
}

fn history_names(root: &RootDom) -> Vec<RootDomMetricName> {
    root.history().map(|entry| entry.name).collect()
}

fn assert_root_counts(root: &RootDom, expected: &[(RootDomMetricName, u64)], label: &str) {
    let snapshot = root.metrics();
    let mut expected_total = 0;
    for &(name, count) in expected {
        assert_eq!(
            snapshot.count(name),
            count,
            "{label}: {name:?} count mismatch",
        );
        expected_total += count;
    }
    let actual_total: u64 = snapshot.raw_counts().iter().sum();
    assert_eq!(
        actual_total, expected_total,
        "{label}: unexpected metric work outside the pinned set: {snapshot:?}",
    );
}

fn assert_node_counts(root: &RootDom, get: u64, get_mut: u64, insert: u64, label: &str) {
    let snapshot = root.node_metrics();
    assert_eq!(
        snapshot.count(RootDomNodesMetricName::Get),
        get,
        "{label}: node Get count mismatch",
    );
    assert_eq!(
        snapshot.count(RootDomNodesMetricName::GetMut),
        get_mut,
        "{label}: node GetMut count mismatch",
    );
    assert_eq!(
        snapshot.count(RootDomNodesMetricName::Insert),
        insert,
        "{label}: node Insert count mismatch",
    );
    let pinned_total = get + get_mut + insert;
    let actual_total: u64 = snapshot.raw_counts().iter().sum();
    assert_eq!(
        actual_total, pinned_total,
        "{label}: unexpected node-store work outside Get/GetMut/Insert: {snapshot:?}",
    );
}

#[test]
fn steady_state_update_is_root_metric_silent_after_warmup() {
    let (mut world, mut root, _chip) = chip_tree_setup();
    root.reset_metrics();
    root.reset_node_metrics();

    support::assert_root_dom_update(&mut world, &mut root, 0.016, DomPointerUpdate::Unchanged);

    assert_eq!(
        root.metrics(),
        ui_dom::RootDomMetrics::default(),
        "a clean update must not start any style/layout/paint/composite work",
    );
    assert!(
        root.history().next().is_none(),
        "a clean update must not record metric history",
    );
    // The three per-layer dirty gates read the tree to prove there is no
    // work; 22 Gets is the pinned gate-scan cost for this tree shape and
    // must not grow.
    assert_node_counts(&root, 22, 0, 0, "steady-state update");
}

#[test]
fn hover_style_recompute_metric_deltas_stay_pinned() {
    let (mut world, mut root, _chip) = chip_tree_setup();
    root.reset_metrics();
    root.reset_node_metrics();

    support::assert_root_dom_update(&mut world, &mut root, 0.016, pointer_at(10.0, 10.0));

    assert_root_counts(
        &root,
        &[
            (HoverPseudoStateSet, 1),
            (EmittedEventAllocation, 1),
            (StyleUpdated, 1),
            (LayoutUpdated, 1),
            (PaintUpdated, 1),
            (PaintPlanBuilt, 1),
            (LayerPainted, 1),
            // spine step 4: style-pass work counters; visited vs changed is
            // proposal J's evidence gate.
            (StylePassNodeVisited, 1),
            (StylePassNodeChanged, 1),
        ],
        "hover recompute",
    );
    let history = history_names(&root);
    assert!(
        metric_index(&history, HoverPseudoStateSet)
            < metric_index(&history, EmittedEventAllocation),
        "hover recompute: HoverPseudoStateSet before EmittedEventAllocation"
    );
    assert!(
        metric_index(&history, EmittedEventAllocation) < metric_index(&history, StyleUpdated),
        "hover recompute: EmittedEventAllocation before StyleUpdated"
    );
    assert!(
        metric_index(&history, StyleUpdated) < metric_index(&history, LayoutUpdated),
        "hover recompute: StyleUpdated before LayoutUpdated"
    );
    assert!(
        metric_index(&history, LayoutUpdated) < metric_index(&history, PaintUpdated),
        "hover recompute: LayoutUpdated before PaintUpdated"
    );
    assert!(
        metric_index(&history, PaintUpdated) < metric_index(&history, PaintPlanBuilt),
        "hover recompute: PaintUpdated before PaintPlanBuilt"
    );
    assert!(
        metric_index(&history, PaintPlanBuilt) < metric_index(&history, LayerPainted),
        "hover recompute: PaintPlanBuilt before LayerPainted"
    );
    // The terminal clear walk adds 7 node-store Gets on this paint frame
    // (dirt clearing moved out of the build walk into one terminal traversal).
    // computed-projection dual-write (PLAN.md spine 3.3a): 1 node access per
    // visited element for the parent font-seed ancestor walk; reclaimed when
    // the legacy write path dies in 3.3g.
    // synchronous dual-write re-projection on presentation writes (PLAN.md
    // spine 3.3 fix): the `width` write this hover recompute applies refreshes
    // the computed groups in place (+5 Get, +2 GetMut from 108/15/0);
    // reclaimed when the legacy write path dies in 3.3g.
    // spine step 3.3c (−1 Get from 113/17/0): the element rect paint reads
    // `computed_visual().radius` directly instead of `border_radii_px(root_dom,
    // key)`, removing one node-store Get per painted element.
    // spine step 3.3d (−2 Get from 112/17/0): the projection now seeds from one
    // `parent_inherited_groups` walk (replacing the two 3.3a walks
    // `parent_font_seed` + `parent_font_size_px`); the hover recompute pays two
    // re-projections (the style pass + the `width` write), one fewer Get each.
    // spine step 3.3g-1b (+7 Get, +4 GetMut from 110/17/0): the pass now
    // captures `previous_computed`, reads the resolved longhand slot, and
    // records the new baseline per visited element, and A7's group-diff
    // classification re-marks layout+paint dirty on the hover width change.
    // declaration-store style system (PLAN.md spine 3.3g-2a, −11 Get, −5
    // GetMut from 117/21/0): the legacy attr-projection channel is deleted —
    // style_mut writes the inline block and marks dirty only, and the style
    // pass writes the seven computed groups directly on the node record.
    // flex arc step 9 leading-margin-chain walk (+1 Get from 106/16/0): the
    // collapsed leading-margin chain reads the first in-flow block child of the
    // one positioned chip (a `first_child_key` lookup that returns none) — one
    // node-store read per positioned container, style-read-only, bounded by the
    // empty-run length (FLEX_STEP9_DESIGN.md D1 cost model).
    assert_node_counts(&root, 107, 16, 0, "hover recompute");
}

/// Post style-dirty propagation fix pin: one append now arms the layer
/// root's style phase, so the update pays one full style recompute for the
/// two styled chips (one tracked-attribute set per styled element) before
/// layout and paint, matching the browser pipeline order (style before
/// layout) that `layout_proofs/pipeline/stylesheet_lifecycle.rs` proves
/// behaviorally. The pre-fix pin had no `StyleUpdated` and no tracked-set
/// allocations on this path; that absence was the first-render bug.
#[test]
fn append_then_update_metric_deltas_match_baseline() {
    let (mut world, mut root, _chip) = chip_tree_setup();
    root.reset_metrics();
    root.reset_node_metrics();

    let document_root = root.document_node().0;
    let second = root.create_element(DomTag::Div).expect("div should insert");
    support::assert_set_attr_str(&mut root, second, "class", "chip");
    root.append_child(document_root, second)
        .expect("div should append");
    support::assert_root_dom_update(&mut world, &mut root, 0.016, DomPointerUpdate::Unchanged);

    assert_root_counts(
        &root,
        &[
            (NodeSlotAllocation, 1),
            (StyleUpdated, 1),
            (LayoutUpdated, 1),
            (PaintUpdated, 1),
            (PaintPlanBuilt, 1),
            (LayerPainted, 1),
            (LayerPresented, 1),
            // spine step 4: style-pass work counters; visited vs changed is
            // proposal J's evidence gate. The appended chip resolves on its
            // first (baseline) pass and the settled chip's groups do not move,
            // so changed stays 0 while visited prices both elements.
            (StylePassNodeVisited, 2),
            (StylePassNodeChanged, 0),
        ],
        "append+update",
    );
    assert_second_append_then_update_history(&root);
    // The terminal clear walk adds 11 node-store Gets on this paint frame
    // (dirt clearing moved out of the build walk into one terminal traversal).
    // computed-projection dual-write (PLAN.md spine 3.3a): 1 node access per
    // visited element for the parent font-seed ancestor walk (2 styled chips);
    // reclaimed when the legacy write path dies in 3.3g.
    // synchronous dual-write re-projection on presentation writes (PLAN.md
    // spine 3.3 fix): the two `width`/`height` writes this recompute applies
    // refresh the computed groups in place (+10 Get, +4 GetMut from 116/23/1);
    // reclaimed when the legacy write path dies in 3.3g.
    // spine step 3.3c (−2 Get from 126/27/1): the two painted chips' rect
    // paint now reads `computed_visual().radius` directly, removing one
    // node-store Get per painted element.
    // spine step 3.3d (−4 Get from 124/27/1): the two chips each pay two
    // re-projections (style pass + their `width`/`height` writes), and each
    // projection now seeds from one `parent_inherited_groups` walk instead of
    // two 3.3a ancestor walks, one fewer Get each.
    // spine step 3.3g-1b (+4 Get, +2 GetMut from 120/27/1): the pass now
    // captures `previous_computed`, reads the resolved longhand slot, and
    // records the new baseline per visited element, and A7's group-diff
    // classification re-marks layout+paint dirty on the width/height writes.
    // declaration-store style system (PLAN.md spine 3.3g-2a, −21 Get, −8
    // GetMut from 124/29/1): the legacy attr-projection channel is deleted —
    // style_mut writes the inline block and marks dirty only, and the style
    // pass writes the seven computed groups directly on the node record.
    // flex arc step 9 leading-margin-chain walk (+1 Get from 103/21/1): the
    // collapsed leading-margin chain reads the first in-flow block child of the
    // first positioned chip only (the second chip is not a first block child) —
    // one node-store read per positioned container, style-read-only, bounded by
    // the empty-run length (FLEX_STEP9_DESIGN.md D1 cost model).
    assert_node_counts(&root, 104, 21, 1, "append+update");
}

fn assert_second_append_then_update_history(root: &RootDom) {
    let history = history_names(root);
    assert!(
        metric_index(&history, NodeSlotAllocation) < metric_index(&history, StyleUpdated),
        "append+update: NodeSlotAllocation before StyleUpdated"
    );
    assert!(
        metric_index(&history, StyleUpdated) < metric_index(&history, LayoutUpdated),
        "append+update: StyleUpdated before LayoutUpdated"
    );
    assert!(
        metric_index(&history, LayoutUpdated) < metric_index(&history, PaintUpdated),
        "append+update: LayoutUpdated before PaintUpdated"
    );
    assert!(
        metric_index(&history, PaintUpdated) < metric_index(&history, PaintPlanBuilt),
        "append+update: PaintUpdated before PaintPlanBuilt"
    );
    assert!(
        metric_index(&history, PaintPlanBuilt) < metric_index(&history, LayerPainted),
        "append+update: PaintPlanBuilt before LayerPainted"
    );
    assert!(
        metric_index(&history, LayerPainted) < metric_index(&history, LayerPresented),
        "append+update: LayerPainted before LayerPresented"
    );
}

/// Parity proof for the style-dirty propagation fix: the style phase the
/// insert path arms must cost exactly what the long-standing attr-triggered
/// path already pays for the same tree. Window A appends a second styled
/// chip; window B toggles a style-affecting attribute on that connected
/// chip. Both windows recomputed style for the same two-chip tree, so their
/// style-phase counters must be identical.
#[test]
fn insert_style_phase_deltas_match_attr_triggered_style_phase() {
    let (mut world, mut root, _chip) = chip_tree_setup();

    root.reset_metrics();
    let document_root = root.document_node().0;
    let second = root.create_element(DomTag::Div).expect("div should insert");
    support::assert_set_attr_str(&mut root, second, "class", "chip");
    root.append_child(document_root, second)
        .expect("div should append");
    support::assert_root_dom_update(&mut world, &mut root, 0.016, DomPointerUpdate::Unchanged);
    let insert_snapshot = root.metrics();

    root.reset_metrics();
    support::assert_set_attr_str(&mut root, second, "class", "chip other");
    support::assert_root_dom_update(&mut world, &mut root, 0.032, DomPointerUpdate::Unchanged);
    let attr_snapshot = root.metrics();

    assert_eq!(
        insert_snapshot.count(StyleUpdated),
        attr_snapshot.count(StyleUpdated),
        "insert and attr triggers must run the same number of style phases",
    );
}

#[test]
fn stylesheet_apply_metric_deltas_stay_pinned() {
    let (mut world, mut root, _chip) = chip_tree_setup();
    root.reset_metrics();
    root.reset_node_metrics();

    support::assert_load_root_stylesheet(
        &root,
        ui_dom::StyleRuleOrigin::new("late", 1),
        ".chip { height: 44px; }",
    );
    support::assert_root_dom_update(&mut world, &mut root, 0.016, DomPointerUpdate::Unchanged);

    assert_root_counts(
        &root,
        &[
            (StyleUpdated, 1),
            (LayoutUpdated, 1),
            (PaintUpdated, 1),
            (PaintPlanBuilt, 1),
            (LayerPainted, 1),
            (LayerPresented, 1),
            // spine step 4: style-pass work counters; visited vs changed is
            // proposal J's evidence gate. The new `height` declaration moves
            // the chip's box group, so changed == visited == 1.
            (StylePassNodeVisited, 1),
            (StylePassNodeChanged, 1),
        ],
        "stylesheet apply",
    );
    let history = history_names(&root);
    assert!(
        metric_index(&history, StyleUpdated) < metric_index(&history, LayoutUpdated),
        "stylesheet apply: StyleUpdated before LayoutUpdated"
    );
    assert!(
        metric_index(&history, LayoutUpdated) < metric_index(&history, PaintUpdated),
        "stylesheet apply: LayoutUpdated before PaintUpdated"
    );
    assert!(
        metric_index(&history, PaintUpdated) < metric_index(&history, PaintPlanBuilt),
        "stylesheet apply: PaintUpdated before PaintPlanBuilt"
    );
    assert!(
        metric_index(&history, PaintPlanBuilt) < metric_index(&history, LayerPainted),
        "stylesheet apply: PaintPlanBuilt before LayerPainted"
    );
    assert!(
        metric_index(&history, LayerPainted) < metric_index(&history, LayerPresented),
        "stylesheet apply: LayerPainted before LayerPresented"
    );
    // The terminal clear walk adds 7 node-store Gets on this paint frame
    // (dirt clearing moved out of the build walk into one terminal traversal).
    // spine step 3.3g-2a (−11 Get, −5 GetMut from 80/19/0): the legacy
    // attr-projection channel is gone — the write-path synchronous
    // re-projection, the winners→legacy-map rebuild, and `apply_styles` are
    // deleted; the typed winners fold resolves the seven groups + transition
    // slot directly on the node record, and the baseline layer-membership
    // re-evaluation replaces the write-path layer queueing.
    // flex arc step 9 leading-margin-chain walk (+1 Get from 69/14/0): the
    // collapsed leading-margin chain reads the first in-flow block child of the
    // one positioned chip (a `first_child_key` lookup that returns none) — one
    // node-store read per positioned container, style-read-only, bounded by the
    // empty-run length (FLEX_STEP9_DESIGN.md D1 cost model).
    assert_node_counts(&root, 70, 14, 0, "stylesheet apply");
}

#[test]
fn flex_row_relayout_metric_deltas_stay_pinned() {
    let (mut world, mut root) = support::attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = support::assert_append_div_to(&mut root, document_root);
    support::assert_style_set(&mut root, container, "display", "flex");
    support::assert_style_set(&mut root, container, "width", "200px");
    support::assert_style_set(&mut root, container, "height", "40px");
    for _ in 0..2 {
        let item = support::assert_append_div_to(&mut root, container);
        support::assert_style_set(&mut root, item, "flex", "0 0 50px");
        support::assert_style_set(&mut root, item, "height", "40px");
    }
    support::assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    root.reset_metrics();
    root.reset_node_metrics();

    support::assert_style_set(&mut root, container, "width", "220px");
    support::assert_root_dom_update(&mut world, &mut root, 0.016, DomPointerUpdate::Unchanged);

    assert_root_counts(
        &root,
        &[
            (StyleUpdated, 1),
            (LayoutUpdated, 1),
            (PaintUpdated, 1),
            (PaintPlanBuilt, 1),
            (LayerPainted, 1),
            (StyleDeclarationUpsert, 1),
            // spine step 4: style-pass work counters; visited vs changed is
            // proposal J's evidence gate. The `width` write re-resolves the
            // container and its two items (visited == 3); only the container's
            // box group moves (changed == 1).
            (StylePassNodeVisited, 3),
            (StylePassNodeChanged, 1),
            // flex step 1 (T6): the measure/place work counters. The fold
            // computes a record for each of the two block items (one
            // `compute_node_measurement` each); placement relayouts each of
            // the two children (one `relayout_active_flex_child` each). No
            // text children and no nested flex items, so measured == placed ==
            // item count == 2.
            (FlexNodesMeasured, 2),
            (FlexNodesPlaced, 2),
        ],
        "flex relayout",
    );
    let history = history_names(&root);
    assert!(
        metric_index(&history, StyleUpdated) < metric_index(&history, LayoutUpdated),
        "flex relayout: StyleUpdated before LayoutUpdated"
    );
    assert!(
        metric_index(&history, LayoutUpdated) < metric_index(&history, PaintUpdated),
        "flex relayout: LayoutUpdated before PaintUpdated"
    );
    assert!(
        metric_index(&history, PaintUpdated) < metric_index(&history, PaintPlanBuilt),
        "flex relayout: PaintUpdated before PaintPlanBuilt"
    );
    assert!(
        metric_index(&history, PaintPlanBuilt) < metric_index(&history, LayerPainted),
        "flex relayout: PaintPlanBuilt before LayerPainted"
    );
    // The terminal clear walk adds 15 node-store Gets on this paint frame
    // (dirt clearing moved out of the build walk into one terminal traversal).
    // synchronous dual-write re-projection on presentation writes (PLAN.md
    // spine 3.3 fix): the direct `width` write inside the measured window
    // refreshes the computed groups in place (+3 Get, +1 GetMut from
    // 111/13/0); reclaimed when the legacy write path dies in 3.3g.
    // spine step 3.3c (−3 Get from 114/14/0): the container and two flex
    // items' rect paint now reads `computed_visual().radius` directly,
    // removing one node-store Get per painted element.
    // spine step 3.3d (−1 Get from 111/14/0): the direct `width` write's
    // re-projection now seeds from one `parent_inherited_groups` walk instead
    // of two 3.3a ancestor walks.
    // spine step 3.3g-1b (+39 Get, +18 GetMut from 110/14/0, +StyleUpdated):
    // the direct `width` write now marks the node style-dirty, arming a full
    // style phase over the container and two items (fold + re-projection +
    // previous-computed capture per element) plus A7's group-diff
    // classification re-marking layout+paint on the width change.
    // declaration-store style system (PLAN.md spine 3.3g-2a, −10 Get, −5
    // GetMut from 149/32/0, +StyleDeclarationUpsert): the `width` write now
    // goes through style_mut (one declaration upsert, style-dirty only), and
    // the style pass writes the seven computed groups directly on the node
    // record — the legacy per-attr projection channel is deleted.
    // flex measure-fold record writes (T4a, +2 Get +2 GetMut from 139/27/0):
    // the fold now writes each measured flex item's measurement record through
    // a read-first gate — one immutable read per item compares the freshly
    // computed record (content contributions: h_min/h_max 0, height 40) against
    // the stored one before taking the mutable borrow. On this relayout the two
    // `flex: 0 0 50px` items' measure-fold records (0/0/40) differ from the
    // block-world `exit_node` records they land on later in the same pass
    // (resolved width 50 → 50/50/40), so the gate's mutable borrow fires each
    // pass (+2 GetMut) alongside its read (+2 Get). Documented cause: record
    // writes in the flex measure fold; the T4a ruling, FLEX_STEP1_EXECUTION.md
    // §10; PLAN.md PR 3 permits re-pin with documented cause.
    // lane-based flex solve (T4b, −3 Get from 141/29/0): placement no longer
    // walks the DOM children through `first_child_key`/`next_sibling_key` (one
    // Get per call over the two-item container) — it iterates the item lane's
    // DOM-ordered range directly and reads each child's resolution via its
    // `order_index` back-reference, so the per-item key lookups that backed the
    // former HashMap re-join are gone. Documented cause: the lane read path
    // replaces the DOM walk; FLEX_STEP1_EXECUTION.md §4 (four-lane design).
    // flex step 2 paint order (+2 Get from 138/29/0): the paint walk now detects
    // the flex container and derives the order-modified child sequence via the
    // `order_modified_children` fast path, whose one detection pass reads each
    // of the two children's `computed_flex().order` (+1 Get each). Documented
    // cause: paint derives the order-modified sequence itself — the order lane
    // is update-scoped and empty on paint-dirty-only updates, so paint cannot
    // consume it (the architect's amendment to FLEX_MAPPING §4 step 2, pin
    // `off_spec_flex_paint_ignores_order_modified_sequence` relabeled on-spec).
    // flex arc step 3a-ii run formation (+2 Get from 140/29/0): the measurement
    // fold now fetches each child once more (`node_for_key`) to classify it
    // against the D1 run-membership predicate (`node_is_inline_run_member`)
    // before routing it to the run fold or the node-item measure (+1 Get each
    // for the two block items). Documented cause: anonymous inline-run
    // formation (CSS Flexbox §4) classifies each child by display/position.
    // flex arc step 3b abs-pos/none exclusion (+3 Get from 142/29/0): placement
    // (`place_excluded_abs_flex_children`, `flex.rs`) walks the container's DOM
    // children once more to find excluded abs-pos/fixed children for the D3
    // static-position layout (+1 Get each for the two children, +1 for the
    // container's first-child lookup). Documented cause: excluded children are
    // absent from the item lane (they form no item view), so placement must
    // read them from the DOM child list; FLEX_STEP3B_DESIGN.md D4.
    // flex arc step 4 auto-min by fold (−2 Get from 145/29/0): the recursive
    // auto-min walk died (D5); the derivation now reads the already-written
    // child record from the item view (no extra `node_for_key` re-fetch) and
    // the §9.9 intrinsic fold reads the item lane (D2a), neither of which walks
    // the DOM child list. Documented cause: auto-min by fold
    // (FLEX_STEP4_DESIGN.md D0/D2a).
    // flex arc step 7 main_influences_cross propagation (+2 Get from 143/29/0):
    // the measure fold reads each measured item's first-child link once to
    // propagate the `main_influences_cross` flag bottom-up (a container whose
    // in-flow child is flagged is text-bearing, so its own cross is width-
    // dependent). The two `flex: 0 0 50px` items are childless, so the read is
    // one `first_child_key` lookup each. Documented cause: D2's flag
    // propagation (FLEX_STEP7_DESIGN.md D2).
    assert_node_counts(&root, 145, 29, 0, "flex relayout");
}

/// Builds the (d1) scenario (`FLEX_DEBATE_BRIEF.md` (d1)(v), ratified): a
/// wrapping row flex container at 240px content width holding 12 short text
/// items plus 4 nested flex items each with a text child. Returns
/// `(world, root, container, first_text)`.
fn flex_step1_scenario_setup() -> (World, RootDom, DomNodeSlotKey, DomNodeSlotKey) {
    let (world, mut root) = support::attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = support::assert_append_div_to(&mut root, document_root);
    support::assert_style_set(&mut root, container, "display", "flex");
    support::assert_style_set(&mut root, container, "flex-wrap", "wrap");
    support::assert_style_set(&mut root, container, "width", "240px");
    let mut first_text = None;
    for index in 0..12 {
        let text = support::assert_append_text_to(&mut root, container, "aa");
        if index == 0 {
            first_text = Some(text);
        }
    }
    for _ in 0..4 {
        let nested = support::assert_append_div_to(&mut root, container);
        support::assert_style_set(&mut root, nested, "display", "flex");
        support::assert_append_text_to(&mut root, nested, "bb");
    }
    let first_text = first_text.expect("first text item should exist");
    (world, root, container, first_text)
}

/// The (d1) scenario pin: exact `FlexNodesMeasured`/`FlexNodesPlaced` counts
/// per update, derived from the implementation.
///
/// Tree: container (flex row, wrap, 240px) with 12 bare text children plus 4
/// nested flex items, each nested item holding one text child.
///
/// `FlexNodesMeasured` is one per `compute_node_measurement` on the flex
/// measure path (`measure_exited_node_flow_size`). The measurement fold over
/// the container's 16 children folds the 12 bare text children into ONE
/// anonymous inline run and measures each of its members for the run's cross
/// (D4, 12 measured nodes), plus 4 nested flex items (two each: the item's
/// text child via its inner fold, and the nested container's own record), so
/// the fold measures 20 nodes. The staged double-layout formerly re-measured
/// each nested item's text child once more when placement re-entered the flex
/// solver; the record-serving placement (flex arc step 7, D3) reads the fresh
/// wrapped-text-height record for width-unchanged items instead of re-shaping,
/// so the 4 placement re-measures are gone. Total measured = 20.
///
/// `FlexNodesPlaced` is one per flex ITEM the placement walk relayouts
/// (`relayout_active_flex_child` for a block item, `relayout_anonymous_inline_run`
/// for a run). Re-pinned in flex arc step 3a-ii: the 12 bare text children now
/// merge into ONE anonymous run, so top-level placement relayouts 1 run + 4
/// nested flex items (5), and each nested item's placement relayouts its own
/// single-text run (4). Total placed = 9 (was 20 under the per-child model
/// that relaid all 16 top-level children plus the 4 nested text children).
/// Documented cause: anonymous inline-run merging (CSS Flexbox §4) — the item
/// count replaces the child count.
///
/// `FlexNodesRefolded` is the step-7 counter for flagged-and-width-changed
/// re-measures. The scenario's items resolve at their measured widths
/// (content-driven auto sizes; wrapping is line-level, not item shrink), so the
/// observed refold fraction is 0 by construction — the ≤50% ceiling is not
/// breached, and the within-update memo stays designed-not-built.
#[test]
fn flex_step1_scenario_measure_place_counts_stay_pinned() {
    let (mut world, mut root, _container, first_text) = flex_step1_scenario_setup();
    support::assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    root.reset_metrics();
    root.reset_node_metrics();

    root.set_text_content(first_text, "longer text")
        .expect("text update should succeed");
    support::assert_root_dom_update(&mut world, &mut root, 0.016, DomPointerUpdate::Unchanged);

    assert_root_counts(
        &root,
        &[
            (TextIntrinsicMeasurement, 1),
            (TextStorageAllocation, 1),
            (LayoutUpdated, 1),
            (PaintUpdated, 1),
            (PaintPlanBuilt, 1),
            (LayerPainted, 1),
            (FlexNodesMeasured, 20),
            (FlexNodesPlaced, 9),
            (FlexNodesRefolded, 0),
        ],
        "flex step 1 scenario",
    );
    let history = history_names(&root);
    assert!(
        metric_index(&history, FlexNodesMeasured) < metric_index(&history, FlexNodesPlaced),
        "flex step 1: FlexNodesMeasured before FlexNodesPlaced (fold precedes placement)"
    );
    assert!(
        metric_index(&history, LayoutUpdated) < metric_index(&history, PaintUpdated),
        "flex step 1: LayoutUpdated before PaintUpdated"
    );
}

#[test]
fn text_rewrap_metric_deltas_stay_pinned() {
    let (mut world, mut root) = support::attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let block = support::assert_append_div_to(&mut root, document_root);
    support::assert_style_set(&mut root, block, "width", "80px");
    let text = support::assert_append_text_to(&mut root, block, "one two three four");
    support::assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    root.reset_metrics();
    root.reset_node_metrics();

    root.set_text_content(text, "five six seven eight")
        .expect("text update should succeed");
    support::assert_root_dom_update(&mut world, &mut root, 0.016, DomPointerUpdate::Unchanged);

    assert_root_counts(
        &root,
        &[
            (TextIntrinsicMeasurement, 1),
            (TextStorageAllocation, 1),
            (LayoutUpdated, 1),
            (PaintUpdated, 1),
            (PaintPlanBuilt, 1),
            (LayerPainted, 1),
        ],
        "text rewrap",
    );
    let history = history_names(&root);
    assert!(
        metric_index(&history, TextIntrinsicMeasurement)
            < metric_index(&history, TextStorageAllocation),
        "text rewrap: TextIntrinsicMeasurement before TextStorageAllocation"
    );
    assert!(
        metric_index(&history, TextStorageAllocation) < metric_index(&history, LayoutUpdated),
        "text rewrap: TextStorageAllocation before LayoutUpdated"
    );
    assert!(
        metric_index(&history, LayoutUpdated) < metric_index(&history, PaintUpdated),
        "text rewrap: LayoutUpdated before PaintUpdated"
    );
    assert!(
        metric_index(&history, PaintUpdated) < metric_index(&history, PaintPlanBuilt),
        "text rewrap: PaintUpdated before PaintPlanBuilt"
    );
    assert!(
        metric_index(&history, PaintPlanBuilt) < metric_index(&history, LayerPainted),
        "text rewrap: PaintPlanBuilt before LayerPainted"
    );
    // The terminal clear walk adds 11 node-store Gets on this paint frame
    // (dirt clearing moved out of the build walk into one terminal traversal).
    // spine step 3.3c (−1 Get from 79/14/0): the block's rect paint now reads
    // `computed_visual().radius` directly, removing one node-store Get.
    // spine step 3.3d (−1 Get from 78/14/0): `set_text_content`'s ancestor
    // seed stops at the nearest element instead of climbing the full chain
    // (the legacy first-set-wins climb never completed for an attr-less block).
    // flex arc step 9 leading-margin-chain walk (+3 Get from 77/14/0): the
    // collapsed leading-margin chain walks the one positioned block's first
    // in-flow block child — `first_child_key`, the inline text child's style
    // read (skipped as inline), then `next_sibling_key` returning none — one
    // node-store read per leading-descendant level, style-read-only, bounded by
    // the empty-run length (FLEX_STEP9_DESIGN.md D1 cost model).
    assert_node_counts(&root, 80, 14, 0, "text rewrap");
}

// The four `root_node_store_metrics_*` tests below were relocated from
// `layout_proofs/layout/normal_flow.rs`: they pin node-store instrumentation, not
// layout, and the layout_proofs module policy asserts layout results only.

fn node_store_metric_history_names(root: &RootDom) -> Vec<RootDomNodesMetricName> {
    root.node_history().map(|entry| entry.name).collect()
}

fn metric_index(history: &[RootDomMetricName], metric_name: RootDomMetricName) -> usize {
    history
        .iter()
        .position(|name| *name == metric_name)
        .unwrap_or_else(|| panic!("{metric_name:?} should be present in history"))
}

fn node_metric_index(
    history: &[RootDomNodesMetricName],
    metric_name: RootDomNodesMetricName,
) -> usize {
    history
        .iter()
        .position(|name| *name == metric_name)
        .unwrap_or_else(|| panic!("{metric_name:?} should be present in history"))
}

#[test]
fn root_node_store_metrics_track_existing_public_root_accesses() {
    let (_world, mut root) = support::attached_gpu_root_dom_default_viewport_setup();

    root.reset_node_metrics();

    let document_root = root.document_node().0;
    let child = root
        .create_element(DomTag::Div)
        .expect("div should insert through the root node store");
    let node = root.node(document_root);

    assert!(node.is_some(), "document root should remain readable");
    assert_eq!(root.node_count(RootDomNodesMetricName::Get), 3);
    assert_eq!(root.node_count(RootDomNodesMetricName::Insert), 1);

    let history = node_store_metric_history_names(&root);
    assert!(
        node_metric_index(&history, RootDomNodesMetricName::Get)
            < node_metric_index(&history, RootDomNodesMetricName::Insert),
        "node store: Get must appear before Insert"
    );

    let snapshot = root.node_metrics();
    assert_eq!(snapshot.count(RootDomNodesMetricName::Get), 3);
    assert_eq!(snapshot.count(RootDomNodesMetricName::Insert), 1);

    assert!(
        root.node(child).is_some(),
        "inserted child should stay readable"
    );
}

#[test]
fn root_node_store_metrics_track_document_root_lookup_by_itself() {
    let (_world, mut root) = support::attached_gpu_root_dom_default_viewport_setup();

    root.reset_node_metrics();

    let _document_root = root.document_node().0;

    assert_eq!(root.node_count(RootDomNodesMetricName::Get), 1);
    assert!(node_store_metric_history_names(&root).contains(&RootDomNodesMetricName::Get));
}

#[test]
fn root_node_store_metrics_track_text_node_insert_and_lookup() {
    let (_world, mut root) = support::attached_gpu_root_dom_default_viewport_setup();

    root.reset_node_metrics();

    let text = root
        .create_text_node("metrics")
        .expect("text should insert through the root node store");

    assert!(
        root.node(text).is_some(),
        "inserted text should remain readable"
    );
    assert_eq!(root.node_count(RootDomNodesMetricName::Insert), 1);
    assert_eq!(root.node_count(RootDomNodesMetricName::Get), 2);
    let history = node_store_metric_history_names(&root);
    assert!(
        node_metric_index(&history, RootDomNodesMetricName::Insert)
            < node_metric_index(&history, RootDomNodesMetricName::Get),
        "node store: Insert must appear before Get"
    );
}

#[test]
fn root_node_store_metrics_reset_clears_previous_accesses() {
    let (_world, mut root) = support::attached_gpu_root_dom_default_viewport_setup();

    root.reset_node_metrics();
    let _document_root = root.document_node().0;
    let _text = root
        .create_text_node("before reset")
        .expect("text should insert before reset");

    assert!(
        !node_store_metric_history_names(&root).is_empty(),
        "metric history should record accesses before reset"
    );

    root.reset_node_metrics();

    assert_eq!(root.node_count(RootDomNodesMetricName::Get), 0);
    assert_eq!(root.node_count(RootDomNodesMetricName::Insert), 0);
    assert!(node_store_metric_history_names(&root).is_empty());
}

/// A relayout of an already-built fixed-layer tree must not insert or remove
/// node slots: the node store is the retained DOM, not a per-frame structure.
/// Relocated from `layout_proofs/layout/fixed_layers.rs`, where this invariant was
/// embedded inside two layout proofs.
#[test]
fn fixed_layer_relayout_does_not_churn_node_store() {
    let (mut world, mut root, fixed_panel, _first_block, _second_block) =
        support::fixed_layer_with_two_blocks_setup();
    support::assert_style_set(&mut root, fixed_panel, "position", "fixed");
    support::assert_style_set(&mut root, fixed_panel, "top", "20px");
    support::assert_style_set(&mut root, fixed_panel, "right", "20px");

    root.reset_node_metrics();
    support::assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);

    let snapshot = root.node_metrics();
    assert_eq!(
        snapshot.count(RootDomNodesMetricName::Insert),
        0,
        "layout update should not insert nodes for a pure fixed-position relayout"
    );
    assert_eq!(
        snapshot.count(RootDomNodesMetricName::Remove),
        0,
        "layout update should not remove nodes for a pure fixed-position relayout"
    );
}
