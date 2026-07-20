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
            (StylesheetTrackedAttributeSetAllocation, 1),
            (LayoutUpdated, 1),
            (PaintUpdated, 1),
            (PaintPlanBuilt, 1),
            (LayerPainted, 1),
        ],
        "hover recompute",
    );
    assert_eq!(
        history_names(&root),
        [
            HoverPseudoStateSet,
            EmittedEventAllocation,
            StyleUpdated,
            StylesheetTrackedAttributeSetAllocation,
            LayoutUpdated,
            PaintUpdated,
            PaintPlanBuilt,
            LayerPainted,
        ],
        "hover recompute must run style before layout before paint",
    );
    assert_node_counts(&root, 100, 15, 0, "hover recompute");
}

/// Post style-dirty propagation fix pin: one append now arms the layer
/// root's style phase, so the update pays one full style recompute for the
/// two styled chips (one tracked-attribute set per styled element) before
/// layout and paint, matching the browser pipeline order (style before
/// layout) that `layout_proofs/stylesheet_lifecycle.rs` proves
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
            (StylesheetTrackedAttributeSetAllocation, 2),
            (LayoutUpdated, 1),
            (PaintUpdated, 1),
            (PaintPlanBuilt, 1),
            (LayerPainted, 1),
            (LayerPresented, 1),
        ],
        "append+update",
    );
    assert_eq!(
        history_names(&root),
        [
            NodeSlotAllocation,
            StyleUpdated,
            StylesheetTrackedAttributeSetAllocation,
            StylesheetTrackedAttributeSetAllocation,
            LayoutUpdated,
            PaintUpdated,
            PaintPlanBuilt,
            LayerPainted,
            LayerPresented,
        ],
        "append+update must restyle before it relays out and repaints",
    );
    assert_node_counts(&root, 103, 23, 1, "append+update");
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
    assert_eq!(
        insert_snapshot.count(StylesheetTrackedAttributeSetAllocation),
        attr_snapshot.count(StylesheetTrackedAttributeSetAllocation),
        "insert and attr triggers must pay the same stylesheet tracked-set cost for the same tree",
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
            (StylesheetTrackedAttributeSetAllocation, 1),
            (LayoutUpdated, 1),
            (PaintUpdated, 1),
            (PaintPlanBuilt, 1),
            (LayerPainted, 1),
            (LayerPresented, 1),
        ],
        "stylesheet apply",
    );
    assert_eq!(
        history_names(&root),
        [
            StyleUpdated,
            StylesheetTrackedAttributeSetAllocation,
            LayoutUpdated,
            PaintUpdated,
            PaintPlanBuilt,
            LayerPainted,
            LayerPresented,
        ],
        "stylesheet apply must run style before layout before paint before present",
    );
    assert_node_counts(&root, 63, 13, 0, "stylesheet apply");
}

#[test]
fn flex_row_relayout_metric_deltas_stay_pinned() {
    let (mut world, mut root) = support::attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let container = support::assert_append_div_to(&mut root, document_root);
    support::assert_set_attr_str(&mut root, container, "display", "flex");
    support::assert_set_attr_str(&mut root, container, "width", "200px");
    support::assert_set_attr_str(&mut root, container, "height", "40px");
    for _ in 0..2 {
        let item = support::assert_append_div_to(&mut root, container);
        support::assert_set_attr_str(&mut root, item, "flex", "0 0 50px");
        support::assert_set_attr_str(&mut root, item, "height", "40px");
    }
    support::assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    root.reset_metrics();
    root.reset_node_metrics();

    support::assert_set_attr_str(&mut root, container, "width", "220px");
    support::assert_root_dom_update(&mut world, &mut root, 0.016, DomPointerUpdate::Unchanged);

    assert_root_counts(
        &root,
        &[
            (LayoutUpdated, 1),
            (PaintUpdated, 1),
            (PaintPlanBuilt, 1),
            (LayerPainted, 1),
        ],
        "flex relayout",
    );
    assert_eq!(
        history_names(&root),
        [LayoutUpdated, PaintUpdated, PaintPlanBuilt, LayerPainted],
        "a width-only attr change must relayout and repaint without a style phase",
    );
    assert_node_counts(&root, 110, 13, 0, "flex relayout");
}

#[test]
fn text_rewrap_metric_deltas_stay_pinned() {
    let (mut world, mut root) = support::attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let block = support::assert_append_div_to(&mut root, document_root);
    support::assert_set_attr_str(&mut root, block, "width", "80px");
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
    assert_eq!(
        history_names(&root),
        [
            TextIntrinsicMeasurement,
            TextStorageAllocation,
            LayoutUpdated,
            PaintUpdated,
            PaintPlanBuilt,
            LayerPainted,
        ],
        "a text content change must remeasure once, then relayout and repaint",
    );
    assert_node_counts(&root, 68, 14, 0, "text rewrap");
}

// The four `root_node_store_metrics_*` tests below were relocated from
// `layout_proofs/normal_flow.rs`: they pin node-store instrumentation, not
// layout, and the layout_proofs module policy asserts layout results only.

fn node_store_metric_history_names(root: &RootDom) -> Vec<RootDomNodesMetricName> {
    root.node_history().map(|entry| entry.name).collect()
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
    assert_eq!(
        history,
        vec![
            RootDomNodesMetricName::Get,
            RootDomNodesMetricName::Insert,
            RootDomNodesMetricName::Get,
            RootDomNodesMetricName::Get,
        ]
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
    assert_eq!(
        node_store_metric_history_names(&root),
        vec![RootDomNodesMetricName::Get]
    );
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
    assert_eq!(
        node_store_metric_history_names(&root),
        vec![
            RootDomNodesMetricName::Insert,
            RootDomNodesMetricName::Get,
            RootDomNodesMetricName::Get,
        ]
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
    assert_eq!(node_store_metric_history_names(&root), Vec::new());
}

/// A relayout of an already-built fixed-layer tree must not insert or remove
/// node slots: the node store is the retained DOM, not a per-frame structure.
/// Relocated from `layout_proofs/fixed_layers.rs`, where this invariant was
/// embedded inside two layout proofs.
#[test]
fn fixed_layer_relayout_does_not_churn_node_store() {
    let (mut world, mut root, fixed_panel, _first_block, _second_block) =
        support::fixed_layer_with_two_blocks_setup();
    support::assert_set_attr_str(&mut root, fixed_panel, "position", "fixed");
    support::assert_set_attr_str(&mut root, fixed_panel, "top", "20px");
    support::assert_set_attr_str(&mut root, fixed_panel, "right", "20px");

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
