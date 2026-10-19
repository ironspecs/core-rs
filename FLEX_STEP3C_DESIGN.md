# FLEX_STEP3C_DESIGN.md — the blockification row, re-adjudicated under the run model (complete design)

Status: `COMPLETE DESIGN — verification-first; transcription is
mechanical`. Date: 2026-10-19. Author: the architect. Authority: CSS
Flexbox 1 §4 (anonymous items), CSS Display 3 §2.7 (blockification);
the engine corpus (`.tmp/engine-corpus/`: Blink `layout_block.cc:147-171`;
Gecko `nsCSSFrameConstructor.cpp:8134`); the T1 finding
(`7946aa7`: `display: inline` flex items kept their line-height cross
size instead of stretching — `off_spec_flex_item_inline_level_display_blockification`).

## D0. The model, stated exactly

Per both engines, a lone `display: inline` element in a flex container
is NOT a flex item and is NOT blockified: it joins a length-1 anonymous
run, wrapped in an anonymous BLOCK box (Blink: anonymous `LayoutBlock`;
Gecko: `-moz-anonymous-item` `nsBlockFrame`). The anonymous block
participates as the item — it STRETCHES to the line cross size when
`align-items`/`align-self` is stretch and the cross size is auto. The
inline content inside it renders at its own inline height, positioned
inside the stretched block per the block's line box. CSS Display 3
§2.7's blockification (`inline-block` → `block`, `inline-flex` →
`flex`, `inline` → `block`) applies to boxes that become ITEMS;
inline-level children become run members instead, so `inline → block`
has no application.

**Consequence:** the T1 pin's spec-correct expectation ("blockified →
the item stretches to 20px") was written against a blockification
model the engines do not use. The engine-correct observable is: the
anonymous wrapper stretches (its cross = line cross), and the span/
text renders at its inline height (14px) positioned at the line's
cross-start. The row re-adjudicates to THAT.

## D1. Verification BEFORE any change (transcription step 1)

Determine the current behavior post-3a and report it:
1. Does a lone `display: inline` child form a length-1 run today?
   (It must, per `FLEX_STEP3A_DESIGN.md` D1.)
2. Does the run item STRETCH — i.e., its resolved cross == the line
   cross size when `align-items` is stretch and the cross is auto?
   Read the stretch path (`stretched_flex_item_content_cross_size_px`
   and the aligned-resolved-item computation): does it apply to
   `AnonymousInlineGroup` sources, and does the run's placement lay out
   children inside the STRETCHED content area (children at cross-start,
   their own inline heights)?
3. Does `display: inline-block` still block-measure and stretch (the
   T1 row (b) outcome)?

If (2) stretches and positions children at cross-start inside the
stretched area: 3c is PIN-ONLY (re-adjudication). If (2) fails, the
specified fix below applies.

## D2. The conditional fix (only if D1.2 fails)

If the stretch path does not apply to run items (or the children do
not lay out at cross-start inside the stretched content area): extend
the existing stretch machinery to `AnonymousInlineGroup` sources —
stretch applies when the item's cross is auto (a run's is), the
stretched content cross = line cross minus zero border/padding, and the
run's child context lays out children at cross-start inside it
(their own inline heights, matching the node-item stretch behavior for
their subtrees). NO blockification map is added;
`blockified_flex_item_style` stays `InlineFlex → Flex` (update its
comment: `inline-block` is already block-routed; `inline` elements are
run members, never items).

## D3. The pin table (the re-adjudicated family)

Family `flex_item_inline_level_display_participation` (renamed from
`off_spec_flex_item_inline_level_display_blockification`; old name in a
relabel comment; registry row moves on-spec):

| Row | New asserted behavior |
|---|---|
| (a) lone `display: inline` child, definite 200×20 row | the child forms a length-1 anonymous run; the run item's resolved cross == 20 (the STRETCH, observable via the item's placed cross); the rendered text rect height == its inline height (≈14) at the line's cross-start — the engine model, not blockification |
| (b) `display: inline-block` child, same | UNCHANGED on-spec (block-measures, stretches to 20; T1's original outcome) |
| (c) `display: inline` under `align-items: stretch` vs `display: block` control | both stretch; the inline child's rendered content keeps its inline height inside the stretched wrapper; the block control's content box == 50 |
| (d) `display: block` control | UNCHANGED on-spec reference |

**Audit rule:** any other proof moving under this landing follows the
standing rule (spec-correct → update with documented cause and report;
spec-wrong → STOP).

## D4. Explicitly untouched

The run fold, the lanes, the solve, paint derivation, abs-pos model,
`blockified_flex_item_style`'s mapping (comment-only update allowed).

## D5. File map

| File | Change |
|---|---|
| `crates/ui-dom/src/domains/layout/flex.rs` | D2's conditional fix ONLY if D1.2 fails; otherwise comment-only (the `blockified_flex_item_style` note) |
| `crates/ui-dom/tests/layout_proofs/layout/flex_layout.rs` | the D3 family |
| `crates/ui-dom/tests/layout_proofs/README.md` | the registry move |
| `crates/ui-dom/tests/perf_metric.rs` | CONDITIONAL re-pin only, documented cause |

## D6. The transcription contract

Execute D1 FIRST and report its three answers with evidence before
changing anything. Then: pin-only if D1.2 stretches; the D2 fix if it
fails. STOP conditions: (1) any case the document does not answer;
(2) any proof moves spec-wrong; (3) a perf pin moves without a
documented cause. ONE commit, full gate green.
