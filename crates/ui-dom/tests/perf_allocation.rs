//! Allocation contract tests for the `ui-dom` retained pipeline.
//!
//! This target installs a counting global allocator and pins the per-window
//! allocation, deallocation, and reallocation counts of one warmed update
//! per scenario. Counts are platform-stable, so a correctness fix that adds
//! allocator traffic to a pinned window breaks the contract and must be
//! justified in the test comment when the pin is updated.
//!
//! Leak detection comes from the pinned deallocation counts: a leaked
//! allocation shows up as fewer deallocations than the pin, and free counts
//! are platform-stable even where freed sizes differ (e.g. std `Mutex` is 40
//! bytes under glibc vs 4 bytes under `os_unfair_lock`).
//!
//! Exact byte totals are informational-only: they are reported in failure
//! messages, never pinned, because they are platform-fragile (std `Mutex`
//! is 40 bytes under glibc vs 4 bytes under `os_unfair_lock`; dependency
//! type sizes shift with feature flags). The C1 clone removal's −128 bytes
//! was a one-time measurement recorded in commit `0f7a807`; its durable pin
//! is the −1 allocation count.
//!
//! The measurement story is two layers: semantic work-site counters are
//! pinned in `tests/perf_metric.rs` under the repo's `metrics()` pattern;
//! raw allocator counts are pinned here.
//!
//! All windows live in one test so they run sequentially: `stats_alloc`
//! counts process-wide allocator activity, so windows must not overlap with
//! another test in this binary. Keep every allocation-measuring scenario in
//! `allocation_windows_match_baseline_after_warmup` and do not add a second
//! `#[test]` to this target.

use stats_alloc::{INSTRUMENTED_SYSTEM, Region, Stats, StatsAlloc};
use std::alloc::System;
use ui_dom::{DomPointerState, DomPointerUpdate, DomTag};

#[global_allocator]
static GLOBAL_ALLOCATOR: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

#[allow(dead_code)]
#[path = "layout_proofs/support.rs"]
mod support;

fn pointer_at(x_px: f32, y_px: f32) -> DomPointerUpdate {
    DomPointerUpdate::Set(DomPointerState {
        x_px,
        y_px,
        primary_button_down: false,
        scroll_delta_y_px: 0.0,
    })
}

/// The pinned, platform-stable facts of one update window: counts only.
/// Byte totals are platform-dependent (std `Mutex` is 40 bytes under glibc
/// vs 4 bytes under `os_unfair_lock`, dependency type sizes vary with
/// feature flags) and are reported in failure messages, never pinned.
#[derive(Clone, Copy, Debug)]
struct WindowPin {
    allocations: usize,
    deallocations: usize,
    reallocations: usize,
}

fn assert_window_stats(label: &str, expected: WindowPin, actual: Stats) {
    assert_eq!(
        actual.allocations, expected.allocations,
        "{label}: allocation count drifted from the pinned baseline (bytes: {} allocated / {} deallocated)",
        actual.bytes_allocated, actual.bytes_deallocated,
    );
    assert_eq!(
        actual.deallocations, expected.deallocations,
        "{label}: deallocation count drifted from the pinned baseline (bytes: {} allocated / {} deallocated)",
        actual.bytes_allocated, actual.bytes_deallocated,
    );
    assert_eq!(
        actual.reallocations, expected.reallocations,
        "{label}: reallocation count drifted from the pinned baseline (bytes: {} reallocated)",
        actual.bytes_reallocated,
    );
}

