# PR5_DECOMPOSITION_DESIGN.md — RootDom decomposition by bottom-up extraction (v2)

Status: `DIRECTION ADJUDICATED (2026-10-23, owner) — this document is
the design authority; per-extraction briefs derive from it`.
Supersedes v1 (the top-down newtype split) in full. Author: the
architect. Authority: PLAN.md's amended PR 5 section (dated
2026-10-23); the structural survey of 2026-10-23 (the full
field/method/caller census of `root_dom.rs`, 7,464 lines).

## D0. The direction

No top-down field split. The root fields are **retained memory** —
arenas, indexes, journals, sessions, clock, context, metrics — and
the 219 methods are **visits over that memory**. The decomposition
finds the repeated algorithms at the bottom of the file and draws
them out as fast, testable primitives: free functions over explicit
memory shapes, and lookup maps for configured behavior. The
top-level fields then sort themselves by their actual content and
means — the struct stays the memory layout; the operations become
shared primitives. No newtype subsystems unless the primitives' own
signatures later prove a grouping by access granularity (evidence,
not filing).

The two idioms this follows (the library's own, pre-existing):

1. **Lookup maps to functionality keyed by configured behavior** —
   behavior is a settable property on the thing, not a method set on
   the document (AGENTS.md's maps-of-functions rule applied at the
   architecture scale).
2. **Good primitives building up a shape of memory per visit,
   single-pass where possible** — memory-efficient, and local to the
   sections of the tree being changed (the dominant workloads:
   mostly-static retained trees; animations within a surface).

## D1. The back-off doctrine (formalized)

Every extraction may STOP on complication. Complications are signals,
triaged:

- **(a) Pointless misalignment** — naming, ordering, or structural
  differences between instances with identical semantics. Action:
  rename/restructure toward alignment FIRST (its own behavior-
  preserving changeset), then extract. This is where the tangible
  memory/speed wins live.
- **(a′) ALIGNMENT HAS A DIRECTION, and the direction is the
  architect's call** (adjudicated 2026-10-25 after the splice
  recursion miss): when two instances disagree in SHAPE, align toward
  the better memory shape — never average them, never default to
  whichever is convenient. The direction rules, in order:
  (i) explicit bounded state beats implicit unbounded state — an
  iterative visitor with an explicit work buffer beats recursion
  (call-stack depth is unbounded state; the arc's doctrine is "no
  free recursion"; rule 4 is visitor-not-batch);
  (ii) a reused/borrowed buffer beats a per-call allocation;
  (iii) one pass beats two. A transcriber that finds itself aligning
  a visitor DOWN to recursion (or a borrowed buffer UP to an
  allocation) has the direction backwards — STOP and escalate
  instead of proceeding.
- **(b) Meaningful divergence** — an instance does extra work
  (metrics, dirty marks, notifications). Action: the extra moves
  into the primitive's journal/context parameter; the algorithm
  stays common.
- **(c) Genuine divergence** — a real semantic difference. Action:
  the instance stays bespoke with a NAMED note (or takes a mode
  parameter, sparingly). The extraction continues with the aligned
  majority. Every backed-off instance is recorded — the notes are
  the bug/perf-opportunity list.

Measurement: perf pins never regress silently; an extraction that
improves a pinned window gets a documented-cause re-pin (recorded as
good news); renames/restructures land as separate commits from the
extraction they enable.

## D2. The extraction catalog

Ordered by purity. Each entry: the memory shape, today's bespoke
instances (from the survey), the primitive's contract, the
misalignment signals to triage, the win.

### E1 — Tree link machinery and the unified splice

- **Memory shape**: a slot map whose entries carry
  `{parent, first_child, next_sibling}` links (both `RootDom` and
  `RootDomFragment` are this shape).
- **Today**: ~24 free fns in `root_dom.rs` (link get/set ×8,
  `append_child_to_parent`, `validate_append_child`,
  `append_is_noop`, `child_is_partially_linked`,
  `append_creates_cycle`, `detach_*`, `collect_subtree_keys`,
  `mark_node_born_dirty`) PLUS two near-duplicate import walks:
  fragment→fragment (`dom_ops.rs`) and fragment→live
  (`root_dom.rs:1473-1553`, with extra dirty/metric behavior).
- **The primitive**: `tree_links` — append/detach/validate/collect +
  ONE `splice_children` walk taking a **journal** (born-dirty marks,
  metric counts, layer-queue notes) so the live-vs-fragment
  difference is data, not code.
- **Triage signals**: the two import walks' textual differences
  beyond the journal — if structural (not journal-shaped), that's a
  bug smell to surface, not parameterize.
- **Win**: one splice algorithm; fragment becomes "another tree";
  the survey's pre-existing duplication dies as a side effect.

### E2 — The id index

- **Memory shape**: `HashMap<String, DomNodeSlotKey>` mirroring
  nodes-with-id-attrs.
- **Today**: `index_node_id_if_present`, `replace_indexed_id`,
  `assert_id_is_available{,_for}` (two variants), plus mutation
  points scattered in create/set_attr/remove/import.
- **The primitive**: a tiny maintained-index module
  (`insert/remove/rename/assert-free`) called from the link
  machinery's mutation points.
- **Triage signals**: why two assert variants — batch-vs-single, or
  accidental? Align or name.
- **Win**: the mirror invariant is pinned per operation, not by
  whole-document audit.

### E3 — Purge-on-remove (kills a bug class)

- **Memory shape**: dependents keyed by node key — map-shaped
  (scroll offsets, layer registries), option-shaped (selection,
  widget sessions, pseudo-state pieces), set-shaped (transitions'
  active nodes).
- **Today**: ~6 bespoke cleanups (`clear_pseudo_state_for_removed_keys`,
  `clear_hover_for_removed_keys`, `clear_active_for_removed_keys`,
  `clear_focus_selection_for_removed_keys`, plus in-body purge code
  in `remove_node` for scroll/transitions/store).
- **The primitive**: `retain_not_in(map, &removed)` +
  `clear_if_in(option, &removed)` + a **static dependents table**
  `{name, purge_fn}` — `remove_node`'s body collapses to: collect
  the removed set (E1) → run the table → journal the presentation
  teardown.
- **Triage signals**: a dependent that "purges and also notifies"
  (selection firing events?) — split purge from notify, or surface
  the bug. A dependent MISSING from today's cleanups is a latent
  bug — the table makes absence visible.
- **Win**: the forget-to-purge bug class dies; remove_node reads as
  its meaning.

### E4 — The dirty channel

- **Memory shape**: ancestor walk over the link shape, setting
  marks, stopping at the layer root.
- **Today**: `mark_{layout,paint,style}_dirty_for_attribute_change`
  (a trio), `mark_dirty_for_invalidation`,
  `mark_dirty_up_to_layer_root`,
  `mark_newly_attached_subtree_dirty`,
  `attribute_invalidation_from_classification` (the policy table,
  half-existing).
- **The primitive**: one parameterized upward walk
  (`mark_up_to_layer_root(nodes, layer_roots_view, from, marks)`)
  + the classification→marks lookup as the policy owner.
- **Triage signals**: walk-logic differences between the trio
  (detached-node handling, early-stop conditions) — align or name.
- **Win**: one hot walk (every attr change flows here), perf-pinnable;
  the policy is a table.

### E5 — Event propagation

- **Memory shape**: ancestor path from target to root over links.
- **Today**: `dispatch_event_to_target_key*`, `dispatch_*_for_update`,
  `dispatch_and_record_*` variants.
- **The primitive**: `propagate(nodes, target, event, journal)` —
  the "record if bubbled" behavior is journal data.
- **Triage signals**: does recording change propagation or just
  observe it? (Survey says observe — confirm.)
- **Win**: one walk; dispatch methods become table entries over it.

### E6 — The behavior table (the deep extraction)

**Staged (amended 2026-10-25):** E6 lands in three stages, each
independently reviewable:

- **E6-a — the table skeleton + the lifecycle.** The static
  `ControlKind → BehaviorRow` table (enum-indexed, zero-alloc — a
  lookup map, not a HashMap); the generic activation lifecycle
  (press → dispatch → commit/revert) extracted as one state machine
  whose per-kind seams invoke the row's hooks; EVERY control kind
  becomes a row in one pass (the per-row work is mechanical: today's
  per-kind methods become free fns in one
  `element_behaviors/<kind>.rs` file per kind — the folder earns its
  keep). Stage-a rows take `&mut RootDom` (today's context — the
  dispatch-shape proof; the narrowed `BehaviorContext` is E6-c's
  evidence-driven work, since the fields the rows actually need are
  only knowable once the rows exist).
- **E6-b — the popup migration.** The select/datalist popup
  construction dedups into one `build_options_popup` builder (the
  two shells triage-align first); the popup's state machine and
  commit logic become the select/datalist rows' internals; the
  facade's popup methods leave `RootDom`.
- **E6-c — context narrowing + the session slot.** The
  `BehaviorContext` narrows to the disjoint field borrows the rows
  demonstrably need (evidence from E6-a/b); `pressed_pointer_click_target`
  + `active_range_drag_session` unify into ONE
  `Option<PointerSession>` with a kind tag (confirm the mutual
  exclusion empirically first — if both can be live, that's a
  latent bug, report verbatim).

- **Memory shape**: the element's configured behavior
  (`ElementControl`/role — already resolved by
  `resolved_element_control_for_key`) as the KEY; a per-kind row of
  entry-point functions as the VALUE; a narrow `BehaviorContext` as
  the visit's memory shape (nodes+attrs, selection, the session
  slot, the event queue, a styling writer, layer ordering, metrics)
  — replacing today's `&mut RootDom`, which lets each behavior touch
  anything (the OOP leak).
