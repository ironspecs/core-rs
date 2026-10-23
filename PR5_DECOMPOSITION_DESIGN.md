# PR5_DECOMPOSITION_DESIGN.md — RootDom decomposition, re-scoped from the structural survey (complete design)

Status: `COMPLETE DESIGN — presented for owner decision before transcription`. Date: 2026-10-23. Author: the architect. Authority: PLAN.md "PR 5 — RootDom decomposition" (lines 709–743: the settled direction, the compatibility contract, the migration policy, the honest cost, and the recorded scope re-evaluation owed at start); the structural survey of 2026-10-23 (the full field/method/caller census of `root_dom.rs`, 7,464 lines).

**This document amends the settled direction's letter.** The plan's
three-bucket sketch (`tree` / `styling` / `transitions`) describes
~8 of the file's 24 fields. The survey's census shows the real
cohesion structure and the migration policy's unscheduled
preconditions. The settled direction's SPIRIT — narrow newtypes, the
existing facade, the 48 `pub fn` as thin delegations, one file per
subsystem, structural rules 1/5 — is kept exactly. The plan's own
recorded authority for this amendment: "its scope should be
re-evaluated when it starts."

## D0. The re-scope in one paragraph

`RootDom` decomposes into **four** newtypes plus facade-kept
orchestration state — `RootDomTree` (root, nodes, id_lookup,
pseudo_state), `RootDomStyling` (inline decl store, computed texts,
style revision), `RootDomTransitions` (store, clock), and
`RootDomPresentation` (the five layer-state fields, scroll, layout
scratch, bevy attach) — while the facade KEEPS the state that is its
own orchestration surface (context, metrics — pinned to `RootDom` by
`root_dom_metrics.rs`, the event queue + dispatch, selection,
pointer state, widget sessions until P5-5). The work is staged
cleanest-first with an explicit precondition stage (P5-0: route
direct field access through the newtype seams), and the popup/widget
extraction (P5-5) lands last against the settled facade shape. Every
stage is independently valuable, independently gated, and
independently stoppable — the honest-cost note stands: no
user-visible win, and any stage that cannot land byte-behavior-
identical STOPS rather than improvising.

## D1. The field census, assigned (the survey's table, adjudicated)

**`RootDomTree`** (4 fields): `root`, `nodes`, `id_lookup`,
`pseudo_state` (hovered_layers ×2, active, focused — the "hit-test
state" the plan named).
**`RootDomStyling`** (3 fields): `inline_decl_store`,
`computed_texts`, `style_runtime_revision`.
**`RootDomTransitions`** (2 fields): `transitions`, `clock_s`.
**`RootDomPresentation`** (9 fields): `pending_layer_append_roots`,
`pending_layer_remove_roots`, `raised_layer_roots`,
`layer_entity_by_root`, `layer_lease_by_root`,
`vertical_scroll_offset_by_node`, `attached_bevy_parent`,
`layout_scratch`, plus the layer/scroll/bevy-attach methods
(`layer_entity` — the hottest `pub(crate)` method at 48 call sites —
the register/unregister/drain cluster, the scroll trio).
**Facade-kept** (6 fields): `context`, `metrics` (pinned),
`emitted_events`, `selection`, `previous_pointer_state`,
`pressed_pointer_click_target` + `active_range_drag_session` (widget
sessions — P5-5 re-homes them with the widget layer).

Rationale for the fourth bucket and the facade-kept set: the
layer/presentation cluster has independent cohesion (consumed by
composite, layer_hit, paint — all downward) and its own invariants
(the lease/registry/queue trio); the event queue and dispatch are
the facade's orchestration role (update() drains them; listeners
receive the facade); metrics is pinned to `RootDom` by its own
file's invariant. Forcing presentation into `tree` would re-create
the coupling the split exists to remove (the dirty channel walks
nodes UP TO layer roots — two clusters, not one).

## D2. The stages (each independently gated, cleanest-first)

