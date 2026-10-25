# element_behaviors

The behavior table for `ui-dom`: default control behavior expressed as data —
a static `ControlKind → BehaviorRow` lookup map — plus the generic activation
lifecycle that invokes each kind's hooks at its seams.

## Purpose and scope

This module is the deep extraction (PR 5, E6). It turns the former
per-kind `RootDom` methods into one enum-indexed table (`behavior_table.rs`)
whose rows are entry-point functions, one file per control kind. The facade
seams on `RootDom` (focus/blur/pointer/keyboard) resolve a control kind, look
up the row, and invoke the row's hook. It does not own the document memory
(`RootDom` still does) or the runtime domains (`domains/`).

## Ownership boundaries

- `control_kind.rs` — `ControlKind`, the single behavior KEY, and
  `ControlKind::ALL`, its exhaustive census.
- `behavior_table.rs` — the static `BEHAVIOR_TABLE` array, the `behavior_row`
  enum-indexed lookup, and `resolve_kind` (tag → kind). `ControlKind` is the
  single dispatch key; `resolve_kind` maps a tag (and, for inputs, the
  resolved `ResolvedElementControl`) to a kind.
- `activation.rs` — the generic click activation lifecycle
  (press → dispatch → commit/revert) plus the retained pointer-session state
  (`PressedPointerClickTarget`, `ActiveRangeDragSession`).
- `button.rs`, `checkbox.rs`, `range.rs`, `select.rs`, `text_entry.rs` — the
  per-kind row hooks.
- `controls.rs` — the helpers shared by the four input-control kinds
  (checkbox/number/range/text input), named rather than duplicated.
- `present_options.rs` — presented-options definitions: owner kinds, popup
  role/state constants, records, measurement and record-reading helpers.
- `popup.rs` / `popup_shell.rs` — the presented-options popup lifecycle and
  DOM construction (E6-b: the former `RootDom` popup methods re-homed as free
  functions).
- `paint.rs`, `intrinsic_size.rs`, `dom_event_dispatch_outcome.rs` — the
  element paint/intrinsic/dispatch leaves that sit beside the behavior rows.

## Architectural approach

Row hooks take `&mut RootDom` exactly as the former methods did — the stage-a
context. That shape is deliberate and temporary: it proves the dispatch shape
first; the narrowed `BehaviorContext` (a visit-memory struct of the disjoint
fields the rows demonstrably need) is E6-c's evidence-driven work, since those
fields are only knowable once the rows exist. The two pointer-engagement slots
(`pressed_pointer_click_target` and `active_range_drag_session`) stay separate
— E6-b's empirical trace found a range engagement keeps both live at once (the
press record dispatches the click on pointer-up; the drag session drives
pointer-moves), so the session-slot unification was REJECTED on evidence. E6-c
is the context narrowing only.

## Invariants, rules, and decisions

- The table is complete by construction: every `ControlKind` arm maps to a row
  (the census test iterates `ControlKind::ALL`; the exhaustive `match` in
  `behavior_row` is the compile-time guard).
- A `None` hook field is the honest "no default behavior at this seam"; the
  lifecycle treats it as today's no-op path.
- The row field set matches the discovered seams: keyboard, pointer-down,
  click-commit, focus, blur. There is no `on_text_input` or `on_transition_end`
  row — text input runs inside the keyboard handler, and transition-end does
  not dispatch per kind.
- Adding a control kind is adding a row (and one entry to `ControlKind::ALL`);
  no facade seam changes shape.
