use stats_alloc::{INSTRUMENTED_SYSTEM, StatsAlloc};
use stats_alloc::{Region, Stats};
use std::alloc::System;
use ui_dom::{RootDom, RootDomContext};

#[global_allocator]
static GLOBAL_ALLOCATOR: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

#[test]
fn unchanged_text_content_update_is_allocation_free_after_warmup() {
    let mut root = RootDom::new(RootDomContext::new_cpu());
    let text_key = root.create_text_node("steady").expect("text should insert");

    root.set_text_content(text_key, "steady")
        .expect("warm unchanged text update should succeed");

    assert_zero_allocator_activity(GLOBAL_ALLOCATOR, "unchanged text content update", || {
        root.set_text_content(text_key, "steady")
            .expect("unchanged text update should stay allocation-free");
    });
}

fn assert_zero_allocator_activity<R>(
    allocator: &'static StatsAlloc<System>,
    action_name: &str,
    run: impl FnOnce() -> R,
) -> R {
    let region = Region::new(allocator);
    let result = run();
    assert_zero_allocator_stats(&region.change(), action_name);
    result
}

fn assert_zero_allocator_stats(stats: &Stats, action_name: &str) {
    assert_eq!(
        stats.allocations, 0,
        "{action_name} must not perform allocations: {stats:?}"
    );
    assert_eq!(
        stats.deallocations, 0,
        "{action_name} must not perform deallocations: {stats:?}"
    );
    assert_eq!(
        stats.reallocations, 0,
        "{action_name} must not perform reallocations: {stats:?}"
    );
    assert_eq!(
        stats.bytes_allocated, 0,
        "{action_name} must not allocate bytes: {stats:?}"
    );
    assert_eq!(
        stats.bytes_deallocated, 0,
        "{action_name} must not deallocate bytes: {stats:?}"
    );
    assert_eq!(
        stats.bytes_reallocated, 0,
        "{action_name} must not reallocate bytes: {stats:?}"
    );
}
