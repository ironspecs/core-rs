# PLAN.md — next task (TBD)

Fresh plan document. It will be filled when the next task is defined.

## Where things stand (2026-10-18)

- **The declaration-store style system is landed at v0.6.0** (merged, PR
  #15): typed declarations in arenas, seven-group computed projection on the
  node, winners fold, group-diff dirty classification, pass-scheduled
  transitions, the D5 enforcement (`style_mut` + compile break +
  `PresentationRequiresStyleMut`). API record: `MIGRATION.md`.
- **The coverage accounting (Phase 0 + Phase 1) is complete** (this branch):
  the `layout_proofs` suite is reorganized into subsystem directories
  (`style/`, `layout/`, `text/`, `widgets/`, `pipeline/`, plus
  `robustness.rs`); **364 proofs** pin every deliberately set-aside spec
  area — unsupported value forms, shorthands, properties, selectors,
  at-rules, cascade tiers, admitted-but-unconsumed gaps, preserved defaults,
  and a robustness safety net with zero panics. Phase 1 landed four
  production fixes beyond pins: the B1 sign-rule family (negative
  non-negative lengths rejected on longhand and shorthand paths;
  `font-size: unset` admitted), the F1 load-atomicity fix (failed
  stylesheet loads are all-or-nothing), and two queue-jumped
  crash-hardening fixes (`font-size: 0`, the multi-line caret path).
- **The divergent registry is the honest current map**: 8 running
  `off_spec_*` proofs (negative margins, opacity clamping, font-family/
  weight/style unconsumed, transform transitions, range pill default,
  `line-height: normal` → 1.0×, `break-spaces` ≈ `pre-wrap`) plus deliberate
  defaults (near-white initial color, line-height floor, border-0 initial,
  16:9 placeholder) and grouped `unsupported_*` gates. See
  `crates/ui-dom/tests/layout_proofs/README.md`.
- Both prior plans are in git history: the style-system redesign plan
  (landed v0.6.0) and the coverage accounting plan (landed on this branch).

## Next task

_To be defined. If it is the Phase 2 march on-spec: the pins are the
witnesses — admission follows the D6 policy in `MIGRATION.md` (named
consumer need + spec citation + Blink/Gecko reference + proof in the same
change), and each admission flips its pin on-spec in the same change. The
named leader is font-family/weight/style consumption (the step-5 follower)._