#[expect(clippy::too_many_lines)]
#[test]
fn allocation_windows_match_baseline_after_warmup() {
    // Steady-state window: a clean update currently allocates twice (40
    // bytes). This pre-existing cost is pinned so it cannot grow.
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
    let region = Region::new(GLOBAL_ALLOCATOR);
    support::assert_root_dom_update(&mut world, &mut root, 0.016, DomPointerUpdate::Unchanged);
    assert_window_stats(
        "steady-state update",
        WindowPin {
            allocations: 2,
            deallocations: 2,
            reallocations: 0,
        },
        region.change(),
    );
    drop((world, root));

    // Hover-on window: pointer enter over the styled chip triggers one full
    // style recompute, layout, and paint. Pinned so the style-dirty
    // propagation fix cannot add allocator traffic to the shared style path.
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
    support::assert_root_dom_update(&mut world, &mut root, 0.016, pointer_at(10.0, 10.0));
    support::assert_root_dom_update(&mut world, &mut root, 0.032, pointer_at(300.0, 300.0));
    let region = Region::new(GLOBAL_ALLOCATOR);
    support::assert_root_dom_update(&mut world, &mut root, 0.048, pointer_at(10.0, 10.0));
    assert_window_stats(
        "hover-on update",
        WindowPin {
            // declaration-store substrate: matched-rule attr_map clones
            // eliminated; values format in place under one lock; the radius
            // longhand read uses the non-allocating custom-attr lookup —
            // PLAN.md spine step 2 (−10 allocations, −10 deallocations vs the
            // pre-step pin of 59/59/0).
            // spine step 3.3b (−1 allocation, −1 deallocation vs the pre-step
            // pin of 49/49/0): `node_style` now reads the computed font group,
            // so the block/inline child loops no longer compute the child's
            // inherited context solely to feed `effective_font_size_px`; the
            // removed `inherited_context_from_node` call dropped its one
            // `inherited_color_attribute` "color" key String allocation.
            // spine step 3.3c (−12 allocations, −12 deallocations vs the
            // pre-step pin of 48/48/0): the element rect paint now reads the
            // computed visual group (`background_rgba`, `border_rgba`, `radius`,
            // `outline_*`) instead of the stringly `custom_color_attr_rgba`/
            // `custom_length_attr_px` helpers, dropping one `DomAttribute::
            // Custom` key String allocation per background/border-side/outline
            // read (12 per painted element).
            // spine step 3.3d (−5 allocations, −5 deallocations vs the pre-step
            // pin of 36/36/0): `inherited_context_from_node` now reads the
            // node's computed groups instead of `presented_attr`, dropping the
            // `inherited_color_attribute` "color" key String it allocated on
            // every remaining call site (paint walk + layout node entry).
            // spine step 3.3g-1a typed winners fold (−9 allocations, −9
            // deallocations vs the pre-step pin of 31/31/0): the fold resolves
            // the stringly map from typed winners under one runtime lock, and
            // the base display is applied at element creation rather than
            // re-projected through the legacy map; the legacy map rebuild dies
            // in 3.3g-2.
            // declaration-store style system (PLAN.md spine 3.3g-2a, −6
            // allocations, −6 deallocations vs the pre-step pin of 22/22/0):
            // the style pass writes the seven computed groups directly on the
            // node record; the per-attr write channel and its bookkeeping are
            // deleted, and no matched-rule map rebuild remains.
            allocations: 16,
            deallocations: 16,
            reallocations: 0,
        },
        region.change(),
    );
    drop((world, root));

    // Append + update window: node insertion plus one style recompute, one
    // layout, and one paint. Post style-dirty propagation fix, this window
    // includes the style phase the insert path was wrongly skipping (29
    // extra balanced allocations for matched-rule application and the two
    // stylesheet tracked-attribute sets); the propagation marks themselves
    // are allocation-free ancestor walks, proven by the unchanged
    // class-toggle window below, whose style phase was already armed
    // pre-fix.
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
    let warm = root
        .create_element(DomTag::Div)
        .expect("warm div should insert");
    root.append_child(document_root, warm)
        .expect("warm div should append");
    support::assert_root_dom_update(&mut world, &mut root, 0.016, DomPointerUpdate::Unchanged);
    let region = Region::new(GLOBAL_ALLOCATOR);
    let second = root.create_element(DomTag::Div).expect("div should insert");
    root.append_child(document_root, second)
        .expect("div should append");
    support::assert_root_dom_update(&mut world, &mut root, 0.032, DomPointerUpdate::Unchanged);
    assert_window_stats(
        "append+update",
        WindowPin {
            // declaration-store substrate: matched-rule attr_map clones
            // eliminated; values format in place under one lock; the radius
            // longhand read uses the non-allocating custom-attr lookup —
            // PLAN.md spine step 2 (−6 allocations, −6 deallocations vs the
            // pre-step pin of 68/66/2).
            // spine step 3.3b (−3 allocations, −3 deallocations vs the pre-step
            // pin of 62/60/2): three block/inline child-loop iterations
            // (chip, warm, second) no longer compute the child inherited
            // context solely for `effective_font_size_px`, dropping one
            // `inherited_color_attribute` "color" key String allocation each.
            // spine step 3.3c (−12 allocations, −12 deallocations vs the
            // pre-step pin of 59/57/2): the element rect paint now reads the
            // computed visual group, dropping the stringly
            // `custom_color_attr_rgba`/`custom_length_attr_px` key Strings.
            // spine step 3.3d (−11 allocations, −11 deallocations vs the
            // pre-step pin of 47/45/2): `inherited_context_from_node` no longer
            // allocates the `inherited_color_attribute` "color" key String on
            // its remaining call sites (paint + layout node entries across the
            // three styled chips' layout/paint visits).
            // spine step 3.3g-1a typed winners fold (−20 allocations, −20
            // deallocations vs the pre-step pin of 36/34/2): the fold resolves
            // the stringly map from typed winners under one runtime lock, and
            // the base display is applied at element creation rather than
            // re-projected through the legacy map; the legacy map rebuild dies
            // in 3.3g-2.
            // declaration-store style system (PLAN.md spine 3.3g-2a, −8
            // allocations, −8 deallocations vs the pre-step pin of 16/14/2):
            // the style pass writes the seven computed groups directly on the
            // node record; the per-attr write channel and its bookkeeping are
            // deleted, and no matched-rule map rebuild remains.
            allocations: 8,
            deallocations: 8,
            reallocations: 2,
        },
        region.change(),
    );
    drop((world, root));

    // Class-toggle window: style-affecting attr change on a connected node
    // plus one update. The style-dirty propagation fix must not change it.
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
    support::assert_set_attr_str(&mut root, chip, "class", "chip other");
    support::assert_root_dom_update(&mut world, &mut root, 0.016, DomPointerUpdate::Unchanged);
    let region = Region::new(GLOBAL_ALLOCATOR);
    support::assert_set_attr_str(&mut root, chip, "class", "chip");
    support::assert_root_dom_update(&mut world, &mut root, 0.032, DomPointerUpdate::Unchanged);
    assert_window_stats(
        "class-toggle update",
        WindowPin {
            // declaration-store substrate: matched-rule attr_map clones
            // eliminated; values format in place under one lock — PLAN.md
            // spine step 2 (−5 allocations, −5 deallocations vs the pre-step
            // pin of 26/27/0).
            // spine step 3.3d (−1 allocation, −1 deallocation vs the pre-step
            // pin of 21/22/0): the remaining `inherited_context_from_node`
            // call sites no longer allocate the `inherited_color_attribute`
            // "color" key String.
            // spine step 3.3g-1a typed winners fold (−6 allocations, −6
            // deallocations vs the pre-step pin of 20/21/0): the fold resolves
            // the stringly map from typed winners under one runtime lock; the
            // legacy map rebuild dies in 3.3g-2.
            // declaration-store style system (PLAN.md spine 3.3g-2a, −6
            // allocations, −6 deallocations vs the pre-step pin of 14/15/0):
            // the style pass writes the seven computed groups directly on the
            // node record; the per-attr write channel and its bookkeeping are
            // deleted, and no matched-rule map rebuild remains.
            allocations: 8,
            deallocations: 9,
            reallocations: 0,
        },
        region.change(),
    );
    drop((world, root));

    // Text rewrap window: changed text content plus one update. Text
    // shaping owns most of this traffic; pinned so layout/paint changes
    // cannot hide added allocations inside it.
    let (mut world, mut root) = support::attached_gpu_root_dom_default_viewport_setup();
    let document_root = root.document_node().0;
    let block = support::assert_append_div_to(&mut root, document_root);
    support::assert_style_set(&mut root, block, "width", "80px");
    let text = support::assert_append_text_to(&mut root, block, "one two three four");
    support::assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    root.set_text_content(text, "five six seven eight")
        .expect("warm rewrap should succeed");
    support::assert_root_dom_update(&mut world, &mut root, 0.016, DomPointerUpdate::Unchanged);
    let region = Region::new(GLOBAL_ALLOCATOR);
    root.set_text_content(text, "nine ten eleven twelve")
        .expect("rewrap should succeed");
    support::assert_root_dom_update(&mut world, &mut root, 0.032, DomPointerUpdate::Unchanged);
    assert_window_stats(
        "text-rewrap update",
        WindowPin {
            // declaration-store substrate: the radius longhand read uses the
            // non-allocating custom-attr lookup (removes the one per-element
            // "border-radius" key String) — PLAN.md spine step 2 (−1
            // allocation, −1 deallocation vs the pre-step pin of 163/151/26).
            // spine step 3.3b (−2 allocations, −2 deallocations vs the pre-step
            // pin of 162/150/26): the document→block and block→text child-loop
            // iterations no longer compute the child inherited context solely
            // for `effective_font_size_px`, dropping one `inherited_color_attribute`
            // "color" key String allocation each.
            // spine step 3.3c (−12 allocations, −12 deallocations vs the
            // pre-step pin of 160/148/26): the block element's rect paint now
            // reads the computed visual group, dropping the stringly
            // `custom_color_attr_rgba`/`custom_length_attr_px` key Strings.
            // spine step 3.3d (−8 allocations, −8 deallocations vs the pre-step
            // pin of 148/136/26): `inherited_context_from_node` no longer
            // allocates the `inherited_color_attribute` "color" key String on
            // the rewrap's layout/paint node visits.
            // spine step 3.3g-1a typed winners fold (−2 allocations, −2
            // deallocations vs the pre-step pin of 140/128/26): the fold
            // resolves the stringly map from typed winners under one runtime
            // lock; the legacy map rebuild dies in 3.3g-2.
            allocations: 138,
            deallocations: 126,
            reallocations: 26,
        },
        region.change(),
    );

    // flex step 1, T6b — the text-free (d1) shape, layout-phase window.
    //
    // The §3.4 pass-ownership decomposition (amended 2026-10-19): the reflow
    // machinery (four lanes, record, solve, placement) allocates NOTHING after
    // warmup — proven by the lanes at high-water mark and no
    // `Vec`/`String`/`collect` in the flex solve. This window is the narrowest
    // the harness can isolate: the CPU backend skips the per-paint
    // `LayerPaintPlan` rebuild and the width toggle runs OUTSIDE the region, so
    // the pin below is the update-only window. The residual is the clean-update
    // base + the style pass's winners fold (proposal J's sleeping target) + a
    // small mixed-size flex-path residual — none of which is the reflow
    // machinery (the lanes sit at high-water mark for the whole solve).
    let (mut world, mut root) = support::attached_gpu_root_dom_setup_with_context(
        ui_dom::RootDomContext::new_cpu(),
        320.0,
        320.0,
    );
    let document_root = root.document_node().0;
    let container = support::assert_append_div_to(&mut root, document_root);
    support::assert_style_set(&mut root, container, "display", "flex");
    support::assert_style_set(&mut root, container, "flex-wrap", "wrap");
    support::assert_style_set(&mut root, container, "width", "240px");
    let first_item = support::assert_append_div_to(&mut root, container);
    support::assert_style_set(&mut root, first_item, "width", "40px");
    support::assert_style_set(&mut root, first_item, "height", "20px");
    for _ in 1..12 {
        let item = support::assert_append_div_to(&mut root, container);
        support::assert_style_set(&mut root, item, "width", "40px");
        support::assert_style_set(&mut root, item, "height", "20px");
    }
    for _ in 0..4 {
        let nested = support::assert_append_div_to(&mut root, container);
        support::assert_style_set(&mut root, nested, "display", "flex");
        let child = support::assert_append_div_to(&mut root, nested);
        support::assert_style_set(&mut root, child, "width", "30px");
        support::assert_style_set(&mut root, child, "height", "10px");
    }
    support::assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    support::assert_style_set(&mut root, first_item, "width", "50px");
    support::assert_root_dom_update(&mut world, &mut root, 0.016, DomPointerUpdate::Unchanged);
    support::assert_style_set(&mut root, first_item, "width", "40px");
    let region = Region::new(GLOBAL_ALLOCATOR);
    support::assert_root_dom_update(&mut world, &mut root, 0.032, DomPointerUpdate::Unchanged);
    assert_window_stats(
        "flex step 1 text-free layout phase",
        WindowPin {
            allocations: 7,
            deallocations: 7,
            reallocations: 2,
        },
        region.change(),
    );
    drop((world, root));

    // flex step 1, T6b — the (d1) text-bearing layout-phase window.
    //
    // inherited text-pipeline cadence; machinery zero — proven by (a); the
    // change-detection-gated fold (step 4+) is the designed answer.
    let (mut world, mut root) = support::attached_gpu_root_dom_setup_with_context(
        ui_dom::RootDomContext::new_cpu(),
        320.0,
        320.0,
    );
    let document_root = root.document_node().0;
    let container = support::assert_append_div_to(&mut root, document_root);
    support::assert_style_set(&mut root, container, "display", "flex");
    support::assert_style_set(&mut root, container, "flex-wrap", "wrap");
    support::assert_style_set(&mut root, container, "width", "240px");
    let first_text = support::assert_append_text_to(&mut root, container, "aa");
    for _ in 1..12 {
        support::assert_append_text_to(&mut root, container, "aa");
    }
    for _ in 0..4 {
        let nested = support::assert_append_div_to(&mut root, container);
        support::assert_style_set(&mut root, nested, "display", "flex");
        support::assert_append_text_to(&mut root, nested, "bb");
    }
    support::assert_root_dom_update(&mut world, &mut root, 0.0, DomPointerUpdate::Unchanged);
    root.set_text_content(first_text, "longer text")
        .expect("warm rewrap should succeed");
    support::assert_root_dom_update(&mut world, &mut root, 0.016, DomPointerUpdate::Unchanged);
    root.set_text_content(first_text, "aa")
        .expect("rewrap should succeed");
    let region = Region::new(GLOBAL_ALLOCATOR);
    support::assert_root_dom_update(&mut world, &mut root, 0.032, DomPointerUpdate::Unchanged);
    assert_window_stats(
        "flex step 1 (d1) text layout phase",
        WindowPin {
            // Re-pinned in flex arc step 3a-ii: the 12 bare text children now
            // fold into ONE anonymous inline run instead of 12 per-child items,
            // so the measurement fold and the lane solve allocate far fewer
            // item/order/resolution lane entries (allocations 1934 → 1014,
            // reallocations 85 → 45). Run formation itself is fold-local state
            // (no allocation); the drop is the reduced ITEM count, CSS Flexbox
            // §4. Documented cause: anonymous inline-run merging.
            // Re-pinned in flex arc step 4 (allocations 1014 → 830,
            // reallocations 45 → 37): the recursive auto-min walk and its
            // per-child re-resolution died — the derivation is now two record
            // reads and one `min` (D1) plus the §9.9 intrinsic fold (D2a),
            // neither of which allocates. Documented cause: auto-min by fold
            // (FLEX_STEP4_DESIGN.md D0/D2a).
            allocations: 830,
            deallocations: 830,
            reallocations: 37,
        },
        region.change(),
    );
    drop((world, root));
}
