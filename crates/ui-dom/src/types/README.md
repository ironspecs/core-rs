# types

The shared DOM type boundary for `ui-dom`: authored DOM values, errors,
retained document memory, and the low-level primitives the rest of the crate
builds on.

## Purpose and scope

This module is the crate's "types" layer. It owns the DOM vocabulary that
external callers and the rest of the crate share — node and attribute types,
the canonical snapshot shapes, the root-owned document (`RootDom`) and its
context/config/metric vocabulary, the event model, and the authored-value
parser in `mod.rs`. It does not own runtime-domain *policy*: layout, paint,
style resolution, hit-testing, and composition live in `domains/`, and
product/app orchestration lives outside this crate entirely.

## Ownership boundaries

Two kinds of files live here, and the boundary between them matters:

- **The authored DOM vocabulary** — the `dom_*` types (node, element, text,
  attribute, event, transition, pseudo-state, style object, tag, canonical
  snapshots), `primitives`, `box_model`, `paint_config`, `pixel_density`,
  `white_space`, `root_dom*` (context/config/metrics/nodes/fragment), and
  `simple_selector`/`dom_query_selector`. These are literal data shapes; they
  carry no runtime policy.
- **The extraction primitives** (PR 5, E1–E7) — small, testable free functions
  over an explicit memory shape, drawn out of `root_dom.rs` bottom-up:
  - `tree_links.rs` — append/detach/validate/collect over the slot-map link
    shape (E1); `splice.rs` — the one unified fragment-import walk (E1 phase 2);
    `node_store.rs`/`raw_node_store.rs` — the arena read surface (E1 phase 2).
  - `id_index.rs` — the id-attr mirror invariant (`insert/remove/rename/assert`)
    (E2).
  - `purge.rs` — `retain_not_in`/`clear_if_in`/`remove_each_in`, the dependents
    purge leaves (E3).
  - `dirty_channel.rs` — the classification→marks policy table plus the upward
    dirty walk (E4).
  - `event_propagation.rs` — the ancestor-path propagate walk (E5);
    `event_pointer.rs` — the pointer-input constructor leaves (E5).
  - `transition_projection.rs` — the transition inverse-projection and
    payload-validation leaves (E7).
  - `layout_scratch.rs` — the per-update flex lane scratch (retained,
    high-water-mark).

`RootDom` itself (`root_dom.rs`) remains the retained-memory owner: the arenas,
indexes, journals, sessions, clock, context, and metrics. The primitives visit
that memory; they do not own it.

## Architectural approach

The decomposition is bottom-up (PR5_DECOMPOSITION_DESIGN.md v2): no top-down
field split, no newtype subsystems. Root fields are retained memory; operations
become shared free functions over explicit memory shapes. Behavior tables
(`element_behaviors/`) turn configured behavior into lookup-map data. When two
instances agree in shape, they align toward the better memory shape
(D1(a′)); when they genuinely diverge, the divergent instance stays bespoke
with a named note (the back-off doctrine, D1).

## Invariants, rules, and decisions

- `mod.rs` owns cross-cutting value types and the authored-value parser; it
  stays strict and deterministic and never absorbs runtime policy.
- Presentation properties are written through `style_mut` (the declaration
  store), never the attribute API; the attribute API rejects presentation
  names with `PresentationRequiresStyleMut`.
- The metrics/history emission surface is zero-allocation: static metric-name
  vocabulary, enum-indexed counters, a fixed-capacity name-only history ring.
  No per-event buffers, payloads, or dynamic strings.
- Primitives fail fast and are strict: no graceful empty fallbacks unless the
  function is named `*_safe`/`try_*` and that weaker contract is the real one.
- `element_behaviors/` is the behavior-table home (see its own README).
