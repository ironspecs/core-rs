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
            // C1 move-only ops remove the paint plan clone (PLAN.md §4.4 C1):
            // −1 allocation. The −128 bytes was a one-time macOS measurement
            // recorded in commit 0f7a807; the durable pin is the −1
            // allocation count.
            allocations: 59,
            deallocations: 59,
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
            // C1 move-only ops remove the paint plan clone (PLAN.md §4.4 C1):
            // −1 allocation. The −128 bytes was a one-time macOS measurement
            // recorded in commit 0f7a807; the durable pin is the −1
            // allocation count.
            allocations: 68,
            deallocations: 66,
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
            allocations: 26,
            deallocations: 27,
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
        WindowPin {
            // C1 move-only ops remove the paint plan clone (PLAN.md §4.4 C1):
            // −2 allocations, −2 deallocations. The plan owns one rect Vec and
            // one text-run Vec; the −1 reallocation is the allocator's
            // second-order response to the removed allocations (a shaping
            // buffer growth shifts from realloc-in-place to a fresh alloc), not
            // a separate production change. (The −768 reallocated bytes was a
            // one-time measurement and is not pinned.)
            allocations: 163,
            deallocations: 151,
            reallocations: 26,
        },
        region.change(),
    );
}