- **Today**: 35 default-behavior methods + 45 popup methods as
  `RootDom` methods, called from the facade's lifecycle seams
  (focus/blur/keyboard/click/text-input/transition-end).
- **The primitive (two levels)**:
  1. **The generic activation lifecycle** — press → dispatch →
     commit/revert (`pressed_pointer_click_target`,
     `apply_after_click_before_dispatch`,
     `revert_after_click_after_canceled`,
     `commit_after_click_after_dispatch`) is control-AGNOSTIC: one
     shared state machine.
  2. **The per-kind hooks table** — `control_kind →
     {on_keyboard, on_pointer_down, on_click_commit, on_focus,
      on_blur, on_text_input, on_transition_end}` — the lifecycle
     invokes the row's hooks at its seams. Button's space/enter,
     range's arrow-step, select's popup, datalist's popup: all rows.
- **The popup decomposition**: the 45 methods are really ~3
  concerns — popup DOM construction (shell/scroll/rows with
  role/state attrs + inline styles + layer ordering), a
  highlight/open-close state machine, and commit logic. Select and
  datalist popup construction are near-identical (triage: align
  first, then one `build_options_popup` builder primitive serves
  both); the state machine is one parameter-over-owner-kind; commit
  is the row's hook.
- **The session slot**: `pressed_pointer_click_target` AND
  `active_range_drag_session` are the same shape — an in-progress
  pointer engagement. Unify to ONE `Option<PointerSession>` with a
  kind tag (a pointer press is one engagement — they cannot be
  simultaneously active). Win: one slot instead of two, and "no two
  active sessions" by construction. Triage: confirm the mutual
  exclusion empirically before unifying; if both CAN be live, name
  the divergence (likely a bug).
