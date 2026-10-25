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

Row hooks take `&mut RootDom` exactly as the former methods did. That shape is
no longer temporary: E6-c's evidence-gated narrowing was REJECTED (2026-10-25)
— the usage inventory showed the rows are document-wide, not a disjoint-field
borrow. The popup (select + the datalist text-input) needs tree mutation,
styling, layer ordering, the id index, and box measurement, and the shared
input-control/text-entry paths wire the popup lifecycle into the
checkbox/number/range/text-input rows' focus/blur and keyboard seams. Only a
minority of hooks (button keyboard, checkbox commit, pointer-down, range drag)
are cleanly narrow, and they share files with the wide paths while the table's
per-seam hook types stay uniform. Narrowing would have to carry the whole
document or split shared function bodies — neither is a disjoint-field-borrow
win — so the rows keep `&mut RootDom`. The two pointer-engagement slots
(`pressed_pointer_click_target` and `active_range_drag_session`) stay separate
(E6-b rejected the session-slot unification on evidence; a range engagement is
press-PLUS-drag). See `PR5_DECOMPOSITION_DESIGN.md`'s E6 entry for the usage
inventory table.

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