| Stage | Scope | Gate |
|---|---|---|
| **P5-0 — the accessor precondition** | Route the ~130+ direct `root_dom.root` field reads, the ~80 `layout_scratch` accesses, and the `inline_decl_store`/`computed_texts` reference-takings through `pub(crate)` accessors or `Deref`-newtype fields (the `RootDomNodes` Deref precedent). Behavior-identical; mechanical; the unscheduled precondition the survey found (finding #5). | Full suite green UNCHANGED; zero behavior delta |
| **P5-1 — `RootDomStyling`** | The 3 fields + the 12 inline-store `pub(crate)`s + `style_mut` + `canonical_style_snapshot` + `compute_style_map` + `route_style_attribute` + the 2 dead candidates (`inline_texts_mut`, `resolve_computed_text` — dead-code removal is a named, allowed behavior-adjacent cleanup, pinned by the lint). Cross-cuts (the upsert/replace/remove trio's dirty + metrics) via the split-borrow template (`remove_collected_subtree_keys`'s four-disjoint-refs style). | Full gate; styling tests re-home with their methods |
| **P5-2 — `RootDomTransitions`** | The 2 fields + `set_transition`/`sync_transitions`/`schedule_pass_transitions` + the inverse-projection free fns + `transition_elapsed_s_at`. Cross-cuts (sync's dirty + layer queues + events + popup hook) via the template. | Full gate; the 3 transition tests re-home |
| **P5-3 — `RootDomPresentation`** | The 9 fields + the layer/scroll/attach method cluster. composite/layer_hit/paint call sites re-point (they are P5-0-prepped). The tree's dirty channel now reads presentation's layer-root state through the split-borrow discipline. | Full gate; the layer tests (6 external-layer + 3 layer_offset) re-home |
| **P5-4 — `RootDomTree`** | The 4 fields + impl #1/#2's tree methods + the ~24 tree free fns + fragment import (fragments are tree-family). The two widest mutations (`remove_node` 9-group, `set_attr` 8-group) decompose via the template. The popup's transition-end hook (`queue_presented_options_popup_shell_for_layer_removal_after_transition_end`, impl #2) moves to the widget surface in P5-5 — P5-4 leaves it facade-parked. | Full gate; tree-categorized tests re-home |
| **P5-5 — the widget extraction** (the ownership correction) | impl #5's 45 popup methods + impl #4's 35 default-behavior methods + the 2 widget-session fields move to the widget layer (`element_behaviors/`), called from the facade's lifecycle seams through a narrow context object (D3). The paint popup-shell special-case becomes a widget-owned query. The 7 focus-invariant tests + 2 end-to-end tests re-home; the private-field pins (`root.selection`, `root.pending_layer_remove_roots` writes) re-express through public surface per AGENTS.md. | Full gate; the paint special-case asserts through the widget query |

**Stop rules (every stage):** any method that cannot move without a
behavior change stays facade-parked with a named note; any stage
whose diff stops reading as cut/paste-plus-imports STOPS and returns
to the architect; the migration policy is amended per D4.

## D3. The popup/widget extraction shape (P5-5's mutual dependency)

Today the coupling dissolves inside one struct: the facade's
lifecycle seams (focus/blur/keyboard/click/text-input/
transition-event) call the widget methods; the widget methods call
back into the facade (tree mutation, `style_mut`, layer ordering).
The extraction shape:

```rust
// types/widget_behavior_context.rs (new)
pub struct WidgetBehaviorContext<'a> {
    pub tree: &'a mut RootDomTree,
    pub styling: &'a mut RootDomStyling,
    pub presentation: &'a mut RootDomPresentation,
    pub interaction: &'a mut RootDomInteractionState, // selection + widget sessions
}
```

The widget layer's functions take the context (the AGENTS.md
context-object doctrine); the facade's seams construct it and call.
The dependency direction is acyclic: widget → newtypes (downward),
facade → widget (upward through the context). The paint domain's
popup-shell special-case (`paint/mod.rs:258,377`) calls a
widget-owned query (`element_behaviors` already hosts that seam).

## D4. The migration-policy amendment (recorded, per the survey's finding #3)

The plan's "byte-identical moves" policy is amended PRECISELY:
(a) P5-0's accessor routing is behavior-identical but NOT
byte-identical at the ~200+ call sites it touches — it lands as its
own reviewed changeset with the full suite green as the gate, named
as the precondition; (b) within each P5-n move, the moved items
themselves relocate byte-identically — only their SELF-REFERENCES
(`self.field` → the newtype path) and call-site paths change;
(c) the split-borrow conversions (free fns taking `&mut RootDom` →
disjoint `&mut` newtype refs, per the
`remove_collected_subtree_keys` template) are mechanical signature
changes at ~30 call sites, reviewed as part of their stage, never
bundled with algorithm changes. Anything that cannot satisfy (b) is
facade-parked with a named note (D2's stop rules).

## D5. What this PR does NOT do (named)

No user-visible behavior change of any kind. No algorithm changes.
The fragment-import duplication (fragment→fragment via `dom_ops.rs`
vs fragment→live re-implemented walk) is a pre-existing observation,
not this PR's scope. `update()`'s pipeline shape is NOT redesigned —
it stays a facade orchestration method that reaches into the
newtypes (its 7-subsystem nature is the facade's JOB; the split
gives it narrower targets, not a new pipeline). Per-subsystem
metrics tallies are rejected (the survey's §6: the closed 44-name
vocabulary stays one tally on the facade).

## D6. The honest cost, re-stated for the decision

No user-visible win; a large mechanical diff; the return is that the
crate's core file stops absorbing everything (7,464 lines, +1,000
this arc, 219 methods — a shape the crate's own structural rules
would reject in any new file). Each stage is independently
stoppable: if P5-0/P5-1 lands and P5-2+ is judged not worth the
risk at that point, the crate keeps the win of the landed stages.
The alternatives on the table: (a) this staged plan; (b) descope to
P5-0+P5-1 only (the precondition + the cleanest subsystem, ~20% of
the diff for the naming win); (c) defer the PR entirely and let
root_dom.rs stand (the plan's words: the challenge is welcomed).

## D7. The pin and test plan

Every stage: full gate (`fmt`, `clippy -D warnings`, `test
--workspace`, `xtask lint`) green per commit; perf pins move only
with documented cause (they should not move at all — no algorithm
touches). The 34 in-file `root_dom.rs` tests re-home by the survey's
categorization (styling-serde ×6 → P5-1; transitions ×3 → P5-2;
layer ×9 → P5-3; popup ×9 → P5-5; the rest follow their methods).
The three private-field/private-method pins (`root.selection`,
`root.pending_layer_remove_roots`, `dispatch_and_record_transition_event`
direct calls) re-express through public surface — that re-expression
is a named behavior-adjacent change within P5-5, not smuggled.
The facade contract (48 `pub fn`) is asserted UNCHANGED at each
stage by a generated signature pin (a compile-time list of the 48
names in a doc test or a `tests/` surface check — the compatibility
contract made mechanical).
