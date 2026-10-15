//! Allocation contract tests for the `ui-dom` retained pipeline.
//!
//! This target installs a counting global allocator and pins the exact
//! allocator activity of one warmed update per scenario. It exists so that
//! correctness fixes can prove zero added allocations: any change that adds
//! allocator traffic to a pinned window breaks the contract and must be
//! justified in the test comment when the pin is updated.
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

fn assert_window_stats(label: &str, expected: Stats, actual: Stats) {
    assert_eq!(
        actual, expected,
        "{label}: allocator activity drifted from the pinned baseline",
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
        Stats {
            allocations: 2,
            deallocations: 2,
            reallocations: 0,
            bytes_allocated: 40,
            bytes_deallocated: 40,
            bytes_reallocated: 0,
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
        Stats {
            // C1 move-only ops remove the paint plan clone (PLAN.md §4.4 C1):
            // −1 allocation, −128 bytes in this window.
            allocations: 59,
            deallocations: 59,
            reallocations: 0,
            bytes_allocated: 5888,
            bytes_deallocated: 5888,
            bytes_reallocated: 0,
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
        Stats {
            // C1 move-only ops remove the paint plan clone (PLAN.md §4.4 C1):
            // −1 allocation, −128 bytes in this window.
            allocations: 68,
            deallocations: 66,
            reallocations: 2,
            bytes_allocated: 14169,
            bytes_deallocated: 8180,
            bytes_reallocated: 5440,
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
        Stats {
            allocations: 26,
            deallocations: 27,
            reallocations: 0,
            bytes_allocated: 3661,
            bytes_deallocated: 3672,
            bytes_reallocated: 0,
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
    support::assert_set_attr_str(&mut root, block, "width", "80px");
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
        Stats {
            // C1 move-only ops remove the paint plan clone (PLAN.md §4.4 C1):
            // −2 allocations, −2 deallocations. The plan owns one rect Vec and
            // one text-run Vec; the −1 reallocation / −768 reallocated bytes is
            // the allocator's second-order response to the removed allocations
            // (a shaping buffer growth shifts from realloc-in-place to a fresh
            // alloc), not a separate production change.
            allocations: 163,
            deallocations: 151,
            reallocations: 26,
            bytes_allocated: 91632,
            bytes_deallocated: 32621,
            bytes_reallocated: 8736,
        },
        region.change(),
    );
}