- **Triage signals**: behaviors with "unique" context needs (what
  beyond the standard shape?); the popup's inverted calls back into
  tree mutation — under the builder primitive these become ordinary
  tree ops through the context.
- **Win**: behaviors become data; adding a control kind is adding a
  row; the facade's lifecycle seams shrink to
  resolve-kind → lookup → invoke; the widget layer
  (`element_behaviors/`) absorbs the popup as rows, not as an impl
  block.

### E7 — Transition store and tick

- **Memory shape**: store keyed by node + clock + active set.
- **Today**: `set_transition`, `sync_transitions`,
  `sync_transitions_at_current_time`, `schedule_pass_transitions`,
  `transition_elapsed_s_at`, `validate_transition_payload`, and the
  inverse-projection fns (`decl_value_for_groups` + 8 helpers).
- **The primitive**: `tick(store, now) -> journal(events, marks)` +
  `schedule(store, key, payload)`; the inverse-projection fns are
  already free — they become the shared "read computed groups"
  primitive with style.
- **Triage signals**: do the three entry shapes apply validation at
  the same points?
- **Win**: one scheduler; the tick is one walk over the active set
  (the animation-locality workload).

### E8 — Small generics (candidates for `collections`)

- `retain_not_in`, `clear_if_in` (E3's leaves), the hovered-layers
  swap-buffer (take/store ×2). Extract only where genuinely generic
  (per the `collections` ownership boundary); DOM-shaped pieces stay
  in `types/`.

## D3. What the facade becomes

After E1–E7, `RootDom`'s methods are thin compositions over the
primitives: `update` = tick transitions + sync style + dispatch
pointer + drain events (orchestration, visibly — not a hidden
pipeline); `remove_node` = collect + purge-table + journal;
`set_attr` = validate + write + classification-table → channel walk.
The 48-`pub fn` contract stands. The fields remain the memory
layout — flat. If the primitives' signatures end up borrowing the
same field triple at every pass, THAT is the only evidence that
could justify a grouping, and it is evaluated then, not now.

## D4. The stage contract (per extraction)

1. **Prep changeset** (if triage found (a)): rename/restructure
   toward alignment — behavior-preserving, full gate, its own
   commit(s).
2. **Extraction changeset**: the primitive + its tests (each
   primitive file has real unit tests per structural rule 2) + the
   bespoke instances re-pointed. Full gate; existing pins green;
   perf re-pins only with documented cause.
3. **The record**: PLAN.md entry (what was extracted, what was
   triaged, what backed off and why).

## D5. Explicitly not this PR

No newtype subsystems; no module sealing; no `update()` pipeline
redesign; no field regrouping by topic; no algorithm changes beyond
alignment-driven unification (each is its own reviewed changeset
with the perf gate); no consumer-visible change (the 48-fn
contract).

## D6. The win ledger (expected, to be verified per stage)

| Extraction | Tangible win |
|---|---|
| E1 splice unification | −1 duplicated walk; fragment = "another tree" |
| E3 purge table | the forget-to-purge bug class dies |
| E4 channel + table | one hot walk; policy as data |
| E6 behavior table | −80 methods → rows; behaviors as data; the popup's inverted dependency dissolves into ordinary tree ops |
| E6 session slot | −1 field; "one active engagement" by construction |
| E6 popup builder | select/datalist construction dedup |
| E7 tick | one active-set walk for the animation workload |

## D7. The opportunity ledger (grows per stage; the doctrine's yield)

| Found in | Finding | Disposition |
|---|---|---|
| E2 | `index_node_id_if_present` is coupled to the metric-instrumented `RootDomNodes::get` | deferred insert primitive (pre-resolved id signature) |
| E3 | `DomSelection::DocumentSelected` is never purged (unreachable today — never constructed) | editing epic, with the dead caret entries |
| E3 follow-up | scroll-offset purge changed access class in migration | FIXED (`remove_each_in`, `ff152fe`); the audit is a doc comment on the table |
| E4 | the metric proxy blocks SlotMap-coerced extraction | the `NodeStore` trait (E1-phase-2) is the principled resolution |
| E1-phase-2 | the unified splice first aligned DOWN to recursion | FIXED (`0175e16` — iterative work stack, no HashMap); the D1(a′) direction rule was born here |
| E1-phase-2 | `append_child_to_parent`'s per-append cycle check + `current_layer_root_key` walk make deep builds/imports O(depth²) (measured: 10k-depth splice test, 10.95s, dominated by this pre-existing cost) | **open opportunity** — the cycle check is only needed when appending an already-linked node (fresh births can't cycle); a future optimization, needs its own design pass (any fix must keep the layer-root walk's semantics and the metric counts) |
| E5 | the capture phase is O(depth²) — `ancestor_count_to_root` plus a per-ancestor `ancestor_key_at_distance` re-walk the ancestor path | **open opportunity** — preserved byte-identically (an algorithm/perf change needs its own reviewed commit per rule 4); a reused path buffer or a collect-then-reverse walk would be O(depth), adjudicated against D1(a′) (reused buffer vs the walk's current zero-allocation) |
| E7 | the transition tick's per-node effects do not factor into a journal | **backed off (c)** — inline `&mut RootDom` mutations (dirty walks, layer queues, event dispatch with the popup transition-end hook) with semantic marks-vs-events ordering; extraction would be a batch (rule 4) or a borrow-conflicting closure over `&mut self` |
